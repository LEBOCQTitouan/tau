//! Host-side [`crate::builder::DynSandbox`] implementation backed by a
//! spawned plugin subprocess.
//!
//! **PROVISIONAL** — mirrors [`tau_ports::Sandbox`]'s provisional
//! status. v0.1 ships the loader for symmetry across the four ports;
//! the kernel doesn't route through `Sandbox::create` from the run
//! loop yet (per spec §1.1). Mock-peer tests cover the dispatch
//! plumbing.
//!
//! Wire shape (spec §7.4): `sandbox.run` carries `[SandboxPlan]` as
//! params and returns `()` (the v0.1 `Sandbox::create` returns `()`
//! because the dyn-compatible `DynSandbox` restricts to `Handle = ()`,
//! see `crate::builder::DynSandbox`).

use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use tau_plugin_protocol::Frame;
use tau_ports::{SandboxError, SandboxPlan};
use tokio::sync::oneshot;

use crate::builder::DynSandbox;

use super::process::{PluginProcess, RpcResult};

/// Wire method name for the sandbox creation call.
const SANDBOX_RUN_METHOD: &str = "sandbox.run";

/// IPC-backed [`DynSandbox`] adapter.
///
/// Public for the same `__internals` test-export reasons as
/// [`super::ipc_llm::IpcLlmBackend`].
pub struct IpcSandbox {
    pub(crate) name: String,
    pub(crate) process: Arc<PluginProcess>,
}

impl IpcSandbox {
    /// Construct an `IpcSandbox` from a plugin name and a shared
    /// [`PluginProcess`].
    pub fn new(name: String, process: Arc<PluginProcess>) -> Self {
        Self { name, process }
    }
}

impl DynSandbox for IpcSandbox {
    fn name(&self) -> &str {
        &self.name
    }

    fn create<'a>(
        &'a self,
        plan: SandboxPlan,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), SandboxError>> + 'a>> {
        let process = self.process.clone();
        Box::pin(async move {
            let id = process.next_msgid.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel::<RpcResult>();
            {
                let mut map = process.in_flight_responses.lock().await;
                map.insert(id, tx);
            }
            // Wire shape: 1-element array `[SandboxPlan]`, matching
            // the LlmBackend convention.
            let params_bytes =
                rmp_serde::to_vec(&vec![&plan]).map_err(|e| SandboxError::Internal {
                    message: format!("rmp encode SandboxPlan: {e}"),
                })?;
            let frame = Frame::Request {
                id,
                method: SANDBOX_RUN_METHOD.to_string(),
                params: params_bytes,
            };
            let frame_bytes = frame.encode().map_err(|e| SandboxError::Internal {
                message: format!("frame encode: {e}"),
            })?;
            {
                let mut writer = process.writer.lock().await;
                writer
                    .write_frame(&frame_bytes)
                    .await
                    .map_err(|e| SandboxError::Internal {
                        message: format!("write frame: {e}"),
                    })?;
            }
            let result = rx.await.map_err(|_| SandboxError::Internal {
                message: "in-flight response sender dropped (plugin crashed?)".to_string(),
            })?;
            match result {
                Ok(bytes) => {
                    if bytes.is_empty() {
                        Ok(())
                    } else {
                        rmp_serde::from_slice::<()>(&bytes).map_err(|e| SandboxError::Internal {
                            message: format!("rmp decode sandbox.run response: {e}"),
                        })
                    }
                }
                Err(envelope) => Err(SandboxError::Internal {
                    message: format!(
                        "plugin error code {} message {}",
                        envelope.code, envelope.message
                    ),
                }),
            }
        })
    }
}
