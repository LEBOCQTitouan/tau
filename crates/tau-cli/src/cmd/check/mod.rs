//! `tau check` — pre-flight validation aggregator.
//!
//! See spec at `docs/superpowers/specs/2026-05-18-tau-check-design.md`.
//!
//! Bare `tau check` runs all 6 categories; subcommands run one each.
//! Output: human (default), `--json` (JSONL), `--sarif` (SARIF 2.1.0).
//! Exit codes: 0 clean / 2 fixable / 3 needs-setup / 64 usage / 70 internal.

mod result;

pub use result::{
    compute_exit, CheckCategory, CheckFinding, CheckResult, CheckStatus, FindingLocation, Severity,
};

use anyhow::Result;

/// Entry point for `tau check`. Stub until Task 14 wires the runner.
pub async fn run() -> Result<()> {
    eprintln!("tau check: not yet wired (Tasks 14+)");
    Ok(())
}
