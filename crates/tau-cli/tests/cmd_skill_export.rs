//! Integration tests for `tau skill export` (Skills-5 D3 / Task 6).
//!
//! Fixture layout mirrors the Skills-4 `find_installed_skill` test in
//! `tau-pkg/src/skill_resolve.rs`: install path is
//! `<tmp>/.tau/packages/<name>/<version>/` (state_path / packages).
//!
//! Tests:
//! 1. `export_capability_less_skill_roundtrips`          — SKILL.md copied, no tau.toml, no warning.
//! 2. `export_capability_bearing_skill_warns_and_drops`  — capability dropped with stderr note.
//! 3. `export_strict_fails_when_capabilities_present`    — --strict hard-errors.
//! 4. `export_refuses_existing_output_without_force`     — existing output dir rejected.
//! 5. `export_multi_file_skill_copies_all_referenced_files` — subdirectory files copied.

use assert_cmd::Command;
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Lockfile TOML template (schema_version = 6)
// ---------------------------------------------------------------------------

fn lockfile_toml(name: &str, version: &str, source: &str) -> String {
    format!(
        "schema_version = 6\n\
         generated_by_tau_version = \"0.0.0\"\n\
         generated_at = \"2026-05-14T00:00:00Z\"\n\n\
         [[package]]\n\
         name = \"{name}\"\n\
         active_version = \"{version}\"\n\
         source = \"{source}\"\n\
         \n\
         [package.skill]\n\
         content_sha256 = \"deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef\"\n\
         [package.skill.frontmatter]\n\
         name = \"{name}\"\n\
         description = \"Test skill.\"\n\
         \n\
         [[package.versions]]\n\
         version = \"{version}\"\n\
         resolved_commit = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n\
         sha256 = \"\"\n\
         installed_at = \"2026-05-14T00:00:00Z\"\n"
    )
}

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Create a minimal scope under `tmp` with the given skill package.
///
/// - `<tmp>/.tau/` — scope marker (state_path)
/// - `<tmp>/tau-lock.toml` — lockfile with one skill entry
/// - `<tmp>/.tau/packages/<name>/<version>/tau.toml` — package manifest
/// - `<tmp>/.tau/packages/<name>/<version>/SKILL.md` — skill content
///
/// Set `with_capabilities` to `true` to inject an `fs.read` capability.
/// When `false`, the manifest uses `capabilities = []` (empty inline array).
/// TOML does not allow both `capabilities = []` and `[[capabilities]]` in
/// the same document, so the two forms are mutually exclusive.
///
/// Returns the `TempDir` (stays alive for the test's duration).
fn make_scope_with_skill(name: &str, version: &str, with_capabilities: bool) -> TempDir {
    let tmp = TempDir::new().unwrap();

    // Scope marker.
    std::fs::create_dir_all(tmp.path().join(".tau")).unwrap();

    // Lockfile at project root.
    std::fs::write(
        tmp.path().join("tau-lock.toml"),
        lockfile_toml(name, version, "https://example.com/critic.git"),
    )
    .unwrap();

    // Package directory (state_path / packages / name / version).
    let pkg_dir = tmp
        .path()
        .join(".tau")
        .join("packages")
        .join(name)
        .join(version);
    std::fs::create_dir_all(&pkg_dir).unwrap();

    // tau.toml — capabilities block differs based on `with_capabilities`.
    let cap_section = if with_capabilities {
        "\n[[capabilities]]\nkind = \"fs.read\"\npaths = [\"/workspace/**\"]\n".to_string()
    } else {
        "capabilities = []\n".to_string()
    };
    let tau_toml = format!(
        "name = \"{name}\"\n\
         version = \"{version}\"\n\
         description = \"Test skill.\"\n\
         authors = []\n\
         source = \"https://example.com/critic.git\"\n\
         kind = \"skill\"\n\
         dependencies = []\n\
         {cap_section}\n\
         [skill]\n"
    );
    std::fs::write(pkg_dir.join("tau.toml"), tau_toml).unwrap();

    // SKILL.md
    std::fs::write(
        pkg_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Test skill.\n---\nDo the thing.\n"),
    )
    .unwrap();

    tmp
}

/// Run `tau skill export <name> --output <out> [extra_args...]` from `scope_root`.
fn run_export(
    scope_root: &Path,
    name: &str,
    out: &Path,
    extra_args: &[&str],
) -> std::process::Output {
    let mut args = vec!["skill", "export", name, "--output", out.to_str().unwrap()];
    args.extend_from_slice(extra_args);
    Command::cargo_bin("tau")
        .unwrap()
        .args(&args)
        .current_dir(scope_root)
        .output()
        .unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A capability-less skill exports cleanly: SKILL.md is copied, tau.toml is
/// omitted, stdout says "Exported", no "dropped" note on stderr.
#[test]
fn export_capability_less_skill_roundtrips() {
    let scope = make_scope_with_skill("critic", "0.1.0", false);
    let out_base = TempDir::new().unwrap();
    let out_path = out_base.path().join("exported");

    let result = run_export(scope.path(), "critic", &out_path, &[]);

    assert!(
        result.status.success(),
        "expected success; stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    // SKILL.md must be present.
    assert!(
        out_path.join("SKILL.md").exists(),
        "SKILL.md should be in output dir"
    );
    // tau.toml must NOT be present.
    assert!(
        !out_path.join("tau.toml").exists(),
        "tau.toml must not appear in Anthropic-format export"
    );

    // No drop warning when no capabilities.
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        !stderr.contains("dropped"),
        "expected no drop note when no capabilities; stderr: {stderr}"
    );

    // Success message on stdout.
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("Exported"),
        "expected 'Exported' in stdout; got: {stdout}"
    );
}

/// A capability-bearing skill exports with a drop warning on stderr.
/// The output dir still has SKILL.md; tau.toml is absent.
#[test]
fn export_capability_bearing_skill_warns_and_drops() {
    let scope = make_scope_with_skill("critic", "0.1.0", true);
    let out_base = TempDir::new().unwrap();
    let out_path = out_base.path().join("exported-caps");

    let result = run_export(scope.path(), "critic", &out_path, &[]);

    // Should succeed (no --strict).
    assert!(
        result.status.success(),
        "expected success without --strict; stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    // SKILL.md present, tau.toml absent.
    assert!(
        out_path.join("SKILL.md").exists(),
        "SKILL.md should be in output dir"
    );
    assert!(
        !out_path.join("tau.toml").exists(),
        "tau.toml must not appear in output"
    );

    // stderr must mention "dropped" and "fs.read".
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("dropped"),
        "expected drop note on stderr; got: {stderr}"
    );
    assert!(
        stderr.contains("fs.read"),
        "expected 'fs.read' in drop note; got: {stderr}"
    );
}

/// With --strict, a capability-bearing skill export must fail with a non-zero
/// exit and a message mentioning "would drop metadata".
#[test]
fn export_strict_fails_when_capabilities_present() {
    let scope = make_scope_with_skill("critic", "0.1.0", true);
    let out_base = TempDir::new().unwrap();
    let out_path = out_base.path().join("exported-strict");

    let result = run_export(scope.path(), "critic", &out_path, &["--strict"]);

    assert!(
        !result.status.success(),
        "expected failure with --strict when capabilities present"
    );

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.to_lowercase().contains("would drop metadata")
            || stderr.to_lowercase().contains("drop"),
        "expected 'would drop metadata' or 'drop' in stderr; got: {stderr}"
    );
}

/// Without --force, exporting to an existing directory must fail with a
/// message mentioning "already exists".
#[test]
fn export_refuses_existing_output_without_force() {
    let scope = make_scope_with_skill("critic", "0.1.0", false);
    let out_base = TempDir::new().unwrap();
    let out_path = out_base.path().join("pre-existing");

    // Pre-create the target directory to trigger the conflict.
    std::fs::create_dir_all(&out_path).unwrap();
    std::fs::write(out_path.join("placeholder.txt"), "x").unwrap();

    let result = run_export(scope.path(), "critic", &out_path, &[]);

    assert!(
        !result.status.success(),
        "expected failure when output dir already exists"
    );

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("already exists"),
        "expected 'already exists' in stderr; got: {stderr}"
    );

    // With --force, the same export should succeed.
    let result_force = run_export(scope.path(), "critic", &out_path, &["--force"]);
    assert!(
        result_force.status.success(),
        "--force should succeed over existing dir; stderr: {}",
        String::from_utf8_lossy(&result_force.stderr)
    );
    // tau.toml still absent after forced overwrite.
    assert!(
        !out_path.join("tau.toml").exists(),
        "tau.toml must not appear in output even after --force"
    );
}

/// A multi-file skill with SKILL.md + refs/style-guide.md + refs/examples.md.
/// All three content files must appear in the output; tau.toml must not.
#[test]
fn export_multi_file_skill_copies_all_referenced_files() {
    let scope = make_scope_with_skill("critic", "0.1.0", false);

    // Add subdirectory files alongside SKILL.md.
    let pkg_dir = scope
        .path()
        .join(".tau")
        .join("packages")
        .join("critic")
        .join("0.1.0");
    let refs_dir = pkg_dir.join("refs");
    std::fs::create_dir_all(&refs_dir).unwrap();
    std::fs::write(refs_dir.join("style-guide.md"), "# Style Guide\n").unwrap();
    std::fs::write(refs_dir.join("examples.md"), "# Examples\n").unwrap();

    let out_base = TempDir::new().unwrap();
    let out_path = out_base.path().join("multi-file");

    let result = run_export(scope.path(), "critic", &out_path, &[]);

    assert!(
        result.status.success(),
        "expected success; stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    // All three content files present.
    assert!(out_path.join("SKILL.md").exists(), "SKILL.md missing");
    assert!(
        out_path.join("refs").join("style-guide.md").exists(),
        "refs/style-guide.md missing"
    );
    assert!(
        out_path.join("refs").join("examples.md").exists(),
        "refs/examples.md missing"
    );

    // tau.toml absent.
    assert!(
        !out_path.join("tau.toml").exists(),
        "tau.toml must not appear in output"
    );
}
