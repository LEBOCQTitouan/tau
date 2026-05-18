//! `tau check skills` — cross-check each installed skill package's
//! manifest against its declared and live grants.
//!
//! Default (full): parse manifest + run `cross_check_skill_package`.
//! `--fast`: parse manifest only (no SKILL.md content validation).

use crate::cmd::check::result::{
    CheckCategory, CheckFinding, CheckResult, CheckStatus, FindingLocation, Severity,
};
use crate::cmd::check::runner::CheckCtx;
use serde_json::json;
use tau_pkg::LockFile;

pub fn run_skills(ctx: &CheckCtx) -> CheckResult {
    let lockfile_path = ctx.scope.lockfile_path();
    if !lockfile_path.exists() {
        return CheckResult {
            category: CheckCategory::Skills,
            status: CheckStatus::Skipped {
                reason: "no lockfile (no skills installed)".into(),
            },
            findings: Vec::new(),
            duration: std::time::Duration::ZERO,
        };
    }

    let lockfile = match LockFile::load(&lockfile_path) {
        Ok(l) => l,
        Err(e) => {
            return CheckResult {
                category: CheckCategory::Skills,
                status: CheckStatus::Failed,
                findings: vec![CheckFinding {
                    category: CheckCategory::Skills,
                    severity: Severity::Error,
                    rule_id: "tau.skills.lockfile_unreadable",
                    summary: format!("lockfile read failed: {e}"),
                    detail: None,
                    location: Some(FindingLocation {
                        path: lockfile_path.to_path_buf(),
                        line: None,
                        column: None,
                    }),
                    remediation: Some("inspect the lockfile or re-run `tau install`".into()),
                    structured: json!({"kind": "LockFileLoad"}),
                }],
                duration: std::time::Duration::ZERO,
            };
        }
    };

    let mut findings = Vec::new();
    let mut skills_seen = 0usize;

    // `skill: Option<LockedSkill>` is on `LockedPackage` (lockfile schema v5+).
    // Each package has at most one skill entry; `active_version` carries the
    // canonical version used to locate the install dir.
    for pkg in &lockfile.packages {
        let Some(_skill) = &pkg.skill else { continue };
        skills_seen += 1;

        let pkg_name = pkg.name.to_string();
        let pkg_version = pkg.active_version.to_string();

        // Locate the install directory: <packages_dir>/<name>/<active_version>
        let install_dir = ctx
            .scope
            .package_dir(&pkg.name, &pkg.active_version);

        if !install_dir.exists() {
            findings.push(CheckFinding {
                category: CheckCategory::Skills,
                severity: Severity::NeedsSetup,
                rule_id: "tau.skills.install_dir_missing",
                summary: format!(
                    "skill {}@{} install directory missing at {}",
                    pkg_name,
                    pkg_version,
                    install_dir.display()
                ),
                detail: None,
                location: Some(FindingLocation {
                    path: install_dir,
                    line: None,
                    column: None,
                }),
                remediation: Some("tau install --force".into()),
                structured: json!({
                    "package": pkg_name,
                    "version": pkg_version,
                }),
            });
            continue;
        }

        // Read and parse the manifest to verify it's still intact.
        let manifest_path = install_dir.join("tau.toml");
        let manifest = match tau_pkg::read_manifest(&manifest_path) {
            Ok(m) => m,
            Err(e) => {
                findings.push(CheckFinding {
                    category: CheckCategory::Skills,
                    severity: Severity::Error,
                    rule_id: "tau.skills.manifest_unreadable",
                    summary: format!(
                        "skill {}@{} manifest not readable at {}: {}",
                        pkg_name,
                        pkg_version,
                        manifest_path.display(),
                        e
                    ),
                    detail: None,
                    location: Some(FindingLocation {
                        path: manifest_path,
                        line: None,
                        column: None,
                    }),
                    remediation: Some("tau install".into()),
                    structured: json!({
                        "package": pkg_name,
                        "version": pkg_version,
                    }),
                });
                continue;
            }
        };

        // --fast: manifest-parse-only; skip SKILL.md content validation.
        if ctx.fast {
            continue;
        }

        // Full: run cross_check_skill_package (validates SKILL.md content,
        // frontmatter name match, and ${SKILL_DIR} reference lint).
        match tau_pkg::cross_check_skill_package(&install_dir, &manifest) {
            Ok(_) => {}
            Err(e) => {
                findings.push(CheckFinding {
                    category: CheckCategory::Skills,
                    severity: Severity::Error,
                    rule_id: "tau.skills.mismatch",
                    summary: format!(
                        "skill {}@{} cross-check failed: {}",
                        pkg_name, pkg_version, e
                    ),
                    detail: None,
                    location: Some(FindingLocation {
                        path: install_dir,
                        line: None,
                        column: None,
                    }),
                    remediation: Some(
                        "tau install --force or report the skill author".into(),
                    ),
                    structured: json!({
                        "package": pkg_name,
                        "version": pkg_version,
                        "kind": "CrossCheckError",
                    }),
                });
            }
        }
    }

    let status = if findings.is_empty() {
        if skills_seen == 0 {
            CheckStatus::Skipped {
                reason: "no skills installed".into(),
            }
        } else {
            CheckStatus::Ok
        }
    } else {
        CheckStatus::Failed
    };
    CheckResult {
        category: CheckCategory::Skills,
        status,
        findings,
        duration: std::time::Duration::ZERO,
    }
}
