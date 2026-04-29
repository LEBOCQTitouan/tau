#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Anthropic Claude (Messages API) plugin internals.
//!
//! The binary entrypoint at `src/main.rs` calls
//! `tau_plugin_sdk::run_llm_backend_with_config::<AnthropicPlugin>(...)`.
//! Modules below populate as Tasks 3 — 9 land.
//!
//! See `docs/superpowers/specs/2026-04-29-anthropic-plugin-design.md`
//! for the design rationale.

// Modules populate progressively across Tasks 3-9. For Task 1 the
// crate just compiles to an empty library + stub binary.
