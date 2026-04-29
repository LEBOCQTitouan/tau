#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! OpenAI (Chat Completions API) plugin internals.
//!
//! The binary entrypoint at `src/main.rs` calls
//! `tau_plugin_sdk::run_llm_backend_with_config::<OpenAIPlugin>(...)`.
//!
//! See `docs/superpowers/specs/2026-04-29-openai-plugin-design.md`
//! for the design rationale.

pub mod config;
pub(crate) mod request;
