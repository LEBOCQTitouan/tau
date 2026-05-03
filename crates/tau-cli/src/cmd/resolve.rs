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
/// Exit 0 if all plugins pass; exit 2 if any fail or no adapter is
/// available (per ADR-0007 three-bucket exit codes).
async fn run_check_sandbox(_args: &ResolveArgs, output: &mut Output) -> anyhow::Result<()> {
    use tau_pkg::scope::ScopeConfig;
    use tau_runtime::sandbox::{build_plan, select_adapter, validate_plan_against_adapter};

    // 1. Resolve the scope.
    let cwd = std::env::current_dir()?;
    let scope = tau_pkg::Scope::resolve(&cwd).context("resolving package scope")?;

    // 2. Read scope config for [sandbox] section.
    let config_path = scope.config_path();
    let sandbox_config = if config_path.exists() {
        let text = std::fs::read_to_string(&config_path)
            .with_context(|| format!("reading scope config at {config_path:?}"))?;
        let scope_config = ScopeConfig::read_from_str(&text)
            .with_context(|| format!("parsing scope config at {config_path:?}"))?;
        scope_config.sandbox
    } else {
        tau_pkg::scope::SandboxConfig::default()
    };

    // 3. Select the sandbox adapter.
    let adapter = match select_adapter(&sandbox_config).await {
        Ok(a) => a,
        Err(e) => {
            let msg = format!("no sandbox adapter available: {e}");
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
        let Some(_locked_plugin) = &pkg.plugin else {
            // Data-only package — no sandbox check needed.
            continue;
        };

        let plugin_id = pkg.name.as_str().to_owned();

        // Read the package manifest from disk to get declared capabilities.
        let pkg_dir = scope.package_dir(&pkg.name, &pkg.active_version);
        let manifest_path = pkg_dir.join("tau.toml");

        let package_caps = match tau_pkg::read_manifest(&manifest_path) {
            Ok(manifest) => manifest.capabilities().to_vec(),
            Err(e) => {
                // Manifest missing or unreadable — treat as empty capabilities
                // and log a warning. The plugin may still have been installed
                // in a previous tau version; we skip shape-level checking.
                let reason = format!(
                    "could not read manifest for {plugin_id}: {e} — skipping capability check"
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
                continue;
            }
        };

        // Build a plan (no project overrides at this layer).
        let plan = match build_plan(&package_caps, &[], None, None) {
            Ok(p) => p,
            Err(e) => {
                let reason = format!("build_plan failed for {plugin_id}: {e}");
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
                error_count += 1;
                continue;
            }
        };

        // Validate the plan against the adapter.
        match validate_plan_against_adapter(&plugin_id, &plan, &adapter) {
            Ok(()) => {
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
            Err(errors) => {
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
