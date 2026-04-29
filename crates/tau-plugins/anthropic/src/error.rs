//! HTTP status + headers + Anthropic error JSON → typed
//! `tau_ports::LlmError`.
//!
//! Per ADR-0009 (typed-error migration policy):
//! - HTTP responses map to typed variants (`RateLimited`, `Auth`,
//!   `InvalidRequest`, `Provider`).
//! - Transport failures map to `Transport`.
//! - `LlmError::Internal` is RESERVED for plugin-internal translation
//!   errors (e.g., wrapping a `BuildError`); never used here for HTTP
//!   responses.
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

/// Anthropic's error envelope shape.
#[derive(Debug, Deserialize)]
struct AnthropicErrorBody {
    #[allow(dead_code)]
    r#type: String, // always "error"
    error: AnthropicErrorDetail,
}

#[derive(Debug, Deserialize, Default)]
struct AnthropicErrorDetail {
    #[serde(default, rename = "type")]
    error_type: String, // e.g. "rate_limit_error", "authentication_error"
    #[serde(default)]
    message: String,
}

fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<u32> {
    headers
        .get("retry-after")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<u32>().ok())
}

/// Map a non-2xx Anthropic response to a typed `LlmError`.
///
/// Preserves Anthropic's `error.type` + `error.message` detail in the
/// resulting variant's payload. `headers` is needed for `RateLimited`'s
/// `retry_after_seconds` from the `Retry-After` HTTP header on 429.
pub(crate) fn map_response_error(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> LlmError {
    let detail = serde_json::from_str::<AnthropicErrorBody>(body)
        .ok()
        .map(|p| p.error)
        .unwrap_or_else(|| AnthropicErrorDetail {
            error_type: String::new(),
            message: body.to_string(),
        });

    match status.as_u16() {
        400 => LlmError::InvalidRequest {
            reason: format_with_type("anthropic bad request", &detail),
        },
        401 | 403 => LlmError::Auth {
            message: detail.message,
        },
        404 => LlmError::InvalidRequest {
            reason: format_with_type("anthropic not found", &detail),
        },
        429 => LlmError::RateLimited {
            retry_after_seconds: parse_retry_after(headers),
        },
        500..=599 => LlmError::Provider {
            message: format!(
                "anthropic server error ({status}): {}",
                format_with_type_inline(&detail),
            ),
        },
        _ => LlmError::Provider {
            message: format!(
                "anthropic unexpected status ({status}): {}",
                format_with_type_inline(&detail),
            ),
        },
    }
}

fn format_with_type(prefix: &str, detail: &AnthropicErrorDetail) -> String {
    if detail.error_type.is_empty() {
        format!("{prefix}: {}", detail.message)
    } else {
        format!("{prefix}: {}: {}", detail.error_type, detail.message)
    }
}

fn format_with_type_inline(detail: &AnthropicErrorDetail) -> String {
    if detail.error_type.is_empty() {
        detail.message.clone()
    } else {
        format!("{}: {}", detail.error_type, detail.message)
    }
}

/// Map a `ClientError` (transport or retry-exhausted) to a typed `LlmError`.
pub(crate) fn map_client_error(err: ClientError) -> LlmError {
    match err {
        ClientError::Transport(e) => LlmError::Transport {
            message: format!("anthropic transport: {e}"),
        },
        ClientError::Exhausted { status, attempts } => match status.as_u16() {
            429 => LlmError::RateLimited {
                retry_after_seconds: None,
            },
            408 => LlmError::Transport {
                message: format!("anthropic retries exhausted on timeout ({attempts} attempts)"),
            },
            500..=599 => LlmError::Provider {
                message: format!(
                    "anthropic retries exhausted ({attempts} attempts, last status {status})",
                ),
            },
            _ => LlmError::Provider {
                message: format!(
                    "anthropic retries exhausted ({attempts} attempts, last status {status})",
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

    fn err_body(error_type: &str, message: &str) -> String {
        format!(r#"{{"type":"error","error":{{"type":"{error_type}","message":"{message}"}}}}"#)
    }

    #[test]
    fn map_429_returns_rate_limited_with_retry_after() {
        let body = err_body("rate_limit_error", "throttled");
        let err = map_response_error(
            StatusCode::TOO_MANY_REQUESTS,
            &headers_with_retry_after("7"),
            &body,
        );
        let LlmError::RateLimited {
            retry_after_seconds,
        } = err
        else {
            panic!("expected RateLimited, got {err:?}");
        };
        assert_eq!(retry_after_seconds, Some(7));
    }

    #[test]
    fn map_429_without_retry_after_returns_none() {
        let body = err_body("rate_limit_error", "throttled");
        let err = map_response_error(StatusCode::TOO_MANY_REQUESTS, &empty_headers(), &body);
        let LlmError::RateLimited {
            retry_after_seconds,
        } = err
        else {
            panic!("expected RateLimited, got {err:?}");
        };
        assert_eq!(retry_after_seconds, None);
    }

    #[test]
    fn map_401_returns_auth() {
        let body = err_body("authentication_error", "invalid x-api-key");
        let err = map_response_error(StatusCode::UNAUTHORIZED, &empty_headers(), &body);
        let LlmError::Auth { message } = err else {
            panic!("expected Auth, got {err:?}");
        };
        assert!(message.contains("invalid x-api-key"));
    }

    #[test]
    fn map_400_returns_invalid_request() {
        let body = err_body("invalid_request_error", "tools[0].name does not match");
        let err = map_response_error(StatusCode::BAD_REQUEST, &empty_headers(), &body);
        let LlmError::InvalidRequest { reason } = err else {
            panic!("expected InvalidRequest, got {err:?}");
        };
        assert!(reason.contains("anthropic bad request"));
        assert!(reason.contains("invalid_request_error"));
    }

    #[test]
    fn map_500_returns_provider_retryable() {
        let body = err_body("api_error", "Internal server error");
        let err = map_response_error(StatusCode::INTERNAL_SERVER_ERROR, &empty_headers(), &body);
        let LlmError::Provider { ref message } = err else {
            panic!("expected Provider, got {err:?}");
        };
        assert!(message.contains("server error"));
        assert!(err.is_retryable());
    }

    #[test]
    fn map_unstructured_body_falls_back() {
        let body = "<html>503 Service Unavailable</html>";
        let err = map_response_error(StatusCode::SERVICE_UNAVAILABLE, &empty_headers(), body);
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
    }
}
