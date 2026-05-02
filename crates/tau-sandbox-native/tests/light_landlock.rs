//! Real landlock integration. Linux-only; gated `#[ignore]` so the standard
//! `cargo test` ignores it. Run with:
//!   `cargo test -p tau-sandbox-native --features integration-tests -- --ignored`

#![cfg(all(target_os = "linux", feature = "integration-tests"))]

use std::process::Command;
use tau_ports::{Sandbox, SandboxPlan, SandboxTier};
use tau_sandbox_native::NativeSandbox;
use tempfile::TempDir;

#[tokio::test]
#[ignore]
async fn landlock_blocks_unlisted_path() {
    let allowed = TempDir::new().unwrap();
    let blocked = TempDir::new().unwrap();

    let s = NativeSandbox::new("native", SandboxTier::Light);

    // Use JSON round-trip to construct a Capability::Filesystem(Read).
    let plan_json = serde_json::json!({
        "capabilities": [{
            "kind": "fs.read",
            "paths": [allowed.path().to_string_lossy()],
        }],
        "context": null,
        "limits": null,
    });
    let plan: SandboxPlan = serde_json::from_value(plan_json).expect("decode plan");

    // /bin/cat against an allowed path: should succeed.
    {
        let allowed_file = allowed.path().join("ok.txt");
        std::fs::write(&allowed_file, b"hello").unwrap();
        let mut cmd = Command::new("/bin/cat");
        cmd.arg(&allowed_file);
        let _h = s.wrap_spawn(&plan, &mut cmd).await.unwrap();
        let status = cmd.status().unwrap();
        assert!(status.success(), "allowed path should be readable");
    }

    // /bin/cat against a blocked path: landlock should deny the read.
    {
        let blocked_file = blocked.path().join("nope.txt");
        std::fs::write(&blocked_file, b"secret").unwrap();
        let mut cmd = Command::new("/bin/cat");
        cmd.arg(&blocked_file);
        let _h = s.wrap_spawn(&plan, &mut cmd).await.unwrap();
        let status = cmd.status().unwrap();
        assert!(
            !status.success(),
            "blocked path should be denied — got status {status:?}"
        );
    }
}
