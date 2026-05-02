//! Integration tests for Strict-tier per-host network filtering.
//!
//! These tests verify that:
//! 1. A plan with no `Network(Http)` capability → socket syscalls are blocked by
//!    seccomp (`KillProcess` fires; process exits non-zero).
//! 2. A plan with `Network(Http)` capability → socket syscalls are allowed by
//!    seccomp (the process can call `socket()` and exit zero).
//!
//! # v0.1 limitation
//!
//! The `Network(Http)` path grants full parent netns access (no per-host
//! filtering). See `net.rs` for the full rationale and the Phase 2 deferred work.
//!
//! Run with:
//! ```sh
//! cargo test -p tau-sandbox-native --features integration-tests -- --include-ignored
//! ```

#![cfg(all(target_os = "linux", feature = "integration-tests"))]

use std::process::Command;
use tau_ports::{Sandbox, SandboxPlan, SandboxTier};
use tau_sandbox_native::NativeSandbox;

/// A plan with no network capability: `socket(2)` must be blocked by seccomp.
///
/// Spawns `python3 -c 'import socket; socket.socket()'` under Strict with a
/// plan that has only `fs.read`. When seccomp `KillProcess` fires on
/// `socket(AF_INET, SOCK_DGRAM, 0)`, python3 is killed with SIGSYS (signal 31)
/// and `cmd.status()` returns a non-zero exit code.
///
/// Requires `python3` on PATH.
#[test]
#[ignore = "requires Linux + integration-tests feature + unprivileged user namespaces + python3 on PATH"]
fn strict_blocks_socket_when_no_network_capability() {
    let plan_json = serde_json::json!({
        "capabilities": [
            { "kind": "fs.read", "paths": ["/tmp"] },
        ],
        "context": null,
        "limits": null,
    });
    let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");
    let sandbox = NativeSandbox::new("test-net-block", SandboxTier::Strict);

    let mut cmd = Command::new("python3");
    cmd.args(["-c", "import socket; socket.socket()"]);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let _handle = rt
        .block_on(sandbox.wrap_spawn(&plan, &mut cmd))
        .expect("wrap_spawn must succeed");

    let status = cmd.status().expect("spawn must succeed");
    assert!(
        !status.success(),
        "socket(2) must be blocked by seccomp when plan has no Network(Http) capability; \
         got exit status: {status:?}"
    );
}

/// A plan with `Network(Http)` capability: `socket(2)` must be allowed by seccomp.
///
/// Spawns `python3 -c 'import socket; socket.socket()'` under Strict with a
/// plan that has `Network(Http)`. The socket syscall must be in the allow-list;
/// python3 should exit zero.
///
/// # v0.1 note
///
/// The child inherits the parent's netns (no `CLONE_NEWNET`). There is no
/// per-host filtering. The test only asserts that seccomp allows `socket()`.
///
/// Requires `python3` on PATH.
#[test]
#[ignore = "requires Linux + integration-tests feature + unprivileged user namespaces + python3 on PATH"]
fn strict_allows_socket_when_network_capability() {
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
    let sandbox = NativeSandbox::new("test-net-allow", SandboxTier::Strict);

    let mut cmd = Command::new("python3");
    cmd.args(["-c", "import socket; socket.socket()"]);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let _handle = rt
        .block_on(sandbox.wrap_spawn(&plan, &mut cmd))
        .expect("wrap_spawn must succeed");

    let status = cmd.status().expect("spawn must succeed");
    assert!(
        status.success(),
        "socket(2) must be allowed when plan has Network(Http) capability; \
         got exit status: {status:?}"
    );
}
