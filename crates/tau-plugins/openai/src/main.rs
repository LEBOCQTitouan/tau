//! `openai-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! Thin shim over [`tau_plugin_sdk::run_llm_backend_with_config`]:
//! the SDK runner drives the handshake, deserializes [`OpenAIConfig`]
//! from the handshake `config` field, constructs the plugin via
//! [`OpenAIPlugin::from_config`], and runs the dispatch loop.
//!
//! [`OpenAIConfig`]: openai_plugin_lib::config::OpenAIConfig
//! [`OpenAIPlugin::from_config`]: openai_plugin_lib::plugin::OpenAIPlugin

use openai_plugin_lib::plugin::OpenAIPlugin;
use tau_plugin_sdk::{run_llm_backend_with_config, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_llm_backend_with_config::<OpenAIPlugin>(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
        .await
}
