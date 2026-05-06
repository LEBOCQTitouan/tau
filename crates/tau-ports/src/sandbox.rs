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
///
/// Sub-project F task 6.5 adds two fields:
/// - `sync_write_fd`: when set, [`SandboxHandle::signal_post_spawn_complete`]
///   writes 1 byte to it to release the child from a sync-pipe block in
///   pre_exec. NativeSandbox sets this when applying per-host filtering.
///   The `OwnedFd` is closed on drop if not consumed by `signal_post_spawn_complete`.
/// - `nested`: drop guards that run LIFO before the main cleanup closure.
///   NativeSandbox uses this to nest a `NetFilterHandle` whose Drop calls
///   `ip link del`.
#[non_exhaustive]
pub struct SandboxHandle {
    cleanup: Option<Box<dyn FnOnce() + Send + 'static>>,
    sync_write_fd: Option<std::os::fd::OwnedFd>,
    nested: Vec<Box<dyn Send>>,
}

impl SandboxHandle {
    /// Construct a handle from an adapter-defined cleanup closure.
    /// The closure runs exactly once when the handle is dropped.
    pub fn new<F: FnOnce() + Send + 'static>(cleanup: F) -> Self {
        Self {
            cleanup: Some(Box::new(cleanup)),
            sync_write_fd: None,
            nested: Vec::new(),
        }
    }

    /// A handle that releases nothing (mock / no-op).
    pub fn noop() -> Self {
        Self {
            cleanup: None,
            sync_write_fd: None,
            nested: Vec::new(),
        }
    }

    /// Encode the sync-pipe write fd. Used by NativeSandbox when applying
    /// per-host network filtering (sub-project F task 6.5).
    ///
    /// Takes ownership of the `OwnedFd`; the fd will be closed either by
    /// [`SandboxHandle::signal_post_spawn_complete`] (after writing 1 byte)
    /// or by the `Drop` impl (without writing, causing child to read EOF).
    pub fn with_sync_write_fd(mut self, fd: std::os::fd::OwnedFd) -> Self {
        self.sync_write_fd = Some(fd);
        self
    }

    /// Read the raw fd value from the encoded sync_write_fd, if any.
    /// Used by adapter implementations that need to inspect or duplicate the fd.
    pub fn sync_write_fd_value(&self) -> Option<std::os::fd::RawFd> {
        use std::os::fd::AsRawFd;
        self.sync_write_fd.as_ref().map(|fd| fd.as_raw_fd())
    }

    /// Add a drop guard nested inside this handle's lifetime.
    ///
    /// Drop order: nested guards drop LIFO (latest-attached drops first)
    /// before the main cleanup closure. NativeSandbox uses this to nest
    /// a `NetFilterHandle` whose Drop runs `ip link del <veth-host>`.
    pub fn nest_handle(&mut self, guard: Box<dyn Send>) {
        self.nested.push(guard);
    }

    /// Release the child from its pre_exec sync-pipe block by writing 1
    /// byte to the encoded sync_write_fd, then closing the fd.
    ///
    /// Idempotent. No-op for handles without a sync_write_fd. Returns
    /// `Err` if the write fails (e.g., child already exited).
    pub fn signal_post_spawn_complete(&mut self) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(owned_fd) = self.sync_write_fd.take() {
            // Convert OwnedFd → File (safe: File::from is a From impl).
            let mut file = std::fs::File::from(owned_fd);
            // Write exactly 1 byte; file is dropped (fd closed) afterwards.
            file.write_all(&[0u8])?;
        }
        Ok(())
    }
}

impl Drop for SandboxHandle {
    fn drop(&mut self) {
        // Defensive: if signal_post_spawn_complete wasn't called and there's
        // an OwnedFd remaining, drop it (closes the fd without writing).
        // The child reads EOF in pre_exec and returns an error; the spawn's
        // wait() reaps the child.
        drop(self.sync_write_fd.take());

        // Drop nested guards LIFO (latest-attached drops first).
        for guard in self.nested.drain(..).rev() {
            drop(guard);
        }

        // Run main cleanup closure.
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

    /// Adapter-specific post-spawn setup. Called by the runtime after
    /// `cmd.spawn()` succeeds and the child PID is known.
    ///
    /// Default: no-op. Mock + Container adapters use the default.
    /// NativeSandbox (Linux) applies per-host nftables filtering inside
    /// the child's netns when the plan has `Capability::Network(Http)`.
    ///
    /// On `Ok(())`: the caller MUST call
    /// [`SandboxHandle::signal_post_spawn_complete`] to release the child
    /// from its sync-pipe block in pre_exec.
    /// On `Err(_)`: the caller drops `handle` (which dismisses sync_write_fd
    /// → child reads EOF → exits cleanly) and reaps the child via wait().
    async fn apply_post_spawn(
        &self,
        plan: &SandboxPlan,
        child_pid: i32,
        handle: &mut SandboxHandle,
    ) -> Result<(), SandboxError> {
        let _ = (plan, child_pid, handle);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nest_handle_drops_in_lifo_order() {
        use std::sync::{Arc, Mutex};

        let order: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let order_main = Arc::clone(&order);

        let mut handle = SandboxHandle::new(move || {
            order_main.lock().unwrap().push("main_cleanup");
        });

        // Add 2 nested guards. Each pushes its label on Drop.
        struct Guard(Arc<Mutex<Vec<&'static str>>>, &'static str);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.lock().unwrap().push(self.1);
            }
        }
        handle.nest_handle(Box::new(Guard(Arc::clone(&order), "first_nested")));
        handle.nest_handle(Box::new(Guard(Arc::clone(&order), "second_nested")));

        drop(handle);

        // Expected order: LIFO of nested, then main cleanup.
        assert_eq!(
            *order.lock().unwrap(),
            vec!["second_nested", "first_nested", "main_cleanup"]
        );
    }

    #[test]
    fn signal_post_spawn_complete_is_noop_without_fd() {
        let mut handle = SandboxHandle::new(|| {});
        // No sync_write_fd set; signal should succeed no-op.
        handle.signal_post_spawn_complete().expect("noop signal");
    }
}
