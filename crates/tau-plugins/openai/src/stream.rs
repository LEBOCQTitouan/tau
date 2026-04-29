//! Parse OpenAI SSE event stream into a `tau_ports::CompletionStream`.
//!
//! Per spec §5:
//! - `data: <json>` events parse as `StreamEvent`; non-empty
//!   `delta.content` yields `CompletionChunk::Text { delta }`.
//! - Tool-call deltas accumulate via per-index
//!   `tau_ports::ToolUseAccumulator`. When the finish-reason event
//!   arrives, each accumulator is finalized in index order and its
//!   `CompletionChunk::ToolUse(_)` is emitted before the terminal
//!   `CompletionChunk::Finish { stop_reason, usage }`.
//! - The `data: [DONE]` sentinel is consumed silently after the
//!   finish-reason event.
//! - Truncated stream (no `finish_reason` event before the body ends
//!   OR a `[DONE]` arrives without prior `finish_reason`) → final
//!   yield is `Err(LlmError::Stream { message: "openai stream ended
//!   without finish_reason" })`.
//!
//! Mid-stream errors do NOT retry (matches Anthropic stream §5
//! convention): the retry layer in `client.rs` only retries before
//! bytes are consumed.

use std::collections::BTreeMap;

use async_stream::try_stream;
use eventsource_stream::Eventsource;
use futures_core::Stream;
use futures_util::StreamExt;
use serde::Deserialize;
use tau_ports::{
    CompletionChunk, CompletionStream, LlmError, TokenUsage, ToolUse, ToolUseAccumulator,
};

use crate::response::map_finish_reason;

/// Drive `body.bytes_stream()` through `eventsource-stream` to
/// completion, yielding `CompletionChunk`s as deltas arrive.
#[allow(dead_code)]
pub(crate) async fn parse_sse(body: reqwest::Response) -> Result<CompletionStream, LlmError> {
    let bytes_stream = body.bytes_stream();
    Ok(Box::pin(stream_events(bytes_stream)))
}

/// Outer driver. Owns the eventsource_stream::Eventsource adapter and
/// the per-index `ToolUseAccumulator` map; yields
/// `Result<CompletionChunk, LlmError>` items.
fn stream_events<S>(bytes: S) -> impl Stream<Item = Result<CompletionChunk, LlmError>> + Send
where
    S: Stream<Item = reqwest::Result<bytes::Bytes>> + Send + Unpin + 'static + Eventsource,
{
    try_stream! {
        let mut events = bytes.eventsource();

        // Accumulators keyed by OpenAI tool_calls[].index. Names captured
        // from the first delta carrying `function.name` for that index.
        let mut accumulators: BTreeMap<u32, ToolUseAccumulator> = BTreeMap::new();

        let mut saw_finish = false;

        while let Some(ev_res) = events.next().await {
            let ev = ev_res.map_err(|e| LlmError::Stream {
                message: format!("openai stream transport: {e}"),
            })?;

            // OpenAI sends `data: [DONE]` to mark stream end.
            if ev.data.trim() == "[DONE]" {
                if !saw_finish {
                    Err(LlmError::Stream {
                        message: "openai stream ended without finish_reason".into(),
                    })?;
                }
                return;
            }

            // Skip empty `data:` lines (heartbeats, comments).
            if ev.data.trim().is_empty() {
                continue;
            }

            let parsed: StreamEvent = serde_json::from_str(&ev.data)
                .map_err(|e| LlmError::Stream {
                    message: format!("openai stream event decode: {e} (raw: {})", ev.data),
                })?;

            // OpenAI puts everything in `choices[0]`. v0.1 only handles
            // n=1; tolerate empty choices defensively.
            let Some(choice) = parsed.choices.into_iter().next() else {
                continue;
            };

            // Yield text delta if non-empty.
            if let Some(content) = choice.delta.content.as_ref() {
                if !content.is_empty() {
                    yield CompletionChunk::Text { delta: content.clone() };
                }
            }

            // Accumulate tool-call deltas. The first delta for each
            // index carries id+name; subsequent deltas only carry
            // `function.arguments` fragments.
            if let Some(calls) = choice.delta.tool_calls.as_ref() {
                for call in calls {
                    let acc = accumulators.entry(call.index).or_insert_with(|| {
                        let id = call
                            .id
                            .clone()
                            .unwrap_or_else(|| format!("openai-tool-{}", call.index));
                        let name = call
                            .function
                            .as_ref()
                            .and_then(|f| f.name.clone())
                            .unwrap_or_default();
                        ToolUseAccumulator::new(id, name)
                    });
                    if let Some(f) = call.function.as_ref() {
                        if let Some(arguments) = f.arguments.as_deref() {
                            acc.append(arguments);
                        }
                    }
                }
            }

            // finish_reason event terminates the stream. Drain
            // accumulators (in index order — BTreeMap iterates ordered)
            // and emit ToolUse chunks, then emit Finish.
            if let Some(fr) = choice.finish_reason.as_deref() {
                let stop_reason = map_finish_reason(fr);
                let usage = parsed.usage.and_then(|u| {
                    match (u.prompt_tokens, u.completion_tokens) {
                        (Some(p), Some(c)) => Some(TokenUsage::new(p, c)),
                        _ => None,
                    }
                });

                let drained: Vec<ToolUseAccumulator> =
                    std::mem::take(&mut accumulators).into_values().collect();
                for acc in drained {
                    let tool_use: ToolUse = acc.finalize_with(|s| {
                        serde_json::from_str::<tau_domain::Value>(s)
                            .map_err(|e| e.to_string())
                    })?;
                    yield CompletionChunk::ToolUse(tool_use);
                }

                yield CompletionChunk::Finish { stop_reason, usage };
                saw_finish = true;
                // Don't return yet — wait for the [DONE] sentinel to
                // arrive on the next event-loop iteration. If the
                // stream ends without [DONE], that's fine; we already
                // emitted Finish.
            }
        }

        // Stream ended (no more events). If we saw finish_reason but
        // not [DONE], that's acceptable — the Finish chunk was already
        // emitted. If we never saw finish_reason, report the truncation.
        if !saw_finish {
            Err(LlmError::Stream {
                message: "openai stream ended without finish_reason".into(),
            })?;
        }
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct StreamEvent {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<StreamUsage>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCallDelta>>,
}

#[derive(Deserialize)]
struct StreamToolCallDelta {
    /// Index of the tool call. OpenAI guarantees a stable index per
    /// tool call across deltas; the id+name only appear on the first
    /// delta for that index.
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamToolFnDelta>,
}

#[derive(Deserialize)]
struct StreamToolFnDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct StreamUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::StreamExt;
    use std::pin::Pin;
    use tau_ports::StopReason;

    fn bytes_stream_from_chunks(
        chunks: Vec<&'static str>,
    ) -> Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>> {
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
        let body = bytes_stream_from_chunks(vec![
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\" there\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n",
        ]);
        let chunks = drain(Box::pin(stream_events(body))).await;
        assert_eq!(chunks.len(), 3, "got: {chunks:?}");
        let CompletionChunk::Text { delta } = chunks[0].as_ref().unwrap() else {
            panic!("expected Text");
        };
        assert_eq!(delta, "Hi");
        let CompletionChunk::Text { delta } = chunks[1].as_ref().unwrap() else {
            panic!("expected Text");
        };
        assert_eq!(delta, " there");
        let CompletionChunk::Finish { stop_reason, usage } = chunks[2].as_ref().unwrap() else {
            panic!("expected Finish");
        };
        assert!(matches!(stop_reason, StopReason::EndTurn));
        let u = usage.as_ref().expect("usage");
        assert_eq!(u.input_tokens, 5);
        assert_eq!(u.output_tokens, 2);
    }

    #[tokio::test]
    async fn stream_with_tool_use_accumulator_emits_one_tool_use() {
        // First delta has id+name; subsequent deltas just append args.
        let body = bytes_stream_from_chunks(vec![
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_abc\",\"type\":\"function\",\"function\":{\"name\":\"echo\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"te\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"xt\\\":\\\"hi\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ]);
        let chunks = drain(Box::pin(stream_events(body))).await;
        // Expected: 1 ToolUse + 1 Finish.
        assert_eq!(chunks.len(), 2, "got: {chunks:?}");
        let CompletionChunk::ToolUse(tu) = chunks[0].as_ref().unwrap() else {
            panic!("expected ToolUse, got {:?}", chunks[0]);
        };
        assert_eq!(tu.id, "call_abc");
        assert_eq!(tu.name, "echo");
        let tau_domain::Value::Object(map) = &tu.input else {
            panic!("expected Object, got {:?}", tu.input);
        };
        let text = map.get("text").expect("text key");
        let tau_domain::Value::String(s) = text else {
            panic!("expected String");
        };
        assert_eq!(s, "hi");
        let CompletionChunk::Finish { stop_reason, .. } = chunks[1].as_ref().unwrap() else {
            panic!("expected Finish");
        };
        assert!(matches!(stop_reason, StopReason::ToolUse));
    }

    #[tokio::test]
    async fn stream_two_tool_calls_indexed_separately() {
        // Two tool calls at index 0 and 1; ids and args interleave.
        let body = bytes_stream_from_chunks(vec![
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"function\":{\"name\":\"a\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_b\",\"function\":{\"name\":\"b\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ]);
        let chunks = drain(Box::pin(stream_events(body))).await;
        // Expected: 2 ToolUse + 1 Finish, in index order.
        assert_eq!(chunks.len(), 3, "got: {chunks:?}");
        let CompletionChunk::ToolUse(tu0) = chunks[0].as_ref().unwrap() else {
            panic!("expected ToolUse");
        };
        let CompletionChunk::ToolUse(tu1) = chunks[1].as_ref().unwrap() else {
            panic!("expected ToolUse");
        };
        assert_eq!(tu0.id, "call_a");
        assert_eq!(tu0.name, "a");
        assert_eq!(tu1.id, "call_b");
        assert_eq!(tu1.name, "b");
    }

    #[tokio::test]
    async fn stream_tool_call_id_from_first_delta_preserved() {
        // The id arrives ONLY on the first delta for that index; the
        // subsequent fragment-only deltas omit it. The accumulator
        // must preserve the id from the first delta.
        let body = bytes_stream_from_chunks(vec![
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_first\",\"function\":{\"name\":\"echo\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ]);
        let chunks = drain(Box::pin(stream_events(body))).await;
        let CompletionChunk::ToolUse(tu) = chunks[0].as_ref().unwrap() else {
            panic!("expected ToolUse");
        };
        assert_eq!(tu.id, "call_first");
    }

    #[tokio::test]
    async fn stream_tool_call_arguments_invalid_json_yields_stream_error() {
        // `{not valid json` will fail to parse at finalize time.
        let body = bytes_stream_from_chunks(vec![
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_bad\",\"function\":{\"name\":\"echo\",\"arguments\":\"{not valid json\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n",
        ]);
        let chunks = drain(Box::pin(stream_events(body))).await;
        // Expect the finalize error to surface.
        assert!(
            chunks
                .iter()
                .any(|c| matches!(c, Err(LlmError::Stream { .. }))),
            "expected at least one Stream error, got: {chunks:?}",
        );
    }

    #[tokio::test]
    async fn stream_truncated_without_finish_reason_yields_stream_error() {
        // Body ends without a finish_reason event AND without [DONE].
        let body = bytes_stream_from_chunks(vec![
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n",
        ]);
        let chunks = drain(Box::pin(stream_events(body))).await;
        // 1 Text + 1 Stream error.
        assert_eq!(chunks.len(), 2, "got: {chunks:?}");
        let last = chunks.last().unwrap();
        assert!(last.is_err());
        if let Err(LlmError::Stream { message, .. }) = last {
            assert!(
                message.contains("ended without finish_reason"),
                "expected truncation message; got: {message}",
            );
        } else {
            panic!("expected LlmError::Stream");
        }
    }

    #[tokio::test]
    async fn stream_done_sentinel_without_finish_reason_yields_stream_error() {
        // [DONE] arrives but no finish_reason event preceded it.
        let body = bytes_stream_from_chunks(vec![
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"x\"},\"finish_reason\":null}]}\n\n",
            "data: [DONE]\n\n",
        ]);
        let chunks = drain(Box::pin(stream_events(body))).await;
        // 1 Text + 1 Stream error.
        assert_eq!(chunks.len(), 2, "got: {chunks:?}");
        let last = chunks.last().unwrap();
        assert!(last.is_err());
    }

    #[tokio::test]
    async fn stream_done_sentinel_consumed_silently_after_finish() {
        // Happy-path: finish_reason event followed by [DONE]; only
        // 1 Text + 1 Finish should be emitted (no spurious chunks).
        let body = bytes_stream_from_chunks(vec![
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n",
        ]);
        let chunks = drain(Box::pin(stream_events(body))).await;
        assert_eq!(chunks.len(), 2, "got: {chunks:?}");
        assert!(matches!(chunks[0], Ok(CompletionChunk::Text { .. })));
        assert!(matches!(chunks[1], Ok(CompletionChunk::Finish { .. })));
    }
}
