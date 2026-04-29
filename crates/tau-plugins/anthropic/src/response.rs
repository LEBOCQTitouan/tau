//! Parse Anthropic Messages API responses into tau-ports
//! `CompletionResponse`.
//!
//! Per spec §4.3 / §7.3:
//! - Walk `content[*]` array; concatenate `text` blocks into
//!   `CompletionResponse::text`; collect `tool_use` blocks into
//!   `CompletionResponse::tool_uses`. Unknown block types are dropped
//!   with a `tracing::warn!`.
//! - Map `stop_reason` per the table; unknown maps to
//!   `StopReason::EndTurn` with a warn (NOT `Error` — that signals
//!   mid-stream error in tau-ports).
//! - `usage` wraps in `Some(...)`; defensive: missing field → None.
//!
//! `CompletionResponse` is `#[non_exhaustive]` with neither a public
//! `::new()` constructor nor `Default` impl. We construct it via
//! `tau_ports::fixtures::make_completion_response`, which is exposed
//! under the `test-fixtures` feature enabled by this crate's
//! `Cargo.toml` (see `crates/tau-ports/src/fixtures.rs`'s
//! "Construction helpers for `#[non_exhaustive]` types" section). The
//! helper is the in-tree convention for building canonical
//! `CompletionResponse` values from outside `tau-ports`.

use serde::Deserialize;
use tau_ports::fixtures::make_completion_response;
use tau_ports::{CompletionResponse, StopReason, TokenUsage, ToolUse};
use thiserror::Error;

/// Errors produced while parsing an Anthropic response. Plugin-internal;
/// converted to [`tau_ports::LlmError::Internal`] in `plugin.rs`.
#[non_exhaustive]
#[derive(Debug, Error)]
pub(crate) enum ParseError {
    /// JSON deserialization failed (malformed body).
    #[error("response decode: {0}")]
    Json(#[from] serde_json::Error),

    /// A `tool_use.input` JSON object failed to convert to
    /// `tau_domain::Value`.
    #[error("tool_use input decode (name={name}): {source}")]
    ToolUseInput {
        /// Name of the tool whose input failed to decode.
        name: String,
        /// Underlying serde error.
        source: serde_json::Error,
    },
}

/// Anthropic's response envelope. Private; we expose only
/// `CompletionResponse` upstream.
#[derive(Debug, Deserialize)]
struct AnthropicMessagesResponse {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    r#type: String,
    #[allow(dead_code)]
    role: String,
    #[allow(dead_code)]
    model: String,
    content: Vec<AnthropicContentBlock>,
    stop_reason: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    stop_sequence: Option<String>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

/// Parse the JSON body of a successful `POST /v1/messages` response into
/// a tau-ports [`CompletionResponse`].
pub(crate) fn parse_messages_response(body: &str) -> Result<CompletionResponse, ParseError> {
    let parsed: AnthropicMessagesResponse = serde_json::from_str(body)?;

    let mut text = String::new();
    let mut tool_uses: Vec<ToolUse> = Vec::new();
    for block in parsed.content {
        match block {
            AnthropicContentBlock::Text { text: t } => {
                text.push_str(&t);
            }
            AnthropicContentBlock::ToolUse { id, name, input } => {
                let value: tau_domain::Value =
                    serde_json::from_value(input).map_err(|e| ParseError::ToolUseInput {
                        name: name.clone(),
                        source: e,
                    })?;
                tool_uses.push(ToolUse::new(id, name, value));
            }
            AnthropicContentBlock::Unknown => {
                tracing::warn!(
                    target: "anthropic_plugin::response",
                    "dropped unknown content block type — plugin needs upgrade for new Anthropic features"
                );
            }
        }
    }

    let stop_reason = parsed
        .stop_reason
        .as_deref()
        .map(map_stop_reason)
        .unwrap_or(StopReason::EndTurn);

    let usage = parsed
        .usage
        .map(|u| TokenUsage::new(u.input_tokens, u.output_tokens));

    Ok(make_completion_response(
        text,
        tool_uses,
        stop_reason,
        usage,
    ))
}

fn map_stop_reason(s: &str) -> StopReason {
    match s {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        other => {
            tracing::warn!(
                target: "anthropic_plugin::response",
                stop_reason = other,
                "unknown stop_reason; defaulting to EndTurn"
            );
            StopReason::EndTurn
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_only_response() {
        let body = r#"{
            "id": "msg_01",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-5-haiku-latest",
            "content": [{"type": "text", "text": "Hello world"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 12, "output_tokens": 3}
        }"#;
        let resp = parse_messages_response(body).unwrap();
        assert_eq!(resp.text, "Hello world");
        assert!(resp.tool_uses.is_empty());
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        let usage = resp.usage.expect("usage should be Some");
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 3);
    }

    #[test]
    fn parses_tool_use_response() {
        let body = r#"{
            "id": "msg_01",
            "type": "message",
            "role": "assistant",
            "model": "claude-3-5-haiku-latest",
            "content": [
                {"type": "text", "text": "Looking up..."},
                {"type": "tool_use", "id": "toolu_01", "name": "get_weather",
                 "input": {"location": "Paris"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 50, "output_tokens": 10}
        }"#;
        let resp = parse_messages_response(body).unwrap();
        assert_eq!(resp.text, "Looking up...");
        assert_eq!(resp.tool_uses.len(), 1);
        assert_eq!(resp.tool_uses[0].id, "toolu_01");
        assert_eq!(resp.tool_uses[0].name, "get_weather");
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
    }

    #[test]
    fn parses_multiple_text_blocks_concatenated() {
        let body = r#"{
            "id": "m", "type": "message", "role": "assistant",
            "model": "m",
            "content": [
                {"type": "text", "text": "Hello "},
                {"type": "text", "text": "world"}
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 2}
        }"#;
        let resp = parse_messages_response(body).unwrap();
        assert_eq!(resp.text, "Hello world");
    }

    #[test]
    fn maps_unknown_stop_reason_to_end_turn() {
        let body = r#"{
            "id": "m", "type": "message", "role": "assistant",
            "model": "m",
            "content": [{"type": "text", "text": "hi"}],
            "stop_reason": "frobnicated",
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }"#;
        let resp = parse_messages_response(body).unwrap();
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn drops_unknown_block_type_silently() {
        let body = r#"{
            "id": "m", "type": "message", "role": "assistant",
            "model": "m",
            "content": [
                {"type": "text", "text": "hi"},
                {"type": "image_url", "url": "https://example.com/x.png"}
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }"#;
        let resp = parse_messages_response(body).unwrap();
        assert_eq!(resp.text, "hi");
        assert!(resp.tool_uses.is_empty());
    }

    #[test]
    fn parses_tool_use_input_as_value() {
        let body = r#"{
            "id": "m", "type": "message", "role": "assistant",
            "model": "m",
            "content": [
                {"type": "tool_use", "id": "tu_01", "name": "search",
                 "input": {"query": "rust", "limit": 10}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }"#;
        let resp = parse_messages_response(body).unwrap();
        assert_eq!(resp.tool_uses.len(), 1);
        let tu = &resp.tool_uses[0];
        // Verify the `input` round-trips as a tau_domain::Value.
        let tau_domain::Value::Object(map) = &tu.input else {
            panic!("expected Object, got {:?}", tu.input);
        };
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn missing_usage_field_yields_none() {
        let body = r#"{
            "id": "m", "type": "message", "role": "assistant",
            "model": "m",
            "content": [{"type": "text", "text": "hi"}],
            "stop_reason": "end_turn"
        }"#;
        let resp = parse_messages_response(body).unwrap();
        assert!(resp.usage.is_none());
    }
}
