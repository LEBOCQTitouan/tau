//! Layer 2 — runtime.run_streaming tests.
//!
//! Tests the `runtime.run_streaming` dispatcher surface:
//!   - Pre-handshake call returns -32002 HANDSHAKE_REQUIRED.
//!   - Unknown agent returns -32010 UNKNOWN_AGENT.
//!   - Missing `agent` param returns -32602 INVALID_PARAMS.
//!   - Missing `prompt` param returns -32602 INVALID_PARAMS.
//!
//! The streaming event emission path (TextDelta, TurnCompleted, etc.)
//! requires a fully-resolved agent with an installed package; that is
//! exercised in Layer 3 e2e tests where the full serve process runs.

mod common;
use common::Harness;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/handshake-only")
}

/// `runtime.run_streaming` before handshake returns -32002 HANDSHAKE_REQUIRED.
/// This is the canonical pre-handshake rejection test for the streaming method.
#[tokio::test]
async fn streaming_before_handshake_returns_32002() {
    let mut h = Harness::new(fixture_dir()).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":20,"method":"runtime.run_streaming","params":{"agent":"x","prompt":"y"}}"#).await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 20);
    assert_eq!(resp["error"]["code"], -32002, "expected HANDSHAKE_REQUIRED, got: {resp}");
}

/// After handshake, `runtime.run_streaming` with an unknown agent returns
/// -32010 UNKNOWN_AGENT.
#[tokio::test]
async fn streaming_unknown_agent_returns_32010() {
    let mut h = Harness::new(fixture_dir()).await;
    h.handshake().await;

    h.send_raw(r#"{"jsonrpc":"2.0","id":21,"method":"runtime.run_streaming","params":{"agent":"ghost-agent","prompt":"hello"}}"#).await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 21);
    assert_eq!(resp["error"]["code"], -32010, "expected UNKNOWN_AGENT, got: {resp}");
    assert!(
        resp["error"]["data"]["agent_id"] == "ghost-agent",
        "expected agent_id in error data, got: {resp}"
    );
}

/// `runtime.run_streaming` with a missing `agent` param returns -32602.
#[tokio::test]
async fn streaming_missing_agent_param_returns_32602() {
    let mut h = Harness::new(fixture_dir()).await;
    h.handshake().await;

    h.send_raw(r#"{"jsonrpc":"2.0","id":22,"method":"runtime.run_streaming","params":{"prompt":"hello"}}"#).await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 22);
    assert_eq!(resp["error"]["code"], -32602, "expected INVALID_PARAMS, got: {resp}");
}

/// `runtime.run_streaming` with a missing `prompt` param returns -32602.
#[tokio::test]
async fn streaming_missing_prompt_returns_32602() {
    let mut h = Harness::new(fixture_dir()).await;
    h.handshake().await;

    h.send_raw(r#"{"jsonrpc":"2.0","id":23,"method":"runtime.run_streaming","params":{"agent":"some-agent"}}"#).await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 23);
    assert_eq!(resp["error"]["code"], -32602, "expected INVALID_PARAMS, got: {resp}");
}
