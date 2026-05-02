//! Property tests for `ScopeConfig` TOML round-trip.

use std::time::{Duration, UNIX_EPOCH};

use proptest::prelude::*;
use tau_pkg::{ScopeConfig, ScopeError, ScopeKind};

fn arb_scope_kind() -> impl Strategy<Value = ScopeKind> {
    prop_oneof![Just(ScopeKind::Global), Just(ScopeKind::Project)]
}

fn arb_scope_config() -> impl Strategy<Value = ScopeConfig> {
    (
        arb_scope_kind(),
        (0u64..=4_000_000_000u64).prop_map(|s| UNIX_EPOCH + Duration::from_secs(s)),
        "[0-9]+\\.[0-9]+\\.[0-9]+",
    )
        .prop_map(|(kind, created_at, version)| {
            let mut cfg = ScopeConfig::new(kind);
            cfg.created_at = created_at;
            cfg.created_by_tau_version = version;
            cfg
        })
}

proptest! {
    #[test]
    fn scope_config_roundtrips_through_toml(cfg in arb_scope_config()) {
        let serialized = cfg.to_toml_string().expect("serialize");
        let parsed = ScopeConfig::read_from_str(&serialized).expect("deserialize");

        prop_assert_eq!(parsed.schema_version, cfg.schema_version);
        prop_assert_eq!(parsed.kind, cfg.kind);
        prop_assert_eq!(parsed.created_by_tau_version.clone(), cfg.created_by_tau_version.clone());

        let cfg_secs = cfg.created_at.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let parsed_secs = parsed.created_at.duration_since(UNIX_EPOCH).unwrap().as_secs();
        prop_assert_eq!(parsed_secs, cfg_secs);
    }
}

#[test]
fn scope_config_rejects_too_new_schema_version() {
    let toml_str = r#"
        schema_version = 999
        kind = "global"
        created_at = "2026-04-27T10:00:00Z"
        created_by_tau_version = "0.0.0"
    "#;
    let err = ScopeConfig::read_from_str(toml_str).unwrap_err();
    assert!(matches!(
        err,
        ScopeError::ConfigSchemaTooNew {
            found: 999,
            supported: 2,
        }
    ));
}
