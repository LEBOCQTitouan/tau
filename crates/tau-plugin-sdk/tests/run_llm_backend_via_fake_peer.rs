//! Integration test: drive `run_llm_backend_with_io` end-to-end via a
//! `FakeStdioPeer`, asserting the handshake → llm.complete →
//! meta.shutdown lifecycle.

use tau_domain::PortKind;
use tau_plugin_protocol::{
    handshake::{meta, HandshakeRequest, HandshakeResponse, TraceContext},
    test_support::FakeStdioPeer,
    Frame, PROTOCOL_VERSION,
};
use tau_plugin_sdk::run_llm_backend_with_io;
use tau_ports::{
    fixtures::{
        make_completion_request, make_completion_response, make_token_usage, MockLlmBackend,
    },
    CompletionResponse, StopReason,
};

#[tokio::test]
async fn run_llm_backend_handshake_and_complete() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();

    // Configure the mock backend with a canned response.
    let canned = make_completion_response(
        "hello world".to_string(),
        Vec::new(),
        StopReason::EndTurn,
        Some(make_token_usage(7, 11)),
    );
    let backend = MockLlmBackend::new("echo-llm").with_response(canned.clone());

    // Spawn the runner.
    let runner = tokio::spawn(async move {
        run_llm_backend_with_io(
            &mut sut_reader,
            &mut sut_writer,
            backend,
            "echo-llm",
            "0.1.0",
        )
        .await
    });

    // ---- Step 1: drive the handshake from the peer (host) side ----
    let req = HandshakeRequest::new(
        PROTOCOL_VERSION.to_string(),
        PortKind::LlmBackend,
        TraceContext::new("r".into(), "a".into(), "s".into()),
        serde_json::Value::Null,
    );
    let params_bytes = rmp_serde::to_vec(&vec![&req]).unwrap();
    let handshake_frame = Frame::Request {
        id: 1,
        method: meta::HANDSHAKE_METHOD.to_string(),
        params: params_bytes,
    };
    peer.writer
        .write_frame(&handshake_frame.encode().unwrap())
        .await
        .unwrap();

    let body = peer.reader.next_frame().await.unwrap().unwrap();
    let frame = Frame::decode(&body).unwrap();
    let Frame::Response { id, error, result } = frame else {
        panic!("expected handshake response, got {frame:?}")
    };
    assert_eq!(id, 1);
    assert!(error.is_none(), "handshake error: {error:?}");
    let resp: HandshakeResponse = rmp_serde::from_slice(&result.unwrap()).unwrap();
    assert_eq!(resp.provides, PortKind::LlmBackend);
    assert_eq!(resp.plugin_name, "echo-llm");
    assert_eq!(resp.plugin_version, "0.1.0");
    assert!(resp.methods.iter().any(|m| m == "llm.complete"));
    assert!(resp.methods.iter().any(|m| m == "llm.stream"));

    // ---- Step 2: send an `llm.complete` request, expect the canned response ----
    let req = make_completion_request("test-model".to_string());
    let params_bytes = rmp_serde::to_vec(&vec![&req]).unwrap();
    let complete_frame = Frame::Request {
        id: 2,
        method: "llm.complete".to_string(),
        params: params_bytes,
    };
    peer.writer
        .write_frame(&complete_frame.encode().unwrap())
        .await
        .unwrap();

    let body = peer.reader.next_frame().await.unwrap().unwrap();
    let frame = Frame::decode(&body).unwrap();
    let Frame::Response { id, error, result } = frame else {
        panic!("expected llm.complete response, got {frame:?}")
    };
    assert_eq!(id, 2);
    assert!(error.is_none(), "complete error: {error:?}");
    let resp: CompletionResponse = rmp_serde::from_slice(&result.unwrap()).unwrap();
    assert_eq!(resp.text, "hello world");
    assert_eq!(resp.stop_reason, canned.stop_reason);
    assert_eq!(resp.usage, canned.usage);

    // ---- Step 3: send `meta.shutdown`, expect the runner to exit ----
    let shutdown_frame = Frame::Notification {
        method: meta::SHUTDOWN_METHOD.to_string(),
        // 0-element array (empty params).
        params: rmp_serde::to_vec::<Vec<()>>(&Vec::new()).unwrap(),
    };
    peer.writer
        .write_frame(&shutdown_frame.encode().unwrap())
        .await
        .unwrap();

    runner.await.expect("runner task join").expect("runner ok");
}

#[tokio::test]
async fn run_llm_backend_streaming_emits_chunks_then_summary() {
    use tau_ports::CompletionChunk;

    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();

    // Configure the mock with explicit chunks (Text + Finish) so we can assert
    // that the runner forwards them as `stream.chunk` notifications.
    let chunks = vec![
        CompletionChunk::Text {
            delta: "hello ".to_string(),
        },
        CompletionChunk::Text {
            delta: "world".to_string(),
        },
        CompletionChunk::Finish {
            stop_reason: StopReason::EndTurn,
            usage: Some(make_token_usage(3, 5)),
        },
    ];
    let backend = MockLlmBackend::new("echo-llm").with_chunks(chunks);

    let runner = tokio::spawn(async move {
        run_llm_backend_with_io(
            &mut sut_reader,
            &mut sut_writer,
            backend,
            "echo-llm",
            "0.1.0",
        )
        .await
    });

    // Handshake.
    let req = HandshakeRequest::new(
        PROTOCOL_VERSION.to_string(),
        PortKind::LlmBackend,
        TraceContext::new("r".into(), "a".into(), "s".into()),
        serde_json::Value::Null,
    );
    let params_bytes = rmp_serde::to_vec(&vec![&req]).unwrap();
    let handshake_frame = Frame::Request {
        id: 10,
        method: meta::HANDSHAKE_METHOD.to_string(),
        params: params_bytes,
    };
    peer.writer
        .write_frame(&handshake_frame.encode().unwrap())
        .await
        .unwrap();
    let _ = peer.reader.next_frame().await.unwrap().unwrap();

    // Send `llm.stream`.
    let req = make_completion_request("test-model".to_string());
    let params_bytes = rmp_serde::to_vec(&vec![&req]).unwrap();
    let stream_frame = Frame::Request {
        id: 11,
        method: "llm.stream".to_string(),
        params: params_bytes,
    };
    peer.writer
        .write_frame(&stream_frame.encode().unwrap())
        .await
        .unwrap();

    // Read frames until the final response.
    let mut chunk_count = 0;
    let final_summary: Option<(Option<StopReason>, Option<tau_ports::TokenUsage>)>;
    loop {
        let body = peer.reader.next_frame().await.unwrap().unwrap();
        let frame = Frame::decode(&body).unwrap();
        match frame {
            Frame::Notification { method, params } if method == "stream.chunk" => {
                let (msgid, _chunk): (u32, CompletionChunk) =
                    rmp_serde::from_slice(&params).unwrap();
                assert_eq!(msgid, 11);
                chunk_count += 1;
            }
            Frame::Response { id, error, result } => {
                assert_eq!(id, 11);
                assert!(error.is_none(), "stream error: {error:?}");
                #[derive(serde::Deserialize)]
                struct Summary {
                    stop_reason: Option<StopReason>,
                    usage: Option<tau_ports::TokenUsage>,
                }
                let summary: Summary = rmp_serde::from_slice(&result.unwrap()).unwrap();
                final_summary = Some((summary.stop_reason, summary.usage));
                break;
            }
            other => panic!("unexpected frame during stream: {other:?}"),
        }
    }

    assert_eq!(chunk_count, 3, "expected three stream.chunk notifications");
    let (stop_reason, usage) = final_summary.expect("final summary");
    assert_eq!(stop_reason, Some(StopReason::EndTurn));
    assert_eq!(usage, Some(make_token_usage(3, 5)));

    // Drive shutdown to terminate the runner.
    let shutdown_frame = Frame::Notification {
        method: meta::SHUTDOWN_METHOD.to_string(),
        params: rmp_serde::to_vec::<Vec<()>>(&Vec::new()).unwrap(),
    };
    peer.writer
        .write_frame(&shutdown_frame.encode().unwrap())
        .await
        .unwrap();

    runner.await.expect("runner task join").expect("runner ok");
}
