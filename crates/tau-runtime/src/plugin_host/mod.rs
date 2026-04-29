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
//! Task 13 lands the **skeleton only**: the public type surface
//! ([`PluginHostOptions`], [`RecordingSink`]) and the four
//! [`load_llm_backend`] / [`load_tool`] / [`load_storage`] /
//! [`load_sandbox`] entry-point signatures. The function bodies are
//! `unimplemented!()` placeholders; spawn + dispatch land in Tasks
//! 14-15, streaming in Task 16, and recording wiring in Task 17.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-loading-design.md` §7
//! and (forthcoming) ADR-0008 for the design rationale.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tau_pkg::LockedPlugin;
use tau_plugin_protocol::handshake::TraceContext;

use crate::builder::{DynLlmBackend, DynSandbox, DynStorage, DynTool};
use crate::error::RuntimeError;

mod handshake;
mod process;

/// Internal-but-test-visible re-exports. Hidden from rustdoc and not
/// covered by stability guarantees: integration tests under
/// `tau-runtime/tests/plugin_host_handshake.rs` reach in here to drive
/// [`handshake::drive_handshake`] over a `FakeStdioPeer`. Task 15's
/// `IpcLlmBackend` consumes [`process::PluginProcess`] via the
/// `pub(crate)` path; this `__internals` namespace is only the
/// integration-test escape hatch.
#[doc(hidden)]
pub mod __internals {
    pub use super::handshake::drive_handshake;
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
/// # Lifecycle (Tasks 14-15)
///
/// 1. Spawn `plugin.binary_path` with stdio piped, env scrubbed
///    except for `TAU_PLUGIN_RUN_ID` / `TAU_PLUGIN_AGENT_ID`.
/// 2. Send `meta.handshake` with `protocol_version`, the requested
///    [`tau_domain::PortKind::LlmBackend`], the supplied
///    `trace_context`, and the supplied `config`.
/// 3. Validate the handshake response: protocol version match,
///    `provides == LlmBackend`, required methods present.
/// 4. Spawn the per-process read loop and stderr re-emit task.
/// 5. Return an [`crate::plugin_host::ipc_llm::IpcLlmBackend`] wrapped
///    in `Arc<dyn DynLlmBackend>`.
///
/// On any of those steps failing, [`RuntimeError::PluginSpawnFailed`],
/// [`RuntimeError::PluginHandshakeFailed`], or
/// [`RuntimeError::PluginContractViolation`] is returned and the
/// child process is reaped.
///
/// # Task 13 stub
///
/// Returns `unimplemented!()` until Tasks 14-15 wire the actual spawn
/// and dispatch. The signature is final and Task 14+ fill the body
/// in-place.
pub async fn load_llm_backend(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    trace_context: TraceContext,
    options: PluginHostOptions,
) -> Result<Arc<dyn DynLlmBackend>, RuntimeError> {
    let _ = (plugin, config, trace_context, options);
    unimplemented!("load_llm_backend is implemented in Task 14-15")
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
/// # Task 13 stub
///
/// Returns `unimplemented!()` until Tasks 14-15.
pub async fn load_tool(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    trace_context: TraceContext,
    options: PluginHostOptions,
) -> Result<Arc<dyn DynTool>, RuntimeError> {
    let _ = (plugin, config, trace_context, options);
    unimplemented!("load_tool is implemented in Task 14-15")
}

/// Load a plugin that provides the `Storage` port and return a
/// kernel-ready `Arc<dyn DynStorage>` shim.
///
/// See [`load_llm_backend`] for the shared lifecycle / error taxonomy.
/// The required wire methods are `storage.get`, `storage.put`,
/// `storage.delete`, `storage.list` plus the meta methods.
///
/// # Task 13 stub
///
/// Returns `unimplemented!()` until Tasks 14-15.
pub async fn load_storage(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    trace_context: TraceContext,
    options: PluginHostOptions,
) -> Result<Arc<dyn DynStorage>, RuntimeError> {
    let _ = (plugin, config, trace_context, options);
    unimplemented!("load_storage is implemented in Task 14-15")
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
/// # Task 13 stub
///
/// Returns `unimplemented!()` until Tasks 14-15.
pub async fn load_sandbox(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    trace_context: TraceContext,
    options: PluginHostOptions,
) -> Result<Arc<dyn DynSandbox>, RuntimeError> {
    let _ = (plugin, config, trace_context, options);
    unimplemented!("load_sandbox is implemented in Task 14-15")
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
