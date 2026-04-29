//! Run the conformance suite against the OpenAI plugin.
//!
//! See `docs/superpowers/specs/2026-04-29-openai-plugin-design.md`
//! §8.3 for the catalog and charter.

use std::path::Path;

use openai_plugin_lib::{config::OpenAIConfig, plugin::OpenAIPlugin};
use tau_plugin_conformance::ConformanceSuite;
use tau_plugin_sdk::Configure;

#[tokio::test]
async fn run_conformance_suite() {
    let cassettes = Path::new("tests/conformance-cassettes");
    ConformanceSuite::default()
        .run(
            |base_url: String| {
                let mut cfg = OpenAIConfig::default();
                cfg.api_key = Some("sk-test".into());
                cfg.base_url = base_url;
                cfg.retry.max_attempts = 3;
                cfg.retry.base_delay_ms = 0;
                OpenAIPlugin::from_config(cfg).expect("build plugin")
            },
            cassettes,
        )
        .await;
}
