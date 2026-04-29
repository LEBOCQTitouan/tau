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
#[allow(dead_code)]
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
}
