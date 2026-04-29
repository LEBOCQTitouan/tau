//! Integration tests: AnthropicPlugin::stream against cassette replayer.

mod common;

use anthropic_plugin_lib::plugin::AnthropicPlugin;
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
    let CompletionChunk::Text { ref delta } = chunks[0].as_ref().unwrap() else {
        panic!("expected Text, got {:?}", chunks[0]);
    };
    assert_eq!(delta, "Hello");

    let CompletionChunk::Text { ref delta } = chunks[1].as_ref().unwrap() else {
        panic!("expected Text, got {:?}", chunks[1]);
    };
    assert_eq!(delta, " world");

    let CompletionChunk::Finish {
        stop_reason,
        ref usage,
    } = chunks[2].as_ref().unwrap()
    else {
        panic!("expected Finish, got {:?}", chunks[2]);
    };
    assert_eq!(*stop_reason, StopReason::EndTurn);
    let usage = usage.as_ref().expect("usage should be Some");
    assert_eq!(usage.input_tokens, 10);
    assert_eq!(usage.output_tokens, 3);
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
    let CompletionChunk::Text { ref delta } = chunks[0].as_ref().unwrap() else {
        panic!("expected Text, got {:?}", chunks[0]);
    };
    assert_eq!(delta, "Looking up...");

    // Second chunk: ToolUse with parsed input.
    let CompletionChunk::ToolUse(ref tu) = chunks[1].as_ref().unwrap() else {
        panic!("expected ToolUse, got {:?}", chunks[1]);
    };
    assert_eq!(tu.id, "toolu_01");
    assert_eq!(tu.name, "echo");
    let tau_domain::Value::Object(map) = &tu.input else {
        panic!("expected Object, got {:?}", tu.input);
    };
    assert_eq!(map.len(), 1);
    let text_value = map.get("text").expect("text key in input");
    let tau_domain::Value::String(s) = text_value else {
        panic!("expected String, got {text_value:?}");
    };
    assert_eq!(s, "hi");

    // Third chunk: Finish with stop_reason = ToolUse.
    let CompletionChunk::Finish { stop_reason, .. } = chunks[2].as_ref().unwrap() else {
        panic!("expected Finish, got {:?}", chunks[2]);
    };
    assert_eq!(*stop_reason, StopReason::ToolUse);
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
    assert!(matches!(&chunks[0], Ok(CompletionChunk::Text { .. })));

    let Err(LlmError::Stream { ref message }) = chunks[1] else {
        panic!("expected Stream error, got {:?}", chunks[1]);
    };
    assert!(message.contains("overloaded_error"), "msg: {message}");
    assert!(message.contains("Service overloaded"), "msg: {message}");
}
