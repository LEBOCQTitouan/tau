//! Host-side [`crate::builder::DynTool`] implementation backed by a
//! spawned plugin subprocess.
//!
//! The kernel's run loop drives `init` → `invoke` → `teardown` per
//! tool_use; on the IPC side, the SDK's tool runner collapses that
//! into a single `tool.call` RPC carrying `(SessionContext, Value)`
//! and returning a [`tau_ports::ToolResult`]. This adapter therefore
//! treats `init` / `teardown` as no-ops, builds the
//! `(SessionContext, Value)` tuple in `invoke`, and dispatches one RPC
//! per call. Identical to the SDK side at
//! `tau_plugin_sdk::runners::tool::dispatch_tool`'s wire shape.
//!
//! `name`/`schema`/`capabilities` are cached at construction time:
//! `name` from the plugin manifest, `schema` from the `tool.describe`
//! RPC the host issues during loading, and `capabilities` is empty at
//! v0.1 because the host can't introspect plugin capabilities over
//! the wire (additive in Phase 1).
//!
//! See spec §7.4. Streaming tool output is out of scope for v0.1.

use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use tau_domain::Value;
use tau_plugin_protocol::Frame;
use tau_ports::{SessionContext, ToolError, ToolResult, ToolSpec};
use tokio::sync::oneshot;

use crate::builder::DynTool;

use super::process::{PluginProcess, RpcResult};

/// Wire method names for the tool port.
const TOOL_CALL_METHOD: &str = "tool.call";
const TOOL_DESCRIBE_METHOD: &str = "tool.describe";

/// IPC-backed [`DynTool`] adapter.
///
/// `schema` is captured up-front via a `tool.describe` RPC during
/// loading so the kernel's tool-spec broadcast (in
/// `CompletionRequest.tools`) doesn't pay an RPC round-trip per turn.
///
/// Public for the same `__internals` test-export reasons as
/// [`super::ipc_llm::IpcLlmBackend`].
pub struct IpcTool {
    pub(crate) name: String,
    pub(crate) schema: ToolSpec,
    /// Empty at v0.1 — the wire protocol doesn't yet surface plugin-
    /// declared capabilities. The kernel's capability filter therefore
    /// trivially admits every IPC-backed tool; agent-package grants
    /// are still enforced for in-process tools.
    pub(crate) capabilities: Vec<tau_domain::Capability>,
    pub(crate) process: Arc<PluginProcess>,
}

impl IpcTool {
    /// Construct an `IpcTool` from a plugin name, a pre-fetched
    /// [`ToolSpec`], and a shared [`PluginProcess`].
    pub fn new(name: String, schema: ToolSpec, process: Arc<PluginProcess>) -> Self {
        Self {
            name,
            schema,
            capabilities: Vec::new(),
            process,
        }
    }

    /// Issue a `tool.describe` RPC and decode the [`ToolSpec`] response.
    /// Used by [`crate::plugin_host::load_tool`] during loading so the
    /// returned `IpcTool` has its schema cached.
    pub async fn fetch_schema(process: &PluginProcess) -> Result<ToolSpec, ToolError> {
        let id = process.next_msgid.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel::<RpcResult>();
        {
            let mut map = process.in_flight_responses.lock().await;
            map.insert(id, tx);
        }
        // Wire shape: params is a 0-element array, per the SDK side at
        // `tau_plugin_sdk::runners::tool::dispatch_tool`.
        let params_bytes =
            rmp_serde::to_vec::<Vec<()>>(&Vec::new()).map_err(|e| ToolError::Internal {
                message: format!("rmp encode tool.describe params: {e}"),
            })?;
        let frame = Frame::Request {
            id,
            method: TOOL_DESCRIBE_METHOD.to_string(),
            params: params_bytes,
        };
        let frame_bytes = frame.encode().map_err(|e| ToolError::Internal {
            message: format!("frame encode: {e}"),
        })?;
        process
            .send_frame(&frame_bytes)
            .await
            .map_err(|e| ToolError::Internal {
                message: format!("write frame: {e}"),
            })?;
        let result = rx.await.map_err(|_| ToolError::Internal {
            message: "in-flight response sender dropped (plugin crashed?)".to_string(),
        })?;
        match result {
            Ok(bytes) => {
                rmp_serde::from_slice::<ToolSpec>(&bytes).map_err(|e| ToolError::Internal {
                    message: format!("rmp decode ToolSpec: {e}"),
                })
            }
            Err(envelope) => Err(ToolError::Internal {
                message: format!(
                    "plugin error code {} message {}",
                    envelope.code, envelope.message
                ),
            }),
        }
    }
}

impl DynTool for IpcTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn schema(&self) -> ToolSpec {
        self.schema.clone()
    }

    fn capabilities(&self) -> &[tau_domain::Capability] {
        &self.capabilities
    }

    fn init<'a>(
        &'a self,
        _ctx: SessionContext,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), ToolError>> + 'a>> {
        // The SDK's tool runner runs init+invoke+teardown inside a
        // single `tool.call` dispatch; the host-side `init` is a no-op
        // (the SessionContext is forwarded as part of the `invoke`
        // RPC).
        Box::pin(async { Ok(()) })
    }

    fn invoke<'a>(
        &'a self,
        _session: &'a mut (),
        args: Value,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + 'a>> {
        let process = self.process.clone();
        Box::pin(async move {
            let id = process.next_msgid.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel::<RpcResult>();
            {
                let mut map = process.in_flight_responses.lock().await;
                map.insert(id, tx);
            }
            // Wire shape: params is `(SessionContext, Value)`. The
            // host-side `invoke` doesn't have access to the kernel's
            // SessionContext (the v0.1 `DynTool` shape passes `&mut ()`
            // not the context), so we synthesize a fresh one here. The
            // plugin side uses it for tracing fields only — the actual
            // session lifetime is collapsed inside the SDK runner.
            let ctx = SessionContext::new(
                tau_domain::AgentInstanceId::new(),
                uuid::Uuid::new_v4(),
                None,
            );
            let params_bytes =
                rmp_serde::to_vec(&(ctx, &args)).map_err(|e| ToolError::Internal {
                    message: format!("rmp encode tool.call params: {e}"),
                })?;
            let frame = Frame::Request {
                id,
                method: TOOL_CALL_METHOD.to_string(),
                params: params_bytes,
            };
            let frame_bytes = frame.encode().map_err(|e| ToolError::Internal {
                message: format!("frame encode: {e}"),
            })?;
            process
                .send_frame(&frame_bytes)
                .await
                .map_err(|e| ToolError::Internal {
                    message: format!("write frame: {e}"),
                })?;
            let result = rx.await.map_err(|_| ToolError::Internal {
                message: "in-flight response sender dropped (plugin crashed?)".to_string(),
            })?;
            match result {
                Ok(bytes) => {
                    rmp_serde::from_slice::<ToolResult>(&bytes).map_err(|e| ToolError::Internal {
                        message: format!("rmp decode ToolResult: {e}"),
                    })
                }
                Err(envelope) => Err(ToolError::Internal {
                    message: format!(
                        "plugin error code {} message {}",
                        envelope.code, envelope.message
                    ),
                }),
            }
        })
    }

    fn teardown<'a>(
        &'a self,
        _session: (),
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), ToolError>> + 'a>> {
        // Symmetric with `init` — see the comment there.
        Box::pin(async { Ok(()) })
    }
}
