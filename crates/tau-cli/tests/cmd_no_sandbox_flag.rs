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

// ---- Test 3: --sandbox native on non-Linux errors clearly ------------------

#[cfg(not(target_os = "linux"))]
#[test]
fn sandbox_native_on_non_linux_errors_clearly() {
    // --sandbox native forces the native (Linux-only) adapter.
    // On macOS/Windows, resolve_adapter_forced(Native) probes Unavailable and
    // returns an error mentioning "native". We must NOT use --dry-run because
    // the error surfaces inside load_plugins (after the dry-run early-return).
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
        // Pipe empty stdin so the binary doesn't block on tty input.
        .write_stdin("")
        .assert()
        .failure()
        .stderr(predicate::str::contains("native"));
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
