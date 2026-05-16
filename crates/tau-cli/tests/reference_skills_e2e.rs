//! End-to-end integration tests for Skills-6 reference skills.
//!
//! Drives the `tau` binary via `assert_cmd` against a tempdir scope.
//! Each test installs one or more reference skills from
//! `<workspace>/skills/<name>/` then exercises `tau skill list/show/export`.
//!
//! These tests are the public-facing user-story validation for the
//! Skills track. The proof that a contributor can: clone, build, install
//! reference skills, render them, and export them back to Anthropic.

use assert_cmd::Command;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set in cargo test runs");
    Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolvable")
        .to_path_buf()
}

fn in_tree_skill_path(name: &str) -> PathBuf {
    workspace_root().join("skills").join(name)
}

/// Set up a tempdir as a tau project scope (`.tau/config.toml` present).
fn setup_scope(scope_root: &Path) {
    std::fs::create_dir_all(scope_root.join(".tau")).unwrap();
    std::fs::write(
        scope_root.join(".tau").join("config.toml"),
        "schema_version = 3\n\n[sandbox]\nrequired_tier = \"none\"\n",
    )
    .unwrap();
}

/// Invoke `tau install <file-url-of-in-tree-skill>` in `scope_root`.
fn run_tau_install(scope_root: &Path, skill: &str) -> std::process::Output {
    let skill_path = in_tree_skill_path(skill);
    let url = format!("file://{}", skill_path.display());
    Command::cargo_bin("tau")
        .unwrap()
        .args(["install", &url])
        .current_dir(scope_root)
        .env("TAU_TESTING_ALLOW_MOCK_SANDBOX", "1")
        .output()
        .unwrap()
}

/// Invoke `tau skill <subcommand>` in `scope_root`.
fn run_tau_skill(scope_root: &Path, args: &[&str]) -> std::process::Output {
    let mut full_args = vec!["skill"];
    full_args.extend_from_slice(args);
    Command::cargo_bin("tau")
        .unwrap()
        .args(&full_args)
        .current_dir(scope_root)
        .env("TAU_TESTING_ALLOW_MOCK_SANDBOX", "1")
        .output()
        .unwrap()
}

// ----------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------

#[test]
fn tau_skill_list_shows_three_installed_references() {
    let tmp = TempDir::new().unwrap();
    setup_scope(tmp.path());

    for skill in ["critic", "fact-checker", "pr-reviewer"] {
        let out = run_tau_install(tmp.path(), skill);
        assert!(
            out.status.success(),
            "{} install failed:\nstdout: {}\nstderr: {}",
            skill,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }

    let out = run_tau_skill(tmp.path(), &["list"]);
    assert!(
        out.status.success(),
        "skill list failed:\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("critic"), "critic missing from list output");
    assert!(stdout.contains("fact-checker"), "fact-checker missing");
    assert!(stdout.contains("pr-reviewer"), "pr-reviewer missing");
}

#[test]
fn tau_skill_show_critic_renders_anthropic_compatible() {
    let tmp = TempDir::new().unwrap();
    setup_scope(tmp.path());

    let install = run_tau_install(tmp.path(), "critic");
    assert!(
        install.status.success(),
        "install failed:\nstderr: {}",
        String::from_utf8_lossy(&install.stderr)
    );

    let out = run_tau_skill(tmp.path(), &["show", "critic", "--json"]);
    assert!(
        out.status.success(),
        "skill show failed:\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("show --json is valid JSON");

    assert_eq!(parsed["name"], serde_json::Value::String("critic".into()));
    assert_eq!(parsed["version"], serde_json::Value::String("0.1.0".into()));
    // synthesized_from should be null for in-tree (real) tau.toml.
    assert!(
        parsed["synthesized_from"].is_null(),
        "expected synthesized_from=null for in-tree critic; got {:?}",
        parsed["synthesized_from"]
    );
}

#[test]
fn tau_skill_export_critic_is_byte_identical() {
    let tmp = TempDir::new().unwrap();
    setup_scope(tmp.path());

    let install = run_tau_install(tmp.path(), "critic");
    assert!(install.status.success());

    let out_dir = tmp.path().join("critic-exported");
    let out = run_tau_skill(
        tmp.path(),
        &["export", "critic", "--output", out_dir.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "skill export failed:\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Compare byte-identically (no LF/CRLF mangling — .gitattributes forces LF).
    let in_tree = std::fs::read(in_tree_skill_path("critic").join("SKILL.md")).unwrap();
    let exported = std::fs::read(out_dir.join("SKILL.md")).unwrap();
    assert_eq!(in_tree, exported, "SKILL.md not byte-identical after export");

    // tau.toml must NOT be in the exported directory.
    assert!(
        !out_dir.join("tau.toml").exists(),
        "tau.toml leaked into Anthropic export"
    );
}

#[test]
fn tau_skill_export_fact_checker_drops_capabilities_warns() {
    let tmp = TempDir::new().unwrap();
    setup_scope(tmp.path());

    let install = run_tau_install(tmp.path(), "fact-checker");
    assert!(install.status.success());

    let out_dir = tmp.path().join("fact-checker-exported");
    let out = run_tau_skill(
        tmp.path(),
        &[
            "export",
            "fact-checker",
            "--output",
            out_dir.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "expected success despite warning");

    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("dropped") || stderr.contains("fs.read"),
        "expected drop warning in stderr; got: {stderr}"
    );
}

#[test]
fn tau_skill_export_fact_checker_preserves_references() {
    let tmp = TempDir::new().unwrap();
    setup_scope(tmp.path());

    let install = run_tau_install(tmp.path(), "fact-checker");
    assert!(install.status.success());

    let out_dir = tmp.path().join("fact-checker-exported");
    let out = run_tau_skill(
        tmp.path(),
        &[
            "export",
            "fact-checker",
            "--output",
            out_dir.to_str().unwrap(),
        ],
    );
    assert!(out.status.success());

    // references/ subdir should survive the export.
    assert!(
        out_dir.join("references").join("style-guide.md").exists(),
        "style-guide.md missing from export"
    );
    assert!(
        out_dir.join("references").join("common-claims.md").exists(),
        "common-claims.md missing from export"
    );
    // tau.toml stripped.
    assert!(!out_dir.join("tau.toml").exists(), "tau.toml leaked");
}
