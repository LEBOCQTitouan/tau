//! End-to-end native sandbox integration tests.
//!
//! Linux-only. Run with:
//!   cargo test -p tau-runtime --features integration-tests -- --ignored

#![cfg(all(target_os = "linux", feature = "integration-tests"))]

use std::process::Command;
use tau_ports::{Sandbox, SandboxPlan, SandboxTier};
use tau_sandbox_native::NativeSandbox;
use tempfile::TempDir;

fn plan_for_read(path: &std::path::Path) -> SandboxPlan {
    let plan_json = serde_json::json!({
        "capabilities": [{
            "kind": "fs.read",
            "paths": [path.to_string_lossy()]
        }],
        "context": null,
        "limits": null,
    });
    serde_json::from_value(plan_json).expect("decode plan")
}

#[tokio::test]
#[ignore = "requires Linux + integration-tests feature + landlock (kernel >= 5.13)"]
async fn fs_read_plugin_reads_allowed_path() {
    let allowed = TempDir::new().unwrap();
    let target = allowed.path().join("ok.txt");
    std::fs::write(&target, b"hello").unwrap();

    let s = NativeSandbox::new("native", SandboxTier::Light);
    let plan = plan_for_read(allowed.path());

    let mut cmd = Command::new("/bin/cat");
    cmd.arg(&target);
    let _h = s.wrap_spawn(&plan, &mut cmd).await.unwrap();
    let status = cmd.status().unwrap();
    assert!(status.success(), "allowed read should succeed");
}

#[tokio::test]
#[ignore = "requires Linux + integration-tests feature + landlock (kernel >= 5.13)"]
async fn fs_read_plugin_rejected_for_unlisted_path() {
    let allowed = TempDir::new().unwrap();
    let blocked = TempDir::new().unwrap();
    let target = blocked.path().join("nope.txt");
    std::fs::write(&target, b"secret").unwrap();

    let s = NativeSandbox::new("native", SandboxTier::Light);
    let plan = plan_for_read(allowed.path());

    let mut cmd = Command::new("/bin/cat");
    cmd.arg(&target);
    let _h = s.wrap_spawn(&plan, &mut cmd).await.unwrap();
    let status = cmd.status().unwrap();
    assert!(!status.success(), "unlisted read should be denied");
}

#[tokio::test]
#[ignore = "requires Linux + integration-tests feature + unprivileged user namespaces"]
async fn shell_plugin_spawns_allowed_command_under_strict() {
    // Strict tier: tests the seccomp + landlock + namespaces stack
    // end-to-end. /bin/echo is a tiny binary that should run under
    // strict tier with default plan (no caps needed).
    let s = NativeSandbox::new("native", SandboxTier::Strict);
    let plan = SandboxPlan::new(vec![], None, None);

    let mut cmd = Command::new("/bin/echo");
    cmd.arg("hello");
    let _h = s.wrap_spawn(&plan, &mut cmd).await.unwrap();
    let status = cmd.status().unwrap();
    assert!(status.success(), "echo under strict tier should succeed");
}

#[tokio::test]
#[ignore = "requires Linux + integration-tests feature + landlock (kernel >= 5.13)"]
async fn fs_write_plugin_writes_to_allowed_path() {
    let allowed = TempDir::new().unwrap();
    let target = allowed.path().join("written.txt");

    let plan_json = serde_json::json!({
        "capabilities": [{
            "kind": "fs.write",
            "paths": [allowed.path().to_string_lossy()]
        }],
        "context": null,
        "limits": null,
    });
    let plan: SandboxPlan = serde_json::from_value(plan_json).expect("decode plan");

    let s = NativeSandbox::new("native", SandboxTier::Light);
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", &format!("echo hello > {}", target.to_string_lossy())]);
    let _h = s.wrap_spawn(&plan, &mut cmd).await.unwrap();
    let status = cmd.status().unwrap();
    assert!(status.success(), "allowed write should succeed");
}
