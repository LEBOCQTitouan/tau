//! Linux native sandbox adapter for tau.
//!
//! Implements [`tau_ports::Sandbox`] using:
//! - **landlock** (kernel 5.13+) for filesystem path isolation,
//! - **seccompiler** for syscall filtering (Strict tier — Task 4),
//! - **nix unshare** for user/network namespaces (Strict tier — Task 5).
//!
//! On non-Linux hosts the adapter exists but `probe()` returns
//! `SandboxProbe::Unavailable` and all other methods return
//! `SandboxError::Unavailable`.

#![deny(missing_docs)]

mod shape;

#[cfg(target_os = "linux")]
mod light;
#[cfg(target_os = "linux")]
mod probe;

#[cfg(not(target_os = "linux"))]
mod stub;

use std::process::Command;

use tau_domain::CapabilityShapeSet;
use tau_ports::{Sandbox, SandboxError, SandboxHandle, SandboxPlan, SandboxProbe, SandboxTier};

/// Linux native sandbox adapter. Probe-driven: at construction time the
/// adapter is inert; calling [`Sandbox::probe`] discovers what the host
/// kernel can offer and the runtime caches the result.
pub struct NativeSandbox {
    name: String,
    // Used in #[cfg(target_os = "linux")] branches; suppress dead_code on other platforms.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    requested_tier: SandboxTier,
}

impl NativeSandbox {
    /// Construct an adapter that will deliver up to the given tier. The
    /// effective tier is `min(requested_tier, probe_tier)`.
    pub fn new(name: impl Into<String>, requested_tier: SandboxTier) -> Self {
        Self {
            name: name.into(),
            requested_tier,
        }
    }
}

impl Sandbox for NativeSandbox {
    fn name(&self) -> &str {
        &self.name
    }

    async fn probe(&self) -> SandboxProbe {
        #[cfg(target_os = "linux")]
        {
            probe::probe(self.requested_tier).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            stub::unavailable_probe()
        }
    }

    fn supported_shapes(&self) -> CapabilityShapeSet {
        #[cfg(target_os = "linux")]
        {
            shape::shapes_for_tier(self.requested_tier)
        }
        #[cfg(not(target_os = "linux"))]
        {
            CapabilityShapeSet::new()
        }
    }

    fn validate_plan(&self, plan: &SandboxPlan) -> Result<(), SandboxError> {
        let supported = self.supported_shapes();
        if supported.is_empty() {
            return Err(SandboxError::Unavailable {
                reason: "tau-sandbox-native requires Linux".into(),
            });
        }
        for cap in &plan.capabilities {
            let shape = cap.required_shape();
            if !supported.contains(&shape) {
                return Err(SandboxError::ShapeUnsupported { shape });
            }
        }
        Ok(())
    }

    async fn wrap_spawn(
        &self,
        plan: &SandboxPlan,
        cmd: &mut Command,
    ) -> Result<SandboxHandle, SandboxError> {
        self.validate_plan(plan)?;
        #[cfg(target_os = "linux")]
        {
            match self.requested_tier {
                SandboxTier::Light | SandboxTier::Strict => light::apply_landlock(plan, cmd),
                SandboxTier::None => Ok(SandboxHandle::noop()),
                other => Err(SandboxError::Unsupported {
                    what: format!("tier {other:?} not implemented"),
                }),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (plan, cmd);
            Err(SandboxError::Unavailable {
                reason: "tau-sandbox-native requires Linux".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use tau_domain::CapabilityShape;

    #[test]
    fn name_and_tier_round_trip() {
        let s = NativeSandbox::new("native-light", SandboxTier::Light);
        assert_eq!(s.name(), "native-light");
    }

    #[test]
    fn supported_shapes_light_includes_fs() {
        let s = NativeSandbox::new("n", SandboxTier::Light);
        let supported = s.supported_shapes();
        #[cfg(target_os = "linux")]
        {
            assert!(supported.contains(&CapabilityShape::FilesystemRead));
            assert!(supported.contains(&CapabilityShape::FilesystemWrite));
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert!(supported.is_empty());
        }
    }

    #[test]
    fn validate_plan_rejects_unsupported_shape_at_light_tier() {
        let s = NativeSandbox::new("n", SandboxTier::Light);
        // Use serde JSON round-trip to construct a Custom (non-exhaustive
        // variants block direct struct-literal construction).
        let plan_json = serde_json::json!({
            "capabilities": [{ "kind": "weird" }],
            "context": null,
            "limits": null,
        });
        let plan: SandboxPlan = serde_json::from_value(plan_json).expect("decode");
        let err = s.validate_plan(&plan).expect_err("must reject");
        #[cfg(target_os = "linux")]
        assert!(matches!(err, SandboxError::ShapeUnsupported { .. }));
        #[cfg(not(target_os = "linux"))]
        assert!(matches!(err, SandboxError::Unavailable { .. }));
    }

    #[tokio::test]
    async fn probe_on_non_linux_is_unavailable() {
        #[cfg(not(target_os = "linux"))]
        {
            let s = NativeSandbox::new("n", SandboxTier::Light);
            let p = s.probe().await;
            assert!(matches!(p, SandboxProbe::Unavailable { .. }));
        }
    }

    #[test]
    fn validate_plan_unavailable_on_non_linux() {
        #[cfg(not(target_os = "linux"))]
        {
            let s = NativeSandbox::new("n", SandboxTier::Light);
            let plan_json = serde_json::json!({
                "capabilities": [{ "kind": "fs.read", "paths": ["/tmp"] }],
                "context": null,
                "limits": null,
            });
            let plan: SandboxPlan = serde_json::from_value(plan_json).expect("decode");
            assert!(matches!(
                s.validate_plan(&plan),
                Err(SandboxError::Unavailable { .. })
            ));
        }
    }

    #[test]
    fn shapes_strict_tier_includes_exec_and_net() {
        let s = NativeSandbox::new("n", SandboxTier::Strict);
        let supported = s.supported_shapes();
        #[cfg(target_os = "linux")]
        {
            assert!(supported.contains(&CapabilityShape::ProcessExec));
            assert!(supported.contains(&CapabilityShape::NetworkHttp));
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert!(supported.is_empty());
        }
    }
}
