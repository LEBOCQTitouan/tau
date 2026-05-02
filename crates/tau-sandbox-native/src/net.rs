//! Per-host network filtering helpers for the Strict sandbox tier.
//!
//! # v0.1 status — per-host filtering not yet implemented
//!
//! True per-host egress filtering (e.g. nftables rules inside an isolated
//! network namespace) is **deferred** from v0.1. The reasons are:
//!
//! - Setting up nftables inside a fresh user-namespaced netns requires
//!   `CAP_NET_ADMIN` inside the namespace; acquiring and using it correctly
//!   is non-trivial and introduces portability risks across Linux distributions
//!   and kernel versions.
//! - The nftables API is not exposed through a stable Rust crate with the
//!   maturity required for Phase 0 work.
//!
//! # What this module does today
//!
//! Two decisions are made at plan-evaluation time (parent process, before fork):
//!
//! 1. **Unshare flags** (`unshare_flags_for_plan`):
//!    - No `Capability::Network(Http)` in plan → `CLONE_NEWUSER | CLONE_NEWNET`:
//!      child gets an isolated netns with no interfaces; all egress fails.
//!    - `Capability::Network(Http)` present → `CLONE_NEWUSER` only (no `CLONE_NEWNET`):
//!      child inherits the parent's network namespace; full egress is available.
//!      A `tracing::warn!` is emitted to signal this v0.1 over-permissiveness.
//!
//! 2. **Seccomp socket syscalls** (`extend_with_network_rules`):
//!    - No `Capability::Network(Http)` → `SYS_socket`, `SYS_connect`,
//!      `SYS_getpeername`, `SYS_getsockname` are **absent** from the baseline
//!      allow-list, so seccomp `KillProcess` fires on any socket attempt.
//!    - `Capability::Network(Http)` present → those 4 client-side syscalls are
//!      added to the allow-list so HTTP clients can open TCP connections.
//!      Server-side syscalls (`SYS_bind`, `SYS_listen`, `SYS_accept`,
//!      `SYS_accept4`) are intentionally omitted.
//!
//! # TODO(future) — real per-host egress filtering (Phase 2)
//!
//! - Create a new netns via `unshare(CLONE_NEWNET)` for ALL plans (including those
//!   with `Network(Http)`).
//! - Inside the netns, configure a loopback + veth pair for outbound traffic.
//! - Install nftables rules that allow egress only to the hosts listed in the
//!   `Http { hosts }` capability.
//! - Wire `SYS_socket`/`SYS_connect` additions to the allow-list behind the same
//!   capability check (already done in v0.1).
//! - Remove the `tracing::warn!` once real filtering is in place.

use std::collections::BTreeMap;
use std::sync::Once;

use nix::sched::CloneFlags;
use seccompiler::SeccompRule;
use tau_domain::{Capability, NetCapability};
use tau_ports::SandboxPlan;

/// Return the `unshare(2)` flags appropriate for the plan's network capabilities.
///
/// - If the plan has **any** `Capability::Network(Http)` capability, returns
///   `CLONE_NEWUSER` only. The child inherits the parent's netns so HTTP
///   clients can reach external hosts. A warning is logged because this is
///   over-permissive (no per-host filtering yet).
/// - Otherwise, returns `CLONE_NEWUSER | CLONE_NEWNET`. The child is placed in
///   an empty netns with no interfaces; all network egress fails.
///
/// # v0.1 limitation
///
/// The `Network(Http)` path grants full parent netns access, not filtered
/// per-host access. Per-host nftables-based filtering is deferred to Phase 2.
pub(crate) fn unshare_flags_for_plan(plan: &SandboxPlan) -> CloneFlags {
    let has_http = plan
        .capabilities
        .iter()
        .any(|c| matches!(c, Capability::Network(NetCapability::Http { .. })));

    if has_http {
        static V01_NETNS_WARNING: Once = Once::new();
        V01_NETNS_WARNING.call_once(|| {
            tracing::warn!(
                "v0.1 limitation: plan has Network(Http) capability; \
                 child inherits parent netns (no per-host filtering). \
                 Phase 2 will add nftables-based egress filtering."
            );
        });
        CloneFlags::CLONE_NEWUSER
    } else {
        CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNET
    }
}

/// Extend the seccomp rules map with network-related syscalls for the given plan.
///
/// If the plan contains **any** `Capability::Network(Http)` capability, the
/// following syscalls are added to the allow-list:
/// - `SYS_socket`, `SYS_connect` — open TCP/UDP sockets and connect to peers.
/// - `SYS_getpeername`, `SYS_getsockname` — query socket addresses.
///
/// `SYS_bind`, `SYS_listen`, `SYS_accept`, and `SYS_accept4` are intentionally
/// **absent**: `Network(Http)` is a CLIENT capability; server-side syscalls are
/// not needed. If a future plugin requires server behaviour, add a new capability
/// variant (e.g. `NetCapability::HttpServer`) with its own extension function.
///
/// If the plan has no network capability, these syscalls are **not** added; the
/// baseline already excludes them, so any socket attempt triggers seccomp
/// `KillProcess`.
///
/// Note: `sendto`/`recvfrom`/`sendmsg`/`recvmsg` are in the baseline (needed for
/// plugin IPC) and therefore available to `Network(Http)` plans, covering UDP-
/// based transports (DNS resolution, HTTP/3).
///
/// # Arguments
///
/// - `rules` — mutable reference to the rules map built by `baseline_syscall_map`.
/// - `plan` — the sandbox plan to inspect for network capabilities.
pub(crate) fn extend_with_network_rules(
    rules: &mut BTreeMap<i64, Vec<SeccompRule>>,
    plan: &SandboxPlan,
) {
    let has_http = plan
        .capabilities
        .iter()
        .any(|c| matches!(c, Capability::Network(NetCapability::Http { .. })));

    if !has_http {
        return;
    }

    // Allow socket-family syscalls needed by HTTP clients.
    // Server-side syscalls (bind, listen, accept, accept4) are intentionally omitted.
    let net_syscalls: &[i64] = &[
        libc::SYS_socket,
        libc::SYS_connect,
        libc::SYS_getpeername,
        libc::SYS_getsockname,
    ];

    for &nr in net_syscalls {
        rules.entry(nr).or_insert_with(Vec::new);
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;
    use crate::strict::baseline_syscall_map;

    // ---- unshare_flags_for_plan tests ----

    /// An empty plan (no capabilities) should yield both CLONE_NEWUSER and
    /// CLONE_NEWNET, isolating the child into an empty netns.
    #[test]
    fn unshare_flags_default_no_network() {
        let plan_json = serde_json::json!({
            "capabilities": [],
            "context": null,
            "limits": null,
        });
        let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");
        let flags = unshare_flags_for_plan(&plan);
        assert!(
            flags.contains(CloneFlags::CLONE_NEWUSER),
            "must include CLONE_NEWUSER"
        );
        assert!(
            flags.contains(CloneFlags::CLONE_NEWNET),
            "must include CLONE_NEWNET for empty plan"
        );
    }

    /// A plan with Network(Http) should yield only CLONE_NEWUSER (no CLONE_NEWNET),
    /// so the child inherits the parent's netns.
    #[test]
    fn unshare_flags_with_http_drops_newnet() {
        let plan_json = serde_json::json!({
            "capabilities": [{
                "kind": "net.http",
                "hosts": ["api.example.com"],
                "methods": ["GET"],
            }],
            "context": null,
            "limits": null,
        });
        let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");
        let flags = unshare_flags_for_plan(&plan);
        assert!(
            flags.contains(CloneFlags::CLONE_NEWUSER),
            "must include CLONE_NEWUSER"
        );
        assert!(
            !flags.contains(CloneFlags::CLONE_NEWNET),
            "CLONE_NEWNET must be absent when Network(Http) is present"
        );
    }

    /// A plan with only fs.read (no network capability) should yield both
    /// CLONE_NEWUSER and CLONE_NEWNET.
    #[test]
    fn unshare_flags_no_network_capability() {
        let plan_json = serde_json::json!({
            "capabilities": [{ "kind": "fs.read", "paths": ["/tmp"] }],
            "context": null,
            "limits": null,
        });
        let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");
        let flags = unshare_flags_for_plan(&plan);
        assert!(
            flags.contains(CloneFlags::CLONE_NEWUSER),
            "must include CLONE_NEWUSER"
        );
        assert!(
            flags.contains(CloneFlags::CLONE_NEWNET),
            "must include CLONE_NEWNET when no network capability is present"
        );
    }

    // ---- extend_with_network_rules tests ----

    /// When the plan has a Network(Http) capability, socket-family syscalls
    /// must be added to the rules map.
    #[test]
    fn extend_adds_socket_when_http_capability_present() {
        let plan_json = serde_json::json!({
            "capabilities": [{
                "kind": "net.http",
                "hosts": ["api.example.com"],
                "methods": ["GET"],
            }],
            "context": null,
            "limits": null,
        });
        let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");

        let mut rules = baseline_syscall_map();
        // Baseline must NOT contain socket before extension (verified by Task 4 tests).
        assert!(
            !rules.contains_key(&libc::SYS_socket),
            "precondition: SYS_socket absent in baseline"
        );

        extend_with_network_rules(&mut rules, &plan);

        assert!(
            rules.contains_key(&libc::SYS_socket),
            "SYS_socket must be present after extension"
        );
        assert!(
            rules.contains_key(&libc::SYS_connect),
            "SYS_connect must be present after extension"
        );
        assert!(
            rules.contains_key(&libc::SYS_getpeername),
            "SYS_getpeername must be present after extension"
        );
        assert!(
            rules.contains_key(&libc::SYS_getsockname),
            "SYS_getsockname must be present after extension"
        );

        // Server-side syscalls must NOT be added by extend_with_network_rules.
        assert!(
            !rules.contains_key(&libc::SYS_bind),
            "SYS_bind must NOT be present (server-side; not needed for HTTP client)"
        );
        assert!(
            !rules.contains_key(&libc::SYS_listen),
            "SYS_listen must NOT be present (server-side; not needed for HTTP client)"
        );
        assert!(
            !rules.contains_key(&libc::SYS_accept),
            "SYS_accept must NOT be present (server-side; not needed for HTTP client)"
        );
        assert!(
            !rules.contains_key(&libc::SYS_accept4),
            "SYS_accept4 must NOT be present (server-side; not needed for HTTP client)"
        );
    }

    /// When the plan has no network capability, extend_with_network_rules is a
    /// no-op: socket-family syscalls remain absent from the rules map.
    #[test]
    fn extend_no_op_when_no_http_capability() {
        let plan_json = serde_json::json!({
            "capabilities": [{ "kind": "fs.read", "paths": ["/tmp"] }],
            "context": null,
            "limits": null,
        });
        let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");

        let mut rules = baseline_syscall_map();
        let snapshot_before: Vec<i64> = rules.keys().copied().collect();

        extend_with_network_rules(&mut rules, &plan);

        let snapshot_after: Vec<i64> = rules.keys().copied().collect();
        assert_eq!(
            snapshot_before, snapshot_after,
            "extend_with_network_rules must be a no-op when no Network(Http) capability is present"
        );
        assert!(
            !rules.contains_key(&libc::SYS_socket),
            "SYS_socket must remain absent when no network capability"
        );
    }
}
