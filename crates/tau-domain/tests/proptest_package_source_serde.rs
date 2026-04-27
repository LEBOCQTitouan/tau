//! Property tests for PackageSource and PackageKind serde round-trip.
//!
//! After ADR-0005 (forthcoming), PackageSource and PackageKind use
//! custom Serialize/Deserialize impls that round-trip through their
//! Display/FromStr string forms. This test verifies that arbitrary
//! valid inputs survive serialization to TOML and JSON.

#![cfg(feature = "serde")]

use std::str::FromStr;

use proptest::prelude::*;
use tau_domain::{PackageKind, PackageSource};

fn arb_url_source_str() -> impl Strategy<Value = String> {
    let scheme = prop_oneof![Just("https"), Just("http"), Just("ssh"), Just("git")];
    let host = "[a-z][a-z0-9]{0,15}(\\.[a-z][a-z0-9]{0,15}){1,3}";
    let path = "[a-z0-9]{1,20}(/[a-z0-9]{1,20}){0,3}\\.git";
    (scheme, host, path).prop_map(|(s, h, p)| format!("{s}://{h}/{p}"))
}

fn arb_scp_source_str() -> impl Strategy<Value = String> {
    let user = "[a-z][a-z0-9]{0,15}";
    let host = "[a-z][a-z0-9]{0,15}(\\.[a-z][a-z0-9]{0,15}){1,3}";
    let path = "[a-z0-9]{1,20}(/[a-z0-9]{1,20}){0,3}\\.git";
    (user, host, path).prop_map(|(u, h, p)| format!("{u}@{h}:{p}"))
}

fn arb_rev() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_.-]{1,20}".prop_map(String::from)
}

fn arb_source_str() -> impl Strategy<Value = String> {
    prop_oneof![
        arb_url_source_str(),
        arb_scp_source_str(),
        (arb_url_source_str(), arb_rev()).prop_map(|(s, r)| format!("{s}#{r}")),
        (arb_scp_source_str(), arb_rev()).prop_map(|(s, r)| format!("{s}#{r}")),
    ]
}

fn arb_kind_str() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{0,15}".prop_map(String::from)
}

proptest! {
    /// PackageSource → JSON → PackageSource round-trips.
    #[test]
    fn package_source_json_round_trips(s in arb_source_str()) {
        let original = PackageSource::from_str(&s).unwrap();
        let json = serde_json::to_string(&original).expect("serialize");
        prop_assert_eq!(json.clone(), format!("\"{s}\""));
        let parsed: PackageSource = serde_json::from_str(&json).expect("deserialize");
        prop_assert_eq!(parsed, original);
    }

    /// PackageSource → TOML (wrapped in a struct) → PackageSource round-trips.
    #[test]
    fn package_source_toml_round_trips(s in arb_source_str()) {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Wrap {
            source: PackageSource,
        }

        let original = PackageSource::from_str(&s).unwrap();
        let wrap = Wrap { source: original.clone() };
        let toml_str = toml::to_string(&wrap).expect("serialize");
        prop_assert!(
            toml_str.contains(&format!("source = \"{s}\"")),
            "expected `source = \"{s}\"` in: {toml_str}"
        );
        let parsed: Wrap = toml::from_str(&toml_str).expect("deserialize");
        prop_assert_eq!(parsed.source, original);
    }

    /// PackageKind → JSON → PackageKind round-trips.
    #[test]
    fn package_kind_json_round_trips(k in arb_kind_str()) {
        let json_in = format!("\"{k}\"");
        let kind: PackageKind = serde_json::from_str(&json_in).expect("deserialize");
        let json_out = serde_json::to_string(&kind).expect("serialize");
        prop_assert_eq!(json_out, json_in);
    }

    /// PackageKind → TOML (wrapped) → PackageKind round-trips.
    #[test]
    fn package_kind_toml_round_trips(k in arb_kind_str()) {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Wrap {
            kind: PackageKind,
        }

        let kind: PackageKind = serde_json::from_str(&format!("\"{k}\"")).expect("deserialize");
        let wrap = Wrap { kind };
        let toml_str = toml::to_string(&wrap).expect("serialize");
        prop_assert_eq!(toml_str.trim_end(), &format!("kind = \"{k}\""));
        let parsed: Wrap = toml::from_str(&toml_str).expect("deserialize");
        // Equality not derivable for non_exhaustive; compare by re-serialization.
        let original_json = serde_json::to_string(&parsed.kind).unwrap();
        prop_assert_eq!(original_json, format!("\"{k}\""));
    }
}

#[test]
fn package_kind_rejects_empty_string() {
    let result: Result<PackageKind, _> = serde_json::from_str("\"\"");
    assert!(result.is_err(), "empty string should fail to deserialize");
}

#[test]
fn package_source_rejects_empty_string() {
    let result: Result<PackageSource, _> = serde_json::from_str("\"\"");
    assert!(result.is_err(), "empty string should fail to deserialize");
}

#[test]
fn package_source_rejects_empty_rev() {
    let result: Result<PackageSource, _> = serde_json::from_str("\"https://x.com/y.git#\"");
    assert!(
        result.is_err(),
        "empty revision after `#` should fail to deserialize"
    );
}
