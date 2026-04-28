//! Shared test helpers for tau-cli integration tests.
//!
//! Two families of fixtures live here:
//!
//! - [`temp_project`] / [`temp_project_with_tau_toml`] / [`read_tau_toml`]:
//!   minimal cwd helpers that landed with sub-project 9 Task 10.
//! - [`install_fixture`] / [`setup_project_with_installed_agent`] /
//!   [`setup_project`]: hand-author a project `tau.toml` plus a matching
//!   `.tau/` lockfile + on-disk package tree, mirroring the unit-test
//!   `install_fixture` from `crates/tau-cli/src/config/agent.rs`. These
//!   keep the `tau run` / `tau chat` integration suites hermetic — no
//!   `git`, no network — and let every test exercise the binary
//!   end-to-end through the compiled-in mock LLM backend (gated by
//!   `--features test-mock`).
//! - [`run_git`] / [`file_url`] / [`setup_local_package_fixture`]: the
//!   bare-repo + working-repo `file://` git fixture used by `tau install`
//!   and `tau list` integration tests. Honours the `init.defaultBranch`
//!   override (`refs/heads/main`) and tau-pkg's
//!   `protocol.file.allow=always` plumbing so the suite is portable across
//!   CI runners.
//!
//! All helpers are `#[allow(dead_code)]` so that no-features and partial
//! `--test <foo>` builds compile without warnings — different `cmd_*.rs`
//! files use different subsets.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use tempfile::TempDir;

// ---- minimal cwd helpers ----------------------------------------------------

/// Create a tempdir to use as the project cwd.
pub fn temp_project() -> TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// Create a tempdir with a tau.toml of the given contents.
pub fn temp_project_with_tau_toml(contents: &str) -> TempDir {
    let dir = temp_project();
    std::fs::write(dir.path().join("tau.toml"), contents).expect("write tau.toml");
    dir
}

/// Read the tau.toml from a tempdir; panic if missing.
pub fn read_tau_toml(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("tau.toml")).expect("read tau.toml")
}

// ---- hand-authored lockfile + project fixtures (run / chat) -----------------

/// Hand-author a lockfile + on-disk package tree under `<root>/.tau/`.
///
/// Uses raw TOML I/O because `LockedPackage` / `LockedVersion` are
/// `#[non_exhaustive]` (E0639). Schema is stable per Task 6.
///
/// Each call appends one `[[package]]` entry to `<root>/tau-lock.toml`,
/// upserting if the lockfile already exists, so a single project can
/// stack a tool package + an LLM-backend package side-by-side.
pub fn install_fixture(root: &Path, name: &str, version: &str, kind: &str, source_url: &str) {
    let dot_tau = root.join(".tau");
    std::fs::create_dir_all(dot_tau.join("packages").join(name).join(version)).unwrap();

    // Manifest (package's tau.toml).
    let manifest = format!(
        r#"name = "{name}"
version = "{version}"
description = "fixture"
authors = ["tester <test@example.com>"]
source = "{source_url}"
kind = "{kind}"
dependencies = []
capabilities = []
"#
    );
    std::fs::write(
        dot_tau
            .join("packages")
            .join(name)
            .join(version)
            .join("tau.toml"),
        manifest,
    )
    .unwrap();

    // Append/upsert lockfile entry. `tau-pkg` reads `<root>/tau-lock.toml`
    // (project-scope lockfile lives at the scope root, not inside .tau/).
    let lockfile_path = root.join("tau-lock.toml");
    let existing = if lockfile_path.exists() {
        std::fs::read_to_string(&lockfile_path).unwrap()
    } else {
        String::new()
    };

    let now_rfc3339 = "2026-04-28T00:00:00Z";
    let resolved_commit = "0".repeat(40);
    let new_entry = format!(
        r#"
[[package]]
name = "{name}"
active_version = "{version}"
source = "{source_url}"

[[package.versions]]
version = "{version}"
resolved_commit = "{resolved_commit}"
sha256 = ""
installed_at = "{now_rfc3339}"
"#
    );

    let new_lockfile = if existing.is_empty() {
        format!(
            r#"schema_version = 1
generated_by_tau_version = "0.0.0"
generated_at = "{now_rfc3339}"
{new_entry}"#
        )
    } else {
        format!("{existing}\n{new_entry}")
    };
    std::fs::write(&lockfile_path, new_lockfile).unwrap();
}

/// Stand up a project-scope tempdir with a `tau.toml` declaring a
/// single agent and the matching package + LLM backend pre-installed
/// in the project's `.tau/` lockfile.
///
/// The `[agents.<id>]` table is pre-populated with the names the
/// test-mock backend expects.
pub fn setup_project_with_installed_agent(
    agent_id: &str,
    pkg_name: &str,
    pkg_version: &str,
    llm_backend: &str,
) -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    install_fixture(
        root,
        pkg_name,
        pkg_version,
        "tool",
        "https://example.com/pkg.git",
    );
    install_fixture(
        root,
        llm_backend,
        "0.1.0",
        "llm-backend",
        "https://example.com/llm.git",
    );

    let project_toml = format!(
        r#"[project]
name = "demo"

[agents.{agent_id}]
display_name = "Test Agent"
package      = "{pkg_name}@^0.1"
llm_backend  = "{llm_backend}"
"#
    );
    std::fs::write(root.join("tau.toml"), project_toml).unwrap();

    dir
}

/// Convenience wrapper: `setup_project_with_installed_agent("reviewer",
/// "code-reviewer", "0.1.0", "mock-llm")`.
///
/// Used by `cmd_chat.rs` and the cross-cutting suites for the canonical
/// "happy-path agent" fixture.
pub fn setup_project() -> TempDir {
    setup_project_with_installed_agent("reviewer", "code-reviewer", "0.1.0", "mock-llm")
}

// ---- bare-repo `file://` git fixtures (install / list) ----------------------

/// Run `git` with `args` in `cwd`, panicking with stderr/stdout on failure.
pub fn run_git(cwd: &Path, args: &[&str]) {
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
pub fn file_url(path: &Path) -> String {
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
/// URL so tau-pkg's source/manifest match check passes. The bare HEAD
/// is forced to `refs/heads/main` to defeat host `init.defaultBranch`
/// drift across CI runners.
pub fn setup_local_package_fixture(
    name: &str,
    version: &str,
) -> (tempfile::TempDir, String, PathBuf) {
    setup_local_package_fixture_with_kind(name, version, "tool")
}

/// Same as [`setup_local_package_fixture`] but with an explicit `kind`.
pub fn setup_local_package_fixture_with_kind(
    name: &str,
    version: &str,
    kind: &str,
) -> (tempfile::TempDir, String, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");

    // Bare repo (clone target).
    let bare = dir.path().join(format!("{name}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    run_git(&bare, &["init", "--bare", "-q"]);
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
