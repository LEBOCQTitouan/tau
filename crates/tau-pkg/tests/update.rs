//! Integration tests for `tau_pkg::update_package`.
//!
//! Tests use `file://` git fixtures with multiple tags (v1.0.0, v1.1.0, etc.)
//! to exercise the update lifecycle without network access.
//!
//! Placed here (integration tests) rather than in `src/update.rs` because
//! the tests need git process execution and multi-tagged bare repos — the
//! same rationale that moved Task 2's tree_hash / sha256 tests to
//! `tests/install_lifecycle.rs` rather than unit tests.

mod fixtures;

use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use semver::Version;
use tempfile::TempDir;

use tau_domain::{PackageName, PackageSource};
use tau_pkg::{install, list, update::update_package, Scope};

// ── Fixture helpers ─────────────────────────────────────────────────────────

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

/// Create a bare git repo containing **multiple versions** of a package,
/// each tagged with a semver tag (`v1.0.0`, `v1.1.0`, etc.).
///
/// Returns (bare_repo_path, working_dir_path).
/// The bare repo is suitable as a `file://` source for `install` /
/// `update_package`.
fn make_multi_version_repo(parent: &Path, name: &str, versions: &[&str]) -> std::path::PathBuf {
    let bare = parent.join(format!("{name}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    run_git(&bare, &["init", "--bare", "-q"]);
    run_git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let working = parent.join(format!("{name}-working"));
    std::fs::create_dir_all(&working).unwrap();
    run_git(&working, &["init", "-q", "-b", "main"]);
    run_git(&working, &["config", "user.email", "test@example.com"]);
    run_git(&working, &["config", "user.name", "Test User"]);
    run_git(
        &working,
        &["remote", "add", "origin", &bare.to_string_lossy()],
    );

    let source_url = fixtures::file_url(&bare);

    for &version in versions {
        // Each tagged commit's tau.toml declares a source URL that includes
        // the version tag as the rev pin. This matches what `install` passes
        // when cloning `file://...#v<version>`, satisfying the source/manifest
        // match check in `install_with_options`.
        let versioned_source_url = format!("{source_url}#v{version}");
        let manifest = format!(
            r#"name = "{name}"
version = "{version}"
description = "Multi-version fixture for update tests"
authors = ["Test <test@example.com>"]
source = "{versioned_source_url}"
kind = "tool"
dependencies = []
capabilities = []
"#
        );
        std::fs::write(working.join("tau.toml"), manifest).unwrap();
        run_git(&working, &["add", "tau.toml"]);
        run_git(
            &working,
            &["commit", "-q", "-m", &format!("bump to {version}")],
        );
        run_git(&working, &["tag", &format!("v{version}")]);
    }

    // Push all commits + tags.
    run_git(&working, &["push", "-q", "origin", "main"]);
    run_git(&working, &["push", "-q", "--tags", "origin"]);

    bare
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// Install v1.0.0 from a repo that also has v1.1.0 tagged.
/// Call `update_package(name, None, scope, false)`.
/// Assert `to_version == 1.1.0` and that v1.0.0 is still in `installed_versions`
/// (cohabitation, no prune).
#[test]
fn update_package_to_latest_tag() {
    if !fixtures::git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("tau-home");
    std::fs::create_dir_all(&project_root).unwrap();
    let scope = Scope::new_project(&project_root).unwrap();

    // Bare repo with two tags.
    let bare = make_multi_version_repo(tmp.path(), "update-tool", &["1.0.0", "1.1.0"]);
    let bare_url = fixtures::file_url(&bare);

    // Install v1.0.0 explicitly (pin rev to v1.0.0 tag).
    let source_v1 = PackageSource::from_str(&format!("{bare_url}#v1.0.0")).unwrap();
    install(&source_v1, &scope).unwrap();

    // Confirm v1.0.0 is active.
    let pkgs = list(&scope).unwrap();
    assert_eq!(pkgs[0].active_version.to_string(), "1.0.0");

    // Now update without a pin — should pick 1.1.0.
    // update_package internally strips the rev for source listing so we
    // don't need to mutate the lockfile source.
    let name: PackageName = "update-tool".parse().unwrap();

    let result = update_package(&name, None, &scope, false).unwrap();

    assert_eq!(result.from_version, Version::parse("1.0.0").unwrap());
    assert_eq!(result.to_version, Version::parse("1.1.0").unwrap());

    // Old version must still be in installed_versions (no prune).
    let pkgs = list(&scope).unwrap();
    let pkg = pkgs.iter().find(|p| p.name == name).unwrap();
    assert_eq!(pkg.active_version.to_string(), "1.1.0");
    assert!(
        pkg.installed_versions
            .iter()
            .any(|v| v.version.to_string() == "1.0.0"),
        "v1.0.0 should still be present in installed_versions when prune=false"
    );
    assert!(
        pkg.installed_versions
            .iter()
            .any(|v| v.version.to_string() == "1.1.0"),
        "v1.1.0 should be in installed_versions after update"
    );
}

/// Install v1.0.0; call `update_package(name, Some(1.2.0), scope, false)`.
/// Assert `to_version == 1.2.0`.
#[test]
fn update_package_to_specific_version() {
    if !fixtures::git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("tau-home");
    std::fs::create_dir_all(&project_root).unwrap();
    let scope = Scope::new_project(&project_root).unwrap();

    let bare = make_multi_version_repo(tmp.path(), "pinned-tool", &["1.0.0", "1.1.0", "1.2.0"]);
    let bare_url = fixtures::file_url(&bare);

    // Install v1.0.0 with an explicit rev pin.
    let source_v1 = PackageSource::from_str(&format!("{bare_url}#v1.0.0")).unwrap();
    install(&source_v1, &scope).unwrap();

    let name: PackageName = "pinned-tool".parse().unwrap();

    // update_package strips the rev internally for source listing.
    let result =
        update_package(&name, Some(Version::parse("1.2.0").unwrap()), &scope, false).unwrap();

    assert_eq!(result.from_version, Version::parse("1.0.0").unwrap());
    assert_eq!(result.to_version, Version::parse("1.2.0").unwrap());

    let pkgs = list(&scope).unwrap();
    let pkg = pkgs.iter().find(|p| p.name == name).unwrap();
    assert_eq!(pkg.active_version.to_string(), "1.2.0");
}

/// Install v1.0.0; call `update_package(name, None, scope, true)`.
/// Assert old version directory is gone and v1.0.0 is no longer in the lockfile.
#[test]
fn update_package_with_prune_removes_old() {
    if !fixtures::git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("tau-home");
    std::fs::create_dir_all(&project_root).unwrap();
    let scope = Scope::new_project(&project_root).unwrap();

    let bare = make_multi_version_repo(tmp.path(), "prune-tool", &["1.0.0", "1.1.0"]);
    let bare_url = fixtures::file_url(&bare);

    let source_v1 = PackageSource::from_str(&format!("{bare_url}#v1.0.0")).unwrap();
    let installed_v1 = install(&source_v1, &scope).unwrap();
    assert!(
        installed_v1.installed_path.is_dir(),
        "v1.0.0 dir must exist"
    );

    let name: PackageName = "prune-tool".parse().unwrap();
    let result = update_package(&name, None, &scope, true).unwrap();

    assert_eq!(result.from_version, Version::parse("1.0.0").unwrap());
    assert_eq!(result.to_version, Version::parse("1.1.0").unwrap());

    // Old version directory must be gone.
    assert!(
        !installed_v1.installed_path.exists(),
        "v1.0.0 directory should have been pruned"
    );

    // v1.0.0 must not be in installed_versions.
    let pkgs = list(&scope).unwrap();
    let pkg = pkgs.iter().find(|p| p.name == name).unwrap();
    assert!(
        !pkg.installed_versions
            .iter()
            .any(|v| v.version.to_string() == "1.0.0"),
        "v1.0.0 should be absent from installed_versions after prune"
    );
    assert_eq!(pkg.active_version.to_string(), "1.1.0");
}

/// Install v1.0.0; call `update_package(name, Some(9.9.9), scope, false)`.
/// Assert that `UpdateError::Resolve { .. }` is returned.
#[test]
fn update_package_unreachable_version_fails() {
    if !fixtures::git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("tau-home");
    std::fs::create_dir_all(&project_root).unwrap();
    let scope = Scope::new_project(&project_root).unwrap();

    let bare = make_multi_version_repo(tmp.path(), "ghost-tool", &["1.0.0", "1.1.0"]);
    let bare_url = fixtures::file_url(&bare);

    let source_v1 = PackageSource::from_str(&format!("{bare_url}#v1.0.0")).unwrap();
    install(&source_v1, &scope).unwrap();

    let name: PackageName = "ghost-tool".parse().unwrap();
    let err =
        update_package(&name, Some(Version::parse("9.9.9").unwrap()), &scope, false).unwrap_err();

    assert!(
        matches!(err, tau_pkg::update::UpdateError::Resolve { .. }),
        "expected UpdateError::Resolve, got: {err}"
    );
}
