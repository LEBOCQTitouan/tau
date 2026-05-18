//! `tau check lockfile` — recompute install-tree hashes vs lockfile.
//!
//! Wraps `tau_pkg::verify_all_with_options`. Each non-Ok `VerifyStatus`
//! becomes one finding with severity `Error` (drift is a real bug).

use crate::cmd::check::result::{
    CheckCategory, CheckFinding, CheckResult, CheckStatus, FindingLocation, Severity,
};
use crate::cmd::check::runner::CheckCtx;
use serde_json::json;
use tau_pkg::verify_all_with_options;

pub fn run_lockfile(ctx: &CheckCtx) -> CheckResult {
    let lockfile_path = ctx.scope.lockfile_path();
    let location = Some(FindingLocation {
        path: lockfile_path.to_path_buf(),
        line: None,
        column: None,
    });

    let reports = match verify_all_with_options(&ctx.scope, /*anthropic_strict=*/ false) {
        Ok(reports) => reports,
        Err(e) => {
            // Lockfile load/parse error becomes a single finding.
            let findings = vec![CheckFinding {
                category: CheckCategory::Lockfile,
                severity: Severity::Error,
                rule_id: "tau.lockfile.invalid",
                summary: format!("lockfile error: {e}"),
                detail: None,
                location,
                remediation: Some("run `tau install` to regenerate the lockfile".into()),
                structured: json!({"kind": "LoadError"}),
            }];
            return CheckResult {
                category: CheckCategory::Lockfile,
                status: CheckStatus::Failed,
                findings,
                duration: std::time::Duration::ZERO,
            };
        }
    };

    let mut findings = Vec::new();
    for report in &reports {
        if !report.status.is_drift() {
            continue;
        }
        findings.push(CheckFinding {
            category: CheckCategory::Lockfile,
            severity: Severity::Error,
            rule_id: "tau.lockfile.drift",
            summary: format!(
                "{}@{} drift: {:?}",
                report.name, report.version, report.status
            ),
            detail: None,
            location: location.clone(),
            remediation: Some("run `tau verify` for details, then `tau install` to refresh".into()),
            structured: json!({
                "package": report.name.to_string(),
                "version": report.version.to_string(),
                "status": format!("{:?}", report.status),
            }),
        });
    }

    let status = if findings.is_empty() {
        CheckStatus::Ok
    } else {
        CheckStatus::Failed
    };
    CheckResult {
        category: CheckCategory::Lockfile,
        status,
        findings,
        duration: std::time::Duration::ZERO,
    }
}
