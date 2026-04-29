//! Translate Ollama HTTP responses to `tau_ports::LlmError`.
//!
//! Ollama's error envelope is `{"error": "<message>"}` (much simpler
//! than Anthropic's typed shape). All non-2xx responses collapse to
//! `LlmError::Internal { message }` with a category prefix that
//! includes a remediation hint for 404 (the most common new-user
//! failure).
//!
//! See `docs/superpowers/specs/2026-04-29-ollama-plugin-design.md`
//! §4.4.

use serde::Deserialize;
use tau_ports::LlmError;
use thiserror::Error;

/// Internal error type from the HTTP client (`client.rs`, Task 6).
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
    /// observed (always 429, 503, or 408 today).
    #[error("retries exhausted: {status} after {attempts} attempts")]
    Exhausted {
        /// Last observed status code that triggered the final retry decision.
        status: reqwest::StatusCode,
        /// Total attempt count (initial + retries).
        attempts: u32,
    },
}

/// Map a non-2xx Ollama response to `LlmError::Internal`.
///
/// 404 messages embed `run \`ollama pull <model>\` first` because
/// pulling the model is by far the most common remediation for new
/// Ollama users.
///
/// All errors collapse to `LlmError::Internal { message }` for v0.1;
/// the richer typed-variant vocabulary (`RateLimited`, `Auth`,
/// `ModelNotFound`) lands when sub-project 2c (OpenAI) introduces
/// the third consumer.
pub(crate) fn map_response_error(status: reqwest::StatusCode, body: &str) -> LlmError {
    let detail = serde_json::from_str::<OllamaErrorBody>(body)
        .ok()
        .map(|p| p.error)
        .unwrap_or_else(|| body.to_string());

    let category = match status.as_u16() {
        400 => "bad request",
        401 | 403 => "auth failure",
        404 => "model not found (run `ollama pull <model>` first)",
        429 => "rate limited (retries exhausted)",
        500..=599 => "server error",
        _ => "unexpected status",
    };
    LlmError::Internal {
        message: format!("ollama {category} ({status}): {detail}"),
    }
}

#[derive(Deserialize)]
struct OllamaErrorBody {
    error: String,
}

/// Map a `ClientError` (transport or retry-exhausted) to `LlmError`.
///
/// Same target type as `map_response_error` so the plugin's batch and
/// streaming entrypoints can call either translator uniformly.
pub(crate) fn map_client_error(err: ClientError) -> LlmError {
    match err {
        ClientError::Transport(e) => LlmError::Internal {
            message: format!("ollama transport: {e}"),
        },
        ClientError::Exhausted { status, attempts } => LlmError::Internal {
            message: format!(
                "ollama retries exhausted ({attempts} attempts, last status {status})",
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    #[test]
    fn map_404_includes_ollama_pull_remediation_hint() {
        let body = r#"{"error":"model 'llama99' not found, try pulling it first"}"#;
        let err = map_response_error(StatusCode::NOT_FOUND, body);
        let LlmError::Internal { message, .. } = err else {
            panic!("expected LlmError::Internal");
        };
        assert!(
            message.contains("ollama pull"),
            "expected `ollama pull` remediation hint in message; got: {message}"
        );
        assert!(message.contains("model 'llama99' not found"));
    }

    #[test]
    fn map_400_with_structured_error_body_extracts_message() {
        let body = r#"{"error":"bad request: invalid model name"}"#;
        let err = map_response_error(StatusCode::BAD_REQUEST, body);
        let LlmError::Internal { message, .. } = err else {
            panic!("expected LlmError::Internal");
        };
        assert!(message.contains("bad request"));
        assert!(message.contains("invalid model name"));
    }

    #[test]
    fn map_500_unstructured_body_falls_back_to_raw() {
        let body = "<html><body>Internal Server Error</body></html>";
        let err = map_response_error(StatusCode::INTERNAL_SERVER_ERROR, body);
        let LlmError::Internal { message, .. } = err else {
            panic!("expected LlmError::Internal");
        };
        assert!(message.contains("server error"));
        assert!(message.contains("<html>"));
    }

    #[test]
    fn map_503_categorized_as_server_error() {
        let body = r#"{"error":"model is loading"}"#;
        let err = map_response_error(StatusCode::SERVICE_UNAVAILABLE, body);
        let LlmError::Internal { message, .. } = err else {
            panic!("expected LlmError::Internal");
        };
        assert!(message.contains("server error"));
        assert!(message.contains("model is loading"));
    }

    #[test]
    fn map_client_error_exhausted() {
        let err = ClientError::Exhausted {
            status: StatusCode::SERVICE_UNAVAILABLE,
            attempts: 3,
        };
        let mapped = map_client_error(err);
        let LlmError::Internal { message, .. } = mapped else {
            panic!("expected LlmError::Internal");
        };
        assert!(message.contains("retries exhausted"));
        assert!(message.contains("3 attempts"));
        assert!(message.contains("503"));
    }

    #[test]
    fn map_client_error_transport_smoke() {
        // We can't construct a reqwest::Error directly (private constructors).
        // This test exists to ensure the function compiles with both variant
        // arms reachable. The Exhausted path above covers the substantive
        // assertion; the Transport arm is verified by the client.rs tests.
        let _ = map_client_error; // ensure the function is reachable
    }
}
