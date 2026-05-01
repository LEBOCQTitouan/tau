//! Integration tests for `tau session delete` (Tier 3 priority 11).

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

fn write_fixture_session(dir: &std::path::Path, id: &str, agent: &str, created_at: &str) {
    let content = format!(
        r#"{{"type":"header","schema":1,"id":"{id}","created_at":"{created_at}","agent_id":"{agent}","package":{{"name":"x","version":"1.0.0","resolved_commit":"0000000000000000000000000000000000000000"}},"llm_backend":"anthropic"}}
"#
    );
    fs::write(dir.join(format!("{id}.jsonl")), content).unwrap();
}

#[test]
fn session_delete_with_force_removes_file() {
    let scope_dir = TempDir::new().unwrap();
    let sessions_dir = scope_dir.path().join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    let id = "01234567-0000-7000-8000-000000000001";
    write_fixture_session(&sessions_dir, id, "coder", "2026-05-01T14:33:21Z");

    let path = sessions_dir.join(format!("{id}.jsonl"));
    assert!(path.exists());

    Command::cargo_bin("tau")
        .unwrap()
        .args(["session", "delete", "01234567", "--global", "--force"])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .success();

    assert!(!path.exists(), "session file should be deleted");
}

#[test]
fn session_delete_prompt_yes_removes_file() {
    let scope_dir = TempDir::new().unwrap();
    let sessions_dir = scope_dir.path().join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    let id = "01234567-0000-7000-8000-000000000002";
    write_fixture_session(&sessions_dir, id, "coder", "2026-05-01T14:33:21Z");

    let path = sessions_dir.join(format!("{id}.jsonl"));

    Command::cargo_bin("tau")
        .unwrap()
        .args(["session", "delete", "01234567", "--global"])
        .env("TAU_HOME", scope_dir.path())
        .write_stdin("y\n")
        .assert()
        .success();

    assert!(!path.exists());
}

#[test]
fn session_delete_prompt_no_keeps_file() {
    let scope_dir = TempDir::new().unwrap();
    let sessions_dir = scope_dir.path().join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    let id = "01234567-0000-7000-8000-000000000003";
    write_fixture_session(&sessions_dir, id, "coder", "2026-05-01T14:33:21Z");

    let path = sessions_dir.join(format!("{id}.jsonl"));

    Command::cargo_bin("tau")
        .unwrap()
        .args(["session", "delete", "01234567", "--global"])
        .env("TAU_HOME", scope_dir.path())
        .write_stdin("n\n")
        .assert()
        .success();

    assert!(
        path.exists(),
        "session file should still exist after declining prompt"
    );
}

#[test]
fn session_delete_unknown_id_exits_2() {
    let scope_dir = TempDir::new().unwrap();

    Command::cargo_bin("tau")
        .unwrap()
        .args(["session", "delete", "00000000", "--global", "--force"])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .failure()
        .code(2);
}
