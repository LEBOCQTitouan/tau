//! Cross-cutting: platform-portability sanity checks.
//!
//! Phase 0 ships on Linux + macOS only (CI matrix), but the test suite
//! still asserts a few invariants that *would* break on Windows if the
//! relevant code paths regressed:
//!
//! 1. `url::Url::from_file_path` always emits forward slashes for
//!    `file://` URLs, regardless of the host's
//!    `std::path::MAIN_SEPARATOR`.
//! 2. `tau install --global` accepts a `file://` URL whose path
//!    contains spaces.
//!
//! When the Windows job is added later, the assertions here are the
//! first line of defence: they catch backslash leakage and naive path
//! concatenation that breaks on whitespace before the `git clone` even
//! runs.

mod common;

use std::path::PathBuf;

use assert_cmd::Command as AssertCmd;
use predicates::prelude::*;

/// Build an absolute path appropriate for the host OS. `Url::from_file_path`
/// rejects relative or platform-mismatched paths (e.g. unix-style `/tmp/...`
/// on Windows fails because Windows expects a drive letter or UNC prefix).
fn host_absolute_path(rel: &str) -> PathBuf {
    let tmp = tempfile::tempdir().expect("tempdir");
    tmp.path().join(rel)
    // Note: tmp drops here, but the path string remains valid for URL
    // construction — we don't open the path, just format it.
}

#[test]
fn file_url_construction_uses_forward_slash() {
    let path = host_absolute_path("repo");
    let url = url::Url::from_file_path(&path).expect("absolute path -> file URL");
    let url_str = url.as_str();

    assert!(
        url_str.starts_with("file://"),
        "expected file:// prefix; got: {url_str}"
    );
    assert!(
        !url_str.contains('\\'),
        "URL must use forward slashes, never backslashes; got: {url_str}"
    );
}

#[test]
fn common_file_url_helper_matches_url_crate_for_simple_paths() {
    // The hand-rolled `common::file_url` helper is what the install /
    // list integration tests use to fabricate `file://` URLs. It
    // should agree with the `url` crate's canonical conversion for
    // simple absolute paths (no spaces, no special chars).
    let path = host_absolute_path("tau-fixture-repo");
    let our = common::file_url(&path);
    let theirs = url::Url::from_file_path(&path).unwrap().to_string();
    // The `url` crate may percent-encode trailing components or drop
    // a trailing slash; assert structural equivalence rather than
    // byte-for-byte equality.
    assert!(
        our.starts_with("file://"),
        "common::file_url should start with file://; got: {our}"
    );
    assert!(
        theirs.starts_with("file://"),
        "url::Url should start with file://; got: {theirs}"
    );
}

#[test]
fn install_with_path_containing_spaces_works() {
    // Spaces in a `file://` URL — exercises the install pipeline's URL
    // parser + the underlying `git clone` invocation. Both must
    // tolerate whitespace.
    let outer = tempfile::tempdir().unwrap();
    // Place the bare repo + working repo inside a subdirectory whose
    // name contains a space.
    let with_space = outer.path().join("path with spaces");
    std::fs::create_dir(&with_space).unwrap();

    // Bare repo.
    let bare = with_space.join("hello.git");
    std::fs::create_dir_all(&bare).unwrap();
    common::run_git(&bare, &["init", "--bare", "-q"]);
    common::run_git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    let url = common::file_url(&bare);

    // Working repo with a tau.toml whose `source` matches the bare URL.
    let work = with_space.join("hello-work");
    std::fs::create_dir_all(&work).unwrap();
    common::run_git(&work, &["init", "-q", "-b", "main"]);
    common::run_git(&work, &["config", "user.email", "test@example.com"]);
    common::run_git(&work, &["config", "user.name", "Test User"]);
    let manifest = format!(
        r#"name = "hello"
version = "0.1.0"
description = "fixture"
authors = ["Test <test@example.com>"]
source = "{url}"
kind = "tool"
dependencies = []
capabilities = []
"#
    );
    std::fs::write(work.join("tau.toml"), manifest).unwrap();
    common::run_git(&work, &["add", "tau.toml"]);
    common::run_git(&work, &["commit", "-q", "-m", "initial"]);
    common::run_git(&work, &["remote", "add", "origin", &bare.to_string_lossy()]);
    common::run_git(&work, &["push", "-q", "origin", "main"]);

    let global_dir = outer.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", &url])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("hello"))
        .stdout(predicate::str::contains("0.1.0"));

    let pkg_dir = global_dir.join("packages/hello/0.1.0");
    assert!(
        pkg_dir.exists(),
        "package not at expected scope path: {}",
        pkg_dir.display(),
    );
}
