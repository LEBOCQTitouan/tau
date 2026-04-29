//! Parse Anthropic Server-Sent Events into a `tau_ports::CompletionStream`.
//!
//! Per spec §5:
//! - text_delta events yield `CompletionChunk::Text { delta }` immediately.
//! - tool_use blocks accumulate via `ToolUseAccumulator`; emit
//!   `CompletionChunk::ToolUse(ToolUse)` once on content_block_stop.
//! - message_stop emits the terminal `CompletionChunk::Finish`.
//! - event:error mid-stream yields `LlmError::Stream` and terminates.
//! - ping events are heartbeats, ignored.
//!
//! Mid-stream errors do NOT retry (spec §5.3): the retry layer in
//! `client.rs` only retries before bytes are consumed.

use std::collections::HashMap;

use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde::Deserialize;
use tau_ports::{
    CompletionChunk, CompletionStream, LlmError, StopReason, TokenUsage, ToolUse,
    ToolUseAccumulator,
};

/// Per-block state during streaming. Indexed by Anthropic's
/// `content_block_*` event `index` field.
enum BlockState {
    Text(String),
    ToolUse(ToolUseAccumulator),
}

/// Top-level discriminated union for Anthropic SSE event payloads.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageStartPayload },

    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: u64,
        content_block: ContentBlockStart,
    },

    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: u64, delta: Delta },

    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u64 },

    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDeltaInner,
        #[serde(default)]
        usage: Option<MessageDeltaUsage>,
    },

    #[serde(rename = "message_stop")]
    MessageStop,

    #[serde(rename = "ping")]
    Ping,

    #[serde(rename = "error")]
    Error { error: AnthropicErrorDetail },

    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct MessageStartPayload {
    #[allow(dead_code)]
    id: String,
    #[allow(dead_code)]
    model: String,
    usage: MessageStartUsage,
}

#[derive(Debug, Deserialize)]
struct MessageStartUsage {
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlockStart {
    #[serde(rename = "text")]
    Text {
        #[allow(dead_code)]
        text: String,
    },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        #[allow(dead_code)]
        input: serde_json::Value,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::enum_variant_names)] // names mirror Anthropic's wire format.
enum Delta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct MessageDeltaInner {
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    stop_sequence: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageDeltaUsage {
    #[serde(default)]
    output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct AnthropicErrorDetail {
    r#type: String,
    message: String,
}

/// Parse Anthropic's SSE response body into a `CompletionStream`.
pub(crate) async fn parse_sse(body: reqwest::Response) -> Result<CompletionStream, LlmError> {
    let bytes = body.bytes_stream();
    let events = bytes.eventsource();
    Ok(Box::pin(stream_from_events(events)))
}

fn stream_from_events<S, E>(
    mut events: S,
) -> impl futures_core::Stream<Item = Result<CompletionChunk, LlmError>> + Send
where
    S: futures_core::Stream<
            Item = Result<eventsource_stream::Event, eventsource_stream::EventStreamError<E>>,
        > + Send
        + Unpin
        + 'static,
    E: std::fmt::Display + Send + 'static,
{
    async_stream::try_stream! {
        let mut blocks: HashMap<u64, BlockState> = HashMap::new();
        let mut final_stop: Option<StopReason> = None;
        let mut input_tokens: Option<u32> = None;
        let mut output_tokens: Option<u32> = None;

        while let Some(event_res) = events.next().await {
            let event = event_res.map_err(|e| LlmError::Stream {
                message: format!("sse transport: {e}"),
            })?;

            // Skip eventsource-stream's own keepalive comments / empty events.
            if event.data.is_empty() {
                continue;
            }

            // Decode the event data field as a typed AnthropicEvent.
            let payload: AnthropicEvent = match serde_json::from_str(&event.data) {
                Ok(p) => p,
                Err(e) => {
                    Err(LlmError::Stream {
                        message: format!("event decode: {e} (raw: {})", event.data),
                    })?;
                    return;
                }
            };

            match payload {
                AnthropicEvent::MessageStart { message } => {
                    input_tokens = Some(message.usage.input_tokens);
                    output_tokens = Some(message.usage.output_tokens);
                }
                AnthropicEvent::ContentBlockStart { index, content_block } => {
                    let state = match content_block {
                        ContentBlockStart::Text { .. } => BlockState::Text(String::new()),
                        ContentBlockStart::ToolUse { id, name, .. } => {
                            BlockState::ToolUse(ToolUseAccumulator::new(id, name))
                        }
                        ContentBlockStart::Unknown => {
                            tracing::warn!(
                                target: "anthropic_plugin::stream",
                                index, "unknown content_block_start type — ignoring",
                            );
                            continue;
                        }
                    };
                    blocks.insert(index, state);
                }
                AnthropicEvent::ContentBlockDelta { index, delta } => {
                    let block = match blocks.get_mut(&index) {
                        Some(b) => b,
                        None => {
                            // Anthropic shouldn't emit deltas for unknown blocks;
                            // log and continue rather than terminate.
                            tracing::warn!(
                                target: "anthropic_plugin::stream",
                                index, "content_block_delta for unknown block index — ignoring",
                            );
                            continue;
                        }
                    };
                    match (block, delta) {
                        (BlockState::Text(buf), Delta::TextDelta { text }) => {
                            buf.push_str(&text);
                            yield CompletionChunk::Text { delta: text };
                        }
                        (BlockState::ToolUse(acc), Delta::InputJsonDelta { partial_json }) => {
                            acc.append(&partial_json);
                        }
                        (_, Delta::Unknown) => {
                            tracing::warn!(
                                target: "anthropic_plugin::stream",
                                index, "unknown delta type — ignoring",
                            );
                        }
                        (_, _) => {
                            Err(LlmError::Stream {
                                message: format!(
                                    "delta/block kind mismatch at index {index}",
                                ),
                            })?;
                            return;
                        }
                    }
                }
                AnthropicEvent::ContentBlockStop { index } => {
                    // Text blocks: nothing to emit on stop; deltas already streamed.
                    if let Some(BlockState::ToolUse(acc)) = blocks.remove(&index) {
                        // ToolUseAccumulator::finalize_with already wraps
                        // parse failures in LlmError::Stream, so just `?`.
                        let tool_use: ToolUse = acc.finalize_with(|s| {
                            serde_json::from_str::<tau_domain::Value>(s)
                                .map_err(|e| e.to_string())
                        })?;
                        yield CompletionChunk::ToolUse(tool_use);
                    }
                }
                AnthropicEvent::MessageDelta { delta, usage } => {
                    if let Some(s) = delta.stop_reason.as_deref() {
                        final_stop = Some(map_stop_reason(s));
                    }
                    let _ = delta.stop_sequence; // accepted but unused at v0.1
                    if let Some(u) = usage {
                        output_tokens = Some(u.output_tokens);
                    }
                }
                AnthropicEvent::MessageStop => {
                    let usage = match (input_tokens, output_tokens) {
                        (Some(i), Some(o)) => Some(TokenUsage::new(i, o)),
                        _ => None,
                    };
                    yield CompletionChunk::Finish {
                        stop_reason: final_stop.unwrap_or(StopReason::EndTurn),
                        usage,
                    };
                    return;
                }
                AnthropicEvent::Ping => {
                    // Heartbeat — ignore.
                }
                AnthropicEvent::Error { error } => {
                    Err(LlmError::Stream {
                        message: format!(
                            "anthropic stream error ({}): {}",
                            error.r#type, error.message,
                        ),
                    })?;
                    return;
                }
                AnthropicEvent::Unknown => {
                    tracing::warn!(
                        target: "anthropic_plugin::stream",
                        raw = %event.data,
                        "unknown SSE event type — ignoring",
                    );
                }
            }
        }
    }
}

fn map_stop_reason(s: &str) -> StopReason {
    match s {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        other => {
            tracing::warn!(
                target: "anthropic_plugin::stream",
                stop_reason = other,
                "unknown stop_reason; defaulting to EndTurn",
            );
            StopReason::EndTurn
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a one-shot byte stream from a single SSE-formatted body.
    /// `eventsource-stream` accepts any `B: AsRef<[u8]>` chunk; we feed
    /// the whole body in one chunk via `stream::iter` (Unpin-friendly).
    fn bytes_stream(
        raw: &str,
    ) -> impl futures_core::Stream<Item = Result<Vec<u8>, std::io::Error>> + Send + Unpin {
        let raw = raw.as_bytes().to_vec();
        futures_util::stream::iter(std::iter::once(Ok(raw)))
    }

    async fn drive(raw: &str) -> Vec<Result<CompletionChunk, LlmError>> {
        let bs = bytes_stream(raw);
        let events = bs.eventsource();
        let stream = stream_from_events(events);
        let mut chunks = Vec::new();
        let mut s = Box::pin(stream);
        while let Some(item) = s.next().await {
            chunks.push(item);
        }
        chunks
    }

    fn finish_chunk(c: &CompletionChunk) -> Option<(StopReason, Option<TokenUsage>)> {
        if let CompletionChunk::Finish { stop_reason, usage } = c {
            Some((*stop_reason, *usage))
        } else {
            None
        }
    }

    #[tokio::test]
    async fn parses_text_only_stream() {
        let raw = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"m\",\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":3}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";
        let chunks = drive(raw).await;
        assert_eq!(chunks.len(), 3, "got: {chunks:?}");
        // First two are text deltas
        let CompletionChunk::Text { delta } = chunks[0].as_ref().unwrap() else {
            panic!("expected Text, got {:?}", chunks[0]);
        };
        assert_eq!(delta, "Hello");
        let CompletionChunk::Text { delta } = chunks[1].as_ref().unwrap() else {
            panic!("expected Text, got {:?}", chunks[1]);
        };
        assert_eq!(delta, " world");
        // Final is Finish.
        let (stop, usage) = finish_chunk(chunks[2].as_ref().unwrap()).unwrap();
        assert_eq!(stop, StopReason::EndTurn);
        let u = usage.expect("usage should be Some");
        assert_eq!(u.input_tokens, 10);
        assert_eq!(u.output_tokens, 3);
    }

    #[tokio::test]
    async fn accumulates_tool_use_input_json() {
        let raw = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"m\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_01\",\"name\":\"echo\",\"input\":{}}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"q\\\":\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"hi\\\"}\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":2}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";
        let chunks = drive(raw).await;
        assert_eq!(chunks.len(), 2, "got {chunks:?}");
        let CompletionChunk::ToolUse(tu) = chunks[0].as_ref().unwrap() else {
            panic!("got {:?}", chunks[0]);
        };
        assert_eq!(tu.id, "tu_01");
        assert_eq!(tu.name, "echo");
        // Verify input parsed as Value::Object with "q":"hi"
        let tau_domain::Value::Object(map) = &tu.input else {
            panic!("expected Object, got {:?}", tu.input);
        };
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("q"), Some(&tau_domain::Value::String("hi".into())),);
        // Final Finish.
        let (stop, _) = finish_chunk(chunks[1].as_ref().unwrap()).unwrap();
        assert_eq!(stop, StopReason::ToolUse);
    }

    #[tokio::test]
    async fn propagates_mid_stream_error_event() {
        let raw = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"m\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\
\n\
event: error\n\
data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Service overloaded\"}}\n\
\n";
        let chunks = drive(raw).await;
        // 1 Text + 1 Err
        assert_eq!(chunks.len(), 2, "got {chunks:?}");
        assert!(matches!(&chunks[0], Ok(CompletionChunk::Text { .. })));
        let Err(LlmError::Stream { message }) = &chunks[1] else {
            panic!("expected Stream error; got {:?}", chunks[1]);
        };
        assert!(message.contains("overloaded_error"), "msg: {message}");
        assert!(message.contains("Service overloaded"), "msg: {message}");
    }

    #[tokio::test]
    async fn ignores_ping_events() {
        let raw = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"m\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\
\n\
event: ping\n\
data: {\"type\":\"ping\"}\n\
\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\
\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\
\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\
\n\
event: ping\n\
data: {\"type\":\"ping\"}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";
        let chunks = drive(raw).await;
        // 1 Text + 1 Finish (pings invisible).
        assert_eq!(chunks.len(), 2, "got {chunks:?}");
        assert!(matches!(&chunks[0], Ok(CompletionChunk::Text { .. })));
        assert!(matches!(&chunks[1], Ok(CompletionChunk::Finish { .. })));
    }

    #[tokio::test]
    async fn tracks_usage_across_message_start_and_message_delta() {
        let raw = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"m\",\"usage\":{\"input_tokens\":50,\"output_tokens\":1}}}\n\
\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":12}}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";
        let chunks = drive(raw).await;
        assert_eq!(chunks.len(), 1, "got {chunks:?}");
        let (_, usage) = finish_chunk(chunks[0].as_ref().unwrap()).unwrap();
        let u = usage.unwrap();
        assert_eq!(u.input_tokens, 50);
        assert_eq!(
            u.output_tokens, 12,
            "message_delta should override message_start output_tokens",
        );
    }

    #[tokio::test]
    async fn unknown_event_kind_logs_warn_and_continues() {
        let raw = "\
event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"m\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\
\n\
event: future_event\n\
data: {\"type\":\"future_event\",\"foo\":\"bar\"}\n\
\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\
\n";
        let chunks = drive(raw).await;
        // Unknown event ignored; only Finish emitted.
        assert_eq!(chunks.len(), 1, "got {chunks:?}");
        assert!(matches!(&chunks[0], Ok(CompletionChunk::Finish { .. })));
    }
}
