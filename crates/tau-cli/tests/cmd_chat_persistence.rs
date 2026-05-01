//! Integration tests for `tau chat` session persistence (Tier 3 priority 11).
//!
//! Mirrors the established pattern from `cmd_chat.rs`: each test uses
//! [`common::setup_echo_project`] to spin up a real echo-llm plugin, then
//! drives the REPL via `write_stdin` and asserts filesystem or stdout/stderr
//! side-effects.

mod common;

use assert_cmd::Command as AssertCmd;
use predicates::prelude::*;

// ---- Test 1: non-ephemeral session creates a JSONL file --------------------

/// `tau chat <agent>` with one prompt + `/exit` should create a session file
/// under `<scope>/.tau/sessions/`.  The file must contain a header line and
/// at least one message line.
#[test]
fn chat_creates_session_file() {
    let dir = common::setup_echo_project("echo", "canned_text = \"hello\"\n", &[]);
    // The project-scope state dir is `<dir>/.tau`.  The global dir is set
    // to a sub-path that doesn't exist so the scope resolves to Project.
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "echo"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .write_stdin("hello world\n/exit\n")
        .assert()
        .success();

    // The project-scope .tau/sessions directory should exist and contain 1 file.
    let sessions_dir = dir.path().join(".tau").join("sessions");
    assert!(
        sessions_dir.exists(),
        "sessions dir should exist after a non-ephemeral chat session"
    );

    let files: Vec<_> = std::fs::read_dir(&sessions_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()).unwrap_or("") == "jsonl")
        .collect();

    assert_eq!(
        files.len(),
        1,
        "expected exactly 1 session file, found: {:?}",
        files.iter().map(|e| e.path()).collect::<Vec<_>>()
    );

    // The file must have at least 2 lines: header + ≥1 message.
    let path = files[0].path();
    let contents = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert!(
        lines.len() >= 2,
        "session file should have header + ≥1 message line; got {} lines",
        lines.len()
    );
    assert!(
        lines[0].contains(r#""type":"header""#),
        "first line should be the session header JSON"
    );
}

// ---- Test 2: --ephemeral writes no file ------------------------------------

/// `tau chat <agent> --ephemeral` should never touch the sessions directory.
#[test]
fn chat_ephemeral_writes_no_file() {
    let dir = common::setup_echo_project("echo", "canned_text = \"hi\"\n", &[]);
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "echo", "--ephemeral"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .write_stdin("hi\n/exit\n")
        .assert()
        .success()
        // Ephemeral sessions print "Session discarded." on exit.
        .stderr(predicate::str::contains("Session discarded."));

    // Sessions dir must not exist (or be empty) after an ephemeral session.
    let sessions_dir = dir.path().join(".tau").join("sessions");
    if sessions_dir.exists() {
        let jsonl_count = std::fs::read_dir(&sessions_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()).unwrap_or("") == "jsonl")
            .count();
        assert_eq!(
            jsonl_count, 0,
            "ephemeral session must not create any .jsonl files"
        );
    }
}

// ---- Test 3: /clear prints deprecation message ----------------------------

/// Driving `/clear\n/exit\n` should emit the deprecation guidance message
/// and then exit cleanly — it must NOT clear history or terminate.
#[test]
fn chat_clear_prints_deprecation_message() {
    let dir = common::setup_echo_project("echo", "canned_text = \"reply\"\n", &[]);
    let global_dir = dir.path().join("global");

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["chat", "echo"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .write_stdin("/clear\n/exit\n")
        .assert()
        .success()
        // Deprecation message must appear in stderr (status channel).
        .stderr(predicate::str::contains("/clear was removed"));
}
