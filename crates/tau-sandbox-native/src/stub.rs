//! Non-Linux fallback: every probe returns Unavailable.

use tau_ports::SandboxProbe;

#[allow(dead_code)] // Used only on non-Linux.
pub(crate) fn unavailable_probe() -> SandboxProbe {
    SandboxProbe::Unavailable {
        reason: "tau-sandbox-native requires Linux".into(),
    }
}
