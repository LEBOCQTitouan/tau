//! Integration tests for `tau list`.
//!
//! Mirrors the local-`file://` git fixture pattern from `cmd_install.rs`
//! so the suite has no network requirement, and isolates each test's
//! global scope under a tempdir via `TAU_HOME`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::Command;
use predicates::prelude::*;

/// Run `git` with `args` in `cwd`, panicking with stderr/stdout on failure.
fn run_git(cwd: &Path, args: &[&str]) {
    let output = StdCommand::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|e| panic!("git {args:?} spawn failure: {e}"));
    assert!(
        output.status.success(),
        "git {args:?} in {cwd:?} failed:\nstderr: {}\nstdout: {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout),
    );
}

/// Build a `file://` URL from a path, with forward slashes for portability.
fn file_url(path: &Path) -> String {
    let forward = path
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    if forward.starts_with('/') {
        format!("file://{forward}")
    } else {
        format!("file:///{forward}")
    }
}

/// Set up a bare git repository containing a minimal package `tau.toml`.
/// Returns `(tempdir, file_url, bare_path)`.
fn setup_local_package_fixture(name: &str, version: &str) -> (tempfile::TempDir, String, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");

    let bare = dir.path().join(format!("{name}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    run_git(&bare, &["init", "--bare", "-q"]);
    run_git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    let url = file_url(&bare);

    let work = dir.path().join(format!("{name}-work"));
    std::fs::create_dir_all(&work).unwrap();
    run_git(&work, &["init", "-q", "-b", "main"]);
    run_git(&work, &["config", "user.email", "test@example.com"]);
    run_git(&work, &["config", "user.name", "Test User"]);

    let manifest = format!(
        r#"name = "{name}"
version = "{version}"
description = "test fixture"
authors = ["Test <test@example.com>"]
source = "{url}"
kind = "tool"
dependencies = []
capabilities = []
"#
    );
    std::fs::write(work.join("tau.toml"), manifest).unwrap();

    run_git(&work, &["add", "tau.toml"]);
    run_git(&work, &["commit", "-q", "-m", "initial"]);
    run_git(&work, &["remote", "add", "origin", &bare.to_string_lossy()]);
    run_git(&work, &["push", "-q", "origin", "main"]);

    (dir, url, bare)
}

#[test]
fn list_packages_empty_scope_says_no_packages() {
    let global_dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("tau")
        .unwrap()
        .args(["list", "--global"])
        .env("TAU_HOME", global_dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("no packages installed"));
}

#[test]
fn list_packages_after_install_shows_row() {
    let (fixture, url, _bare) = setup_local_package_fixture("hello-tool", "0.1.0");
    let global_dir = fixture.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    // Install first.
    Command::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", &url])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success();

    // Then list.
    Command::cargo_bin("tau")
        .unwrap()
        .args(["list", "--global"])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("hello-tool"))
        .stdout(predicate::str::contains("0.1.0"))
        .stdout(predicate::str::contains("global"));
}

#[test]
fn list_packages_json_output_is_array_with_expected_fields() {
    let (fixture, url, _bare) = setup_local_package_fixture("hello-tool", "0.1.0");
    let global_dir = fixture.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    Command::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", &url])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success();

    let output = Command::cargo_bin("tau")
        .unwrap()
        .args(["list", "--global", "--json"])
        .env("TAU_HOME", &global_dir)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "tau list --json failed: stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("list --json should emit a JSON array");
    assert!(parsed.is_array(), "expected JSON array, got: {parsed}");
    let rows = parsed.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "hello-tool");
    assert_eq!(rows[0]["version"], "0.1.0");
    assert_eq!(rows[0]["scope"], "global");
}

#[test]
fn list_agents_with_no_project_tau_toml_fails_with_init_hint() {
    let dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("tau")
        .unwrap()
        .args(["list", "agents"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path())
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("tau init"));
}

#[test]
fn list_agents_reads_project_tau_toml() {
    let dir = tempfile::tempdir().unwrap();
    let toml_str = r#"
[project]
name = "demo"

[agents.reviewer]
display_name = "Code Reviewer"
package      = "code-reviewer@^0.1"
llm_backend  = "anthropic"

[agents.committer]
display_name = "Code Committer"
package      = "code-committer@^0.1"
llm_backend  = "anthropic"
"#;
    std::fs::write(dir.path().join("tau.toml"), toml_str).unwrap();

    Command::cargo_bin("tau")
        .unwrap()
        .args(["list", "agents"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("reviewer"))
        .stdout(predicate::str::contains("committer"))
        .stdout(predicate::str::contains("Code Reviewer"));
}

#[test]
fn list_agents_json_output_is_array_with_expected_fields() {
    let dir = tempfile::tempdir().unwrap();
    let toml_str = r#"
[project]
name = "demo"

[agents.reviewer]
display_name = "Code Reviewer"
package      = "code-reviewer@^0.1"
llm_backend  = "anthropic"
"#;
    std::fs::write(dir.path().join("tau.toml"), toml_str).unwrap();

    let output = Command::cargo_bin("tau")
        .unwrap()
        .args(["list", "agents", "--json"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "tau list agents --json failed: stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("list agents --json should emit a JSON array");
    assert!(parsed.is_array());
    let rows = parsed.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "reviewer");
    assert_eq!(rows[0]["display_name"], "Code Reviewer");
    assert_eq!(rows[0]["package"], "code-reviewer@^0.1");
    assert_eq!(rows[0]["llm_backend"], "anthropic");
}

#[test]
fn list_dry_run_rejected_with_read_only_message() {
    let global_dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("tau")
        .unwrap()
        .args(["list", "--dry-run"])
        .env("TAU_HOME", global_dir.path())
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("dry-run").and(predicate::str::contains("read-only")));
}

#[test]
fn list_global_and_all_mutually_exclusive() {
    Command::cargo_bin("tau")
        .unwrap()
        .args(["list", "--global", "--all"])
        .assert()
        .failure();
}
