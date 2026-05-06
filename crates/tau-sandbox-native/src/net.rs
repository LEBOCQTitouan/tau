//! Per-host network filtering helpers for the Strict sandbox tier.
//!
//! # Sub-project F — per-host egress filtering
//!
//! Two decisions are made at plan-evaluation time (parent process, before fork):
//!
//! 1. **Unshare flags** (`unshare_flags_for_plan`):
//!    - Always returns `CLONE_NEWUSER | CLONE_NEWNET` for all plans. The child
//!      is placed in an isolated network namespace; sub-project F's
//!      `net_filter::apply_per_host_filter` then configures a veth pair and
//!      nftables ruleset inside the child netns to allow only the declared hosts.
//!
//! 2. **Seccomp socket syscalls** (`extend_with_network_rules`):
//!    - No `Capability::Network(Http)` → `SYS_socket`, `SYS_connect`,
//!      `SYS_getpeername`, `SYS_getsockname` are **absent** from the baseline
//!      allow-list, so seccomp `KillProcess` fires on any socket attempt.
//!    - `Capability::Network(Http)` present → those 4 client-side syscalls are
//!      added to the allow-list so HTTP clients can open TCP connections.
//!      Server-side syscalls (`SYS_bind`, `SYS_listen`, `SYS_accept`,
//!      `SYS_accept4`) are intentionally omitted.

use std::collections::BTreeMap;

use nix::sched::CloneFlags;
use seccompiler::SeccompRule;
use tau_domain::{Capability, NetCapability};
use tau_ports::SandboxPlan;

/// Returns the flags to pass to `unshare(2)` for the given plan.
///
/// Always includes both `CLONE_NEWUSER` (mandatory for unprivileged
/// sandboxing) and `CLONE_NEWNET` (per-host filtering enforces network
/// isolation via the new netns; sub-project F replaces the v0.1 fallback
/// that stripped CLONE_NEWNET when Network(Http) was in the plan).
pub(crate) fn unshare_flags_for_plan(plan: &SandboxPlan) -> CloneFlags {
    let _ = plan;
    CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNET
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
        rules.entry(nr).or_default();
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;
    use crate::strict::baseline_syscall_map;

    // ---- unshare_flags_for_plan tests ----

    /// An empty plan (no capabilities) should yield both CLONE_NEWUSER and
    /// CLONE_NEWNET (sub-project F: always isolate into a fresh netns).
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

    /// A plan with Network(Http) now also gets CLONE_NEWNET. Sub-project F's
    /// apply_per_host_filter configures the netns with a veth pair + nftables
    /// rather than inheriting the parent netns.
    #[test]
    fn unshare_flags_with_http_includes_newnet() {
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
            flags.contains(CloneFlags::CLONE_NEWNET),
            "CLONE_NEWNET must be present (sub-project F: isolated netns for all plans)"
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
