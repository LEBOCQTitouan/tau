//! Shared helpers for resolve-then-install flows used by
//! `tau run`, `tau chat`, and `tau resolve`.
//!
//! Centralizes the npm-style progress output (status + json events) +
//! the install loop. Each consumer is responsible for assembling the
//! `Vec<(AgentId, RequiredTool)>` input — single-agent (run/chat) or
//! all-agents (resolve).

use std::str::FromStr;

use anyhow::Context as _;

use crate::config::AgentEntry;
use crate::output::Output;

/// Resolve + (optionally) install requires.tools for one agent.
///
/// Builds the `(AgentId, RequiredTool)` list from the agent's entry,
/// resolves via `tau_pkg::resolve_requires_tools`, emits npm-style
/// progress lines, and either installs the planned packages or
/// short-circuits with copy-pasteable hints when `no_install` is set.
pub(crate) fn resolve_and_install_for_agent(
    entry: &AgentEntry,
    scope: &tau_pkg::Scope,
    no_install: bool,
    output: &mut Output,
) -> anyhow::Result<()> {
    let agent_id = tau_domain::AgentId::from_str(&entry.id).expect("AgentId from validated entry");
    let requires: Vec<(tau_domain::AgentId, tau_pkg::RequiredTool)> = entry
        .requires
        .tools
        .iter()
        .map(|t| (agent_id.clone(), t.clone()))
        .collect();
    let plan = tau_pkg::resolve_requires_tools(&requires, scope)
        .with_context(|| format!("resolving requires.tools for agent {:?}", entry.id))?;

    emit_resolve_start(&plan, output)?;

    if no_install && !plan.installs.is_empty() {
        emit_no_install_hints(&plan, output)?;
        anyhow::bail!("--no-install set; {} tool(s) missing", plan.installs.len());
    }

    install_planned(&plan, scope, output)?;
    Ok(())
}

/// Resolve + (optionally) install requires.tools for ALL agents in the
/// project tau.toml. Used by `tau resolve` (Task 8).
#[allow(dead_code)] // wired up by Task 8
pub(crate) fn resolve_and_install_for_project(
    agents: impl IntoIterator<Item = AgentEntry>,
    scope: &tau_pkg::Scope,
    no_install: bool,
    dry_run: bool,
    output: &mut Output,
) -> anyhow::Result<()> {
    let mut requires: Vec<(tau_domain::AgentId, tau_pkg::RequiredTool)> = Vec::new();
    for agent in agents {
        let agent_id =
            tau_domain::AgentId::from_str(&agent.id).expect("AgentId from validated entry");
        for tool in &agent.requires.tools {
            requires.push((agent_id.clone(), tool.clone()));
        }
    }
    let plan = tau_pkg::resolve_requires_tools(&requires, scope)
        .with_context(|| "resolving requires.tools for project".to_string())?;

    emit_resolve_start(&plan, output)?;

    if dry_run {
        for install in &plan.installs {
            output.status(format!(
                "[plan] {} {} from {} (would install)",
                install.name, install.version, install.source
            ))?;
        }
        return Ok(());
    }

    if no_install && !plan.installs.is_empty() {
        emit_no_install_hints(&plan, output)?;
        anyhow::bail!("--no-install set; {} tool(s) missing", plan.installs.len());
    }

    install_planned(&plan, scope, output)?;
    Ok(())
}

fn emit_resolve_start(plan: &tau_pkg::ResolutionPlan, output: &mut Output) -> anyhow::Result<()> {
    output.status(format!(
        "[resolve] {} required tools — {} already installed, {} to fetch",
        plan.installs.len() + plan.reuses.len(),
        plan.reuses.len(),
        plan.installs.len(),
    ))?;
    output.json(&serde_json::json!({
        "event": "resolve_start",
        "required": plan.installs.len() + plan.reuses.len(),
        "installed": plan.reuses.len(),
        "to_fetch": plan.installs.len(),
    }))?;
    Ok(())
}

fn install_planned(
    plan: &tau_pkg::ResolutionPlan,
    scope: &tau_pkg::Scope,
    output: &mut Output,
) -> anyhow::Result<()> {
    let resolve_start = std::time::Instant::now();
    for install in &plan.installs {
        output.status(format!(
            "[install] {} {} from {}",
            install.name, install.version, install.source
        ))?;
        output.json(&serde_json::json!({
            "event": "install_start",
            "name": install.name.as_str(),
            "version": install.version.to_string(),
            "source": install.source.to_string(),
        }))?;
        let started = std::time::Instant::now();
        let _installed = tau_pkg::install_with_options(
            &install.source,
            scope,
            tau_pkg::InstallOptions::default(),
        )
        .with_context(|| format!("installing {}", install.name))?;
        let elapsed = started.elapsed().as_millis();
        output.status(format!(
            "[install] {} {} ({}ms)",
            install.name, install.version, elapsed
        ))?;
        output.json(&serde_json::json!({
            "event": "install_complete",
            "name": install.name.as_str(),
            "version": install.version.to_string(),
            "duration_ms": elapsed as u64,
        }))?;
    }
    let total_ms = resolve_start.elapsed().as_millis();
    if !plan.installs.is_empty() {
        output.status(format!("[resolve] done in {total_ms}ms"))?;
        output.json(&serde_json::json!({
            "event": "resolve_complete",
            "duration_ms": total_ms as u64,
        }))?;
    }
    Ok(())
}

fn emit_no_install_hints(
    plan: &tau_pkg::ResolutionPlan,
    output: &mut Output,
) -> anyhow::Result<()> {
    output.warn(format!(
        "tau: {} tool(s) missing; --no-install set. To install:",
        plan.installs.len(),
    ))?;
    for install in &plan.installs {
        output.warn(format!("  tau install {}", install.source))?;
    }
    Ok(())
}
