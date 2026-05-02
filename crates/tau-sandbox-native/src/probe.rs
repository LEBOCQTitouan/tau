//! Linux kernel feature probe.
//!
//! Detects which sandbox tier the running kernel can support:
//! - **Strict**: landlock V1 + seccomp BPF + unprivileged user namespaces.
//! - **Light**: landlock V1 only.
//! - **Unavailable**: landlock V1 missing (kernel < 5.13).
//!
//! The probe does NOT call `restrict_self()` and does NOT install any filter,
//! so it has no side-effects on the calling process.
//!
//! # Simplification note (v0.1)
//! - seccomp BPF is assumed available on any Linux kernel ≥ 3.5 (all current
//!   production kernels qualify). No explicit `prctl(PR_GET_SECCOMP)` probe is
//!   performed.
//! - Unprivileged user namespaces are assumed available unless the file
//!   `/proc/sys/kernel/unprivileged_userns_clone` exists and contains `0`.
//!   On kernels that do not have this sysctl (i.e. vanilla upstream), unprivileged
//!   user namespaces are enabled by default. A future task can add a fork-based
//!   probe for more precise detection.

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
        // Strict needs landlock + seccomp + user namespaces.
        // seccomp is assumed available (Linux 3.5+, see module-level note).
        // User namespaces are checked via unprivileged_userns_clone sysctl.
        SandboxTier::Strict => {
            if user_ns_supported() {
                SandboxTier::Strict
            } else {
                tracing::info!("unprivileged user namespaces disabled; capping Strict -> Light");
                SandboxTier::Light
            }
        }
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
        details: format!("landlock V1 ok, effective tier: {effective:?}"),
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

/// Check whether unprivileged user namespaces are available.
///
/// # Simplification (v0.1)
/// Reads `/proc/sys/kernel/unprivileged_userns_clone`. If the file exists and
/// contains `0`, user namespaces are disabled. On kernels that do not expose
/// this sysctl (mainline upstream), the file is absent and we conservatively
/// assume user namespaces are enabled. A full probe would `fork()` a child and
/// attempt `unshare(CLONE_NEWUSER)`, but that adds latency and complexity.
fn user_ns_supported() -> bool {
    match std::fs::read_to_string("/proc/sys/kernel/unprivileged_userns_clone") {
        Ok(contents) => contents.trim() != "0",
        // File absent: sysctl not present → assume enabled (mainline default).
        Err(_) => true,
    }
}
