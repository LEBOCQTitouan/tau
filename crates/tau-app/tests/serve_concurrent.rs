//! Layer 2 — max_concurrent cap tests.
//!
//! Tests the server-busy path when `max_concurrent_runs` is exhausted:
//!   - With `max_concurrent = 1` and one token pre-registered in the
//!     cancel registry, a second `runtime.run` returns -32004 SERVER_BUSY.
//!   - Ditto for `runtime.run_streaming`.
//!   - After the in-flight token is removed, the next call is not rejected.
//!
//! The pre-registration technique is deterministic: rather than relying on
//! timing to keep a real run in-flight, we exploit the fact that
//! `Dispatcher::cancel_reg` is shared with the Harness (both hold a clone of
//! the same `Arc<DashMap>` inside `CancelRegistry`). We register a dummy
//! cancellation token to simulate a saturated slot, verify the cap fires,
//! then remove it and verify the cap no longer fires.

mod common;
use common::Harness;
use std::path::PathBuf;
use tau_app::serve::RequestId;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/handshake-only")
}

/// With `max_concurrent = 1` and one dummy in-flight token registered,
/// `runtime.run` returns -32004 SERVER_BUSY.
#[tokio::test]
async fn run_rejected_when_cap_reached() {
    let mut h = Harness::with_options(fixture_dir(), 1).await;
    h.handshake().await;

    // Simulate one in-flight run by pre-registering a token.
    let _tok = h.cancel_reg.register(RequestId::Int(100));

    h.send_raw(r#"{"jsonrpc":"2.0","id":40,"method":"runtime.run","params":{"agent":"any","prompt":"hello"}}"#).await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 40);
    assert_eq!(
        resp["error"]["code"], -32004,
        "expected SERVER_BUSY, got: {resp}"
    );
    assert!(
        resp["error"]["data"]["max_concurrent"].as_u64() == Some(1),
        "expected max_concurrent=1 in error data, got: {resp}"
    );
}

/// Same check but for `runtime.run_streaming`.
#[tokio::test]
async fn streaming_rejected_when_cap_reached() {
    let mut h = Harness::with_options(fixture_dir(), 1).await;
    h.handshake().await;

    let _tok = h.cancel_reg.register(RequestId::Int(101));

    h.send_raw(r#"{"jsonrpc":"2.0","id":41,"method":"runtime.run_streaming","params":{"agent":"any","prompt":"hello"}}"#).await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 41);
    assert_eq!(
        resp["error"]["code"], -32004,
        "expected SERVER_BUSY, got: {resp}"
    );
}

/// After the in-flight token is removed from the registry, the next
/// `runtime.run` is no longer rejected by the cap — it proceeds to the
/// unknown-agent check (returns -32010 instead of -32004).
#[tokio::test]
async fn run_allowed_after_slot_freed() {
    let mut h = Harness::with_options(fixture_dir(), 1).await;
    h.handshake().await;

    // Register then immediately cancel (removes from registry).
    let dummy_id = RequestId::Int(102);
    let _tok = h.cancel_reg.register(dummy_id.clone());
    h.cancel_reg.forget(&dummy_id);

    // Now the slot is free — the run should reach the unknown-agent check.
    h.send_raw(r#"{"jsonrpc":"2.0","id":42,"method":"runtime.run","params":{"agent":"no-such-agent","prompt":"hello"}}"#).await;
    let resp = h.recv().await.expect("no response");
    assert_eq!(resp["id"], 42);
    // Should be UNKNOWN_AGENT (-32010), not SERVER_BUSY (-32004).
    assert_eq!(
        resp["error"]["code"], -32010,
        "expected UNKNOWN_AGENT after slot freed, got: {resp}"
    );
}
