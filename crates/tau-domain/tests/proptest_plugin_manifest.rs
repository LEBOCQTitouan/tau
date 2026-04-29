//! Proptest: PortKind / PluginKind / PluginManifest TOML round-trip.

#![cfg(feature = "serde")]

use proptest::prelude::*;
use tau_domain::{PluginKind, PluginManifest, PortKind};

prop_compose! {
    fn arb_port_kind()(idx in 0u8..4u8) -> PortKind {
        match idx {
            0 => PortKind::LlmBackend,
            1 => PortKind::Tool,
            2 => PortKind::Storage,
            _ => PortKind::Sandbox,
        }
    }
}

prop_compose! {
    fn arb_plugin_kind()(_pad in 0u8..1u8) -> PluginKind {
        PluginKind::RustCargo
    }
}

prop_compose! {
    fn arb_bin()(s in "[a-z][a-z0-9_-]{0,30}") -> String {
        s
    }
}

prop_compose! {
    fn arb_plugin_manifest()(
        provides in arb_port_kind(),
        kind in arb_plugin_kind(),
        bin in arb_bin(),
    ) -> PluginManifest {
        PluginManifest::new(provides, kind, bin)
    }
}

proptest! {
    #[test]
    fn plugin_manifest_toml_round_trip(m in arb_plugin_manifest()) {
        let s = toml::to_string(&m).unwrap();
        let back: PluginManifest = toml::from_str(&s).unwrap();
        prop_assert_eq!(m, back);
    }
}
