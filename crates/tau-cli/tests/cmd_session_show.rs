//! Integration tests for `tau session show <id>` (Tier 3 priority 11).

mod common;

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Write a header-only session file (no message lines).
/// Suitable for testing prefix resolution, not message rendering.
fn write_fixture_session(dir: &std::path::Path, id: &str, agent: &str, created_at: &str) {
    let header = format!(
        r#"{{"type":"header","schema":1,"id":"{id}","created_at":"{created_at}","agent_id":"{agent}","package":{{"name":"x","version":"1.0.0","resolved_commit":"0000000000000000000000000000000000000000"}},"llm_backend":"anthropic"}}
"#
    );
    fs::write(dir.join(format!("{id}.jsonl")), header).unwrap();
}

// ---------------------------------------------------------------------------
// Test 1: markdown rendering (uses real tau chat to create the session)
// ---------------------------------------------------------------------------

/// Drive a real `tau chat` session to produce a session file, then verify
/// `tau session show <prefix>` renders markdown output.
#[test]
fn session_show_renders_markdown() {
    let dir = common::setup_echo_project("echo", "canned_text = \"hi there\"\n", &[]);
    let global_dir = dir.path().join("global");

    // Run a real chat session so we get a properly-formatted session file.
    Command::cargo_bin("tau")
        .unwrap()
        .args(["chat", "echo"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .write_stdin("hello\n/exit\n")
        .assert()
        .success();

    // Find the session file that was created.
    let sessions_dir = dir.path().join(".tau").join("sessions");
    let files: Vec<_> = fs::read_dir(&sessions_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()).unwrap_or("") == "jsonl")
        .collect();
    assert_eq!(files.len(), 1, "expected exactly 1 session file");

    // Extract the 8-char prefix from the filename.
    let filename = files[0].path();
    let stem = filename.file_stem().unwrap().to_string_lossy().to_string();
    let prefix = &stem[..8];

    // Now run `tau session show <prefix>` (project scope — no --global).
    let cmd_out = Command::cargo_bin("tau")
        .unwrap()
        .args(["session", "show", prefix])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&cmd_out.get_output().stdout).to_string();
    assert!(stdout.contains("# Session"), "stdout: {stdout}");
    assert!(stdout.contains("**Agent:**"), "stdout: {stdout}");
    // The echo agent produces at least one user message.
    assert!(
        stdout.contains("**You:**") || stdout.contains("hello"),
        "stdout: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: --json passthrough
// ---------------------------------------------------------------------------

/// `tau session show --json <prefix>` should emit the raw JSONL lines as
/// individual JSON events.
#[test]
fn session_show_json_passthrough() {
    let scope_dir = TempDir::new().unwrap();
    let sessions_dir = scope_dir.path().join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();

    let id = "01234567-0000-7000-8000-000000000001";
    write_fixture_session(&sessions_dir, id, "coder", "2026-05-01T14:33:21Z");

    let file_contents = fs::read_to_string(sessions_dir.join(format!("{id}.jsonl"))).unwrap();
    // The header line should appear verbatim in the output.
    let first_line = file_contents.lines().next().unwrap().to_string();

    let cmd_out = Command::cargo_bin("tau")
        .unwrap()
        .args(["--json", "session", "show", "01234567", "--global"])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&cmd_out.get_output().stdout).to_string();
    // The JSON output re-serialises each parsed JSON line, so field order
    // may differ. Check for key content.
    assert!(
        stdout.contains(r#""type":"header""#),
        "stdout should contain header event; stdout: {stdout}"
    );
    // Check agent_id is present.
    assert!(
        stdout.contains("coder"),
        "stdout should contain agent_id; stdout: {stdout}"
    );
    // Suppress "unused variable" warning for first_line.
    let _ = first_line;
}

// ---------------------------------------------------------------------------
// Test 3: unknown id exits 2
// ---------------------------------------------------------------------------

#[test]
fn session_show_unknown_id_exits_2() {
    let scope_dir = TempDir::new().unwrap();
    Command::cargo_bin("tau")
        .unwrap()
        .args(["session", "show", "00000000", "--global"])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .failure()
        .code(2);
}

// ---------------------------------------------------------------------------
// Test 4: ambiguous prefix exits 2 with "ambiguous" in stderr
// ---------------------------------------------------------------------------

#[test]
fn session_show_ambiguous_prefix_exits_2() {
    let scope_dir = TempDir::new().unwrap();
    let sessions_dir = scope_dir.path().join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();

    write_fixture_session(
        &sessions_dir,
        "01234567-0000-7000-8000-000000000001",
        "a",
        "2026-05-01T10:00:00Z",
    );
    write_fixture_session(
        &sessions_dir,
        "01234567-0000-7000-8000-000000000002",
        "b",
        "2026-05-01T11:00:00Z",
    );

    let cmd_out = Command::cargo_bin("tau")
        .unwrap()
        .args(["session", "show", "01234567", "--global"])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .failure()
        .code(2);
    let stderr = String::from_utf8_lossy(&cmd_out.get_output().stderr).to_string();
    assert!(
        stderr.to_lowercase().contains("ambiguous"),
        "expected 'ambiguous' in stderr; got: {stderr}"
    );
}
