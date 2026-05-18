//! `tau check packages` — every agent's `requires.tools` references
//! are present in the lockfile at a satisfying version.

use crate::cmd::check::result::{
    CheckCategory, CheckFinding, CheckResult, CheckStatus, FindingLocation, Severity,
};
use crate::cmd::check::runner::CheckCtx;
use serde_json::json;
use tau_pkg::LockFile;

pub fn run_packages(ctx: &CheckCtx) -> CheckResult {
    let Some(project) = &ctx.project else {
        return CheckResult {
            category: CheckCategory::Packages,
            status: CheckStatus::Skipped {
                reason: "tau.toml malformed (see config check)".into(),
            },
            findings: Vec::new(),
            duration: std::time::Duration::ZERO,
        };
    };

    let lockfile_path = ctx.scope.lockfile_path();
    let lockfile: Option<LockFile> = if lockfile_path.exists() {
        LockFile::load(&lockfile_path).ok()
    } else {
        None
    };

    let tau_toml = ctx.project_root.join("tau.toml");
    let location = Some(FindingLocation {
        path: tau_toml,
        line: None,
        column: None,
    });

    let mut findings = Vec::new();
    for (agent_id, agent) in &project.agents {
        for required in &agent.requires.tools {
            let satisfied = lockfile
                .as_ref()
                .map(|lf| is_satisfied(lf, &required.name.to_string(), &required.version_req))
                .unwrap_or(false);
            if !satisfied {
                findings.push(CheckFinding {
                    category: CheckCategory::Packages,
                    severity: Severity::NeedsSetup,
                    rule_id: "tau.packages.missing",
                    summary: format!(
                        "agent `{}` requires `{}` {} but it isn't installed at a satisfying version",
                        agent_id, required.name, required.version_req
                    ),
                    detail: None,
                    location: location.clone(),
                    remediation: Some("tau resolve".into()),
                    structured: json!({
                        "agent_id": agent_id,
                        "package": required.name.to_string(),
                        "version_req": required.version_req.to_string(),
                    }),
                });
            }
        }
    }

    let status = if findings.is_empty() {
        CheckStatus::Ok
    } else {
        CheckStatus::Failed
    };
    CheckResult {
        category: CheckCategory::Packages,
        status,
        findings,
        duration: std::time::Duration::ZERO,
    }
}

/// True if `lockfile` has an installed version of `name` satisfying `req`.
fn is_satisfied(lockfile: &LockFile, name: &str, req: &semver::VersionReq) -> bool {
    for pkg in &lockfile.packages {
        if pkg.name.to_string() != name {
            continue;
        }
        for v in &pkg.installed_versions {
            if req.matches(&v.version) {
                return true;
            }
        }
    }
    false
}
