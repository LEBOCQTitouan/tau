#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Plugin author SDK for tau.
//!
//! Plugin authors implement the same `tau_ports::*` traits the in-process
//! tau runtime kernel uses, then call one of the per-port generic runner
//! functions (`run_llm_backend`, `run_tool`, `run_storage`, `run_sandbox`)
//! from their `#[tokio::main]` entry point. This crate contains the
//! tracing layer, handshake response builder, dispatch loop, and
//! streaming helper.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-loading-design.md` §5
//! and ADR-0008 for the design rationale.

pub mod configure;
pub mod error;
pub mod handshake;
pub mod runners;
pub mod streaming;
pub mod tracing_layer;

pub use configure::{ConfigError, Configure};
pub use error::SdkError;
pub use handshake::{drive_handshake, PluginMeta};
pub use runners::{
    run_llm_backend, run_llm_backend_with_config, run_llm_backend_with_config_with_io,
    run_llm_backend_with_io, run_sandbox, run_sandbox_with_config, run_sandbox_with_config_with_io,
    run_sandbox_with_io, run_storage, run_storage_with_config, run_storage_with_config_with_io,
    run_storage_with_io, run_tool, run_tool_with_config, run_tool_with_config_with_io,
    run_tool_with_io,
};
pub use streaming::stream_completion;

// Re-export framer types from tau-plugin-protocol so plugin authors
// have one obvious crate to depend on.
pub use tau_plugin_protocol::{FramedReader, FramedWriter, FramerOptions};
