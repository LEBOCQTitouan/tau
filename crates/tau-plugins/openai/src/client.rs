//! OpenAI HTTP client with retry + required Bearer auth.
//!
//! `post_chat_completions` is the only outbound entrypoint. Owns a
//! `reqwest::Client` with TLS + timeout configured by the plugin's
//! `Configure::from_config`.
//!
//! Retry policy per spec §4.1:
//! - 2xx → return immediately.
//! - 429, 503 → retry with exponential backoff; honor `Retry-After`
//!   when `retry.respect_retry_after` is true. Exhausted →
//!   `ClientError::Exhausted`.
//! - 4xx other than 429 → return immediately (caller maps to LlmError).
//! - 5xx other than 503 → retry (treated as transient).
//! - Network timeout → retry (synthesized as 408).
//! - Other transport error (DNS, TLS, connection refused) →
//!   `ClientError::Transport`, no retry.

use reqwest::{Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use std::time::Duration;
use tokio::time::sleep;

use crate::config::RetryConfig;
use crate::error::ClientError;

/// HTTP client for OpenAI's `/v1/chat/completions` endpoint.
pub(crate) struct OpenAIClient {
    inner: reqwest::Client,
    base_url: String,
    api_key: SecretString,
    organization: Option<String>,
    retry: RetryConfig,
}

impl OpenAIClient {
    /// Construct a client. The caller (Task 11 `Configure::from_config`)
    /// validates inputs.
    pub(crate) fn new(
        inner: reqwest::Client,
        base_url: String,
        api_key: SecretString,
        organization: Option<String>,
        retry: RetryConfig,
    ) -> Self {
        Self {
            inner,
            base_url,
            api_key,
            organization,
            retry,
        }
    }

    /// `POST /v1/chat/completions` with retry. The body is a
    /// `serde_json::Value` produced by `request::build_chat_completions_body`.
    /// `stream == true` adds the `accept: text/event-stream` header.
    pub(crate) async fn post_chat_completions(
        &self,
        body: &serde_json::Value,
        stream: bool,
    ) -> Result<Response, ClientError> {
        let url = format!(
            "{}/v1/chat/completions",
            self.base_url.trim_end_matches('/'),
        );
        let mut attempt: u32 = 0;

        loop {
            attempt += 1;

            let mut req = self
                .inner
                .post(&url)
                .header(
                    "authorization",
                    format!("Bearer {}", self.api_key.expose_secret()),
                )
                .header("content-type", "application/json")
                .json(body);
            if stream {
                req = req.header("accept", "text/event-stream");
            }
            if let Some(org) = &self.organization {
                req = req.header("openai-organization", org);
            }

            let send_result = req.send().await;
            match self.classify(send_result, attempt) {
                Decision::Return(resp) => return Ok(resp),
                Decision::Error(err) => return Err(err),
                Decision::Retry { delay_ms, status } => {
                    if attempt >= self.retry.max_attempts {
                        return Err(ClientError::Exhausted {
                            status,
                            attempts: attempt,
                        });
                    }
                    tracing::warn!(
                        target: "openai_plugin::retry",
                        attempt,
                        max = self.retry.max_attempts,
                        delay_ms,
                        status = status.as_u16(),
                        "retrying transient error",
                    );
                    sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }

    fn classify(&self, res: reqwest::Result<Response>, attempt: u32) -> Decision {
        match res {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    Decision::Return(resp)
                } else if is_retryable_status(status) {
                    let delay_ms = self.retry_delay(&resp, attempt);
                    Decision::Retry { delay_ms, status }
                } else {
                    Decision::Return(resp)
                }
            }
            Err(e) if e.is_timeout() => {
                let delay_ms = self.backoff_only(attempt);
                Decision::Retry {
                    delay_ms,
                    status: StatusCode::REQUEST_TIMEOUT,
                }
            }
            Err(e) => Decision::Error(ClientError::Transport(e)),
        }
    }

    fn retry_delay(&self, resp: &Response, attempt: u32) -> u64 {
        if self.retry.respect_retry_after {
            if let Some(secs) = resp
                .headers()
                .get("retry-after")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
            {
                return secs * 1000;
            }
        }
        self.backoff_only(attempt)
    }

    fn backoff_only(&self, attempt: u32) -> u64 {
        let shift = (attempt - 1).min(6);
        let delay = self.retry.base_delay_ms.saturating_mul(1u64 << shift);
        delay.min(60_000)
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 503) || (status.as_u16() >= 500 && status.as_u16() != 501)
}

enum Decision {
    Return(Response),
    Retry { delay_ms: u64, status: StatusCode },
    Error(ClientError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Spawn a tiny HTTP/1.1 server on 127.0.0.1:0 that replies with
    /// pre-canned responses for the first N connections.
    /// Returns the listener URL + a counter of accepted connections +
    /// captured request bytes (for header assertions).
    async fn spawn_canned_server(
        responses: Vec<&'static str>,
    ) -> (
        String,
        Arc<AtomicUsize>,
        Arc<tokio::sync::Mutex<Vec<Vec<u8>>>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let url = format!("http://127.0.0.1:{port}");
        let counter = Arc::new(AtomicUsize::new(0));
        let received = Arc::new(tokio::sync::Mutex::new(Vec::<Vec<u8>>::new()));
        let counter_clone = counter.clone();
        let received_clone = received.clone();

        tokio::spawn(async move {
            for canned in responses {
                let (mut sock, _) = listener.accept().await.unwrap();
                counter_clone.fetch_add(1, Ordering::Relaxed);
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                received_clone.lock().await.push(buf[..n].to_vec());
                sock.write_all(canned.as_bytes()).await.unwrap();
                sock.flush().await.unwrap();
                drop(sock);
            }
        });

        (url, counter, received)
    }

    fn ok_body() -> &'static str {
        "HTTP/1.1 200 OK\r\n\
         content-type: application/json\r\n\
         content-length: 2\r\n\
         connection: close\r\n\
         \r\n\
         {}"
    }

    fn rate_limited_body(retry_after: Option<&str>) -> String {
        let mut s = String::from("HTTP/1.1 429 Too Many Requests\r\n");
        if let Some(ra) = retry_after {
            s.push_str(&format!("retry-after: {ra}\r\n"));
        }
        s.push_str(
            "content-type: application/json\r\n\
             content-length: 0\r\n\
             connection: close\r\n\
             \r\n",
        );
        s
    }

    fn build_client(
        base_url: String,
        organization: Option<String>,
        max_attempts: u32,
        base_delay_ms: u64,
    ) -> OpenAIClient {
        OpenAIClient::new(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            base_url,
            SecretString::new("sk-test-1234".into()),
            organization,
            RetryConfig {
                max_attempts,
                base_delay_ms,
                respect_retry_after: true,
            },
        )
    }

    #[tokio::test]
    async fn post_chat_completions_happy_path_sends_authorization_header() {
        let (url, counter, received) = spawn_canned_server(vec![ok_body()]).await;
        let client = build_client(url, None, 3, 0);
        let resp = client
            .post_chat_completions(&serde_json::json!({"model": "m", "messages": []}), false)
            .await
            .unwrap();
        assert!(resp.status().is_success());
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        let raw = received.lock().await;
        let req_str = std::str::from_utf8(&raw[0]).unwrap_or("");
        assert!(
            req_str
                .to_ascii_lowercase()
                .contains("authorization: bearer sk-test-1234"),
            "expected Authorization: Bearer header; got: {req_str}",
        );
        // Organization header NOT sent when None.
        assert!(
            !req_str
                .to_ascii_lowercase()
                .contains("openai-organization:"),
            "no org header expected; got: {req_str}",
        );
    }

    #[tokio::test]
    async fn post_chat_completions_with_organization_sends_org_header() {
        let (url, counter, received) = spawn_canned_server(vec![ok_body()]).await;
        let client = build_client(url, Some("org-test".into()), 3, 0);
        let _ = client
            .post_chat_completions(&serde_json::json!({"model": "m", "messages": []}), false)
            .await
            .unwrap();
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        let raw = received.lock().await;
        let req_str = std::str::from_utf8(&raw[0]).unwrap_or("");
        assert!(
            req_str
                .to_ascii_lowercase()
                .contains("openai-organization: org-test"),
            "expected openai-organization header; got: {req_str}",
        );
    }

    #[tokio::test]
    async fn post_chat_completions_429_then_200_succeeds_after_retry() {
        let canned = vec![
            Box::leak(rate_limited_body(Some("0")).into_boxed_str()) as &'static str,
            ok_body(),
        ];
        let (url, counter, _) = spawn_canned_server(canned).await;
        let client = build_client(url, None, 3, 0);
        let resp = client
            .post_chat_completions(&serde_json::json!({"model": "m", "messages": []}), false)
            .await
            .unwrap();
        assert!(resp.status().is_success());
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn post_chat_completions_429_with_retry_after_honors_header() {
        // 1× 429 + retry-after: 0 + 200; verify 2 attempts and success.
        let canned = vec![
            Box::leak(rate_limited_body(Some("0")).into_boxed_str()) as &'static str,
            ok_body(),
        ];
        let (url, counter, _) = spawn_canned_server(canned).await;
        let client = build_client(url, None, 3, 0);
        let resp = client
            .post_chat_completions(&serde_json::json!({"model": "m", "messages": []}), false)
            .await
            .unwrap();
        assert!(resp.status().is_success());
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn post_chat_completions_exhausts_after_max_attempts() {
        let canned: Vec<&'static str> = (0..3)
            .map(|_| Box::leak(rate_limited_body(Some("0")).into_boxed_str()) as &'static str)
            .collect();
        let (url, counter, _) = spawn_canned_server(canned).await;
        let client = build_client(url, None, 3, 0);
        let err = client
            .post_chat_completions(&serde_json::json!({"model": "m", "messages": []}), false)
            .await
            .unwrap_err();
        let ClientError::Exhausted { status, attempts } = err else {
            panic!("expected Exhausted, got {err:?}");
        };
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(attempts, 3);
        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }
}
