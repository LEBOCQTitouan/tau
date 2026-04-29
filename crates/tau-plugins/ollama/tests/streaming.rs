//! Integration tests: OllamaPlugin::stream against cassette replayer.

mod common;

use common::cassette;
use futures_util::StreamExt;
use ollama_plugin_lib::plugin::OllamaPlugin;
use tau_plugin_sdk::Configure;
use tau_ports::{CompletionChunk, LlmBackend, LlmError, StopReason};

#[tokio::test]
async fn stream_text_only_yields_chunks_then_finish() {
    let server = cassette::replay("tests/cassettes/stream_text_only.yaml").await;
    let plugin = OllamaPlugin::from_config(common::test_config(server.uri().into())).unwrap();

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
async fn stream_with_tool_use_emits_synthesized_tool_use_chunk() {
    let server = cassette::replay("tests/cassettes/stream_with_tool_use.yaml").await;
    let plugin = OllamaPlugin::from_config(common::test_config(server.uri().into())).unwrap();

    let mut stream = plugin.stream(common::sample_request()).await.unwrap();
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item);
    }

    // Expected: 1 Text("Looking up...") + 1 ToolUse + 1 Finish.
    assert_eq!(chunks.len(), 3, "got: {chunks:?}");

    let CompletionChunk::Text { ref delta } = chunks[0].as_ref().unwrap() else {
        panic!("expected Text, got {:?}", chunks[0]);
    };
    assert_eq!(delta, "Looking up...");

    let CompletionChunk::ToolUse(ref tu) = chunks[1].as_ref().unwrap() else {
        panic!("expected ToolUse, got {:?}", chunks[1]);
    };
    // Ollama doesn't include tool_call.id in stream lines either —
    // the parser synthesizes "ollama-tool-{counter}" per turn.
    assert_eq!(tu.id, "ollama-tool-0");
    assert_eq!(tu.name, "echo");
    let tau_domain::Value::Object(ref map) = tu.input else {
        panic!("expected Object, got {:?}", tu.input);
    };
    let text_value = map.get("text").expect("text key in input");
    let tau_domain::Value::String(s) = text_value else {
        panic!("expected String, got {text_value:?}");
    };
    assert_eq!(s, "hi");

    // Finish: Ollama doesn't have a tool-use-specific stop reason;
    // done_reason "stop" maps to EndTurn even when tool_calls are
    // present (caller infers from non-empty tool_uses).
    let CompletionChunk::Finish { stop_reason, .. } = chunks[2].as_ref().unwrap() else {
        panic!("expected Finish, got {:?}", chunks[2]);
    };
    assert_eq!(*stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn stream_truncated_response_yields_stream_error_at_end() {
    let server = cassette::replay("tests/cassettes/stream_truncated_response.yaml").await;
    let plugin = OllamaPlugin::from_config(common::test_config(server.uri().into())).unwrap();

    let mut stream = plugin.stream(common::sample_request()).await.unwrap();
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item);
    }

    // 1 Text + 1 Err.
    assert_eq!(chunks.len(), 2, "got: {chunks:?}");
    assert!(matches!(&chunks[0], Ok(CompletionChunk::Text { .. })));

    let Err(LlmError::Stream { ref message }) = chunks[1] else {
        panic!("expected Stream error, got {:?}", chunks[1]);
    };
    assert!(
        message.contains("ended before done:true"),
        "expected truncated-stream message; got: {message}"
    );
}
