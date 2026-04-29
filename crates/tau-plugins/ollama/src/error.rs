//! Translate Ollama HTTP responses + transport failures to typed
//! `tau_ports::LlmError` variants.
//!
//! Per ADR-0009 (typed-error migration policy):
//! - HTTP responses map to typed variants (`RateLimited`, `Auth`,
//!   `InvalidRequest`, `Provider`).
//! - Transport failures map to `Transport`.
//! - `LlmError::Internal` is RESERVED for plugin-internal translation
//!   errors (e.g., wrapping a `BuildError`); never used here for HTTP
//!   responses.
//!
//! Ollama's error envelope is `{"error": "<message>"}` — much simpler
//! than Anthropic's typed shape. The 404 mapping preserves the
//! "ollama pull" remediation hint inline in
//! `LlmError::InvalidRequest.reason`. 503-on-model-load is the load-
//! bearing retryable case; maps to `Provider` (retryable per
//! `is_retryable()`).
//!
//! `map_response_error` signature `(status, headers, body)` is shared
//! across all three plugins so 429 responses can populate
//! `RateLimited.retry_after_seconds` from the `Retry-After` HTTP header.

use serde::Deserialize;
use tau_ports::LlmError;
use thiserror::Error;

/// Internal error type from the HTTP client (`client.rs`).
///
/// Defined here so this module can be the single point of `LlmError`
/// translation. `client.rs` imports this and constructs the variants
/// from its retry loop.
#[non_exhaustive]
#[derive(Debug, Error)]
pub(crate) enum ClientError {
    /// Underlying transport failure (network, TLS, DNS, etc.).
    /// Distinct from a non-success status code, which is handled
    /// separately via `map_response_error`.
    #[error("transport: {0}")]
    Transport(reqwest::Error),

    /// Retry budget exhausted. The `status` is the last status
    /// observed (typically 429, 503, or 408 synthesized from timeout).
    #[error("retries exhausted: {status} after {attempts} attempts")]
    Exhausted {
        /// Last observed status code that triggered the final retry decision.
        status: reqwest::StatusCode,
        /// Total attempt count (initial + retries).
        attempts: u32,
    },
}

#[derive(Deserialize)]
struct OllamaErrorBody {
    error: String,
}

fn extract_detail(body: &str) -> String {
    serde_json::from_str::<OllamaErrorBody>(body)
        .ok()
        .map(|p| p.error)
        .unwrap_or_else(|| body.to_string())
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u32> {
    headers
        .get("retry-after")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<u32>().ok())
}

/// Map a non-2xx Ollama response to a typed `LlmError`.
///
/// 404 messages embed the `ollama pull <model>` remediation hint
/// inline in `InvalidRequest.reason` since pulling the model is by
/// far the most common remediation for new Ollama users.
pub(crate) fn map_response_error(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> LlmError {
    let detail = extract_detail(body);

    match status.as_u16() {
        400 => LlmError::InvalidRequest {
            reason: format!("ollama bad request: {detail}"),
        },
        401 | 403 => LlmError::Auth { message: detail },
        404 => LlmError::InvalidRequest {
            // Preserve the existing remediation hint inline.
            reason: format!("ollama model not found (run `ollama pull <model>` first): {detail}"),
        },
        429 => LlmError::RateLimited {
            retry_after_seconds: parse_retry_after(headers),
        },
        500..=599 => LlmError::Provider {
            // 503-on-model-load is the load-bearing case; Provider is
            // retryable per is_retryable().
            message: format!("ollama server error ({status}): {detail}"),
        },
        _ => LlmError::Provider {
            message: format!("ollama unexpected status ({status}): {detail}"),
        },
    }
}

/// Map a `ClientError` (transport or retry-exhausted) to a typed `LlmError`.
pub(crate) fn map_client_error(err: ClientError) -> LlmError {
    match err {
        ClientError::Transport(e) => LlmError::Transport {
            message: format!("ollama transport: {e}"),
        },
        ClientError::Exhausted { status, attempts } => match status.as_u16() {
            429 => LlmError::RateLimited {
                retry_after_seconds: None,
            },
            408 => LlmError::Transport {
                message: format!("ollama retries exhausted on timeout ({attempts} attempts)"),
            },
            500..=599 => LlmError::Provider {
                message: format!(
                    "ollama retries exhausted ({attempts} attempts, last status {status})",
                ),
            },
            _ => LlmError::Provider {
                message: format!(
                    "ollama retries exhausted ({attempts} attempts, last status {status})",
                ),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    fn empty_headers() -> reqwest::header::HeaderMap {
        reqwest::header::HeaderMap::new()
    }

    fn headers_with_retry_after(seconds: &str) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert(
            reqwest::header::RETRY_AFTER,
            reqwest::header::HeaderValue::from_str(seconds).unwrap(),
        );
        h
    }

    #[test]
    fn map_404_returns_invalid_request_with_remediation_hint() {
        let body = r#"{"error":"model 'llama99' not found, try pulling it first"}"#;
        let err = map_response_error(StatusCode::NOT_FOUND, &empty_headers(), body);
        let LlmError::InvalidRequest { reason } = err else {
            panic!("expected InvalidRequest, got {err:?}");
        };
        assert!(
            reason.contains("ollama pull"),
            "expected ollama pull remediation hint; got: {reason}"
        );
        assert!(reason.contains("model 'llama99' not found"));
    }

    #[test]
    fn map_400_returns_invalid_request() {
        let body = r#"{"error":"bad request: invalid model name"}"#;
        let err = map_response_error(StatusCode::BAD_REQUEST, &empty_headers(), body);
        let LlmError::InvalidRequest { reason } = err else {
            panic!("expected InvalidRequest, got {err:?}");
        };
        assert!(reason.contains("ollama bad request"));
        assert!(reason.contains("invalid model name"));
    }

    #[test]
    fn map_429_returns_rate_limited_with_retry_after() {
        let body = r#"{"error":"throttled"}"#;
        let err = map_response_error(
            StatusCode::TOO_MANY_REQUESTS,
            &headers_with_retry_after("4"),
            body,
        );
        let LlmError::RateLimited {
            retry_after_seconds,
        } = err
        else {
            panic!("expected RateLimited, got {err:?}");
        };
        assert_eq!(retry_after_seconds, Some(4));
    }

    #[test]
    fn map_503_returns_provider_retryable() {
        let body = r#"{"error":"model is loading"}"#;
        let err = map_response_error(StatusCode::SERVICE_UNAVAILABLE, &empty_headers(), body);
        let LlmError::Provider { ref message } = err else {
            panic!("expected Provider, got {err:?}");
        };
        assert!(message.contains("server error"));
        assert!(message.contains("model is loading"));
        assert!(err.is_retryable(), "Provider should be retryable");
    }

    #[test]
    fn map_500_unstructured_body_falls_back_to_raw() {
        let body = "<html><body>Internal Server Error</body></html>";
        let err = map_response_error(StatusCode::INTERNAL_SERVER_ERROR, &empty_headers(), body);
        let LlmError::Provider { message } = err else {
            panic!("expected Provider, got {err:?}");
        };
        assert!(message.contains("<html>"));
    }

    #[test]
    fn map_401_returns_auth() {
        let body = r#"{"error":"unauthorized"}"#;
        let err = map_response_error(StatusCode::UNAUTHORIZED, &empty_headers(), body);
        let LlmError::Auth { message } = err else {
            panic!("expected Auth, got {err:?}");
        };
        assert!(message.contains("unauthorized"));
    }

    #[test]
    fn map_client_error_exhausted_503_returns_provider() {
        let err = map_client_error(ClientError::Exhausted {
            status: StatusCode::SERVICE_UNAVAILABLE,
            attempts: 3,
        });
        let LlmError::Provider { message } = err else {
            panic!("expected Provider, got {err:?}");
        };
        assert!(message.contains("retries exhausted"));
        assert!(message.contains("503"));
    }

    #[test]
    fn map_client_error_exhausted_429_returns_rate_limited() {
        let err = map_client_error(ClientError::Exhausted {
            status: StatusCode::TOO_MANY_REQUESTS,
            attempts: 3,
        });
        let LlmError::RateLimited {
            retry_after_seconds,
        } = err
        else {
            panic!("expected RateLimited, got {err:?}");
        };
        assert_eq!(retry_after_seconds, None);
    }
}
