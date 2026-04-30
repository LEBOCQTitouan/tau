//! `shell-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! Thin shim over [`tau_plugin_sdk::run_tool_with_config`].
//!
//! [`ShellConfig`]: shell_plugin_lib::config::ShellConfig
//! [`ShellPlugin::from_config`]: shell_plugin_lib::plugin::ShellPlugin

use shell_plugin_lib::plugin::ShellPlugin;
use tau_plugin_sdk::{run_tool_with_config, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_tool_with_config::<ShellPlugin>(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).await
}
