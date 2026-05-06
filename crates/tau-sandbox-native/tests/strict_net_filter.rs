//! Sub-project F Task 6 — net-filter e2e integration tests.
//!
//! These tests exercise the full net-filter pipeline end-to-end:
//! 1. `localhost_socket_allowed_with_http_cap` — plan with `Network(Http)` for
//!    `127.0.0.1`; the child's `socket()` call succeeds (nftables permits loopback).
//! 2. `external_host_socket_allowed_with_http_cap` — plan with `Network(Http)`
//!    for `localhost` (hostname, resolves without external DNS); `socket()` succeeds.
//! 3. `no_network_cap_socket_denied_by_seccomp` — no `Network(Http)` capability;
//!    seccomp fires `KillProcess` on `socket()`.
//! 4. `net_filter_handle_drop_removes_parent_veth` — after spawn + handle drop,
//!    `ip link show <veth>` returns non-zero (interface deleted).
//!
//! Run via the `test-net-filter` CI job (privileged Docker with `CAP_NET_ADMIN`).
//! On Darwin all four tests are compiled (`--no-run` passes) but not executed.

#![cfg(feature = "integration-tests")]
#![cfg(target_os = "linux")]

use std::collections::HashSet;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;

use tau_domain::fixtures::{cap_fs_read, cap_net_http};
use tau_ports::{fixtures::plan_from_capabilities, Sandbox, SandboxPlan, SandboxTier};
use tau_sandbox_native::NativeSandbox;

// ---------------------------------------------------------------------------
// Helpers
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

fn bin_parent_str() -> String {
    locate_controlled_env_bin()
        .parent()
        .expect("controlled-env binary has parent dir")
        .to_string_lossy()
        .into_owned()
}

/// Plan with `Network(Http)` for the given hosts plus `fs.read` on the
/// controlled-env binary so landlock permits exec of the binary.
fn plan_with_http_cap(hosts: &[&str]) -> SandboxPlan {
    let bin_parent = bin_parent_str();
    plan_from_capabilities(vec![
        cap_net_http(hosts, &["GET"]),
        cap_fs_read(&[bin_parent.as_str()]),
    ])
}

/// Plan without any network capability; landlock allows exec of the binary.
fn plan_no_network() -> SandboxPlan {
    let bin_parent = bin_parent_str();
    plan_from_capabilities(vec![cap_fs_read(&[bin_parent.as_str()])])
}

/// Snapshot the set of `tsb*`-prefixed network interface names visible in `ip link show`.
///
/// Used by test 4 to take before/after snapshots so only the *newly* created
/// veth is asserted on, avoiding false positives from interfaces left by
/// other concurrently-running test binaries.
fn list_tsb_interfaces() -> HashSet<String> {
    let out = Command::new("ip")
        .args(["link", "show"])
        .output()
        .expect("ip link show must succeed");
    let text = String::from_utf8_lossy(&out.stdout);
    // Lines look like: `42: tsb12345-0h: <FLAGS> ...`
    // Strip leading `<digits>: ` then take the token before the next `:`.
    text.lines()
        .filter_map(|line| {
            let trimmed = line
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == ':' || c == ' ');
            let name = trimmed.split(':').next()?.trim();
            if name.starts_with("tsb") {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Test 1: localhost socket allowed with Network(Http) for 127.0.0.1
// ---------------------------------------------------------------------------

/// Sandbox with `Network(Http)` for `127.0.0.1` permits `socket()` in the child.
///
/// Full lifecycle:
/// - `wrap_spawn` installs landlock + sync-pipe + seccomp pre-exec hooks.
/// - `spawn()` forks the child (child blocks on sync pipe).
/// - `apply_post_spawn` creates the veth pair + nftables ruleset.
/// - `signal_post_spawn_complete` writes 1 byte → child unblocks.
/// - Child calls `socket(AF_INET, SOCK_STREAM, 0)`, emits `SOCKET_OK`, exits 0.
#[tokio::test]
async fn localhost_socket_allowed_with_http_cap() {
    let plan = plan_with_http_cap(&["127.0.0.1"]);

    let sandbox = NativeSandbox::new("test-net-localhost", SandboxTier::Strict);
    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "open-socket");

    let mut handle = sandbox
        .wrap_spawn(&plan, &mut cmd)
        .await
        .expect("wrap_spawn must succeed for Network(Http) plan");

    // Spawn the child. The child blocks in pre_exec on the sync pipe.
    let mut child = cmd.spawn().expect("child spawn must succeed");
    let child_pid = child.id() as i32;

    // Set up veth pair + nftables inside the child's netns.
    sandbox
        .apply_post_spawn(&plan, child_pid, &mut handle)
        .await
        .expect("apply_post_spawn must succeed");

    // Release the child: write 1 byte to sync pipe.
    handle
        .signal_post_spawn_complete()
        .expect("signal_post_spawn_complete must succeed");

    // Wait for the child to exit.
    let status = child.wait().expect("child wait must succeed");

    assert!(
        status.success(),
        "expected exit 0 with Network(Http) cap for 127.0.0.1; got status={:?}",
        status,
    );
}

// ---------------------------------------------------------------------------
// Test 2: external host socket allowed with Network(Http) (uses 'localhost')
// ---------------------------------------------------------------------------

/// Sandbox with `Network(Http)` for `localhost` (hostname) permits `socket()`.
///
/// `localhost` resolves via `/etc/hosts` without external DNS — safe in Docker.
/// This exercises the hostname-resolution path (not the `127.0.0.1` literal
/// short-circuit) while remaining deterministic in an offline environment.
#[tokio::test]
async fn external_host_socket_allowed_with_http_cap() {
    let plan = plan_with_http_cap(&["localhost"]);

    let sandbox = NativeSandbox::new("test-net-external", SandboxTier::Strict);
    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "open-socket");

    let mut handle = sandbox
        .wrap_spawn(&plan, &mut cmd)
        .await
        .expect("wrap_spawn must succeed for Network(Http) plan with 'localhost'");

    let mut child = cmd.spawn().expect("child spawn must succeed");
    let child_pid = child.id() as i32;

    sandbox
        .apply_post_spawn(&plan, child_pid, &mut handle)
        .await
        .expect("apply_post_spawn must succeed");

    handle
        .signal_post_spawn_complete()
        .expect("signal_post_spawn_complete must succeed");

    let status = child.wait().expect("child wait must succeed");

    assert!(
        status.success(),
        "expected exit 0 with Network(Http) for 'localhost'; got status={:?}, stdout=(not captured — use cmd.output() in local repro)",
        status,
    );
}

// ---------------------------------------------------------------------------
// Test 3: no network cap → seccomp kills the child on socket()
// ---------------------------------------------------------------------------

/// Without `Network(Http)`, seccomp fires `KillProcess` on `socket()`.
///
/// No `Network(Http)` capability → seccomp does NOT add socket-family syscalls →
/// the child's `socket(2)` call triggers `SeccompAction::KillProcess` (SIGSYS = 31).
///
/// Note: no sync pipe exists for plans without `Network(Http)`, so `wrap_spawn`
/// returns a noop handle and the child runs immediately without blocking.
#[tokio::test]
async fn no_network_cap_socket_denied_by_seccomp() {
    let plan = plan_no_network();

    let sandbox = NativeSandbox::new("test-net-denied", SandboxTier::Strict);
    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "open-socket");

    let _handle = sandbox
        .wrap_spawn(&plan, &mut cmd)
        .await
        .expect("wrap_spawn must succeed for no-network plan");

    // No sync pipe for non-network plans; cmd.output() works directly.
    let output = cmd.output().expect("child spawn must succeed");

    // seccomp at strict tier without Network(Http) must block socket().
    assert!(
        !output.status.success(),
        "expected non-zero/signal exit; got status={:?}, stdout={:?}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
    );
    assert!(
        output.stdout.is_empty()
            || !String::from_utf8_lossy(&output.stdout).contains("SOCKET_OK"),
        "expected no SOCKET_OK in stdout; got {:?}",
        String::from_utf8_lossy(&output.stdout),
    );
    // seccomp KillProcess → SIGSYS (31). Use assert_eq! (not if-let) so the
    // assertion is never silently skipped if the seccomp action changes.
    assert_eq!(
        output.status.signal(),
        Some(31),
        "expected SIGSYS (31) from seccomp KillProcess; got status={:?}",
        output.status,
    );
}

// ---------------------------------------------------------------------------
// Test 4: NetFilterHandle Drop removes parent veth
// ---------------------------------------------------------------------------

/// Dropping the `SandboxHandle` (which nests a `NetFilterHandle`) runs
/// `ip link del <veth>` on the parent-side interface.
///
/// Steps:
/// 1. Build a plan with `Network(Http)` and wrap_spawn (creates the sync pipe
///    + pre-allocates the veth subnet).
/// 2. Spawn the child (it blocks on the sync pipe).
/// 3. `apply_post_spawn` creates the real veth pair.
/// 4. Record the veth interface name via `ip link show` (before drop).
/// 5. `signal_post_spawn_complete` → child proceeds to open-socket and exits.
/// 6. Wait for child.
/// 7. Drop the `SandboxHandle` → `NetFilterHandle::drop` runs `ip link del`.
/// 8. Assert `ip link show <veth>` returns non-zero.
#[tokio::test]
async fn net_filter_handle_drop_removes_parent_veth() {
    let plan = plan_with_http_cap(&["127.0.0.1"]);

    let sandbox = NativeSandbox::new("test-net-drop", SandboxTier::Strict);
    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "open-socket");

    // Snapshot tsb* interfaces BEFORE apply_post_spawn to isolate the veth
    // created by *this* test from any interfaces left by other concurrently-
    // running test binaries or previous test runs on the same host.
    let before: HashSet<String> = list_tsb_interfaces();

    let mut handle = sandbox
        .wrap_spawn(&plan, &mut cmd)
        .await
        .expect("wrap_spawn must succeed");

    let mut child = cmd.spawn().expect("child spawn must succeed");
    let child_pid = child.id() as i32;

    sandbox
        .apply_post_spawn(&plan, child_pid, &mut handle)
        .await
        .expect("apply_post_spawn must succeed");

    // Snapshot AFTER apply_post_spawn; the set difference is the newly-added
    // interface(s) created by this test.
    let after: HashSet<String> = list_tsb_interfaces();
    let new_interfaces: Vec<String> = after.difference(&before).cloned().collect();
    assert!(
        !new_interfaces.is_empty(),
        "apply_post_spawn must have created at least one new tsb* interface; \
         before={:?} after={:?}",
        before,
        after,
    );

    // Signal child → it proceeds, opens socket (should succeed), exits.
    handle
        .signal_post_spawn_complete()
        .expect("signal_post_spawn_complete must succeed");

    let _ = child.wait().expect("child wait must succeed");

    // Drop the SandboxHandle (which Drops the nested NetFilterHandle,
    // running `ip link del` on the parent-side veth).
    drop(handle);

    // Assert each newly-added interface is now gone.
    let final_interfaces: HashSet<String> = list_tsb_interfaces();
    for name in &new_interfaces {
        assert!(
            !final_interfaces.contains(name),
            "veth {name} should have been removed by NetFilterHandle::drop, \
             but ip link show still lists it"
        );
    }
}
