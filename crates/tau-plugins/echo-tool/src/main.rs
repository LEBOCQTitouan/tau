//! Toy `Tool` plugin that echoes its args back as a text content block.
//!
//! Used by tau-cli integration tests to exercise the plugin loading
//! mechanism end-to-end without depending on a real tool implementation.
//!
//! # Configuration
//!
//! Configurable via the handshake `config` field (set in
//! `[agents.<id>.config]` of the project tau.toml):
//!
//! - `error_on_invoke: bool` — return `Err(ToolError::Internal)` on
//!   every `tool.call`. Default: `false`.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use serde::Deserialize;
use tau_domain::Value;
use tau_plugin_sdk::{run_tool_with_config, ConfigError, Configure, SdkError};
use tau_ports::{
    fixtures::{make_tool_result, make_tool_spec},
    SessionContext, Tool, ToolContent, ToolError, ToolResult, ToolSpec,
};

/// Static configuration consumed from the handshake `config` field.
#[derive(Debug, Default, Deserialize)]
struct EchoConfig {
    /// If `true`, every `tool.call` returns `Err(ToolError::Internal)`.
    #[serde(default)]
    error_on_invoke: bool,
}

/// Toy `Tool` plugin.
struct EchoTool {
    config: EchoConfig,
}

impl Configure for EchoTool {
    type Config = EchoConfig;

    fn from_config(config: Self::Config) -> Result<Self, ConfigError> {
        Ok(EchoTool { config })
    }
}

impl Tool for EchoTool {
    type Session = ();

    fn name(&self) -> &str {
        "echo"
    }

    fn schema(&self) -> ToolSpec {
        // `ToolSpec` is `#[non_exhaustive]`; build via the test-fixtures
        // helper. The input schema is a JSON Schema describing the
        // single `text` arg.
        let schema_json = serde_json::json!({
            "type": "object",
            "properties": { "text": { "type": "string" } },
            "required": ["text"]
        });
        // Round-trip serde_json -> tau_domain::Value via JSON text.
        let schema_value: Value = serde_json::from_str(
            &serde_json::to_string(&schema_json).expect("static JSON schema serializes"),
        )
        .expect("static JSON schema round-trips through tau_domain::Value");
        make_tool_spec(
            "echo".to_string(),
            "Echoes its arguments back as a text content block.".to_string(),
            schema_value,
        )
    }

    async fn init(&self, _ctx: SessionContext) -> Result<Self::Session, ToolError> {
        Ok(())
    }

    async fn invoke(
        &self,
        _session: &mut Self::Session,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        if self.config.error_on_invoke {
            return Err(ToolError::Internal {
                message: "echo-tool error_on_invoke test mode".to_string(),
            });
        }
        let text = args
            .as_object()
            .and_then(|o| o.get("text"))
            .and_then(Value::as_string)
            .ok_or_else(|| ToolError::BadArgs {
                reason: "missing 'text' arg or wrong shape".to_string(),
            })?;
        Ok(make_tool_result(
            vec![ToolContent::Text {
                text: format!("echo: {text}"),
            }],
            false,
        ))
    }

    async fn teardown(&self, _session: Self::Session) -> Result<(), ToolError> {
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_tool_with_config::<EchoTool>(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).await
}
