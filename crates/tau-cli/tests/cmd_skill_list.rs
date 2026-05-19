//! Integration tests for `tau skill list`.
//!
//! Synthesizes a v5 lockfile directly in a tempdir scope (no `tau install`
//! invocation). Uses a project scope: create `<tmp>/.tau/` so that
//! `Scope::resolve(&cwd)` finds it when `current_dir` is `<tmp>`.
//!
//! Test list (5 test fns, 6 cases, 2 insta snapshots):
//! 1. `list_three_skills[human|json]`         — parametrized format-parity pilot:
//!    same 3-skill fixture; `human` snapshots stdout, `json` asserts via
//!    JSON parse.
//! 2. `list_human_empty_state`                — "no skills installed." + hint.
//! 3. `list_lockfile_schema_too_new_errors`   — schema_version=99 → SchemaTooNew (exit 2).
//! 4. `list_lockfile_malformed_toml_errors`   — garbage TOML → parse error (exit 2).
//! 5. `list_plugin_only_lockfile_shows_empty` — plugin packages filtered out → empty state.

use assert_cmd::Command;
use rstest::rstest;
use serde_json::Value;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a v5 LockFile TOML string with the supplied packages.
///
/// Each entry in `skills` is `(name, version, description)`.
///
/// NOTE: `[package.skill]` MUST appear before `[[package.versions]]` because
/// TOML does not allow adding keys to a table after an array-of-tables
/// sub-element is opened.
fn v5_lockfile_toml(skills: &[(&str, &str, &str)]) -> String {
    let mut s = String::from(
        "schema_version = 6\n\
         generated_by_tau_version = \"0.0.0\"\n\
         generated_at = \"2026-05-12T10:00:00Z\"\n\n",
    );
    for (name, version, description) in skills {
        s.push_str(&format!(
            "[[package]]\n\
             name = \"{name}\"\n\
             active_version = \"{version}\"\n\
             source = \"https://example.com/{name}.git\"\n\
             \n\
             [package.skill]\n\
             content_sha256 = \"{sha}\"\n\
             [package.skill.frontmatter]\n\
             name = \"{name}\"\n\
             description = \"{description}\"\n\
             \n\
             [[package.versions]]\n\
             version = \"{version}\"\n\
             resolved_commit = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n\
             sha256 = \"\"\n\
             installed_at = \"2026-05-12T10:00:00Z\"\n\
             \n",
            name = name,
            version = version,
            description = description,
            sha = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        ));
    }
    s
}

/// Create a tempdir project scope with `tau-lock.toml` for the given skills.
///
/// Layout:
/// ```
/// <tmp>/
///   .tau/          ← Scope::resolve finds this when current_dir = <tmp>
///   tau-lock.toml  ← lockfile (at scope.path() level, not state_path)
/// ```
///
/// Returns the `TempDir` (kept alive for the duration of the test).
fn make_scope_with_skills(skills: &[(&str, &str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    // Create the .tau/ directory so Scope::resolve finds a project scope.
    std::fs::create_dir_all(dir.path().join(".tau")).unwrap();
    // Write lockfile at the project root (scope.lockfile_path() = <root>/tau-lock.toml).
    let lockfile_toml = v5_lockfile_toml(skills);
    std::fs::write(dir.path().join("tau-lock.toml"), lockfile_toml).unwrap();
    dir
}

/// Run `tau skill list [extra_args...]` with `current_dir` set to the scope root.
fn run_skill_list(scope_root: &std::path::Path, extra_args: &[&str]) -> std::process::Output {
    let mut args = vec!["skill", "list"];
    args.extend_from_slice(extra_args);
    Command::cargo_bin("tau")
        .unwrap()
        .args(&args)
        .current_dir(scope_root)
        .env("TAU_TESTING_ALLOW_MOCK_SANDBOX", "1")
        .output()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Format-parity pilot (rstest): same 3-skill fixture, two output formats.
///
/// - `human` case: snapshots stdout (column-aligned table, 3 alphabetical rows).
/// - `json`  case: asserts via JSON parse (length + alphabetical ordering).
///
/// This is the **only** parametrized pair in `cmd_skill_list`; the empty-state
/// test below remains standalone because no JSON counterpart exists.
#[rstest]
#[case::human(&[], "list_human_three_skills")]
#[case::json(&["--json"], "")]
fn list_three_skills(#[case] extra_args: &[&str], #[case] snapshot_name: &str) {
    let skills = [
        ("fact-check", "0.2.0", "Verifies factual claims."),
        ("proofread", "1.0.0", "Proofreads prose for grammar."),
        ("critic", "0.1.0", "Reviews drafts for quality."),
    ];
    let dir = make_scope_with_skills(&skills);
    let output = run_skill_list(dir.path(), extra_args);

    assert!(
        output.status.success(),
        "expected success; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();

    if snapshot_name.is_empty() {
        // JSON case: parse-and-assert (no snapshot).
        let parsed: Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
        let skill_arr = parsed["skills"].as_array().expect("skills is an array");
        assert_eq!(skill_arr.len(), 3, "expected 3 skills in JSON output");
        // Items are alphabetical: critic < fact-check < proofread.
        assert_eq!(
            parsed["skills"][0]["name"],
            Value::String("critic".into()),
            "first skill should be 'critic' (alphabetical)"
        );
        assert_eq!(
            parsed["skills"][1]["name"],
            Value::String("fact-check".into()),
            "second skill should be 'fact-check' (alphabetical)"
        );
        assert_eq!(
            parsed["skills"][2]["name"],
            Value::String("proofread".into()),
            "third skill should be 'proofread' (alphabetical)"
        );
    } else {
        // Human case: snapshot.
        insta::assert_snapshot!(snapshot_name, stdout);
    }
}

/// Empty lockfile — no skill packages installed.
/// Expects "no skills installed." message + hint.
#[test]
fn list_human_empty_state() {
    let dir = make_scope_with_skills(&[]);
    let output = run_skill_list(dir.path(), &[]);

    assert!(
        output.status.success(),
        "expected success; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    insta::assert_snapshot!("list_human_empty_state", stdout);
}

/// `--json` for the empty-skill case. Pins the JSON contract:
/// `{"skills": []}` (empty array, NOT null, NOT a missing key) so
/// downstream tooling that iterates `.skills` keeps working when
/// no skills are installed.
#[test]
fn list_json_empty_state() {
    let dir = make_scope_with_skills(&[]);
    let output = run_skill_list(dir.path(), &["--json"]);

    assert!(
        output.status.success(),
        "expected success; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: Value = serde_json::from_str(stdout.trim()).expect("valid JSON");
    let skill_arr = parsed["skills"]
        .as_array()
        .expect("`skills` must be an array (not null or missing) even when empty");
    assert!(
        skill_arr.is_empty(),
        "expected `skills: []`; got {} entries",
        skill_arr.len()
    );
}

// ---------------------------------------------------------------------------
// Negative-path tests
// ---------------------------------------------------------------------------

/// Lockfile with `schema_version = 99` (future / unknown schema).
/// Expect exit 2 with `error: loading lockfile` on stderr. The underlying
/// `RegistryError::SchemaTooNew` is hidden behind the anyhow `Context` wrap
/// in cmd/skill/list.rs unless `--debug` is set. Pin the user-visible shape:
/// the command MUST NOT panic and MUST surface a clear top-line error.
#[test]
fn list_lockfile_schema_too_new_errors() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".tau")).unwrap();
    // Inline setup — no helper for "bogus schema version" since this is the
    // only test that needs it.
    std::fs::write(
        dir.path().join("tau-lock.toml"),
        "schema_version = 99\n\
         generated_by_tau_version = \"0.0.0\"\n\
         generated_at = \"2026-05-12T10:00:00Z\"\n",
    )
    .unwrap();

    let output = run_skill_list(dir.path(), &[]);
    let exit_code = output.status.code().unwrap_or(-1);
    assert_eq!(exit_code, 2, "expected exit code 2; got {exit_code}");

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("loading lockfile"),
        "expected 'loading lockfile' in stderr; got: {stderr}"
    );
}

/// Lockfile present but its bytes are not valid TOML.
/// Expect exit 2 with `error: loading lockfile` on stderr.
/// Pins: no panic; user-visible top-line error mentions the lockfile load step.
#[test]
fn list_lockfile_malformed_toml_errors() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".tau")).unwrap();
    // Inline setup — garbage TOML bytes. No helper for this scenario.
    std::fs::write(
        dir.path().join("tau-lock.toml"),
        "this is not valid toml = = = [[[\n",
    )
    .unwrap();

    let output = run_skill_list(dir.path(), &[]);
    let exit_code = output.status.code().unwrap_or(-1);
    assert_eq!(exit_code, 2, "expected exit code 2; got {exit_code}");

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("loading lockfile"),
        "expected 'loading lockfile' in stderr; got: {stderr}"
    );
}

/// Lockfile contains a package entry that is NOT a skill (no `[package.skill]`
/// table — i.e. a plugin or other package kind). `tau skill list` MUST filter
/// these out and report the empty state, not surface them as skills.
#[test]
fn list_plugin_only_lockfile_shows_empty() {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".tau")).unwrap();
    // Inline setup — a plugin-only entry (no [package.skill] table). The
    // v5_lockfile_toml helper always emits skill entries, so we write this
    // by hand.
    let toml = "schema_version = 6\n\
                generated_by_tau_version = \"0.0.0\"\n\
                generated_at = \"2026-05-12T10:00:00Z\"\n\n\
                [[package]]\n\
                name = \"some-plugin\"\n\
                active_version = \"1.0.0\"\n\
                source = \"https://example.com/plugin.git\"\n\
                \n\
                [[package.versions]]\n\
                version = \"1.0.0\"\n\
                resolved_commit = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n\
                sha256 = \"\"\n\
                installed_at = \"2026-05-12T10:00:00Z\"\n";
    std::fs::write(dir.path().join("tau-lock.toml"), toml).unwrap();

    let output = run_skill_list(dir.path(), &[]);
    assert!(
        output.status.success(),
        "expected success; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("no skills installed."),
        "expected 'no skills installed.' in stdout; got: {stdout}"
    );
    assert!(
        !stdout.contains("some-plugin"),
        "plugin package leaked into skill list output: {stdout}"
    );
}
