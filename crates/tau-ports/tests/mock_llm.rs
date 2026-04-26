//! Integration tests for [`tau_ports::fixtures::MockLlmBackend`].
//!
//! Asserts that the mock satisfies the [`LlmBackend`] contract:
//! - `complete()` returns the canned [`CompletionResponse`] (or a
//!   default empty response when none was configured).
//! - `stream()` either replays canned chunks or derives them from the
//!   canned response via [`batch_to_stream`].
//! - Recorded invocations match the requests issued by the caller, in
//!   order.
//!
//! Gated behind the `test-fixtures` feature: imports `MockLlmBackend`
//! and the fixture factory helpers.

#![cfg(feature = "test-fixtures")]

use std::pin::Pin;

use futures_core::Stream;
use tau_ports::fixtures::{make_completion_request, make_completion_response, MockLlmBackend};
use tau_ports::llm::{
    CompletionChunk, CompletionRequest, CompletionStream, LlmBackend, StopReason,
};

/// Build a minimal `CompletionRequest` for the mock to record.
fn make_request(model: &str) -> CompletionRequest {
    make_completion_request(model.into())
}

/// Drain a `CompletionStream` into a `Vec<CompletionChunk>`. Errors
/// short-circuit.
async fn drain(mut stream: CompletionStream) -> Vec<CompletionChunk> {
    let mut out = Vec::new();
    loop {
        let next = std::future::poll_fn(|cx| Pin::new(&mut stream).poll_next(cx)).await;
        match next {
            None => break,
            Some(Ok(c)) => out.push(c),
            Some(Err(e)) => panic!("unexpected stream error: {e:?}"),
        }
    }
    out
}

/// `complete()` returns the canned response and records the request.
#[tokio::test]
async fn complete_returns_canned_response_and_records() {
    let canned =
        make_completion_response("canned reply".into(), Vec::new(), StopReason::EndTurn, None);
    let backend = MockLlmBackend::new("mock-llm").with_response(canned.clone());

    assert_eq!(backend.name(), "mock-llm");

    let req = make_request("model-a");
    let resp = backend.complete(req.clone()).await.expect("complete");
    assert_eq!(resp.text, "canned reply");
    assert_eq!(resp.stop_reason, StopReason::EndTurn);

    let recorded = backend.invocations();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].model, "model-a");
}

/// Without configured chunks, `stream()` derives chunks from the
/// canned response via `batch_to_stream` (one Text + one Finish for a
/// non-empty text response).
#[tokio::test]
async fn stream_derives_chunks_from_canned_response() {
    let canned = make_completion_response("hello".into(), Vec::new(), StopReason::EndTurn, None);
    let backend = MockLlmBackend::new("m").with_response(canned);

    let stream = backend.stream(make_request("m")).await.expect("stream");
    let chunks = drain(stream).await;

    // batch_to_stream emits: Text("hello"), Finish.
    assert_eq!(chunks.len(), 2);
    assert!(matches!(&chunks[0], CompletionChunk::Text { delta } if delta == "hello"));
    assert!(matches!(
        &chunks[1],
        CompletionChunk::Finish {
            stop_reason: StopReason::EndTurn,
            usage: None,
        },
    ));

    assert_eq!(backend.invocations().len(), 1);
}

/// Configured chunks override the response-derived stream verbatim.
#[tokio::test]
async fn stream_replays_configured_chunks() {
    let configured = vec![
        CompletionChunk::Text {
            delta: "alpha ".into(),
        },
        CompletionChunk::Text {
            delta: "beta".into(),
        },
        CompletionChunk::Finish {
            stop_reason: StopReason::MaxTokens,
            usage: None,
        },
    ];
    let backend = MockLlmBackend::new("m").with_chunks(configured);

    let stream = backend.stream(make_request("m")).await.expect("stream");
    let chunks = drain(stream).await;

    assert_eq!(chunks.len(), 3);
    assert!(matches!(&chunks[0], CompletionChunk::Text { delta } if delta == "alpha "));
    assert!(matches!(&chunks[1], CompletionChunk::Text { delta } if delta == "beta"));
    assert!(matches!(
        &chunks[2],
        CompletionChunk::Finish {
            stop_reason: StopReason::MaxTokens,
            ..
        },
    ));
}

/// Multiple invocations are recorded in order.
#[tokio::test]
async fn invocations_recorded_in_order() {
    let backend = MockLlmBackend::new("m");

    backend.complete(make_request("first")).await.expect("c1");
    backend.complete(make_request("second")).await.expect("c2");
    backend.complete(make_request("third")).await.expect("c3");

    let recorded = backend.invocations();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0].model, "first");
    assert_eq!(recorded[1].model, "second");
    assert_eq!(recorded[2].model, "third");
}
