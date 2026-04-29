//! Run the conformance suite against the Ollama plugin.

use std::path::Path;

use ollama_plugin_lib::{config::OllamaConfig, plugin::OllamaPlugin};
use tau_plugin_conformance::ConformanceSuite;
use tau_plugin_sdk::Configure;

#[tokio::test]
async fn run_conformance_suite() {
    let cassettes = Path::new("tests/conformance-cassettes");
    ConformanceSuite::default()
        .run(
            |base_url: String| {
                let mut cfg = OllamaConfig::default();
                cfg.base_url = base_url;
                cfg.bearer_token_env =
                    "OLLAMA_BEARER_TOKEN_DEFINITELY_NOT_SET_FOR_CONFORMANCE".into();
                cfg.retry.max_attempts = 3;
                cfg.retry.base_delay_ms = 0;
                OllamaPlugin::from_config(cfg).expect("build plugin")
            },
            cassettes,
        )
        .await;
}
