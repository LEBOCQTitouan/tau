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

pub(crate) mod client;
pub mod config;
pub(crate) mod error;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod stream;
