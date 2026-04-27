//! Integration test: a programmatically-constructed manifest round-trips
//! through `UncheckedManifest → validate → PackageManifest → serde_json
//! → UncheckedManifest → validate`.
//!
//! Constructs the base manifest via `tau_domain::fixtures::any_unchecked_manifest()`
//! because `UncheckedManifest` is `#[non_exhaustive]`, blocking struct-literal
//! construction from outside the crate (E0639). The test then mutates the
//! fields it cares about.
//!
//! The TOML-shaped sample below is kept (unused) as documentation of the
//! intended manifest shape; the JSON path is the one actually exercised.

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
dependencies = []

[source]
[source.Git]
rev = "v0.3.0"
[source.Git.location]
[source.Git.location.Url]
"https" = "_"
# A placeholder; the test below uses programmatic construction to avoid
# TOML representation gymnastics for the url::Url internals.

kind = "tool"
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

    // Reference the SAMPLE constant so it's not dead code; TOML round-trip
    // for url::Url internals is finicky and out of scope for this test.
    let _ = SAMPLE;
}
