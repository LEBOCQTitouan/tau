//! Integration tests for `tau session export` (Tier 3 priority 11).

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
fn session_export_jsonl_passthrough() {
    let scope_dir = TempDir::new().unwrap();
    let sessions_dir = scope_dir.path().join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    let id = "01234567-0000-7000-8000-000000000001";
    write_fixture_session(&sessions_dir, id, "coder", "2026-05-01T14:33:21Z");
    let path = sessions_dir.join(format!("{id}.jsonl"));
    let expected = fs::read_to_string(&path).unwrap();

    let cmd_out = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "session", "export", "01234567", "--format", "jsonl", "--global",
        ])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&cmd_out.get_output().stdout).to_string();
    assert_eq!(
        stdout, expected,
        "stdout should byte-equal the file contents"
    );
}

#[test]
fn session_export_md_renders_markdown() {
    let scope_dir = TempDir::new().unwrap();
    let sessions_dir = scope_dir.path().join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    let id = "01234567-0000-7000-8000-000000000002";
    write_fixture_session(&sessions_dir, id, "coder", "2026-05-01T14:33:21Z");

    let cmd_out = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "session", "export", "01234567", "--format", "md", "--global",
        ])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&cmd_out.get_output().stdout).to_string();
    assert!(stdout.contains("# Session"), "stdout: {stdout}");
    assert!(stdout.contains("**Agent:**"), "stdout: {stdout}");
}

#[test]
fn session_export_json_envelope() {
    let scope_dir = TempDir::new().unwrap();
    let sessions_dir = scope_dir.path().join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    let id = "01234567-0000-7000-8000-000000000003";
    write_fixture_session(&sessions_dir, id, "coder", "2026-05-01T14:33:21Z");

    let cmd_out = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "session", "export", "01234567", "--format", "json", "--global",
        ])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&cmd_out.get_output().stdout).to_string();
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("envelope should parse as JSON");
    assert!(
        parsed.get("header").is_some(),
        "envelope missing header field"
    );
    assert!(
        parsed.get("messages").is_some(),
        "envelope missing messages field"
    );
    assert!(parsed["messages"].is_array(), "messages should be an array");
}

#[test]
fn session_export_unknown_id_exits_2() {
    let scope_dir = TempDir::new().unwrap();
    Command::cargo_bin("tau")
        .unwrap()
        .args([
            "session", "export", "00000000", "--format", "jsonl", "--global",
        ])
        .env("TAU_HOME", scope_dir.path())
        .assert()
        .failure()
        .code(2);
}
