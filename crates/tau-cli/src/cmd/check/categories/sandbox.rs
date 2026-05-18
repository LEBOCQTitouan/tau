//! `tau check sandbox` — validate sandbox plans for each installed plugin.
//!
//! Default (full): build plan AND validate against the resolved adapter.
//! `--fast`: build plan only; skip adapter probe + validation.
//!
//! # Implementation note (Path C — adapted from resolve.rs)
//!
//! The core plan-build/validate logic is derived from
//! `crates/tau-cli/src/cmd/resolve.rs::run_check_sandbox`. That function
//! is monolithic (interleaved output formatting), so rather than surgically
//! refactoring it we duplicate the ~50 LOC of validation logic here and
//! translate failures into `CheckFinding`s instead of I/O calls.
//!
//! TODO: extract a shared helper (Path B) in a follow-up commit so that
//! resolve.rs and this module don't diverge.

use crate::cmd::check::result::{
    CheckCategory, CheckFinding, CheckResult, CheckStatus, FindingLocation, Severity,
};
use crate::cmd::check::runner::CheckCtx;
use serde_json::json;

pub async fn run_sandbox(ctx: &CheckCtx) -> CheckResult {
    // project.is_none() means tau.toml is malformed — the config check
    // reports this; we just skip to avoid duplicate noise.
    if ctx.project.is_none() {
        return CheckResult {
            category: CheckCategory::Sandbox,
            status: CheckStatus::Skipped {
                reason: "tau.toml malformed (see config check)".into(),
            },
            findings: Vec::new(),
            duration: std::time::Duration::ZERO,
        };
    }

    use tau_pkg::scope::{SandboxRequiredTier, SandboxRequirements, ScopeConfig};
    use tau_runtime::sandbox::{
        build_plan, resolve_adapter, resolve_strict_for_validation, validate_plan_against_adapter,
    };

    let scope_config_path = ctx.scope.config_path();
    let tau_toml_path = ctx.project_root.join("tau.toml");

    // Read sandbox requirements from the scope config file.
    let sandbox_requirements = if scope_config_path.exists() {
        match std::fs::read_to_string(&scope_config_path) {
            Ok(text) => match ScopeConfig::read_from_str(&text) {
                Ok(cfg) => cfg.sandbox,
                Err(_) => SandboxRequirements::default(),
            },
            Err(_) => SandboxRequirements::default(),
        }
    } else {
        SandboxRequirements::default()
    };

    let mut findings: Vec<CheckFinding> = Vec::new();

    // Load the lockfile. If missing or unreadable, skip — the lockfile
    // check will already report this.
    let lockfile_path = ctx.scope.lockfile_path();
    let lockfile = if lockfile_path.exists() {
        match tau_pkg::LockFile::load(&lockfile_path) {
            Ok(lf) => lf,
            Err(_) => {
                return CheckResult {
                    category: CheckCategory::Sandbox,
                    status: CheckStatus::Skipped {
                        reason: "lockfile missing or unreadable (see lockfile check)".into(),
                    },
                    findings: Vec::new(),
                    duration: std::time::Duration::ZERO,
                };
            }
        }
    } else {
        return CheckResult {
            category: CheckCategory::Sandbox,
            status: CheckStatus::Skipped {
                reason: "lockfile missing or unreadable (see lockfile check)".into(),
            },
            findings: Vec::new(),
            duration: std::time::Duration::ZERO,
        };
    };

    // Collect only packages that have a plugin entry — data-only packages
    // don't need sandbox plans.
    let plugin_pkgs: Vec<_> = lockfile
        .packages
        .iter()
        .filter(|p| p.plugin.is_some())
        .collect();

    if plugin_pkgs.is_empty() {
        return CheckResult {
            category: CheckCategory::Sandbox,
            status: CheckStatus::Skipped {
                reason: "no plugin packages in lockfile".into(),
            },
            findings: Vec::new(),
            duration: std::time::Duration::ZERO,
        };
    }

    // `--fast`: build plans but skip adapter probe + validation.
    if ctx.fast {
        for pkg in &plugin_pkgs {
            let plugin_id = pkg.name.as_str().to_owned();
            let pkg_dir = ctx.scope.package_dir(&pkg.name, &pkg.active_version);
            let manifest_path = pkg_dir.join("tau.toml");

            let package_caps = match tau_pkg::read_manifest(&manifest_path) {
                Ok(m) => m.capabilities().to_vec(),
                Err(_) => continue, // skip unreadable manifests silently in fast mode
            };

            if let Err(e) = build_plan(&package_caps, &[], None, None) {
                findings.push(build_plan_finding(
                    &plugin_id,
                    format!("{e}"),
                    &tau_toml_path,
                ));
            }
        }

        let status = if findings.is_empty() {
            CheckStatus::Ok
        } else {
            CheckStatus::Failed
        };
        return CheckResult {
            category: CheckCategory::Sandbox,
            status,
            findings,
            duration: std::time::Duration::ZERO,
        };
    }

    // Full mode: resolve adapter then build + validate each plan.
    //
    // When required_tier is None the runtime would pick Passthrough, which
    // trivially accepts every plan. We mirror resolve.rs's behaviour and
    // pick the highest-priority non-passthrough adapter instead to surface
    // what would happen if the user strengthens the requirement.
    let adapter_result = if matches!(
        sandbox_requirements.required_tier,
        SandboxRequiredTier::None
    ) {
        resolve_strict_for_validation().await
    } else {
        resolve_adapter(&sandbox_requirements, &[]).await
    };

    let adapter = match adapter_result {
        Ok(a) => a,
        Err(e) => {
            // No adapter available — emit an advisory warning and skip
            // validation rather than hard-failing. The user may be on a
            // platform where no strict adapter is installed; that is not
            // a check *failure* by itself.
            findings.push(CheckFinding {
                category: CheckCategory::Sandbox,
                severity: Severity::Warning,
                rule_id: "tau.sandbox.no_adapter",
                summary: format!("no sandbox adapter available for validation: {e}"),
                detail: Some(
                    "Sandbox plan shapes could not be validated. \
                     Install a sandbox adapter (e.g. tau-sandbox-darwin) to enable full checks."
                        .into(),
                ),
                location: None,
                remediation: None,
                structured: json!({ "kind": "NoAdapterAvailable", "error": e.to_string() }),
            });
            let status = CheckStatus::Ok; // advisory only, not a hard failure
            return CheckResult {
                category: CheckCategory::Sandbox,
                status,
                findings,
                duration: std::time::Duration::ZERO,
            };
        }
    };

    for pkg in &plugin_pkgs {
        let plugin_id = pkg.name.as_str().to_owned();
        let pkg_dir = ctx.scope.package_dir(&pkg.name, &pkg.active_version);
        let manifest_path = pkg_dir.join("tau.toml");

        let package_caps = match tau_pkg::read_manifest(&manifest_path) {
            Ok(m) => m.capabilities().to_vec(),
            Err(e) => {
                findings.push(CheckFinding {
                    category: CheckCategory::Sandbox,
                    severity: Severity::Warning,
                    rule_id: "tau.sandbox.manifest_unreadable",
                    summary: format!(
                        "could not read manifest for `{plugin_id}`: {e} — skipping capability check"
                    ),
                    detail: None,
                    location: Some(FindingLocation {
                        path: manifest_path,
                        line: None,
                        column: None,
                    }),
                    remediation: Some("tau resolve".into()),
                    structured: json!({
                        "plugin_id": plugin_id,
                        "kind": "ManifestUnreadable",
                        "error": e.to_string(),
                    }),
                });
                continue;
            }
        };

        // Build the sandbox plan (no project-level overrides at this layer).
        let plan = match build_plan(&package_caps, &[], None, None) {
            Ok(p) => p,
            Err(e) => {
                findings.push(build_plan_finding(
                    &plugin_id,
                    format!("{e}"),
                    &tau_toml_path,
                ));
                continue;
            }
        };

        // Validate the plan against the resolved adapter.
        if let Err(errors) = validate_plan_against_adapter(&plugin_id, &plan, &adapter) {
            for err in errors {
                findings.push(CheckFinding {
                    category: CheckCategory::Sandbox,
                    severity: Severity::Error,
                    rule_id: "tau.sandbox.plan_invalid",
                    summary: format!("plugin `{plugin_id}`: {}", err.reason),
                    detail: None,
                    location: Some(FindingLocation {
                        path: tau_toml_path.clone(),
                        line: None,
                        column: None,
                    }),
                    remediation: None,
                    structured: json!({
                        "plugin_id": plugin_id,
                        "kind": "SandboxValidationFailed",
                        "reason": err.reason,
                    }),
                });
            }
        }
    }

    let status = if findings.iter().any(|f| f.severity == Severity::Error) {
        CheckStatus::Failed
    } else {
        CheckStatus::Ok
    };
    CheckResult {
        category: CheckCategory::Sandbox,
        status,
        findings,
        duration: std::time::Duration::ZERO,
    }
}

fn build_plan_finding(
    plugin_id: &str,
    message: String,
    tau_toml_path: &std::path::Path,
) -> CheckFinding {
    CheckFinding {
        category: CheckCategory::Sandbox,
        severity: Severity::Error,
        rule_id: "tau.sandbox.plan_invalid",
        summary: format!("build_plan failed for `{plugin_id}`: {message}"),
        detail: None,
        location: Some(FindingLocation {
            path: tau_toml_path.to_path_buf(),
            line: None,
            column: None,
        }),
        remediation: None,
        structured: json!({
            "plugin_id": plugin_id,
            "kind": "BuildPlanFailed",
            "error": message,
        }),
    }
}
