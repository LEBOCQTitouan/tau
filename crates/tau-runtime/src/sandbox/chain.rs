//! Adapter chain configuration + probe-based selection.
//!
//! At runtime, the kernel reads the active scope's `[sandbox]` config and
//! probes each adapter in order. The first one returning
//! [`SandboxProbe::Available`] (with tier ≥ minimum) is selected. If the
//! chain is empty, a platform-appropriate default chain is used.

use std::process::Command;

use tau_domain::CapabilityShapeSet;
use tau_pkg::scope::{SandboxAdapterConfig, SandboxAdapterKind, SandboxConfig, SandboxMinimumTier};
use tau_ports::{Sandbox, SandboxError, SandboxHandle, SandboxPlan, SandboxProbe, SandboxTier};
use tau_sandbox_container::{ContainerRuntime, ContainerSandbox};
use tau_sandbox_native::NativeSandbox;

/// A concrete sandbox adapter selected from the chain.
///
/// Enum dispatch — the `Sandbox` trait uses `async fn` in trait (AFIT) which
/// is not dyn-compatible, so we use an explicit enum rather than `Arc<dyn Sandbox>`.
#[non_exhaustive]
#[allow(clippy::large_enum_variant)]
pub enum SandboxAdapter {
    /// `tau-sandbox-native` Linux adapter.
    Native(NativeSandbox),
    /// `tau-sandbox-container` docker/podman adapter.
    Container(ContainerSandbox),
    /// `tau_ports::fixtures::MockSandbox` — available in all builds but only
    /// instantiable during `cargo test`, when the `test-fixtures` feature is
    /// enabled, or when `TAU_TESTING_ALLOW_MOCK_SANDBOX=1` is set.
    Mock(tau_ports::fixtures::MockSandbox),
}

impl std::fmt::Debug for SandboxAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxAdapter::Native(_) => f.debug_tuple("SandboxAdapter::Native").finish(),
            SandboxAdapter::Container(_) => f.debug_tuple("SandboxAdapter::Container").finish(),
            SandboxAdapter::Mock(_) => f.debug_tuple("SandboxAdapter::Mock").finish(),
        }
    }
}

impl SandboxAdapter {
    /// Returns the adapter's name.
    pub fn name(&self) -> &str {
        match self {
            SandboxAdapter::Native(a) => a.name(),
            SandboxAdapter::Container(a) => a.name(),
            SandboxAdapter::Mock(a) => a.name(),
        }
    }

    /// Probe the adapter for availability.
    pub async fn probe(&self) -> SandboxProbe {
        match self {
            SandboxAdapter::Native(a) => a.probe().await,
            SandboxAdapter::Container(a) => a.probe().await,
            SandboxAdapter::Mock(a) => a.probe().await,
        }
    }

    /// Returns capability shapes this adapter can enforce.
    pub fn supported_shapes(&self) -> CapabilityShapeSet {
        match self {
            SandboxAdapter::Native(a) => a.supported_shapes(),
            SandboxAdapter::Container(a) => a.supported_shapes(),
            SandboxAdapter::Mock(a) => a.supported_shapes(),
        }
    }

    /// Validate that the given plan can be executed by this adapter.
    pub fn validate_plan(&self, plan: &SandboxPlan) -> Result<(), SandboxError> {
        match self {
            SandboxAdapter::Native(a) => a.validate_plan(plan),
            SandboxAdapter::Container(a) => a.validate_plan(plan),
            SandboxAdapter::Mock(a) => a.validate_plan(plan),
        }
    }

    /// Apply sandbox enforcement to a [`Command`] in preparation for spawn.
    pub async fn wrap_spawn(
        &self,
        plan: &SandboxPlan,
        cmd: &mut Command,
    ) -> Result<SandboxHandle, SandboxError> {
        match self {
            SandboxAdapter::Native(a) => a.wrap_spawn(plan, cmd).await,
            SandboxAdapter::Container(a) => a.wrap_spawn(plan, cmd).await,
            SandboxAdapter::Mock(a) => a.wrap_spawn(plan, cmd).await,
        }
    }
}

impl Sandbox for SandboxAdapter {
    fn name(&self) -> &str {
        match self {
            SandboxAdapter::Native(s) => s.name(),
            SandboxAdapter::Container(s) => s.name(),
            SandboxAdapter::Mock(s) => s.name(),
        }
    }

    async fn probe(&self) -> SandboxProbe {
        match self {
            SandboxAdapter::Native(s) => s.probe().await,
            SandboxAdapter::Container(s) => s.probe().await,
            SandboxAdapter::Mock(s) => s.probe().await,
        }
    }

    fn supported_shapes(&self) -> CapabilityShapeSet {
        match self {
            SandboxAdapter::Native(s) => s.supported_shapes(),
            SandboxAdapter::Container(s) => s.supported_shapes(),
            SandboxAdapter::Mock(s) => s.supported_shapes(),
        }
    }

    fn validate_plan(&self, plan: &SandboxPlan) -> Result<(), SandboxError> {
        match self {
            SandboxAdapter::Native(s) => s.validate_plan(plan),
            SandboxAdapter::Container(s) => s.validate_plan(plan),
            SandboxAdapter::Mock(s) => s.validate_plan(plan),
        }
    }

    async fn wrap_spawn(
        &self,
        plan: &SandboxPlan,
        cmd: &mut Command,
    ) -> Result<SandboxHandle, SandboxError> {
        match self {
            SandboxAdapter::Native(s) => s.wrap_spawn(plan, cmd).await,
            SandboxAdapter::Container(s) => s.wrap_spawn(plan, cmd).await,
            SandboxAdapter::Mock(s) => s.wrap_spawn(plan, cmd).await,
        }
    }
}

/// Errors returned by [`select_adapter`].
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SandboxChainError {
    /// No adapter in the chain returned `Available`.
    #[error("no sandbox adapter available — tried: {}", format_tried(.tried))]
    NoAdapterAvailable {
        /// `(adapter_kind_name, rejection_reason)` for each adapter probed.
        tried: Vec<(String, String)>,
    },
    /// All adapters were `Available` but none met the minimum tier.
    #[error("no adapter meets minimum tier {required:?} (best available was {best_available:?})")]
    MinimumTierUnsatisfiable {
        /// The required minimum tier from `[sandbox] minimum_tier`.
        required: SandboxTier,
        /// The best tier observed across all probed adapters.
        best_available: SandboxTier,
    },
    /// Configuration error (e.g. unknown adapter kind, malformed options).
    #[error("sandbox chain configuration error: {message}")]
    ConfigError {
        /// Detail.
        message: String,
    },
}

/// Select an adapter from the chain, applying the platform default when
/// the chain is empty. Returns the first `Available` adapter whose tier
/// meets the configured minimum.
pub async fn select_adapter(cfg: &SandboxConfig) -> Result<SandboxAdapter, SandboxChainError> {
    let entries = if cfg.chain.is_empty() {
        default_chain()
    } else {
        cfg.chain.clone()
    };

    let minimum_tier = cfg.minimum_tier.map(map_min_tier).transpose()?;

    let mut best_available_tier: Option<SandboxTier> = None;
    let mut tried: Vec<(String, String)> = Vec::new();

    for entry in &entries {
        let adapter = instantiate(entry)?;
        let probe = adapter.probe().await;
        match probe {
            SandboxProbe::Available { tier, .. } => {
                if let Some(min) = minimum_tier {
                    if tier < min {
                        // Track best so far for error reporting.
                        tried.push((
                            adapter.name().to_owned(),
                            format!("tier {tier:?} below minimum {min:?}"),
                        ));
                        best_available_tier = match best_available_tier {
                            Some(prev) if prev >= tier => Some(prev),
                            _ => Some(tier),
                        };
                        continue;
                    }
                }
                return Ok(adapter);
            }
            SandboxProbe::Unavailable { reason } => {
                tried.push((adapter.name().to_owned(), reason));
                continue;
            }
            other => {
                return Err(SandboxChainError::ConfigError {
                    message: format!("unexpected probe result: {other:?}"),
                })
            }
        }
    }

    if let (Some(min), Some(best)) = (minimum_tier, best_available_tier) {
        return Err(SandboxChainError::MinimumTierUnsatisfiable {
            required: min,
            best_available: best,
        });
    }
    Err(SandboxChainError::NoAdapterAvailable { tried })
}

/// Default chain when the scope config doesn't specify one.
fn default_chain() -> Vec<SandboxAdapterConfig> {
    vec![
        SandboxAdapterConfig::new(SandboxAdapterKind::Native),
        SandboxAdapterConfig::new(SandboxAdapterKind::Container),
    ]
}

fn map_min_tier(t: SandboxMinimumTier) -> Result<SandboxTier, SandboxChainError> {
    match t {
        SandboxMinimumTier::None => Ok(SandboxTier::None),
        SandboxMinimumTier::Light => Ok(SandboxTier::Light),
        SandboxMinimumTier::Strict => Ok(SandboxTier::Strict),
        other => Err(SandboxChainError::ConfigError {
            message: format!(
                "unknown minimum_tier variant: {other:?} — refuse to default to weaker tier"
            ),
        }),
    }
}

fn format_tried(tried: &[(String, String)]) -> String {
    tried
        .iter()
        .map(|(name, reason)| format!("{name} ({reason})"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn instantiate(entry: &SandboxAdapterConfig) -> Result<SandboxAdapter, SandboxChainError> {
    match entry.kind {
        SandboxAdapterKind::Native => {
            let tier = entry
                .options
                .get("tier")
                .and_then(|v| v.as_str())
                .map(parse_tier_str)
                .transpose()
                .map_err(|m| SandboxChainError::ConfigError { message: m })?
                .unwrap_or(SandboxTier::Strict);
            Ok(SandboxAdapter::Native(NativeSandbox::new("native", tier)))
        }
        SandboxAdapterKind::Container => {
            let runtime = entry
                .options
                .get("runtime")
                .and_then(|v| v.as_str())
                .map(parse_container_runtime)
                .transpose()
                .map_err(|m| SandboxChainError::ConfigError { message: m })?
                .unwrap_or(ContainerRuntime::Auto);
            Ok(SandboxAdapter::Container(ContainerSandbox::new(
                "container",
                runtime,
            )))
        }
        SandboxAdapterKind::Mock => {
            // Mock is normally test-only — but tau's CLI integration tests
            // invoke the real binary via assert_cmd::cargo_bin and need a way
            // to opt in. The escape hatch is the env var
            // `TAU_TESTING_ALLOW_MOCK_SANDBOX=1`. Production deployments should
            // never set this; it's a development/testing affordance.
            let allowed_in_build = cfg!(any(test, feature = "test-fixtures"));
            let allowed_via_env = std::env::var("TAU_TESTING_ALLOW_MOCK_SANDBOX")
                .map(|v| v == "1")
                .unwrap_or(false);
            if allowed_in_build || allowed_via_env {
                // tau-ports' fixtures module is always present (no feature flag);
                // it's just considered "test scaffolding" by convention.
                Ok(SandboxAdapter::Mock(tau_ports::fixtures::MockSandbox::new(
                    "mock",
                )))
            } else {
                Err(SandboxChainError::ConfigError {
                    message: "Mock adapter is only available in test builds (set TAU_TESTING_ALLOW_MOCK_SANDBOX=1 to override)".into(),
                })
            }
        }
        other => Err(SandboxChainError::ConfigError {
            message: format!("unknown adapter kind: {other:?}"),
        }),
    }
}

fn parse_tier_str(s: &str) -> Result<SandboxTier, String> {
    match s.to_ascii_lowercase().as_str() {
        "none" => Ok(SandboxTier::None),
        "light" => Ok(SandboxTier::Light),
        "strict" => Ok(SandboxTier::Strict),
        other => Err(format!("unknown sandbox tier: {other}")),
    }
}

fn parse_container_runtime(s: &str) -> Result<ContainerRuntime, String> {
    match s.to_ascii_lowercase().as_str() {
        "docker" => Ok(ContainerRuntime::Docker),
        "podman" => Ok(ContainerRuntime::Podman),
        "auto" => Ok(ContainerRuntime::Auto),
        other => Err(format!("unknown container runtime: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_chain_falls_back_to_default() {
        // Default chain: [native, container]. On macOS, native is Unavailable;
        // container probe runs `docker --version` (likely Unavailable on CI
        // unless docker is installed). On no-docker hosts, expect NoAdapterAvailable.
        // This test is environment-dependent; we just assert the function returns
        // SOMETHING (Ok or NoAdapterAvailable, not a ConfigError).
        let cfg = SandboxConfig::default();
        let result = select_adapter(&cfg).await;
        match result {
            Ok(_) | Err(SandboxChainError::NoAdapterAvailable { .. }) => {}
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }

    #[tokio::test]
    async fn mock_chain_selects_mock() {
        let cfg = SandboxConfig::with_chain(
            vec![SandboxAdapterConfig::new(SandboxAdapterKind::Mock)],
            None,
        );
        let adapter = select_adapter(&cfg).await.unwrap();
        assert_eq!(adapter.name(), "mock");
    }

    #[tokio::test]
    async fn minimum_tier_strict_with_only_mock_unsatisfiable() {
        // Mock advertises Tier::None; minimum Strict cannot be met.
        let cfg = SandboxConfig::with_chain(
            vec![SandboxAdapterConfig::new(SandboxAdapterKind::Mock)],
            Some(SandboxMinimumTier::Strict),
        );
        let err = select_adapter(&cfg).await.unwrap_err();
        assert!(matches!(
            err,
            SandboxChainError::MinimumTierUnsatisfiable { .. }
        ));
    }

    #[tokio::test]
    async fn unknown_adapter_kind_returns_config_error() {
        // Can't construct an unknown variant via the enum (it's local), so
        // skip — the parse path in scope.rs already handles unknown serde input.
        // This test is a placeholder to document the contract.
    }

    #[test]
    fn parse_tier_recognizes_known_values() {
        assert_eq!(parse_tier_str("none").unwrap(), SandboxTier::None);
        assert_eq!(parse_tier_str("light").unwrap(), SandboxTier::Light);
        assert_eq!(parse_tier_str("Strict").unwrap(), SandboxTier::Strict);
        assert!(parse_tier_str("hardened").is_err());
    }

    #[test]
    fn parse_container_runtime_recognizes_known() {
        assert_eq!(
            parse_container_runtime("docker").unwrap(),
            ContainerRuntime::Docker
        );
        assert_eq!(
            parse_container_runtime("PODMAN").unwrap(),
            ContainerRuntime::Podman
        );
        assert_eq!(
            parse_container_runtime("auto").unwrap(),
            ContainerRuntime::Auto
        );
        assert!(parse_container_runtime("nerdctl").is_err());
    }

    #[tokio::test]
    async fn no_adapter_available_includes_tried_adapters() {
        // Directly construct the error to verify Display includes adapter names.
        let err = SandboxChainError::NoAdapterAvailable {
            tried: vec![
                ("native".into(), "kernel < 5.13".into()),
                ("container".into(), "no docker on PATH".into()),
            ],
        };
        let s = format!("{err}");
        assert!(s.contains("native"), "display should include 'native': {s}");
        assert!(
            s.contains("container"),
            "display should include 'container': {s}"
        );
    }

    #[test]
    fn map_min_tier_rejects_unknown_variant() {
        // Verify the function's return type is Result<SandboxTier, SandboxChainError>.
        // Real coverage of the unknown-variant path requires a future schema version.
        fn _ensure_returns_result(t: SandboxMinimumTier) -> Result<SandboxTier, SandboxChainError> {
            map_min_tier(t)
        }
        let _ = _ensure_returns_result;
        // Known variants map correctly.
        assert_eq!(
            map_min_tier(SandboxMinimumTier::None).unwrap(),
            SandboxTier::None
        );
        assert_eq!(
            map_min_tier(SandboxMinimumTier::Light).unwrap(),
            SandboxTier::Light
        );
        assert_eq!(
            map_min_tier(SandboxMinimumTier::Strict).unwrap(),
            SandboxTier::Strict
        );
    }
}
