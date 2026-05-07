//! Layer 4 integration tests for the sandbox proxy.
//!
//! Replaces the deleted strict_net_filter.rs (sub-project F). These tests
//! exercise the full proxy + bridge + plugin chain via real spawn,
//! real netns, real seccomp.
//!
//! Linux-only; gated by feature `integration-tests`. Run via:
//!   cargo nextest run -p tau-sandbox-native --features integration-tests --test strict_proxy

#![cfg(target_os = "linux")]
#![cfg(feature = "integration-tests")]

use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;
use tau_domain::fixtures::{cap_fs_read, cap_net_http};
use tau_ports::fixtures::plan_from_capabilities;
use tau_ports::{Sandbox, SandboxPlan, SandboxTier};
use tau_sandbox_native::NativeSandbox;

// ---------------------------------------------------------------------------
// Helpers (mirrors strict_seccomp.rs patterns)
// ---------------------------------------------------------------------------

fn locate_controlled_env_bin() -> PathBuf {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    let bin = workspace_root.join(
        "crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env",
    );
    if !bin.exists() {
        panic!(
            "controlled-env binary not found at {}. Run: \
             cargo build --manifest-path \
             crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml --release",
            bin.display()
        );
    }
    bin
}

/// Returns the controlled-env binary's parent dir as a string, for
/// inclusion in `fs.read` paths so landlock allows exec of the binary.
fn bin_parent_str() -> String {
    locate_controlled_env_bin()
        .parent()
        .expect("controlled-env binary has parent dir")
        .to_string_lossy()
        .into_owned()
}

fn plan_no_network() -> SandboxPlan {
    let bin_parent = bin_parent_str();
    plan_from_capabilities(vec![cap_fs_read(&[&bin_parent])])
}

fn plan_with_http_cap(hosts: &[&str]) -> SandboxPlan {
    let bin_parent = bin_parent_str();
    plan_from_capabilities(vec![
        cap_net_http(hosts, &["GET"]),
        cap_fs_read(&[&bin_parent]),
    ])
}

/// Set TAU_NET_BRIDGE_PATH to the bin target's compile-time path so the
/// container/native adapter can find tau-net-bridge during tests.
fn ensure_bridge_path() {
    if std::env::var_os("TAU_NET_BRIDGE_PATH").is_none() {
        let path = env!("CARGO_BIN_EXE_tau-net-bridge");
        std::env::set_var("TAU_NET_BRIDGE_PATH", path);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Without a Network(Http) capability, the strict-tier seccomp filter must
/// block socket() with SIGSYS (or EACCES exit). Mirrors
/// strict_seccomp::socket_blocked_without_network_capability but verifies
/// the proxy code-path is correctly absent (no proxy socket, no wrapping).
#[tokio::test]
async fn no_network_cap_socket_denied_by_seccomp() {
    ensure_bridge_path();
    let plan = plan_no_network();

    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "open-socket");

    let sandbox = NativeSandbox::new("test-strict", SandboxTier::Strict);
    let _handle = sandbox
        .wrap_spawn(&plan, &mut cmd)
        .await
        .expect("wrap_spawn must succeed for no-network plan");

    let output = cmd.output().expect("child spawn must succeed");

    // seccomp at strict tier without Network(Http) must block socket().
    assert!(
        !output.status.success(),
        "expected non-zero/signal exit; got status={:?}, stdout={:?}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
    );
    // SIGSYS = 31 from seccomp KillProcess; tolerate either signal or
    // the controlled-env binary's own non-zero exit on EACCES.
    if let Some(sig) = output.status.signal() {
        assert_eq!(sig, 31, "expected SIGSYS (31); got signal {sig}");
    }
}

/// When a Network(Http) plan is used, wrap_spawn spawns a proxy and creates
/// a temp socket file. Dropping the returned SandboxHandle must unlink it.
#[tokio::test]
async fn proxy_handle_drop_cleans_up_temp_socket() {
    ensure_bridge_path();
    let plan = plan_with_http_cap(&["127.0.0.1"]);

    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "default");

    let sandbox = NativeSandbox::new("test-strict", SandboxTier::Strict);
    let handle = sandbox
        .wrap_spawn(&plan, &mut cmd)
        .await
        .expect("wrap_spawn must succeed for Network(Http) plan");

    // After wrap_spawn, the proxy socket file should exist in the temp dir.
    // The naming pattern is tau-proxy-{pid}-{n}.sock (see tau-sandbox-proxy).
    let temp_dir = std::env::temp_dir();
    let proxy_files_before: Vec<_> = std::fs::read_dir(&temp_dir)
        .expect("read temp dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("tau-proxy-")
        })
        .collect();
    assert!(
        !proxy_files_before.is_empty(),
        "expected at least one tau-proxy-*.sock in {} after wrap_spawn",
        temp_dir.display()
    );

    drop(handle);
    // Give the OS a beat to unlink.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let proxy_files_after: Vec<_> = std::fs::read_dir(&temp_dir)
        .expect("read temp dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("tau-proxy-")
        })
        .collect();

    // Every socket that existed before must be gone.
    let names_before: Vec<_> = proxy_files_before
        .iter()
        .map(|e| e.file_name())
        .collect();
    let names_after: Vec<_> = proxy_files_after
        .iter()
        .map(|e| e.file_name())
        .collect();
    for name in &names_before {
        assert!(
            !names_after.contains(name),
            "expected proxy socket {} to be unlinked on handle drop",
            name.to_string_lossy()
        );
    }
}

// ---------------------------------------------------------------------------
// Tests that need a real cassette server (deferred to T9)
// ---------------------------------------------------------------------------

// `localhost_socket_allowed_with_http_cap`, `external_host_blocked_when_not_in_allowlist`,
// and `sni_mismatch_rejected` would each need:
//   - a cassette HTTP server running on the host (use tau-plugin-test-support if it exposes
//     a constructor; otherwise a simple tokio TcpListener test fixture)
//   - the controlled-env binary must be configured to make an actual HTTPS request via
//     HTTPS_PROXY (which it gets from cmd.env)
//
// These are deferred to layer4_container.rs (T9) because:
//   1. The controlled-env binary's existing TAU_FIXTURE_MODE values don't include
//      "make-https-request"; adding that mode is out of scope here
//   2. End-to-end TLS testing in a sandboxed netns requires careful cassette setup
//      that's better tested at the layer4_container.rs level (T9)
