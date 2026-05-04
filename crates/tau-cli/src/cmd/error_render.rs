//! Guided multi-option error renderer for sandbox resolution failures.
//!
//! Spec §6 of 2026-05-04-sandbox-activation-design.md.

use std::fmt::Write as _;

use tau_runtime::sandbox::{ResolutionError, ResolutionRejection};

/// Render a [`ResolutionError`] as a guided multi-option error message.
///
/// Output is plain text (no color); each line is one logical step. The
/// caller (typically tau-cli) prints this to stderr and exits 2.
pub fn render_resolution_error(err: &ResolutionError) -> String {
    match err {
        ResolutionError::NoAdapterMatches {
            tried,
            platform,
            required_tier,
        } => render_no_adapter_matches(tried, platform, *required_tier),
        ResolutionError::PluginTierMismatch {
            plugin,
            required,
            delivered,
        } => render_plugin_tier_mismatch(plugin, *required, *delivered),
        ResolutionError::ConfigError { message } => {
            format!("sandbox configuration error: {message}")
        }
        // Catch-all for #[non_exhaustive] forward-compat.
        other => format!("sandbox resolution error: {other}"),
    }
}

fn render_no_adapter_matches(
    tried: &[(String, ResolutionRejection)],
    platform: &str,
    required_tier: tau_ports::SandboxTier,
) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "no sandbox adapter satisfies project requirements");
    let _ = writeln!(out);
    let _ = writeln!(out, "  required: tier={required_tier:?}");
    let _ = writeln!(out, "  detected platform: {platform}");
    let _ = writeln!(out);
    let _ = writeln!(out, "  adapter status on this machine:");
    for (name, rejection) in tried {
        let pad = if name.len() < 12 { 12 - name.len() } else { 1 };
        let padding = " ".repeat(pad);
        let line = match rejection {
            ResolutionRejection::PlatformMismatch => "not applicable on this platform".to_string(),
            ResolutionRejection::ProbeUnavailable(reason) => {
                format!("unavailable: {reason}")
            }
            ResolutionRejection::TierTooLow {
                delivered,
                required,
            } => {
                format!("available, but tier={delivered:?} below required={required:?}")
            }
            ResolutionRejection::ShapesUnsupported { missing } => {
                format!("missing shapes: {missing:?}")
            }
            ResolutionRejection::PluginTierTooLow { plugin, required } => {
                format!("plugin '{plugin}' requires tier {required:?}; this adapter cannot deliver")
            }
            other => format!("rejected: {other}"),
        };
        let _ = writeln!(out, "    {name}:{padding}{line}");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "  options to proceed:");
    let _ = writeln!(
        out,
        "    - install a container runtime (Docker Desktop or Podman)"
    );
    let _ = writeln!(
        out,
        "    - reduce required_tier in <scope>/config.toml to \"light\" or \"none\""
    );
    let _ = writeln!(out, "    - run with --no-sandbox (this invocation only)");
    out
}

/// Render a plugin-tier mismatch as a guided error.
pub fn render_plugin_tier_mismatch(
    plugin: &str,
    required: tau_domain::PluginRequiredTier,
    delivered: tau_ports::SandboxTier,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "plugin '{plugin}' requires tier {required:?}; selected adapter delivers {delivered:?}"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "  options to proceed:");
    let _ = writeln!(
        out,
        "    - upgrade required_tier in <scope>/config.toml to match the plugin's floor"
    );
    let _ = writeln!(
        out,
        "    - remove the plugin from agents that don't actually use it"
    );
    let _ = writeln!(
        out,
        "    - the plugin author can reduce required_tier (NOT recommended for security-sensitive plugins)"
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_adapter_matches_renders_all_sections() {
        let err = ResolutionError::NoAdapterMatches {
            tried: vec![
                ("native".into(), ResolutionRejection::PlatformMismatch),
                (
                    "container".into(),
                    ResolutionRejection::ProbeUnavailable(
                        "neither docker nor podman on PATH".into(),
                    ),
                ),
                (
                    "passthrough".into(),
                    ResolutionRejection::TierTooLow {
                        delivered: tau_ports::SandboxTier::None,
                        required: tau_ports::SandboxTier::Strict,
                    },
                ),
            ],
            platform: "macos".into(),
            required_tier: tau_ports::SandboxTier::Strict,
        };
        let rendered = render_resolution_error(&err);
        insta::assert_snapshot!(rendered);
    }

    #[test]
    fn plugin_tier_mismatch_renders_with_options() {
        let err = ResolutionError::PluginTierMismatch {
            plugin: "credentials-store".into(),
            required: tau_domain::PluginRequiredTier::Strict,
            delivered: tau_ports::SandboxTier::None,
        };
        let rendered = render_resolution_error(&err);
        insta::assert_snapshot!(rendered);
    }

    #[test]
    fn config_error_renders_message() {
        let err = ResolutionError::ConfigError {
            message: "unknown adapter kind: weird".into(),
        };
        let rendered = render_resolution_error(&err);
        assert!(rendered.contains("unknown adapter kind: weird"));
        assert!(rendered.contains("configuration error"));
    }

    #[test]
    fn render_plugin_tier_mismatch_helper_works_directly() {
        let rendered = render_plugin_tier_mismatch(
            "auth-store",
            tau_domain::PluginRequiredTier::Strict,
            tau_ports::SandboxTier::None,
        );
        insta::assert_snapshot!(rendered);
    }
}
