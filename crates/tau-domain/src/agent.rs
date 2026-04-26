//! Agent definition + lifecycle status types.
//!
//! tau-domain holds the *vocabulary* (identity, definition, status enum).
//! State-machine *transitions* live in tau-runtime — see G2 / spec §3.4.

/// Agent lifecycle status. Carries diagnostic data only on `Failed`;
/// transition rules live in tau-runtime.
///
/// State graph (informational; not enforced here):
/// `Declared → Installed → Ready → Running ↔ Stopped`,
/// with `Failed` reachable from any non-terminal state.
///
/// # Example
///
/// ```ignore
/// // E0639: variant-level `#[non_exhaustive]` blocks struct-literal
/// // construction from outside the crate. Internal callers (and the
/// // unit test in this module) construct `Failed { .. }` directly.
/// use tau_domain::{AgentStatus, FailureKind};
/// let s = AgentStatus::Failed {
///     kind: FailureKind::BackendError,
///     detail: Some("connection refused: api.openai.com".into()),
/// };
/// match s {
///     AgentStatus::Failed { kind: FailureKind::BackendError, .. } => {
///         // retry with backoff
///     }
///     _ => {}
/// }
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AgentStatus {
    /// Manifest seen, package not yet installed.
    Declared,
    /// Package installed on disk, ready to instantiate.
    Installed,
    /// Instance created, idle.
    Ready,
    /// Actively processing a message.
    Running,
    /// Intentionally halted.
    Stopped,
    /// The agent failed. `kind` enables typed retry/restart logic;
    /// `detail` carries human-readable specifics.
    #[non_exhaustive]
    Failed {
        /// Typed failure category for restart logic.
        kind: FailureKind,
        /// Human-readable detail (e.g. `"panic at src/foo.rs:42"`).
        detail: Option<String>,
    },
}

/// Categorical failure kinds. New typed kinds are added additively;
/// `InternalError` is the catch-all escape hatch.
///
/// See: [escape-hatches.md#failurekind-internalerror](../docs/explanation/escape-hatches.md#failurekind-internalerror).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FailureKind {
    /// Agent process crashed unexpectedly (panic, signal, abort).
    Crashed,
    /// Configured LLM backend returned an error or was unreachable.
    BackendError,
    /// A capability check denied an operation the agent attempted.
    PolicyDenied,
    /// Agent exceeded a resource limit (memory, message rate, timeout).
    OutOfResources,
    /// Catch-all for failures that don't match the named kinds.
    /// See: [escape-hatches.md#failurekind-internalerror](../docs/explanation/escape-hatches.md#failurekind-internalerror).
    InternalError,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_can_carry_detail() {
        let s = AgentStatus::Failed {
            kind: FailureKind::Crashed,
            detail: Some("SIGSEGV".into()),
        };
        match s {
            AgentStatus::Failed {
                kind: FailureKind::Crashed,
                detail: Some(d),
            } => {
                assert_eq!(d, "SIGSEGV");
            }
            _ => panic!(),
        }
    }
}
