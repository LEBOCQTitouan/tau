//! `tau sandbox` subcommand group — diagnostic + scaffolding.

use crate::cli::{SandboxArgs, SandboxCommand, SandboxSetupArgs};
use crate::output::Output;

/// Dispatch `tau sandbox <subcommand>`.
pub async fn run(args: &SandboxArgs, output: &mut Output) -> anyhow::Result<()> {
    match &args.command {
        SandboxCommand::Status => run_status(output).await,
        SandboxCommand::Setup(setup_args) => run_setup(setup_args, output).await,
    }
}

async fn run_status(output: &mut Output) -> anyhow::Result<()> {
    use tau_pkg::scope::{SandboxRequirements, ScopeConfig};
    use tau_runtime::sandbox::registry::{detect_platform, REGISTRY};

    let platform = detect_platform();

    // Platform line
    output.human(&format!("platform: {platform}"))?;
    output.human("")?;

    // Per-adapter probe table
    output.human("adapters detected:")?;
    for entry in REGISTRY.iter() {
        let name = entry.kind.name();
        // Pad to 14 chars so columns align
        let pad = if name.len() < 14 {
            " ".repeat(14 - name.len())
        } else {
            " ".to_string()
        };

        let status = if !entry.platforms.includes(platform) {
            "not applicable on this platform".to_string()
        } else {
            match tau_runtime::sandbox::instantiate_for_probe(entry.kind) {
                Ok(adapter) => match adapter.probe().await {
                    tau_ports::SandboxProbe::Available { tier, details } => {
                        if details.is_empty() {
                            format!("available, tier={tier:?}")
                        } else {
                            format!("available, tier={tier:?}; {details}")
                        }
                    }
                    tau_ports::SandboxProbe::Unavailable { reason } => {
                        format!("unavailable: {reason}")
                    }
                    other => format!("probe returned: {other:?}"),
                },
                Err(msg) => format!("unavailable: {msg}"),
            }
        };
        output.human(&format!("  {name}:{pad}{status}"))?;
    }
    output.human("")?;

    // Project requirements — read scope config; render errors inline (never bail).
    output.human("project requirements:")?;

    let scope_and_config: Result<(tau_pkg::scope::Scope, ScopeConfig), String> =
        std::env::current_dir()
            .map_err(|e| format!("could not read cwd: {e}"))
            .and_then(|cwd| {
                tau_pkg::scope::Scope::resolve(&cwd).map_err(|e| format!("scope resolve: {e}"))
            })
            .and_then(|scope| {
                let config_path = scope.config_path();
                if config_path.exists() {
                    std::fs::read_to_string(&config_path)
                        .map_err(|e| format!("reading scope config at {config_path:?}: {e}"))
                        .and_then(|text| {
                            ScopeConfig::read_from_str(&text).map_err(|e| {
                                format!("parsing scope config at {config_path:?}: {e}")
                            })
                        })
                        .map(|cfg| (scope, cfg))
                } else {
                    // No config.toml — use defaults.
                    let kind = scope.kind();
                    Ok((scope, ScopeConfig::new(kind)))
                }
            });

    match scope_and_config {
        Ok((_scope, cfg)) => {
            output.human(&format!("  required_tier: {:?}", cfg.sandbox.required_tier))?;
            if cfg.sandbox.required_shapes.is_empty() {
                output.human("  required_shapes: (auto-derived from plugins)")?;
            } else {
                output.human(&format!(
                    "  required_shapes: {:?}",
                    cfg.sandbox.required_shapes
                ))?;
            }
            output.human("")?;

            // Resolution outcome — no plugin-specific requirements for status.
            output.human("resolution:")?;
            let plugin_reqs: Vec<tau_domain::PluginSandboxRequirements> = vec![];
            let resolution =
                tau_runtime::sandbox::resolve_adapter(&cfg.sandbox, &plugin_reqs).await;
            match resolution {
                Ok(adapter) => {
                    output.human(&format!("  selected adapter: {}", adapter.name()))?;
                }
                Err(e) => {
                    output.human(&format!("  error: {e}"))?;
                }
            }
        }
        Err(e) => {
            output.human(&format!("  (could not read config: {e})"))?;
            // Use defaults for resolution section.
            output.human("")?;
            output.human("resolution:")?;
            let plugin_reqs: Vec<tau_domain::PluginSandboxRequirements> = vec![];
            let resolution = tau_runtime::sandbox::resolve_adapter(
                &SandboxRequirements::default(),
                &plugin_reqs,
            )
            .await;
            match resolution {
                Ok(adapter) => {
                    output.human(&format!("  selected adapter: {}", adapter.name()))?;
                }
                Err(e) => {
                    output.human(&format!("  error: {e}"))?;
                }
            }
        }
    }

    Ok(())
}

async fn run_setup(_args: &SandboxSetupArgs, _output: &mut Output) -> anyhow::Result<()> {
    anyhow::bail!(
        "`tau sandbox setup` is not yet implemented; see Task 10 of the sandbox activation plan"
    );
}
