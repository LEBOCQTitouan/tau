//! Integration tests for `tau init`.

mod common;

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn init_creates_tau_toml() {
    let dir = common::temp_project();

    Command::cargo_bin("tau")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("created"))
        .stderr(predicate::str::contains(".tau/"));

    let contents = common::read_tau_toml(dir.path());
    assert!(contents.contains("[project]"));
    assert!(contents.contains("[agents.example]"));
}

#[test]
fn init_refuses_existing_tau_toml() {
    let dir = common::temp_project_with_tau_toml(
        r#"[project]
name = "existing"
"#,
    );

    Command::cargo_bin("tau")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("already exists").or(predicate::str::contains("--force")));
}

#[test]
fn init_with_force_overwrites_existing() {
    let dir = common::temp_project_with_tau_toml(
        r#"[project]
name = "old"
"#,
    );

    Command::cargo_bin("tau")
        .unwrap()
        .args(["init", "--force"])
        .current_dir(dir.path())
        .assert()
        .success();

    let contents = common::read_tau_toml(dir.path());
    assert!(
        !contents.contains(r#"name = "old""#),
        "should have overwritten"
    );
    assert!(contents.contains("[agents.example]"));
}

#[test]
fn init_dry_run_does_not_write() {
    let dir = common::temp_project();

    Command::cargo_bin("tau")
        .unwrap()
        .args(["init", "--dry-run"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("[dry-run]"))
        .stderr(predicate::str::contains("would create"));

    assert!(
        !dir.path().join("tau.toml").exists(),
        "dry-run should not write"
    );
}

#[test]
fn init_dry_run_emits_scaffold_content_to_stderr() {
    // The scaffold contents are dumped via output.dry_run() lines, all on stderr.
    let dir = common::temp_project();

    let output = Command::cargo_bin("tau")
        .unwrap()
        .args(["init", "--dry-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("[project]"));
    assert!(stderr.contains("[agents.example]"));
    assert!(stderr.contains("Example Agent"));
}

#[test]
fn init_json_output_includes_path() {
    let dir = common::temp_project();

    let output = Command::cargo_bin("tau")
        .unwrap()
        .args(["init", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(parsed["created"], "tau.toml");
    assert_eq!(parsed["force"], false);
    assert!(parsed["path"].is_string());
}

#[test]
fn init_json_with_force_includes_true() {
    let dir = common::temp_project_with_tau_toml(
        r#"[project]
name = "old"
"#,
    );

    let output = Command::cargo_bin("tau")
        .unwrap()
        .args(["init", "--force", "--json"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    assert_eq!(parsed["force"], true);
}

#[test]
fn init_project_name_from_basename() {
    let parent = common::temp_project();
    let child_path = parent.path().join("my-cool-project");
    std::fs::create_dir(&child_path).unwrap();

    Command::cargo_bin("tau")
        .unwrap()
        .arg("init")
        .current_dir(&child_path)
        .assert()
        .success();

    let contents = std::fs::read_to_string(child_path.join("tau.toml")).unwrap();
    assert!(contents.contains(r#"name = "my-cool-project""#));
}
