//! Integration tests for Strict-tier per-command exec gating.
//!
//! # v0.1 status
//!
//! Per-command exec gating via seccomp argument filtering is deferred in v0.1.
//! The seccomp baseline always allows `execve`/`execveat` so the plugin's own
//! initial exec can succeed. See `exec.rs` for the full rationale.
//!
//! These tests therefore verify that:
//! 1. `apply_strict` compiles and returns a `SandboxHandle` successfully for a
//!    plan with exec-related capabilities (smoke test).
//! 2. The strict tier allows `execve` at the seccomp layer in all cases.
//!
//! A TODO is left for the future tightening work (landlock V2 Execute + seccomp-notify).
//!
//! # TODO(future) — per-command exec tightening integration tests
//!
//! Once landlock V2 Execute wiring is added:
//! - `strict_blocks_exec_outside_allowed_paths` — plan with `Process(Spawn { commands: [] })`
//!   (empty allow-list); executing a binary outside the landlock allow-list should fail.
//! - `strict_allows_exec_within_allowed_paths` — plan with `Filesystem(Exec { paths: ["/bin/echo"] })`;
//!   executing `/bin/echo hello` should succeed.
//!
//! Run with:
//! ```sh
//! cargo test -p tau-sandbox-native --features integration-tests -- --include-ignored
//! ```

#![cfg(all(target_os = "linux", feature = "integration-tests"))]

use std::process::Command;
use tau_ports::{Sandbox, SandboxPlan, SandboxTier};
use tau_sandbox_native::NativeSandbox;

/// `apply_strict` must succeed (not panic or return Err) for a plan with
/// `Process(Spawn)` capability. This is a parent-side smoke test: it does
/// NOT spawn the command, only installs the pre_exec hook.
#[test]
#[ignore = "requires Linux + integration-tests feature"]
fn strict_apply_succeeds_with_process_spawn_capability() {
    let plan_json = serde_json::json!({
        "capabilities": [
            { "kind": "process.spawn", "commands": ["echo"] },
        ],
        "context": null,
        "limits": null,
    });
    let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");
    let sandbox = NativeSandbox::new("test-exec-spawn", SandboxTier::Strict);

    let mut cmd = Command::new("/bin/echo");
    cmd.arg("hello");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // wrap_spawn must not return an error (BPF compilation succeeds).
    let _handle = rt
        .block_on(sandbox.wrap_spawn(&plan, &mut cmd))
        .expect("apply_strict must succeed with Process(Spawn) capability");
}

/// `apply_strict` must succeed for a plan with `Filesystem(Exec)` capability.
#[test]
#[ignore = "requires Linux + integration-tests feature"]
fn strict_apply_succeeds_with_fs_exec_capability() {
    let plan_json = serde_json::json!({
        "capabilities": [
            { "kind": "fs.exec", "paths": ["/bin/echo"] },
        ],
        "context": null,
        "limits": null,
    });
    let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");
    let sandbox = NativeSandbox::new("test-exec-fs", SandboxTier::Strict);

    let mut cmd = Command::new("/bin/echo");
    cmd.arg("hello");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let _handle = rt
        .block_on(sandbox.wrap_spawn(&plan, &mut cmd))
        .expect("apply_strict must succeed with Filesystem(Exec) capability");
}

/// When no exec-related capability is in the plan, `apply_strict` must still
/// succeed (execve allowed for plugin startup; see exec.rs v0.1 rationale).
#[test]
#[ignore = "requires Linux + integration-tests feature"]
fn strict_apply_succeeds_with_no_exec_capability() {
    let plan_json = serde_json::json!({
        "capabilities": [
            { "kind": "fs.read", "paths": ["/tmp"] },
        ],
        "context": null,
        "limits": null,
    });
    let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");
    let sandbox = NativeSandbox::new("test-exec-noexec", SandboxTier::Strict);

    let mut cmd = Command::new("/bin/true");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let _handle = rt.block_on(sandbox.wrap_spawn(&plan, &mut cmd)).expect(
        "apply_strict must succeed even with no exec capability (execve allowed for startup)",
    );
}
