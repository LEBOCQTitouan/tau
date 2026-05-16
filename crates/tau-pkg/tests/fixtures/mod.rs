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

/// Run a git command in `cwd`, panicking on failure. Public so that
/// integration tests that need to build custom fixtures can reuse it.
pub fn run_git_in(cwd: &Path, args: &[&str]) {
    run_git(cwd, args);
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
    // Set the bare repo's HEAD to `main` so `git clone` checks out the
    // right branch regardless of the host's `init.defaultBranch` setting.
    // On CI runners that haven't configured `init.defaultBranch`, `git
    // init --bare` defaults to `master`, causing `git clone` to check out
    // a non-existent branch and produce an empty working tree.
    run_git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
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

    let source_url = file_url(&bare);
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
///
/// On Windows, `Path::display()` uses backslashes, which are not valid in
/// URLs (TOML parses them as escape sequences). This function converts to
/// forward slashes before embedding in the URL.
pub fn file_url(path: &Path) -> String {
    // Convert the path to a string with forward slashes for URL compatibility.
    // On POSIX systems the separator is already `/`; on Windows we replace `\`.
    let forward_slashed = path
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    // On Windows, absolute paths look like `C:/path/...`. A file URL for an
    // absolute Windows path needs three slashes: `file:///C:/path/...`.
    // On POSIX, the path already starts with `/`, so `file:///path` also works
    // and is the canonical form (host=empty, path=/...).
    if forward_slashed.starts_with('/') {
        format!("file://{forward_slashed}")
    } else {
        format!("file:///{forward_slashed}")
    }
}
