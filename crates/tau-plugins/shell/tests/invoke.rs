//! Integration tests: ShellPlugin driven via FakeStdioPeer.
//!
//! Each test:
//! 1. Builds a SessionContext with specific granted_capabilities.
//! 2. Spawns the ShellPlugin via run_tool_with_io + a FakeStdioPeer.
//! 3. Sends handshake → tool.call((ctx, args)) → meta.shutdown.
//! 4. Decodes the response and asserts on its shape.

#![cfg(unix)]

use assert_matches::assert_matches;
use shell_plugin_lib::plugin::ShellPlugin;
use std::time::SystemTime;
use tau_domain::{AgentInstanceId, Capability, PortKind, Value};
use tau_plugin_protocol::{
    handshake::{meta, HandshakeRequest, TraceContext},
    test_support::FakeStdioPeer,
    Frame, PROTOCOL_VERSION,
};
use tau_plugin_sdk::{run_tool_with_io, Configure};
use tau_ports::SessionContext;
use uuid::Uuid;

// ---- helpers (mirror crates/tau-plugins/fs-read/tests/invoke.rs) ----

/// Build a `process.spawn` capability via JSON (`ProcessCapability::Spawn` is
/// `#[non_exhaustive]`, so struct-literal construction from external
/// crates is blocked).
fn process_spawn_cap(commands: &[&str]) -> Capability {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        cap: Capability,
    }
    let cmds_json: Vec<serde_json::Value> = commands
        .iter()
        .map(|c| serde_json::Value::String((*c).to_string()))
        .collect();
    let json = serde_json::json!({
        "cap": { "kind": "process.spawn", "commands": cmds_json }
    });
    serde_json::from_value::<Wrapper>(json)
        .expect("test process.spawn capability must parse")
        .cap
}

/// Drive the SDK handshake. Mirrors the helper in
/// crates/tau-plugins/fs-read/tests/invoke.rs.
async fn do_handshake(peer: &mut FakeStdioPeer) {
    let req = HandshakeRequest::new(
        PROTOCOL_VERSION.to_string(),
        PortKind::Tool,
        TraceContext::new("test-run".into(), "test-agent".into(), "test-span".into()),
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
async fn integration_echo_returns_expected_stdout() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
    let plugin = ShellPlugin::from_config(Default::default()).unwrap();

    let runner = tokio::spawn(async move {
        run_tool_with_io(&mut sut_reader, &mut sut_writer, plugin, "shell", "0.1.0").await
    });

    do_handshake(&mut peer).await;
    let ctx = make_ctx(vec![process_spawn_cap(&["echo"])]);
    send_tool_call(
        &mut peer,
        2,
        &ctx,
        serde_json::json!({ "command": "echo", "args": ["hi"] }),
    )
    .await;
    let result = recv_tool_response(&mut peer).await.expect("Ok response");

    assert!(!result.is_error, "expected non-error; got {result:?}");
    assert_matches!(&result.content[0], tau_ports::ToolContent::Json { .. });
    let tau_ports::ToolContent::Json { data } = &result.content[0] else {
        unreachable!("just asserted Json shape above")
    };
    let map = data.as_object().expect("Object");
    let stdout = map
        .get("stdout")
        .and_then(Value::as_string)
        .expect("stdout string");
    assert_eq!(stdout, "hi\n");
    assert_matches!(
        map.get("exit_code"),
        Some(Value::Integer(code)) => {
            assert_eq!(*code, 0);
        }
    );

    shutdown(&mut peer).await;
    drop(peer);
    let _ = runner.await;
}

#[tokio::test]
async fn integration_long_running_killed_by_timeout() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
    let plugin = ShellPlugin::from_config(Default::default()).unwrap();

    let runner = tokio::spawn(async move {
        run_tool_with_io(&mut sut_reader, &mut sut_writer, plugin, "shell", "0.1.0").await
    });

    do_handshake(&mut peer).await;
    let ctx = make_ctx(vec![process_spawn_cap(&["sh"])]);
    send_tool_call(
        &mut peer,
        2,
        &ctx,
        serde_json::json!({
            "command": "sh",
            "args": ["-c", "sleep 5"],
            "timeout_secs": 1
        }),
    )
    .await;
    let result = recv_tool_response(&mut peer).await.expect("Ok response");

    assert_matches!(&result.content[0], tau_ports::ToolContent::Json { .. });
    let tau_ports::ToolContent::Json { data } = &result.content[0] else {
        unreachable!("just asserted Json shape above")
    };
    let map = data.as_object().expect("Object");
    assert_matches!(
        map.get("timed_out"),
        Some(Value::Bool(t)) => {
            assert!(*t, "expected timed_out: true");
        }
    );
    assert_matches!(
        map.get("exit_code"),
        Some(Value::Integer(code)) => {
            assert_eq!(*code, -1);
        }
    );

    shutdown(&mut peer).await;
    drop(peer);
    let _ = runner.await;
}

#[tokio::test]
async fn integration_command_outside_allowlist_bad_args() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
    let plugin = ShellPlugin::from_config(Default::default()).unwrap();

    let runner = tokio::spawn(async move {
        run_tool_with_io(&mut sut_reader, &mut sut_writer, plugin, "shell", "0.1.0").await
    });

    do_handshake(&mut peer).await;
    // Grant only "echo"; try to run "cat".
    let ctx = make_ctx(vec![process_spawn_cap(&["echo"])]);
    send_tool_call(
        &mut peer,
        2,
        &ctx,
        serde_json::json!({ "command": "cat", "args": [] }),
    )
    .await;
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

#[tokio::test]
async fn integration_large_stdout_truncated_and_flagged() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
    let plugin = ShellPlugin::from_config(Default::default()).unwrap();

    let runner = tokio::spawn(async move {
        run_tool_with_io(&mut sut_reader, &mut sut_writer, plugin, "shell", "0.1.0").await
    });

    do_handshake(&mut peer).await;
    // `yes` runs forever producing "a\n" — kill it via timeout.
    let ctx = make_ctx(vec![process_spawn_cap(&["yes"])]);
    send_tool_call(
        &mut peer,
        2,
        &ctx,
        serde_json::json!({
            "command": "yes",
            "args": ["a"],
            "timeout_secs": 3
        }),
    )
    .await;
    let result = recv_tool_response(&mut peer).await.expect("Ok response");

    assert_matches!(&result.content[0], tau_ports::ToolContent::Json { .. });
    let tau_ports::ToolContent::Json { data } = &result.content[0] else {
        unreachable!("just asserted Json shape above")
    };
    let map = data.as_object().expect("Object");
    let stdout = map
        .get("stdout")
        .and_then(Value::as_string)
        .expect("stdout string");
    // The output should be exactly MAX_OUTPUT_BYTES (1 MiB).
    assert_eq!(stdout.len(), 1024 * 1024);
    assert_matches!(
        map.get("stdout_truncated"),
        Some(Value::Bool(t)) => {
            assert!(*t, "expected stdout_truncated: true");
        }
    );

    shutdown(&mut peer).await;
    drop(peer);
    let _ = runner.await;
}

#[tokio::test]
async fn integration_deny_overrides_allow_for_shell() {
    // Allow includes "echo"; deny lists "echo" → expect rejection.
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
    let plugin = ShellPlugin::from_config(Default::default()).unwrap();

    let runner = tokio::spawn(async move {
        run_tool_with_io(&mut sut_reader, &mut sut_writer, plugin, "shell", "0.1.0").await
    });

    do_handshake(&mut peer).await;
    let ctx = SessionContext::new(
        AgentInstanceId::new(),
        Uuid::now_v7(),
        Some(SystemTime::UNIX_EPOCH),
    )
    .with_granted_capabilities(vec![process_spawn_cap(&["echo"])])
    .with_deny_entries(vec![tau_ports::DenyEntry::new(
        "process.spawn".into(),
        vec!["echo".into()],
    )]);
    send_tool_call(
        &mut peer,
        2,
        &ctx,
        serde_json::json!({ "command": "echo", "args": ["hi"] }),
    )
    .await;
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
