//! Re-exports the project-config module that now lives in `tau-pkg`.
//!
//! This shim preserves the existing `tau_cli::config::*` import paths
//! for downstream code (integration tests, future external consumers).
//! New code SHOULD use `tau_pkg::project::*` directly.
//!
//! See ADR-0029 (or its successor) for the refactor motivation.

pub use tau_pkg::project::{
    agent, build_agent_definition, project, AgentEntry, AgentResolutionError, ProjectConfig,
    ProjectConfigError, PromptEntry, RequiresEntry,
};
