//! Linux kernel feature probe.
//!
//! Uses the landlock crate to check whether the running kernel supports
//! Landlock V1 (Linux 5.13+). The probe does NOT call `restrict_self()` so
//! it has no side-effects on the calling process.

use tau_ports::{SandboxProbe, SandboxTier};

/// Probe the host kernel for sandbox features.
///
/// Returns `Available { tier }` where `tier` is the strongest tier the
/// adapter can support, capped at the caller's requested tier.
pub(crate) async fn probe(requested: SandboxTier) -> SandboxProbe {
    if !landlock_v1_supported() {
        return SandboxProbe::Unavailable {
            reason: "landlock V1 unsupported (kernel < 5.13)".into(),
        };
    }
    let effective = match requested {
        SandboxTier::None => SandboxTier::None,
        // Light needs landlock only — already verified above.
        SandboxTier::Light => SandboxTier::Light,
        // Strict needs seccomp + namespaces — Tasks 4-5 wire those up.
        // For now (Task 3, Light tier only) we cap at Light.
        SandboxTier::Strict => SandboxTier::Light,
        // Non-exhaustive arm: unknown future tier — warn and report unavailable.
        other => {
            tracing::warn!(
                ?other,
                "unknown SandboxTier in probe — returning Unavailable"
            );
            return SandboxProbe::Unavailable {
                reason: format!("tier {other:?} not implemented"),
            };
        }
    };
    SandboxProbe::Available {
        tier: effective,
        details: format!("landlock V1 ok (cap to {effective:?})"),
    }
}

/// Check whether this kernel supports Landlock V1 by attempting to create a
/// V1 ruleset with `HardRequirement` compat level. Creating a ruleset (without
/// calling `restrict_self()`) is a side-effect-free probe: the kernel FD is
/// opened and immediately dropped, applying no restrictions to the process.
fn landlock_v1_supported() -> bool {
    use landlock::{AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, ABI};

    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(ABI::V1))
        .and_then(|r| r.create())
        .is_ok()
}
