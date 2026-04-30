//! Integration test: drive `run_tool_with_io` end-to-end via a
//! `FakeStdioPeer`, asserting the handshake → tool.call →
//! meta.shutdown lifecycle.

use std::time::SystemTime;

use tau_domain::{AgentInstanceId, Capability, FsCapability, PortKind, Value};
use tau_plugin_protocol::{
    handshake::{meta, HandshakeRequest, HandshakeResponse, TraceContext},
    test_support::FakeStdioPeer,
    Frame, PROTOCOL_VERSION,
};
use tau_plugin_sdk::run_tool_with_io;
use tau_ports::{
    fixtures::{make_session_context, make_tool_result, make_tool_spec, MockTool},
    SessionContext, ToolContent, ToolError, ToolResult, ToolSpec,
};

// ---------------------------------------------------------------------------
// Fixture tools for describe_capabilities tests
// ---------------------------------------------------------------------------

/// Build an `fs.read` capability via the canonical JSON deserialization path.
/// Variant-level `#[non_exhaustive]` blocks struct-literal construction of
/// `FsCapability::Read { paths }` from outside `tau-domain`, so we
/// round-trip through the manifest wire form (same technique used in
/// `tau-runtime` tests).
fn fs_read_cap(paths: &[&str]) -> Capability {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        cap: Capability,
    }
    let paths_json: Vec<serde_json::Value> = paths
        .iter()
        .map(|p| serde_json::Value::String(p.to_string()))
        .collect();
    let json = serde_json::json!({ "cap": { "kind": "fs.read", "paths": paths_json } });
    serde_json::from_value::<Wrapper>(json)
        .expect("test fs.read capability must parse")
        .cap
}

/// A minimal tool that declares one `fs.read` capability.
struct ToolWithCaps;

impl tau_ports::Tool for ToolWithCaps {
    type Session = ();

    fn name(&self) -> &str {
        "caps-tool"
    }

    fn schema(&self) -> ToolSpec {
        make_tool_spec(
            "caps-tool".to_string(),
            "tool with capabilities".to_string(),
            Value::Object(Default::default()),
        )
    }

    fn capabilities(&self) -> &[Capability] {
        static CAPS: std::sync::OnceLock<Vec<Capability>> = std::sync::OnceLock::new();
        CAPS.get_or_init(|| vec![fs_read_cap(&[])])
    }

    async fn init(&self, _ctx: SessionContext) -> Result<Self::Session, ToolError> {
        Ok(())
    }

    async fn invoke(
        &self,
        _session: &mut Self::Session,
        _args: Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(make_tool_result(vec![], false))
    }

    async fn teardown(&self, _session: Self::Session) -> Result<(), ToolError> {
        Ok(())
    }
}

/// A minimal tool that uses the default capabilities() impl (returns &[]).
struct ToolWithoutCaps;

impl tau_ports::Tool for ToolWithoutCaps {
    type Session = ();

    fn name(&self) -> &str {
        "nocaps-tool"
    }

    fn schema(&self) -> ToolSpec {
        make_tool_spec(
            "nocaps-tool".to_string(),
            "tool without capabilities".to_string(),
            Value::Object(Default::default()),
        )
    }

    async fn init(&self, _ctx: SessionContext) -> Result<Self::Session, ToolError> {
        Ok(())
    }

    async fn invoke(
        &self,
        _session: &mut Self::Session,
        _args: Value,
    ) -> Result<ToolResult, ToolError> {
        Ok(make_tool_result(vec![], false))
    }

    async fn teardown(&self, _session: Self::Session) -> Result<(), ToolError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper: perform handshake and discard the response frame.
// ---------------------------------------------------------------------------
async fn do_handshake(peer: &mut tau_plugin_protocol::test_support::FakeStdioPeer) {
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
    // Consume the handshake response.
    let _ = peer.reader.next_frame().await.unwrap().unwrap();
}

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

#[tokio::test]
async fn runner_handles_describe_capabilities_for_tool_with_caps() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();

    let runner = tokio::spawn(async move {
        run_tool_with_io(
            &mut sut_reader,
            &mut sut_writer,
            ToolWithCaps,
            "caps-tool",
            "0.1.0",
        )
        .await
    });

    do_handshake(&mut peer).await;

    // tool.describe_capabilities: 0-element params.
    let params_bytes = rmp_serde::to_vec::<Vec<()>>(&Vec::new()).unwrap();
    peer.writer
        .write_frame(
            &Frame::Request {
                id: 2,
                method: "tool.describe_capabilities".to_string(),
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
        panic!("expected tool.describe_capabilities response, got {frame:?}")
    };
    assert_eq!(id, 2);
    assert!(
        error.is_none(),
        "tool.describe_capabilities error: {error:?}"
    );
    let caps: Vec<Capability> = rmp_serde::from_slice(&result.unwrap()).unwrap();
    assert_eq!(caps.len(), 1);
    assert!(
        matches!(
            &caps[0],
            Capability::Filesystem(FsCapability::Read { paths, .. }) if paths.is_empty()
        ),
        "expected Filesystem(Read {{ paths: [] }}), got {:?}",
        caps[0]
    );

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

#[tokio::test]
async fn runner_handles_describe_capabilities_for_tool_without_caps() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();

    let runner = tokio::spawn(async move {
        run_tool_with_io(
            &mut sut_reader,
            &mut sut_writer,
            ToolWithoutCaps,
            "nocaps-tool",
            "0.1.0",
        )
        .await
    });

    do_handshake(&mut peer).await;

    // tool.describe_capabilities: 0-element params.
    let params_bytes = rmp_serde::to_vec::<Vec<()>>(&Vec::new()).unwrap();
    peer.writer
        .write_frame(
            &Frame::Request {
                id: 2,
                method: "tool.describe_capabilities".to_string(),
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
        panic!("expected tool.describe_capabilities response, got {frame:?}")
    };
    assert_eq!(id, 2);
    assert!(
        error.is_none(),
        "tool.describe_capabilities error: {error:?}"
    );
    let caps: Vec<Capability> = rmp_serde::from_slice(&result.unwrap()).unwrap();
    assert!(caps.is_empty(), "expected empty capabilities, got {caps:?}");

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
