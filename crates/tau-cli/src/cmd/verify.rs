//! `tau verify` — recompute install-tree hashes, compare to lockfile.
//!
//! Per spec §3:
//!
//! - Resolves the active [`Scope`] (project or global).
//! - Parses the optional package name and version.
//! - Delegates to [`tau_pkg::verify()`] (single version) or
//!   [`tau_pkg::verify_all`] (all installed packages).
//! - Prints either a human-readable summary (§3.3) or, when `--json`
//!   is set, per-line JSON events (§3.4).
//!
//! Exit codes per ADR-0007 §7:
//! - 0: all packages `Ok` or `Unverified` (no drift detected).
//! - 2: any `TreeDrift`, `BinaryDrift`, or `Missing` status.
//!
//! Orphan detection (install dirs not in the lockfile) is skipped in
//! v0.1; the lockfile is the source of truth and orphan cleanup is a
//! separate concern. This can be added in a future iteration.

use std::str::FromStr;

use semver::Version;
use tau_domain::PackageName;
use tau_pkg::{
    verify, verify_all_with_options, AnthropicConformanceIssue, LockFile, Scope, VerifyReport,
    VerifyStatus,
};

use crate::cli::VerifyArgs;
use crate::output::Output;

/// Run `tau verify`.
pub async fn run(args: &VerifyArgs, output: &mut Output) -> anyhow::Result<()> {
    // 1. Resolve scope.
    let scope = if args.global {
        Scope::global()?
    } else {
        let cwd = std::env::current_dir()?;
        Scope::resolve(&cwd)?
    };

    // 2. Collect reports.
    let reports: Vec<VerifyReport> = match &args.package {
        None => {
            // No package filter — verify everything in the lockfile.
            // If the lockfile doesn't exist yet, treat as empty (0 packages).
            if !scope.lockfile_path().exists() {
                vec![]
            } else {
                verify_all_with_options(&scope, args.anthropic_strict)
                    .map_err(|e| anyhow::anyhow!("{}", e))?
            }
        }
        Some(pkg_str) => {
            let name = PackageName::from_str(pkg_str)
                .map_err(|e| anyhow::anyhow!("invalid package name {:?}: {}", pkg_str, e))?;

            match &args.version {
                Some(v_str) => {
                    // Single (pkg, version) pair.
                    let version = Version::parse(v_str)
                        .map_err(|e| anyhow::anyhow!("invalid version {:?}: {}", v_str, e))?;
                    let report =
                        verify(&scope, &name, &version).map_err(|e| anyhow::anyhow!("{}", e))?;
                    vec![report]
                }
                None => {
                    // All versions of the named package.
                    let lockfile = LockFile::load(&scope.lockfile_path())
                        .map_err(|e| anyhow::anyhow!("loading lockfile: {}", e))?;
                    let pkg = lockfile
                        .find(&name)
                        .ok_or_else(|| anyhow::anyhow!("package {:?} not installed", pkg_str))?;
                    let mut reports = Vec::new();
                    for lv in &pkg.installed_versions {
                        let report = verify(&scope, &name, &lv.version)
                            .map_err(|e| anyhow::anyhow!("{}", e))?;
                        reports.push(report);
                    }
                    reports
                }
            }
        }
    };

    let total = reports.len();

    // 3. JSON: emit verify_started.
    if output.is_json() {
        output.json(&serde_json::json!({
            "event": "verify_started",
            "total": total,
        }))?;
    }

    // 4. Emit per-package events and track drift.
    let mut ok_count: usize = 0;
    let mut drift_count: usize = 0;
    let mut unverified_count: usize = 0;

    for report in &reports {
        if output.is_json() {
            emit_json_event(report, output)?;
        } else {
            emit_human_line(report, output)?;
        }

        match &report.status {
            VerifyStatus::Ok => ok_count += 1,
            VerifyStatus::Unverified => unverified_count += 1,
            VerifyStatus::TreeDrift { .. }
            | VerifyStatus::BinaryDrift { .. }
            | VerifyStatus::Missing { .. }
            | VerifyStatus::SkillContentDrift { .. }
            | VerifyStatus::AnthropicConformance { .. } => drift_count += 1,
            // The enum is #[non_exhaustive] — any future variant is
            // treated conservatively as non-drift to avoid false exits.
            _ => unverified_count += 1,
        }
    }

    // 5. Summary line / JSON completed event.
    if output.is_json() {
        output.json(&serde_json::json!({
            "event": "verify_completed",
            "total": total,
            "ok": ok_count,
            "drift": drift_count,
            "unverified": unverified_count,
        }))?;
    } else {
        output.human(&format!(
            "\n{} package{} verified, {} drifted.",
            total,
            if total == 1 { "" } else { "s" },
            drift_count,
        ))?;
    }

    // 6. Exit 2 if any drift detected.
    if drift_count > 0 {
        return Err(anyhow::anyhow!(
            "{} package{} drifted",
            drift_count,
            if drift_count == 1 { "" } else { "s" }
        ));
    }

    Ok(())
}

/// Emit a single per-package JSON event.
fn emit_json_event(report: &VerifyReport, output: &mut Output) -> anyhow::Result<()> {
    let name = report.name.as_str();
    let version = report.version.to_string();
    let event = match &report.status {
        VerifyStatus::Ok => {
            serde_json::json!({
                "event": "verify_package",
                "name": name,
                "version": version,
                "status": "ok",
            })
        }
        VerifyStatus::Unverified => {
            serde_json::json!({
                "event": "verify_package",
                "name": name,
                "version": version,
                "status": "unverified",
            })
        }
        VerifyStatus::TreeDrift { expected, actual } => {
            serde_json::json!({
                "event": "verify_package",
                "name": name,
                "version": version,
                "status": "drift",
                "kind": "tree",
                "expected": expected,
                "actual": actual,
            })
        }
        VerifyStatus::BinaryDrift {
            path,
            expected,
            actual,
        } => {
            serde_json::json!({
                "event": "verify_package",
                "name": name,
                "version": version,
                "status": "drift",
                "kind": "binary",
                "path": path.to_string_lossy(),
                "expected": expected,
                "actual": actual,
            })
        }
        VerifyStatus::Missing { path } => {
            serde_json::json!({
                "event": "verify_package",
                "name": name,
                "version": version,
                "status": "drift",
                "kind": "missing",
                "path": path.to_string_lossy(),
            })
        }
        VerifyStatus::SkillContentDrift {
            name: skill_name,
            expected,
            got,
        } => {
            serde_json::json!({
                "event": "verify_package",
                "name": name,
                "version": version,
                "status": "drift",
                "kind": "skill_content",
                "skill_name": skill_name,
                "expected": expected,
                "actual": got,
            })
        }
        VerifyStatus::AnthropicConformance { skill_name, issue } => {
            let (issue_kind, detail) = match issue {
                AnthropicConformanceIssue::MissingDescription => ("missing_description", None),
                AnthropicConformanceIssue::EmptyBody => ("empty_body", None),
                AnthropicConformanceIssue::MalformedFrontmatter { detail } => {
                    ("malformed_frontmatter", Some(detail.as_str()))
                }
                _ => ("unknown_issue", None),
            };
            if let Some(d) = detail {
                serde_json::json!({
                    "event": "verify_package",
                    "name": name,
                    "version": version,
                    "status": "drift",
                    "kind": "anthropic_conformance",
                    "skill_name": skill_name,
                    "issue": issue_kind,
                    "detail": d,
                })
            } else {
                serde_json::json!({
                    "event": "verify_package",
                    "name": name,
                    "version": version,
                    "status": "drift",
                    "kind": "anthropic_conformance",
                    "skill_name": skill_name,
                    "issue": issue_kind,
                })
            }
        }
        // Future variants: emit as unverified.
        _ => {
            serde_json::json!({
                "event": "verify_package",
                "name": name,
                "version": version,
                "status": "unverified",
            })
        }
    };
    output.json(&event)?;
    Ok(())
}

/// Emit a human-readable line for one package verification result.
///
/// Per spec §3.3:
/// ```text
/// verify <pkg>@1.0.0... ok
/// verify <other>@2.1.0... ✗ drift (tree)
///   expected: abc123...
///   actual:   xyz789...
/// verify <plugin>@1.2.0... ✗ drift (binary)
///   path: ...
///   expected: def...
///   actual:   ghi...
/// verify <missing>@1.0.0... ✗ drift (missing)
///   path: ...
/// verify <unverified>@1.0.0... (unverified — no checksum recorded)
/// ```
fn emit_human_line(report: &VerifyReport, output: &mut Output) -> anyhow::Result<()> {
    let prefix = format!("verify {}@{}... ", report.name.as_str(), report.version);
    match &report.status {
        VerifyStatus::Ok => {
            output.human(&format!("{}ok", prefix))?;
        }
        VerifyStatus::Unverified => {
            output.human(&format!(
                "{}(unverified \u{2014} no checksum recorded)",
                prefix
            ))?;
        }
        VerifyStatus::TreeDrift { expected, actual } => {
            output.human(&format!("{}\u{2717} drift (tree)", prefix))?;
            output.human(&format!("  expected: {}", expected))?;
            output.human(&format!("  actual:   {}", actual))?;
        }
        VerifyStatus::BinaryDrift {
            path,
            expected,
            actual,
        } => {
            output.human(&format!("{}\u{2717} drift (binary)", prefix))?;
            output.human(&format!("  path: {}", path.display()))?;
            output.human(&format!("  expected: {}", expected))?;
            output.human(&format!("  actual:   {}", actual))?;
        }
        VerifyStatus::Missing { path } => {
            output.human(&format!("{}\u{2717} drift (missing)", prefix))?;
            output.human(&format!("  path: {}", path.display()))?;
        }
        VerifyStatus::SkillContentDrift {
            name: skill_name,
            expected,
            got,
        } => {
            output.human(&format!("{}\u{2717} drift (skill content)", prefix))?;
            output.human(&format!("  skill: {}", skill_name))?;
            output.human(&format!("  expected: {}", expected))?;
            output.human(&format!("  actual:   {}", got))?;
        }
        VerifyStatus::AnthropicConformance { skill_name, issue } => {
            output.human(&format!(
                "{}\u{2717} AnthropicConformance (skill: {})",
                prefix, skill_name
            ))?;
            match issue {
                AnthropicConformanceIssue::MissingDescription => {
                    output.human("  issue: description field is missing or empty")?;
                }
                AnthropicConformanceIssue::EmptyBody => {
                    output.human("  issue: SKILL.md body is empty or whitespace-only")?;
                }
                AnthropicConformanceIssue::MalformedFrontmatter { detail } => {
                    output.human(&format!("  issue: malformed frontmatter — {}", detail))?;
                }
                _ => {
                    output.human("  issue: unknown conformance issue")?;
                }
            }
        }
        // Future variants: print as unverified.
        _ => {
            output.human(&format!("{}(unverified \u{2014} unknown status)", prefix))?;
        }
    }
    Ok(())
}
