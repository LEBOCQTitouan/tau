//! Sandbox port — `kind = "sandbox"` plugin contracts.
//!
//! **PROVISIONAL** — the v0.1 sandbox surface is a sketch. Phase-1
//! implementation work (WASM, OS-native, container) will likely
//! require breaking changes. Treat as forward-compatible documentation,
//! not a SemVer commitment beyond the major-version bump that
//! introduces actual sandboxing.
//!
//! Sandbox trait lands in T13.

use std::collections::BTreeMap;
use std::path::PathBuf;

use tau_domain::Capability;

/// Plan provided to `crate::sandbox::Sandbox::create` (T13).
///
/// **PROVISIONAL** — see module-level caveat.
#[non_exhaustive]
#[derive(Debug, Clone)]
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
