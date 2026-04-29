//! Translate Ollama's `/api/chat` (non-streaming) JSON response to
//! `tau_ports::CompletionResponse`.
//!
//! See `docs/superpowers/specs/2026-04-29-ollama-plugin-design.md`
//! §4.3, §7.2, §7.3.

use serde::Deserialize;
use tau_ports::{
    fixtures::make_completion_response, CompletionResponse, StopReason, TokenUsage, ToolUse,
};
use thiserror::Error;

/// Errors raised while parsing the Ollama `/api/chat` batch response.
#[derive(Debug, Error)]
pub(crate) enum ParseError {
    /// The response body could not be deserialized as the Ollama
    /// response shape.
    #[error("could not decode response JSON: {0}")]
    Decode(#[from] serde_json::Error),

    /// A `tool_call.function.arguments` value could not be reified
    /// from `serde_json::Value` into `tau_domain::Value`.
    #[error("tool_call {name} arguments could not decode: {source}")]
    ToolUseInput {
        /// Name of the tool whose arguments failed to decode.
        name: String,
        /// Source error from `serde_json::from_value`.
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OllamaChatResponse {
    message: OllamaMessage,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    prompt_eval_count: Option<u32>,
    #[serde(default)]
    eval_count: Option<u32>,
}

#[derive(Deserialize)]
struct OllamaMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

#[derive(Deserialize)]
struct OllamaToolCall {
    #[serde(default)]
    id: Option<String>,
    function: OllamaToolFn,
}

#[derive(Deserialize)]
struct OllamaToolFn {
    name: String,
    arguments: serde_json::Value,
}

/// Parse the body of an Ollama `/api/chat` non-streaming response.
///
/// `tool_call.id` is synthesized as `"ollama-tool-{i}"` when Ollama
/// doesn't provide one — required for the kernel's multi-turn loop
/// pairing.
pub(crate) fn parse_chat_response(body: &str) -> Result<CompletionResponse, ParseError> {
    let parsed: OllamaChatResponse = serde_json::from_str(body)?;

    let text = parsed.message.content.unwrap_or_default();

    let tool_uses: Vec<ToolUse> =
        parsed
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, tc)| -> Result<ToolUse, ParseError> {
                let id = tc.id.unwrap_or_else(|| format!("ollama-tool-{i}"));
                let input: tau_domain::Value = serde_json::from_value(tc.function.arguments)
                    .map_err(|e| ParseError::ToolUseInput {
                        name: tc.function.name.clone(),
                        source: e,
                    })?;
                Ok(ToolUse::new(id, tc.function.name, input))
            })
            .collect::<Result<Vec<_>, _>>()?;

    let stop_reason = parsed
        .done_reason
        .as_deref()
        .map(map_done_reason)
        .unwrap_or(StopReason::EndTurn);

    let usage = match (parsed.prompt_eval_count, parsed.eval_count) {
        (Some(input), Some(output)) => Some(TokenUsage::new(input, output)),
        _ => None,
    };

    Ok(make_completion_response(
        text,
        tool_uses,
        stop_reason,
        usage,
    ))
}

/// Map Ollama's `done_reason` string to a `StopReason`.
///
/// Ollama doesn't have a tool-use-specific stop reason — when a model
/// returns `tool_calls`, `done_reason` is still `"stop"`. Caller
/// infers tool-use from non-empty `tool_uses`.
pub(crate) fn map_done_reason(s: &str) -> StopReason {
    match s {
        "stop" => StopReason::EndTurn,
        "length" => StopReason::MaxTokens,
        other => {
            tracing::warn!(
                target: "ollama_plugin::response",
                done_reason = other,
                "unknown done_reason; defaulting to EndTurn",
            );
            StopReason::EndTurn
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_text_only() -> &'static str {
        r#"{
            "model": "llama3.2",
            "created_at": "2026-04-29T00:00:00Z",
            "message": {"role": "assistant", "content": "hello"},
            "done": true,
            "done_reason": "stop"
        }"#
    }

    #[test]
    fn parse_text_only_response() {
        let resp = parse_chat_response(fixture_text_only()).unwrap();
        assert_eq!(resp.text, "hello");
        assert!(resp.tool_uses.is_empty());
        assert!(matches!(resp.stop_reason, StopReason::EndTurn));
        assert!(resp.usage.is_none());
    }

    #[test]
    fn parse_response_with_tool_call_synthesizes_id() {
        let body = r#"{
            "model": "llama3.2",
            "created_at": "2026-04-29T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{"function": {"name": "echo", "arguments": {"text": "hi"}}}]
            },
            "done": true,
            "done_reason": "stop"
        }"#;
        let resp = parse_chat_response(body).unwrap();
        assert_eq!(resp.tool_uses.len(), 1);
        assert_eq!(resp.tool_uses[0].id, "ollama-tool-0");
        assert_eq!(resp.tool_uses[0].name, "echo");
    }

    #[test]
    fn parse_response_with_two_tool_calls_synthesizes_sequential_ids() {
        let body = r#"{
            "model": "llama3.2",
            "created_at": "2026-04-29T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"function": {"name": "echo1", "arguments": {}}},
                    {"function": {"name": "echo2", "arguments": {}}}
                ]
            },
            "done": true,
            "done_reason": "stop"
        }"#;
        let resp = parse_chat_response(body).unwrap();
        assert_eq!(resp.tool_uses.len(), 2);
        assert_eq!(resp.tool_uses[0].id, "ollama-tool-0");
        assert_eq!(resp.tool_uses[1].id, "ollama-tool-1");
    }

    #[test]
    fn parse_response_preserves_provided_tool_call_id() {
        // Defensive: if Ollama starts including ids, we round-trip.
        let body = r#"{
            "model": "llama3.2",
            "created_at": "2026-04-29T00:00:00Z",
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{"id": "real-id-42", "function": {"name": "echo", "arguments": {}}}]
            },
            "done": true,
            "done_reason": "stop"
        }"#;
        let resp = parse_chat_response(body).unwrap();
        assert_eq!(resp.tool_uses[0].id, "real-id-42");
    }

    #[test]
    fn parse_response_maps_done_reason_length_to_max_tokens() {
        let body = r#"{
            "model": "llama3.2",
            "created_at": "2026-04-29T00:00:00Z",
            "message": {"role": "assistant", "content": "trunc"},
            "done": true,
            "done_reason": "length"
        }"#;
        let resp = parse_chat_response(body).unwrap();
        assert!(matches!(resp.stop_reason, StopReason::MaxTokens));
    }

    #[test]
    fn parse_response_maps_unknown_done_reason_to_end_turn() {
        let body = r#"{
            "model": "llama3.2",
            "created_at": "2026-04-29T00:00:00Z",
            "message": {"role": "assistant", "content": ""},
            "done": true,
            "done_reason": "weird-future-reason"
        }"#;
        let resp = parse_chat_response(body).unwrap();
        assert!(matches!(resp.stop_reason, StopReason::EndTurn));
    }

    #[test]
    fn parse_response_with_usage_counts() {
        let body = r#"{
            "model": "llama3.2",
            "created_at": "2026-04-29T00:00:00Z",
            "message": {"role": "assistant", "content": "ok"},
            "done": true,
            "done_reason": "stop",
            "prompt_eval_count": 12,
            "eval_count": 3
        }"#;
        let resp = parse_chat_response(body).unwrap();
        let usage = resp.usage.expect("usage should be Some");
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 3);
    }
}
