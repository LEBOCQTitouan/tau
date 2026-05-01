//! Integration tests for `tau session list` (Tier 3 priority 11).

mod common;

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn session_list_empty_returns_zero() {
    let scope_dir = TempDir::new().unwrap();
    let cmd_out = Command::cargo_bin("tau")
        .unwrap()
        .args(["session", "list", "--global"])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&cmd_out.get_output().stdout).to_string();
    assert!(stdout.contains("No sessions"), "stdout was: {stdout}");
}

#[test]
fn session_list_multiple_returns_descending() {
    let scope_dir = TempDir::new().unwrap();
    let sessions_dir = scope_dir.path().join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();

    // Manually craft 2 session files (simpler than running tau chat twice).
    write_fixture_session(
        &sessions_dir,
        "01234567-0000-7000-8000-000000000001",
        "agent-a",
        "2026-04-30T09:12:04Z",
    );
    write_fixture_session(
        &sessions_dir,
        "abcdef00-0000-7000-8000-000000000002",
        "agent-b",
        "2026-05-01T14:33:21Z",
    );

    let cmd_out = Command::cargo_bin("tau")
        .unwrap()
        .args(["session", "list", "--global"])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&cmd_out.get_output().stdout).to_string();
    let agent_b_pos = stdout.find("agent-b").expect("agent-b should appear");
    let agent_a_pos = stdout.find("agent-a").expect("agent-a should appear");
    assert!(
        agent_b_pos < agent_a_pos,
        "agent-b (newer) should come before agent-a (older); stdout: {stdout}"
    );
}

#[test]
fn session_list_filter_by_agent() {
    let scope_dir = TempDir::new().unwrap();
    let sessions_dir = scope_dir.path().join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();

    write_fixture_session(
        &sessions_dir,
        "01234567-0000-7000-8000-000000000001",
        "coder",
        "2026-05-01T10:00:00Z",
    );
    write_fixture_session(
        &sessions_dir,
        "abcdef00-0000-7000-8000-000000000002",
        "notes",
        "2026-05-01T11:00:00Z",
    );

    let cmd_out = Command::cargo_bin("tau")
        .unwrap()
        .args(["session", "list", "coder", "--global"])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&cmd_out.get_output().stdout).to_string();
    assert!(stdout.contains("coder"), "stdout: {stdout}");
    assert!(!stdout.contains("notes"), "stdout: {stdout}");
}

#[test]
fn session_list_json_emits_one_event_per_line() {
    let scope_dir = TempDir::new().unwrap();
    let sessions_dir = scope_dir.path().join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();

    write_fixture_session(
        &sessions_dir,
        "01234567-0000-7000-8000-000000000001",
        "coder",
        "2026-05-01T14:33:21Z",
    );

    let cmd_out = Command::cargo_bin("tau")
        .unwrap()
        .args(["--json", "session", "list", "--global"])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&cmd_out.get_output().stdout).to_string();

    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(
        lines.len() >= 2,
        "expected at least 2 events; stdout: {stdout}"
    );
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|e| panic!("invalid JSON: {line:?} ({e})"));
        assert!(parsed.get("event").is_some(), "no event field in: {line}");
    }
}

fn write_fixture_session(dir: &std::path::Path, id: &str, agent: &str, created_at: &str) {
    let header = format!(
        r#"{{"type":"header","schema":1,"id":"{id}","created_at":"{created_at}","agent_id":"{agent}","package":{{"name":"x","version":"1.0.0","resolved_commit":"0000000000000000000000000000000000000000"}},"llm_backend":"anthropic"}}
"#
    );
    fs::write(dir.join(format!("{id}.jsonl")), header).unwrap();
}
