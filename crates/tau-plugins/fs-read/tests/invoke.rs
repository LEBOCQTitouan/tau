//! Integration tests: FsReadPlugin driven via FakeStdioPeer.
//!
//! Each test:
//! 1. Builds a SessionContext with specific granted_capabilities.
//! 2. Spawns the FsReadPlugin via run_tool_with_io + a FakeStdioPeer.
//! 3. Sends handshake → tool.call((ctx, args)) → meta.shutdown.
//! 4. Decodes the response and asserts on its shape.

use assert_matches::assert_matches;
use base64::Engine as _;
use fs_read_plugin_lib::plugin::FsReadPlugin;
use std::time::SystemTime;
use tau_domain::{AgentInstanceId, Capability, PortKind, Value};
use tau_plugin_protocol::{
    handshake::{meta, HandshakeRequest, TraceContext},
    test_support::FakeStdioPeer,
    Frame, PROTOCOL_VERSION,
};
use tau_plugin_sdk::{run_tool_with_io, Configure};
use tau_ports::{DenyEntry, SessionContext};
use uuid::Uuid;

// ---- helpers ----

/// Build an `fs.read` capability via JSON (FsCapability::Read is
/// `#[non_exhaustive]`, so struct-literal construction from external
/// crates is blocked).
fn fs_read_cap(paths: &[&str]) -> Capability {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        cap: Capability,
    }
    let paths_json: Vec<serde_json::Value> = paths
        .iter()
        .map(|p| serde_json::Value::String((*p).to_string()))
        .collect();
    let json = serde_json::json!({ "cap": { "kind": "fs.read", "paths": paths_json } });
    serde_json::from_value::<Wrapper>(json)
        .expect("test fs.read capability must parse")
        .cap
}

/// Drive the SDK handshake. Mirrors the helper in
/// crates/tau-plugin-sdk/tests/run_tool_via_fake_peer.rs.
async fn do_handshake(peer: &mut FakeStdioPeer) {
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

async fn send_tool_call(
    peer: &mut FakeStdioPeer,
    id: u32,
    ctx: &SessionContext,
    args: serde_json::Value,
) {
    let args_value: Value = serde_json::from_value(args).expect("args round-trip to tau Value");
    let params_bytes = rmp_serde::to_vec(&(ctx, &args_value)).unwrap();
    peer.writer
        .write_frame(
            &Frame::Request {
                id,
                method: "tool.call".to_string(),
                params: params_bytes,
            }
            .encode()
            .unwrap(),
        )
        .await
        .unwrap();
}

async fn recv_tool_response(peer: &mut FakeStdioPeer) -> Result<tau_ports::ToolResult, String> {
    let body = peer.reader.next_frame().await.unwrap().unwrap();
    let frame = Frame::decode(&body).map_err(|e| format!("frame decode: {e}"))?;
    match frame {
        Frame::Response {
            result: Some(bytes),
            error: None,
            ..
        } => {
            let result: tau_ports::ToolResult =
                rmp_serde::from_slice(&bytes).map_err(|e| format!("rmp decode ToolResult: {e}"))?;
            Ok(result)
        }
        Frame::Response {
            error: Some(env),
            result: None,
            ..
        } => Err(format!("rpc error code={} msg={}", env.code, env.message)),
        other => Err(format!("unexpected frame: {other:?}")),
    }
}

async fn shutdown(peer: &mut FakeStdioPeer) {
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
}

fn make_ctx(grants: Vec<Capability>) -> SessionContext {
    SessionContext::new(
        AgentInstanceId::new(),
        Uuid::now_v7(),
        Some(SystemTime::UNIX_EPOCH),
    )
    .with_granted_capabilities(grants)
}

// ---- tests ----

#[tokio::test]
async fn integration_read_tempfile_succeeds() {
    let tmpfile = tempfile::NamedTempFile::new().unwrap();
    let path = tmpfile.path().to_str().unwrap().to_string();
    let content = b"hello tau\n";
    std::fs::write(tmpfile.path(), content).unwrap();

    // Cover the tempfile's parent dir so the glob matches.
    let parent = tmpfile.path().parent().unwrap().to_str().unwrap();
    let glob = format!("{parent}/**");

    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
    let plugin = FsReadPlugin::from_config(Default::default()).unwrap();

    let runner = tokio::spawn(async move {
        run_tool_with_io(&mut sut_reader, &mut sut_writer, plugin, "fs-read", "0.1.0").await
    });

    do_handshake(&mut peer).await;
    let ctx = make_ctx(vec![fs_read_cap(&[&glob])]);
    send_tool_call(&mut peer, 2, &ctx, serde_json::json!({ "path": path })).await;
    let result = recv_tool_response(&mut peer).await.expect("Ok response");

    assert!(!result.is_error, "expected success; got {result:?}");
    assert_eq!(result.content.len(), 1);
    assert_matches!(&result.content[0], tau_ports::ToolContent::Json { .. });
    let tau_ports::ToolContent::Json { data } = &result.content[0] else {
        unreachable!("just asserted Json shape above")
    };
    let map = data.as_object().expect("Object data");
    let contents_b64 = map
        .get("contents")
        .and_then(Value::as_string)
        .expect("contents string");
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(contents_b64)
        .expect("base64 decode");
    assert_eq!(decoded, content);

    shutdown(&mut peer).await;
    drop(peer);
    let _ = runner.await;
}

#[tokio::test]
async fn integration_read_outside_glob_scope_bad_args() {
    let tmpfile = tempfile::NamedTempFile::new().unwrap();
    let path = tmpfile.path().to_str().unwrap().to_string();

    // Grant a glob that does NOT cover the tempfile.
    let unrelated_glob = "/var/definitely-not-tmpfile-dir/**";

    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
    let plugin = FsReadPlugin::from_config(Default::default()).unwrap();

    let runner = tokio::spawn(async move {
        run_tool_with_io(&mut sut_reader, &mut sut_writer, plugin, "fs-read", "0.1.0").await
    });

    do_handshake(&mut peer).await;
    let ctx = make_ctx(vec![fs_read_cap(&[unrelated_glob])]);
    send_tool_call(&mut peer, 2, &ctx, serde_json::json!({ "path": path })).await;
    let err = recv_tool_response(&mut peer)
        .await
        .expect_err("expected RPC error");
    assert!(
        err.contains("not in capability scope"),
        "expected scope-violation error; got: {err}"
    );

    shutdown(&mut peer).await;
    drop(peer);
    let _ = runner.await;
}

// Uses Unix-style path "/tmp/../etc/passwd" to drive the traversal
// rejection. On Windows that path is not absolute, so validate_path
// short-circuits at NotAbsolute before reaching the traversal check —
// the test would assert wrong error wording. The traversal logic
// itself is OS-agnostic; this test documents Unix-fixture behavior.
#[cfg(unix)]
#[tokio::test]
async fn integration_traversal_rejected() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
    let plugin = FsReadPlugin::from_config(Default::default()).unwrap();

    let runner = tokio::spawn(async move {
        run_tool_with_io(&mut sut_reader, &mut sut_writer, plugin, "fs-read", "0.1.0").await
    });

    do_handshake(&mut peer).await;
    // Even with a permissive grant, ../ is rejected at validate_path.
    let ctx = make_ctx(vec![fs_read_cap(&["/**"])]);
    send_tool_call(
        &mut peer,
        2,
        &ctx,
        serde_json::json!({ "path": "/tmp/../etc/passwd" }),
    )
    .await;
    let err = recv_tool_response(&mut peer)
        .await
        .expect_err("expected RPC error");
    assert!(
        err.contains("contains a `..` segment") || err.contains("traversal"),
        "expected traversal error; got: {err}"
    );

    shutdown(&mut peer).await;
    drop(peer);
    let _ = runner.await;
}

#[cfg(unix)]
#[tokio::test]
async fn integration_deny_overrides_allow() {
    let tmpfile = tempfile::NamedTempFile::new().unwrap();
    let path = tmpfile.path().to_str().unwrap().to_string();
    std::fs::write(tmpfile.path(), b"secret").unwrap();

    // Allow covers the file's parent dir; deny lists the exact file.
    let parent = tmpfile.path().parent().unwrap().to_str().unwrap();
    let allow_glob = format!("{parent}/**");

    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
    let plugin = FsReadPlugin::from_config(Default::default()).unwrap();

    let runner = tokio::spawn(async move {
        run_tool_with_io(&mut sut_reader, &mut sut_writer, plugin, "fs-read", "0.1.0").await
    });

    do_handshake(&mut peer).await;
    // Allow covers the file; deny lists the exact file → expect rejection.
    let ctx = SessionContext::new(
        AgentInstanceId::new(),
        Uuid::now_v7(),
        Some(SystemTime::UNIX_EPOCH),
    )
    .with_granted_capabilities(vec![fs_read_cap(&[&allow_glob])])
    .with_deny_entries(vec![DenyEntry::new("fs.read".into(), vec![path.clone()])]);
    send_tool_call(&mut peer, 2, &ctx, serde_json::json!({ "path": path })).await;
    let err = recv_tool_response(&mut peer)
        .await
        .expect_err("expected scope-rejection RPC error");
    assert!(
        err.contains("not in capability scope"),
        "expected scope-violation error; got: {err}"
    );

    shutdown(&mut peer).await;
    drop(peer);
    let _ = runner.await;
}
