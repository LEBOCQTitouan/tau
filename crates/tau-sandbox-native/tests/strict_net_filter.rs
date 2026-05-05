//! Sub-project D Task 2 — real-kernel network-filter e2e tests.

#![cfg(feature = "integration-tests")]
#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::Command;
use tau_ports::{Sandbox, SandboxPlan, SandboxTier};
use tau_sandbox_native::NativeSandbox;

fn locate_controlled_env_bin() -> PathBuf {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    workspace_root.join(
        "crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env",
    )
}

fn plan_with_network(hosts: Vec<&str>) -> SandboxPlan {
    let host_array: Vec<serde_json::Value> = hosts.iter().map(|h| serde_json::json!(h)).collect();
    let bin_parent = locate_controlled_env_bin()
        .parent()
        .expect("controlled-env binary has parent dir")
        .to_string_lossy()
        .into_owned();
    serde_json::from_value(serde_json::json!({
        "capabilities": [
            {
                "kind": "net.http",
                "hosts": host_array,
                "methods": ["GET"]
            },
            {"kind": "fs.read", "paths": [bin_parent]}
        ],
        "context": null,
        "limits": null,
    }))
    .expect("valid plan")
}

#[tokio::test]
async fn localhost_socket_allowed_with_http_cap() {
    let plan = plan_with_network(vec!["127.0.0.1", "localhost"]);

    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "open-socket");

    let sandbox = NativeSandbox::new("test-strict-net", SandboxTier::Strict);
    let _handle = sandbox
        .wrap_spawn(&plan, &mut cmd)
        .await
        .expect("wrap_spawn");
    let output = cmd.output().expect("spawn");

    assert!(
        output.status.success(),
        "socket() should succeed with Network(Http) cap; got status={:?}, stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("SOCKET_OK"));
}

#[tokio::test]
async fn external_host_socket_allowed_with_http_cap() {
    let plan = plan_with_network(vec!["api.example.com"]);

    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "open-socket");

    let sandbox = NativeSandbox::new("test-strict-net", SandboxTier::Strict);
    let _handle = sandbox
        .wrap_spawn(&plan, &mut cmd)
        .await
        .expect("wrap_spawn");
    let output = cmd.output().expect("spawn");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("SOCKET_OK"));
}
