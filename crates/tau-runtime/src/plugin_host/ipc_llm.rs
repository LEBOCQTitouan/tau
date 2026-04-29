//! Host-side [`crate::builder::DynLlmBackend`] implementation backed by
//! a spawned plugin subprocess.
//!
//! Thin RPC dispatcher: each `complete()` call allocates a msgid,
//! registers a `oneshot::Sender` in the shared `in_flight_responses`
//! map, sends a single `Frame::Request` over the writer, and awaits the
//! response.
//!
//! See spec §7.4 (per-port adapters) and §7.3 (PluginProcess + read
//! loop). Streaming (`llm.stream`) is wired in Task 16 via the
//! `stream_router`; this task returns
//! [`tau_ports::LlmError::Internal`] for `stream()`.

use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use tau_plugin_protocol::Frame;
use tau_ports::{CompletionRequest, CompletionResponse, CompletionStream, LlmError};
use tokio::sync::oneshot;

use crate::builder::DynLlmBackend;

use super::process::{PluginProcess, RpcResult};

/// Wire method name for the non-streaming LLM completion.
const LLM_COMPLETE_METHOD: &str = "llm.complete";

/// IPC-backed [`DynLlmBackend`] adapter. Holds an `Arc<PluginProcess>`
/// shared with the read loop and any other adapters bound to the same
/// plugin instance.
///
/// Constructed by [`crate::plugin_host::load_llm_backend`] after the
/// handshake validates that the plugin advertises `llm.complete`. The
/// type is `pub` so the integration-test `__internals` re-exports
/// (gated by `feature = "test-support"`) can build instances without
/// spawning a real subprocess; production code only sees an
/// `Arc<dyn DynLlmBackend>` and never the concrete type.
pub struct IpcLlmBackend {
    pub(crate) name: String,
    pub(crate) process: Arc<PluginProcess>,
}

impl IpcLlmBackend {
    /// Construct an adapter from a plugin name (host-visible identity)
    /// and a shared [`PluginProcess`]. The name is what the kernel uses
    /// to resolve `agent_def.llm_backend` against the registry, and is
    /// expected to match the plugin's [`tau_pkg::LockedPlugin`] manifest
    /// name.
    pub fn new(name: String, process: Arc<PluginProcess>) -> Self {
        Self { name, process }
    }
}

impl DynLlmBackend for IpcLlmBackend {
    fn name(&self) -> &str {
        &self.name
    }

    fn complete<'a>(
        &'a self,
        req: CompletionRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<CompletionResponse, LlmError>> + 'a>> {
        let process = self.process.clone();
        Box::pin(async move {
            // Dispatch via the shared msgid+oneshot pattern (spec §7.4).
            let id = process.next_msgid.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel::<RpcResult>();
            {
                let mut map = process.in_flight_responses.lock().await;
                map.insert(id, tx);
            }
            // Wire shape: params is a 1-element array `[CompletionRequest]`,
            // matching `tau_plugin_sdk::runners::llm_backend::dispatch_llm`.
            let params_bytes = rmp_serde::to_vec(&vec![&req]).map_err(|e| LlmError::Internal {
                message: format!("rmp encode CompletionRequest: {e}"),
            })?;
            let frame = Frame::Request {
                id,
                method: LLM_COMPLETE_METHOD.to_string(),
                params: params_bytes,
            };
            let frame_bytes = frame.encode().map_err(|e| LlmError::Internal {
                message: format!("frame encode: {e}"),
            })?;
            {
                let mut writer = process.writer.lock().await;
                writer
                    .write_frame(&frame_bytes)
                    .await
                    .map_err(|e| LlmError::Internal {
                        message: format!("write frame: {e}"),
                    })?;
            }
            let result = rx.await.map_err(|_| LlmError::Internal {
                message: "in-flight response sender dropped (plugin crashed?)".to_string(),
            })?;
            match result {
                Ok(bytes) => rmp_serde::from_slice::<CompletionResponse>(&bytes).map_err(|e| {
                    LlmError::Internal {
                        message: format!("rmp decode CompletionResponse: {e}"),
                    }
                }),
                Err(envelope) => Err(LlmError::Internal {
                    message: format!(
                        "plugin error code {} message {}",
                        envelope.code, envelope.message
                    ),
                }),
            }
        })
    }

    fn stream<'a>(
        &'a self,
        _req: CompletionRequest,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<CompletionStream, LlmError>> + 'a>> {
        // Task 16 wires the stream router; until then, return Internal
        // so callers that try to stream from an IPC-backed backend see
        // a typed failure rather than panicking.
        Box::pin(async {
            Err(LlmError::Internal {
                message: "IpcLlmBackend::stream is wired in Task 16".to_string(),
            })
        })
    }
}
