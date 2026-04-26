//! Table-driven test: malformed manifests produce specific
//! `PackageManifestError` variants. Replaces a proptest that would have
//! had to generate "valid except for one thing" — which is more brittle
//! than hand-picked cases.
//!
//! Constructs base manifests via `tau_domain::fixtures::any_unchecked_manifest()`
//! because `UncheckedManifest` is `#[non_exhaustive]`, blocking struct-literal
//! construction from outside the crate (E0639). Each test then mutates the
//! field it cares about.

#![cfg(feature = "test-fixtures")]

use std::collections::BTreeMap;

use tau_domain::fixtures::any_unchecked_manifest;
use tau_domain::{Capability, PackageManifestError, UncheckedManifest};

fn good() -> UncheckedManifest {
    any_unchecked_manifest()
}

#[test]
fn good_validates() {
    assert!(good().validate().is_ok());
}

#[test]
fn empty_description() {
    let mut u = good();
    u.description = String::new();
    assert_eq!(
        u.validate().unwrap_err(),
        PackageManifestError::EmptyDescription
    );
}

#[test]
fn empty_capability_custom_name() {
    let mut u = good();
    u.capabilities = vec![Capability::Custom {
        name: String::new(),
        params: BTreeMap::new(),
    }];
    assert_eq!(
        u.validate().unwrap_err(),
        PackageManifestError::CapabilityEmptyName { index: 0 },
    );
}

#[test]
fn empty_capability_at_nonzero_index() {
    let mut u = good();
    u.capabilities = vec![
        Capability::Custom {
            name: "ok".into(),
            params: BTreeMap::new(),
        },
        Capability::Custom {
            name: String::new(),
            params: BTreeMap::new(),
        },
    ];
    assert_eq!(
        u.validate().unwrap_err(),
        PackageManifestError::CapabilityEmptyName { index: 1 },
    );
}
