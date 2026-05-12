//! CLI integration tests for `tau workflow ...`.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn workflow_list_prints_each_toml_basename() {
    let dir = TempDir::new().unwrap();
    let wf_dir = dir.path().join("workflows");
    fs::create_dir_all(&wf_dir).unwrap();
    fs::write(wf_dir.join("alpha.toml"), b"[workflow]\n").unwrap();
    fs::write(wf_dir.join("beta.toml"), b"[workflow]\n").unwrap();

    let assert = Command::cargo_bin("tau")
        .unwrap()
        .arg("workflow")
        .arg("list")
        .current_dir(dir.path())
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(out.contains("alpha"), "missing alpha; got {out}");
    assert!(out.contains("beta"), "missing beta; got {out}");
}

#[test]
fn workflow_list_handles_no_workflows_dir() {
    let dir = TempDir::new().unwrap();
    Command::cargo_bin("tau")
        .unwrap()
        .arg("workflow")
        .arg("list")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("No workflows/ directory"));
}
