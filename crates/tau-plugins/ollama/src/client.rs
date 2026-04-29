//! Ollama HTTP client with retry + optional bearer-token auth.
//!
//! `post_chat` is the only outbound entrypoint. Owns a `reqwest::Client`
//! with TLS + timeout configured by the plugin's `Configure::from_config`.
//!
//! Retry policy per spec §4.1:
//! - 2xx → return immediately.
//! - 429, 503 → retry with exponential backoff; honor `Retry-After`
//!   when `retry.respect_retry_after` is true. Exhausted →
//!   `ClientError::Exhausted`. **503 is the load-bearing case for
//!   Ollama** (returned during model load).
//! - 4xx other than 429 → return immediately (caller maps to LlmError).
//! - 5xx other than 503 → retry (treated as transient).
//! - Network timeout → retry.
//! - Other transport error (DNS, TLS, connection refused) →
//!   `ClientError::Transport`, no retry.

use reqwest::{Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use std::time::Duration;
use tokio::time::sleep;

use crate::config::RetryConfig;
use crate::error::ClientError;

/// HTTP client that knows how to call Ollama's `/api/chat` endpoint.
pub(crate) struct OllamaClient {
    inner: reqwest::Client,
    base_url: String,
    bearer_token: Option<SecretString>,
    retry: RetryConfig,
}

impl OllamaClient {
    /// Construct a client. The caller (Task 8 `Configure::from_config`)
    /// validates inputs.
    #[allow(dead_code)]
    pub(crate) fn new(
        inner: reqwest::Client,
        base_url: String,
        bearer_token: Option<SecretString>,
        retry: RetryConfig,
    ) -> Self {
        Self {
            inner,
            base_url,
            bearer_token,
            retry,
        }
    }

    /// `POST /api/chat` with retry. The body is a `serde_json::Value`
    /// produced by `request::build_chat_body`. The `stream` flag is
    /// already encoded inside `body.stream`; no additional `Accept`
    /// header is needed (Ollama emits NDJSON when `body.stream: true`).
    #[allow(dead_code)]
    pub(crate) async fn post_chat(
        &self,
        body: &serde_json::Value,
        _stream: bool,
    ) -> Result<Response, ClientError> {
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));
        let mut attempt: u32 = 0;

        loop {
            attempt += 1;

            let mut req = self
                .inner
                .post(&url)
                .header("content-type", "application/json")
                .json(body);
            if let Some(token) = &self.bearer_token {
                req = req.header("authorization", format!("Bearer {}", token.expose_secret()));
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
                        target: "ollama_plugin::retry",
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
                    // Non-retryable 4xx (or unexpected): caller maps.
                    Decision::Return(resp)
                }
            }
            Err(e) if e.is_timeout() => {
                let delay_ms = self.backoff_only(attempt);
                // No status; use 408 (Request Timeout) as a placeholder
                // for the Exhausted variant when timeouts pile up.
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
        // base * 2^(attempt-1), capped at 60s.
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
    /// pre-canned responses for the first N connections, then becomes
    /// silent. Returns the listener URL + a counter of accepted
    /// connections + a vector of received request bytes (for asserting
    /// header presence).
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

    fn service_unavailable_body(retry_after: Option<&str>) -> String {
        let mut s = String::from("HTTP/1.1 503 Service Unavailable\r\n");
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
        bearer_token: Option<SecretString>,
        max_attempts: u32,
        base_delay_ms: u64,
    ) -> OllamaClient {
        OllamaClient::new(
            reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            base_url,
            bearer_token,
            RetryConfig {
                max_attempts,
                base_delay_ms,
                respect_retry_after: true,
            },
        )
    }

    #[tokio::test]
    async fn post_chat_happy_path_no_bearer_token() {
        let (url, counter, received) = spawn_canned_server(vec![ok_body()]).await;
        let client = build_client(url, None, 3, 0);
        let resp = client
            .post_chat(&serde_json::json!({"model": "m", "messages": []}), false)
            .await
            .unwrap();
        assert!(resp.status().is_success());
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        // Critical: NO authorization header should have been sent.
        let raw = received.lock().await;
        let req_str = std::str::from_utf8(&raw[0]).unwrap_or("");
        assert!(
            !req_str.to_ascii_lowercase().contains("authorization"),
            "expected NO Authorization header on local Ollama; got: {req_str}"
        );
    }

    #[tokio::test]
    async fn post_chat_with_bearer_token_sends_authorization_header() {
        let (url, counter, received) = spawn_canned_server(vec![ok_body()]).await;
        let token = SecretString::new("hosted-token-xyz".into());
        let client = build_client(url, Some(token), 3, 0);
        let resp = client
            .post_chat(&serde_json::json!({"model": "m", "messages": []}), false)
            .await
            .unwrap();
        assert!(resp.status().is_success());
        assert_eq!(counter.load(Ordering::Relaxed), 1);

        let raw = received.lock().await;
        let req_str = std::str::from_utf8(&raw[0]).unwrap_or("");
        assert!(
            req_str
                .to_ascii_lowercase()
                .contains("authorization: bearer hosted-token-xyz"),
            "expected `Authorization: Bearer hosted-token-xyz` header; got: {req_str}"
        );
    }

    #[tokio::test]
    async fn post_chat_503_then_200_succeeds_after_retry() {
        // THE LOAD-BEARING CASE: Ollama returns 503 during model load.
        let canned = vec![
            Box::leak(service_unavailable_body(Some("0")).into_boxed_str()) as &'static str,
            Box::leak(service_unavailable_body(Some("0")).into_boxed_str()) as &'static str,
            ok_body(),
        ];
        let (url, counter, _) = spawn_canned_server(canned).await;
        let client = build_client(url, None, 3, 0);
        let resp = client
            .post_chat(&serde_json::json!({"model": "m", "messages": []}), false)
            .await
            .unwrap();
        assert!(resp.status().is_success());
        assert_eq!(counter.load(Ordering::Relaxed), 3, "should have 3 attempts");
    }

    #[tokio::test]
    async fn post_chat_429_with_retry_after_honors_header() {
        let canned = vec![
            Box::leak(rate_limited_body(Some("0")).into_boxed_str()) as &'static str,
            ok_body(),
        ];
        let (url, counter, _) = spawn_canned_server(canned).await;
        let client = build_client(url, None, 3, 0);
        let resp = client
            .post_chat(&serde_json::json!({"model": "m", "messages": []}), false)
            .await
            .unwrap();
        assert!(resp.status().is_success());
        assert_eq!(counter.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn post_chat_exhausts_after_max_attempts() {
        let canned: Vec<&'static str> = (0..3)
            .map(|_| {
                Box::leak(service_unavailable_body(Some("0")).into_boxed_str()) as &'static str
            })
            .collect();
        let (url, counter, _) = spawn_canned_server(canned).await;
        let client = build_client(url, None, 3, 0);
        let err = client
            .post_chat(&serde_json::json!({"model": "m", "messages": []}), false)
            .await
            .unwrap_err();
        let ClientError::Exhausted { status, attempts } = err else {
            panic!("expected Exhausted, got {err:?}");
        };
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(attempts, 3);
        assert_eq!(counter.load(Ordering::Relaxed), 3);
    }
}
