//! Integration tests for `tau chat --resume <id>` (Tier 3 priority 11).
//!
//! Mirrors the established pattern from `cmd_chat_persistence.rs` (Task 5):
//! each test uses [`common::setup_echo_project`] to spin up a real echo-llm
//! plugin, drives the REPL via `write_stdin`, and asserts filesystem or
//! stdout/stderr side-effects.
//!
//! Drift tests (strict + force) use manually-crafted session files with
//! a mismatched `package.version` header field to avoid requiring two
//! real package versions to be installed.

mod common;

use assert_cmd::Command as AssertCmd;
use predicates::prelude::*;

// ---- Helpers -----------------------------------------------------------------

/// Build a valid session header JSONL line with the given id and package
/// version. Uses serde_json to guarantee valid JSON.
fn make_drifted_session_header(id: &str, version: &str) -> String {
    // Build the JSON manually but safely.
    let resolved_commit = "0".repeat(40);
    format!(
        concat!(
            r#"{{"type":"header","schema":1,"id":"{id}","created_at":"2026-05-01T00:00:00Z","#,
            r#""agent_id":"echo","package":{{"name":"echo-llm","version":"{version}","#,
            r#""resolved_commit":"{commit}"}},"llm_backend":"echo-llm"}}"#
        ),
        id = id,
        version = version,
        commit = resolved_commit,
    )
}

/// Run `tau chat <agent>` with the given extra args and stdin, returning
/// the assert handle. TAU_HOME is redirected to a non-existent subdir so
/// the scope resolves to Project scope.
fn tau_chat(
    dir: &std::path::Path,
    global_dir: &std::path::Path,
    extra_args: &[&str],
    stdin: &str,
) -> assert_cmd::assert::Assert {
    let mut cmd = AssertCmd::cargo_bin("tau").unwrap();
    cmd.args(["chat", "echo"])
        .args(extra_args)
        .current_dir(dir)
        .env("TAU_HOME", global_dir)
        .write_stdin(stdin.to_string());
    cmd.assert()
}

/// Find the single `.jsonl` session file created under `<dir>/.tau/sessions/`.
/// Panics if none or more than one.
fn find_session_file(dir: &std::path::Path) -> std::path::PathBuf {
    let sessions_dir = dir.join(".tau").join("sessions");
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
    files[0].path()
}

// ---- Test 1: resume loads history (session file grows) ----------------------

/// Start a session, exit, resume with --resume <prefix>, assert the session
/// file grew (more lines than before resume).
#[test]
fn chat_resume_loads_history() {
    let dir = common::setup_echo_project("echo", "canned_text = \"hello\"\n", &[]);
    let global_dir = dir.path().join("global");

    // First session: one prompt + /exit.
    tau_chat(dir.path(), &global_dir, &[], "first message\n/exit\n").success();

    let path = find_session_file(dir.path());
    let lines_before = std::fs::read_to_string(&path).unwrap().lines().count();
    assert!(
        lines_before >= 2,
        "first session should have at least header + 1 message line"
    );

    // Extract the 8-char prefix from the filename stem.
    let stem = path.file_stem().unwrap().to_string_lossy().to_string();
    let prefix = &stem[..8];

    // Second session: resume with prefix, send another prompt + /exit.
    tau_chat(
        dir.path(),
        &global_dir,
        &["--resume", prefix],
        "second message\n/exit\n",
    )
    .success()
    .stderr(predicate::str::contains("Resumed session"));

    let lines_after = std::fs::read_to_string(&path).unwrap().lines().count();
    assert!(
        lines_after > lines_before,
        "session file should have grown after resume; before={lines_before} after={lines_after}"
    );
}

// ---- Test 2: unknown id exits 2 ---------------------------------------------

/// `tau chat <agent> --resume 00000000` → exit 2; stderr contains "not found".
#[test]
fn chat_resume_unknown_id_exits_2() {
    let dir = common::setup_echo_project("echo", "canned_text = \"hello\"\n", &[]);
    let global_dir = dir.path().join("global");

    // Create the sessions directory so scope resolves correctly, but no
    // matching file exists.
    std::fs::create_dir_all(dir.path().join(".tau").join("sessions")).unwrap();

    tau_chat(
        dir.path(),
        &global_dir,
        &["--resume", "00000000"],
        "", // no stdin needed — exits before entering REPL
    )
    .failure()
    .code(2)
    .stderr(predicate::str::contains("not found"));
}

// ---- Test 3: drift detection (strict) exits 2 --------------------------------

/// Manually craft a session file with `package.version = "999.999.999"` (which
/// differs from the installed `0.1.0`). Resume without --force → exit 2 with
/// "drift" in stderr.
#[test]
fn chat_resume_strict_drift_exits_2() {
    let dir = common::setup_echo_project("echo", "canned_text = \"hello\"\n", &[]);
    let global_dir = dir.path().join("global");

    // Create the sessions directory.
    let sessions_dir = dir.path().join(".tau").join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();

    // Mint a session id and craft a header with a mismatched package version.
    let id = uuid::Uuid::now_v7().to_string();
    let path = sessions_dir.join(format!("{id}.jsonl"));
    let header_json = make_drifted_session_header(&id, "999.999.999");
    std::fs::write(&path, format!("{header_json}\n")).unwrap();

    let prefix = &id[..8];
    tau_chat(dir.path(), &global_dir, &["--resume", prefix], "")
        .failure()
        .code(2)
        .stderr(predicate::str::contains("drift"));
}

// ---- Test 4: --force bypasses drift -----------------------------------------

/// Same setup as the drift test, but with --force. Exit 0. stderr should
/// contain a warning about the version mismatch.
#[test]
fn chat_resume_force_bypasses_drift() {
    let dir = common::setup_echo_project("echo", "canned_text = \"hello\"\n", &[]);
    let global_dir = dir.path().join("global");

    // Create the sessions directory.
    let sessions_dir = dir.path().join(".tau").join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();

    // Craft a session file with a mismatched package version.
    let id = uuid::Uuid::now_v7().to_string();
    let path = sessions_dir.join(format!("{id}.jsonl"));
    let header_json = make_drifted_session_header(&id, "999.999.999");
    std::fs::write(&path, format!("{header_json}\n")).unwrap();

    let prefix = &id[..8];
    tau_chat(
        dir.path(),
        &global_dir,
        &["--resume", prefix, "--force"],
        "/exit\n",
    )
    .success()
    .stderr(predicate::str::contains("warning: resuming with"));
}
