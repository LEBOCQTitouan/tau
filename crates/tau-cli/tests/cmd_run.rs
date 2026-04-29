//! Integration tests for `tau run`.
//!
//! Two flavours of fixture:
//!
//! - "Easy" tests use `tempfile::TempDir` directly — no plugin spawn,
//!   so they cover the early-exit paths (`run` without a project
//!   `tau.toml`, an unknown agent id).
//! - Run-loop tests use [`common::setup_echo_project`], which builds
//!   `echo-llm` (and optionally `echo-tool`) once per session and
//!   synthesizes a project tau.toml + lockfile pointing at the
//!   pre-built binaries. The CLI then drives a full `tau run` against
//!   real plugin processes via `tau_runtime::plugin_host::load_*`.
//!
//! Plugin-spawning tests are slower (~30 s for the binary's first
//! invocation, sub-second after); the build cost is amortized via the
//! `OnceLock`-guarded `ensure_echo_plugins_built` helper so the per-
//! test overhead is just process spawn + handshake + a single RPC.

mod common;

use assert_cmd::Command as AssertCmd;
use predicates::prelude::*;

// ---- "easy" tests (no fixture / no plugin spawn needed) --------------------

#[test]
fn run_missing_project_tau_toml_exits_two() {
    let dir = tempfile::tempdir().unwrap();

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "reviewer", "hello"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("tau.toml"));
}

#[test]
fn run_agent_id_not_found_exits_two() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("tau.toml"),
        r#"[project]
name = "demo"

[agents.reviewer]
display_name = "Code Reviewer"
package      = "code-reviewer@^0.1"
llm_backend  = "anthropic"
"#,
    )
    .unwrap();

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "ghost", "hi"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("ghost"));
}

// ---- run-loop tests (real echo-llm spawn) ----------------------------------

#[test]
fn run_dry_run_prints_preview_and_makes_no_llm_call() {
    // --dry-run short-circuits before plugin loading so we don't even
    // need the echo binary built — but we still go through
    // `setup_echo_project` for parity with the other tests in this
    // module (and to keep the fixture authoring in one place).
    let dir = common::setup_echo_project("echo", "canned_text = \"unused on dry-run\"\n", &[]);

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "echo", "Review src/auth.rs", "--dry-run"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .success()
        .stderr(predicate::str::contains("[dry-run]"))
        .stderr(predicate::str::contains("agent:"))
        .stderr(predicate::str::contains("echo"))
        .stderr(predicate::str::contains("max_turns:"))
        .stderr(predicate::str::contains("no LLM call"));
}

#[test]
fn run_completed_happy_path_emits_text() {
    let dir = common::setup_echo_project(
        "echo",
        "canned_text = \"review complete: looks good\"\n",
        &[],
    );

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "echo", "Review src/auth.rs"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected success; stderr={}\nstdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("review complete: looks good"),
        "stdout: {stdout}"
    );
}

#[test]
fn run_propagates_plugin_crash_as_exit_code_two() {
    // `crash_after_handshake = true` causes echo-llm to panic on the
    // first `llm.complete` RPC. The host-side dispatch surfaces this
    // as a `RuntimeError`, which `run_main` maps to exit code 2.
    let dir = common::setup_echo_project("echo", "crash_after_handshake = true\n", &[]);

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "echo", "anything"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "expected kernel-error exit code 2; got {:?}\nstderr={}\nstdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("echo-llm") || stderr.contains("plugin"),
        "stderr should mention the plugin; got:\n{stderr}"
    );
}

#[test]
fn run_json_completed_emits_outcome_payload() {
    let dir = common::setup_echo_project("echo", "canned_text = \"pong\"\n", &[]);

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "echo", "ping", "--json"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected success; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("--json should emit a JSON object");
    assert_eq!(parsed["outcome"], "completed");
    assert_eq!(parsed["final_message"], "pong");
    assert!(parsed["total_turns"].is_number(), "total_turns: {parsed}");
    assert!(parsed["token_usage"].is_object());
}
