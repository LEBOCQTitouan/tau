//! NDJSON streaming parser for Ollama's `/api/chat` with `stream: true`.
//!
//! Ollama emits one JSON object per `\n`-terminated line. The final
//! line has `"done": true` and may include `done_reason`,
//! `prompt_eval_count`, `eval_count`. Hand-rolled to avoid the
//! `eventsource-stream` dep (which is for SSE, not NDJSON).
//!
//! See `docs/superpowers/specs/2026-04-29-ollama-plugin-design.md` §5.

use async_stream::try_stream;
use futures_core::Stream;
use futures_util::StreamExt;
use serde::Deserialize;
use tau_ports::{CompletionChunk, CompletionStream, LlmError, StopReason, TokenUsage, ToolUse};

use crate::response::map_done_reason;

/// Drive `body.bytes_stream()` to completion, yielding
/// `CompletionChunk`s as `\n`-delimited JSON lines arrive.
///
/// On a stream that ends without a `done: true` line, yields a final
/// `LlmError::Stream` so the caller knows the response was truncated.
pub(crate) async fn parse_ndjson(body: reqwest::Response) -> Result<CompletionStream, LlmError> {
    let bytes_stream = body.bytes_stream();
    Ok(Box::pin(stream_from_bytes(bytes_stream)))
}

#[derive(Deserialize)]
struct StreamLine {
    #[serde(default)]
    message: Option<StreamMessage>,
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
struct StreamMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Deserialize)]
struct StreamToolCall {
    #[serde(default)]
    id: Option<String>,
    function: StreamToolFn,
}

#[derive(Deserialize)]
struct StreamToolFn {
    name: String,
    arguments: serde_json::Value,
}

fn stream_from_bytes<S>(
    mut bytes: S,
) -> impl Stream<Item = Result<CompletionChunk, LlmError>> + Send
where
    S: Stream<Item = reqwest::Result<bytes::Bytes>> + Send + Unpin + 'static,
{
    try_stream! {
        let mut buf: Vec<u8> = Vec::new();
        let mut tool_call_index: usize = 0;

        while let Some(chunk_res) = bytes.next().await {
            let chunk: bytes::Bytes = chunk_res.map_err(|e| LlmError::Stream {
                message: format!("ollama stream transport: {e}"),
            })?;
            buf.extend_from_slice(&chunk);

            // Drain complete lines (separated by '\n').
            while let Some(nl_pos) = buf.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buf.drain(..=nl_pos).collect();
                let line_text = std::str::from_utf8(&line_bytes[..nl_pos])
                    .map_err(|e| LlmError::Stream {
                        message: format!("ollama stream UTF-8: {e}"),
                    })?;
                let line = line_text.trim();
                if line.is_empty() {
                    continue;
                }

                let parsed: StreamLine = serde_json::from_str(line)
                    .map_err(|e| LlmError::Stream {
                        message: format!("ollama stream line decode: {e} (raw: {line})"),
                    })?;

                if let Some(msg) = parsed.message.as_ref() {
                    if let Some(text) = msg.content.as_ref() {
                        if !text.is_empty() {
                            yield CompletionChunk::Text { delta: text.clone() };
                        }
                    }
                    if let Some(calls) = msg.tool_calls.as_ref() {
                        for call in calls {
                            let id = call.id.clone().unwrap_or_else(|| {
                                format!("ollama-tool-{tool_call_index}")
                            });
                            tool_call_index += 1;
                            let input: tau_domain::Value =
                                serde_json::from_value(call.function.arguments.clone())
                                    .map_err(|e| LlmError::Stream {
                                        message: format!(
                                            "ollama stream tool_use input decode: {e}"
                                        ),
                                    })?;
                            yield CompletionChunk::ToolUse(ToolUse::new(
                                id,
                                call.function.name.clone(),
                                input,
                            ));
                        }
                    }
                }

                if parsed.done {
                    let stop_reason = parsed
                        .done_reason
                        .as_deref()
                        .map(map_done_reason)
                        .unwrap_or(StopReason::EndTurn);
                    let usage = match (parsed.prompt_eval_count, parsed.eval_count) {
                        (Some(i), Some(o)) => Some(TokenUsage::new(i, o)),
                        _ => None,
                    };
                    yield CompletionChunk::Finish { stop_reason, usage };
                    return;
                }
            }
        }

        // Stream ended without a `done: true` line — defensive.
        Err(LlmError::Stream {
            message: "ollama stream ended before done:true line".into(),
        })?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::StreamExt;
    use std::pin::Pin;

    fn bytes_stream_from_lines(
        chunks: Vec<&'static str>,
    ) -> Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>> {
        // Convert each input chunk into a Bytes; emit them in order.
        let items: Vec<reqwest::Result<Bytes>> = chunks
            .into_iter()
            .map(|s| Ok::<Bytes, reqwest::Error>(Bytes::from_static(s.as_bytes())))
            .collect();
        Box::pin(futures_util::stream::iter(items))
    }

    async fn drain(
        mut s: impl Stream<Item = Result<CompletionChunk, LlmError>> + Send + Unpin,
    ) -> Vec<Result<CompletionChunk, LlmError>> {
        let mut out = Vec::new();
        while let Some(item) = s.next().await {
            out.push(item);
        }
        out
    }

    #[tokio::test]
    async fn stream_text_only_yields_chunks_then_finish() {
        let body = bytes_stream_from_lines(vec![
            "{\"model\":\"llama3.2\",\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"done\":false}\n",
            "{\"model\":\"llama3.2\",\"message\":{\"role\":\"assistant\",\"content\":\" world\"},\"done\":false}\n",
            "{\"model\":\"llama3.2\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":5,\"eval_count\":2}\n",
        ]);
        let stream = stream_from_bytes(body);
        let chunks = drain(Box::pin(stream)).await;
        assert_eq!(chunks.len(), 3);
        let CompletionChunk::Text { delta } = chunks[0].as_ref().unwrap() else {
            panic!("expected Text");
        };
        assert_eq!(delta, "Hello");
        let CompletionChunk::Text { delta } = chunks[1].as_ref().unwrap() else {
            panic!("expected Text");
        };
        assert_eq!(delta, " world");
        let CompletionChunk::Finish { stop_reason, usage } = chunks[2].as_ref().unwrap() else {
            panic!("expected Finish");
        };
        assert!(matches!(stop_reason, StopReason::EndTurn));
        let u = usage.as_ref().expect("usage");
        assert_eq!(u.input_tokens, 5);
        assert_eq!(u.output_tokens, 2);
    }

    #[tokio::test]
    async fn stream_skips_empty_lines() {
        let body = bytes_stream_from_lines(vec![
            "\n\n",
            "{\"message\":{\"content\":\"hi\"},\"done\":false}\n",
            "\n",
            "{\"message\":{\"content\":\"\"},\"done\":true}\n",
        ]);
        let stream = stream_from_bytes(body);
        let chunks = drain(Box::pin(stream)).await;
        assert_eq!(chunks.len(), 2); // Text + Finish, no spurious entries
    }

    #[tokio::test]
    async fn stream_tool_use_synthesizes_id_when_absent() {
        let body = bytes_stream_from_lines(vec![
            "{\"message\":{\"content\":\"\",\"tool_calls\":[{\"function\":{\"name\":\"echo\",\"arguments\":{\"text\":\"hi\"}}}]},\"done\":false}\n",
            "{\"message\":{\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\"}\n",
        ]);
        let stream = stream_from_bytes(body);
        let chunks = drain(Box::pin(stream)).await;
        assert_eq!(chunks.len(), 2);
        let CompletionChunk::ToolUse(tu) = chunks[0].as_ref().unwrap() else {
            panic!("expected ToolUse");
        };
        assert_eq!(tu.id, "ollama-tool-0");
        assert_eq!(tu.name, "echo");
    }

    #[tokio::test]
    async fn stream_two_tool_calls_synthesize_sequential_ids() {
        // Two tool_calls on a single line; ids should be 0 and 1.
        let body = bytes_stream_from_lines(vec![
            "{\"message\":{\"content\":\"\",\"tool_calls\":[{\"function\":{\"name\":\"a\",\"arguments\":{}}},{\"function\":{\"name\":\"b\",\"arguments\":{}}}]},\"done\":false}\n",
            "{\"message\":{\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\"}\n",
        ]);
        let stream = stream_from_bytes(body);
        let chunks = drain(Box::pin(stream)).await;
        assert_eq!(chunks.len(), 3); // ToolUse, ToolUse, Finish
        let CompletionChunk::ToolUse(tu0) = chunks[0].as_ref().unwrap() else {
            panic!("expected ToolUse");
        };
        let CompletionChunk::ToolUse(tu1) = chunks[1].as_ref().unwrap() else {
            panic!("expected ToolUse");
        };
        assert_eq!(tu0.id, "ollama-tool-0");
        assert_eq!(tu1.id, "ollama-tool-1");
    }

    #[tokio::test]
    async fn stream_truncated_yields_stream_error_at_end() {
        // No `done:true` line — body just stops.
        let body = bytes_stream_from_lines(vec![
            "{\"message\":{\"content\":\"partial\"},\"done\":false}\n",
        ]);
        let stream = stream_from_bytes(body);
        let chunks = drain(Box::pin(stream)).await;
        assert_eq!(chunks.len(), 2); // Text + Stream error
        let last = chunks.last().unwrap();
        assert!(last.is_err());
        if let Err(LlmError::Stream { message, .. }) = last {
            assert!(
                message.contains("ended before done:true"),
                "expected truncated-stream message; got: {message}"
            );
        } else {
            panic!("expected LlmError::Stream");
        }
    }

    #[tokio::test]
    async fn stream_chunked_lines_assembled_across_byte_boundaries() {
        // Feed half a line, then the rest, then the done.
        let body = bytes_stream_from_lines(vec![
            "{\"message\":{\"content\":\"hel",
            "lo\"},\"done\":false}\n",
            "{\"message\":{\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\"}\n",
        ]);
        let stream = stream_from_bytes(body);
        let chunks = drain(Box::pin(stream)).await;
        assert_eq!(chunks.len(), 2); // Text("hello") + Finish
        let CompletionChunk::Text { delta } = chunks[0].as_ref().unwrap() else {
            panic!("expected Text");
        };
        assert_eq!(delta, "hello");
    }
}
