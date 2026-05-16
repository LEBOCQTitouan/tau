//! Integration tests for `tau skill import` (Skills-5).

use assert_cmd::Command;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Create a minimal Anthropic-format skill directory in a tempdir.
/// The directory contains only SKILL.md with valid YAML frontmatter.
fn make_anthropic_source() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: critic\ndescription: Reviews drafts.\n---\nReview the draft.\n",
    )
    .unwrap();
    dir
}

/// Create a tau-native skill directory in a tempdir.
/// The directory contains both tau.toml and SKILL.md.
fn make_tau_source() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("tau.toml"),
        r#"name = "critic"
version = "0.1.0"
description = "Reviews drafts."
authors = []
source = "https://example.com/critic.git"
kind = "skill"
dependencies = []
capabilities = []

[skill]
"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: critic\ndescription: Reviews drafts.\n---\nReview.\n",
    )
    .unwrap();
    dir
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Happy path: import an Anthropic-format source → output dir has both
/// SKILL.md (copied from source) and a synthesized tau.toml.
#[test]
fn import_anthropic_source_writes_tau_toml() {
    let source = make_anthropic_source();
    let out_dir = tempfile::tempdir().unwrap();
    let out_path = out_dir.path().join("my-critic");
    // Ensure the target path does not exist (it is a sub-directory of out_dir).
    let _ = std::fs::remove_dir_all(&out_path);

    let result = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "skill",
            "import",
            source.path().to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        out_path.join("tau.toml").exists(),
        "tau.toml should exist in output dir"
    );
    assert!(
        out_path.join("SKILL.md").exists(),
        "SKILL.md should be copied to output dir"
    );

    let toml_text = std::fs::read_to_string(out_path.join("tau.toml")).unwrap();
    assert!(
        toml_text.contains("name = \"critic\""),
        "tau.toml should contain name"
    );
    assert!(
        toml_text.contains("version = \"0.1.0\""),
        "tau.toml should contain version"
    );
    assert!(
        toml_text.contains("kind = \"skill\""),
        "tau.toml should contain kind"
    );
}

/// Refusing to overwrite: without --force, an existing output dir is an error.
#[test]
fn import_refuses_existing_output_without_force() {
    let source = make_anthropic_source();
    let out_dir = tempfile::tempdir().unwrap();
    let out_path = out_dir.path().join("existing");
    // Pre-create the output directory to trigger the conflict.
    std::fs::create_dir(&out_path).unwrap();
    std::fs::write(out_path.join("placeholder"), "x").unwrap();

    let result = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "skill",
            "import",
            source.path().to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        !result.status.success(),
        "should fail when output exists without --force"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("already exists"),
        "error message should mention 'already exists', got: {stderr}"
    );
}

/// Refusing tau-format source: if the source has a tau.toml, import should
/// reject it and hint the user toward `tau install`.
#[test]
fn import_refuses_tau_format_source() {
    let source = make_tau_source();
    let out_dir = tempfile::tempdir().unwrap();
    let out_path = out_dir.path().join("would-fail");

    let result = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "skill",
            "import",
            source.path().to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        !result.status.success(),
        "should fail when source is already a tau-format skill"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    // Error message should mention tau.toml or tau install.
    assert!(
        stderr.contains("tau.toml") || stderr.contains("tau install"),
        "error message should mention tau.toml or tau install, got: {stderr}"
    );
}

/// Content validation: the synthesized tau.toml must parse as TOML and
/// contain the expected field values (name, version, kind, empty arrays).
#[test]
fn import_synthesized_tau_toml_matches_expected_content() {
    let source = make_anthropic_source();
    let out_dir = tempfile::tempdir().unwrap();
    let out_path = out_dir.path().join("assert-content");

    let result = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "skill",
            "import",
            source.path().to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );

    let toml_text = std::fs::read_to_string(out_path.join("tau.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&toml_text).unwrap_or_else(|e| {
        panic!("synthesized tau.toml is not valid TOML: {e}\ncontent:\n{toml_text}")
    });

    assert_eq!(
        parsed["name"].as_str().unwrap(),
        "critic",
        "name field should match SKILL.md frontmatter"
    );
    assert_eq!(
        parsed["version"].as_str().unwrap(),
        "0.1.0",
        "synthesized version should be 0.1.0"
    );
    assert_eq!(
        parsed["kind"].as_str().unwrap(),
        "skill",
        "kind should be 'skill'"
    );
    assert!(
        parsed["capabilities"].as_array().unwrap().is_empty(),
        "capabilities should be empty for imported Anthropic skill"
    );
    assert!(
        parsed["dependencies"].as_array().unwrap().is_empty(),
        "dependencies should be empty for imported Anthropic skill"
    );
    assert!(
        parsed.get("skill").and_then(|v| v.as_table()).is_some(),
        "synthesized tau.toml should have a [skill] table"
    );
}
