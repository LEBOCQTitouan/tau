//! Sandbox port — `kind = "sandbox"` plugin contracts.
//!
//! **PROVISIONAL** — the v0.1 sandbox surface is a sketch. Phase-1
//! implementation work (WASM, OS-native, container) will likely
//! require breaking changes. Treat as forward-compatible documentation,
//! not a SemVer commitment beyond the major-version bump that
//! introduces actual sandboxing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use tau_domain::Capability;

use crate::error::SandboxError;

/// Plan provided to [`Sandbox::create`].
///
/// **PROVISIONAL** — see module-level caveat.
#[non_exhaustive]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SandboxPlan {
    /// Capabilities the sandboxed code is allowed to exercise.
    /// The runtime composes this from the package's manifest before
    /// calling `create`.
    pub capabilities: Vec<Capability>,
    /// Optional working-context hint (working dir + env). OS-native
    /// sandboxes use; WASM sandboxes typically ignore.
    pub context: Option<WorkingContext>,
    /// Optional resource limits.
    pub limits: Option<ResourceLimits>,
}

/// Working-context hint for the sandboxed execution.
///
/// **PROVISIONAL** — see module-level caveat.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorkingContext {
    /// Working directory hint. OS-native sandboxes use; WASM ignores.
    pub working_dir: Option<PathBuf>,
    /// Environment variables to seed the sandboxed context.
    pub env: BTreeMap<String, String>,
}

/// Resource limits for the sandboxed execution.
///
/// **PROVISIONAL** — see module-level caveat.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ResourceLimits {
    /// Maximum memory, in bytes.
    pub memory_bytes: Option<u64>,
    /// Maximum CPU time, in seconds.
    pub cpu_seconds: Option<u32>,
    /// Maximum wall-clock time, in seconds. Different from cpu_seconds:
    /// agents that block on I/O don't accumulate CPU time but still
    /// should be killable.
    pub wall_clock_seconds: Option<u32>,
    /// Maximum concurrent subprocesses (OS-native; WASM impls ignore).
    pub max_subprocesses: Option<u32>,
}

/// Trait implemented by `kind = "sandbox"` plugins.
///
/// **PROVISIONAL** — this trait is a v0.1 sketch for plugin authors to
/// anticipate the shape Phase-1 sandboxing will take. The actual
/// implementation (WASM, OS-native, container) is not yet picked, and
/// when it lands, breaking changes to this trait surface are likely.
/// Treat as forward-compatible documentation, not a SemVer commitment
/// beyond the major-version bump that introduces actual sandboxing.
///
/// At v0.1 there are zero implementations; this trait exists so plugin
/// authors writing for v0.1 can anticipate the shape Phase-1 sandboxing
/// will take.
#[allow(async_fn_in_trait)]
pub trait Sandbox: Send + Sync {
    /// Per-sandbox handle. Opaque at v0.1; Phase-1 implementations may
    /// add methods or trait-bound additional behavior.
    type Handle: Send + 'static;

    /// Plugin-visible name (matches the package name; for diagnostics).
    fn name(&self) -> &str;

    /// Provision a sandboxed execution context with the given plan.
    /// Returns an opaque handle whose meaning is implementation-defined.
    async fn create(&self, plan: SandboxPlan) -> Result<Self::Handle, SandboxError>;
}
