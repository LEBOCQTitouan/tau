//! Per-operation typed errors for `tau-runtime`.
//!
//! All public errors are `#[non_exhaustive]` so additive variants are
//! non-breaking. All errors derive `Debug + Clone + PartialEq + Eq +
//! Error`. Tests with free-form `String` fields use `matches!()` to
//! avoid brittle wording comparisons.
//!
//! The error taxonomy splits into two layers:
//!
//! - [`BuildError`] — failures during `RuntimeBuilder::build()`. The
//!   runtime never gets constructed.
//! - `RuntimeError` (added in Task 4) — kernel-level operational
//!   failures during `Runtime::run`. Composes `tau_ports` plugin errors
//!   via `#[from]`. Agent-level failures (capability denied, max turns
//!   reached) are reported via `Ok(RunOutcome::Failed { status:
//!   AgentStatus::Failed })`, NOT `Err(RuntimeError)`.
//!
//! [`CapabilityDenial`] is a helper type embedded as the `detail`
//! string of `AgentStatus::Failed { kind: PolicyDenied }` when
//! capability enforcement rejects a tool call. It is NOT a variant
//! of `RuntimeError`.

use thiserror::Error;

/// Tag identifying a plugin kind in error messages and tracing fields.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginKind {
    /// LLM backend plugin (`kind = "llm-backend"`).
    LlmBackend,
    /// Tool plugin (`kind = "tool"`).
    Tool,
    /// Storage plugin (`kind = "storage"`).
    Storage,
    /// Sandbox plugin (`kind = "sandbox"`); reserved for forward compat.
    Sandbox,
}

impl std::fmt::Display for PluginKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginKind::LlmBackend => f.write_str("llm-backend"),
            PluginKind::Tool => f.write_str("tool"),
            PluginKind::Storage => f.write_str("storage"),
            PluginKind::Sandbox => f.write_str("sandbox"),
        }
    }
}

/// Errors from `RuntimeBuilder::build()` (added in Task 7).
///
/// # Example
///
/// ```ignore
/// // `BuildError` is `#[non_exhaustive]`; constructed by `build()`.
/// // Construction example deferred to Task 7 when the builder lands.
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuildError {
    /// At least one LLM backend must be registered before `build()`.
    #[error("no LLM backends registered; at least one is required")]
    NoLlmBackend,

    /// Two plugins of the same kind registered with the same `name()`.
    #[error("name collision: two {kind}s registered as {name:?}")]
    NameCollision {
        /// Which plugin kind collided.
        kind: PluginKind,
        /// The colliding name.
        name: String,
    },

    /// Catch-all for invariant violations during build.
    /// See: [escape-hatches.md#builderror-internal](../docs/explanation/escape-hatches.md#builderror-internal).
    #[error("internal: {message}")]
    Internal {
        /// Human-readable message describing the internal failure.
        message: String,
    },
}

/// Capability-denial detail. Embedded as the `detail` string of
/// `AgentStatus::Failed { kind: PolicyDenied, .. }` when capability
/// enforcement rejects a tool call.
///
/// NOT a variant of `RuntimeError` (added in Task 4) — capability
/// denial is an agent-level failure (`Ok(RunOutcome::Failed)`), not
/// a kernel-level error (`Err(RuntimeError)`). See ADR-0006 for the
/// dichotomy.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDenial {
    /// `AgentDefinition::id` formatted via `Display`.
    pub agent_id: String,
    /// `AgentDefinition::package` formatted via `Display`.
    pub package_id: String,
    /// The tool the agent attempted to call.
    pub tool_name: String,
    /// Top-level kind of the missing capability ("filesystem.read",
    /// "network.http", "tool.echo" — convention).
    pub required_kind: String,
    /// Human-readable description of the capability that wasn't satisfied.
    pub required_detail: String,
}

impl std::fmt::Display for CapabilityDenial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "agent {} (package {}) lacks capability `{}` ({}) required to call tool `{}`",
            self.agent_id,
            self.package_id,
            self.required_kind,
            self.required_detail,
            self.tool_name,
        )
    }
}

/// Errors from `Runtime::run` (added in Task 10) — kernel-level
/// operational failures.
///
/// Agent-level failures (capability denied, max turns reached) flow
/// through `Ok(RunOutcome::Failed { status: AgentStatus::Failed{..} })`
/// instead. See [`crate::error`] module-level docs for the dichotomy.
///
/// Plugin errors (`LlmError`, `ToolError`, `StorageError`, `SandboxError`)
/// compose via `#[from]` for ergonomic `?`-propagation throughout the
/// agent loop.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RuntimeError {
    /// Agent's `llm_backend` references a backend that wasn't registered.
    #[error("LLM backend `{backend}` not registered (agent {agent_id} requested it)")]
    LlmBackendNotRegistered {
        /// The agent's `id` formatted via `Display`.
        agent_id: String,
        /// The backend name the agent requested.
        backend: String,
    },

    /// LLM emitted a tool_use targeting a tool not in the registry.
    #[error("tool `{tool_name}` not registered; registered: {registered:?}")]
    ToolNotRegistered {
        /// The tool name the LLM requested.
        tool_name: String,
        /// Names of registered tools (for diagnostics).
        registered: Vec<String>,
    },

    /// Plugin returned successfully but its output violates the contract
    /// (malformed JSON args from LLM, undeserializable content, etc.).
    #[error("plugin contract violation: {plugin_kind} `{plugin_name}` returned malformed {what}: {detail}")]
    PluginContractViolation {
        /// The plugin kind ("llm", "tool", "storage", "sandbox").
        plugin_kind: String,
        /// The plugin name (its `name()` value).
        plugin_name: String,
        /// What was malformed ("tool_use args", "completion response", etc.).
        what: String,
        /// Human-readable detail.
        detail: String,
    },

    /// LLM backend plugin returned an error.
    #[error("llm: {0}")]
    Llm(#[from] tau_ports::LlmError),

    /// Tool plugin returned an error.
    #[error("tool: {0}")]
    Tool(#[from] tau_ports::ToolError),

    /// Storage plugin returned an error.
    #[error("storage: {0}")]
    Storage(#[from] tau_ports::StorageError),

    /// Sandbox plugin returned an error. Reserved for forward compat;
    /// v0.1 doesn't wire `Sandbox::create` (Q7=A from the spec).
    #[error("sandbox: {0}")]
    Sandbox(#[from] tau_ports::SandboxError),

    /// Manifest validation failed (caller-supplied manifest invalid).
    #[error("manifest validation: {0}")]
    Manifest(#[from] tau_domain::PackageManifestError),

    /// Catch-all for invariant violations / unexpected states.
    /// See: [escape-hatches.md#runtimeerror-internal](../docs/explanation/escape-hatches.md#runtimeerror-internal).
    #[error("internal: {message}")]
    Internal {
        /// Human-readable message describing the internal failure.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_kind_display() {
        assert_eq!(PluginKind::LlmBackend.to_string(), "llm-backend");
        assert_eq!(PluginKind::Tool.to_string(), "tool");
        assert_eq!(PluginKind::Storage.to_string(), "storage");
        assert_eq!(PluginKind::Sandbox.to_string(), "sandbox");
    }

    #[test]
    fn build_error_no_llm_backend_display() {
        let err = BuildError::NoLlmBackend;
        let s = format!("{err}");
        assert!(s.contains("no LLM backends registered"), "got: {s}");
    }

    #[test]
    fn build_error_name_collision_display() {
        let err = BuildError::NameCollision {
            kind: PluginKind::Tool,
            name: "echo".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("name collision"), "got: {s}");
        assert!(s.contains("tool"), "got: {s}");
        assert!(s.contains("echo"), "got: {s}");
    }

    #[test]
    fn capability_denial_display_includes_all_fields() {
        let denial = CapabilityDenial {
            agent_id: "agent-x".into(),
            package_id: "pkg/y@1.0.0".into(),
            tool_name: "file_read".into(),
            required_kind: "filesystem.read".into(),
            required_detail: "/etc/passwd".into(),
        };
        let s = format!("{denial}");
        assert!(s.contains("agent-x"), "got: {s}");
        assert!(s.contains("pkg/y@1.0.0"), "got: {s}");
        assert!(s.contains("filesystem.read"), "got: {s}");
        assert!(s.contains("/etc/passwd"), "got: {s}");
        assert!(s.contains("file_read"), "got: {s}");
    }

    use tau_ports::{LlmError, StorageError, ToolError};

    #[test]
    fn runtime_error_llm_backend_not_registered_display() {
        let err = RuntimeError::LlmBackendNotRegistered {
            agent_id: "agent-1".into(),
            backend: "anthropic".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("anthropic"), "got: {s}");
        assert!(s.contains("agent-1"), "got: {s}");
        assert!(s.contains("not registered"), "got: {s}");
    }

    #[test]
    fn runtime_error_tool_not_registered_display() {
        let err = RuntimeError::ToolNotRegistered {
            tool_name: "ghost".into(),
            registered: vec!["echo".into(), "file_read".into()],
        };
        let s = format!("{err}");
        assert!(s.contains("ghost"), "got: {s}");
        assert!(s.contains("echo"), "got: {s}");
        assert!(s.contains("file_read"), "got: {s}");
    }

    #[test]
    fn runtime_error_plugin_contract_violation_display() {
        let err = RuntimeError::PluginContractViolation {
            plugin_kind: "llm".into(),
            plugin_name: "anthropic".into(),
            what: "tool_use args".into(),
            detail: "expected JSON object, got array".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("plugin contract violation"), "got: {s}");
        assert!(s.contains("anthropic"), "got: {s}");
        assert!(s.contains("tool_use args"), "got: {s}");
    }

    #[test]
    fn runtime_error_composes_llm_via_from() {
        let llm_err = LlmError::Internal {
            message: "x".into(),
        };
        let runtime_err: RuntimeError = llm_err.into();
        assert!(matches!(
            runtime_err,
            RuntimeError::Llm(LlmError::Internal { .. })
        ));
    }

    #[test]
    fn runtime_error_composes_tool_via_from() {
        let tool_err = ToolError::Internal {
            message: "x".into(),
        };
        let runtime_err: RuntimeError = tool_err.into();
        assert!(matches!(
            runtime_err,
            RuntimeError::Tool(ToolError::Internal { .. })
        ));
    }

    #[test]
    fn runtime_error_composes_storage_via_from() {
        let storage_err = StorageError::Internal {
            message: "x".into(),
        };
        let runtime_err: RuntimeError = storage_err.into();
        assert!(matches!(
            runtime_err,
            RuntimeError::Storage(StorageError::Internal { .. })
        ));
    }

    #[test]
    fn runtime_error_composes_sandbox_via_from() {
        use tau_ports::SandboxError;
        let sandbox_err = SandboxError::Internal {
            message: "x".into(),
        };
        let runtime_err: RuntimeError = sandbox_err.into();
        assert!(matches!(
            runtime_err,
            RuntimeError::Sandbox(SandboxError::Internal { .. })
        ));
    }

    #[test]
    fn runtime_error_internal_display() {
        let err = RuntimeError::Internal {
            message: "unexpected".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("internal"), "got: {s}");
        assert!(s.contains("unexpected"), "got: {s}");
    }
}
