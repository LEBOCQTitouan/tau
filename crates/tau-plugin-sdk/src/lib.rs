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

// Modules and re-exports populate as Tasks 7 — 10 land.
