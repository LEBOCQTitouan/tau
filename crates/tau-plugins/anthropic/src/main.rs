//! `anthropic-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! The binary is a thin shim over
//! [`tau_plugin_sdk::run_llm_backend_with_config`]: the SDK runner drives
//! the handshake, deserializes [`AnthropicConfig`] from the handshake
//! `config` field, constructs the plugin via
//! [`AnthropicPlugin::from_config`], and runs the dispatch loop.
//!
//! [`AnthropicConfig`]: anthropic_plugin_lib::config::AnthropicConfig
//! [`AnthropicPlugin::from_config`]: anthropic_plugin_lib::plugin::AnthropicPlugin

use anthropic_plugin_lib::plugin::AnthropicPlugin;
use tau_plugin_sdk::{run_llm_backend_with_config, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_llm_backend_with_config::<AnthropicPlugin>(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    )
    .await
}
