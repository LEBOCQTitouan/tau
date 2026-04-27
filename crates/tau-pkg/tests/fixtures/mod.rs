//! Shared test fixtures for tau-pkg integration tests.
//!
//! Provides helpers to create local bare git repos with synthetic
//! `tau.toml` manifests, suitable for `file://`-based install tests.
//! No network access required.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// Returns true if `git --version` succeeds on the host. Integration
/// tests that need git skip cleanly when this returns false.
pub fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
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

/// Create an empty bare git repo at `<parent>/<name>.git`. Returns the path.
pub fn make_bare_repo(parent: &Path, name: &str) -> PathBuf {
    let bare = parent.join(format!("{name}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    run_git(&bare, &["init", "--bare", "-q"]);
    bare
}

/// Create a bare repo and populate it with a single commit containing a
/// minimal valid `tau.toml`. Returns the bare repo's filesystem path
/// (suitable for use as a `file://` URL).
///
/// `name`/`version` go into the manifest. `kind` is the `kind` string
/// (e.g. "tool"). The manifest's `source` field is set to the file://
/// URL of the bare repo so install's source/manifest match check passes.
pub fn make_fixture_repo(parent: &Path, name: &str, version: &str, kind: &str) -> PathBuf {
    let bare = make_bare_repo(parent, name);
    let working = parent.join(format!("{name}-working"));
    std::fs::create_dir_all(&working).unwrap();

    // Configure git identity for the test commit (CI runners may not have one).
    run_git(&working, &["init", "-q", "-b", "main"]);
    run_git(&working, &["config", "user.email", "test@example.com"]);
    run_git(&working, &["config", "user.name", "Test User"]);

    let source_url = format!("file://{}", bare.display());
    let manifest = format!(
        r#"name = "{name}"
version = "{version}"
description = "Synthetic fixture for tau-pkg integration tests"
authors = ["Test <test@example.com>"]
source = "{source_url}"
kind = "{kind}"
dependencies = []
capabilities = []
"#
    );
    std::fs::write(working.join("tau.toml"), manifest).unwrap();

    run_git(&working, &["add", "tau.toml"]);
    run_git(&working, &["commit", "-q", "-m", "initial fixture commit"]);
    run_git(
        &working,
        &["remote", "add", "origin", &bare.to_string_lossy()],
    );
    run_git(&working, &["push", "-q", "origin", "main"]);

    bare
}

/// Convenience: build a `file://` URL string from a path.
pub fn file_url(path: &Path) -> String {
    format!("file://{}", path.display())
}
