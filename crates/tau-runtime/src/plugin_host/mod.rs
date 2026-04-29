//! Host-side plugin loader for `tau-runtime`.
//!
//! This module spawns plugin processes, drives the `meta.handshake`
//! exchange, and produces `Arc<dyn DynLlmBackend>` (and friends) that
//! the kernel consumes unchanged. Plugin authors sit on the SDK side
//! ([`tau_plugin_sdk`](../../tau_plugin_sdk/index.html)); this module
//! is the host counterpart, the *opposite* end of the same wire
//! protocol defined in `tau-plugin-protocol`.
//!
//! # Module status
//!
//! Task 15 wires spawn + dispatch end-to-end: the four [`load_*`]
//! entry points spawn the plugin binary, drive the handshake, and
//! return an `Arc<dyn Dyn*>` shim backed by an `Ipc*` adapter
//! (`IpcLlmBackend` / `IpcTool` / `IpcStorage` / `IpcSandbox`).
//! Streaming (`llm.stream`) is wired in Task 16; protocol recording
//! is wired in Task 17.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-loading-design.md` §7
//! and (forthcoming) ADR-0008 for the design rationale.
//!
//! [`load_*`]: load_llm_backend

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tau_domain::PortKind;
use tau_pkg::LockedPlugin;
use tau_plugin_protocol::handshake::TraceContext;
use tau_plugin_protocol::FramerOptions;

use crate::builder::{DynLlmBackend, DynSandbox, DynStorage, DynTool};
use crate::error::RuntimeError;

mod handshake;
mod ipc_llm;
mod ipc_sandbox;
mod ipc_storage;
mod ipc_tool;
mod process;

/// Internal-but-test-visible re-exports. Hidden from rustdoc and not
/// covered by stability guarantees: integration tests under
/// `tau-runtime/tests/plugin_host_*.rs` reach in here to drive the
/// handshake driver and the per-port IPC adapters over a
/// [`tau_plugin_protocol::test_support::FakeStdioPeer`] without
/// spawning real subprocesses.
///
/// The non-test items
/// ([`__internals::drive_handshake`]) are always available; the test
/// constructors ([`__internals::PluginProcess`],
/// [`__internals::IpcLlmBackend`], etc.) require the
/// `test-support` cargo feature so the production build can drop them.
#[doc(hidden)]
pub mod __internals {
    pub use super::handshake::drive_handshake;

    #[cfg(any(test, feature = "test-support"))]
    pub use super::ipc_llm::IpcLlmBackend;
    #[cfg(any(test, feature = "test-support"))]
    pub use super::ipc_sandbox::IpcSandbox;
    #[cfg(any(test, feature = "test-support"))]
    pub use super::ipc_storage::IpcStorage;
    #[cfg(any(test, feature = "test-support"))]
    pub use super::ipc_tool::IpcTool;
    #[cfg(any(test, feature = "test-support"))]
    pub use super::process::{DynAsyncWriter, PluginProcess};
}

/// Optional protocol-recording sink. Currently only
/// [`RecordingSink::JsonlFile`] is defined; Task 17 wires the actual
/// frame-by-frame tap.
///
/// `#[non_exhaustive]`: future sinks (e.g. an in-memory ring buffer
/// for tests, a UDS-streamed sink for live inspection) are additive.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum RecordingSink {
    /// Append-mode JSONL file. Each frame becomes one line carrying
    /// metadata (`ts`, `plugin`, `dir`, `msgid`, `method`) and the
    /// base64-encoded MessagePack frame, per spec §7.8.
    JsonlFile {
        /// Path to the recording file. The file is opened in append
        /// mode and created if it doesn't exist.
        path: PathBuf,
    },
}

/// Tunables controlling plugin-host behavior.
///
/// `#[non_exhaustive]`: additive fields are non-breaking. Use
/// [`PluginHostOptions::default`] and mutate the fields you care about
/// rather than struct-literal construction across crate boundaries.
///
/// # Example
///
/// ```rust,ignore
/// // `PluginHostOptions` is `#[non_exhaustive]`; doctests can't
/// // construct via struct-literal syntax. Use `default()` and mutate.
/// use std::time::Duration;
/// use tau_runtime::plugin_host::PluginHostOptions;
///
/// let mut opts = PluginHostOptions::default();
/// opts.handshake_timeout = Duration::from_secs(10);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct PluginHostOptions {
    /// How long the host waits for the plugin's `meta.handshake`
    /// response before declaring the plugin unresponsive (surfaces as
    /// [`RuntimeError::PluginHandshakeFailed`] with reason
    /// [`crate::error::HandshakeFailureReason::Timeout`]). Default
    /// `5s`.
    pub handshake_timeout: Duration,

    /// How long the host waits for the plugin to exit cleanly after a
    /// `meta.shutdown` notification before escalating to SIGTERM /
    /// SIGKILL. Default `2s`.
    pub shutdown_timeout: Duration,

    /// Maximum decoded frame body size (bytes), passed to the framer
    /// to bound memory use against malformed peers. Default 64 MiB.
    pub max_message_size: usize,

    /// If `Some`, every frame in either direction is mirrored to the
    /// supplied sink. Used by `tau --record-protocol <path>` (Task 20)
    /// and integration-test golden files. Defaults to `None`.
    pub recording: Option<RecordingSink>,
}

impl Default for PluginHostOptions {
    fn default() -> Self {
        Self {
            handshake_timeout: Duration::from_secs(5),
            shutdown_timeout: Duration::from_secs(2),
            // 64 MiB matches `tau-plugin-protocol::framer`'s default
            // `max_frame_size` so the same ceiling applies on both
            // sides of the wire.
            max_message_size: 64 * 1024 * 1024,
            recording: None,
        }
    }
}

/// Load a plugin that provides the `LlmBackend` port and return a
/// kernel-ready `Arc<dyn DynLlmBackend>` shim.
///
/// # Lifecycle
///
/// 1. Spawn `plugin.binary_path` with stdio piped, env scrubbed
///    except for `TAU_PLUGIN_RUN_ID` / `TAU_PLUGIN_AGENT_ID` (plus
///    `PATH` for shared-library resolution).
/// 2. Send `meta.handshake` with `protocol_version`, the requested
///    [`tau_domain::PortKind::LlmBackend`], the supplied
///    `trace_context`, and the supplied `config`.
/// 3. Validate the handshake response: protocol version match,
///    `provides == LlmBackend`, required methods include
///    `llm.complete`.
/// 4. Spawn the per-process read loop and stderr re-emit task.
/// 5. Return an `IpcLlmBackend` wrapped in `Arc<dyn DynLlmBackend>`.
///
/// On any of those steps failing, [`RuntimeError::PluginSpawnFailed`],
/// [`RuntimeError::PluginHandshakeFailed`], or
/// [`RuntimeError::PluginContractViolation`] is returned and the
/// child process is reaped.
pub async fn load_llm_backend(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    trace_context: TraceContext,
    options: PluginHostOptions,
) -> Result<Arc<dyn DynLlmBackend>, RuntimeError> {
    let plugin_name = plugin.manifest.bin.clone();
    let run_id = trace_context.run_id.clone();
    let agent_id = trace_context.agent_id.clone();
    let trace_for_handshake = trace_context.clone();
    let config_for_handshake = config;
    let plugin_name_for_handshake = plugin_name.clone();
    let handshake_timeout = options.handshake_timeout;
    // `FramerOptions` is `#[non_exhaustive]`; use `default()` and
    // mutate the field rather than struct-literal construction.
    let mut framer_options = FramerOptions::default();
    framer_options.max_message_size = options.max_message_size;

    let (process, _handshake_response) = process::PluginProcess::spawn_and_handshake(
        &plugin.binary_path,
        plugin_name.clone(),
        &run_id,
        &agent_id,
        framer_options,
        options.shutdown_timeout,
        |reader, writer| {
            Box::pin(async move {
                handshake::drive_handshake(
                    reader,
                    writer,
                    &plugin_name_for_handshake,
                    PortKind::LlmBackend,
                    &["llm.complete"],
                    config_for_handshake,
                    trace_for_handshake,
                    handshake_timeout,
                )
                .await
            })
        },
    )
    .await?;

    Ok(Arc::new(ipc_llm::IpcLlmBackend::new(plugin_name, process)) as Arc<dyn DynLlmBackend>)
}

/// Load a plugin that provides the `Tool` port and return a
/// kernel-ready `Arc<dyn DynTool>` shim.
///
/// See [`load_llm_backend`] for the shared lifecycle / error
/// taxonomy. `Tool`-specific notes:
///
/// - The host drives the full `Tool` lifecycle (`init` / `invoke` /
///   `teardown`) per `tool.call`; the SDK runner on the plugin side
///   handles the corresponding session lifetime.
/// - The required wire methods are `tool.call` (and the meta methods
///   shared by every port).
///
/// # Implementation
///
/// 1. Spawn + handshake against [`tau_domain::PortKind::Tool`] +
///    `tool.call`.
/// 2. Issue a `tool.describe` RPC over the running process to capture
///    the [`tau_ports::ToolSpec`] eagerly (the kernel embeds it in
///    each `CompletionRequest.tools`).
/// 3. Wrap the process + cached schema in
///    [`ipc_tool::IpcTool`] and erase to `Arc<dyn DynTool>`.
pub async fn load_tool(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    trace_context: TraceContext,
    options: PluginHostOptions,
) -> Result<Arc<dyn DynTool>, RuntimeError> {
    let plugin_name = plugin.manifest.bin.clone();
    let run_id = trace_context.run_id.clone();
    let agent_id = trace_context.agent_id.clone();
    let trace_for_handshake = trace_context.clone();
    let config_for_handshake = config;
    let plugin_name_for_handshake = plugin_name.clone();
    let handshake_timeout = options.handshake_timeout;
    let mut framer_options = FramerOptions::default();
    framer_options.max_message_size = options.max_message_size;

    let (process, _handshake_response) = process::PluginProcess::spawn_and_handshake(
        &plugin.binary_path,
        plugin_name.clone(),
        &run_id,
        &agent_id,
        framer_options,
        options.shutdown_timeout,
        |reader, writer| {
            Box::pin(async move {
                handshake::drive_handshake(
                    reader,
                    writer,
                    &plugin_name_for_handshake,
                    PortKind::Tool,
                    &["tool.call"],
                    config_for_handshake,
                    trace_for_handshake,
                    handshake_timeout,
                )
                .await
            })
        },
    )
    .await?;

    // Eagerly fetch the tool's schema so the kernel doesn't pay an
    // RPC round-trip per turn for `CompletionRequest.tools`.
    let schema = ipc_tool::IpcTool::fetch_schema(&process)
        .await
        .map_err(|e| RuntimeError::PluginContractViolation {
            plugin: plugin_name.clone(),
            detail: format!("tool.describe failed: {e}"),
        })?;

    Ok(Arc::new(ipc_tool::IpcTool::new(plugin_name, schema, process)) as Arc<dyn DynTool>)
}

/// Load a plugin that provides the `Storage` port and return a
/// kernel-ready `Arc<dyn DynStorage>` shim.
///
/// See [`load_llm_backend`] for the shared lifecycle / error taxonomy.
/// The required wire methods are `storage.get`, `storage.put`,
/// `storage.delete`, `storage.list` plus the meta methods.
///
/// # Implementation
///
/// Spawn + handshake against [`tau_domain::PortKind::Storage`]; the
/// host doesn't require any specific methods at handshake time
/// because the kernel doesn't route through `Storage` in v0.1 (per
/// spec §1.1). The returned [`ipc_storage::IpcStorage`] dispatches
/// `storage.{get,put,list,delete}` per call.
pub async fn load_storage(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    trace_context: TraceContext,
    options: PluginHostOptions,
) -> Result<Arc<dyn DynStorage>, RuntimeError> {
    let plugin_name = plugin.manifest.bin.clone();
    let run_id = trace_context.run_id.clone();
    let agent_id = trace_context.agent_id.clone();
    let trace_for_handshake = trace_context.clone();
    let config_for_handshake = config;
    let plugin_name_for_handshake = plugin_name.clone();
    let handshake_timeout = options.handshake_timeout;
    let mut framer_options = FramerOptions::default();
    framer_options.max_message_size = options.max_message_size;

    let (process, _handshake_response) = process::PluginProcess::spawn_and_handshake(
        &plugin.binary_path,
        plugin_name.clone(),
        &run_id,
        &agent_id,
        framer_options,
        options.shutdown_timeout,
        |reader, writer| {
            Box::pin(async move {
                handshake::drive_handshake(
                    reader,
                    writer,
                    &plugin_name_for_handshake,
                    PortKind::Storage,
                    &[],
                    config_for_handshake,
                    trace_for_handshake,
                    handshake_timeout,
                )
                .await
            })
        },
    )
    .await?;

    Ok(Arc::new(ipc_storage::IpcStorage::new(plugin_name, process)) as Arc<dyn DynStorage>)
}

/// Load a plugin that provides the `Sandbox` port and return a
/// kernel-ready `Arc<dyn DynSandbox>` shim.
///
/// **PROVISIONAL** — the `Sandbox` port itself is a v0.1 sketch
/// (see `tau_ports::Sandbox` doc comment). Phase 1 will likely
/// require breaking changes to the trait and to the wire methods;
/// this entry point exists so plugin-host wiring is symmetric across
/// the four ports.
///
/// # Implementation
///
/// Spawn + handshake against [`tau_domain::PortKind::Sandbox`]; the
/// host doesn't require any specific methods at handshake time
/// because the kernel doesn't route through `Sandbox::create` in
/// v0.1 (per spec §1.1). The returned
/// [`ipc_sandbox::IpcSandbox`] dispatches `sandbox.run` per call.
pub async fn load_sandbox(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    trace_context: TraceContext,
    options: PluginHostOptions,
) -> Result<Arc<dyn DynSandbox>, RuntimeError> {
    let plugin_name = plugin.manifest.bin.clone();
    let run_id = trace_context.run_id.clone();
    let agent_id = trace_context.agent_id.clone();
    let trace_for_handshake = trace_context.clone();
    let config_for_handshake = config;
    let plugin_name_for_handshake = plugin_name.clone();
    let handshake_timeout = options.handshake_timeout;
    let mut framer_options = FramerOptions::default();
    framer_options.max_message_size = options.max_message_size;

    let (process, _handshake_response) = process::PluginProcess::spawn_and_handshake(
        &plugin.binary_path,
        plugin_name.clone(),
        &run_id,
        &agent_id,
        framer_options,
        options.shutdown_timeout,
        |reader, writer| {
            Box::pin(async move {
                handshake::drive_handshake(
                    reader,
                    writer,
                    &plugin_name_for_handshake,
                    PortKind::Sandbox,
                    &[],
                    config_for_handshake,
                    trace_for_handshake,
                    handshake_timeout,
                )
                .await
            })
        },
    )
    .await?;

    Ok(Arc::new(ipc_sandbox::IpcSandbox::new(plugin_name, process)) as Arc<dyn DynSandbox>)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::error::HandshakeFailureReason;

    #[test]
    fn plugin_host_options_default_has_5_second_handshake_timeout() {
        let opts = PluginHostOptions::default();
        assert_eq!(opts.handshake_timeout, Duration::from_secs(5));
    }

    #[test]
    fn plugin_host_options_default_has_2_second_shutdown_timeout() {
        let opts = PluginHostOptions::default();
        assert_eq!(opts.shutdown_timeout, Duration::from_secs(2));
    }

    #[test]
    fn plugin_host_options_default_has_64mib_max_message_size() {
        let opts = PluginHostOptions::default();
        assert_eq!(opts.max_message_size, 64 * 1024 * 1024);
    }

    #[test]
    fn plugin_host_options_default_has_no_recording() {
        let opts = PluginHostOptions::default();
        assert!(opts.recording.is_none());
    }

    #[test]
    fn recording_sink_jsonl_file_is_constructible() {
        // Smoke: the variant is constructible with a path. Actual sink
        // wiring lands in Task 17.
        let sink = RecordingSink::JsonlFile {
            path: PathBuf::from("/tmp/tau-protocol.jsonl"),
        };
        match sink {
            RecordingSink::JsonlFile { path } => {
                assert_eq!(path, PathBuf::from("/tmp/tau-protocol.jsonl"));
            }
        }
    }

    #[test]
    fn handshake_failure_reason_displays_as_expected() {
        let r = HandshakeFailureReason::Timeout;
        assert_eq!(format!("{r}"), "timeout");

        let r = HandshakeFailureReason::ProtocolVersionMismatch {
            host: "1".into(),
            plugin: "2".into(),
        };
        let s = format!("{r}");
        assert!(s.contains("1"), "got: {s}");
        assert!(s.contains("2"), "got: {s}");
    }
}
