//! Integration tests for the Strict tier seccomp BPF filter.
//!
//! These tests require:
//! 1. Linux with landlock V1 (kernel >= 5.13).
//! 2. `--features integration-tests` passed to `cargo test`.
//! 3. Unprivileged user namespaces enabled (check via `probe::user_ns_supported`).
//!
//! Run with:
//! ```sh
//! cargo test -p tau-sandbox-native --features integration-tests -- --include-ignored
//! ```

#![cfg(all(target_os = "linux", feature = "integration-tests"))]

use std::process::Command;
use tau_ports::{SandboxPlan, SandboxTier};

/// Helper: build a minimal plan with no capabilities.
fn empty_plan() -> SandboxPlan {
    let json = serde_json::json!({
        "capabilities": [],
        "context": null,
        "limits": null,
    });
    serde_json::from_value(json).expect("valid plan")
}

/// Strict tier should block `socket(2)` (not in baseline allow-list).
///
/// Spawns `python3` directly (no shell wrapper) to invoke `socket.socket()`.
/// When seccomp `KillProcess` fires, python3 is killed with SIGSYS (signal 31)
/// and `cmd.status()` returns a non-zero exit, so `!status.success()` holds for
/// the right reason.
///
/// Requires `python3` on PATH. This is acceptable since the test is `#[ignore]`-gated
/// and only runs on Linux with `--features integration-tests`.
#[test]
#[ignore = "requires Linux + integration-tests feature + unprivileged user namespaces + python3 on PATH"]
fn strict_blocks_socket() {
    use tau_ports::Sandbox;
    use tau_sandbox_native::NativeSandbox;

    // Use a plan with no capabilities — baseline allow-list, no socket.
    let plan = empty_plan();
    let sandbox = NativeSandbox::new("test-strict-socket", SandboxTier::Strict);

    // Spawn python3 directly — no shell wrapper, no `|| true` that would swallow the
    // non-zero exit. When seccomp KillProcess fires on socket(2), python3 is killed
    // (SIGSYS/signal 31) and status.success() returns false.
    let mut cmd = Command::new("python3");
    cmd.args(["-c", "import socket; socket.socket()"]);

    // apply_strict installs the pre_exec hook.
    // We call the internal function directly via the public NativeSandbox::wrap_spawn.
    // Since wrap_spawn is async, we need a minimal runtime.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let _handle = rt
        .block_on(sandbox.wrap_spawn(&plan, &mut cmd))
        .expect("wrap_spawn succeeds");

    let status = cmd.status().expect("spawn succeeds");
    // The child was killed by seccomp (SIGSYS / signal 31).
    assert!(
        !status.success(),
        "process should have been killed by seccomp when calling socket(2); \
         got exit status: {status:?}"
    );
}

/// The child cannot call `unshare(CLONE_NEWUSER)` again after seccomp is installed,
/// because `unshare(2)` is not in the baseline allow-list.
///
/// TODO(task-5): Tighten to verify specifically that the `unshare` syscall is
/// blocked rather than relying on a shell command that may fail for other reasons.
#[test]
#[ignore = "requires Linux + integration-tests feature + unprivileged user namespaces"]
fn strict_blocks_unshare_recursion() {
    use tau_ports::Sandbox;
    use tau_sandbox_native::NativeSandbox;

    let plan = empty_plan();
    let sandbox = NativeSandbox::new("test-strict-unshare", SandboxTier::Strict);

    // unshare(1) is the userspace command that calls unshare(2).
    // It will be blocked by seccomp after the filter is installed.
    let mut cmd = Command::new("/usr/bin/unshare");
    cmd.args(["--user", "/bin/true"]);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let _handle = rt
        .block_on(sandbox.wrap_spawn(&plan, &mut cmd))
        .expect("wrap_spawn succeeds");

    let status = cmd.status().expect("spawn succeeds");
    assert!(
        !status.success(),
        "unshare(CLONE_NEWUSER) should be blocked by seccomp; \
         got exit status: {status:?}"
    );
}
