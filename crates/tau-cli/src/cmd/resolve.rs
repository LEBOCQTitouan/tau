//! `tau resolve` — install missing requires.tools dependencies for
//! ALL agents in the project tau.toml.
//!
//! Lazy `tau run` / `tau chat` perform the same resolve per-agent at
//! invocation time; this verb is the project-wide form for CI cache
//! warm-up, pre-flight validation, and "fix my deps now" workflows.
//!
//! See `docs/superpowers/specs/2026-04-30-transitive-deps-design.md` §7.2.

use anyhow::Context as _;

use crate::cli::ResolveArgs;
use crate::cmd::resolve_helpers;
use crate::config::{ProjectConfig, ProjectConfigError};
use crate::output::Output;

/// Run `tau resolve`.
pub async fn run(args: &ResolveArgs, output: &mut Output) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let path = cwd.join("tau.toml");
    let config = match ProjectConfig::from_path(&path) {
        Ok(cfg) => cfg,
        Err(ProjectConfigError::NotFound) => {
            anyhow::bail!("no project tau.toml found at {path:?}; run `tau init` to create one");
        }
        Err(e) => return Err(e.into()),
    };

    let scope = tau_pkg::Scope::resolve(&cwd).context("resolving package scope")?;

    resolve_helpers::resolve_and_install_for_project(
        config.agents.into_values(),
        &scope,
        args.no_install,
        args.dry_run,
        output,
    )?;
    Ok(())
}
