//! Integration tests for `tau install`.
//!
//! Uses local `file://`-based git fixtures (bare repo + working repo
//! pattern from sub-project 3 / tau-pkg) so the suite has no network
//! requirement. Each test sets `TAU_HOME` to an isolated tempdir so
//! global-scope installs do not leak across tests or pollute the
//! developer's real `~/.tau`.
//!
//! Fixes preserved from sub-project 3 Task 14:
//! - `git symbolic-ref HEAD refs/heads/main` on the bare repo so the
//!   downstream clone checks out the right branch regardless of the
//!   host's `init.defaultBranch` (CI runners default to `master`).
//! - tau-pkg's install path threads `protocol.file.allow=always` for
//!   CVE-2022-39253 mitigation; tests rely on that being plumbed.

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
///
/// Returns `(tempdir, file_url, bare_path)`. The tempdir owns both the
/// bare repo and the working repo; both go away when it drops.
///
/// The manifest's declared `source` matches the bare repo's `file://`
/// URL so tau-pkg's source/manifest match check passes.
fn setup_local_package_fixture(name: &str, version: &str) -> (tempfile::TempDir, String, PathBuf) {
    setup_local_package_fixture_with_kind(name, version, "tool")
}

/// Same as [`setup_local_package_fixture`] but with an explicit `kind`.
fn setup_local_package_fixture_with_kind(
    name: &str,
    version: &str,
    kind: &str,
) -> (tempfile::TempDir, String, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");

    // Bare repo (clone target).
    let bare = dir.path().join(format!("{name}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    run_git(&bare, &["init", "--bare", "-q"]);
    // Force the bare HEAD to refs/heads/main so `git clone` checks out the
    // right branch regardless of the host's init.defaultBranch.
    run_git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    let url = file_url(&bare);

    // Working repo where we author the initial commit.
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
kind = "{kind}"
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
fn install_local_file_url_writes_to_global_scope() {
    let (fixture, url, _bare) = setup_local_package_fixture("hello-tool", "0.1.0");
    let global_dir = fixture.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    Command::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", &url])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("hello-tool"))
        .stdout(predicate::str::contains("0.1.0"))
        .stdout(predicate::str::contains("global"));

    // Verify the package directory exists at the canonical scope path.
    let pkg_dir = global_dir.join("packages/hello-tool/0.1.0");
    assert!(
        pkg_dir.exists(),
        "package not at expected scope path: {}",
        pkg_dir.display(),
    );
    // And that its tau.toml made it through.
    assert!(
        pkg_dir.join("tau.toml").is_file(),
        "tau.toml missing in installed package: {}",
        pkg_dir.display(),
    );
}

#[test]
fn install_dry_run_does_not_write() {
    let (fixture, url, _bare) = setup_local_package_fixture("hello-tool", "0.1.0");
    let global_dir = fixture.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    Command::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", "--dry-run", &url])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success()
        .stderr(predicate::str::contains("[dry-run]"))
        .stderr(predicate::str::contains("would install"))
        .stderr(predicate::str::contains("hello-tool"))
        .stderr(predicate::str::contains("0.1.0"))
        .stderr(predicate::str::contains("kind:"))
        .stderr(predicate::str::contains("tool"))
        .stderr(predicate::str::contains("no changes written."));

    // Dry-run must NOT create the package directory.
    let pkg_dir = global_dir.join("packages/hello-tool/0.1.0");
    assert!(
        !pkg_dir.exists(),
        "dry-run should not write package dir: {}",
        pkg_dir.display(),
    );
    // And no lockfile entry.
    let lockfile = global_dir.join("tau-lock.toml");
    assert!(
        !lockfile.exists(),
        "dry-run should not write lockfile: {}",
        lockfile.display(),
    );
}

#[test]
fn install_bad_url_fails_with_exit_2() {
    let global_dir = tempfile::tempdir().unwrap();

    Command::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", "not-a-url"])
        .env("TAU_HOME", global_dir.path())
        .assert()
        .failure()
        .code(2);
}

#[test]
fn install_json_output_includes_name_version_scope_path() {
    let (fixture, url, _bare) = setup_local_package_fixture("hello-tool", "0.1.0");
    let global_dir = fixture.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    let output = Command::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", "--json", &url])
        .env("TAU_HOME", &global_dir)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "tau install --json failed: stderr={}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("install --json should emit a JSON object");
    assert_eq!(parsed["name"], "hello-tool");
    assert_eq!(parsed["version"], "0.1.0");
    assert_eq!(parsed["scope"], "global");
    assert!(parsed["path"].is_string());
}
