//! Project-level `tau.toml` parser and validator. Distinct from the
//! package-level `tau.toml` defined in tau-domain (per ADR-0002); see
//! ADR-0007 for the Cargo `[package]` vs `[workspace]` precedent that
//! justifies the shared filename.
//!
//! Per spec §3.2, §3.4, §4.1.

pub mod project;

pub use project::{AgentEntry, ProjectConfig, ProjectConfigError, PromptEntry, RequiresEntry};
