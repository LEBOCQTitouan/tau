//! Integration test: drive `run_tool_with_io` end-to-end via a
//! `FakeStdioPeer`, asserting the handshake → tool.call →
//! meta.shutdown lifecycle.

use std::time::SystemTime;

use tau_domain::{AgentInstanceId, PortKind, Value};
use tau_plugin_protocol::{
    handshake::{meta, HandshakeRequest, HandshakeResponse, TraceContext},
    test_support::FakeStdioPeer,
    Frame, PROTOCOL_VERSION,
};
use tau_plugin_sdk::run_tool_with_io;
use tau_ports::{
    fixtures::{make_session_context, make_tool_result, make_tool_spec, MockTool},
    SessionContext, ToolContent, ToolResult,
};

#[tokio::test]
async fn run_tool_handshake_and_call() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();

    let spec = make_tool_spec(
        "echo".to_string(),
        "echo args back".to_string(),
        Value::Object(Default::default()),
    );
    let canned = make_tool_result(
        vec![ToolContent::Text {
            text: "ok".to_string(),
        }],
        false,
    );
    let tool = MockTool::new("echo", spec).with_result(canned.clone());

    let runner = tokio::spawn(async move {
        run_tool_with_io(&mut sut_reader, &mut sut_writer, tool, "echo-tool", "0.1.0").await
    });

    // Handshake.
    let req = HandshakeRequest::new(
        PROTOCOL_VERSION.to_string(),
        PortKind::Tool,
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
    assert_eq!(resp.provides, PortKind::Tool);
    assert_eq!(resp.plugin_name, "echo-tool");
    assert!(resp.methods.iter().any(|m| m == "tool.call"));
    assert!(resp.methods.iter().any(|m| m == "tool.describe"));

    // tool.call: params is `(SessionContext, Value)`.
    let ctx: SessionContext = make_session_context(
        AgentInstanceId::new(),
        uuid::Uuid::now_v7(),
        Some(SystemTime::now() + std::time::Duration::from_secs(30)),
    );
    let args = Value::String("hi".into());
    let params_bytes = rmp_serde::to_vec(&(&ctx, &args)).unwrap();
    let call_frame = Frame::Request {
        id: 2,
        method: "tool.call".to_string(),
        params: params_bytes,
    };
    peer.writer
        .write_frame(&call_frame.encode().unwrap())
        .await
        .unwrap();

    let body = peer.reader.next_frame().await.unwrap().unwrap();
    let frame = Frame::decode(&body).unwrap();
    let Frame::Response { id, error, result } = frame else {
        panic!("expected tool.call response, got {frame:?}")
    };
    assert_eq!(id, 2);
    assert!(error.is_none(), "tool.call error: {error:?}");
    let result: ToolResult = rmp_serde::from_slice(&result.unwrap()).unwrap();
    assert!(!result.is_error);
    assert_eq!(result.content.len(), 1);

    // Drive shutdown.
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

#[tokio::test]
async fn run_tool_describe_returns_spec() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();

    let spec = make_tool_spec(
        "echo".to_string(),
        "echo args back".to_string(),
        Value::Object(Default::default()),
    );
    let tool = MockTool::new("echo", spec.clone());

    let runner = tokio::spawn(async move {
        run_tool_with_io(&mut sut_reader, &mut sut_writer, tool, "echo-tool", "0.1.0").await
    });

    // Handshake (minimal).
    let req = HandshakeRequest::new(
        PROTOCOL_VERSION.to_string(),
        PortKind::Tool,
        TraceContext::new("r".into(), "a".into(), "s".into()),
        serde_json::Value::Null,
    );
    let params_bytes = rmp_serde::to_vec(&vec![&req]).unwrap();
    peer.writer
        .write_frame(
            &Frame::Request {
                id: 1,
                method: meta::HANDSHAKE_METHOD.to_string(),
                params: params_bytes,
            }
            .encode()
            .unwrap(),
        )
        .await
        .unwrap();
    let _ = peer.reader.next_frame().await.unwrap().unwrap();

    // tool.describe: 0-element params.
    let params_bytes = rmp_serde::to_vec::<Vec<()>>(&Vec::new()).unwrap();
    peer.writer
        .write_frame(
            &Frame::Request {
                id: 2,
                method: "tool.describe".to_string(),
                params: params_bytes,
            }
            .encode()
            .unwrap(),
        )
        .await
        .unwrap();

    let body = peer.reader.next_frame().await.unwrap().unwrap();
    let frame = Frame::decode(&body).unwrap();
    let Frame::Response { id, error, result } = frame else {
        panic!("expected tool.describe response, got {frame:?}")
    };
    assert_eq!(id, 2);
    assert!(error.is_none(), "tool.describe error: {error:?}");
    let returned: tau_ports::ToolSpec = rmp_serde::from_slice(&result.unwrap()).unwrap();
    assert_eq!(returned.name, spec.name);
    assert_eq!(returned.description, spec.description);

    // Shutdown.
    peer.writer
        .write_frame(
            &Frame::Notification {
                method: meta::SHUTDOWN_METHOD.to_string(),
                params: rmp_serde::to_vec::<Vec<()>>(&Vec::new()).unwrap(),
            }
            .encode()
            .unwrap(),
        )
        .await
        .unwrap();

    runner.await.expect("runner task join").expect("runner ok");
}
