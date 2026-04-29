//! Run the conformance suite against the Anthropic plugin.

use std::path::Path;

use anthropic_plugin_lib::{config::AnthropicConfig, plugin::AnthropicPlugin};
use tau_plugin_conformance::ConformanceSuite;
use tau_plugin_sdk::Configure;

#[tokio::test]
async fn run_conformance_suite() {
    let cassettes = Path::new("tests/conformance-cassettes");
    ConformanceSuite::default()
        .run(
            |base_url: String| {
                let mut cfg = AnthropicConfig::default();
                cfg.api_key = Some("sk-ant-test".into());
                cfg.base_url = base_url;
                cfg.retry.max_attempts = 3;
                cfg.retry.base_delay_ms = 0;
                AnthropicPlugin::from_config(cfg).expect("build plugin")
            },
            cassettes,
        )
        .await;
}
