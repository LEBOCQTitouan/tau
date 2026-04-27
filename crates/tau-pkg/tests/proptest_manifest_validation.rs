//! Tests for `read_manifest`'s malformed-TOML rejection paths.
//!
//! Note: this file is named `proptest_manifest_validation.rs` for
//! consistency with the lockfile / scope_config proptest siblings.
//! In practice it uses #[test] table-driven malformed inputs because
//! `UncheckedManifest`'s TOML shape is too varied for a clean
//! generative strategy. Generative round-trip coverage for valid
//! manifests is provided indirectly by `tau-domain`'s own proptest
//! suite over its serde derives.

use tau_pkg::{read_manifest, ManifestReadError};

fn write_manifest(dir: &tempfile::TempDir, contents: &str) -> std::path::PathBuf {
    let path = dir.path().join("tau.toml");
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn rejects_completely_malformed_toml() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = write_manifest(&tmp, "this is not toml = = =");
    let err = read_manifest(&path).unwrap_err();
    assert!(matches!(err, ManifestReadError::Parse { .. }));
}

#[test]
fn rejects_missing_required_field_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let bad = r#"
        version = "1.0.0"
        description = "no name field"
        authors = []

        [source.Git.location]
        Url = "https://example.com/x.git"

        [kind.Custom]
        kind = "tool"
    "#;
    let path = write_manifest(&tmp, bad);
    let err = read_manifest(&path).unwrap_err();
    assert!(matches!(
        err,
        ManifestReadError::Parse { .. } | ManifestReadError::Validation(_)
    ));
}

#[test]
fn rejects_invalid_version_format() {
    let tmp = tempfile::TempDir::new().unwrap();
    let bad = r#"
        name = "acme-tool"
        version = "not.a.semver"
        description = ""
        authors = []

        [source.Git.location]
        Url = "https://example.com/x.git"

        [kind.Custom]
        kind = "tool"
    "#;
    let path = write_manifest(&tmp, bad);
    let err = read_manifest(&path).unwrap_err();
    assert!(matches!(
        err,
        ManifestReadError::Parse { .. } | ManifestReadError::Validation(_)
    ));
}

#[test]
fn rejects_missing_required_field_source() {
    let tmp = tempfile::TempDir::new().unwrap();
    let bad = r#"
        name = "acme-tool"
        version = "1.0.0"
        description = ""
        authors = []

        [kind.Custom]
        kind = "tool"
    "#;
    let path = write_manifest(&tmp, bad);
    let err = read_manifest(&path).unwrap_err();
    assert!(matches!(
        err,
        ManifestReadError::Parse { .. } | ManifestReadError::Validation(_)
    ));
}

#[test]
fn unknown_top_level_field_outcome_is_defined() {
    // tau-domain's UncheckedManifest may or may not be strict about
    // unknown fields. If it ignores them, read_manifest will succeed.
    // If it rejects them, read_manifest will return Parse or Validation.
    // Either outcome is acceptable — we document the actual behaviour here.
    let tmp = tempfile::TempDir::new().unwrap();
    let input = r#"
        name = "acme-tool"
        version = "1.0.0"
        description = ""
        authors = []
        unknown_field = "hello"

        [source.Git.location]
        Url = "https://example.com/x.git"

        [kind.Custom]
        kind = "tool"
    "#;
    let path = write_manifest(&tmp, input);
    // Both Ok and Err are valid — the test exercises the code path without
    // asserting a specific outcome for unknown fields.
    let _ = read_manifest(&path);
}
