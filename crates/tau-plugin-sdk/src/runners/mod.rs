//! Per-port plugin runners.
//!
//! Each runner drives the full plugin lifecycle for one
//! [`tau_domain::PortKind`]: install the SDK tracing layer, run the
//! handshake, dispatch incoming frames to the plugin's port-specific
//! methods, and exit cleanly on `meta.shutdown` or stdin EOF.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-loading-design.md` §5.2.

mod llm_backend;
mod storage;
mod tool;

pub use llm_backend::{
    run_llm_backend, run_llm_backend_with_config, run_llm_backend_with_config_with_io,
    run_llm_backend_with_io,
};
pub use storage::{
    run_storage, run_storage_with_config, run_storage_with_config_with_io, run_storage_with_io,
};
pub use tool::{run_tool, run_tool_with_config, run_tool_with_config_with_io, run_tool_with_io};
