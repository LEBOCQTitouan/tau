//! Table-driven manifest validation tests for `read_manifest`.
//!
//! Each case writes a synthetic `tau.toml` to a tempdir and verifies
//! the expected error variant (or success).

use tau_pkg::{read_manifest, ManifestReadError};
use tau_ports::fixtures::scratch_dir;

fn write_and_read(toml_body: &str) -> Result<tau_domain::PackageManifest, ManifestReadError> {
    let tmp = scratch_dir("manifest-validation");
    let path = tmp.path().join("tau.toml");
    std::fs::write(&path, toml_body).unwrap();
    read_manifest(&path)
}

#[test]
fn accepts_minimal_valid_manifest() {
    let body = r#"
        name = "acme-tool"
        version = "1.0.0"
        description = "Test"
        authors = []
        source = "https://example.com/x.git"
        kind = "tool"
        dependencies = []
        capabilities = []
    "#;
    let manifest = write_and_read(body).expect("should accept minimal valid manifest");
    assert_eq!(manifest.name().as_str(), "acme-tool");
    assert_eq!(manifest.version().to_string(), "1.0.0");
}

#[test]
fn accepts_manifest_with_rev_in_source() {
    let body = r#"
        name = "acme-tool"
        version = "1.2.3"
        description = "Tool with rev"
        authors = []
        source = "https://example.com/x.git#main"
        kind = "tool"
        dependencies = []
        capabilities = []
    "#;
    let manifest = write_and_read(body).expect("should accept source with #rev");
    let src = manifest.source().to_string();
    assert!(src.ends_with("#main"), "expected #main in source: {src}");
}

#[test]
fn rejects_empty_source_string() {
    let body = r#"
        name = "acme-tool"
        version = "1.0.0"
        description = ""
        authors = []
        source = ""
        kind = "tool"
        dependencies = []
        capabilities = []
    "#;
    let err = write_and_read(body).unwrap_err();
    assert!(matches!(
        err,
        ManifestReadError::Parse { .. } | ManifestReadError::Validation(_)
    ));
}

#[test]
fn rejects_empty_kind_string() {
    let body = r#"
        name = "acme-tool"
        version = "1.0.0"
        description = ""
        authors = []
        source = "https://example.com/x.git"
        kind = ""
        dependencies = []
        capabilities = []
    "#;
    let err = write_and_read(body).unwrap_err();
    assert!(matches!(
        err,
        ManifestReadError::Parse { .. } | ManifestReadError::Validation(_)
    ));
}

#[test]
fn rejects_invalid_package_name() {
    let body = r#"
        name = "INVALID NAME"
        version = "1.0.0"
        description = ""
        authors = []
        source = "https://example.com/x.git"
        kind = "tool"
        dependencies = []
        capabilities = []
    "#;
    let err = write_and_read(body).unwrap_err();
    assert!(matches!(
        err,
        ManifestReadError::Parse { .. } | ManifestReadError::Validation(_)
    ));
}
