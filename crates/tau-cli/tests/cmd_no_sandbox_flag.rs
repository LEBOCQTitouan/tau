//! Integration tests for `--no-sandbox` and `--sandbox <kind>`.
//!
//! These tests exercise the global flags introduced in Task 7:
//! `--no-sandbox` (force passthrough adapter, bypass plugin-tier floors)
//! and `--sandbox <kind>` (force a specific adapter).
//!
//! Most tests rely on `tau chat --dry-run` so they don't require an LLM
//! backend: the dry-run path validates CLI parsing and exits before
//! attempting plugin spawn.

use assert_cmd::Command as AssertCmd;
use predicates::prelude::*;

mod common;

// ---- Test 1: --no-sandbox is accepted and chat --dry-run succeeds ----------

#[test]
fn no_sandbox_smokes() {
    // tau chat <agent> --no-sandbox --dry-run should succeed:
    // clap accepts the flag; dry-run returns before plugin loading.
    let dir = common::setup_echo_project("echo", "canned_text = \"reply\"\n", &[]);
    let global_dir = dir.path().join("global");
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["--no-sandbox", "chat", "--dry-run", "echo"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .assert()
        .success();
}

// ---- Test 2: --sandbox passthrough is equivalent to --no-sandbox -----------

#[test]
fn sandbox_passthrough_equivalent_to_no_sandbox() {
    // --sandbox passthrough should behave identically to --no-sandbox.
    let dir = common::setup_echo_project("echo", "canned_text = \"reply\"\n", &[]);
    let global_dir = dir.path().join("global");
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["--sandbox", "passthrough", "chat", "--dry-run", "echo"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .assert()
        .success();
}

// ---- Test 3: --sandbox native on Windows errors clearly --------------------
//
// macOS satisfies `RegistryKind::Native` via `tau-sandbox-darwin`
// (sandbox-exec); Linux satisfies it via `tau-sandbox-native` (landlock +
// seccomp + namespaces). Windows has a Phase 1 scaffold (`tau-sandbox-windows`)
// but probe returns Unavailable until Phase 2 lands the Win32 calls, so
// the forced-Native path on Windows still errors clearly today.
#[cfg(target_os = "windows")]
#[test]
fn sandbox_native_on_windows_errors_clearly() {
    let dir = common::setup_echo_project("echo", "canned_text = \"reply\"\n", &[]);
    let global_dir = dir.path().join("global");
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["--sandbox", "native", "chat", "echo"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        // Unset the mock env var so the forced-kind path is exercised and
        // not bypassed by mock-sandbox injection.
        .env_remove("TAU_TESTING_ALLOW_MOCK_SANDBOX")
        .write_stdin("")
        .assert()
        .failure()
        // The runtime's error formatter capitalizes RegistryKind::Native
        // as `Native`. Match that exactly to keep the assertion robust.
        .stderr(predicate::str::contains("Native"));
}

// ---- Test 4: --no-sandbox and --sandbox conflict ---------------------------

#[test]
fn no_sandbox_and_sandbox_flag_conflict() {
    // clap's conflicts_with attribute should produce a parse error before
    // any command logic runs (no project directory needed).
    let dir = tempfile::tempdir().unwrap();
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args([
            "--no-sandbox",
            "--sandbox",
            "container",
            "chat",
            "--dry-run",
            "echo",
        ])
        .current_dir(dir.path())
        .assert()
        .failure();
    // clap produces exit code 2 for argument errors.
}

// ---- Test 5: --no-sandbox and --sandbox appear in --help -------------------

#[test]
fn no_sandbox_appears_in_help() {
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--no-sandbox"))
        .stdout(predicate::str::contains("--sandbox"));
}
