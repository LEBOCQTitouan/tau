//! Integration tests: AnthropicPlugin::stream against cassette replayer.

mod common;

use anthropic_plugin_lib::plugin::AnthropicPlugin;
use assert_matches::assert_matches;
use common::cassette;
use futures_util::StreamExt;
use tau_plugin_sdk::Configure;
use tau_ports::{CompletionChunk, LlmBackend, LlmError, StopReason};

#[tokio::test]
async fn stream_text_only_yields_chunks_then_finish() {
    let server = cassette::replay("tests/cassettes/stream_text_only.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config(server.uri().into())).unwrap();

    let mut stream = plugin.stream(common::sample_request()).await.unwrap();
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item);
    }

    // 2 text deltas + 1 Finish.
    assert_eq!(chunks.len(), 3, "got: {chunks:?}");
    assert_matches!(
        chunks[0].as_ref().unwrap(),
        CompletionChunk::Text { delta } => {
            assert_eq!(delta, "Hello");
        }
    );
    assert_matches!(
        chunks[1].as_ref().unwrap(),
        CompletionChunk::Text { delta } => {
            assert_eq!(delta, " world");
        }
    );
    assert_matches!(
        chunks[2].as_ref().unwrap(),
        CompletionChunk::Finish { stop_reason, usage } => {
            assert_eq!(*stop_reason, StopReason::EndTurn);
            let usage = usage.as_ref().expect("usage should be Some");
            assert_eq!(usage.input_tokens, 10);
            assert_eq!(usage.output_tokens, 3);
        }
    );
}

#[tokio::test]
async fn stream_with_tool_use_emits_full_tool_use_chunk() {
    let server = cassette::replay("tests/cassettes/stream_with_tool_use.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config(server.uri().into())).unwrap();

    let mut stream = plugin.stream(common::sample_request()).await.unwrap();
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item);
    }

    // Expected: 1 Text("Looking up...") + 1 ToolUse(tu) + 1 Finish.
    assert_eq!(chunks.len(), 3, "got: {chunks:?}");

    // First chunk: Text.
    assert_matches!(
        chunks[0].as_ref().unwrap(),
        CompletionChunk::Text { delta } => {
            assert_eq!(delta, "Looking up...");
        }
    );

    // Second chunk: ToolUse with parsed input.
    assert_matches!(
        chunks[1].as_ref().unwrap(),
        CompletionChunk::ToolUse(tu) => {
            assert_eq!(tu.id, "toolu_01");
            assert_eq!(tu.name, "echo");
            assert_matches!(
                &tu.input,
                tau_domain::Value::Object(map) => {
                    assert_eq!(map.len(), 1);
                    let text_value = map.get("text").expect("text key in input");
                    assert_matches!(
                        text_value,
                        tau_domain::Value::String(s) => {
                            assert_eq!(s, "hi");
                        }
                    );
                }
            );
        }
    );

    // Third chunk: Finish with stop_reason = ToolUse.
    assert_matches!(
        chunks[2].as_ref().unwrap(),
        CompletionChunk::Finish { stop_reason, .. } => {
            assert_eq!(*stop_reason, StopReason::ToolUse);
        }
    );
}

#[tokio::test]
async fn stream_error_mid_stream_terminates_with_err() {
    let server = cassette::replay("tests/cassettes/stream_error_mid_stream.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config(server.uri().into())).unwrap();

    let mut stream = plugin.stream(common::sample_request()).await.unwrap();
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item);
    }

    // 1 Text + 1 Err (Stream error).
    assert_eq!(chunks.len(), 2, "got: {chunks:?}");
    assert_matches!(&chunks[0], Ok(CompletionChunk::Text { .. }));

    assert_matches!(
        &chunks[1],
        Err(LlmError::Stream { message }) => {
            assert!(message.contains("overloaded_error"), "msg: {message}");
            assert!(message.contains("Service overloaded"), "msg: {message}");
        }
    );
}
