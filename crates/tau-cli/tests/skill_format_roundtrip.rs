//! End-to-end roundtrip tests for Skills-5 format handling (Anthropic ↔ tau).
//!
//! These tests exercise the full CLI pipeline:
//! 1. `tau install` auto-detects Anthropic-format → `tau skill export` →
//!    byte-identical SKILL.md + extra files.
//! 2. `tau skill import` + `tau install` (tau-format now) vs direct
//!    `tau install` (Anthropic auto-detect) — same name+version, different
//!    `synthesized_from`.
//!
//! Both tests use `file://` git fixtures to avoid network requirements.
//! They guard with `git_available()` and skip cleanly on headless CI
//! runners without git.

mod common;

use assert_cmd::Command;
use std::path::Path;
use std::process::Command as StdCommand;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a `file://` URL from a local path.
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

/// Run git with `args` in `dir`; panic on failure.
fn run_git(dir: &Path, args: &[&str]) {
    let out = StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("git {:?} spawn: {e}", args));
    assert!(
        out.status.success(),
        "git {:?} in {:?} failed:\n{}",
        args,
        dir,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Return `true` if `git` is on PATH.
fn git_available() -> bool {
    StdCommand::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Create a bare git repo under `parent/<name>.git` seeded with a minimal
/// Anthropic-format SKILL.md (+ optional README.md).
///
/// Returns (bare_path, url_string).
fn make_anthropic_git_fixture(
    parent: &Path,
    name: &str,
    with_readme: bool,
) -> (std::path::PathBuf, String) {
    let bare = parent.join(format!("{name}.git"));
    std::fs::create_dir_all(&bare).unwrap();
    run_git(&bare, &["init", "--bare", "-q"]);
    run_git(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let work = parent.join(format!("{name}-work"));
    std::fs::create_dir_all(&work).unwrap();
    run_git(&work, &["init", "-q", "-b", "main"]);
    run_git(&work, &["config", "user.email", "test@example.com"]);
    run_git(&work, &["config", "user.name", "Test User"]);

    std::fs::write(
        work.join("SKILL.md"),
        format!(
            "---\nname: {name}\ndescription: Reviews drafts.\n---\nReview the draft carefully.\n"
        ),
    )
    .unwrap();

    if with_readme {
        std::fs::write(
            work.join("README.md"),
            format!("# Anthropic skill: {name}\n"),
        )
        .unwrap();
    }

    // Stage all files in the working directory.
    run_git(&work, &["add", "."]);
    run_git(&work, &["commit", "-q", "-m", "initial"]);
    let bare_url = file_url(&bare);
    run_git(&work, &["remote", "add", "origin", &bare.to_string_lossy()]);
    run_git(&work, &["push", "-q", "origin", "main"]);

    (bare, bare_url)
}

/// Set up a project scope in `project_root`:
/// - `<project_root>/.tau/` directory (scope marker)
fn make_project_scope(project_root: &Path) {
    std::fs::create_dir_all(project_root.join(".tau")).unwrap();
}

/// Run `tau install <url>` from `project_root` (project scope).
fn tau_install_project(project_root: &Path, url: &str) {
    let out = Command::cargo_bin("tau")
        .unwrap()
        .args(["install", url])
        .current_dir(project_root)
        .env("TAU_TESTING_ALLOW_MOCK_SANDBOX", "1")
        .output()
        .expect("tau install ran");

    assert!(
        out.status.success(),
        "tau install {url} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

// ---------------------------------------------------------------------------
// Test 1: install Anthropic source → export → byte-identical SKILL.md
// ---------------------------------------------------------------------------

/// Install an Anthropic-format git source via auto-detect, then export it;
/// the exported SKILL.md must be byte-identical to the original source
/// SKILL.md. tau.toml must not appear in the export output.
///
/// Gap closed (Skills-5 T7 follow-up): `install_with_options` now writes
/// the synthesized `tau.toml` to the install directory during Anthropic-format
/// auto-detect install (install.rs fix). `find_installed_skill` can now read
/// the manifest from disk as expected by `tau skill show` and `tau skill export`.
#[test]
fn roundtrip_anthropic_source_through_tau() {
    if !git_available() {
        eprintln!("skipping roundtrip_anthropic_source_through_tau: `git` not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();

    // --- Build the Anthropic git fixture ---
    let (bare, url) = make_anthropic_git_fixture(tmp.path(), "critic", true);

    // Clone a local working copy to capture the raw source files for comparison.
    let source_check = tmp.path().join("source-check");
    std::fs::create_dir_all(&source_check).unwrap();
    run_git(&source_check, &["clone", &url, "."]);

    let original_skill_md = std::fs::read(source_check.join("SKILL.md")).unwrap();
    let original_readme = std::fs::read(source_check.join("README.md")).unwrap();

    // --- Set up project scope and install ---
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    make_project_scope(&project);

    tau_install_project(&project, &url);

    // --- Export the installed skill ---
    let export_out = tmp.path().join("exported");

    let export_result = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "skill",
            "export",
            "critic",
            "--output",
            export_out.to_str().unwrap(),
        ])
        .current_dir(&project)
        .env("TAU_TESTING_ALLOW_MOCK_SANDBOX", "1")
        .output()
        .expect("tau skill export ran");

    assert!(
        export_result.status.success(),
        "tau skill export failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&export_result.stdout),
        String::from_utf8_lossy(&export_result.stderr),
    );

    // SKILL.md must be byte-identical.
    let exported_skill_md =
        std::fs::read(export_out.join("SKILL.md")).expect("SKILL.md should be in export output");
    assert_eq!(
        original_skill_md, exported_skill_md,
        "SKILL.md must be byte-identical after roundtrip"
    );

    // README.md must be byte-identical.
    let exported_readme =
        std::fs::read(export_out.join("README.md")).expect("README.md should be in export output");
    assert_eq!(
        original_readme, exported_readme,
        "README.md must be byte-identical after roundtrip"
    );

    // tau.toml must NOT appear in the export.
    assert!(
        !export_out.join("tau.toml").exists(),
        "tau.toml must not appear in the Anthropic-format export"
    );

    // Suppress "unused variable" warning for `bare` (retained for lifetime).
    drop(bare);
}

// ---------------------------------------------------------------------------
// Test 2: import → install matches direct install (modulo synthesized_from)
// ---------------------------------------------------------------------------

/// `tau skill import <src> --output dir` then `tau install dir` (tau-format)
/// produces a scope where the lockfile records `synthesized_from = None` (or
/// absent).
///
/// A direct `tau install <src>` (Anthropic auto-detect) produces a scope
/// where the lockfile records `synthesized_from = "anthropic"`.
///
/// Both lockfiles must agree on `name` and `version`.
///
/// Gap closed (Skills-5 T7 follow-up): `tau skill import` now routes
/// `file://` URLs through `git clone` (not local fs copy), so bare repos
/// are cloned to a working tree with SKILL.md at the top level. Combined
/// with the synthesized `tau.toml` write fix in install.rs, the full
/// import-then-install roundtrip now works end-to-end.
#[test]
fn import_then_install_matches_direct_install() {
    if !git_available() {
        eprintln!("skipping import_then_install_matches_direct_install: `git` not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();

    // --- Build the Anthropic git fixture ---
    let (_bare, url) = make_anthropic_git_fixture(tmp.path(), "critic", false);

    // --- Path A: direct install (Anthropic auto-detect) ---
    let project_a = tmp.path().join("project-a");
    std::fs::create_dir_all(&project_a).unwrap();
    make_project_scope(&project_a);
    tau_install_project(&project_a, &url);

    // Read the lockfile from project_a (lockfile is at project root, not .tau/).
    let lf_a_text = std::fs::read_to_string(project_a.join("tau-lock.toml"))
        .expect("project-a lockfile should exist");

    // --- Path B: import then install (tau-format) ---
    // Step B1: tau skill import <url> --output <import-dir>
    // NOTE: For file:// URLs the import command copies the local directory.
    // A bare git repo directory doesn't have SKILL.md at the top level, so
    // this will fail with "not a skill package". This is the documented gap.
    let import_dir = tmp.path().join("imported-critic");
    let import_result = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "skill",
            "import",
            &url,
            "--output",
            import_dir.to_str().unwrap(),
        ])
        .output()
        .expect("tau skill import ran");

    assert!(
        import_result.status.success(),
        "tau skill import failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&import_result.stdout),
        String::from_utf8_lossy(&import_result.stderr),
    );

    // import_dir must have tau.toml now.
    assert!(
        import_dir.join("tau.toml").exists(),
        "tau.toml must be synthesized by tau skill import"
    );

    // Step B2: We need to install from a git source. Create a git repo from
    // the imported directory so tau-pkg can clone it.
    let import_bare = tmp.path().join("imported-critic.git");
    std::fs::create_dir_all(&import_bare).unwrap();
    run_git(&import_bare, &["init", "--bare", "-q"]);
    run_git(&import_bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);

    let import_bare_url = file_url(&import_bare);

    // Update the source field in tau.toml to match the new local URL.
    let tau_toml_text = std::fs::read_to_string(import_dir.join("tau.toml")).unwrap();
    let updated_toml = tau_toml_text
        .lines()
        .map(|line| {
            if line.starts_with("source =") {
                format!("source = {:?}", import_bare_url)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(import_dir.join("tau.toml"), &updated_toml).unwrap();

    // `tau skill import` (via git clone) already initialised import_dir as a
    // git repo with remote "origin" pointing to the original critic.git bare.
    // Re-point origin to the new import_bare and push the updated tree.
    run_git(&import_dir, &["config", "user.email", "test@example.com"]);
    run_git(&import_dir, &["config", "user.name", "Test User"]);
    run_git(
        &import_dir,
        &[
            "remote",
            "set-url",
            "origin",
            &import_bare.to_string_lossy(),
        ],
    );
    run_git(&import_dir, &["add", "."]);
    run_git(&import_dir, &["commit", "-q", "-m", "imported tau skill"]);
    run_git(&import_dir, &["push", "-q", "origin", "main"]);

    // Step B3: install from the tau-format git source.
    let project_b = tmp.path().join("project-b");
    std::fs::create_dir_all(&project_b).unwrap();
    make_project_scope(&project_b);
    tau_install_project(&project_b, &import_bare_url);

    // Read the lockfile from project_b.
    let lf_b_text = std::fs::read_to_string(project_b.join("tau-lock.toml"))
        .expect("project-b lockfile should exist");

    // --- Assert: both lockfiles record name = "critic", version = "0.1.0" ---
    assert!(
        lf_a_text.contains("name = \"critic\""),
        "project-a lockfile should record name = critic; got:\n{lf_a_text}"
    );
    assert!(
        lf_b_text.contains("name = \"critic\""),
        "project-b lockfile should record name = critic; got:\n{lf_b_text}"
    );

    assert!(
        lf_a_text.contains("0.1.0"),
        "project-a lockfile should record version 0.1.0; got:\n{lf_a_text}"
    );
    assert!(
        lf_b_text.contains("0.1.0"),
        "project-b lockfile should record version 0.1.0; got:\n{lf_b_text}"
    );

    // --- Assert: synthesized_from differs ---
    // project-a: direct Anthropic install → synthesized_from = "anthropic"
    assert!(
        lf_a_text.contains("synthesized_from = \"anthropic\""),
        "project-a (direct Anthropic install) should have synthesized_from = \"anthropic\";\n\
         got:\n{lf_a_text}"
    );
    // project-b: imported tau.toml → synthesized_from absent or not "anthropic"
    assert!(
        !lf_b_text.contains("synthesized_from = \"anthropic\""),
        "project-b (import-then-install) should NOT have synthesized_from = \"anthropic\";\n\
         got:\n{lf_b_text}"
    );
}
