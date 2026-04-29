//! `ollama-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! Thin shim over [`tau_plugin_sdk::run_llm_backend_with_config`]:
//! the SDK runner drives the handshake, deserializes [`OllamaConfig`]
//! from the handshake `config` field, constructs the plugin via
//! [`OllamaPlugin::from_config`], and runs the dispatch loop.
//!
//! [`OllamaConfig`]: ollama_plugin_lib::config::OllamaConfig
//! [`OllamaPlugin::from_config`]: ollama_plugin_lib::plugin::OllamaPlugin

use ollama_plugin_lib::plugin::OllamaPlugin;
use tau_plugin_sdk::{run_llm_backend_with_config, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_llm_backend_with_config::<OllamaPlugin>(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
        .await
}
