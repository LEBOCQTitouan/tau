//! Sandbox port — the `tau_ports::Sandbox` trait + supporting types.
//!
//! Hexagonal port: `tau-runtime` consumes this trait; `tau-sandbox-native`,
//! `tau-sandbox-container`, and `MockSandbox` (in [`crate::fixtures`])
//! implement it. The runtime selects an adapter via a probe-based chain
//! configured in `<scope>/.tau/config.toml`.
//!
//! Stable as of v0.1 of the sandboxing sub-project. Variant evolution is
//! handled by `#[non_exhaustive]` on every public type.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use tau_domain::{Capability, CapabilityShapeSet};

use crate::error::SandboxError;

/// Plan provided to [`Sandbox::wrap_spawn`].
#[non_exhaustive]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SandboxPlan {
    /// Capabilities the sandboxed code is allowed to exercise. The runtime
    /// composes this from the package's `compute_effective` capability set
    /// before calling `wrap_spawn`.
    pub capabilities: Vec<Capability>,
    /// Optional working-context hint (working dir + env).
    pub context: Option<WorkingContext>,
    /// Optional resource limits.
    pub limits: Option<ResourceLimits>,
}

impl SandboxPlan {
    /// Construct a [`SandboxPlan`].
    ///
    /// `#[non_exhaustive]` blocks struct-literal construction outside
    /// `tau-ports`; use this constructor instead.
    pub fn new(
        capabilities: Vec<Capability>,
        context: Option<WorkingContext>,
        limits: Option<ResourceLimits>,
    ) -> Self {
        Self {
            capabilities,
            context,
            limits,
        }
    }
}

/// Working-context hint for the sandboxed execution.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WorkingContext {
    /// Working directory hint.
    pub working_dir: Option<PathBuf>,
    /// Environment variables to seed the sandboxed context.
    pub env: BTreeMap<String, String>,
}

/// Resource limits for the sandboxed execution.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ResourceLimits {
    /// Maximum memory, in bytes.
    pub memory_bytes: Option<u64>,
    /// Maximum CPU time, in seconds.
    pub cpu_seconds: Option<u32>,
    /// Maximum wall-clock time, in seconds.
    pub wall_clock_seconds: Option<u32>,
    /// Maximum concurrent subprocesses.
    pub max_subprocesses: Option<u32>,
}

/// Probe result describing an adapter's runtime availability.
#[non_exhaustive]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SandboxProbe {
    /// Adapter is usable on this host with the indicated tier.
    Available {
        /// Best tier the adapter can guarantee right now.
        tier: SandboxTier,
        /// Free-form diagnostic ("landlock V1; seccomp BPF; user_ns ok").
        details: String,
    },
    /// Adapter is not usable on this host.
    Unavailable {
        /// Human-readable reason ("kernel < 5.13", "no docker on PATH").
        reason: String,
    },
}

/// Enforcement tier an adapter can deliver. Forms a total order: `None` <
/// `Light` < `Strict`. Higher tiers are stricter; project config can RAISE
/// but never WEAKEN the tier the adapter advertises.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SandboxTier {
    /// No enforcement (only valid for the mock adapter).
    None,
    /// Filesystem isolation only (e.g. landlock without seccomp).
    Light,
    /// Filesystem + syscall + namespace isolation (full Strict tier).
    Strict,
}

/// Opaque handle returned by [`Sandbox::wrap_spawn`]. Drops automatically
/// release any resources the adapter holds (e.g. cgroup, namespace fd).
#[non_exhaustive]
pub struct SandboxHandle {
    cleanup: Option<Box<dyn FnOnce() + Send + 'static>>,
}

impl SandboxHandle {
    /// Construct a handle from an adapter-defined cleanup closure.
    /// The closure runs exactly once when the handle is dropped.
    pub fn new<F: FnOnce() + Send + 'static>(cleanup: F) -> Self {
        Self {
            cleanup: Some(Box::new(cleanup)),
        }
    }

    /// A handle that releases nothing (mock / no-op).
    pub fn noop() -> Self {
        Self { cleanup: None }
    }
}

impl Drop for SandboxHandle {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

impl std::fmt::Debug for SandboxHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SandboxHandle").finish_non_exhaustive()
    }
}

/// Trait implemented by sandbox adapters. The runtime calls these methods
/// in this order:
///
/// 1. [`Sandbox::probe`] at startup (cached) — discover what the adapter can do.
/// 2. [`Sandbox::supported_shapes`] for static cross-checks.
/// 3. [`Sandbox::validate_plan`] before spawning a plugin process.
/// 4. [`Sandbox::wrap_spawn`] applies sandbox enforcement to a `Command`.
#[allow(async_fn_in_trait)]
pub trait Sandbox: Send + Sync {
    /// Plugin-visible name (matches the package name; for diagnostics).
    fn name(&self) -> &str;

    /// Probe the host for adapter availability. Cached by the runtime.
    async fn probe(&self) -> SandboxProbe;

    /// Capability shapes this adapter can enforce. Used at install time
    /// (Layer 2) and at `tau check` time (Layer 3) to refuse plans this
    /// adapter cannot honor.
    fn supported_shapes(&self) -> CapabilityShapeSet;

    /// Validate that this plan can be executed by this adapter.
    /// Returns `Err(SandboxError::ShapeUnsupported)` if any required shape
    /// is not in [`Sandbox::supported_shapes`].
    fn validate_plan(&self, plan: &SandboxPlan) -> Result<(), SandboxError>;

    /// Apply sandbox enforcement to a [`Command`] in preparation for spawn.
    /// On Linux native, this typically registers `pre_exec` hooks. The
    /// returned [`SandboxHandle`] holds any ambient resources (cgroup,
    /// namespace fd) and releases them on drop.
    async fn wrap_spawn(
        &self,
        plan: &SandboxPlan,
        cmd: &mut Command,
    ) -> Result<SandboxHandle, SandboxError>;
}
