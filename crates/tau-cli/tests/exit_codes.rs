//! Cross-cutting matrix of (subcommand × scenario) → expected exit code.
//!
//! Per ADR-0006 / spec §3.5 the exit-code taxonomy is three buckets:
//!
//! - `0` (`ExitCode::Success`): operation completed.
//! - `1` (`ExitCode::AgentFailed`): `tau run` only — agent ran but
//!   reported `RunOutcome::Failed`.
//! - `2` (`ExitCode::Error`): kernel/CLI error (parse failure, missing
//!   project tau.toml, install failure, validation rejection, etc.).
//!
//! Existing per-command suites (`cmd_run.rs`, `cmd_chat.rs`, ...) cover
//! exit codes incidentally. This file makes the matrix explicit so the
//! contract regresses loudly if it ever shifts. Each test asserts the
//! bucket via `assert_cmd`'s `code(...)`.

mod common;

use assert_cmd::Command as AssertCmd;

// ---- install ----------------------------------------------------------------

#[test]
fn install_success_is_zero() {
    let (fixture, url, _bare) = common::setup_local_package_fixture("hello-tool", "0.1.0");
    let global_dir = fixture.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", &url])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success()
        .code(0);
}

#[test]
fn install_bad_url_is_two() {
    let global_dir = tempfile::tempdir().unwrap();

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", "not-a-url"])
        .env("TAU_HOME", global_dir.path())
        .assert()
        .failure()
        .code(2);
}

// ---- list -------------------------------------------------------------------

#[test]
fn list_success_is_zero() {
    let global_dir = tempfile::tempdir().unwrap();

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["list", "--global"])
        .env("TAU_HOME", global_dir.path())
        .assert()
        .success()
        .code(0);
}

#[test]
fn list_dry_run_rejected_is_two() {
    let global_dir = tempfile::tempdir().unwrap();

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["list", "--dry-run"])
        .env("TAU_HOME", global_dir.path())
        .assert()
        .failure()
        .code(2);
}

// ---- run --------------------------------------------------------------------

#[cfg(feature = "test-mock")]
#[test]
fn run_completed_is_zero() {
    let dir = common::setup_project_with_installed_agent(
        "reviewer",
        "code-reviewer",
        "0.1.0",
        "mock-llm",
    );

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "reviewer", "ping"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .env("TAU_MOCK_LLM_TEXT", "pong")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(0),
        "expected 0; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(feature = "test-mock")]
#[test]
fn run_failed_max_turns_is_one() {
    // The mock backend emits a tool_use on turn 0 only; with
    // --max-turns 1 the agent dispatches the tool then runs out
    // of turns -> Failed(OutOfResources) -> exit 1.
    let dir = common::setup_project_with_installed_agent(
        "reviewer",
        "code-reviewer",
        "0.1.0",
        "mock-llm",
    );

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "reviewer", "loop", "--max-turns", "1"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .env("TAU_MOCK_LLM_TEXT", "calling tool")
        .env("TAU_MOCK_LLM_TOOL_USES", "echo")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "expected 1 (AgentFailed); stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_missing_project_is_two() {
    let dir = tempfile::tempdir().unwrap();

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["run", "reviewer", "hi"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .failure()
        .code(2);
}

#[test]
fn run_unknown_agent_is_two() {
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
        .code(2);
}

// ---- init -------------------------------------------------------------------

#[test]
fn init_success_is_zero() {
    let dir = common::temp_project();

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success()
        .code(0);
}

#[test]
fn init_existing_without_force_is_two() {
    let dir = common::temp_project();

    // First init: succeeds.
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success();

    // Second init without --force: rejected.
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(2);
}

// ---- chat -------------------------------------------------------------------

#[cfg(feature = "test-mock")]
#[test]
fn chat_dry_run_is_zero() {
    let dir = common::setup_project();
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "reviewer", "--dry-run"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .assert()
        .success()
        .code(0);
}

#[test]
fn chat_json_flag_is_two() {
    let dir = common::setup_project();
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["--json", "chat", "reviewer"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .write_stdin("")
        .assert()
        .failure()
        .code(2);
}
