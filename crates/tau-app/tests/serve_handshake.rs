//! Layer 2 — handshake protocol tests.
//!
//! Uses the in-memory `Harness` from `common::` to drive a real `Dispatcher`
//! without spawning any subprocess. No real LLM or Runtime calls are made
//! during handshake negotiation.

mod common;
use common::Harness;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/handshake-only")
}

#[tokio::test]
async fn happy_handshake() {
    let mut h = Harness::new(fixture_dir()).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"meta.handshake","params":{"client_name":"test","client_version":"0.1.0","protocol_version":1}}"#).await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocol_version"], 1);
    assert_eq!(resp["result"]["server_name"], "tau");
}

#[tokio::test]
async fn version_mismatch() {
    let mut h = Harness::new(fixture_dir()).await;
    h.send_raw(
        r#"{"jsonrpc":"2.0","id":1,"method":"meta.handshake","params":{"protocol_version":999}}"#,
    )
    .await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["error"]["code"], -32000);
}

#[tokio::test]
async fn pre_handshake_runtime_call_rejected() {
    let mut h = Harness::new(fixture_dir()).await;
    h.send_raw(
        r#"{"jsonrpc":"2.0","id":1,"method":"runtime.run","params":{"agent":"x","prompt":"y"}}"#,
    )
    .await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["error"]["code"], -32002);
}

#[tokio::test]
async fn double_handshake_rejected() {
    let mut h = Harness::new(fixture_dir()).await;
    h.send_raw(
        r#"{"jsonrpc":"2.0","id":1,"method":"meta.handshake","params":{"protocol_version":1}}"#,
    )
    .await;
    let _ = h.recv().await;
    h.send_raw(
        r#"{"jsonrpc":"2.0","id":2,"method":"meta.handshake","params":{"protocol_version":1}}"#,
    )
    .await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["error"]["code"], -32003);
}

#[tokio::test]
async fn ping_works_before_handshake() {
    let mut h = Harness::new(fixture_dir()).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"meta.ping"}"#)
        .await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["result"]["ok"], true);
}
