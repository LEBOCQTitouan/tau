//! Integration tests for `UncheckedManifest` / `PackageManifest` round-trips.
//!
//! Two tests are provided:
//!
//! 1. `programmatic_manifest_round_trips_through_serde_json` — constructs a
//!    manifest programmatically (via `tau_domain::fixtures::any_unchecked_manifest()`
//!    because `UncheckedManifest` is `#[non_exhaustive]`) and exercises the
//!    JSON path: `validate → serde_json::to_string → serde_json::from_str → validate`.
//!
//! 2. `manifest_round_trips_through_toml` — parses the `SAMPLE` TOML constant
//!    directly and exercises the full TOML path: `toml::from_str → validate →
//!    toml::to_string_pretty → toml::from_str → validate`. This test is the
//!    end-to-end proof that the natural TOML form works after ADR-0005 replaced
//!    the old verbose `url::Url` serde with a Display/FromStr string form.

#![cfg(feature = "test-fixtures")]

use std::str::FromStr;

use tau_domain::fixtures::any_unchecked_manifest;
use tau_domain::{
    PackageKind, PackageManifest, PackageName, PackageSource, UncheckedManifest, Version,
};

const SAMPLE: &str = r#"
name = "fs-tools"
version = "0.3.0"
description = "Filesystem tools"
authors = ["Acme <hi@acme.dev>"]
license = "MIT OR Apache-2.0"
source = "https://example.com/fs.git#v0.3.0"
kind = "tool"
dependencies = []
capabilities = []
"#;

#[test]
fn programmatic_manifest_round_trips_through_serde_json() {
    let mut unchecked = any_unchecked_manifest();
    unchecked.name = PackageName::from_str("fs-tools").unwrap();
    unchecked.version = Version::parse("0.3.0").unwrap();
    unchecked.description = "Filesystem tools".into();
    unchecked.authors = vec!["Acme <hi@acme.dev>".into()];
    unchecked.license = Some("MIT OR Apache-2.0".into());
    unchecked.source = PackageSource::from_str("https://example.com/fs.git#v0.3.0").unwrap();
    unchecked.kind = PackageKind::Custom {
        kind: "tool".into(),
    };

    let manifest: PackageManifest = unchecked.clone().validate().unwrap();
    let json = serde_json::to_string(&manifest).unwrap();
    let back: UncheckedManifest = serde_json::from_str(&json).unwrap();
    let revalidated = back.validate().unwrap();

    assert_eq!(revalidated.name().as_str(), "fs-tools");
    assert_eq!(revalidated.description(), "Filesystem tools");
    assert_eq!(revalidated.version().to_string(), "0.3.0");
    assert_eq!(revalidated.license(), Some("MIT OR Apache-2.0"));
}

#[test]
fn manifest_round_trips_through_toml() {
    let unchecked: UncheckedManifest = toml::from_str(SAMPLE).expect("parse SAMPLE");

    assert_eq!(unchecked.name.as_str(), "fs-tools");
    assert_eq!(
        unchecked.source.to_string(),
        "https://example.com/fs.git#v0.3.0"
    );
    assert!(
        matches!(&unchecked.kind, PackageKind::Custom { kind } if kind == "tool"),
        "expected PackageKind::Custom {{ kind: \"tool\" }}, got {:?}",
        unchecked.kind
    );

    let manifest: PackageManifest = unchecked.validate().expect("validate");

    let toml_str = toml::to_string_pretty(&manifest).expect("serialize");
    let back: UncheckedManifest = toml::from_str(&toml_str).expect("re-parse");
    let revalidated = back.validate().expect("re-validate");

    assert_eq!(revalidated.name().as_str(), "fs-tools");
    assert_eq!(revalidated.version().to_string(), "0.3.0");
    assert_eq!(
        revalidated.source().to_string(),
        "https://example.com/fs.git#v0.3.0"
    );
    assert!(
        matches!(revalidated.kind(), PackageKind::Custom { kind } if kind == "tool"),
        "kind did not round-trip: {:?}",
        revalidated.kind()
    );
}
