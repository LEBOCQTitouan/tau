#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Ollama (local LLM runner) plugin internals.
//!
//! The binary entrypoint at `src/main.rs` calls
//! `tau_plugin_sdk::run_llm_backend_with_config::<OllamaPlugin>(...)`.
//!
//! See `docs/superpowers/specs/2026-04-29-ollama-plugin-design.md`
//! for the design rationale.

pub mod config;
