//! Integration tests for the `batch_to_stream` / `stream_to_batch`
//! round-trip helpers in [`tau_ports::llm`].
//!
//! Builds a canonical [`CompletionResponse`] via the
//! [`make_completion_response`] / [`make_tool_use`] / [`make_token_usage`]
//! factories from `tau_ports::fixtures` (required because
//! `CompletionResponse` and friends are `#[non_exhaustive]`), feeds it
//! through `batch_to_stream` to obtain a stream, and reassembles via
//! `stream_to_batch`. Asserts byte-for-byte / structural equivalence on
//! every field.
//!
//! Gated behind the `test-fixtures` feature: the fixture factories
//! ship under that gate.

#![cfg(feature = "test-fixtures")]

use tau_domain::Value;
use tau_ports::fixtures::{make_completion_response, make_token_usage, make_tool_use};
use tau_ports::llm::{batch_to_stream, stream_to_batch, StopReason};

/// Empty response (no text, no tool uses, no usage) round-trips cleanly.
#[tokio::test]
async fn empty_response_round_trip() {
    let resp = make_completion_response(String::new(), Vec::new(), StopReason::EndTurn, None);

    let stream = batch_to_stream(resp);
    let resp2 = stream_to_batch(stream).await.expect("stream_to_batch");

    assert_eq!(resp2.text, "");
    assert!(resp2.tool_uses.is_empty());
    assert_eq!(resp2.stop_reason, StopReason::EndTurn);
    assert!(resp2.usage.is_none());
}

/// Text-only response round-trips with text bytes preserved.
#[tokio::test]
async fn text_only_round_trip() {
    let resp = make_completion_response(
        "hello world".into(),
        Vec::new(),
        StopReason::MaxTokens,
        Some(make_token_usage(7, 11)),
    );

    let stream = batch_to_stream(resp);
    let resp2 = stream_to_batch(stream).await.expect("stream_to_batch");

    assert_eq!(resp2.text, "hello world");
    assert!(resp2.tool_uses.is_empty());
    assert_eq!(resp2.stop_reason, StopReason::MaxTokens);
    assert_eq!(resp2.usage, Some(make_token_usage(7, 11)));
}

/// Tool-use response (text + multiple tool uses + StopReason::ToolUse)
/// round-trips with tool_use ordering preserved.
#[tokio::test]
async fn tool_use_round_trip() {
    let tu1 = make_tool_use(
        "toolu_a".into(),
        "search".into(),
        Value::String("hello".into()),
    );
    let tu2 = make_tool_use(
        "toolu_b".into(),
        "fetch".into(),
        Value::String("world".into()),
    );

    let resp = make_completion_response(
        "preamble".into(),
        vec![tu1.clone(), tu2.clone()],
        StopReason::ToolUse,
        None,
    );

    let stream = batch_to_stream(resp);
    let resp2 = stream_to_batch(stream).await.expect("stream_to_batch");

    assert_eq!(resp2.text, "preamble");
    assert_eq!(resp2.tool_uses.len(), 2);
    assert_eq!(resp2.tool_uses[0].id, "toolu_a");
    assert_eq!(resp2.tool_uses[0].name, "search");
    assert!(resp2.tool_uses[0].input == Value::String("hello".into()));
    assert_eq!(resp2.tool_uses[1].id, "toolu_b");
    assert_eq!(resp2.tool_uses[1].name, "fetch");
    assert!(resp2.tool_uses[1].input == Value::String("world".into()));
    assert_eq!(resp2.stop_reason, StopReason::ToolUse);
    assert!(resp2.usage.is_none());
}

/// Round-trip preserves stop_reason across all variants.
#[tokio::test]
async fn stop_reason_variants_round_trip() {
    for sr in [
        StopReason::EndTurn,
        StopReason::MaxTokens,
        StopReason::StopSequence,
        StopReason::ToolUse,
        StopReason::Error,
    ] {
        let resp = make_completion_response("x".into(), Vec::new(), sr, None);
        let stream = batch_to_stream(resp);
        let resp2 = stream_to_batch(stream)
            .await
            .expect("stream_to_batch should succeed");
        assert_eq!(resp2.stop_reason, sr, "stop_reason mismatch for {sr:?}");
    }
}
