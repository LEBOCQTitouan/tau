//! Translate OpenAI's `/v1/chat/completions` (non-streaming) JSON
//! response to `tau_ports::CompletionResponse`.
//!
//! See `docs/superpowers/specs/2026-04-29-openai-plugin-design.md`
//! §4.3.

use serde::Deserialize;
use tau_ports::{
    fixtures::make_completion_response, CompletionResponse, StopReason, TokenUsage, ToolUse,
};
use thiserror::Error;

/// Errors raised while parsing the OpenAI `/v1/chat/completions`
/// batch response.
#[derive(Debug, Error)]
pub(crate) enum ParseError {
    /// The response body could not be deserialized as the OpenAI
    /// response shape.
    #[error("could not decode response JSON: {0}")]
    Decode(#[from] serde_json::Error),

    /// A `tool_call.function.arguments` JSON-encoded string could not
    /// be parsed.
    #[error("tool_call {name} arguments not valid JSON: {source}")]
    ToolUseInput {
        /// Name of the tool whose arguments failed to decode.
        name: String,
        /// Source error from `serde_json::from_str`.
        #[source]
        source: serde_json::Error,
    },

    /// `choices` array length was not exactly 1. v0.1 only handles n=1.
    #[error("unexpected choices count: got {got}, expected exactly 1")]
    UnexpectedChoicesCount {
        /// Actual length of the choices array.
        got: usize,
    },
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: Option<OpenAIUsage>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct OpenAIChoice {
    message: OpenAIMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAIMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Deserialize)]
struct OpenAIToolCall {
    #[serde(default)]
    id: Option<String>,
    function: OpenAIToolFn,
}

#[derive(Deserialize)]
struct OpenAIToolFn {
    name: String,
    /// JSON-encoded string. Parse via serde_json::from_str() to
    /// extract the actual tool input.
    arguments: String,
}

#[derive(Deserialize)]
struct OpenAIUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
}

/// Parse the body of an OpenAI `/v1/chat/completions` non-streaming
/// response.
///
/// Errors:
/// - `ParseError::Decode` — body wasn't valid OpenAI JSON.
/// - `ParseError::ToolUseInput { name, source }` — a tool_call's
///   `arguments` string wasn't valid JSON.
/// - `ParseError::UnexpectedChoicesCount { got }` — `choices` had a
///   length other than 1 (v0.1 only handles n=1).
pub(crate) fn parse_chat_completions_response(
    body: &str,
) -> Result<CompletionResponse, ParseError> {
    let parsed: OpenAIChatResponse = serde_json::from_str(body)?;

    if parsed.choices.len() != 1 {
        return Err(ParseError::UnexpectedChoicesCount {
            got: parsed.choices.len(),
        });
    }
    let choice = parsed.choices.into_iter().next().expect("len == 1");

    let text = choice.message.content.unwrap_or_default();

    let tool_uses: Vec<ToolUse> =
        choice
            .message
            .tool_calls
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, tc)| -> Result<ToolUse, ParseError> {
                let id = tc.id.unwrap_or_else(|| format!("openai-tool-{i}"));
                let input: tau_domain::Value = serde_json::from_str(&tc.function.arguments)
                    .map_err(|e| ParseError::ToolUseInput {
                        name: tc.function.name.clone(),
                        source: e,
                    })?;
                Ok(ToolUse::new(id, tc.function.name, input))
            })
            .collect::<Result<Vec<_>, _>>()?;

    let stop_reason = choice
        .finish_reason
        .as_deref()
        .map(map_finish_reason)
        .unwrap_or(StopReason::EndTurn);

    let usage = parsed
        .usage
        .and_then(|u| match (u.prompt_tokens, u.completion_tokens) {
            (Some(p), Some(c)) => Some(TokenUsage::new(p, c)),
            _ => None,
        });

    Ok(make_completion_response(
        text,
        tool_uses,
        stop_reason,
        usage,
    ))
}

/// Map OpenAI's `finish_reason` string to a `StopReason`.
///
/// Per OpenAI:
/// - `"stop"` (model emitted natural end / stop sequence) → `EndTurn`
/// - `"length"` (max tokens hit) → `MaxTokens`
/// - `"tool_calls"` (model emitted tool calls) → `ToolUse`
/// - `"content_filter"` (filtered by safety system) → `Error`
/// - `"function_call"` (deprecated legacy field) → `ToolUse` with warn
/// - any other → `EndTurn` with warn
pub(crate) fn map_finish_reason(s: &str) -> StopReason {
    match s {
        "stop" => StopReason::EndTurn,
        "length" => StopReason::MaxTokens,
        "tool_calls" => StopReason::ToolUse,
        "content_filter" => {
            tracing::warn!(
                target: "openai_plugin::response",
                "content_filter finish_reason; mapping to StopReason::Error",
            );
            StopReason::Error
        }
        "function_call" => {
            tracing::warn!(
                target: "openai_plugin::response",
                "deprecated function_call finish_reason; mapping to StopReason::ToolUse",
            );
            StopReason::ToolUse
        }
        other => {
            tracing::warn!(
                target: "openai_plugin::response",
                finish_reason = other,
                "unknown finish_reason; defaulting to EndTurn",
            );
            StopReason::EndTurn
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_only_response() {
        let body = r#"{
            "id":"chatcmpl-abc","object":"chat.completion","created":1700000000,
            "model":"gpt-4o-mini",
            "choices":[{
                "index":0,
                "message":{"role":"assistant","content":"hello"},
                "finish_reason":"stop"
            }],
            "usage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12}
        }"#;
        let resp = parse_chat_completions_response(body).unwrap();
        assert_eq!(resp.text, "hello");
        assert!(resp.tool_uses.is_empty());
        assert!(matches!(resp.stop_reason, StopReason::EndTurn));
        let u = resp.usage.expect("usage");
        assert_eq!(u.input_tokens, 10);
        assert_eq!(u.output_tokens, 2);
    }

    #[test]
    fn parse_response_with_tool_call_preserves_real_id() {
        let body = r#"{
            "id":"chatcmpl-1","object":"chat.completion","created":1,
            "model":"gpt-4o-mini",
            "choices":[{
                "index":0,
                "message":{
                    "role":"assistant","content":null,
                    "tool_calls":[{"id":"call_abc123","type":"function","function":{"name":"echo","arguments":"{\"text\":\"hi\"}"}}]
                },
                "finish_reason":"tool_calls"
            }],
            "usage":{"prompt_tokens":50,"completion_tokens":10,"total_tokens":60}
        }"#;
        let resp = parse_chat_completions_response(body).unwrap();
        assert_eq!(resp.tool_uses.len(), 1);
        // Real id preserved (NOT synthesized).
        assert_eq!(resp.tool_uses[0].id, "call_abc123");
        assert_eq!(resp.tool_uses[0].name, "echo");
        assert!(matches!(resp.stop_reason, StopReason::ToolUse));
    }

    #[test]
    fn parse_response_with_missing_id_synthesizes_fallback() {
        // Defensive: if OpenAI omits id (rare but seen), synthesize.
        let body = r#"{
            "id":"chatcmpl-1","object":"chat.completion","created":1,
            "model":"gpt-4o-mini",
            "choices":[{
                "index":0,
                "message":{
                    "role":"assistant","content":null,
                    "tool_calls":[{"type":"function","function":{"name":"echo","arguments":"{}"}}]
                },
                "finish_reason":"tool_calls"
            }]
        }"#;
        let resp = parse_chat_completions_response(body).unwrap();
        assert_eq!(resp.tool_uses[0].id, "openai-tool-0");
    }

    #[test]
    fn parse_response_arguments_string_parses_to_value() {
        let body = r#"{
            "id":"x","object":"chat.completion","created":1,"model":"m",
            "choices":[{
                "index":0,
                "message":{
                    "role":"assistant","content":null,
                    "tool_calls":[{"id":"call_1","type":"function","function":{"name":"echo","arguments":"{\"text\":\"hi\",\"n\":3}"}}]
                },
                "finish_reason":"tool_calls"
            }]
        }"#;
        let resp = parse_chat_completions_response(body).unwrap();
        let tau_domain::Value::Object(map) = &resp.tool_uses[0].input else {
            panic!("expected Object input, got {:?}", resp.tool_uses[0].input);
        };
        let text = map.get("text").expect("text key");
        let tau_domain::Value::String(s) = text else {
            panic!("expected String, got {text:?}");
        };
        assert_eq!(s, "hi");
    }

    #[test]
    fn parse_response_finish_reason_length_maps_to_max_tokens() {
        let body = r#"{
            "id":"x","object":"chat.completion","created":1,"model":"m",
            "choices":[{"index":0,"message":{"role":"assistant","content":"trunc"},"finish_reason":"length"}]
        }"#;
        let resp = parse_chat_completions_response(body).unwrap();
        assert!(matches!(resp.stop_reason, StopReason::MaxTokens));
    }

    #[test]
    fn parse_response_finish_reason_unknown_maps_to_end_turn() {
        let body = r#"{
            "id":"x","object":"chat.completion","created":1,"model":"m",
            "choices":[{"index":0,"message":{"role":"assistant","content":""},"finish_reason":"weird-future-reason"}]
        }"#;
        let resp = parse_chat_completions_response(body).unwrap();
        assert!(matches!(resp.stop_reason, StopReason::EndTurn));
    }

    #[test]
    fn parse_response_finish_reason_content_filter_maps_to_error() {
        let body = r#"{
            "id":"x","object":"chat.completion","created":1,"model":"m",
            "choices":[{"index":0,"message":{"role":"assistant","content":""},"finish_reason":"content_filter"}]
        }"#;
        let resp = parse_chat_completions_response(body).unwrap();
        assert!(matches!(resp.stop_reason, StopReason::Error));
    }

    #[test]
    fn parse_response_zero_choices_returns_unexpected_count_error() {
        let body = r#"{"id":"x","object":"chat.completion","created":1,"model":"m","choices":[]}"#;
        let err = parse_chat_completions_response(body).unwrap_err();
        let ParseError::UnexpectedChoicesCount { got } = err else {
            panic!("expected UnexpectedChoicesCount, got {err:?}");
        };
        assert_eq!(got, 0);
    }

    #[test]
    fn parse_response_two_choices_returns_unexpected_count_error() {
        let body = r#"{
            "id":"x","object":"chat.completion","created":1,"model":"m",
            "choices":[
                {"index":0,"message":{"role":"assistant","content":"a"},"finish_reason":"stop"},
                {"index":1,"message":{"role":"assistant","content":"b"},"finish_reason":"stop"}
            ]
        }"#;
        let err = parse_chat_completions_response(body).unwrap_err();
        let ParseError::UnexpectedChoicesCount { got } = err else {
            panic!("expected UnexpectedChoicesCount, got {err:?}");
        };
        assert_eq!(got, 2);
    }
}
