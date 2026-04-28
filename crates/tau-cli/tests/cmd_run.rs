//! Integration tests for `tau run`.
//!
//! Hand-authors the package fixture (lockfile + `tau.toml`) directly
//! rather than going through `tau install`, mirroring the
//! `install_fixture` helper from `crates/tau-cli/src/config/agent.rs`'s
//! unit tests. The fixture builders themselves now live in
//! `tests/common/mod.rs` (Task 15) so `cmd_chat.rs` and the cross-cutting
//! suites can share them; this file only orchestrates the assertions.
//!
//! Tests that require the mock backend gate themselves with
//! `#[cfg(feature = "test-mock")]` so a no-features `cargo test
//! --test cmd_run` still compiles. The "easy" tests
//! (agent_id_not_found, missing_project_tau_toml) run regardless.

mod common;

use assert_cmd::Command as AssertCmd;
use predicates::prelude::*;

// ---- "easy" tests (no fixture / no mock LLM needed) -------------------------

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

// ---- mock-backend-driven tests ----------------------------------------------
//
// These require the binary to be built with `--features test-mock` so
// the mock LLM backend is compiled in. Without the feature,
// `RuntimeBuilder::build` errors with `BuildError::NoLlmBackend` and
// the tests would fail with a kernel error rather than the asserted
// behavior. We gate the test items on the feature so a no-features
// `cargo test --test cmd_run` is green and `cargo test --test cmd_run
// --features test-mock` exercises the rest.

#[cfg(feature = "test-mock")]
#[test]
fn run_dry_run_prints_preview_and_makes_no_llm_call() {
    let dir = common::setup_project_with_installed_agent(
        "reviewer",
        "code-reviewer",
        "0.1.0",
        "mock-llm",
    );

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "reviewer", "Review src/auth.rs", "--dry-run"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .success()
        .stderr(predicate::str::contains("[dry-run]"))
        .stderr(predicate::str::contains("agent:"))
        .stderr(predicate::str::contains("reviewer"))
        .stderr(predicate::str::contains("max_turns:"))
        .stderr(predicate::str::contains("no LLM call"));
}

#[cfg(feature = "test-mock")]
#[test]
fn run_completed_happy_path_emits_text() {
    let dir = common::setup_project_with_installed_agent(
        "reviewer",
        "code-reviewer",
        "0.1.0",
        "mock-llm",
    );

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "reviewer", "Review src/auth.rs"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .env("TAU_MOCK_LLM_TEXT", "review complete: looks good")
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

#[cfg(feature = "test-mock")]
#[test]
fn run_with_tool_call_dispatches_echo_and_completes() {
    let dir = common::setup_project_with_installed_agent(
        "reviewer",
        "code-reviewer",
        "0.1.0",
        "mock-llm",
    );

    // Turn 0: emit a tool_use for `echo`. Turn 1: end with text.
    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "reviewer", "drive a tool call"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .env("TAU_MOCK_LLM_TEXT", "done after tool")
        .env("TAU_MOCK_LLM_TOOL_USES", "echo")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected success; stderr={}\nstdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    // The final assistant message is the turn-1 text, not the tool result.
    assert!(stdout.contains("done after tool"), "stdout: {stdout}");
}

#[cfg(feature = "test-mock")]
#[test]
fn run_max_turns_reached_when_llm_loops_forever() {
    // Per `MockLlmBackend::build_response`, tool_uses are emitted on
    // turn 0 only — so an "infinite" tool-loop isn't reachable through
    // env-var configuration. Instead, set max_turns = 1 and emit a
    // tool_use: the loop dispatches the tool on turn 1, then runs out
    // of turns before reaching the second LLM call. Result: Failed
    // with OutOfResources → exit code 1.
    let dir = common::setup_project_with_installed_agent(
        "reviewer",
        "code-reviewer",
        "0.1.0",
        "mock-llm",
    );

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "reviewer", "loop forever", "--max-turns", "1"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .env("TAU_MOCK_LLM_TEXT", "calling a tool")
        .env("TAU_MOCK_LLM_TOOL_USES", "echo")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected agent-failed exit code 1; got status={:?}\nstderr={}\nstdout={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("agent failed"),
        "stderr should announce failure: {stderr}"
    );
}

#[cfg(feature = "test-mock")]
#[test]
fn run_json_completed_emits_outcome_payload() {
    let dir = common::setup_project_with_installed_agent(
        "reviewer",
        "code-reviewer",
        "0.1.0",
        "mock-llm",
    );

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "reviewer", "ping", "--json"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .env("TAU_MOCK_LLM_TEXT", "pong")
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
