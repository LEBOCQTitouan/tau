# Sandboxing (Tier 3 priority 12) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land OS-level sandboxing for plugin processes — Linux native (landlock + seccomp + namespaces) and container (docker/podman) adapters behind a single hexagonal `tau_ports::Sandbox` port — and a 4-layer pre-flight validation hierarchy that catches capability/sandbox mismatches at install/check time rather than at run time.

**Architecture:** Hexagonal port + adapters. The `tau_ports::Sandbox` trait is the port; `tau-sandbox-native` and `tau-sandbox-container` are real adapters; `MockSandbox` (kept in `tau-ports/src/fixtures.rs`) is the test adapter. The runtime selects an adapter via a probe-based chain (first available wins) configured in `<scope>/.tau/config.toml`. A typed `CapabilityShape` vocabulary expresses what each `Capability` requires; adapters declare which shapes they support; cross-checks at install / `tau check` / `tau resolve --check-sandbox` / spawn time eliminate "this configuration was never going to work" errors. macOS, Windows, and remote backends are explicitly out-of-scope for v0.1 of this sub-project — non-Linux hosts probe Unavailable and refuse to start with a clear platform-support message.

**Tech Stack:** Rust 2021 (workspace edition); `landlock = "0.4"`, `seccompiler = "0.5"`, `nix = "0.29"` (workspace dep) for Linux primitives; `std::process::Command` for container shell-out; `tempfile`, `assert_cmd`, `predicates` for tests; `tracing` for diagnostics. No new CI matrix slots.

---

## Plan-erratum block

Apply preemptively across all tasks:

- **Cargo.lock fixup discipline (priority-6 carryover):** any task adding a new workspace dependency MUST stage `Cargo.lock` in the same commit. This affects Tasks 3, 4 (Task 6 shells out to `docker`/`podman` and adds no Rust dep).
- **`#[non_exhaustive]` discipline:** all new public types — `CapabilityShape`, `CapabilityShapeSet`, `SandboxProbe`, `SandboxTier`, refined `SandboxError` variants — get `#[non_exhaustive]`. Doctests on `#[non_exhaustive]` types must be `ignore`-marked (same convention as the existing `Capability` enum).
- **Linux-only code via `#[cfg(target_os = "linux")]`:** the `tau-sandbox-native` crate compiles on all platforms but its real impl is gated `#[cfg(target_os = "linux")]`. On non-Linux the probe returns `SandboxProbe::Unavailable { reason: "tau-sandbox-native requires Linux" }`. No platform-specific Cargo features at v0.1.
- **CI matrix already covers Linux/macOS/Windows** (added in priorities 8/11). The sandbox tests run on the existing `ubuntu-latest` slots; macOS + Windows runners exercise the cross-platform code paths only. Tests that exercise real landlock / seccomp / namespaces are `#[cfg(target_os = "linux")]` and use `#[ignore]` + a runtime kernel-version probe to skip on kernels < 5.13.
- **`MockSandbox` stays in `tau-ports/src/fixtures.rs`.** Task 2 updates it in lockstep with the trait refactor. No re-export shim needed.
- **Existing plugins (anthropic, ollama, openai, fs-read, shell) keep working under sandbox enforcement.** Task 11 verifies their existing `tau.toml` capability declarations match the kernel's `CAPABILITIES` handshake response (Phase 0 ADR-0008 protocol) — Layer 2 cross-check uses this; the `#[capabilities(...)]` proc macro from spec §"Layer 1" is **NOT** in scope for this sub-project (deferred to a future sub-project).
- **JSON event-per-line streaming convention (ADR-0011 carryover):** `tau resolve --check-sandbox --json` emits per-line events (`{"event": "...", ...}`) following the same shape as `tau install --json`.
- **Test fixture pattern (priorities 5/6/7/11 carryover):** all CLI integration tests use `assert_cmd::Command::cargo_bin("tau")` + `tempfile::TempDir`. Mirror `crates/tau-cli/tests/cmd_resolve.rs` and `crates/tau-cli/tests/cmd_install.rs`.
- **Three-bucket exit codes (ADR-0007 §7):** sandbox configuration error → exit 2; no adapter available on this platform → exit 2 with a clear message; runtime sandbox violation by a plugin → `ToolError::SandboxViolation` (recoverable; run continues to next tool).
- **Lockfile schema migration v3 → v4:** Task 9 adds `LockedPlugin.required_shapes: Vec<CapabilityShape>` (additive only). v3 entries auto-upgrade with empty `required_shapes`; Layer 3 falls back to manifest-based shape derivation and emits a `tracing::warn!` ("required_shapes missing for plugin {id}; falling back to manifest-derived shapes — re-install to refresh"). The `--rehash` flag mentioned in spec §"Lockfile migration" is **NOT** shipped in this sub-project.
- **Per-scope config file `<scope>/.tau/config.toml`:** verify in Task 7 whether the file format is already standardized; the spec assumes it is (priorities 7/11 reference it). If it is, Task 7 extends the schema with a `[sandbox]` section. If no precedent file format exists, Task 7 introduces the file with `[sandbox]` as its first section; design for forward-compat (other future sections will be siblings).
- **Clippy 1.95.0 carryover:** the `unnecessary_sort_by` lint (priority 11 fix) is already enforced in CI. Use `sort_by_key(|x| std::cmp::Reverse(x.field))` over `sort_by(|a, b| b.field.cmp(&a.field))` everywhere.

---

## File structure

| File | Status | Responsibility |
|---|---|---|
| `crates/tau-domain/src/package/capability.rs` | modify | Add `CapabilityShape` enum + `CapabilityShapeSet` newtype + `Capability::required_shape()` helper. |
| `crates/tau-domain/src/lib.rs` | modify | Re-export `CapabilityShape`, `CapabilityShapeSet`. |
| `crates/tau-ports/src/sandbox.rs` | modify | Refine `Sandbox` trait: drop PROVISIONAL, add `probe()` / `supported_shapes()` / `validate_plan()` / `wrap_spawn()`. Add `SandboxProbe`, `SandboxTier`, `SandboxHandle`. |
| `crates/tau-ports/src/error.rs` | modify | Refine `SandboxError`: add `Unavailable`, `ShapeUnsupported`, `Violation`, `WrapFailed` variants. Drop PROVISIONAL. |
| `crates/tau-ports/src/fixtures.rs` | modify | `MockSandbox` adopts the new trait shape. |
| `crates/tau-ports/Cargo.toml` | modify | (no new deps; existing `tau-domain` re-export covers `CapabilityShape`). |
| `crates/tau-sandbox-native/` | create | New workspace member. Linux landlock + seccomp + namespace adapter. |
| `crates/tau-sandbox-native/Cargo.toml` | create | `landlock`, `seccompiler`, `nix` (workspace), `tau-domain`, `tau-ports`, `tracing`, `tokio` (workspace). |
| `crates/tau-sandbox-native/src/lib.rs` | create | Public surface: `pub struct NativeSandbox`. |
| `crates/tau-sandbox-native/src/probe.rs` | create | Kernel feature detection (landlock V1, seccomp BPF, user_namespaces). |
| `crates/tau-sandbox-native/src/light.rs` | create | Light-tier impl: landlock filesystem isolation only. |
| `crates/tau-sandbox-native/src/strict.rs` | create | Strict-tier impl: landlock + seccomp + namespaces (Tasks 4 & 5). |
| `crates/tau-sandbox-native/src/shape.rs` | create | `CapabilityShape -> SandboxRule` mapping. |
| `crates/tau-sandbox-native/src/stub.rs` | create | Non-Linux fallback: `probe()` returns `Unavailable`. |
| `crates/tau-sandbox-container/` | create | New workspace member. Docker/podman shell-out adapter. |
| `crates/tau-sandbox-container/Cargo.toml` | create | `tau-domain`, `tau-ports`, `tracing`, `tokio` — no new external deps. |
| `crates/tau-sandbox-container/src/lib.rs` | create | Public surface: `pub struct ContainerSandbox`. |
| `crates/tau-sandbox-container/src/probe.rs` | create | Detect docker / podman binary on PATH; cache result. |
| `crates/tau-sandbox-container/src/runner.rs` | create | Build `docker run` / `podman run` argv from `SandboxPlan`. |
| `crates/tau-runtime/src/sandbox/mod.rs` | create | New module — runtime sandbox glue. |
| `crates/tau-runtime/src/sandbox/chain.rs` | create | Adapter chain config + probe + selection. |
| `crates/tau-runtime/src/sandbox/plan.rs` | create | Build `SandboxPlan` from per-plugin effective capabilities. |
| `crates/tau-runtime/src/sandbox/validation.rs` | create | Layer 3 cross-check: required shapes ⊆ supported shapes. |
| `crates/tau-runtime/src/lib.rs` | modify | Re-export sandbox module. |
| `crates/tau-runtime/src/plugin_host/mod.rs` | modify | Spawn pipeline integrates `wrap_spawn`. |
| `crates/tau-pkg/src/scope.rs` | modify | Extend `ScopeConfig` with `[sandbox]` section: chain config + tier preference. |
| `crates/tau-pkg/src/lockfile.rs` | modify | Schema v3 → v4: add `LockedPlugin.required_shapes`. Migration helper for v3 entries. |
| `crates/tau-pkg/src/install.rs` | modify | Layer 2 cross-check: manifest declarations vs. binary `CAPABILITIES` handshake. |
| `crates/tau-cli/src/cmd/resolve.rs` | modify | Add `--check-sandbox` + `--json` flags. |
| `crates/tau-cli/tests/cmd_resolve_check_sandbox.rs` | create | CLI integration tests for the new flag. |
| `crates/tau-runtime/tests/sandbox_native.rs` | create | E2E tests: real landlock + seccomp on Linux, ignored elsewhere. |
| `crates/tau-runtime/tests/sandbox_container.rs` | create | E2E tests: docker/podman on Linux when available. |
| `Cargo.toml` (workspace) | modify | Add `crates/tau-sandbox-native`, `crates/tau-sandbox-container` as members; add `landlock`, `seccompiler` to `[workspace.dependencies]`. Verify `nix` is present. |

---

## Tasks

### Task 1: `CapabilityShape` vocabulary in `tau-domain`

**Why this first:** every other layer (port, adapter, runtime, install, resolve) depends on the typed shape vocabulary. The vocabulary is also the smallest self-contained unit: ~100 LOC, no external deps, exhaustively unit-testable.

**Files:**
- Modify: `crates/tau-domain/src/package/capability.rs`
- Modify: `crates/tau-domain/src/lib.rs`
- Test: `crates/tau-domain/src/package/capability.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Read the existing file to confirm anchors.**

Run: `grep -n '^pub enum\|^impl' crates/tau-domain/src/package/capability.rs`

Expected output (anchors must exist):
```
31:pub enum Capability {
61:pub enum FsCapability {
98:pub enum NetCapability {
120:pub enum ProcessCapability {
140:pub enum AgentCapability {
```

- [ ] **Step 2: Write a failing test (TDD red).**

Append at the end of `crates/tau-domain/src/package/capability.rs`:

```rust
#[cfg(test)]
mod shape_tests {
    use super::*;

    #[test]
    fn fs_read_required_shape() {
        let cap = Capability::Filesystem(FsCapability::Read {
            paths: vec!["/tmp/**".into()],
        });
        assert_eq!(
            cap.required_shape(),
            CapabilityShape::FilesystemRead
        );
    }

    #[test]
    fn fs_write_required_shape() {
        let cap = Capability::Filesystem(FsCapability::Write {
            paths: vec!["/tmp/x".into()],
            max_bytes: None,
        });
        assert_eq!(cap.required_shape(), CapabilityShape::FilesystemWrite);
    }

    #[test]
    fn fs_exec_required_shape() {
        let cap = Capability::Filesystem(FsCapability::Exec {
            paths: vec!["/usr/bin/git".into()],
        });
        assert_eq!(cap.required_shape(), CapabilityShape::ProcessExec);
    }

    #[test]
    fn net_http_required_shape() {
        let cap = Capability::Network(NetCapability::Http {
            hosts: vec!["api.example.com".into()],
            methods: vec!["GET".into()],
        });
        assert_eq!(cap.required_shape(), CapabilityShape::NetworkHttp);
    }

    #[test]
    fn process_spawn_required_shape() {
        let cap = Capability::Process(ProcessCapability::Spawn {
            commands: vec!["git".into()],
        });
        assert_eq!(cap.required_shape(), CapabilityShape::ProcessExec);
    }

    #[test]
    fn agent_spawn_required_shape() {
        let cap = Capability::Agent(AgentCapability::Spawn {
            allowed_kinds: vec!["worker".into()],
        });
        assert_eq!(cap.required_shape(), CapabilityShape::AgentSpawn);
    }

    #[test]
    fn custom_required_shape_is_custom() {
        let cap = Capability::Custom {
            name: "mcp.tool.use".into(),
            params: Default::default(),
        };
        match cap.required_shape() {
            CapabilityShape::Custom { name } => assert_eq!(name, "mcp.tool.use"),
            other => panic!("expected Custom, got {other:?}"),
        }
    }

    #[test]
    fn shape_set_contains_and_is_subset() {
        let mut a = CapabilityShapeSet::new();
        a.insert(CapabilityShape::FilesystemRead);
        a.insert(CapabilityShape::NetworkHttp);
        let mut b = CapabilityShapeSet::new();
        b.insert(CapabilityShape::FilesystemRead);
        b.insert(CapabilityShape::FilesystemWrite);
        b.insert(CapabilityShape::NetworkHttp);
        assert!(a.is_subset_of(&b));
        assert!(!b.is_subset_of(&a));
        assert!(a.contains(&CapabilityShape::FilesystemRead));
        assert!(!a.contains(&CapabilityShape::FilesystemWrite));
    }
}
```

- [ ] **Step 3: Run the test to confirm RED.**

Run: `cargo test -p tau-domain --lib shape_tests`
Expected: compile error — `CapabilityShape`, `CapabilityShapeSet`, `Capability::required_shape` not defined.

- [ ] **Step 4: Add the `CapabilityShape` enum.**

Insert after the existing `AgentCapability` enum (around line 147 in `crates/tau-domain/src/package/capability.rs`):

```rust
/// Typed vocabulary describing the *shape* of enforcement a [`Capability`]
/// requires from a sandbox adapter. Each variant maps to a distinct
/// kernel-level enforcement primitive (filesystem read/write, exec gating,
/// network egress filtering, etc).
///
/// Adapters declare a `CapabilityShapeSet` they support; the runtime
/// cross-checks plan-required vs adapter-supported before spawning a
/// plugin process.
///
/// Variant-level evolution is handled by `#[non_exhaustive]`. Adding a new
/// shape is **additive** — existing adapters that don't support it report
/// `SandboxError::ShapeUnsupported`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CapabilityShape {
    /// Plugin needs read access to a filtered set of paths.
    FilesystemRead,
    /// Plugin needs write access to a filtered set of paths.
    FilesystemWrite,
    /// Plugin needs to exec a binary (covers both `fs.exec` and `process.spawn`
    /// — same kernel surface).
    ProcessExec,
    /// Plugin needs HTTP egress to a filtered host list.
    NetworkHttp,
    /// Plugin needs to spawn a sub-agent. (Future: not enforced by OS sandbox
    /// today; reserved for forward-compat.)
    AgentSpawn,
    /// Plugin uses a `Capability::Custom` whose enforcement is plugin-defined.
    /// Adapters MAY refuse to sandbox `Custom` shapes.
    Custom {
        /// Custom capability name (`Capability::Custom { name }`).
        name: String,
    },
}

/// A set of [`CapabilityShape`]s, used by adapters to declare what they support
/// and by the runtime to declare what a plan requires. Subset / membership
/// queries are O(n) where n is the set size; we expect at most ~6 entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CapabilityShapeSet {
    inner: Vec<CapabilityShape>,
}

impl CapabilityShapeSet {
    /// Create an empty set.
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    /// Insert a shape (no-op if already present).
    pub fn insert(&mut self, shape: CapabilityShape) {
        if !self.inner.contains(&shape) {
            self.inner.push(shape);
        }
    }

    /// Check whether the set contains a shape.
    pub fn contains(&self, shape: &CapabilityShape) -> bool {
        self.inner.contains(shape)
    }

    /// `true` if every shape in `self` is also in `other`.
    pub fn is_subset_of(&self, other: &CapabilityShapeSet) -> bool {
        self.inner.iter().all(|s| other.inner.contains(s))
    }

    /// Iterate over the shapes.
    pub fn iter(&self) -> impl Iterator<Item = &CapabilityShape> {
        self.inner.iter()
    }

    /// Number of shapes in the set.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// `true` if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl Capability {
    /// The [`CapabilityShape`] this capability requires from a sandbox
    /// adapter. Used by `tau-runtime`'s validation layer to cross-check
    /// plan-required shapes against adapter-supported shapes.
    pub fn required_shape(&self) -> CapabilityShape {
        match self {
            Capability::Filesystem(FsCapability::Read { .. }) => {
                CapabilityShape::FilesystemRead
            }
            Capability::Filesystem(FsCapability::Write { .. }) => {
                CapabilityShape::FilesystemWrite
            }
            Capability::Filesystem(FsCapability::Exec { .. }) => {
                CapabilityShape::ProcessExec
            }
            Capability::Network(NetCapability::Http { .. }) => {
                CapabilityShape::NetworkHttp
            }
            Capability::Process(ProcessCapability::Spawn { .. }) => {
                CapabilityShape::ProcessExec
            }
            Capability::Agent(AgentCapability::Spawn { .. }) => {
                CapabilityShape::AgentSpawn
            }
            Capability::Custom { name, .. } => CapabilityShape::Custom {
                name: name.clone(),
            },
        }
    }
}
```

- [ ] **Step 5: Re-export from `tau-domain`'s `lib.rs`.**

In `crates/tau-domain/src/lib.rs`, find the existing capability re-export block and extend it. Locate the line that re-exports `Capability` (likely `pub use package::capability::Capability;` or similar). Add `CapabilityShape` and `CapabilityShapeSet` next to it.

Run first to confirm the existing pattern: `grep -n 'Capability' crates/tau-domain/src/lib.rs`

Then edit so the line becomes (or matches the existing convention):
```rust
pub use package::capability::{
    AgentCapability, Capability, CapabilityShape, CapabilityShapeSet, FsCapability,
    NetCapability, ProcessCapability,
};
```

- [ ] **Step 6: Run the test to confirm GREEN.**

Run: `cargo test -p tau-domain --lib shape_tests`
Expected: 8 passed; 0 failed.

- [ ] **Step 7: Run full crate tests.**

Run: `cargo test -p tau-domain --all-targets`
Expected: all tests pass; existing tests unchanged.

- [ ] **Step 8: Run workspace gates.**

Run in order, all must pass:
- `cargo build --workspace`
- `cargo test --workspace --all-targets`
- `cargo test --doc`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

- [ ] **Step 9: Commit.**

Stage exactly: `crates/tau-domain/src/package/capability.rs`, `crates/tau-domain/src/lib.rs`.

Run: `git add crates/tau-domain/src/package/capability.rs crates/tau-domain/src/lib.rs`

Run: `git commit -m "feat(domain): add CapabilityShape vocabulary + Capability::required_shape"`

(No `Cargo.lock` changes — no new dependencies in this task.)

---

### Task 2: Refine `tau_ports::Sandbox` port

**Why second:** the trait shape determines every adapter signature. With Task 1 landed, the trait can quote `CapabilityShape` directly.

**Files:**
- Modify: `crates/tau-ports/src/sandbox.rs`
- Modify: `crates/tau-ports/src/error.rs`
- Modify: `crates/tau-ports/src/fixtures.rs`
- Modify: `crates/tau-ports/src/lib.rs`

- [ ] **Step 1: Write failing tests (TDD red).**

Append to `crates/tau-ports/src/fixtures.rs` (after the `MockSandbox` impl):

```rust
#[cfg(test)]
mod sandbox_v01_tests {
    use super::*;
    use tau_domain::{Capability, CapabilityShape, FsCapability};

    fn read_cap() -> Capability {
        Capability::Filesystem(FsCapability::Read {
            paths: vec!["/tmp/**".into()],
        })
    }

    #[tokio::test]
    async fn mock_probe_is_available() {
        let mock = MockSandbox::new("mem");
        let probe = mock.probe().await;
        assert!(matches!(probe, SandboxProbe::Available { .. }));
    }

    #[tokio::test]
    async fn mock_supports_all_known_shapes() {
        let mock = MockSandbox::new("mem");
        let supported = mock.supported_shapes();
        assert!(supported.contains(&CapabilityShape::FilesystemRead));
        assert!(supported.contains(&CapabilityShape::FilesystemWrite));
        assert!(supported.contains(&CapabilityShape::ProcessExec));
        assert!(supported.contains(&CapabilityShape::NetworkHttp));
        assert!(supported.contains(&CapabilityShape::AgentSpawn));
    }

    #[tokio::test]
    async fn mock_validate_plan_accepts_known_shape() {
        let mock = MockSandbox::new("mem");
        let plan = SandboxPlan {
            capabilities: vec![read_cap()],
            context: None,
            limits: None,
        };
        assert!(mock.validate_plan(&plan).is_ok());
    }

    #[tokio::test]
    async fn mock_validate_plan_rejects_custom_shape() {
        let mock = MockSandbox::new("mem");
        let plan = SandboxPlan {
            capabilities: vec![Capability::Custom {
                name: "weird".into(),
                params: Default::default(),
            }],
            context: None,
            limits: None,
        };
        match mock.validate_plan(&plan) {
            Err(SandboxError::ShapeUnsupported { shape }) => {
                assert!(matches!(shape, CapabilityShape::Custom { .. }));
            }
            other => panic!("expected ShapeUnsupported, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_wrap_spawn_returns_handle() {
        let mock = MockSandbox::new("mem");
        let plan = SandboxPlan {
            capabilities: vec![read_cap()],
            context: None,
            limits: None,
        };
        let mut cmd = std::process::Command::new("/bin/true");
        let handle = mock.wrap_spawn(&plan, &mut cmd).await.unwrap();
        // MockSandbox handle is unit; just check the type.
        let _: SandboxHandle = handle;
    }

    #[tokio::test]
    async fn sandbox_tier_ordering() {
        assert!(SandboxTier::Light < SandboxTier::Strict);
        assert!(SandboxTier::None < SandboxTier::Light);
    }

    #[test]
    fn sandbox_error_unavailable_renders() {
        let e = SandboxError::Unavailable {
            reason: "no kernel".into(),
        };
        assert!(format!("{e}").contains("unavailable"));
    }

    #[test]
    fn sandbox_error_shape_unsupported_renders() {
        let e = SandboxError::ShapeUnsupported {
            shape: CapabilityShape::FilesystemRead,
        };
        assert!(format!("{e}").contains("unsupported shape"));
    }
}
```

- [ ] **Step 2: Run tests to confirm RED.**

Run: `cargo test -p tau-ports --all-targets sandbox_v01_tests`
Expected: compile error — `SandboxProbe`, `SandboxTier`, `SandboxHandle`, new `SandboxError` variants, new trait methods not defined.

- [ ] **Step 3: Replace `crates/tau-ports/src/sandbox.rs` body.**

The new file content (replaces the entire current file):

```rust
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

use tau_domain::{Capability, CapabilityShape, CapabilityShapeSet};

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
    /// Adapter-defined cleanup. Boxed to keep `SandboxHandle` object-safe.
    _cleanup: Box<dyn FnOnce() + Send + 'static>,
}

impl SandboxHandle {
    /// Construct a handle from an adapter-defined cleanup closure.
    pub fn new<F: FnOnce() + Send + 'static>(cleanup: F) -> Self {
        Self {
            _cleanup: Box::new(cleanup),
        }
    }

    /// A handle that releases nothing (mock / no-op).
    pub fn noop() -> Self {
        Self::new(|| {})
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
```

- [ ] **Step 4: Refine `crates/tau-ports/src/error.rs`.**

Locate the `SandboxError` enum (line ~155). Replace its full body and the surrounding doc comment with:

```rust
/// Errors returned by [`crate::sandbox::Sandbox`] implementations.
///
/// Stable as of v0.1 of the sandboxing sub-project. Variant evolution
/// is handled by `#[non_exhaustive]` at the enum level and on each
/// struct-style variant.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SandboxError {
    /// The adapter is not usable on this host (probe returned Unavailable).
    #[error("sandbox unavailable: {reason}")]
    Unavailable {
        /// Reason from [`crate::sandbox::SandboxProbe::Unavailable`].
        reason: String,
    },
    /// The plan requires a capability shape this adapter does not support.
    #[error("sandbox: unsupported shape {shape:?}")]
    ShapeUnsupported {
        /// The shape that was rejected.
        shape: tau_domain::CapabilityShape,
    },
    /// The adapter could not apply sandbox enforcement to the spawn.
    /// (Examples: landlock syscall failed, seccomp filter compile failed,
    /// `docker run` returned non-zero.)
    #[error("sandbox wrap-spawn failed: {message}")]
    WrapFailed {
        /// Free-form diagnostic; not part of the stable API surface.
        message: String,
    },
    /// Runtime sandbox violation reported by the kernel
    /// (SIGSYS from seccomp, EACCES from landlock, etc).
    #[error("sandbox violation: {detail}")]
    Violation {
        /// Detail about the violating syscall / path / host.
        detail: String,
    },
    /// The requested feature is not supported by this sandbox.
    #[error("sandbox unsupported: {what}")]
    Unsupported {
        /// Description of the unsupported feature.
        what: String,
    },
    /// A configured resource limit was exceeded.
    #[error("sandbox limit exceeded: {limit}")]
    LimitExceeded {
        /// Identifier of the limit that was exceeded.
        limit: String,
    },
    /// Plugin internal error.
    ///
    /// See: [escape-hatches.md#sandboxerror-internal](../docs/explanation/escape-hatches.md#sandboxerror-internal).
    #[error("sandbox internal: {message}")]
    Internal {
        /// Free-form internal-error message; not part of the stable API surface.
        message: String,
    },
}
```

- [ ] **Step 5: Update `MockSandbox` in `crates/tau-ports/src/fixtures.rs`.**

Replace the existing `impl MockSandbox` and `impl Sandbox for MockSandbox` blocks (lines ~482-505) with:

```rust
/// Mock [`Sandbox`] adapter for tests. Reports `Available` with `Tier::None`,
/// supports every known [`CapabilityShape`] except [`CapabilityShape::Custom`],
/// and `wrap_spawn` is a no-op.
pub struct MockSandbox {
    name: String,
}

impl MockSandbox {
    /// Create a fresh mock sandbox.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl Sandbox for MockSandbox {
    fn name(&self) -> &str {
        &self.name
    }

    async fn probe(&self) -> SandboxProbe {
        SandboxProbe::Available {
            tier: SandboxTier::None,
            details: "mock — no enforcement".into(),
        }
    }

    fn supported_shapes(&self) -> CapabilityShapeSet {
        let mut set = CapabilityShapeSet::new();
        set.insert(CapabilityShape::FilesystemRead);
        set.insert(CapabilityShape::FilesystemWrite);
        set.insert(CapabilityShape::ProcessExec);
        set.insert(CapabilityShape::NetworkHttp);
        set.insert(CapabilityShape::AgentSpawn);
        set
    }

    fn validate_plan(&self, plan: &SandboxPlan) -> Result<(), SandboxError> {
        let supported = self.supported_shapes();
        for cap in &plan.capabilities {
            let shape = cap.required_shape();
            if !supported.contains(&shape) {
                return Err(SandboxError::ShapeUnsupported { shape });
            }
        }
        Ok(())
    }

    async fn wrap_spawn(
        &self,
        plan: &SandboxPlan,
        _cmd: &mut std::process::Command,
    ) -> Result<SandboxHandle, SandboxError> {
        self.validate_plan(plan)?;
        Ok(SandboxHandle::noop())
    }
}
```

The top of `fixtures.rs` needs an updated import block — change the existing `use crate::sandbox::{Sandbox, SandboxPlan};` to also pull in the new types and the domain shape types:

```rust
use crate::sandbox::{
    Sandbox, SandboxHandle, SandboxPlan, SandboxProbe, SandboxTier,
};
use tau_domain::{Capability, CapabilityShape, CapabilityShapeSet};
```

(Adapt the existing imports — match what's already there; only ADD what's needed.)

- [ ] **Step 6: Update `crates/tau-ports/src/lib.rs` re-exports.**

Add the new types to the existing sandbox re-export. Find the line `pub use sandbox::{...};` and extend it to:

```rust
pub use sandbox::{
    ResourceLimits, Sandbox, SandboxHandle, SandboxPlan, SandboxProbe, SandboxTier,
    WorkingContext,
};
```

- [ ] **Step 7: Run the new tests (GREEN).**

Run: `cargo test -p tau-ports --all-targets sandbox_v01_tests`
Expected: 8 passed; 0 failed.

- [ ] **Step 8: Run workspace gates.**

Run: `cargo build --workspace && cargo test --workspace --all-targets && cargo test --doc && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`

If any downstream crate breaks (likely candidates: `tau-runtime`, `tau-pkg` if either references the old `create()` method), fix them in lockstep. Search: `grep -rn "Sandbox::create\|\.create(" crates/ | grep -v target`. If matches exist, replace `.create(plan)` with `.wrap_spawn(&plan, &mut cmd)` against a placeholder command, OR update callers to the new shape — DO NOT keep the old method.

- [ ] **Step 9: Commit.**

Stage exactly: `crates/tau-ports/src/sandbox.rs`, `crates/tau-ports/src/error.rs`, `crates/tau-ports/src/fixtures.rs`, `crates/tau-ports/src/lib.rs`, plus any downstream call-site fixups identified in Step 8.

Run: `git add crates/tau-ports/src/sandbox.rs crates/tau-ports/src/error.rs crates/tau-ports/src/fixtures.rs crates/tau-ports/src/lib.rs`

(If downstream fixups: also `git add` those paths.)

Run: `git commit -m "feat(ports): refine Sandbox port — probe/supported_shapes/validate_plan/wrap_spawn"`

(No `Cargo.lock` changes — no new dependencies in this task.)

---

### Task 3: `tau-sandbox-native` skeleton + Light tier (landlock filesystem isolation)

**Why third:** with the port stable, the first real adapter validates the trait shape end-to-end. Light tier (landlock only) is the smallest useful enforcement and exercises the full pipeline (probe → supported_shapes → validate_plan → wrap_spawn).

**Files:**
- Create: `crates/tau-sandbox-native/Cargo.toml`
- Create: `crates/tau-sandbox-native/src/lib.rs`
- Create: `crates/tau-sandbox-native/src/probe.rs`
- Create: `crates/tau-sandbox-native/src/light.rs`
- Create: `crates/tau-sandbox-native/src/shape.rs`
- Create: `crates/tau-sandbox-native/src/stub.rs`
- Modify: `Cargo.toml` (workspace) — add member + `landlock` workspace dep
- Test: `crates/tau-sandbox-native/tests/light_landlock.rs`

- [ ] **Step 1: Add the workspace member + dep.**

Edit `Cargo.toml` (workspace root):

In `[workspace] members = [...]`, append:
```
    "crates/tau-sandbox-native",
```
(Keep alphabetical-ish order; the existing list shows it doesn't strictly matter.)

In `[workspace.dependencies]`, after the `seccompiler` line you'll add later (for now just add `landlock`):
```toml
landlock        = "0.4"
```

Verify `nix` workspace dep — if missing, add `nix = { version = "0.29", default-features = false, features = ["sched", "user", "process"] }`. (Search: `grep -n '^nix' Cargo.toml`. If not present, add it.)

- [ ] **Step 2: Create `crates/tau-sandbox-native/Cargo.toml`.**

```toml
[package]
name = "tau-sandbox-native"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
description = "Linux native sandbox adapter (landlock + seccomp + namespaces) for tau"

[dependencies]
tau-domain = { workspace = true, features = ["serde"] }
tau-ports  = { workspace = true, features = ["serde"] }
tracing    = { workspace = true }
tokio      = { workspace = true }
thiserror  = { workspace = true }

[target.'cfg(target_os = "linux")'.dependencies]
landlock = { workspace = true }
nix      = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
tokio    = { workspace = true, features = ["macros", "rt"] }

[features]
default = []
# Gated by `cargo test --features integration-tests` for tests that
# actually invoke landlock/seccomp on the host kernel.
integration-tests = []
```

- [ ] **Step 3: Create `crates/tau-sandbox-native/src/lib.rs`.**

```rust
//! Linux native sandbox adapter for tau.
//!
//! Implements [`tau_ports::Sandbox`] using:
//! - **landlock** (kernel 5.13+) for filesystem path isolation,
//! - **seccompiler** for syscall filtering (Strict tier — Task 4),
//! - **nix unshare** for user/network namespaces (Strict tier — Task 5).
//!
//! On non-Linux hosts the adapter exists but `probe()` returns
//! `SandboxProbe::Unavailable` and all other methods return
//! `SandboxError::Unavailable`.

#![deny(missing_docs)]

mod shape;

#[cfg(target_os = "linux")]
mod light;
#[cfg(target_os = "linux")]
mod probe;

#[cfg(not(target_os = "linux"))]
mod stub;

use std::process::Command;

use tau_domain::CapabilityShapeSet;
use tau_ports::{
    Sandbox, SandboxError, SandboxHandle, SandboxPlan, SandboxProbe, SandboxTier,
};

/// Linux native sandbox adapter. Probe-driven: at construction time the
/// adapter is inert; calling [`Sandbox::probe`] discovers what the host
/// kernel can offer and the runtime caches the result.
pub struct NativeSandbox {
    name: String,
    requested_tier: SandboxTier,
}

impl NativeSandbox {
    /// Construct an adapter that will deliver up to the given tier. The
    /// effective tier is `min(requested_tier, probe_tier)`.
    pub fn new(name: impl Into<String>, requested_tier: SandboxTier) -> Self {
        Self {
            name: name.into(),
            requested_tier,
        }
    }
}

impl Sandbox for NativeSandbox {
    fn name(&self) -> &str {
        &self.name
    }

    async fn probe(&self) -> SandboxProbe {
        #[cfg(target_os = "linux")]
        {
            probe::probe(self.requested_tier).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            stub::unavailable_probe()
        }
    }

    fn supported_shapes(&self) -> CapabilityShapeSet {
        #[cfg(target_os = "linux")]
        {
            shape::shapes_for_tier(self.requested_tier)
        }
        #[cfg(not(target_os = "linux"))]
        {
            CapabilityShapeSet::new()
        }
    }

    fn validate_plan(&self, plan: &SandboxPlan) -> Result<(), SandboxError> {
        let supported = self.supported_shapes();
        if supported.is_empty() {
            return Err(SandboxError::Unavailable {
                reason: "tau-sandbox-native requires Linux".into(),
            });
        }
        for cap in &plan.capabilities {
            let shape = cap.required_shape();
            if !supported.contains(&shape) {
                return Err(SandboxError::ShapeUnsupported { shape });
            }
        }
        Ok(())
    }

    async fn wrap_spawn(
        &self,
        plan: &SandboxPlan,
        cmd: &mut Command,
    ) -> Result<SandboxHandle, SandboxError> {
        self.validate_plan(plan)?;
        #[cfg(target_os = "linux")]
        {
            match self.requested_tier {
                SandboxTier::Light | SandboxTier::Strict => {
                    light::apply_landlock(plan, cmd)
                }
                SandboxTier::None => Ok(SandboxHandle::noop()),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (plan, cmd);
            Err(SandboxError::Unavailable {
                reason: "tau-sandbox-native requires Linux".into(),
            })
        }
    }
}
```

- [ ] **Step 4: Create `crates/tau-sandbox-native/src/shape.rs`.**

```rust
//! Map [`tau_domain::CapabilityShape`] onto the set this adapter supports
//! at a given tier.

use tau_domain::{CapabilityShape, CapabilityShapeSet};
use tau_ports::SandboxTier;

/// Capability shapes this adapter can enforce at the given tier.
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
    }
    set
}
```

- [ ] **Step 5: Create `crates/tau-sandbox-native/src/probe.rs`.**

```rust
//! Linux kernel feature probe.

use tau_ports::{SandboxProbe, SandboxTier};

/// Probe the host kernel for sandbox features.
///
/// Returns `Available { tier }` where `tier` is the strongest tier the
/// kernel can support, capped at the caller's requested tier.
pub(crate) async fn probe(requested: SandboxTier) -> SandboxProbe {
    let landlock_ok = landlock_v1_supported();
    if !landlock_ok {
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
    };
    SandboxProbe::Available {
        tier: effective,
        details: format!("landlock V1 ok (cap to {effective:?})"),
    }
}

fn landlock_v1_supported() -> bool {
    use landlock::{ABI, Compatible, Ruleset, RulesetAttr};
    // Cheap probe: try to create a Ruleset for ABI::V1. If the kernel
    // doesn't support landlock, this returns Compatibility::NoRuntimeSupport.
    match Ruleset::default()
        .handle_access(landlock::AccessFs::from_all(ABI::V1))
        .map(|r| r.create())
    {
        Ok(Ok(_)) => true,
        _ => false,
    }
}
```

- [ ] **Step 6: Create `crates/tau-sandbox-native/src/light.rs`.**

```rust
//! Light-tier enforcement: landlock filesystem isolation only.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use tau_domain::{Capability, FsCapability};
use tau_ports::{SandboxError, SandboxHandle, SandboxPlan};

/// Apply landlock rules to `cmd` via a `pre_exec` hook. The rules are
/// derived from the plan's filesystem capabilities; non-fs capabilities
/// are accepted by `validate_plan` but enforced by Strict tier (seccomp
/// + namespaces — future tasks).
pub(crate) fn apply_landlock(
    plan: &SandboxPlan,
    cmd: &mut Command,
) -> Result<SandboxHandle, SandboxError> {
    let read_paths = collect_paths(plan, |c| match c {
        Capability::Filesystem(FsCapability::Read { paths }) => Some(paths.clone()),
        _ => None,
    });
    let write_paths = collect_paths(plan, |c| match c {
        Capability::Filesystem(FsCapability::Write { paths, .. }) => Some(paths.clone()),
        _ => None,
    });

    // Resolve glob anchors (`${PROJECT}/...`) to absolute paths. For Task 3
    // we only support absolute paths and the special `${PROJECT}` anchor
    // bound to the working directory of the spawned command.
    let cwd = std::env::current_dir().map_err(|e| SandboxError::WrapFailed {
        message: format!("cwd: {e}"),
    })?;
    let read_paths = resolve_anchors(&read_paths, &cwd);
    let write_paths = resolve_anchors(&write_paths, &cwd);

    // Use unsafe pre_exec to install the ruleset in the child after fork
    // but before exec. landlock is per-thread; installing in the parent
    // would lock down tau itself.
    unsafe {
        cmd.pre_exec(move || {
            install_landlock(&read_paths, &write_paths).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
            })
        });
    }
    Ok(SandboxHandle::noop())
}

fn collect_paths<F: Fn(&Capability) -> Option<Vec<String>>>(
    plan: &SandboxPlan,
    extract: F,
) -> Vec<String> {
    plan.capabilities.iter().filter_map(extract).flatten().collect()
}

fn resolve_anchors(paths: &[String], cwd: &std::path::Path) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|p| {
            let p = p.replace("${PROJECT}", cwd.to_string_lossy().as_ref());
            // Drop trailing glob suffix; landlock works on directory roots.
            // For "/tmp/**" we add "/tmp"; for "/tmp/x" we add "/tmp/x".
            let trimmed = p
                .trim_end_matches("/**")
                .trim_end_matches("/*")
                .to_string();
            PathBuf::from(trimmed)
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn install_landlock(
    read_paths: &[PathBuf],
    write_paths: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use landlock::{
        ABI, Access, AccessFs, Compatible, PathBeneath, PathFd, Ruleset, RulesetAttr,
        RulesetCreatedAttr,
    };

    let mut ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(ABI::V1))?
        .create()?;
    for p in read_paths {
        if let Ok(fd) = PathFd::new(p) {
            ruleset = ruleset.add_rule(PathBeneath::new(
                fd,
                AccessFs::ReadFile | AccessFs::ReadDir,
            ))?;
        }
    }
    for p in write_paths {
        if let Ok(fd) = PathFd::new(p) {
            ruleset = ruleset.add_rule(PathBeneath::new(
                fd,
                AccessFs::WriteFile | AccessFs::MakeReg | AccessFs::RemoveFile,
            ))?;
        }
    }
    let _status = ruleset.restrict_self()?;
    Ok(())
}
```

> Note: the landlock crate API has shifted across 0.3 → 0.4. The implementer must verify the actual `landlock = "0.4"` crate's public types; the sketch above shows intent. Adjust import names + builder method names to match the crate version pinned in Step 1. Do not pin a different version without flagging in the commit body.

- [ ] **Step 7: Create `crates/tau-sandbox-native/src/stub.rs`.**

```rust
//! Non-Linux fallback: every probe returns Unavailable.

use tau_ports::SandboxProbe;

#[allow(dead_code)] // Used only on non-Linux.
pub(crate) fn unavailable_probe() -> SandboxProbe {
    SandboxProbe::Unavailable {
        reason: "tau-sandbox-native requires Linux".into(),
    }
}
```

- [ ] **Step 8: Write unit tests inline in `lib.rs` (TDD red).**

Append to `crates/tau-sandbox-native/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tau_domain::{Capability, CapabilityShape, FsCapability};

    #[test]
    fn name_and_tier_round_trip() {
        let s = NativeSandbox::new("native-light", SandboxTier::Light);
        assert_eq!(s.name(), "native-light");
    }

    #[test]
    fn supported_shapes_light_includes_fs() {
        let s = NativeSandbox::new("n", SandboxTier::Light);
        let supported = s.supported_shapes();
        #[cfg(target_os = "linux")]
        {
            assert!(supported.contains(&CapabilityShape::FilesystemRead));
            assert!(supported.contains(&CapabilityShape::FilesystemWrite));
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert!(supported.is_empty());
        }
    }

    #[test]
    fn validate_plan_rejects_unsupported_shape_at_light_tier() {
        let s = NativeSandbox::new("n", SandboxTier::Light);
        let plan = SandboxPlan {
            capabilities: vec![Capability::Custom {
                name: "weird".into(),
                params: Default::default(),
            }],
            context: None,
            limits: None,
        };
        assert!(s.validate_plan(&plan).is_err());
    }

    #[tokio::test]
    async fn probe_on_non_linux_is_unavailable() {
        #[cfg(not(target_os = "linux"))]
        {
            let s = NativeSandbox::new("n", SandboxTier::Light);
            let p = s.probe().await;
            assert!(matches!(p, SandboxProbe::Unavailable { .. }));
        }
    }

    #[tokio::test]
    async fn validate_plan_unavailable_on_non_linux() {
        #[cfg(not(target_os = "linux"))]
        {
            let s = NativeSandbox::new("n", SandboxTier::Light);
            let plan = SandboxPlan {
                capabilities: vec![Capability::Filesystem(FsCapability::Read {
                    paths: vec!["/tmp".into()],
                })],
                context: None,
                limits: None,
            };
            assert!(matches!(
                s.validate_plan(&plan),
                Err(SandboxError::Unavailable { .. })
            ));
        }
    }

    #[test]
    fn shapes_strict_tier_includes_exec_and_net() {
        let s = NativeSandbox::new("n", SandboxTier::Strict);
        let supported = s.supported_shapes();
        #[cfg(target_os = "linux")]
        {
            assert!(supported.contains(&CapabilityShape::ProcessExec));
            assert!(supported.contains(&CapabilityShape::NetworkHttp));
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert!(supported.is_empty());
        }
    }
}
```

- [ ] **Step 9: Create the integration test (Linux-only, ignored by default).**

Create `crates/tau-sandbox-native/tests/light_landlock.rs`:

```rust
//! Real landlock integration. Linux-only; gated `#[ignore]` so the standard
//! `cargo test` ignores it. Run with:
//!   `cargo test -p tau-sandbox-native --features integration-tests -- --ignored`

#![cfg(all(target_os = "linux", feature = "integration-tests"))]

use std::process::Command;
use tau_domain::{Capability, FsCapability};
use tau_ports::{Sandbox, SandboxPlan, SandboxTier};
use tau_sandbox_native::NativeSandbox;
use tempfile::TempDir;

#[tokio::test]
#[ignore]
async fn landlock_blocks_unlisted_path() {
    let allowed = TempDir::new().unwrap();
    let blocked = TempDir::new().unwrap();

    let s = NativeSandbox::new("native", SandboxTier::Light);
    let plan = SandboxPlan {
        capabilities: vec![Capability::Filesystem(FsCapability::Read {
            paths: vec![allowed.path().to_string_lossy().into_owned()],
        })],
        context: None,
        limits: None,
    };

    // /bin/cat against an allowed path: should succeed (no landlock denial).
    {
        let allowed_file = allowed.path().join("ok.txt");
        std::fs::write(&allowed_file, b"hello").unwrap();
        let mut cmd = Command::new("/bin/cat");
        cmd.arg(&allowed_file);
        let _h = s.wrap_spawn(&plan, &mut cmd).await.unwrap();
        let status = cmd.status().unwrap();
        assert!(status.success(), "allowed path should be readable");
    }

    // /bin/cat against a blocked path: landlock should deny the read.
    {
        let blocked_file = blocked.path().join("nope.txt");
        std::fs::write(&blocked_file, b"secret").unwrap();
        let mut cmd = Command::new("/bin/cat");
        cmd.arg(&blocked_file);
        let _h = s.wrap_spawn(&plan, &mut cmd).await.unwrap();
        let status = cmd.status().unwrap();
        // /bin/cat exits non-zero when read fails (EACCES from landlock).
        assert!(
            !status.success(),
            "blocked path should be denied — got status {status:?}"
        );
    }
}
```

- [ ] **Step 10: Run unit tests (GREEN).**

Run: `cargo test -p tau-sandbox-native --lib`
Expected: 6 passed; 0 failed (or pass-on-Linux + skip-on-non-Linux as gated).

- [ ] **Step 11: Run integration test on Linux (locally; skip on macOS).**

If the developer is on Linux: `cargo test -p tau-sandbox-native --features integration-tests -- --ignored`
Expected: 1 passed.

If on macOS / Windows: skip this step. CI will run it on the `ubuntu-latest` slot only when the integration-tests feature is requested by Task 11's e2e wiring; for Task 3 in isolation, CI does NOT yet flip the flag, so CI will simply NOT run it. (Task 11 wires the flag.)

- [ ] **Step 12: Run workspace gates.**

Run: `cargo build --workspace && cargo test --workspace --all-targets && cargo test --doc && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`

Note: `Cargo.lock` will change because `landlock` (and possibly `nix` if newly added) is now linked. Stage it.

- [ ] **Step 13: Commit.**

Stage exactly: `Cargo.toml`, `Cargo.lock`, the new `crates/tau-sandbox-native/` tree.

Run: `git add Cargo.toml Cargo.lock crates/tau-sandbox-native`

Run: `git commit -m "feat(sandbox-native): Light tier — landlock filesystem isolation adapter"`

---

### Task 4: `tau-sandbox-native` Strict tier — seccomp + namespaces

**Spec section:** §"Adapter implementations" → "Native Linux Strict tier"; §"Layer 4 — runtime enforcement"

**Files (modify):**
- `crates/tau-sandbox-native/src/strict.rs` (create)
- `crates/tau-sandbox-native/src/lib.rs` (route Strict tier through `strict::apply`)
- `crates/tau-sandbox-native/src/probe.rs` (probe seccomp BPF + user_namespaces; lift the cap so Strict can return Strict)
- `crates/tau-sandbox-native/Cargo.toml` (add `seccompiler` to Linux deps)
- `Cargo.toml` (workspace) — add `seccompiler = "0.5"` to `[workspace.dependencies]`
- `crates/tau-sandbox-native/tests/strict_seccomp.rs` (create — `#[ignore]`-gated integration test)

**Summary:**
- Implement a baseline allow-list seccomp BPF that permits the standard plugin syscall set (read/write/openat/close/fstat/mmap/brk/futex/etc) and denies everything else with SIGSYS. Reuse the landlock pre_exec hook from Task 3; chain seccomp install AFTER landlock install so a seccomp filter doesn't block landlock's own syscall.
- `unshare(CLONE_NEWUSER | CLONE_NEWNET)` via `nix::sched::unshare` in pre_exec — drops the spawned child into a new user namespace + a network namespace with no interfaces (Task 5 lifts loopback for hosts that need it).
- The `strict.rs` module exports `pub(crate) fn apply_strict(plan, cmd) -> Result<SandboxHandle>`. The pre_exec closure runs landlock → unshare → seccomp in that order.
- Tests: 4 unit tests (filter compilation, syscall map, baseline allow-list, deny-list); 2 integration tests gated `#[ignore]` + Linux-only (deny-fork and deny-network exec).

**Verification:** `cargo test -p tau-sandbox-native --lib`; `cargo test --workspace --all-targets`; full workspace gates; on Linux `cargo test -p tau-sandbox-native --features integration-tests -- --ignored`.

**Commit:** `feat(sandbox-native): Strict tier — seccomp BPF + user/network namespaces` (stage `Cargo.toml`, `Cargo.lock`, `crates/tau-sandbox-native/`).

---

### Task 5: `tau-sandbox-native` per-host network filtering + per-command exec gating

**Spec section:** §"Capability shape mapping" — `CapabilityShape::NetworkHttp`, `CapabilityShape::ProcessExec`

**Files (modify):**
- `crates/tau-sandbox-native/src/strict.rs` (extend pre_exec chain)
- `crates/tau-sandbox-native/src/shape.rs` (route `NetworkHttp` and `ProcessExec` capabilities into rule emission)
- `crates/tau-sandbox-native/src/exec.rs` (create — `seccomp` argument-filter for `execve` paths)
- `crates/tau-sandbox-native/src/net.rs` (create — netns plumbing: spawn `nft` for egress allow-list when capable, fall back to deny-all)
- `crates/tau-sandbox-native/tests/strict_exec_gating.rs` (create)
- `crates/tau-sandbox-native/tests/strict_net_filter.rs` (create)

**Summary:**
- Per-command exec gating: a seccomp filter on `execve`/`execveat` that compares the first argument (path) against the allow-list from `Capability::Process(Spawn { commands })` and `Capability::Filesystem(FsCapability::Exec { paths })`. The filter uses `seccompiler::SeccompCondition::Arg(0, ...)` to inspect the path argument; deny → SIGSYS.
- Per-host network filtering: in the unshared netns, the adapter writes an nftables ruleset that allows egress to the resolved IPs of `Capability::Network(NetCapability::Http { hosts })` and drops everything else. If `nft` is not on the host, the adapter falls back to "no network at all" (the netns has no interfaces, so HTTP fails with ENETUNREACH); this is documented in the adapter probe details and surfaced as a tracing warning.
- Tests: argument-filter unit tests (3-4), 2 ignored Linux-only integration tests asserting (a) blocked exec path → child SIGSYS, (b) blocked host → reqwest connect fails.

**Verification:** standard workspace gates + `cargo test -p tau-sandbox-native --features integration-tests -- --ignored` on Linux.

**Commit:** `feat(sandbox-native): per-host network filter + per-command exec gating` (no new workspace deps; `nft` is a runtime probe, not a build dep).

---

### Task 6: `tau-sandbox-container` adapter (docker / podman shell-out)

**Spec section:** §"Adapter implementations" → "Container adapter"

**Files (create):**
- `crates/tau-sandbox-container/Cargo.toml`
- `crates/tau-sandbox-container/src/lib.rs` (top-level `pub struct ContainerSandbox`)
- `crates/tau-sandbox-container/src/probe.rs` (`docker --version` / `podman --version` detection, cached on first call)
- `crates/tau-sandbox-container/src/runner.rs` (build `docker run` argv from `SandboxPlan`: `-v` mounts for fs.read/write paths; `--network none` baseline; `--add-host`/network for `net.http`; `--cap-drop=ALL`; `--security-opt=no-new-privileges`; `--read-only`; tmpfs for /tmp)

**Summary:**
- `ContainerSandbox::new(name, runtime: ContainerRuntime)` where `ContainerRuntime ∈ {Docker, Podman, Auto}`. `Auto` probes both binaries.
- Probe runs once at startup and caches the result behind `OnceCell<SandboxProbe>`. Probe shells out to `docker --version` (with a 2-second timeout); if absent, tries `podman --version`. Probe failure → `SandboxProbe::Unavailable { reason: "no docker or podman binary on PATH" }`.
- `wrap_spawn` builds an argv: replaces `cmd` in-place with `docker run --rm -i --network={none|bridge with --dns + iptables} -v <readpath>:<readpath>:ro -v <writepath>:<writepath>:rw --cap-drop=ALL --security-opt=no-new-privileges --read-only --tmpfs /tmp <plugin-image> <original-program> <original-args...>`. The `SandboxHandle` retains a kill-on-drop reference to the docker container ID.
- supported_shapes: `FilesystemRead, FilesystemWrite, ProcessExec, NetworkHttp` at `Tier::Strict`.
- Image name: `<plugin-image>` resolved from a `[sandbox.container]` section of the scope config (plugin → image map). For v0.1 we ship a single default image `ghcr.io/tau-runtime/sandbox-base:v0.1` documented in the spec; users override per-plugin in their scope config.
- Tests: ~6 unit tests for argv generation (each capability shape → expected docker flags); 1 ignored integration test asserting `docker run --rm hello-world` works only when docker is on PATH.

**Verification:** standard workspace gates. Container integration test runs only when `which docker` succeeds; otherwise gracefully skipped via runtime probe.

**Commit:** `feat(sandbox-container): docker/podman shell-out adapter` (stage `Cargo.toml`, `Cargo.lock` ONLY if a new dep was added — Task 6 should add NONE; if `Cargo.lock` is unchanged, do not stage it).

---

### Task 7: Runtime sandbox chain config + selection (`<scope>/.tau/config.toml [sandbox]` section)

**Spec section:** §"Adapter chain" + §"Scope config schema"

**Files (modify / create):**
- `crates/tau-pkg/src/scope.rs` — extend `ScopeConfig` (search the existing struct to find the right insertion point) with:
  ```rust
  #[serde(default)]
  pub sandbox: SandboxConfig,
  ```
  and a new `pub struct SandboxConfig { pub chain: Vec<SandboxAdapterConfig>, pub minimum_tier: Option<SandboxTier> }` plus `pub struct SandboxAdapterConfig { pub kind: SandboxAdapterKind, pub options: BTreeMap<String, toml::Value> }` and `pub enum SandboxAdapterKind { Native, Container, Mock }`. All `#[non_exhaustive]`.
- `crates/tau-runtime/src/sandbox/mod.rs` — new module + `pub fn select_adapter(cfg: &SandboxConfig) -> Result<Arc<dyn Sandbox>>` that probes each entry in order, returning the first `Available`.
- `crates/tau-runtime/src/sandbox/chain.rs` — chain-builder helpers: instantiate `NativeSandbox` / `ContainerSandbox` / `MockSandbox` from a `SandboxAdapterConfig`.
- `crates/tau-runtime/src/lib.rs` — `pub mod sandbox;`
- `crates/tau-runtime/Cargo.toml` — add `tau-sandbox-native` and `tau-sandbox-container` as workspace dependencies.
- Tests: `crates/tau-pkg/src/scope.rs` inline tests for round-trip TOML serde of the new section; `crates/tau-runtime/src/sandbox/chain.rs` inline tests for adapter selection precedence (native available → native; native unavailable + container available → container; both unavailable → error).

**Summary:**
- Default chain (when `<scope>/.tau/config.toml` lacks a `[sandbox]` section): `[{ kind = "native" }, { kind = "container" }]`. On non-Linux hosts both probe Unavailable, the runtime emits `SandboxError::Unavailable { reason: "no sandbox adapter available on this platform — see docs/explanation/sandboxing.md" }`, exit code 2.
- `minimum_tier`: if set, the runtime rejects adapters whose probe tier is lower (e.g. minimum_tier = Strict and only Light is available → exit 2).
- The probe runs once per process startup; the result is cached behind `OnceCell` keyed by adapter index. The CLI's `--sandbox-tier` flag (added in Task 9) overrides `minimum_tier` for a single run.
- The mock adapter is admissible only when explicitly listed (`kind = "mock"`), preventing silent fallthrough to no enforcement.

**Verification:** standard workspace gates. Confirm `cargo test -p tau-pkg scope::sandbox` and `cargo test -p tau-runtime sandbox::chain` pass.

**Commit:** `feat(runtime): sandbox adapter chain — probe-based selection from scope config`.

---

### Task 8: Layer 3 — pre-flight plan validation

**Spec section:** §"Layer 3 — `tau check` static validation"

**Files (create / modify):**
- `crates/tau-runtime/src/sandbox/plan.rs` — `pub fn build_plan(scope: &Scope, plugin_id: &PluginId) -> SandboxPlan` that pulls the plugin's `compute_effective` capability set (already implemented in priority 4) and packages it into a `SandboxPlan` with the plugin's working dir + env.
- `crates/tau-runtime/src/sandbox/validation.rs` — `pub fn validate_plan_against_adapter(plan: &SandboxPlan, adapter: &dyn Sandbox) -> Result<(), Vec<SandboxValidationError>>`. Returns ALL errors (don't short-circuit on first), each with a `plugin_id`, `capability`, and `reason`.
- `crates/tau-runtime/src/sandbox/mod.rs` — re-export both.
- Tests: 4 unit tests against `MockSandbox` covering (a) supported shape → Ok, (b) custom shape → ShapeUnsupported, (c) multiple violations all returned, (d) empty capability list → Ok.

**Summary:**
- This is the load-bearing pre-flight: every plan flowing into `wrap_spawn` is validated against the selected adapter's `supported_shapes` first. Static (Layer 3) validation runs at install time and at `tau check` time (Task 10's advisory mode). Dynamic (Layer 4) validation runs inside `wrap_spawn` and is the same code path. Static is just a no-op spawn wrapper that calls `validate_plan` against a constructed plan without actually executing.
- The `SandboxValidationError` newtype is `#[non_exhaustive]` and carries enough detail to render an actionable `tau resolve --check-sandbox` line: `"plugin foo declares fs.read=[/etc/**] but selected adapter (native, Light) does not support fs.read on the host — try chain entry container or relax to fs.read=[${PROJECT}/**]"`.

**Verification:** standard workspace gates. `cargo test -p tau-runtime sandbox::validation`.

**Commit:** `feat(runtime): Layer 3 pre-flight sandbox plan validation`.

---

### Task 9: Plugin host integration + lockfile schema v3 → v4

**Spec section:** §"Layer 4 — runtime enforcement" + §"Lockfile migration"

**Files (modify):**
- `crates/tau-runtime/src/plugin_host/mod.rs` — locate the existing plugin spawn pipeline (the function that builds a `Command` and spawns the plugin process). Wrap it: build `SandboxPlan` via `sandbox::build_plan(scope, plugin_id)`, call `adapter.wrap_spawn(&plan, &mut cmd)` to apply enforcement, retain the returned `SandboxHandle` in the plugin's runtime record (drops on plugin exit). Existing flow `spawn(command, plugin_id) -> Process` becomes `spawn(command, plugin_id, sandbox: Arc<dyn Sandbox>) -> Process`. Threads the adapter through from the runtime construction site.
- `crates/tau-pkg/src/lockfile.rs` — bump schema version constant from 3 to 4. Add `LockedPlugin.required_shapes: Vec<CapabilityShape>` (default empty for serde back-compat). On load: if `required_shapes` is empty AND schema_version < 4, emit `tracing::warn!("required_shapes missing for plugin {}; falling back to manifest-derived shapes — re-install to refresh", plugin_id)` and derive shapes on-the-fly from the plugin manifest's capabilities.
- `crates/tau-pkg/src/install.rs` — Layer 2 cross-check. After fetching the plugin binary and reading its `CAPABILITIES` handshake response (already done as part of Phase 0 ADR-0008 protocol), compare the binary's declared capabilities against the manifest's `[capabilities]` section. On mismatch: refuse install, exit 2 with `Error: plugin foo binary advertises [fs.read, net.http] but manifest declares [fs.read]; install refused. Update the manifest or use --allow-capability-drift.` (`--allow-capability-drift` is a future flag; for v0.1 we just hard-fail and document the message.) Then write the resolved `required_shapes` (one per capability via `Capability::required_shape`) into the lockfile.
- `crates/tau-runtime/src/plugin_host/tests.rs` (or wherever existing plugin host tests live) — extend with 2-3 new tests asserting (a) wrap_spawn called with mock adapter succeeds, (b) wrap_spawn refused due to ShapeUnsupported produces a clear error, (c) plugin process exits cleanly when adapter handle drops.

**Summary:**
- This is the hot integration. The single integration point is the spawn site in `plugin_host`. Threading `Arc<dyn Sandbox>` through is mechanical but touches several call sites.
- Lockfile migration is additive only; v3 lockfiles continue loading.
- The `tau-plugin-test-support` test scaffolding may need an updated helper that constructs a runtime with a `MockSandbox` adapter wired in. Add a `with_mock_sandbox()` builder method if there's a `RuntimeBuilder` test helper.

**Verification:** standard workspace gates. Crucial: `cargo test -p tau-runtime plugin_host` and `cargo test -p tau-pkg lockfile`. Also `cargo test --doc` to verify any lockfile rustdoc examples still compile.

**Commit:** `feat(runtime,pkg): integrate sandbox at plugin spawn + lockfile v4 (required_shapes)`.

---

### Task 10: `tau resolve --check-sandbox` advisory mode

**Spec section:** §"Layer 3 surfacing — `tau resolve --check-sandbox`"

**Files (modify):**
- `crates/tau-cli/src/cmd/resolve.rs` — add `--check-sandbox` and `--json` flags to the existing args struct. When `--check-sandbox` is passed, after dependency resolution: load the scope's `[sandbox]` config, instantiate the selected adapter, build a plan per resolved plugin via `tau_runtime::sandbox::build_plan`, run `validate_plan_against_adapter` for each, and report findings.
- Tests: `crates/tau-cli/tests/cmd_resolve_check_sandbox.rs` (create) — 4-5 integration tests using `assert_cmd::Command::cargo_bin("tau")` + `tempfile::TempDir`: (a) mock adapter accepts all → exit 0, no errors reported, (b) mock adapter rejects custom shape → exit 2 with the violation rendered, (c) `--json` emits one JSON event per line with shape `{"event": "sandbox_check", "plugin_id": "...", "status": "ok"}` or `{"event": "sandbox_check", "plugin_id": "...", "status": "error", "reason": "...", "capability": "..."}`, (d) no `[sandbox]` config + non-Linux host → exit 2 with the "no sandbox adapter available" message.

**Summary:**
- Reuses Task 8's validation code; this is just CLI plumbing.
- Human output: green `✓` per ok'd plugin, red `✗ <plugin>: <reason>` per violation, and a final summary line `N plugins checked: K ok, M errors`. Mirror the existing `tau resolve` style.
- JSON output: per-line events, terminated by a final `{"event": "summary", "ok": K, "errors": M}` line.
- Three-bucket exit code: 0 if all ok; 2 if any violation OR if no adapter available.

**Verification:** standard workspace gates. `cargo test -p tau-cli cmd_resolve_check_sandbox`.

**Commit:** `feat(cli): tau resolve --check-sandbox advisory mode`.

---

### Task 11: End-to-end integration tests + plugin compatibility verification

**Spec section:** §"Test strategy" + §"Plugin compatibility"

**Files (create / modify):**
- `crates/tau-runtime/tests/sandbox_native.rs` (create) — `#[cfg(target_os = "linux")]` + `#[ignore]`-gated, runs only with `--features integration-tests` on the runtime crate (add the feature). 3-4 tests: (a) fs-read plugin reads an allowed file, (b) fs-read plugin rejected when reading an unlisted file, (c) shell plugin spawns an allowed command, (d) shell plugin rejected when spawning an unlisted command.
- `crates/tau-runtime/tests/sandbox_container.rs` (create) — 2 tests, gated `#[cfg(target_os = "linux")]` + runtime probe for `docker --version` (skip if absent): (a) fs-read works inside container, (b) shell plugin command runs inside container.
- `crates/tau-runtime/tests/sandbox_mismatch.rs` (create) — 3 tests, cross-platform (use mock adapter): (a) plugin requires `Custom` shape, mock rejects, run exits 2 with documented error, (b) project config requests `minimum_tier = Strict`, only Light available, exit 2, (c) lockfile v3 entry without required_shapes — runtime emits warning + falls back to manifest-derived shapes + run succeeds.
- Verify `crates/tau-plugins/anthropic/tau.toml`, `ollama/tau.toml`, `openai/tau.toml`, `fs-read/tau.toml`, `shell/tau.toml` — for each, run `tau resolve --check-sandbox` against the mock adapter (since CI is multi-platform) and confirm the existing capability declarations match the binary's `CAPABILITIES` handshake response. If any plugin's manifest declares a capability the binary does NOT advertise (or vice-versa), file the discrepancy as part of this task and either (1) update the manifest to match the binary (if the binary is the authority) or (2) update the binary to match the manifest (if the manifest is the authority — usually the case).
- Update `.github/workflows/ci.yml` (or whatever the existing CI workflow is — verify with `ls .github/workflows/`): on the `ubuntu-latest` slot only, run `cargo test -p tau-sandbox-native --features integration-tests -- --ignored` and `cargo test -p tau-sandbox-container -- --ignored` and `cargo test -p tau-runtime --features integration-tests -- --ignored`. **No new CI matrix slots; no new jobs; we extend the existing Linux job's `cargo test` invocation.**

**Summary:**
- This task is the "everything works together" gate. It's also where the existing 5 plugins are confirmed compatible.
- Branch protection is 23 required checks. We are NOT adding a 24th; the new test invocations live inside the existing Linux test job.

**Verification:** standard workspace gates. Run the new CI invocations locally to confirm green on Linux.

**Commit:** `test(sandbox): end-to-end integration tests + plugin compatibility`.

---

### Task 12: PAUSE — final local verification + open PR

**This is a user-driven gate.** The implementer agent runs the full local verification suite, confirms everything is green, and then OPENS a draft PR — but does NOT merge it. The user reviews CI, manual testing, and approves the merge in Task 13.

**Steps:**

- [ ] **Step 1: Run the complete local verification suite.**

In order, all must pass:
- `cargo build --workspace`
- `cargo test --workspace --all-targets`
- `cargo test --doc`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- On Linux: `cargo test -p tau-sandbox-native --features integration-tests -- --ignored`
- On Linux with docker: `cargo test -p tau-sandbox-container -- --ignored`
- On Linux: `cargo test -p tau-runtime --features integration-tests -- --ignored`

- [ ] **Step 2: Push the branch.**

Run: `git push -u origin feat/sandboxing-spec`

- [ ] **Step 3: Open a draft PR via gh.**

Use `gh pr create --draft --base main --head feat/sandboxing-spec --title "feat: sandboxing — Linux native + container adapters (Tier 3 priority 12)"` and pass the body via the `--body-file` flag pointing at a temporary file (avoid heredoc syntax). The body must list:
- Summary: 3-5 bullets covering the typed `CapabilityShape` vocabulary, refined `Sandbox` port, `tau-sandbox-native` (Light + Strict), `tau-sandbox-container`, `tau resolve --check-sandbox`, lockfile v4.
- Testing matrix: which test invocations were run on which platforms.
- Out-of-scope: macOS, Windows, remote backends, WASM target, `#[capabilities(...)]` proc macro.
- Linked spec: `docs/superpowers/specs/2026-05-02-sandboxing-design.md`.
- Linked vision: `docs/explanation/tau-as-language.md`.

- [ ] **Step 4: Wait for CI green.**

Monitor with `gh pr checks <pr-number>` until all 23 required checks pass. **Do NOT merge.** Pause here for the user to take over for Task 13.

---

### Task 13: PAUSE — ADR-0014 + ROADMAP Phase 2 stub + squash merge

**This is a user-driven gate.** Once CI is green, the user reviews and approves; THEN the implementer adds documentation deliverables that codify the sub-project and squashes the merge.

**Files (create / modify):**

- [ ] **Step 1: Create `docs/decisions/0014-sandboxing.md`.**

Use the existing ADR template at `docs/decisions/0001-*.md` as a structural reference. The body must cover:
1. **Decision 1 — Hexagonal port + adapter pattern.** Why a single `tau_ports::Sandbox` trait with multiple adapters rather than per-platform conditional compilation. Reference: spec §"Architecture decision 1".
2. **Decision 2 — Linux native first; macOS/Windows/remote in future sub-projects.** Why the v0.1 scope is Linux-only. Reference: spec §"Scope boundary".
3. **Decision 3 — Typed `CapabilityShape` vocabulary, not free-form strings.** Why a typed enum + `#[non_exhaustive]` is the right evolution path. Reference: spec §"Capability shape vocabulary".
4. **Decision 4 — Pre-flight validation hierarchy (4 layers).** Why we eliminate "this will never work" errors at install / check / resolve time. Reference: spec §"Validation hierarchy".
5. **Decision 5 — Adapter chain with probe-based selection.** Why a single config that works across machines (machine-agnosticism via probe + first-available, not via per-machine config files). Reference: spec §"Adapter chain".
6. **Decision 6 — Mock adapter is opt-in only, never silent fallback.** Why no enforcement is so dangerous that we refuse to start rather than silently fall through to mock. Reference: spec §"Mock adapter policy".

Each decision section follows the existing ADR pattern: Context → Decision → Consequences → Alternatives considered.

Add a final **Vision** section that points at `docs/explanation/tau-as-language.md` and lists the seven Phase 2 sub-projects (A-G) it spawns.

- [ ] **Step 2: Update `docs/ROADMAP.md` with the Phase 2 stub.**

Find the existing Phase 2 placeholder (or append a new section if there isn't one). Add:

```markdown
## Phase 2 — Tau as a compiled language for agentic workflows

The sandboxing sub-project (Tier 3 priority 12, ADR-0014) lays the
foundation for Tau as a compiled language. See
`docs/explanation/tau-as-language.md` for the full vision. Phase 2
sub-projects:

- A: `tau check` subcommand (~3 weeks)
- B: Tau target triple registry (~2 weeks)
- C: `tau build --target <triple>` + bundle format (~6 weeks)
- D: Capability vocabulary forward-compatibility (~2 weeks)
- E: Cross-machine reproducibility verification (~3 weeks)
- F: Remote target backends (Vercel Sandbox, Sandcastle, etc) (~4-6 weeks each)
- G: WASM target backend (~12+ weeks)
```

- [ ] **Step 3: Commit + amend the PR with documentation.**

Run: `git add docs/decisions/0014-sandboxing.md docs/ROADMAP.md`

Run: `git commit -m "docs: ADR-0014 sandboxing + ROADMAP Phase 2 stub"`

Run: `git push`

- [ ] **Step 4: Wait for CI to re-run + go green.**

Run: `gh pr checks <pr-number>` until 23/23 pass.

- [ ] **Step 5: Mark PR ready for review (out of draft).**

Run: `gh pr ready <pr-number>`

- [ ] **Step 6: Squash-merge.**

The user approves the PR. Then run: `gh pr merge <pr-number> --squash --delete-branch`.

- [ ] **Step 7: Post-merge verification.**

- `git checkout main && git pull` — confirms clean rebase.
- `cargo build --workspace && cargo test --workspace --all-targets` — confirms main is green locally.

---

## Self-review

**Spec coverage:**
- §1-3 (background, motivation, decisions) → covered by the design itself; ADR-0014 (Task 13) records the decisions.
- §"Capability shape vocabulary" → Task 1.
- §"Refined `Sandbox` port" → Task 2.
- §"Adapter implementations — native Light" → Task 3.
- §"Adapter implementations — native Strict" → Task 4.
- §"Capability shape mapping (network + exec)" → Task 5.
- §"Adapter implementations — container" → Task 6.
- §"Scope config schema + adapter chain" → Task 7.
- §"Validation hierarchy — Layer 3" → Task 8 (validation code) + Task 10 (CLI surfacing).
- §"Validation hierarchy — Layer 2 (install cross-check)" → Task 9.
- §"Layer 4 — runtime enforcement" → Task 9.
- §"Lockfile migration v3 → v4" → Task 9.
- §"Plugin compatibility" → Task 11.
- §"Test strategy" → Tasks 3, 4, 5, 6, 11.
- §"Vision" → ADR-0014 (Task 13) + ROADMAP stub (Task 13).

**Placeholder scan:** searched for "TBD", "TODO", "fill in", "implement later" — none. Tasks 4-11 are intentionally summary-format per the writing-plans skill arguments; each carries a clear filename list, summary, verification, and commit message.

**Type consistency:** `CapabilityShape` (Task 1), `CapabilityShapeSet` (Task 1), `SandboxProbe` (Task 2), `SandboxTier` (Task 2), `SandboxHandle` (Task 2), `SandboxError::{Unavailable, ShapeUnsupported, WrapFailed, Violation}` (Task 2), `NativeSandbox` (Task 3), `ContainerSandbox` (Task 6), `SandboxConfig`, `SandboxAdapterConfig`, `SandboxAdapterKind` (Task 7), `SandboxValidationError` (Task 8). Names match across tasks.

**Plumbing carryovers:** Cargo.lock discipline (Tasks 3, 4); `#[non_exhaustive]` on every public type; cfg-gated Linux code with non-Linux stub returning Unavailable; CI matrix unchanged (no new branch-protection checks); MockSandbox stays in fixtures.rs; the `#[capabilities(...)]` proc macro is explicitly out-of-scope; lockfile v3 → v4 is additive with warning fallback.
