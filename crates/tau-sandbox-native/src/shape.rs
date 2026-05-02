//! Map [`tau_domain::CapabilityShape`] onto the set this adapter supports
//! at a given tier.

use tau_domain::{CapabilityShape, CapabilityShapeSet};
use tau_ports::SandboxTier;

/// Capability shapes this adapter can enforce at the given tier.
// Called only on Linux; suppress dead_code lint on other platforms.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn shapes_for_tier(tier: SandboxTier) -> CapabilityShapeSet {
    let mut set = CapabilityShapeSet::new();
    match tier {
        SandboxTier::None => {}
        SandboxTier::Light => {
            // Light tier: filesystem isolation only.
            set.insert(CapabilityShape::FilesystemRead);
            set.insert(CapabilityShape::FilesystemWrite);
        }
        SandboxTier::Strict => {
            // Strict tier (Tasks 4-5): adds exec gating + network egress.
            set.insert(CapabilityShape::FilesystemRead);
            set.insert(CapabilityShape::FilesystemWrite);
            set.insert(CapabilityShape::ProcessExec);
            set.insert(CapabilityShape::NetworkHttp);
        }
        other => {
            tracing::warn!(?other, "unknown SandboxTier — returning empty shape set");
        }
    }
    set
}
