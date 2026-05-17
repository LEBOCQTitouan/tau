//! Project + agent resolution wrapper for serve mode.
//!
//! Thin orchestration over `tau_pkg::project::*`. Loads a project's
//! `tau.toml`, holds the parsed `ProjectConfig`, and resolves
//! per-call agent ids to `(AgentDefinition, PackageManifest)` pairs
//! via `tau_pkg::project::build_agent_definition`.
//!
//! See ADR-0031 spec §4 and the post-refactor note in the plan.
//!
//! ## Reconciliation note (post-PR #133)
//!
//! The plan assumed `Scope::default_for_cwd` and
//! `AgentResolutionError::AgentNotFound`; neither exists in the actual
//! tau-pkg API. Adaptations made:
//!
//! - `Scope::resolve(root)` is used instead (walk-up from project root,
//!   no side-effecting directory creation).
//! - Unknown-agent detection is done in [`Project::resolve`] by returning
//!   an `anyhow::Error` wrapping a plain message; the dispatcher (Task 10)
//!   pattern-matches the error string for `-32010 Unknown agent` mapping.

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use tau_domain::{AgentDefinition, PackageManifest};
use tau_pkg::project::{build_agent_definition, AgentResolutionError, ProjectConfig};
use tau_pkg::Scope;

/// Loaded project state held by a serve-mode process for its lifetime.
///
/// Built once at startup from `--project <path>` (see Task 12).
/// Read-only thereafter. `Arc<Project>` is shared across concurrent
/// dispatcher tasks.
#[derive(Debug, Clone)]
pub struct Project {
    /// Canonical project root (the directory containing tau.toml).
    pub root: PathBuf,
    /// Scope used for agent resolution. Captured at load time.
    pub scope: Scope,
    /// Parsed tau.toml.
    pub config: ProjectConfig,
}

impl Project {
    /// Load a project from disk.
    ///
    /// Reads + parses `tau.toml`, captures the active `Scope` via
    /// `Scope::resolve` (walk-up from `root`). Does not resolve agents
    /// (that happens lazily per-call via [`Project::resolve`]).
    pub async fn load(root: &Path) -> Result<Self> {
        let root = std::fs::canonicalize(root)
            .with_context(|| format!("canonicalize project root {}", root.display()))?;
        let config = ProjectConfig::from_path(root.join("tau.toml"))
            .with_context(|| format!("parse tau.toml at {}", root.display()))?;
        let scope = Scope::resolve(&root).with_context(|| "compute scope for project root")?;
        Ok(Self { root, scope, config })
    }

    /// Resolve an agent id to `(AgentDefinition, PackageManifest)`.
    ///
    /// Returns `Err` on:
    /// - Unknown agent id: error message begins with `"agent not found: "`.
    ///   The dispatcher (Task 10) maps this to JSON-RPC `-32010 Unknown agent`.
    /// - Package not installed / version not satisfied / manifest invalid:
    ///   flows through the standard `AgentResolutionError` path.
    pub fn resolve(
        &self,
        agent_id: &str,
    ) -> Result<(AgentDefinition, PackageManifest)> {
        let entry = self
            .config
            .agents
            .get(agent_id)
            .ok_or_else(|| {
                anyhow!(
                    "agent not found: {:?} is not defined in {}",
                    agent_id,
                    self.root.display()
                )
            })?;
        build_agent_definition(entry, &self.root, &self.scope)
            .map_err(|e: AgentResolutionError| anyhow!(e))
    }

    /// List all agent ids defined in the project's `tau.toml`.
    pub fn agent_ids(&self) -> Vec<String> {
        self.config.agents.keys().cloned().collect()
    }
}
