#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! `fs-read` Tool plugin internals.
//!
//! The binary entrypoint at `src/main.rs` calls
//! `tau_plugin_sdk::run_tool_with_config::<FsReadPlugin>(...)`.
//!
//! See `docs/superpowers/specs/2026-04-29-tool-plugins-design.md`
//! for the design rationale.

pub mod config;
pub(crate) mod path_check;
pub mod plugin;
