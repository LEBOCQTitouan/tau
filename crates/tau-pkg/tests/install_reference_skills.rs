//! Integration tests for Skills-6 reference skill packages.
//!
//! Each test installs an in-tree skill (`skills/<name>/`) via the tau-pkg
//! install pipeline and asserts on the resulting lockfile + install directory.
//!
//! **Strategy B (bare-repo fixture):** The install pipeline calls `git clone`
//! internally, so the source must be a git repository, not a plain directory.
//! Each test helper copies the in-tree skill content into a temporary working
//! tree, rewrites the `source` field in `tau.toml` to match the bare fixture's
//! `file://` URL (so the source/manifest match check in step 5 passes), commits
//! once, pushes to a local bare repo, and installs from the bare repo's
//! `file://` URL.  This mirrors the pattern in `install_anthropic_format.rs`.
//!
//! The in-tree content (`skills/<name>/`) IS the authoritative source;
//! the bare-repo fixture is merely a transport shim so the install pipeline
//! can `git clone` it.
//!
//! These tests skip cleanly when no `git` binary is on PATH.

mod fixtures;

use std::path::{Path, PathBuf};
use std::str::FromStr;

use tau_domain::PackageSource;
use tau_pkg::{install_with_options, InstallOptions, LockFile, Scope};
use tempfile::TempDir;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Locate the workspace root from `CARGO_MANIFEST_DIR`.
/// `CARGO_MANIFEST_DIR` points at `crates/tau-pkg`; workspace root is two dirs up.
fn workspace_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set in cargo test runs");
    Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolvable from CARGO_MANIFEST_DIR")
        .to_path_buf()
}

/// Path to the in-tree reference skill: `<workspace>/skills/<name>/`.
fn in_tree_skill_path(name: &str) -> PathBuf {
    workspace_root().join("skills").join(name)
}

/// Construct install options suitable for skill tests:
/// - `skip_build` — skills don't compile anything
/// - `skip_cross_check` — not exercising the cross-check path here
fn test_install_options() -> InstallOptions {
    let mut opts = InstallOptions::default();
    opts.skip_cross_check = true;
    opts.build.skip_build = true;
    opts
}

/// Create a `Scope` backed by a fresh project tempdir.
/// Uses `Scope::new_project` which creates `.tau/` + a default `config.toml`.
fn setup_scope(tmp: &Path) -> Scope {
    Scope::new_project(tmp).expect("Scope::new_project succeeds")
}

/// Recursively copy `src` directory tree into `dst` (both must exist or be
/// created by the caller beforehand for `dst`).
fn copy_dir_all(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_all(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

/// Build a temporary bare git repository that contains the files from the
/// in-tree skill at `skills/<name>/`, with the `tau.toml` `source` field
/// rewritten to point at the bare repo's `file://` URL.
///
/// Returns `(bare_path, bare_url)`.  The `bare_path` owns the bare repo;
/// callers must keep the returned `TempDir` alive for the duration of the test.
/// The `bare_url` is the `file://` URL suitable for `PackageSource::from_str`.
fn make_reference_skill_fixture(
    parent: &TempDir,
    name: &str,
) -> (PathBuf, String) {
    let skill_src = in_tree_skill_path(name);
    assert!(
        skill_src.is_dir(),
        "in-tree skill directory missing: {}",
        skill_src.display()
    );

    // 1. Create bare repo and working tree directories under `parent`.
    let bare = fixtures::make_bare_repo(parent.path(), name);
    let working = parent.path().join(format!("{name}-working"));
    std::fs::create_dir_all(&working).unwrap();

    // 2. Initialise working tree.
    fixtures::run_git_in(&working, &["init", "-q", "-b", "main"]);
    fixtures::run_git_in(&working, &["config", "user.email", "test@example.com"]);
    fixtures::run_git_in(&working, &["config", "user.name", "Test User"]);

    // 3. Copy in-tree skill files into the working tree.
    copy_dir_all(&skill_src, &working);

    // 4. Rewrite `source` in tau.toml to match the bare fixture URL so that
    //    the install pipeline's source/manifest match check (step 5) passes.
    let bare_url = fixtures::file_url(&bare);
    let toml_path = working.join("tau.toml");
    let original = std::fs::read_to_string(&toml_path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", toml_path.display()));
    let patched = replace_source_field(&original, &bare_url);
    std::fs::write(&toml_path, patched).unwrap();

    // 5. Commit and push to bare.
    fixtures::run_git_in(&working, &["add", "--all"]);
    fixtures::run_git_in(&working, &["commit", "-q", "-m", "reference skill fixture"]);
    fixtures::run_git_in(
        &working,
        &["remote", "add", "origin", &bare.to_string_lossy()],
    );
    fixtures::run_git_in(&working, &["push", "-q", "origin", "main"]);

    (bare, bare_url)
}

/// Replace the `source = "..."` line in a `tau.toml` string with `new_url`.
/// Panics if the line is not found (all in-tree skills must have a `source` field).
fn replace_source_field(toml_text: &str, new_url: &str) -> String {
    assert!(
        toml_text.lines().any(|l| l.starts_with("source = ")),
        "tau.toml has no `source = ...` line:\n{toml_text}"
    );

    let new_source_line = format!("source = \"{new_url}\"");
    let body = toml_text
        .lines()
        .map(|line| {
            if line.starts_with("source = ") {
                new_source_line.as_str()
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if toml_text.ends_with('\n') {
        body + "\n"
    } else {
        body
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Test 1: Install `skills/critic/` — capability-less skill; verify lockfile entry.
#[test]
fn install_critic_from_in_tree_path() {
    if !fixtures::git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let scope_dir = tmp.path().join("scope");
    std::fs::create_dir_all(&scope_dir).unwrap();
    let scope = setup_scope(&scope_dir);

    let (_bare, bare_url) = make_reference_skill_fixture(&tmp, "critic");
    let source = PackageSource::from_str(&bare_url).expect("valid file:// URL");

    let installed = install_with_options(&source, &scope, test_install_options())
        .expect("install_critic succeeds");

    assert_eq!(installed.name.as_str(), "critic");
    assert_eq!(installed.version.to_string(), "0.1.0");
    assert!(installed.installed_path.is_dir(), "installed_path is a directory");

    // tau.toml has a `[skill]` table → format is Tau-native → synthesized_from = None.
    let lf = LockFile::load(&scope.lockfile_path()).expect("lockfile loads");
    let pkg = lf
        .packages
        .iter()
        .find(|p| p.name.as_str() == "critic")
        .expect("critic in lockfile");

    assert_eq!(pkg.active_version.to_string(), "0.1.0");
    assert!(
        pkg.synthesized_from.is_none(),
        "in-tree skills have tau.toml; synthesized_from should be None"
    );

    // SKILL.md must be present in the install directory.
    assert!(
        installed.installed_path.join("SKILL.md").is_file(),
        "SKILL.md missing from install dir"
    );
    assert!(
        installed.installed_path.join("tau.toml").is_file(),
        "tau.toml missing from install dir"
    );
}

/// Test 2: Install `skills/fact-checker/` — has a `references/` subdirectory
/// with bundled files.  Verify the subdirectory is preserved and the
/// `fs.read` capability is present in the installed tau.toml.
#[test]
fn install_fact_checker_preserves_references_dir() {
    if !fixtures::git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let scope_dir = tmp.path().join("scope");
    std::fs::create_dir_all(&scope_dir).unwrap();
    let scope = setup_scope(&scope_dir);

    let (_bare, bare_url) = make_reference_skill_fixture(&tmp, "fact-checker");
    let source = PackageSource::from_str(&bare_url).expect("valid file:// URL");

    let installed = install_with_options(&source, &scope, test_install_options())
        .expect("install_fact_checker succeeds");

    assert_eq!(installed.name.as_str(), "fact-checker");
    assert_eq!(installed.version.to_string(), "0.1.0");

    // Bundled reference files must survive the install pipeline.
    let refs = installed.installed_path.join("references");
    assert!(refs.is_dir(), "references/ dir missing");
    assert!(
        refs.join("style-guide.md").is_file(),
        "references/style-guide.md missing"
    );
    assert!(
        refs.join("common-claims.md").is_file(),
        "references/common-claims.md missing"
    );

    // Re-parse the installed tau.toml and check capabilities are preserved.
    let toml_text =
        std::fs::read_to_string(installed.installed_path.join("tau.toml")).unwrap();
    assert!(
        toml_text.contains("fs.read"),
        "fs.read capability missing from installed tau.toml"
    );
    assert!(
        toml_text.contains("${SKILL_DIR}/references/**"),
        "${SKILL_DIR}/references/** path missing from installed tau.toml"
    );

    // Lockfile presence.
    let lf = LockFile::load(&scope.lockfile_path()).expect("lockfile loads");
    assert!(
        lf.packages.iter().any(|p| p.name.as_str() == "fact-checker"),
        "fact-checker missing from lockfile"
    );
}

/// Test 3: Install `skills/pr-reviewer/` — has `process.spawn` capabilities
/// for `git` and `rg`.  Verify those survive the install pipeline.
#[test]
fn install_pr_reviewer_records_process_spawn_cap() {
    if !fixtures::git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let scope_dir = tmp.path().join("scope");
    std::fs::create_dir_all(&scope_dir).unwrap();
    let scope = setup_scope(&scope_dir);

    let (_bare, bare_url) = make_reference_skill_fixture(&tmp, "pr-reviewer");
    let source = PackageSource::from_str(&bare_url).expect("valid file:// URL");

    let installed = install_with_options(&source, &scope, test_install_options())
        .expect("install_pr_reviewer succeeds");

    assert_eq!(installed.name.as_str(), "pr-reviewer");
    assert_eq!(installed.version.to_string(), "0.1.0");

    let toml_text =
        std::fs::read_to_string(installed.installed_path.join("tau.toml")).unwrap();
    assert!(
        toml_text.contains("process.spawn"),
        "process.spawn capability missing from installed tau.toml"
    );
    assert!(
        toml_text.contains("\"git\""),
        "git command missing from process.spawn capability"
    );
    assert!(
        toml_text.contains("\"rg\""),
        "rg command missing from process.spawn capability"
    );

    // Lockfile presence.
    let lf = LockFile::load(&scope.lockfile_path()).expect("lockfile loads");
    assert!(
        lf.packages.iter().any(|p| p.name.as_str() == "pr-reviewer"),
        "pr-reviewer missing from lockfile"
    );
}

/// Test 4 (key user-story test): Install all three reference skills into the
/// same scope and verify the lockfile has exactly three entries, one per skill,
/// all tau-native format (synthesized_from = None), at schema_version 6.
#[test]
fn install_all_three_yields_three_lockfile_entries() {
    if !fixtures::git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let scope_dir = tmp.path().join("scope");
    std::fs::create_dir_all(&scope_dir).unwrap();
    let scope = setup_scope(&scope_dir);

    // Each skill needs its own bare-repo fixture but we reuse the same tempdir
    // as parent (distinct subdirectory names per skill avoid collisions).
    let (_bare_c, url_c) = make_reference_skill_fixture(&tmp, "critic");
    let (_bare_f, url_f) = make_reference_skill_fixture(&tmp, "fact-checker");
    let (_bare_p, url_p) = make_reference_skill_fixture(&tmp, "pr-reviewer");

    let src_c = PackageSource::from_str(&url_c).unwrap();
    let src_f = PackageSource::from_str(&url_f).unwrap();
    let src_p = PackageSource::from_str(&url_p).unwrap();

    install_with_options(&src_c, &scope, test_install_options()).expect("critic install");
    install_with_options(&src_f, &scope, test_install_options()).expect("fact-checker install");
    install_with_options(&src_p, &scope, test_install_options()).expect("pr-reviewer install");

    let lf = LockFile::load(&scope.lockfile_path()).expect("lockfile loads");

    assert_eq!(
        lf.packages.len(),
        3,
        "expected 3 lockfile entries, got {}; entries: {:?}",
        lf.packages.len(),
        lf.packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>()
    );

    let names: Vec<&str> = lf.packages.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"critic"), "critic missing from lockfile");
    assert!(names.contains(&"fact-checker"), "fact-checker missing from lockfile");
    assert!(names.contains(&"pr-reviewer"), "pr-reviewer missing from lockfile");

    // All three are tau-native → synthesized_from must be None.
    for pkg in &lf.packages {
        assert!(
            pkg.synthesized_from.is_none(),
            "expected synthesized_from = None for in-tree skill '{}', got {:?}",
            pkg.name.as_str(),
            pkg.synthesized_from
        );
    }

    // Schema version matches Skills-5's constant.
    use tau_pkg::lockfile::MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION;
    assert_eq!(
        lf.schema_version,
        MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION,
        "lockfile schema_version mismatch"
    );
}
