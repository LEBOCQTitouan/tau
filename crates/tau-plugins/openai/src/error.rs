//! Translate OpenAI HTTP responses + transport failures to typed
//! `tau_ports::LlmError` variants.
//!
//! Per spec §4.4 and ADR-0009 (typed-error migration policy):
//! - HTTP responses map to typed variants (`RateLimited`, `Auth`,
//!   `InvalidRequest`, `Provider`).
//! - Transport failures map to `Transport`.
//! - `LlmError::Internal` is RESERVED for plugin-internal translation
//!   errors (e.g., wrapping a `BuildError`); never used here for HTTP
//!   responses.
//!
//! `map_response_error` signature `(status, headers, body)` is shared
//! across all three plugins so 429 responses can populate
//! `RateLimited.retry_after_seconds` from the `Retry-After` HTTP
//! header. Anthropic + Ollama adopt this signature in Tasks 17-18.

use serde::Deserialize;
use tau_ports::LlmError;
use thiserror::Error;

/// Internal error type from the HTTP client (`client.rs`, Task 9).
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

/// OpenAI's error envelope shape.
#[derive(Deserialize)]
struct OpenAIErrorBody {
    error: OpenAIErrorDetail,
}

#[derive(Deserialize, Default)]
struct OpenAIErrorDetail {
    #[serde(default)]
    message: String,
    #[serde(default, rename = "type")]
    error_type: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    code: Option<String>,
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u32> {
    headers
        .get("retry-after")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<u32>().ok())
}

/// Map a non-2xx OpenAI response to a typed `LlmError`.
///
/// `headers` is needed for `RateLimited`'s `retry_after_seconds` from
/// the `Retry-After` header on 429 responses.
pub(crate) fn map_response_error(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> LlmError {
    // Defensive: parse body as OpenAI envelope; fall back to raw on failure.
    let detail = serde_json::from_str::<OpenAIErrorBody>(body)
        .ok()
        .map(|p| p.error)
        .unwrap_or_else(|| OpenAIErrorDetail {
            message: body.to_string(),
            error_type: None,
            code: None,
        });

    match status.as_u16() {
        400 => LlmError::InvalidRequest {
            reason: format_invalid_request("openai bad request", &detail),
        },
        401 | 403 => LlmError::Auth {
            message: detail.message,
        },
        404 => LlmError::InvalidRequest {
            // No typed ModelNotFound at v0.1; embed remediation in reason.
            reason: format!("openai not found: {}", detail.message),
        },
        429 => LlmError::RateLimited {
            retry_after_seconds: parse_retry_after(headers),
        },
        500..=599 => LlmError::Provider {
            message: format!("openai server error ({status}): {}", detail.message),
        },
        _ => LlmError::Provider {
            message: format!("openai unexpected status ({status}): {}", detail.message),
        },
    }
}

fn format_invalid_request(prefix: &str, detail: &OpenAIErrorDetail) -> String {
    match &detail.error_type {
        Some(t) => format!("{prefix}: {t}: {}", detail.message),
        None => format!("{prefix}: {}", detail.message),
    }
}

/// Map a `ClientError` (transport or retry-exhausted) to a typed `LlmError`.
///
/// - `Transport(e)` → `Transport`.
/// - `Exhausted { 429, attempts }` → `RateLimited { retry_after_seconds: None }`
///   (already exhausted; no point passing the last header).
/// - `Exhausted { 408, attempts }` → `Transport` (synthesized from timeout).
/// - `Exhausted { 5xx, attempts }` → `Provider` (retryable transient).
/// - `Exhausted { other, attempts }` → `Provider` (catch-all transient bucket).
pub(crate) fn map_client_error(err: ClientError) -> LlmError {
    match err {
        ClientError::Transport(e) => LlmError::Transport {
            message: format!("openai transport: {e}"),
        },
        ClientError::Exhausted { status, attempts } => match status.as_u16() {
            429 => LlmError::RateLimited {
                retry_after_seconds: None,
            },
            408 => LlmError::Transport {
                message: format!("openai retries exhausted on timeout ({attempts} attempts)"),
            },
            500..=599 => LlmError::Provider {
                message: format!(
                    "openai retries exhausted ({attempts} attempts, last status {status})"
                ),
            },
            _ => LlmError::Provider {
                message: format!(
                    "openai retries exhausted ({attempts} attempts, last status {status})"
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
    fn map_400_returns_invalid_request() {
        let body = r#"{"error":{"message":"bad model","type":"invalid_request_error","code":"model_not_found"}}"#;
        let err = map_response_error(StatusCode::BAD_REQUEST, &empty_headers(), body);
        let LlmError::InvalidRequest { reason } = err else {
            panic!("expected InvalidRequest, got {err:?}");
        };
        assert!(reason.contains("openai bad request"));
        assert!(reason.contains("invalid_request_error"));
        assert!(reason.contains("bad model"));
    }

    #[test]
    fn map_401_returns_auth() {
        let body = r#"{"error":{"message":"Invalid API key","type":"invalid_request_error"}}"#;
        let err = map_response_error(StatusCode::UNAUTHORIZED, &empty_headers(), body);
        let LlmError::Auth { message } = err else {
            panic!("expected Auth, got {err:?}");
        };
        assert!(message.contains("Invalid API key"));
    }

    #[test]
    fn map_404_returns_invalid_request() {
        let body = r#"{"error":{"message":"The model `gpt-99` does not exist"}}"#;
        let err = map_response_error(StatusCode::NOT_FOUND, &empty_headers(), body);
        let LlmError::InvalidRequest { reason } = err else {
            panic!("expected InvalidRequest, got {err:?}");
        };
        assert!(reason.contains("openai not found"));
        assert!(reason.contains("gpt-99"));
    }

    #[test]
    fn map_429_with_retry_after_header_populates_seconds() {
        let body = r#"{"error":{"message":"Rate limit exceeded"}}"#;
        let err = map_response_error(
            StatusCode::TOO_MANY_REQUESTS,
            &headers_with_retry_after("5"),
            body,
        );
        let LlmError::RateLimited {
            retry_after_seconds,
        } = err
        else {
            panic!("expected RateLimited, got {err:?}");
        };
        assert_eq!(retry_after_seconds, Some(5));
    }

    #[test]
    fn map_429_without_retry_after_returns_none() {
        let body = r#"{"error":{"message":"Rate limit exceeded"}}"#;
        let err = map_response_error(StatusCode::TOO_MANY_REQUESTS, &empty_headers(), body);
        let LlmError::RateLimited {
            retry_after_seconds,
        } = err
        else {
            panic!("expected RateLimited, got {err:?}");
        };
        assert_eq!(retry_after_seconds, None);
    }

    #[test]
    fn map_500_returns_provider_retryable() {
        let body = r#"{"error":{"message":"server overloaded"}}"#;
        let err = map_response_error(StatusCode::INTERNAL_SERVER_ERROR, &empty_headers(), body);
        assert!(err.is_retryable(), "Provider should be retryable");
        let LlmError::Provider { message } = err else {
            panic!("expected Provider");
        };
        assert!(message.contains("server error"));
    }

    #[test]
    fn map_unstructured_body_falls_back_to_raw() {
        let body = "<html>Internal Server Error</html>";
        let err = map_response_error(StatusCode::INTERNAL_SERVER_ERROR, &empty_headers(), body);
        let LlmError::Provider { message } = err else {
            panic!("expected Provider, got {err:?}");
        };
        assert!(message.contains("<html>"));
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

    #[test]
    fn map_client_error_exhausted_500_returns_provider() {
        let err = map_client_error(ClientError::Exhausted {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            attempts: 3,
        });
        let LlmError::Provider { message } = err else {
            panic!("expected Provider, got {err:?}");
        };
        assert!(message.contains("retries exhausted"));
        assert!(message.contains("3 attempts"));
    }

    #[test]
    fn map_client_error_exhausted_408_returns_transport() {
        let err = map_client_error(ClientError::Exhausted {
            status: StatusCode::REQUEST_TIMEOUT,
            attempts: 3,
        });
        let LlmError::Transport { message } = err else {
            panic!("expected Transport, got {err:?}");
        };
        assert!(message.contains("timeout"));
    }
}
