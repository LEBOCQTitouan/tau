//! Integration tests for `tau chat`.
//!
//! Same hand-author fixture pattern as `cmd_run.rs` (sub-project 9):
//! write a project `tau.toml` plus an installed package + LLM-backend
//! lockfile entry, then drive the binary via `assert_cmd`. Each REPL
//! interaction is a `write_stdin` payload terminated by `/exit\n` so
//! the loop returns cleanly. Mock-backend-driven tests gate on
//! `feature = "test-mock"` for parity with `cmd_run.rs`.

mod common;

use std::path::Path;

use assert_cmd::Command as AssertCmd;
use predicates::prelude::*;
use tempfile::TempDir;

// ---- fixture helpers --------------------------------------------------------

/// Hand-author a lockfile + on-disk package tree under `<root>/.tau/`.
///
/// Replicates the helper from `cmd_run.rs`. Kept local so the file is
/// self-contained; if a third command needs the same fixture, lift it
/// into `common/mod.rs`.
#[allow(dead_code)]
fn install_fixture(root: &Path, name: &str, version: &str, kind: &str, source_url: &str) {
    let dot_tau = root.join(".tau");
    std::fs::create_dir_all(dot_tau.join("packages").join(name).join(version)).unwrap();

    let manifest = format!(
        r#"name = "{name}"
version = "{version}"
description = "fixture"
authors = ["tester <test@example.com>"]
source = "{source_url}"
kind = "{kind}"
dependencies = []
capabilities = []
"#
    );
    std::fs::write(
        dot_tau
            .join("packages")
            .join(name)
            .join(version)
            .join("tau.toml"),
        manifest,
    )
    .unwrap();

    let lockfile_path = root.join("tau-lock.toml");
    let existing = if lockfile_path.exists() {
        std::fs::read_to_string(&lockfile_path).unwrap()
    } else {
        String::new()
    };

    let now_rfc3339 = "2026-04-28T00:00:00Z";
    let resolved_commit = "0".repeat(40);
    let new_entry = format!(
        r#"
[[package]]
name = "{name}"
active_version = "{version}"
source = "{source_url}"

[[package.versions]]
version = "{version}"
resolved_commit = "{resolved_commit}"
sha256 = ""
installed_at = "{now_rfc3339}"
"#
    );

    let new_lockfile = if existing.is_empty() {
        format!(
            r#"schema_version = 1
generated_by_tau_version = "0.0.0"
generated_at = "{now_rfc3339}"
{new_entry}"#
        )
    } else {
        format!("{existing}\n{new_entry}")
    };
    std::fs::write(&lockfile_path, new_lockfile).unwrap();
}

/// Stand up a project tempdir with a `tau.toml` declaring `reviewer`
/// plus the matching package + `mock-llm` backend pre-installed.
#[allow(dead_code)]
fn setup_project() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    install_fixture(
        root,
        "code-reviewer",
        "0.1.0",
        "tool",
        "https://example.com/pkg.git",
    );
    install_fixture(
        root,
        "mock-llm",
        "0.1.0",
        "llm-backend",
        "https://example.com/llm.git",
    );

    let project_toml = r#"[project]
name = "demo"

[agents.reviewer]
display_name = "Test Agent"
package      = "code-reviewer@^0.1"
llm_backend  = "mock-llm"
"#;
    std::fs::write(root.join("tau.toml"), project_toml).unwrap();

    dir
}

// ---- "easy" tests (no fixture / no mock LLM needed) -------------------------

#[test]
fn chat_rejects_json_flag() {
    // --json is rejected at handler entry — no project setup required
    // for the assertion, but we provide one anyway so the test exercises
    // the same dispatch path the others do.
    let dir = setup_project();
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["--json", "chat", "reviewer"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        // Pipe an empty stdin so the binary doesn't block on a missing
        // tty if the --json check ever moves below readline.
        .write_stdin("")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--json"));
}

#[test]
fn chat_missing_agent_id_exits_two() {
    let dir = setup_project();
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "nonexistent"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .write_stdin("")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("nonexistent"));
}

// ---- mock-backend-driven tests ----------------------------------------------

#[cfg(feature = "test-mock")]
#[test]
fn chat_dry_run_skips_repl() {
    let dir = setup_project();
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "reviewer", "--dry-run"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .assert()
        .success()
        .stderr(predicate::str::contains("[dry-run]"))
        .stderr(predicate::str::contains("REPL would start"))
        .stderr(predicate::str::contains("no session opened"));
}

#[cfg(feature = "test-mock")]
#[test]
fn chat_repl_one_round_via_stdin_pipe() {
    let dir = setup_project();
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "reviewer"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .env("TAU_MOCK_LLM_TEXT", "ok answer")
        .write_stdin("Hi\n/exit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("ok answer"))
        .stderr(predicate::str::contains("session ended"))
        .stderr(predicate::str::contains("Welcome to tau chat"));
}

#[cfg(feature = "test-mock")]
#[test]
fn chat_help_command_lists_slash_commands() {
    let dir = setup_project();
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "reviewer"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .write_stdin("/help\n/exit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("/exit"))
        .stdout(predicate::str::contains("/clear"))
        .stdout(predicate::str::contains("/history"));
}

#[cfg(feature = "test-mock")]
#[test]
fn chat_clear_resets_history() {
    let dir = setup_project();
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "reviewer"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .env("TAU_MOCK_LLM_TEXT", "first")
        .write_stdin("turn1\n/clear\n/history\n/exit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("first"))
        .stdout(predicate::str::contains("(no history yet)"))
        .stderr(predicate::str::contains("history cleared"));
}

#[cfg(feature = "test-mock")]
#[test]
fn chat_eof_ends_session_with_summary() {
    // Closing stdin without /exit should still print the session summary
    // and exit successfully.
    let dir = setup_project();
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "reviewer"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .env("TAU_MOCK_LLM_TEXT", "echo")
        .write_stdin("")
        .assert()
        .success()
        .stderr(predicate::str::contains("session ended"));
}

#[cfg(feature = "test-mock")]
#[test]
fn chat_unknown_slash_is_forwarded_as_prompt() {
    // Per parser docs: `/foo` is not recognised, so it goes to the LLM
    // as a normal prompt. The mock echoes back our configured text;
    // we check the binary doesn't error on the unknown slash form.
    let dir = setup_project();
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "reviewer"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .env("TAU_MOCK_LLM_TEXT", "passthrough-response")
        .write_stdin("/foo\n/exit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("passthrough-response"));
}

#[cfg(feature = "test-mock")]
#[test]
fn chat_history_threads_across_turns() {
    // Two prompts in a row; after both, /history should show 4 entries
    // (user1, assistant1, user2, assistant2). This verifies
    // run_with_history is wiring `all_messages` back into the next call.
    let dir = setup_project();
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "reviewer"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .env("TAU_MOCK_LLM_TEXT", "reply")
        .write_stdin("first\nsecond\n/history\n/exit\n")
        .assert()
        .success()
        // The history rendering tags each line with its index; with two
        // full turns we should see at least entries [0] and [3].
        .stdout(predicate::str::contains("[0]"))
        .stdout(predicate::str::contains("[3]"));
}
