//! Integration tests for `tau skill show`.
//!
//! Synthesizes a v5 lockfile + package directory directly in a tempdir
//! scope (no `tau install` invocation). Uses a project scope: create
//! `<tmp>/.tau/` so that `Scope::resolve(&cwd)` finds it when
//! `current_dir` is `<tmp>`.
//!
//! Test list (5 tests, 3 insta snapshots):
//! 1. `show_human_no_body`                   — human output; no --body flag.
//! 2. `show_human_with_body_raw`             — human output; --body --raw.
//! 3. `show_json_no_body`                    — --json; assert via JSON parse.
//! 4. `show_unknown_name_with_suggestion_exits_2` — typo → exit 2 + suggestion.
//! 5. `show_install_path_missing_errors_clearly`  — tau.toml deleted → hint.

use assert_cmd::Command;
use serde_json::Value;
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Minimal `tau.toml` for a skill package named `critic` v0.1.0 with one
/// `fs.read` capability on `${SKILL_DIR}/references/**`.
const CRITIC_TAU_TOML: &str = r#"name = "critic"
version = "0.1.0"
description = "Reviews drafts for quality."
authors = []
source = "https://example.com/critic.git"
kind = "skill"
dependencies = []

[[capabilities]]
kind = "fs.read"
paths = ["${SKILL_DIR}/references/**"]

[skill]
"#;

/// SKILL.md content with YAML frontmatter + body.
const CRITIC_SKILL_MD: &str = "---\nname: critic\ndescription: Reviews drafts for quality.\n---\n# Critic Skill\n\nYou are a writing critic. Review the provided draft and give feedback.\n";

/// v5 lockfile TOML string for the critic skill.
///
/// NOTE: `[package.skill]` MUST appear before `[[package.versions]]` because
/// TOML does not allow adding keys to a table after an array-of-tables
/// sub-element is opened.
fn critic_lockfile_toml() -> String {
    "schema_version = 5\n\
     generated_by_tau_version = \"0.0.0\"\n\
     generated_at = \"2026-05-12T10:00:00Z\"\n\n\
     [[package]]\n\
     name = \"critic\"\n\
     active_version = \"0.1.0\"\n\
     source = \"https://example.com/critic.git\"\n\
     \n\
     [package.skill]\n\
     content_sha256 = \"deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef\"\n\
     [package.skill.frontmatter]\n\
     name = \"critic\"\n\
     description = \"Reviews drafts for quality.\"\n\
     \n\
     [[package.versions]]\n\
     version = \"0.1.0\"\n\
     resolved_commit = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n\
     sha256 = \"\"\n\
     installed_at = \"2026-05-12T10:00:00Z\"\n"
        .to_string()
}

/// Build a tempdir project scope with:
/// - `<tmp>/.tau/`                         ← project scope marker
/// - `<tmp>/tau-lock.toml`                 ← lockfile (critic v0.1.0)
/// - `<tmp>/packages/critic/0.1.0/tau.toml`
/// - `<tmp>/packages/critic/0.1.0/SKILL.md` (if `with_skill_md` is true)
///
/// `Scope::resolve` finds the project scope when `current_dir = <tmp>`.
/// `scope.path()` = `<tmp>`, so:
///   - `scope.lockfile_path()` = `<tmp>/tau-lock.toml`
///   - install path (via `scope.path().join("packages")`) = `<tmp>/packages/critic/0.1.0`
///
/// Returns the `TempDir` (kept alive for the duration of the test).
fn make_critic_scope(with_skill_md: bool) -> TempDir {
    let dir = TempDir::new().unwrap();

    // Create the .tau/ directory so Scope::resolve finds a project scope.
    std::fs::create_dir_all(dir.path().join(".tau")).unwrap();

    // Write lockfile at the project root.
    std::fs::write(dir.path().join("tau-lock.toml"), critic_lockfile_toml()).unwrap();

    // Create package dir under <scope_root>/packages/... (NOT .tau/packages/...).
    let pkg_dir = dir.path().join("packages").join("critic").join("0.1.0");
    std::fs::create_dir_all(&pkg_dir).unwrap();

    // Write tau.toml.
    std::fs::write(pkg_dir.join("tau.toml"), CRITIC_TAU_TOML).unwrap();

    // Optionally write SKILL.md.
    if with_skill_md {
        std::fs::write(pkg_dir.join("SKILL.md"), CRITIC_SKILL_MD).unwrap();
    }

    dir
}

/// Run `tau skill show <name> [extra_args...]` with `current_dir` at the scope root.
fn run_skill_show(scope_root: &Path, name: &str, extra_args: &[&str]) -> std::process::Output {
    let mut args = vec!["skill", "show", name];
    args.extend_from_slice(extra_args);
    Command::cargo_bin("tau")
        .unwrap()
        .args(&args)
        .current_dir(scope_root)
        .env("TAU_TESTING_ALLOW_MOCK_SANDBOX", "1")
        .output()
        .unwrap()
}

/// Replace dynamic scope root path occurrences in output with `[SCOPE]`
/// so snapshots are stable across runs.
///
/// On macOS, `tempfile::TempDir` paths go through `/var/folders/...` but
/// the binary may resolve symlinks and output `/private/var/folders/...`.
/// We replace the canonical (longer) form first to avoid partial replacement.
///
/// On Windows, `Path::display()` emits backslashes (`C:\Users\...`). After
/// replacing the scope root, we also normalize remaining backslashes in the
/// install_path line so the snapshot matches the forward-slash form used
/// on Unix-style platforms. This is safe because the only paths in the
/// output that follow the scope root are tau-controlled (`packages/<name>/<ver>`).
fn normalize_paths(s: &str, scope_root: &Path) -> String {
    let p = scope_root.to_str().unwrap();
    // Replace canonical form first if it differs (macOS /var → /private/var).
    let canonical = scope_root.canonicalize().ok();
    let mut result = if let Some(canon) = canonical {
        let c = canon.to_str().unwrap();
        if c != p {
            s.replace(c, "[SCOPE]").replace(p, "[SCOPE]")
        } else {
            s.replace(p, "[SCOPE]")
        }
    } else {
        s.replace(p, "[SCOPE]")
    };
    // Also normalize the Windows path-separator form that may appear
    // immediately after our [SCOPE] sentinel (e.g. "[SCOPE]\\packages\\...").
    if std::path::MAIN_SEPARATOR == '\\' {
        result = result.replace("[SCOPE]\\", "[SCOPE]/");
        result = result.replace("\\packages\\", "/packages/");
        // Also normalize any other tau-controlled directories that may
        // appear in show output (defensive — keep generic in case install
        // path layouts grow). The `/` form is the snapshot canonical.
        result = result
            .replace("\\.tau\\", "/.tau/")
            .replace("\\references\\", "/references/")
            .replace("\\templates\\", "/templates/")
            .replace("\\prompts\\", "/prompts/");
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// critic fixture (install path with tau.toml). Snapshot human output.
/// Sections: name+version header, description, source, install path,
/// capabilities (fs.read on `${SKILL_DIR}/references/**`).
#[test]
fn show_human_no_body() {
    let dir = make_critic_scope(false);
    let output = run_skill_show(dir.path(), "critic", &[]);

    assert!(
        output.status.success(),
        "expected success; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let normalized = normalize_paths(&stdout, dir.path());
    insta::assert_snapshot!("show_human_no_body", normalized);
}

/// critic fixture with SKILL.md. Use `--body --raw`. Snapshot human output
/// including the raw markdown body (frontmatter stripped).
#[test]
fn show_human_with_body_raw() {
    let dir = make_critic_scope(true);
    let output = run_skill_show(dir.path(), "critic", &["--body", "--raw"]);

    assert!(
        output.status.success(),
        "expected success; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let normalized = normalize_paths(&stdout, dir.path());
    insta::assert_snapshot!("show_human_with_body_raw", normalized);
}

/// critic fixture. `--json`. Assert via JSON parse:
/// - `parsed["name"] == "critic"`
/// - capabilities contains an `fs.read` entry
/// - `parsed["body"]` is `null` or absent
#[test]
fn show_json_no_body() {
    let dir = make_critic_scope(false);
    let output = run_skill_show(dir.path(), "critic", &["--json"]);

    assert!(
        output.status.success(),
        "expected success; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(stdout.trim()).expect("valid JSON");

    assert_eq!(parsed["name"], Value::String("critic".into()));
    assert_eq!(parsed["version"], Value::String("0.1.0".into()));

    let caps = parsed["capabilities"]
        .as_array()
        .expect("capabilities is array");
    let has_fs_read = caps
        .iter()
        .any(|c| c["kind"] == Value::String("fs.read".into()));
    assert!(has_fs_read, "expected fs.read capability; got {caps:?}");

    // body should be absent (null or missing) when --body is not passed.
    let body = &parsed["body"];
    assert!(
        body.is_null(),
        "expected body to be null when --body not passed; got {body:?}"
    );
}

/// critic fixture, but show "kritic" (typo).
/// Expect exit code 2, stderr contains "skill not found: kritic",
/// stderr contains "did you mean: critic?".
#[test]
fn show_unknown_name_with_suggestion_exits_2() {
    let dir = make_critic_scope(false);
    let output = run_skill_show(dir.path(), "kritic", &[]);

    let exit_code = output.status.code().unwrap_or(-1);
    assert_eq!(exit_code, 2, "expected exit code 2; got {exit_code}");

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("skill not found: kritic"),
        "expected 'skill not found: kritic' in stderr; got: {stderr}"
    );
    assert!(
        stderr.contains("did you mean: critic?"),
        "expected 'did you mean: critic?' in stderr; got: {stderr}"
    );
}

/// critic fixture but with `tau.toml` deleted post-setup.
/// Expect non-zero exit, stderr contains "re-run `tau install`".
#[test]
fn show_install_path_missing_errors_clearly() {
    let dir = make_critic_scope(false);

    // Delete the tau.toml to simulate a partially-removed package.
    let toml_path = dir
        .path()
        .join("packages")
        .join("critic")
        .join("0.1.0")
        .join("tau.toml");
    std::fs::remove_file(&toml_path).unwrap();

    let output = run_skill_show(dir.path(), "critic", &[]);

    assert!(
        !output.status.success(),
        "expected non-zero exit when tau.toml is missing"
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("re-run `tau install`"),
        "expected remediation hint 're-run `tau install`' in stderr; got: {stderr}"
    );
}
