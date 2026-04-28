//! Generic runner for plugins that implement [`tau_ports::Sandbox`].
//!
//! v0.1 stub: drives the handshake correctly so a host can load the
//! plugin, but returns [`METHOD_NOT_FOUND`] for `sandbox.*` methods
//! since no toy plugin exercises them end-to-end. The full dispatch
//! lands once the v0.1 PROVISIONAL [`tau_ports::Sandbox`] surface
//! firms up.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-loading-design.md` §5.2.

use std::collections::BTreeMap;
use std::sync::Arc;

use tau_domain::PortKind;
use tau_plugin_protocol::{
    error::{RpcErrorEnvelope, METHOD_NOT_FOUND},
    handshake::meta,
    Frame, FramedReader, FramedWriter, FramerOptions, MethodSchema,
};
use tau_ports::Sandbox;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::error::SdkError;
use crate::handshake::{drive_handshake, PluginMeta};
use crate::tracing_layer;

/// Run a plugin that implements [`Sandbox`]. Reads frames from stdin,
/// writes frames to stdout. Returns when the host sends
/// `meta.shutdown` or stdin closes.
///
/// v0.1 stub: handshake works; `sandbox.*` methods return
/// `METHOD_NOT_FOUND` until the host wiring lands.
pub async fn run_sandbox<P>(
    plugin: P,
    plugin_name: &str,
    plugin_version: &str,
) -> Result<(), SdkError>
where
    P: Sandbox + Send + Sync + 'static,
{
    tracing_layer::install();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = FramedReader::new(stdin, FramerOptions::default());
    let mut writer = FramedWriter::new(stdout);

    run_sandbox_with_io(
        &mut reader,
        &mut writer,
        plugin,
        plugin_name,
        plugin_version,
    )
    .await
}

/// Same as [`run_sandbox`] but accepts an explicit reader and writer.
pub async fn run_sandbox_with_io<R, W, P>(
    reader: &mut FramedReader<R>,
    writer: &mut FramedWriter<W>,
    plugin: P,
    plugin_name: &str,
    plugin_version: &str,
) -> Result<(), SdkError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    P: Sandbox + Send + Sync + 'static,
{
    let _plugin = Arc::new(plugin);

    let plugin_meta = build_sandbox_meta(plugin_name, plugin_version);
    let _request = drive_handshake(reader, writer, plugin_meta).await?;

    loop {
        let body = match reader.next_frame().await? {
            Some(b) => b,
            None => break,
        };
        let frame = Frame::decode(&body)?;
        match frame {
            Frame::Request { id, method, .. } => {
                let envelope = RpcErrorEnvelope::new(
                    METHOD_NOT_FOUND,
                    format!("sandbox runner does not yet dispatch: {method}"),
                    None,
                );
                let response = Frame::Response {
                    id,
                    error: Some(envelope),
                    result: None,
                };
                writer.write_frame(&response.encode()?).await?;
            }
            Frame::Notification { method, .. } if method == meta::SHUTDOWN_METHOD => {
                tracing::info!(target: "tau_plugin_sdk", "received meta.shutdown");
                break;
            }
            _ => { /* ignore */ }
        }
    }

    Ok(())
}

fn build_sandbox_meta(plugin_name: &str, plugin_version: &str) -> PluginMeta {
    let mut schemas = BTreeMap::new();
    schemas.insert(
        meta::DESCRIBE_METHOD.to_string(),
        MethodSchema::new(serde_json::json!({}), serde_json::json!({})),
    );
    PluginMeta::new(
        plugin_name.to_string(),
        plugin_version.to_string(),
        PortKind::Sandbox,
        vec![meta::DESCRIBE_METHOD.to_string()],
        schemas,
    )
}
