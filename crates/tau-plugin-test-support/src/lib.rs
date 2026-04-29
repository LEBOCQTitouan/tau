#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Shared test-support code for tau LLM-backend plugins.
//!
//! See `docs/superpowers/specs/2026-04-29-openai-plugin-design.md`
//! §3.1 + §9.1 for design rationale (rule-of-three refactor of the
//! cassette replayer that originated in the anthropic plugin).

pub mod cassette;
