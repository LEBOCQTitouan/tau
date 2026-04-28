//! `tau list` — list installed packages or project-declared agents.
//!
//! Per spec §3.13 list row.
//!
//! - `tau list` / `tau list packages` reads the per-scope lockfile via
//!   [`tau_pkg::list`] and prints one row per installed package.
//!   `--global` forces global scope, `--all` lists project + global
//!   side-by-side, otherwise scope is auto-resolved per cwd.
//! - `tau list agents` reads the project `tau.toml` via
//!   [`crate::config::ProjectConfig::from_path`] and prints one row per
//!   declared agent. Errors with exit 2 if no project tau.toml is found.
//! - `--dry-run` is rejected as nonsensical for a read-only command.
//!
//! Both modes support `--json` for scriptable output.

use serde::Serialize;

use crate::cli::{ListArgs, ListResource};
use crate::config::{ProjectConfig, ProjectConfigError};
use crate::output::Output;

/// Run `tau list`.
pub async fn run(args: &ListArgs, output: &mut Output) -> anyhow::Result<()> {
    if args.dry_run {
        anyhow::bail!("--dry-run is not supported on tau list (read-only command)");
    }

    match args.resource {
        ListResource::Packages => list_packages(args, output),
        ListResource::Agents => list_agents(output),
    }
}

/// Tabular row produced for one installed package.
#[derive(Debug, Serialize)]
struct PackageRow {
    name: String,
    version: String,
    source: String,
    scope: String,
    version_count: usize,
}

/// Tabular row produced for one declared agent.
#[derive(Debug, Serialize)]
struct AgentRow {
    id: String,
    display_name: String,
    package: String,
    llm_backend: String,
}

fn list_packages(args: &ListArgs, output: &mut Output) -> anyhow::Result<()> {
    use tau_pkg::{Scope, ScopeKind};

    let cwd = std::env::current_dir()?;

    let mut rows: Vec<PackageRow> = Vec::new();

    if args.all {
        // Project (if any) + global, side-by-side.
        // We can only detect a project scope by walking up from cwd; if the
        // resolved scope is global, there's no project to list.
        let resolved = Scope::resolve(&cwd)?;
        if matches!(resolved.kind(), ScopeKind::Project) {
            collect_packages(&resolved, "project", &mut rows)?;
        }
        let global_scope = Scope::global()?;
        collect_packages(&global_scope, "global", &mut rows)?;
    } else if args.global {
        let global_scope = Scope::global()?;
        collect_packages(&global_scope, "global", &mut rows)?;
    } else {
        let scope = Scope::resolve(&cwd)?;
        let label = scope_label(scope.kind());
        collect_packages(&scope, label, &mut rows)?;
    }

    if output.is_json() {
        output.json(&rows)?;
    } else if rows.is_empty() {
        output.human("(no packages installed)")?;
    } else {
        output.human(
            "name                version    source                                   scope",
        )?;
        output
            .human("--                  --         --                                       --")?;
        for r in &rows {
            output.human(&format!(
                "{:<20}{:<11}{:<41}{}",
                truncate(&r.name, 19),
                truncate(&r.version, 10),
                truncate(&r.source, 40),
                r.scope,
            ))?;
        }
    }

    Ok(())
}

fn collect_packages(
    scope: &tau_pkg::Scope,
    label: &str,
    rows: &mut Vec<PackageRow>,
) -> anyhow::Result<()> {
    let listing = tau_pkg::list(scope).map_err(|e| anyhow::anyhow!("listing packages: {e}"))?;
    for pkg in listing {
        rows.push(PackageRow {
            name: pkg.name.as_str().to_owned(),
            version: pkg.active_version.to_string(),
            source: pkg.source.to_string(),
            scope: label.to_owned(),
            version_count: pkg.installed_versions.len(),
        });
    }
    Ok(())
}

fn scope_label(kind: tau_pkg::ScopeKind) -> &'static str {
    match kind {
        tau_pkg::ScopeKind::Global => "global",
        tau_pkg::ScopeKind::Project => "project",
        // ScopeKind is `#[non_exhaustive]`; future-proof the match.
        _ => "unknown",
    }
}

/// Render `s` truncated to at most `n` characters, no wrapping. Tabular
/// rows use this so a long URL or version doesn't blow out the column.
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_owned()
    } else {
        let cut: String = s.chars().take(n.saturating_sub(1)).collect();
        format!("{cut}\u{2026}")
    }
}

fn list_agents(output: &mut Output) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let path = cwd.join("tau.toml");

    let config = match ProjectConfig::from_path(&path) {
        Ok(cfg) => cfg,
        Err(ProjectConfigError::NotFound) => {
            anyhow::bail!("no project tau.toml found at {path:?}; run `tau init` to create one");
        }
        Err(e) => return Err(e.into()),
    };

    let rows: Vec<AgentRow> = config
        .agents
        .values()
        .map(|a| AgentRow {
            id: a.id.clone(),
            display_name: a.display_name.clone(),
            package: a.package.clone(),
            llm_backend: a.llm_backend.clone(),
        })
        .collect();

    if output.is_json() {
        output.json(&rows)?;
    } else if rows.is_empty() {
        output.human("(no agents declared in project tau.toml)")?;
    } else {
        output.human(
            "id              display_name              package                  llm_backend",
        )?;
        output.human("--              --                        --                       --")?;
        for r in &rows {
            output.human(&format!(
                "{:<16}{:<26}{:<25}{}",
                truncate(&r.id, 15),
                truncate(&r.display_name, 25),
                truncate(&r.package, 24),
                r.llm_backend,
            ))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_label_handles_global_and_project() {
        assert_eq!(scope_label(tau_pkg::ScopeKind::Global), "global");
        assert_eq!(scope_label(tau_pkg::ScopeKind::Project), "project");
    }

    #[test]
    fn truncate_keeps_short_strings_unchanged() {
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn truncate_appends_ellipsis_when_too_long() {
        let out = truncate("abcdefghij", 5);
        assert_eq!(out.chars().count(), 5);
        assert!(out.ends_with('\u{2026}'));
    }
}
