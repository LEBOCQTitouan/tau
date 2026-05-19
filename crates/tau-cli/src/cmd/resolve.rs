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
    if args.check_sandbox {
        return run_check_sandbox(args, output).await;
    }

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

/// Implements `tau resolve --check-sandbox`.
///
/// Loads the lockfile, reads each installed plugin's capabilities from
/// the package manifest on disk, builds a [`tau_ports::SandboxPlan`]
/// per plugin, and validates it against the active scope's configured
/// adapter. Reports ✓ / ✗ per plugin (human-readable) or JSON events
/// when `--json` is set.
///
/// When the project's `required_tier` is `None` (passthrough would be
/// selected at runtime), `--check-sandbox` skips passthrough as a
/// validation target and instead uses the highest-priority
/// non-passthrough adapter available on the current platform. This
/// surfaces what would happen if the user strengthens their requirement.
/// If no non-passthrough adapter is available, exits 2 with a clear
/// error.
///
/// Exit 0 if all plugins pass; exit 2 if any fail or no adapter is
/// available (per ADR-0007 three-bucket exit codes).
async fn run_check_sandbox(_args: &ResolveArgs, output: &mut Output) -> anyhow::Result<()> {
    use crate::cmd::resolve_helpers::{
        check_plugin_sandbox, read_sandbox_requirements_for_check, resolve_sandbox_check_adapter,
        SandboxPluginOutcome,
    };
    use tau_pkg::scope::SandboxRequiredTier;

    // 1. Resolve the scope.
    let cwd = std::env::current_dir()?;
    let scope = tau_pkg::Scope::resolve(&cwd).context("resolving package scope")?;

    // 2. Read [sandbox] from scope config (swallows malformed-config errors;
    //    `tau check config` is the authoritative report surface).
    let sandbox_requirements = read_sandbox_requirements_for_check(&scope);

    // 3. Resolve the adapter (handles the None->strict-for-validation pivot).
    let adapter = match resolve_sandbox_check_adapter(&sandbox_requirements).await {
        Ok(a) => a,
        Err(e) => {
            let msg = match sandbox_requirements.required_tier {
                SandboxRequiredTier::None => {
                    "no non-permissive adapter available to validate against; cannot perform sandbox check".to_string()
                }
                _ => format!("no sandbox adapter available: {e}"),
            };
            if output.is_json() {
                output.json(&serde_json::json!({
                    "event": "error",
                    "reason": msg,
                }))?;
            } else {
                output.error(&msg)?;
            }
            std::process::exit(2);
        }
    };

    // 4. Load the lockfile.
    let lockfile_path = scope.lockfile_path();
    let lockfile = tau_pkg::LockFile::load(&lockfile_path)
        .with_context(|| format!("loading lockfile at {lockfile_path:?}"))?;

    let mut ok_count = 0usize;
    let mut error_count = 0usize;

    // 5. For each installed package that has a plugin entry, validate its plan.
    for pkg in &lockfile.packages {
        if pkg.plugin.is_none() {
            // Data-only package — no sandbox check needed.
            continue;
        }

        let plugin_id = pkg.name.as_str().to_owned();
        let pkg_dir = scope.package_dir(&pkg.name, &pkg.active_version);
        let manifest_path = pkg_dir.join("tau.toml");

        match check_plugin_sandbox(&plugin_id, &manifest_path, Some(&adapter)) {
            SandboxPluginOutcome::Ok => {
                ok_count += 1;
                if output.is_json() {
                    output.json(&serde_json::json!({
                        "event": "sandbox_check",
                        "plugin_id": plugin_id,
                        "status": "ok",
                    }))?;
                } else {
                    output.human(&format!("✓ {plugin_id}"))?;
                }
            }
            SandboxPluginOutcome::BuildPlanFailed(msg) => {
                error_count += 1;
                let reason = format!("build_plan failed for {plugin_id}: {msg}");
                if output.is_json() {
                    output.json(&serde_json::json!({
                        "event": "sandbox_check",
                        "plugin_id": plugin_id,
                        "status": "error",
                        "reason": reason,
                    }))?;
                } else {
                    output.error(&reason)?;
                }
            }
            SandboxPluginOutcome::ValidateFailed(errors) => {
                error_count += 1;
                for err in &errors {
                    if output.is_json() {
                        output.json(&serde_json::json!({
                            "event": "sandbox_check",
                            "plugin_id": plugin_id,
                            "status": "error",
                            "reason": err.reason,
                            "capability": format!("{:?}", err.capability.required_shape()),
                        }))?;
                    } else {
                        output.human(&format!("✗ {plugin_id}: {}", err.reason))?;
                    }
                }
            }
            SandboxPluginOutcome::ManifestUnreadable(msg) => {
                let reason = format!(
                    "could not read manifest for {plugin_id}: {msg} — skipping capability check"
                );
                if output.is_json() {
                    output.json(&serde_json::json!({
                        "event": "sandbox_check",
                        "plugin_id": plugin_id,
                        "status": "skipped",
                        "reason": reason,
                    }))?;
                } else {
                    output.warn(&reason)?;
                }
            }
        }
    }

    // 6. Summary.
    let total = ok_count + error_count;
    if output.is_json() {
        output.json(&serde_json::json!({
            "event": "summary",
            "ok": ok_count,
            "errors": error_count,
        }))?;
    } else {
        output.human(&format!(
            "{total} plugins checked: {ok_count} ok, {error_count} errors"
        ))?;
    }

    // 7. Exit code: 0 if all ok, 2 if any errors.
    if error_count > 0 {
        std::process::exit(2);
    }
    Ok(())
}
