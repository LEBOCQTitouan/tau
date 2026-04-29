//! HTTP status + Anthropic error JSON → tau_ports::LlmError.
//!
//! Per spec §4.4 + decision #16:
//! - All 4xx / 5xx responses collapse to `LlmError::Internal { message }`
//!   with a category prefix.
//! - `ClientError` (transport / retry-exhausted) collapses similarly.
//! - The error message preserves enough detail (Anthropic's `error.type`
//!   + `error.message`) for debugging.
//! - Richer variants (`RateLimited`, `Auth`, etc.) are deferred to a
//!   future ADR-amendment paired with the second LLM-backend plugin.

use serde::Deserialize;
use tau_ports::LlmError;
use thiserror::Error;

/// Internal error type from the HTTP client (`client.rs`, Task 7).
///
/// Defined here so this module can be the single point of `LlmError`
/// translation. Task 7 imports this and constructs the variants from
/// its retry loop.
#[non_exhaustive]
#[derive(Debug, Error)]
pub(crate) enum ClientError {
    /// Underlying transport failure (network, TLS, DNS, etc.).
    /// Distinct from a non-success status code, which is handled
    /// separately via `map_response_error`.
    #[error("transport: {0}")]
    Transport(reqwest::Error),

    /// Retry budget exhausted. The `status` is the last status
    /// observed (always 429 or 503 today).
    #[error("retries exhausted: {status} after {attempts} attempts")]
    Exhausted {
        /// Last observed status code that triggered the final retry decision.
        status: reqwest::StatusCode,
        /// Total attempt count (initial + retries).
        attempts: u32,
    },
}

/// Anthropic's error envelope (when the body parses as one).
///
/// Anthropic returns errors as:
/// ```json
/// {"type": "error", "error": {"type": "rate_limit_error", "message": "..."}}
/// ```
#[derive(Debug, Deserialize)]
struct AnthropicErrorBody {
    #[allow(dead_code)]
    r#type: String, // always "error"
    error: AnthropicErrorDetail,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorDetail {
    r#type: String, // e.g. "rate_limit_error", "authentication_error"
    message: String,
}

/// Map an HTTP non-success response to an `LlmError`.
///
/// Categorizes by status range; preserves Anthropic's `error.type` and
/// `error.message` in the resulting `LlmError::Internal` message when
/// the body parses as Anthropic's standard error envelope. Falls back
/// to the raw body when it doesn't.
pub(crate) fn map_response_error(status: reqwest::StatusCode, body: &str) -> LlmError {
    let parsed: Option<AnthropicErrorBody> = serde_json::from_str(body).ok();
    let detail = parsed
        .as_ref()
        .map(|p| format!("{}: {}", p.error.r#type, p.error.message))
        .unwrap_or_else(|| body.to_string());

    let category = match status.as_u16() {
        400 => "bad request",
        401 | 403 => "auth failure",
        404 => "not found",
        429 => "rate limited (retries exhausted)",
        500..=599 => "server error",
        _ => "unexpected status",
    };

    LlmError::Internal {
        message: format!("anthropic {category} ({status}): {detail}"),
    }
}

/// Map a `ClientError` (transport failure or retry exhaustion) to an
/// `LlmError`. The retry loop in `client.rs` produces these.
pub(crate) fn map_client_error(err: ClientError) -> LlmError {
    match err {
        ClientError::Transport(e) => LlmError::Internal {
            message: format!("anthropic transport error: {e}"),
        },
        ClientError::Exhausted { status, attempts } => LlmError::Internal {
            message: format!("anthropic retries exhausted: {status} after {attempts} attempts",),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    fn err_body(error_type: &str, message: &str) -> String {
        format!(r#"{{"type":"error","error":{{"type":"{error_type}","message":"{message}"}}}}"#)
    }

    #[test]
    fn maps_429_to_rate_limited_internal() {
        let body = err_body(
            "rate_limit_error",
            "Number of request tokens has exceeded your per-minute rate limit",
        );
        let err = map_response_error(StatusCode::TOO_MANY_REQUESTS, &body);
        let LlmError::Internal { ref message } = err else {
            panic!("expected Internal, got {err:?}");
        };
        assert!(message.contains("rate limited"));
        assert!(message.contains("rate_limit_error"));
        assert!(message.contains("per-minute rate limit"));
    }

    #[test]
    fn maps_401_to_auth_failure_internal() {
        let body = err_body("authentication_error", "invalid x-api-key");
        let err = map_response_error(StatusCode::UNAUTHORIZED, &body);
        let LlmError::Internal { ref message } = err else {
            panic!()
        };
        assert!(message.contains("auth failure"));
        assert!(message.contains("authentication_error"));
    }

    #[test]
    fn maps_500_to_server_error_internal() {
        let body = err_body("api_error", "Internal server error");
        let err = map_response_error(StatusCode::INTERNAL_SERVER_ERROR, &body);
        let LlmError::Internal { ref message } = err else {
            panic!()
        };
        assert!(message.contains("server error"));
        assert!(message.contains("api_error"));
    }

    #[test]
    fn maps_400_to_bad_request_internal() {
        let body = err_body("invalid_request_error", "tools[0].name does not match");
        let err = map_response_error(StatusCode::BAD_REQUEST, &body);
        let LlmError::Internal { ref message } = err else {
            panic!()
        };
        assert!(message.contains("bad request"));
    }

    #[test]
    fn falls_back_to_raw_body_on_unparseable_json() {
        let body = "<html>503 Service Unavailable</html>";
        let err = map_response_error(StatusCode::SERVICE_UNAVAILABLE, body);
        let LlmError::Internal { ref message } = err else {
            panic!()
        };
        assert!(message.contains("server error"));
        assert!(message.contains("<html>"));
    }

    #[test]
    fn map_client_error_exhausted_includes_attempt_count() {
        let err = map_client_error(ClientError::Exhausted {
            status: StatusCode::TOO_MANY_REQUESTS,
            attempts: 3,
        });
        let LlmError::Internal { ref message } = err else {
            panic!()
        };
        assert!(message.contains("retries exhausted"));
        assert!(message.contains("3 attempts"));
    }
}
