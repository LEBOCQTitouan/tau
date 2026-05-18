//! `tau check` — pre-flight validation aggregator.
//!
//! See spec at `docs/superpowers/specs/2026-05-18-tau-check-design.md`.
//!
//! Bare `tau check` runs all 6 categories; subcommands run one each.
//! Output: human (default), `--json` (JSONL), `--sarif` (SARIF 2.1.0).
//! Exit codes: 0 clean / 2 fixable / 3 needs-setup / 64 usage / 70 internal.

mod result;
mod runner;

pub use result::{
    compute_exit, CheckCategory, CheckFinding, CheckResult, CheckStatus, FindingLocation, Severity,
};

use anyhow::Result;

/// Entry point for `tau check`. Parses CheckArgs, builds CheckCtx,
/// selects category list, runs the orchestrator, dispatches output
/// (Tasks 10-12 wire renderers), and exits with the appropriate code.
pub async fn run(args: crate::cli::CheckArgs) -> Result<()> {
    // Resolve project root.
    let project_root = match args.project.as_ref() {
        Some(p) => p.clone(),
        None => std::env::current_dir()?,
    };

    let ctx = runner::CheckCtx::load(project_root, args.fast).await?;

    // Determine category list.
    let categories: Vec<CheckCategory> = match args.category.as_deref() {
        None => CheckCategory::ALL.to_vec(),
        Some("config") => vec![CheckCategory::Config],
        Some("lockfile") => vec![CheckCategory::Lockfile],
        Some("packages") => vec![CheckCategory::Packages],
        Some("sandbox") => vec![CheckCategory::Sandbox],
        Some("plugins") => vec![CheckCategory::Plugins],
        Some("skills") => vec![CheckCategory::Skills],
        Some(other) => anyhow::bail!("unknown check category: {other}"),
    };

    let results = runner::run_categories(&ctx, &categories).await;

    // Stub output — Tasks 10-12 replace this with real renderers.
    eprintln!("tau check: {} categories", results.len());
    for r in &results {
        eprintln!("  {} → {:?}", r.category.name(), r.status);
    }

    let code = compute_exit(&results);
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}
