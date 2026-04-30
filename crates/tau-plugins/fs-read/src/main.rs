//! `fs-read-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! Thin shim over [`tau_plugin_sdk::run_tool_with_config`]: the SDK
//! runner drives the handshake, deserializes [`FsReadConfig`] from
//! the handshake `config` field, constructs the plugin via
//! [`FsReadPlugin::from_config`], and runs the dispatch loop.
//!
//! [`FsReadConfig`]: fs_read_plugin_lib::config::FsReadConfig
//! [`FsReadPlugin::from_config`]: fs_read_plugin_lib::plugin::FsReadPlugin

use fs_read_plugin_lib::plugin::FsReadPlugin;
use tau_plugin_sdk::{run_tool_with_config, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_tool_with_config::<FsReadPlugin>(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).await
}
