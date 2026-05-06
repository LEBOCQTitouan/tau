//! Sub-project F Task 6 — net-filter e2e integration tests (stubs).
//!
//! These tests are `#[ignore]`'d pending sub-project F task 6.5: the strict.rs
//! post-spawn hook for net-filter integration. See
//! `crates/tau-sandbox-native/src/net_filter/INTEGRATION.md` for the
//! architectural options (α/β/γ) and the plan for wiring the hook.
//!
//! Once F task 6.5 is complete:
//! - Create the sync pipe in `apply_strict`.
//! - Call `net_filter::apply_per_host_filter(plan, child.id())` in the runtime
//!   after `cmd.spawn()`.
//! - Write 1 byte to the sync pipe.
//! - Remove `#[ignore]` from these tests.

#![cfg(feature = "integration-tests")]
#![cfg(target_os = "linux")]

/// Stub: sandbox with Network(Http) lets the child open a socket to the
/// declared host (localhost loopback).
///
/// When F task 6.5 is complete:
/// - `apply_per_host_filter` configures a veth pair + nftables in the child
///   netns allowing 127.0.0.1.
/// - The child's `socket()` call succeeds.
/// - The child's `connect()` to 127.0.0.1:<port> reaches the parent-side
///   listener bound on the parent veth IP.
///
/// Ignored pending sub-project F task 6.5 — strict.rs post-spawn hook for
/// net-filter integration.
#[tokio::test]
#[ignore = "pending sub-project F task 6.5: strict.rs post-spawn hook for net-filter integration (see net_filter/INTEGRATION.md)"]
async fn localhost_socket_allowed_with_http_cap() {
    todo!("F task 6.5: wire apply_per_host_filter into the spawn lifecycle, then assert SOCKET_OK")
}

/// Stub: sandbox with Network(Http) for an external host lets the child open
/// a socket to the resolved IPs.
///
/// When F task 6.5 is complete:
/// - `apply_per_host_filter` resolves `api.example.com` and installs nftables
///   rules allowing the resolved IPs.
/// - The child's `socket()` + `connect()` to those IPs succeeds.
///
/// Ignored pending sub-project F task 6.5 — strict.rs post-spawn hook for
/// net-filter integration.
#[tokio::test]
#[ignore = "pending sub-project F task 6.5: strict.rs post-spawn hook for net-filter integration (see net_filter/INTEGRATION.md)"]
async fn external_host_socket_allowed_with_http_cap() {
    todo!("F task 6.5: wire apply_per_host_filter into the spawn lifecycle, then assert SOCKET_OK")
}

/// Stub: sandbox WITHOUT Network(Http) must deny socket() calls (seccomp
/// KillProcess fires on SYS_socket).
///
/// When F task 6.5 is complete this test exercises the existing behavior:
/// - `unshare_flags_for_plan` returns CLONE_NEWUSER | CLONE_NEWNET.
/// - No `apply_per_host_filter` call (noop handle).
/// - Child's `socket()` → seccomp KillProcess → process killed.
///
/// Ignored pending sub-project F task 6.5 — strict.rs post-spawn hook for
/// net-filter integration.
#[tokio::test]
#[ignore = "pending sub-project F task 6.5: strict.rs post-spawn hook for net-filter integration (see net_filter/INTEGRATION.md)"]
async fn no_network_cap_socket_denied_by_seccomp() {
    todo!("F task 6.5: verify seccomp kills child on socket() when no Network(Http) cap")
}

/// Stub: NetFilterHandle Drop removes the parent veth when the sandbox exits.
///
/// When F task 6.5 is complete:
/// - Capture the parent veth name from the handle.
/// - Drop the handle (or let the sandbox exit).
/// - Assert `ip link show <veth>` returns non-zero (interface deleted).
///
/// Ignored pending sub-project F task 6.5 — strict.rs post-spawn hook for
/// net-filter integration.
#[tokio::test]
#[ignore = "pending sub-project F task 6.5: strict.rs post-spawn hook for net-filter integration (see net_filter/INTEGRATION.md)"]
async fn net_filter_handle_drop_removes_parent_veth() {
    todo!("F task 6.5: assert ip link del runs on NetFilterHandle drop after plugin exits")
}
