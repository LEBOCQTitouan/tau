//! Integration tests for `tau chat`.
//!
//! Same hand-author fixture pattern as `cmd_run.rs`: write a project
//! `tau.toml` plus an installed package + LLM-backend lockfile entry,
//! then drive the binary via `assert_cmd`. Each REPL interaction is a
//! `write_stdin` payload terminated by `/exit\n` so the loop returns
//! cleanly.
//!
//! Tests that exercise the actual REPL loop are marked
//! `#[ignore = "TODO(task-21): ..."]`: the v0.1 transitional
//! `--features test-mock` mock backend was retired in Task 19, and
//! Task 21 rewrites these against real `echo-llm` / `echo-tool`
//! binary spawns.
//!
//! Fixture builders live in `tests/common/mod.rs`.

mod common;

use assert_cmd::Command as AssertCmd;
use predicates::prelude::*;

// ---- "easy" tests (no fixture / no mock LLM needed) -------------------------

#[test]
fn chat_rejects_json_flag() {
    // --json is rejected at handler entry — no project setup required
    // for the assertion, but we provide one anyway so the test exercises
    // the same dispatch path the others do.
    let dir = common::setup_project();
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
    let dir = common::setup_project();
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

// ---- REPL-driven tests (ignored until Task 21 rewires real plugin spawn) ----

#[test]
#[ignore = "TODO(task-21): rewrite against real echo-llm spawn"]
fn chat_dry_run_skips_repl() {
    let dir = common::setup_project();
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

#[test]
#[ignore = "TODO(task-21): rewrite against real echo-llm spawn"]
fn chat_repl_one_round_via_stdin_pipe() {
    let dir = common::setup_project();
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

#[test]
#[ignore = "TODO(task-21): rewrite against real echo-llm spawn"]
fn chat_help_command_lists_slash_commands() {
    let dir = common::setup_project();
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

#[test]
#[ignore = "TODO(task-21): rewrite against real echo-llm spawn"]
fn chat_clear_resets_history() {
    let dir = common::setup_project();
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

#[test]
#[ignore = "TODO(task-21): rewrite against real echo-llm spawn"]
fn chat_eof_ends_session_with_summary() {
    // Closing stdin without /exit should still print the session summary
    // and exit successfully.
    let dir = common::setup_project();
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

#[test]
#[ignore = "TODO(task-21): rewrite against real echo-llm spawn"]
fn chat_unknown_slash_is_forwarded_as_prompt() {
    // Per parser docs: `/foo` is not recognised, so it goes to the LLM
    // as a normal prompt. The mock echoes back our configured text;
    // we check the binary doesn't error on the unknown slash form.
    let dir = common::setup_project();
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

#[test]
#[ignore = "TODO(task-21): rewrite against real echo-llm spawn"]
fn chat_history_threads_across_turns() {
    // Two prompts in a row; after both, /history should show 4 entries
    // (user1, assistant1, user2, assistant2). This verifies
    // run_with_history is wiring `all_messages` back into the next call.
    let dir = common::setup_project();
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
