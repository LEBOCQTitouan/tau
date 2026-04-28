//! Cross-cutting: error display under default vs `--debug`.
//!
//! Per spec §3.5 + ADR-0006, the kernel/CLI error path emits the
//! top-level message on stderr by default, and (with `--debug`) the
//! full anyhow chain. The lib.rs `run_main` switch is:
//!
//! - default: `eprintln!("error: {err}")` — single-line top-level.
//! - `--debug`: `eprintln!("error: {err:?}")` — multi-line chain.
//!
//! We assert the stderr surface for both modes by reusing a failure
//! scenario that exercises `with_context` chaining: `tau run` against
//! a directory without a project `tau.toml`. The handler wraps the
//! underlying `ProjectConfigError::NotFound` with
//! `"project tau.toml required at <path>"`, so the chain has at least
//! two layers.

mod common;

use assert_cmd::Command as AssertCmd;
use predicates::prelude::*;

#[test]
fn default_error_is_top_level_only() {
    // No project tau.toml at cwd -> `tau run` fails with a context
    // wrapping the NotFound -> single-line top-level message.
    let dir = tempfile::tempdir().unwrap();

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "reviewer", "hi"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .output()
        .unwrap();

    assert!(!output.status.success(), "expected failure");
    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("error:"),
        "expected `error:` prefix; got: {stderr}"
    );
    // Default rendering uses `{err}` (single-line top-level) — the
    // anyhow-style "Caused by:" header that `{err:?}` adds must NOT
    // appear here.
    assert!(
        !stderr.contains("Caused by:"),
        "default error must omit the `Caused by:` chain header; got: {stderr}"
    );
}

#[test]
fn debug_flag_expands_error_chain() {
    let dir = tempfile::tempdir().unwrap();

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["--debug", "run", "reviewer", "hi"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .output()
        .unwrap();

    assert!(!output.status.success(), "expected failure");
    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("error:"),
        "expected `error:` prefix; got: {stderr}"
    );
    // `{err:?}` from anyhow renders the `Caused by:` chain. The
    // ProjectConfig error is wrapped in a `with_context` call, so the
    // chain has at least one inner cause.
    assert!(
        stderr.contains("Caused by:"),
        "--debug should expand the error chain via {{err:?}}; got: {stderr}"
    );
}

#[test]
fn default_error_does_not_leak_debug_chain_for_install() {
    // Same contract for a different subcommand: `tau install bogus`
    // surfaces an "invalid package URL" error wrapping a parse error.
    // Default mode prints only the top-level message.
    let global_dir = tempfile::tempdir().unwrap();

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", "not-a-url"])
        .env("TAU_HOME", global_dir.path())
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("error:"))
        .stderr(predicate::str::contains("Caused by:").not());
}
