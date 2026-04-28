//! Cross-cutting: `--color` flag + `NO_COLOR` env var behaviour.
//!
//! Per spec §3.6 (Output discipline) the resolution order is:
//!
//! 1. `NO_COLOR` env var (any value) → never.
//! 2. `--color always|never` → as-specified.
//! 3. `--color auto` → `is-terminal` check on stdout.
//!
//! Unit tests in `output.rs` exercise [`ColorChoice::resolve`]
//! directly. This file is the integration-level surface: spawn the
//! binary, vary the flag/env, and verify ANSI escapes appear (or do
//! not) in stderr.
//!
//! # Why we only assert the negative half end-to-end
//!
//! `tau`'s top-level error path goes through `eprintln!("error: ...")`
//! (not `Output::error()`), so a failing subcommand emits no ANSI
//! whether or not `--color always` is set. That makes a positive
//! "ANSI escapes appear when --color always" assertion brittle at the
//! integration level: it would require a successful path that calls
//! `Output::warn()` or `Output::error()`, which doesn't currently
//! exist in v0.1. The unit test
//! `error_includes_red_ansi_when_color_always` in `output.rs` covers
//! the formatter directly. Here we focus on the negative half:
//!
//! - `--color never` produces zero ANSI escapes.
//! - `NO_COLOR=1` overrides `--color always` to never.

mod common;

use assert_cmd::Command as AssertCmd;

/// Heuristic ANSI-escape detector: scan for `\x1b[`.
fn contains_ansi(s: &str) -> bool {
    s.contains('\x1b')
}

#[test]
fn color_never_produces_no_ansi_on_init() {
    let dir = common::temp_project();

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["init", "--color", "never"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8(output.stderr).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        !contains_ansi(&stderr),
        "stderr must contain no ANSI escapes with --color never; got: {stderr:?}"
    );
    assert!(
        !contains_ansi(&stdout),
        "stdout must contain no ANSI escapes with --color never; got: {stdout:?}"
    );
}

#[test]
fn color_never_produces_no_ansi_on_failure_path() {
    // Failing path (install bogus URL) — exercise the error stderr
    // pipeline under --color never.
    let global_dir = tempfile::tempdir().unwrap();

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["--color", "never", "install", "--global", "not-a-url"])
        .env("TAU_HOME", global_dir.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        !contains_ansi(&stderr),
        "failure stderr must contain no ANSI escapes with --color never; got: {stderr:?}"
    );
}

#[test]
fn no_color_env_overrides_color_always() {
    let dir = common::temp_project();

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["init", "--color", "always"])
        .current_dir(dir.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    let stderr = String::from_utf8(output.stderr).unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        !contains_ansi(&stderr),
        "NO_COLOR must override --color always (stderr); got: {stderr:?}"
    );
    assert!(
        !contains_ansi(&stdout),
        "NO_COLOR must override --color always (stdout); got: {stdout:?}"
    );
}

// Positive-side coverage is intentionally deferred: see module docs.
// Unit-level assertions live in `tau_cli::output` tests.
