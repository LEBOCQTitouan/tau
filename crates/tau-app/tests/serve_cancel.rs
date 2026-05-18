//! Layer 2 — runtime.cancel tests.
//!
//! Tests the `runtime.cancel` dispatcher surface:
//!   - Cancel unknown id returns `{cancelled: false}`.
//!   - Cancel with missing params.id returns -32602 INVALID_PARAMS.
//!   - Cancel with invalid params.id type returns -32602 INVALID_PARAMS.
//!   - Cancel before handshake returns -32002 HANDSHAKE_REQUIRED.

mod common;
use common::Harness;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/handshake-only")
}

/// Cancelling a request id that is not in-flight returns `{cancelled: false}`.
/// This is the normal case when a client cancels after the run has already
/// completed.
#[tokio::test]
async fn cancel_unknown_id_returns_false() {
    let mut h = Harness::new(fixture_dir()).await;
    h.handshake().await;

    h.send_raw(r#"{"jsonrpc":"2.0","id":30,"method":"runtime.cancel","params":{"id":999}}"#)
        .await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 30);
    assert_eq!(
        resp["result"]["cancelled"], false,
        "expected cancelled=false, got: {resp}"
    );
}

/// Cancelling a string request id that is not in-flight also returns false.
#[tokio::test]
async fn cancel_unknown_string_id_returns_false() {
    let mut h = Harness::new(fixture_dir()).await;
    h.handshake().await;

    h.send_raw(r#"{"jsonrpc":"2.0","id":31,"method":"runtime.cancel","params":{"id":"not-here"}}"#)
        .await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 31);
    assert_eq!(
        resp["result"]["cancelled"], false,
        "expected cancelled=false, got: {resp}"
    );
}

/// `runtime.cancel` with a missing `params.id` returns -32602 INVALID_PARAMS.
#[tokio::test]
async fn cancel_missing_id_param_returns_32602() {
    let mut h = Harness::new(fixture_dir()).await;
    h.handshake().await;

    h.send_raw(r#"{"jsonrpc":"2.0","id":32,"method":"runtime.cancel","params":{}}"#)
        .await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 32);
    assert_eq!(
        resp["error"]["code"], -32602,
        "expected INVALID_PARAMS, got: {resp}"
    );
}

/// `runtime.cancel` with a params.id that is not an int or string returns
/// -32602 INVALID_PARAMS (e.g. an object, which doesn't parse as RequestId).
#[tokio::test]
async fn cancel_invalid_id_type_returns_32602() {
    let mut h = Harness::new(fixture_dir()).await;
    h.handshake().await;

    // `{"id": [1,2,3]}` — array is not a valid RequestId.
    h.send_raw(r#"{"jsonrpc":"2.0","id":33,"method":"runtime.cancel","params":{"id":[1,2,3]}}"#)
        .await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 33);
    assert_eq!(
        resp["error"]["code"], -32602,
        "expected INVALID_PARAMS, got: {resp}"
    );
}

/// `runtime.cancel` before handshake returns -32002 HANDSHAKE_REQUIRED.
/// Even cancel is a runtime-level method that requires prior handshake.
#[tokio::test]
async fn cancel_before_handshake_returns_32002() {
    let mut h = Harness::new(fixture_dir()).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":34,"method":"runtime.cancel","params":{"id":1}}"#)
        .await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 34);
    assert_eq!(
        resp["error"]["code"], -32002,
        "expected HANDSHAKE_REQUIRED, got: {resp}"
    );
}
