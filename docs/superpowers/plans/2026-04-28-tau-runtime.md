# Tau Runtime (sub-project 4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the `tau-runtime` kernel — the embeddable Rust API surface for tau (G6, QG12). Builds a `Runtime` from registered plugins (LlmBackend, Tool, Storage), runs an agent through a multi-turn batch loop, dispatches messages to tools with typed-capability enforcement (G14), and emits structured logs (G9). Solo path only — orchestration is sub-project 5+.

**Architecture:** One crate (`crates/tau-runtime`) under the existing workspace. Module layout per spec §2: `error`, `builder`, `options`, `outcome`, `capability`, `dispatch`, `run`. Async public API via native `async fn` (no tokio runtime dep — tokio is dev-only). Builder pattern with build-time validation; `Box<dyn Tool<Session = ()>>` v0.1 limitation. `Outcome::Failed` for agent-level failures, `Err(RuntimeError)` for kernel-level errors. `tracing` crate for ~45 events across 9 subsystems. Adds `Tool::capabilities() -> &[Capability]` default method to tau-ports' `Tool` trait as a tightly-coupled additive amendment (ADR-0006 covers both).

**Tech Stack:** Rust stable (workspace MSRV 1.91 per QG7), `thiserror = "2"`, `tracing = "0.1"` (new workspace dep), `tau-domain` (workspace, with `serde`), `tau-ports` (workspace; amended in Task 2 to add `Tool::capabilities()`). Dev-deps: `tokio = "1"` (with `macros + rt + rt-multi-thread`), `tau-ports` with `test-fixtures` feature, `tracing-subscriber = "0.3"` (with `fmt + env-filter`), `proptest = "1"`.

**Spec:** `docs/superpowers/specs/2026-04-28-tau-runtime-design.md`

**Working directory:** `/Users/titouanlebocq/code/tau` on branch `feat/tau-runtime-spec`. The spec landed at `ca8e607`; PR #5 is open as a Draft. All implementation commits on this branch auto-trigger CI on the PR per the established branch-protection workflow (Plans 1–4).

**Commit policy:** every task ends with a Conventional Commits-formatted commit. PR is already open (Draft) and will be marked Ready for review after Task 20 (final local verification). Tasks 21 (ADR-0006 sign-off, 24h wait or self-review per QG22) and 22 (Plan 5 sign-off + merge) are user-driven gates.

**Note on TDD strictness:** for tasks producing parsers / validators / branching logic (capability `satisfies` relation in Task 8, agent loop in Task 10, integration tests in Tasks 11–16) follow strict red-green-refactor: write the failing test first, watch it fail, implement, watch it pass. For tasks producing pure data shapes (Tasks 3–7, 9) the cycle collapses — write the type + its tests in one step, then verify the suite passes.

**Plan-erratum carry-overs from sub-projects 1 + 2 + 3 (apply preemptively):**

- **Doctests on `#[non_exhaustive]` types must be marked `ignore`** because doctests compile as external crates and external struct-literal construction of non-exhaustive types fails E0639. Most public types in tau-runtime are `#[non_exhaustive]`. Use the established pattern: `/// ```ignore` plus a one-line comment explaining the constraint.
- **`cargo test --all-targets` does NOT include doctests.** Verification steps that mention "doctests pass" must explicitly run `cargo test -p tau-runtime --doc` separately.
- **For struct-pattern destructuring of `#[non_exhaustive]` enums in tests:** use `let X { fields, .. } = value else { panic!() };` (let-else) rather than irrefutable `let X { fields } = value;`.
- **Same-commit escape-hatch registration**: any commit that introduces a new `Internal` variant on an error enum MUST also append the corresponding entry to `docs/explanation/escape-hatches.md`. The mechanical CI test `crates/tau-domain/tests/escape_hatch_registry.rs` enforces this; failure to register breaks the matrix tests on PR #5.
- **`Tool` trait amendment in Task 2 must be backwards-compatible**: the `capabilities()` default of `&[]` keeps existing impls (the four mocks in `tau_ports::fixtures`) compiling without changes. CI verifies via `cargo test -p tau-ports --all-targets --all-features`.
- **Plugin trait objects use `Arc`, not `Box`, internally**: `RuntimeBuilder` accepts `Box<dyn ...>` for ergonomic call-site construction but converts to `Arc<dyn ...>` for the runtime registries (clone-cheap for future async dispatch needs).

---

## File Structure

| Path | Responsibility | Created in |
|---|---|---|
| `Cargo.toml` (workspace root) | Add `tracing` to `[workspace.dependencies]` | Task 1 |
| `crates/tau-runtime/Cargo.toml` | Add deps + features + dev-deps | Task 1 |
| `crates/tau-ports/src/tool.rs` | Add `Tool::capabilities() -> &[Capability]` default method | Task 2 |
| `crates/tau-runtime/src/lib.rs` | Module declarations, re-exports, crate-level rustdoc | Tasks 1, 3, 4, 5, 6, 7, 8, 9, 10 |
| `crates/tau-runtime/src/error.rs` | `PluginKind` + `BuildError` + `CapabilityDenial` (Task 3); `RuntimeError` with `#[from]` composition (Task 4) | Tasks 3, 4 |
| `crates/tau-runtime/src/options.rs` | `RunOptions` + `TokenUsage` + `Default` impl | Task 5 |
| `crates/tau-runtime/src/outcome.rs` | `RunOutcome` enum (`Completed` / `Failed`) | Task 6 |
| `crates/tau-runtime/src/builder.rs` | `Runtime` + `RuntimeBuilder` + plugin registries + `build()` validation | Task 7 |
| `crates/tau-runtime/src/capability.rs` | Per-variant satisfies functions + `check_capabilities` top-level | Task 8 |
| `crates/tau-runtime/src/dispatch.rs` | `resolve_llm_backend`, `resolve_tool`, address-to-tool resolution | Task 9 |
| `crates/tau-runtime/src/run.rs` | Agent multi-turn loop + tracing instrumentation per spec §3.9 vocabulary | Task 10 |
| `crates/tau-runtime/tests/run_completed.rs` | Integration: happy path, no tool_uses | Task 11 |
| `crates/tau-runtime/tests/run_with_tool_calls.rs` | Integration: multi-turn dispatch with tool round-trip | Task 12 |
| `crates/tau-runtime/tests/run_capability_denied.rs` | Integration: `PolicyDenied` path | Task 13 |
| `crates/tau-runtime/tests/run_max_turns.rs` | Integration: `OutOfResources` path | Task 14 |
| `crates/tau-runtime/tests/run_kernel_errors.rs` | Integration: `LlmBackendNotRegistered`, `ToolNotRegistered`, `PluginContractViolation` (3 `#[test]`s) | Task 15 |
| `crates/tau-runtime/tests/tracing_emission.rs` | Integration: tracing-subscriber capture + assert vocabulary | Task 16 |
| `crates/tau-runtime/tests/proptest_capability_satisfies.rs` | Property tests for the satisfies-relation | Task 17 |
| `.github/workflows/ci.yml` | Add `no-default-features-runtime` job | Task 18 |
| `docs/explanation/escape-hatches.md` | Append 2 new entries (`builderror-internal`, `runtimeerror-internal`) — same-commit-as-introduction in Tasks 3 and 4 | Tasks 3, 4 |
| `docs/decisions/0006-tau-runtime.md` | ADR-0006 (kernel + Tool::capabilities() amendment) | Task 19 |
| `docs/decisions/README.md` | Add ADR-0006 row to index | Task 19 |
| `ROADMAP.md` | Mark sub-project 4 complete | Task 22 |
| `docs/superpowers/plans/2026-04-28-tau-runtime.md` | This plan; checkboxes ticked at sign-off | Task 22 |

---

## Task 1: Workspace + crate dependency setup

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/tau-runtime/Cargo.toml`

- [x] **Step 1.1: Add `tracing` to workspace deps**

Open `/Users/titouanlebocq/code/tau/Cargo.toml`. Locate `[workspace.dependencies]`. Add the `tracing` line at the end:

```toml
[workspace.dependencies]
tau-domain      = { path = "crates/tau-domain", version = "0.0.0" }
thiserror       = "2"
semver          = { version = "1" }
uuid            = { version = "1", features = ["v7"] }
url             = "2"
serde           = { version = "1", features = ["derive"] }
base64          = "0.22"
proptest        = "1"
walkdir         = "2"
futures-core    = "0.3"
toml            = "0.8"
fs4             = "0.8"
humantime-serde = "1"
tempfile        = "3"
tracing         = "0.1"
```

(All other entries remain unchanged from prior sub-projects.)

- [x] **Step 1.2: Update `crates/tau-runtime/Cargo.toml`**

Replace the file contents with:

```toml
[package]
name = "tau-runtime"
description = "Public Rust API surface for embedding tau as a library."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[dependencies]
tau-domain = { workspace = true, features = ["serde"] }
tau-ports  = { workspace = true }
thiserror  = { workspace = true }
tracing    = { workspace = true }

[features]
default = []

[dev-dependencies]
tokio              = { version = "1", features = ["macros", "rt", "rt-multi-thread"] }
tau-ports          = { workspace = true, features = ["test-fixtures"] }
tracing-subscriber = { version = "0.3", features = ["fmt", "env-filter"] }
proptest           = { workspace = true }
```

**Why each dep:**
- `tau-domain` with `serde` feature: kernel reads `AgentDefinition`, `PackageManifest`, `Message`, `Capability` types.
- `tau-ports`: plugin trait surface (`LlmBackend`, `Tool`, `Storage`, `Sandbox`).
- `thiserror`: per QG2.
- `tracing`: structured logs per spec §3.9 vocabulary.
- `tokio` (dev only): drives `#[tokio::test]` for async integration tests. NOT a runtime dep — kernel uses native `async fn`.
- `tau-ports` with `test-fixtures` (dev only): `MockLlmBackend`, `MockTool`, `MockStorage` for integration tests.
- `tracing-subscriber` (dev only): captures emitted events in `tests/tracing_emission.rs`.
- `proptest` (dev only): generative tests for capability satisfies-relation.

- [x] **Step 1.3: Verify the no-default-features build**

```bash
cd /Users/titouanlebocq/code/tau
cargo build -p tau-runtime --no-default-features
```

Expected: success. The current `lib.rs` is a doc-comment-only stub; nothing depends on the new deps yet.

- [x] **Step 1.4: Verify the all-features build**

```bash
cargo build -p tau-runtime --all-features
```

Expected: success.

- [x] **Step 1.5: Verify nothing else regressed**

```bash
cargo build --workspace --all-features
cargo test -p tau-domain --all-targets
cargo test -p tau-ports --all-targets --all-features
cargo test -p tau-pkg --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: all exit 0.

- [x] **Step 1.6: Stage and commit**

```bash
git add Cargo.toml Cargo.lock crates/tau-runtime/Cargo.toml
git commit -m "build(tau-runtime): add deps and feature flags

Adds tau-domain (with serde), tau-ports, thiserror, tracing as runtime
deps. Dev-deps: tokio (with macros + rt + rt-multi-thread), tau-ports
with test-fixtures, tracing-subscriber, proptest. One feature: default
(empty).

Adds tracing = \"0.1\" to workspace.dependencies for shared use.

tokio is a dev-dep, NOT a runtime dep — tau-runtime uses native async fn,
callers pick the executor (tau-cli will bring tokio at the binary level).
This keeps the kernel async-runtime-agnostic in code.

No tau-pkg dep — caller passes the PackageManifest to Runtime::run
(spec §2, ADR-0006 §6).

Refs: spec §2 (sub-project 4), QG2"
```

Push to trigger CI on PR #5:

```bash
git push origin feat/tau-runtime-spec
```

- [x] **Step 1.7: Verify CI on PR #5 is green**

```bash
gh pr checks 5
```

Expected: all required checks pass (none of the new deps are used yet, so this is a sanity check that the workspace still compiles in every CI matrix entry). Don't block on Windows jobs (slow); they're non-blocking per G15.

---

## Task 2: tau-ports Tool::capabilities() additive amendment

**Files:**
- Modify: `crates/tau-ports/src/tool.rs`
- Modify: `crates/tau-ports/src/llm.rs` or wherever the trait is documented (no changes expected; just verify)

This task adds the typed-capability declaration method to tau-ports' `Tool` trait. Backwards-compatible: existing impls (the four mocks in `tau_ports::fixtures`) continue to compile without changes thanks to the default `&[]` return.

ADR-0006 covers this amendment AND the kernel decisions in one bundled ADR (per spec §6) because they're tightly coupled — the trait amendment exists solely because tau-runtime needs typed enforcement.

- [x] **Step 2.1: Locate the `Tool` trait**

Open `/Users/titouanlebocq/code/tau/crates/tau-ports/src/tool.rs`. Find the `Tool` trait definition (around line 145 per the existing file structure):

```rust
#[allow(async_fn_in_trait)]
pub trait Tool: Send + Sync {
    /// Per-session state. Use `()` for stateless tools (or use [`StatelessAdapter`]).
    type Session: Send + 'static;

    /// Stable name used for routing. SemVer-stable surface.
    fn name(&self) -> &str;

    /// JSON Schema describing the tool's input. Used both for runtime
    /// validation and for surfacing to the LLM via
    /// `CompletionRequest.tools`.
    fn schema(&self) -> ToolSpec;

    /// Open a session. Called once before any `invoke`.
    async fn init(&self, ctx: SessionContext) -> Result<Self::Session, ToolError>;

    /// Perform a single tool call within an open session.
    async fn invoke(
        &self,
        session: &mut Self::Session,
        args: Value,
    ) -> Result<ToolResult, ToolError>;

    /// Close the session gracefully. If the runtime drops the session
    /// future (cancellation), `teardown` is NOT called — plugin authors
    /// put critical cleanup in `Drop`.
    async fn teardown(&self, session: Self::Session) -> Result<(), ToolError>;
}
```

- [x] **Step 2.2: Add the `capabilities()` default method**

Insert the new method between `schema()` and `init()`:

```rust
    /// Capabilities this tool requires the calling agent's package to declare.
    /// Default: empty (tool is unrestricted; any agent can call it).
    ///
    /// The runtime checks: for every capability in this list, the agent's
    /// package manifest must contain at least one capability that satisfies
    /// it. See `tau_runtime::capability::check_capabilities` for the
    /// satisfies-relation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // `Capability` is `#[non_exhaustive]`; declared via the manifest path.
    /// use tau_domain::Capability;
    /// use tau_ports::Tool;
    ///
    /// struct MyFileTool;
    /// impl Tool for MyFileTool {
    ///     // ... other methods ...
    ///     fn capabilities(&self) -> &[Capability] { &[] }
    /// }
    /// ```
    fn capabilities(&self) -> &[tau_domain::Capability] {
        &[]
    }
```

The default `&[]` makes this backwards-compatible: existing impls don't need to declare anything.

- [x] **Step 2.3: Verify tau-ports' existing tests pass**

```bash
cd /Users/titouanlebocq/code/tau
cargo test -p tau-ports --all-targets --all-features
cargo test -p tau-ports --doc --all-features
```

Expected: all green. The four mocks (`MockLlmBackend`, `MockTool`, `MockStorage`, `MockSandbox`) and any other `impl Tool` in the codebase continue to compile and pass without modification — they pick up the default `&[]`.

- [x] **Step 2.4: Verify the rest of the workspace**

```bash
cargo build --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: all exit 0. tau-domain and tau-pkg don't depend on `Tool::capabilities()`; nothing should break.

- [x] **Step 2.5: Stage and commit**

```bash
git add crates/tau-ports/src/tool.rs
git commit -m "feat(tau-ports): add Tool::capabilities() default method

Adds a new associated function to the Tool trait:

    fn capabilities(&self) -> &[tau_domain::Capability] { &[] }

The default empty slice keeps existing impls compiling without
changes (the four mocks in tau-ports::fixtures pick it up
transparently). Tools that need typed capability declarations
override the method and return a static slice of required capabilities.

The runtime (sub-project 4 — tau-runtime) checks: for every
capability returned here, the agent's package manifest must contain
at least one capability that satisfies it via the satisfies-relation
in tau_runtime::capability.

This amendment is part of ADR-0006 (forthcoming) — the trait change
exists solely because tau-runtime needs typed enforcement (G14). It
ships bundled in the same PR as the tau-runtime kernel because the
two changes are tightly coupled.

Refs: spec §3.6, G14, ADR-0006 (forthcoming)"

git push origin feat/tau-runtime-spec
```

---

## Task 3: error.rs leaf errors (PluginKind, BuildError, CapabilityDenial) + escape-hatch builderror-internal

**Files:**
- Create: `crates/tau-runtime/src/error.rs`
- Modify: `crates/tau-runtime/src/lib.rs`
- Modify: `docs/explanation/escape-hatches.md` (append builderror-internal entry — same-commit-as-introduction policy)

`BuildError` carries the `Internal` escape-hatch variant; per the policy enforced by `crates/tau-domain/tests/escape_hatch_registry.rs` (sub-project 1), the entry registers in this same commit.

- [x] **Step 3.1: Create `error.rs` with `PluginKind`, `BuildError`, `CapabilityDenial`**

Create `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/error.rs`:

```rust
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
//! - [`RuntimeError`] — kernel-level operational failures during
//!   `Runtime::run`. Composes `tau_ports` plugin errors via `#[from]`.
//!   Agent-level failures (capability denied, max turns reached) are
//!   reported via `Ok(RunOutcome::Failed { status: AgentStatus::Failed })`,
//!   NOT `Err(RuntimeError)`.
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
            PluginKind::Tool       => f.write_str("tool"),
            PluginKind::Storage    => f.write_str("storage"),
            PluginKind::Sandbox    => f.write_str("sandbox"),
        }
    }
}

/// Errors from [`crate::RuntimeBuilder::build`].
///
/// # Example
///
/// ```ignore
/// // `BuildError` is `#[non_exhaustive]`; constructed by `build()`.
/// use tau_runtime::{Runtime, BuildError};
///
/// let err = Runtime::builder().build().unwrap_err();
/// assert!(matches!(err, BuildError::NoLlmBackend));
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
/// NOT a variant of [`RuntimeError`] — capability denial is an
/// agent-level failure (`Ok(RunOutcome::Failed)`), not a kernel-level
/// error (`Err(RuntimeError)`). See ADR-0006 for the dichotomy.
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
}
```

- [x] **Step 3.2: Wire the module into `lib.rs`**

Replace `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/lib.rs` contents with:

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Public Rust API surface for embedding tau as a library. One of
//! tau's two stable surfaces (G6, QG12); the other is the serve-mode
//! protocol (sub-project 5+).
//!
//! tau-runtime is the kernel: it loads pre-constructed plugin
//! instances (LlmBackend, Tool, Storage), runs an agent through a
//! multi-turn batch loop, dispatches messages to tools with typed-
//! capability enforcement (G14), and emits structured logs (G9).
//!
//! Solo path only at v0.1 — orchestration of multiple agents is
//! sub-project 5+ (G10).
//!
//! See `docs/decisions/0006-tau-runtime.md` for the design rationale.

pub mod error;

pub use error::{BuildError, CapabilityDenial, PluginKind};
```

- [x] **Step 3.3: Append `builderror-internal` to escape-hatches.md**

Open `/Users/titouanlebocq/code/tau/docs/explanation/escape-hatches.md`. Locate the "Active escape hatches" table. Append one new row after the last existing row (which is `uninstallerror-internal` from sub-project 3):

```
| <a id="builderror-internal"></a>`builderror-internal` | `BuildError::Internal { message }` | catch-all for invariant violations during `RuntimeBuilder::build()` not yet covered by typed variants | promote when 2+ distinct contexts surface | 4 |
```

- [x] **Step 3.4: Run unit tests**

```bash
cd /Users/titouanlebocq/code/tau
cargo test -p tau-runtime --lib
```

Expected: 4 tests pass (`plugin_kind_display`, `build_error_no_llm_backend_display`, `build_error_name_collision_display`, `capability_denial_display_includes_all_fields`).

- [x] **Step 3.5: Run doctests, clippy, fmt**

```bash
cargo test -p tau-runtime --doc
cargo clippy -p tau-runtime --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: all exit 0. Doctest count: 1 ignored (the `BuildError` example).

- [x] **Step 3.6: Verify the cross-crate registry gate**

```bash
cargo test -p tau-domain --test escape_hatch_registry --all-features
```

Expected: pass. The `BuildError::Internal` variant in `crates/tau-runtime/src/error.rs` matches the newly-registered `builderror-internal` anchor in `escape-hatches.md`.

- [x] **Step 3.7: Verify no-default-features build**

```bash
cargo build -p tau-runtime --no-default-features
```

Expected: success.

- [x] **Step 3.8: Stage and commit**

```bash
git add crates/tau-runtime/src/error.rs \
        crates/tau-runtime/src/lib.rs \
        docs/explanation/escape-hatches.md

git commit -m "feat(tau-runtime): add PluginKind, BuildError, CapabilityDenial

Adds the leaf error types for tau-runtime's BuildError and runtime-
denial paths. All #[non_exhaustive] with uniform Debug + Clone +
PartialEq + Eq + Error derives.

PluginKind tags plugin types in error messages and tracing fields
(LlmBackend, Tool, Storage, Sandbox).

BuildError carries the failures from RuntimeBuilder::build(): no LLM
backend, name collision, internal escape-hatch.

CapabilityDenial is a helper type embedded as the detail string of
AgentStatus::Failed{kind: PolicyDenied} when capability enforcement
rejects a tool call. NOT a variant of RuntimeError — the dichotomy is
documented in ADR-0006: agent-level failures flow through
RunOutcome::Failed; kernel-level errors flow through Err(RuntimeError).

Registers builderror-internal in docs/explanation/escape-hatches.md
per the same-commit-as-introduction policy enforced by the mechanical
registry CI test.

4 unit tests verify Display rendering for all three types.

Refs: QG2, QG3, ADR-0002, spec §3.1"

git push origin feat/tau-runtime-spec
```

---

## Tasks 4-22: see appendix below

> **Note for plan executors:** the remaining 19 tasks follow the same patterns established in Tasks 1-3 and prior plans (tau-pkg's hybrid Tasks 4-20 in particular). Rather than reproducing thousands more lines of plan text, this plan delegates to the spec for code shapes and to Plans 2 + 3 + 4 for verification cadence.
>
> For each remaining task, the executor (subagent-driven-development) should:
> 1. Read the relevant section of `docs/superpowers/specs/2026-04-28-tau-runtime-design.md` (cited per task below).
> 2. Apply the established patterns from prior plans: TDD where parsers/validators/I/O are involved, mechanical-write where pure data; verify with `cargo test --lib`, `cargo test --doc`, `cargo build --no-default-features`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo fmt --check`; commit with the prescribed message; push to `feat/tau-runtime-spec`.
> 3. Apply the plan-erratum carry-overs: `ignore` doctests on non_exhaustive types; run `cargo test --doc` separately from `--all-targets`; use `let X { fields, .. } = value else { panic!() };` for non_exhaustive destructuring in tests; same-commit escape-hatch registration when introducing new `Internal` variants.
> 4. Surface plan deviations or ambiguity to the controller before committing.

| # | Task | Spec section | Commit message |
|---|---|---|---|
| 4 | `RuntimeError` with `#[from]` composition + escape-hatch `runtimeerror-internal` | §3.1 (RuntimeError) | `feat(tau-runtime): add RuntimeError with #[from] composition` |
| 5 | `RunOptions` + `TokenUsage` + `Default` | §3.2 | `feat(tau-runtime): add RunOptions and TokenUsage` |
| 6 | `RunOutcome` enum (`Completed` / `Failed`) | §3.3 | `feat(tau-runtime): add RunOutcome enum` |
| 7 | `Runtime` + `RuntimeBuilder` + plugin registries + `build()` validation | §3.4 | `feat(tau-runtime): add Runtime builder with plugin registration` |
| 8 | `capability.rs`: per-variant satisfies + `check_capabilities` | §3.5 | `feat(tau-runtime): add capability satisfies-relation` |
| 9 | `dispatch.rs`: resolution helpers (LLM, tool, address) | §3.8 | `feat(tau-runtime): add dispatch resolution helpers` |
| 10 | `run.rs`: agent multi-turn loop with tracing instrumentation | §3.7 + §3.9 | `feat(tau-runtime): add agent multi-turn run loop` |
| 11 | Integration test: `run_completed` (happy path) | §5 | `test(tau-runtime): add run_completed integration test` |
| 12 | Integration test: `run_with_tool_calls` (multi-turn dispatch) | §5 | `test(tau-runtime): add run_with_tool_calls integration test` |
| 13 | Integration test: `run_capability_denied` (PolicyDenied path) | §5 | `test(tau-runtime): add run_capability_denied integration test` |
| 14 | Integration test: `run_max_turns` (OutOfResources path) | §5 | `test(tau-runtime): add run_max_turns integration test` |
| 15 | Integration tests: kernel errors (`run_kernel_errors.rs` with 3 `#[test]`s) | §5 | `test(tau-runtime): add kernel-level error integration tests` |
| 16 | Integration test: `tracing_emission` (vocabulary verification) | §3.9 + §5 | `test(tau-runtime): add tracing emission integration test` |
| 17 | Proptest: `proptest_capability_satisfies` | §4 | `test(tau-runtime): add proptest for capability satisfies-relation` |
| 18 | CI: `no-default-features-runtime` job | §5 (CI implications) | `ci: add tau-runtime no-default-features job` |
| 19 | ADR-0006 (filed Proposed) + README index update | §6 | `docs(adr): ADR-0006 tau-runtime kernel + Tool::capabilities() amendment` |
| 20 | Final local verification + mark PR #5 ready | §1 Done-when | (no commit; verification only) |
| 21 | ADR-0006 sign-off (24h fresh-eyes review per QG22, or self-review-checklist) | QG22 | (no commit; self-review only) |
| 22 | Plan 5 sign-off + ROADMAP + plan tick-off + branch-protection update + merge | QG22, PG4 | `docs: sign off Plan 5 (tau-runtime)` |

### Detailed task bodies (for the executor to expand)

#### Task 4: `RuntimeError` with `#[from]` composition

Spec §3.1 (RuntimeError section). Append the `RuntimeError` enum to `crates/tau-runtime/src/error.rs`. Variants per spec: `LlmBackendNotRegistered`, `ToolNotRegistered`, `PluginContractViolation`, `Llm(#[from] LlmError)`, `Tool(#[from] ToolError)`, `Storage(#[from] StorageError)`, `Sandbox(#[from] SandboxError)`, `Manifest(#[from] PackageManifestError)`, `Internal { message }`.

Update `lib.rs` re-export to include `RuntimeError`.

Add to `docs/explanation/escape-hatches.md`:
```
| <a id="runtimeerror-internal"></a>`runtimeerror-internal` | `RuntimeError::Internal { message }` | catch-all for kernel-level invariant violations during `Runtime::run` not yet covered by typed variants | promote when 2+ distinct contexts surface | 4 |
```

Tests: 7 unit tests verifying `?`-composition for each `#[from]` arrow + display of `LlmBackendNotRegistered`, `ToolNotRegistered`, `PluginContractViolation`. Use `matches!()` not equality.

Verification: `cargo test -p tau-runtime --lib`, `cargo test -p tau-domain --test escape_hatch_registry --all-features` (verifies same-commit registration), the rest of the standard verification suite.

#### Task 5: `RunOptions` + `TokenUsage`

Spec §3.2. Mechanical write. `RunOptions::default()` returns `max_turns: 16`, `trace_label: None`. `TokenUsage::default()` returns all zeros. Both `#[non_exhaustive]`.

Update `lib.rs` to add `pub mod options;` and `pub use options::{RunOptions, TokenUsage};`.

3 unit tests: `RunOptions::default` field values, `TokenUsage::default` field values, custom `RunOptions { max_turns: 100, trace_label: Some(...) }` construction works.

#### Task 6: `RunOutcome` enum

Spec §3.3. Two variants: `Completed { final_message, all_messages, total_turns, token_usage }` and `Failed { status, all_messages, total_turns, token_usage }`. `#[non_exhaustive]`. Derives `Debug + Clone + PartialEq` (NOT `Eq` because `Message` carries a `Value` which is not `Eq` — verify by reading tau-domain's Message derives; if `PartialEq + Eq` both available, derive both).

Update `lib.rs` to add `pub mod outcome;` and `pub use outcome::RunOutcome;`.

2 unit tests: construct each variant via struct literal, verify field accessors work.

#### Task 7: `Runtime` + `RuntimeBuilder` + plugin registries + `build()`

Spec §3.4. Strict TDD because `build()` has real validation logic.

Implementation notes:
- `Runtime` struct: `llm_backends: HashMap<String, Arc<dyn LlmBackend>>`, `tools: HashMap<String, Arc<dyn Tool<Session = ()>>>`, `storages: HashMap<String, Arc<dyn Storage>>`. `#[derive(Clone)]` is OK (Arc is cheap to clone) but the type is intended to be held once.
- `RuntimeBuilder` struct: `Vec` accumulators for each kind. Empty by default.
- `with_llm_backend(Box<dyn LlmBackend>)`, `with_tool(Box<dyn Tool<Session = ()>>)`, `with_storage(Box<dyn Storage>)`: convert to `Arc` and push.
- `build()`: validate ≥1 LLM backend, validate no name collisions per kind, return `Runtime` or `BuildError`.

Helper: `fn collect_by_name<P: ?Sized>(items: Vec<Arc<P>>, kind: PluginKind, name: impl Fn(&P) -> &str) -> Result<HashMap<String, Arc<P>>, BuildError>` — iterates, detects collisions, builds the map. The trait-object-by-name pattern needs care because `dyn LlmBackend` has `name(&self) -> &str` directly, but the function generic over `P: ?Sized` accepts a closure for name extraction.

Actually, simpler: write three separate `collect_*` functions per kind (no generic; each calls `plugin.name()` directly). Avoids fighting the type system over `?Sized` trait objects.

Tests: `build_with_no_llm_backend_returns_no_llm_backend`, `build_with_two_llm_with_same_name_returns_collision`, `build_with_unique_llms_succeeds`, `build_with_zero_tools_succeeds` (tools optional), `build_with_two_tools_same_name_returns_collision`, `build_with_zero_storages_succeeds`. Use `MockLlmBackend::new("name")` from `tau_ports::fixtures` to construct test instances. NOTE: must enable `tau-ports/test-fixtures` feature on the test (it's in dev-deps already).

Update `lib.rs` to add `pub mod builder;` and `pub use builder::{Runtime, RuntimeBuilder};`.

Doctest on `Runtime::builder()`: `ignore`-marked because `Runtime` is `#[non_exhaustive]`-impossible-to-construct externally OR because the example would need a real `MockLlmBackend` import.

#### Task 8: `capability.rs`: satisfies-relation + `check_capabilities`

Spec §3.5. Strict TDD because the satisfies-relation has real logic.

Implementation notes:
- `pub(crate) fn capability_satisfies(granted: &Capability, required: &Capability) -> bool` — variant-by-variant matching. Different variants never satisfy each other. Same-variant: per-variant logic.
- Per-variant satisfies functions (all `pub(crate)`):
  - `fs_satisfies(granted: &FsCapability, required: &FsCapability) -> bool`: only `Read` matches `Read`, only `Write` matches `Write`. Path glob match: every required path matches at least one grant pattern.
  - `net_satisfies`: `Http` matches `Http`. Hosts subset, methods subset.
  - `process_satisfies`: executable path subset, arg pattern subset.
  - `agent_satisfies`: `Spawn` kinds subset, packages subset.
  - `custom_params_satisfy(granted: &BTreeMap<String, Value>, required: &BTreeMap<String, Value>) -> bool`: every required key exists in granted with equal Value. Conservative.
- Glob matching at v0.1: simple inline matcher, no `globset` dep. Accept `**` (any depth), `*` (single segment), exact match. Pseudocode:

  ```rust
  fn glob_matches(pattern: &str, candidate: &str) -> bool {
      // Implement: split by '/', then per-segment match where '**'
      // matches any number of segments, '*' matches a single segment,
      // and other strings match exactly. Handle leading '/' and the
      // empty-pattern edge case.
  }
  ```

  Detailed implementation deferred to the executor; aim for correctness on the common cases (`/tmp/**`, `/tmp/*.txt`, `/etc/passwd`) with unit tests covering all of them.
- `pub(crate) fn check_capabilities(granted: &[Capability], required: &[Capability]) -> Option<&Capability>` — returns the FIRST missing capability, or `None` if all satisfied.

Tests (≥10 unit tests):
- `fs_read_grant_satisfies_fs_read_required` (matching variants succeed).
- `fs_read_grant_does_not_satisfy_fs_write_required` (different sub-variants fail).
- `fs_glob_grant_satisfies_specific_required` (`/tmp/**` satisfies `/tmp/foo.txt`).
- `fs_specific_grant_does_not_satisfy_glob_required` (`/tmp/foo.txt` does NOT satisfy `/tmp/**`).
- `net_http_grant_satisfies_subset_methods`.
- `net_http_grant_does_not_satisfy_method_outside_grant`.
- `custom_params_exact_match_satisfies`.
- `custom_params_extra_required_key_fails`.
- `check_capabilities_with_empty_required_returns_none`.
- `check_capabilities_returns_first_missing_when_some_unsatisfied`.

Update `lib.rs` to add `pub(crate) mod capability;` (NOT publicly exported — it's a kernel internal).

#### Task 9: `dispatch.rs`: resolution helpers

Spec §3.8. Pure logic, no I/O.

Implementation:
- `pub(crate) fn resolve_llm_backend<'a>(runtime: &'a Runtime, agent_id: &str, backend_name: &str) -> Result<&'a Arc<dyn LlmBackend>, RuntimeError>` — `HashMap::get`, return `LlmBackendNotRegistered` on miss.
- `pub(crate) fn resolve_tool<'a>(runtime: &'a Runtime, tool_name: &str) -> Result<&'a Arc<dyn Tool<Session = ()>>, RuntimeError>` — same. On miss, populate `registered: Vec<String>` from the registry's keys for diagnostics.
- `pub(crate) fn address_to_tool_name(addr: &Address) -> Option<&str>` — returns `Some(name)` if `Address::Tool(name)`, else `None`.

Tests: 5 unit tests covering present/absent for each resolver + address pattern matching. Use a builder-built test `Runtime` with one mock LLM and one mock tool.

Update `lib.rs` to add `pub(crate) mod dispatch;`.

#### Task 10: `run.rs`: agent multi-turn loop with tracing

Spec §3.7 + §3.9. The HEART of the kernel. Strict TDD via integration tests in Tasks 11-16; this task's unit tests cover only the helpers used by the loop.

Implementation skeleton (full impl deferred to executor):

```rust
#[instrument(
    name = "runtime.agent_run",
    skip_all,
    fields(
        agent_id = %agent_def.id,
        display_name = %agent_def.display_name,
        package_id = %agent_def.package,
        llm_backend_name = %agent_def.llm_backend,
        max_turns = options.max_turns,
    ),
)]
pub async fn run(
    &self,
    agent_def: AgentDefinition,
    package_manifest: PackageManifest,
    initial_message: Message,
    options: RunOptions,
) -> Result<RunOutcome, RuntimeError> {
    // 1. Load capabilities from package_manifest.capabilities()
    //    Emit: runtime.capability_set_loaded
    // 2. Resolve LLM backend by agent_def.llm_backend.as_str()
    //    Emit: dispatch.tool_resolved (mis-emit acknowledged: it's actually for LLM)
    //    Actually emit: runtime.run_started
    // 3. Initialize messages: vec![initial_message]
    // 4. Loop:
    //    a. Emit runtime.turn_started
    //    b. Build CompletionRequest from messages + tools' schemas
    //       Emit: llm.request_built
    //    c. Span llm.complete: backend.complete(req).await
    //       Emit: llm.response_received, llm.token_usage, llm.stop_reason
    //    d. For each tool_use in response.tool_uses:
    //       i.   Emit: llm.tool_use_emitted
    //       ii.  Span dispatch.tool: resolve_tool(...)
    //       iii. Span capability.check: check_capabilities(...)
    //            If denial: emit capability.deny, return Ok(RunOutcome::Failed{PolicyDenied})
    //            Else: emit capability.allow
    //       iv.  Span tool.session_open: tool.init(ctx).await
    //       v.   Span tool.invoke: tool.invoke(&mut session, args).await
    //            Emit: tool.result_received
    //       vi.  Span tool.session_close: tool.teardown(session).await
    //       vii. Append tool_use + tool_result to messages
    //    e. If response.tool_uses is empty: emit runtime.loop_terminated, return Completed
    //    f. If turn count == options.max_turns:
    //       Emit runtime.max_turns_reached
    //       Return Ok(RunOutcome::Failed{kind: OutOfResources})
    //    g. Increment turn, loop
    // 5. End of run: emit runtime.run_completed
}
```

The actual implementation is ~150-250 lines. Integration tests (Tasks 11-16) drive the full behavior. Unit tests in `run.rs` itself cover only:
- A helper to build the initial messages list.
- A helper to deserialize tool_use args (catches `PluginContractViolation` for malformed args).
- A helper to construct the `RunOutcome::Failed { kind: PolicyDenied, .. }` with the `CapabilityDenial` detail.

Update `lib.rs` to declare `mod run;` (no re-export — `Runtime::run` is the public surface).

#### Task 11: Integration test `run_completed`

Create `crates/tau-runtime/tests/run_completed.rs`:

```rust
//! Integration test: agent runs through one turn (no tool_uses), returns Completed.

use tau_domain::{AgentDefinition, /* etc. */};
use tau_ports::fixtures::{MockLlmBackend, MockTool, MockStorage};
use tau_runtime::{Runtime, RunOptions, RunOutcome};

#[tokio::test]
async fn run_completes_with_text_response() {
    // 1. MockLlmBackend configured to return CompletionResponse {
    //      text: "hello", tool_uses: vec![], stop_reason: ...
    //    }
    // 2. Runtime::builder().with_llm_backend(...).with_tool(...).with_storage(...).build().unwrap()
    // 3. AgentDefinition::new(id, "Agent", package_id, llm_backend_name)
    // 4. PackageManifest with empty capabilities
    // 5. Initial Message::text(...)
    // 6. runtime.run(agent_def, manifest, msg, RunOptions::default()).await.unwrap()
    // 7. assert!(matches!(outcome, RunOutcome::Completed { .. }))
    // 8. Verify final_message text, total_turns == 1, all_messages.len() == 2 (initial + response)
}
```

Detailed setup (constructing `AgentDefinition`, `PackageManifest`, `Message`) per the actual constructors in tau-domain. The executor will read tau-domain's public API and adapt.

#### Task 12: Integration test `run_with_tool_calls`

Multi-turn flow:
- Turn 1: `MockLlmBackend` returns response with `tool_uses: vec![ToolUse{tool_name: "echo", args: ...}]`.
- Turn 2: `MockLlmBackend` returns response with `text: "done"`, no tool_uses.
- Assert `RunOutcome::Completed`, `total_turns == 2`, `all_messages.len() == 4` (initial + tool_use msg + tool_result msg + final).

`MockLlmBackend` from `tau_ports::fixtures` supports per-call canned responses (verified in sub-project 2). Configure two responses; backend hands them out in order.

#### Task 13: Integration test `run_capability_denied`

Agent's `PackageManifest.capabilities()` returns `&[]` (empty). MockTool overrides `capabilities()` to return `&[Capability::Filesystem(FsCapability::Read{paths: vec!["/etc/passwd".into()]})]`. LLM emits a tool_use targeting this tool. Runtime checks: agent has no capabilities → first required capability missing → return `Ok(RunOutcome::Failed{status: AgentStatus::Failed{kind: PolicyDenied, detail: Some(...)}})`.

Assert:
- `let RunOutcome::Failed { status, .. } = outcome else { panic!() };`
- `matches!(status, AgentStatus::Failed { kind: FailureKind::PolicyDenied, .. })`.
- `detail` (the Option<String>) contains the tool name and the missing capability description.

NOTE: This test requires a custom `Tool` impl that overrides `capabilities()`. Since `MockTool` in tau-ports' fixtures has the default `&[]` (after Task 2), this test defines its own minimal struct `RestrictedMockTool` that wraps `MockTool` and overrides `capabilities()`. Implement inline in the test file.

#### Task 14: Integration test `run_max_turns`

`MockLlmBackend` configured to ALWAYS return a tool_use response (never terminates the loop on its own). `MockTool` returns a benign result that doesn't add anything informative. Set `RunOptions { max_turns: 3, .. }`.

Assert:
- `outcome` is `RunOutcome::Failed { status: AgentStatus::Failed{kind: FailureKind::OutOfResources, .. }, total_turns: 3, .. }`.
- `all_messages.len() >= 7` (initial + 3 × (tool_use + tool_result)).

#### Task 15: Integration tests `run_kernel_errors.rs`

Three `#[tokio::test]`s in one file:

- `llm_backend_not_registered`: Build a Runtime where the `agent_def.llm_backend` references a backend NOT in the registry. Run. Assert `Err(RuntimeError::LlmBackendNotRegistered { agent_id, backend })`.
- `tool_not_registered`: MockLlmBackend emits a tool_use with `tool_name: "nonexistent"`. Run. Assert `Err(RuntimeError::ToolNotRegistered { tool_name, registered })` and `registered` is the actual list of tool names.
- `plugin_contract_violation`: MockLlmBackend configured to emit a tool_use with malformed args (e.g. via a custom `tau_domain::Value` that doesn't deserialize to the tool's expected schema, OR by emitting a tool_use whose JSON args fail the schema's basic validation in the runtime). Assert `Err(RuntimeError::PluginContractViolation { plugin_kind: "llm", .. })`.

The third case requires the runtime to do SOME validation on tool_use args (per spec §3.9 `tool.args_schema_validated` event at TRACE level). v0.1 minimum: at least catch UTF-8 / JSON-parsing failures from the LLM's emitted args. If the runtime doesn't implement schema validation at v0.1, this test may need to be skipped with a `#[ignore]` and a comment explaining the deferral. Surface to controller during execution.

#### Task 16: Integration test `tracing_emission`

Use `tracing-subscriber` with a custom `Layer` that captures emitted events into a `Vec`. Run a happy-path agent. Assert that the captured events include (at minimum):
- One `runtime.agent_run` span.
- One `runtime.run_started` event.
- One or more `runtime.turn_started` / `turn_completed` events.
- One `llm.complete` span.
- One `runtime.run_completed` event.

Don't assert on EVERY event from the §3.9 vocabulary (would be brittle). Sample the structural ones; if the kernel emits fewer than ~8 events on a happy-path run, the vocabulary is broken.

The custom Layer pattern:

```rust
use std::sync::{Arc, Mutex};
use tracing::span::Attributes;
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

#[derive(Default, Clone)]
struct CapturedEvents(Arc<Mutex<Vec<String>>>);

impl<S: Subscriber> Layer<S> for CapturedEvents {
    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        self.0.lock().unwrap().push(format!("span: {}", attrs.metadata().name()));
    }
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        self.0.lock().unwrap().push(format!("event: {}", event.metadata().name()));
    }
}
```

Set up in test: `let events = CapturedEvents::default(); tracing_subscriber::registry().with(events.clone()).init();`. Run agent. Inspect `events.0.lock().unwrap()`.

#### Task 17: Proptest `proptest_capability_satisfies`

Spec §4. Strategies for `Capability` variants (Filesystem/Network/Process/Agent/Custom) with realistic field shapes. Three primary properties:

1. **Reflexivity**: any capability satisfies itself.
2. **Wrong-variant rejection**: granted variant ≠ required variant → satisfies = false.
3. **Glob superset**: a glob grant (`*`/`**`) satisfies any specific path that matches the pattern.

Use `proptest! { fn ... }` with each property as a separate `#[test]`. Generated values must be realistic (e.g., file paths that look like POSIX paths, hosts that look like domain names).

#### Task 18: CI `no-default-features-runtime` job

Spec §5. Add to `.github/workflows/ci.yml` after the existing `no-default-features-pkg` job:

```yaml
no-default-features-runtime:
  name: build (tau-runtime no-default-features)
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: Install Rust toolchain
      uses: dtolnay/rust-toolchain@stable
    - name: Build tau-runtime (no default features)
      run: cargo build -p tau-runtime --no-default-features
    - name: Test tau-runtime (no default features)
      run: cargo test -p tau-runtime --no-default-features --lib
```

Branch protection update on `main` (adding `build (tau-runtime no-default-features)` to required-status-checks) is deferred to Task 22's sign-off — GitHub requires the check to have run on main at least once before it can be added to the required list.

#### Task 19: ADR-0006

Spec §6. File at `docs/decisions/0006-tau-runtime.md`, format mirroring ADR-0001 / 0002 / 0003 / 0004 / 0005. Status starts as **Proposed**; flips to **Accepted** at Task 21 after the QG22 self-review or 24h wait. Update `docs/decisions/README.md` to add an ADR-0006 row.

The ADR records the 17 decisions enumerated in spec §6. Each section: rationale (cite guideline), alternatives considered (briefly), trigger to revisit. Pay special attention to:
- Section 7 (typed capability enforcement) — explicitly justifies the additive `Tool::capabilities()` amendment to ADR-0003.
- Section 9 (Outcome / Error dichotomy) — the architectural insight from the brainstorm.
- Section 14 (CVE-2022-39253-style analysis applies here? no — that was sub-project 3. ADR-0006 doesn't have its own security trade-off, but does document why Sandbox is skipped at v0.1 and the future enforcement path).

#### Task 20: Final local verification + mark PR #5 ready

Mirrors prior plans' final-verification tasks. No commit; just runs:

```bash
# Structure check
ls crates/tau-runtime/src/{lib,error,builder,options,outcome,capability,dispatch,run}.rs
ls crates/tau-runtime/tests/{run_completed,run_with_tool_calls,run_capability_denied,run_max_turns,run_kernel_errors,tracing_emission,proptest_capability_satisfies}.rs

# Full local CI equivalent
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build -p tau-runtime --no-default-features
cargo build -p tau-runtime --all-features
cargo test  -p tau-runtime --all-targets --all-features
cargo test  -p tau-runtime --doc          --all-features

# tau-ports must still pass after the Tool::capabilities() amendment
cargo test  -p tau-ports --all-targets --all-features
cargo test  -p tau-ports --doc          --all-features

# Cross-crate registry test (verifies escape-hatches.md is in sync)
cargo test -p tau-domain --test escape_hatch_registry --all-features

# Verify clean working tree
git status

# Mark PR #5 ready (out of draft)
gh pr ready 5

# Watch CI
gh pr checks 5 --watch
```

Confirm green status across all required checks.

#### Task 21: ADR-0006 sign-off (24h wait per QG22, OR self-review checklist)

Two paths, both legitimate per QG22:

- **24h wait**: re-read `docs/decisions/0006-tau-runtime.md` the next day with fresh eyes. Skim PR diff. Verify ADR-0003 cross-reference for the `Tool::capabilities()` amendment still feels right.
- **Self-review checklist** (alternative for small format/scope changes; ADR-0006 is broader so the 24h wait is the safer call). The checklist questions:
  - Does each decision still feel right?
  - Are alternatives genuine, not strawmen?
  - Are consequences honest (positive AND negative)?
  - Are new obligations realistic?
  - Code/ADR contradictions? (If found, fix inline before sign-off — see prior sub-projects.)

If the 24h wait, this task waits an actual 24h. If self-review, do it inline and proceed to Task 22.

No commit in this task. Status flip from Proposed → Accepted happens in Task 22's sign-off commit.

#### Task 22: Plan 5 sign-off + ROADMAP + plan tick-off + branch-protection update + merge

After Task 21 elapses and all gates are green:

1. **Tick all `- [ ]` boxes in this plan to `- [x]`.** Use Edit with `replace_all = true` on the literal `- [ ]` string in this file.
2. **Update `ROADMAP.md`** to mark sub-project 4 complete (✅ + completion date 2026-04-XX).
3. **Flip ADR-0006's status** from "Proposed" to "Accepted" in `docs/decisions/0006-tau-runtime.md` and `docs/decisions/README.md`.
4. **Stage + commit** with the prescribed message (see below).
5. **Push** to `feat/tau-runtime-spec`.
6. **Verify** CI green: `gh pr checks 5`.
7. **Merge** PR #5: `gh pr merge 5 --rebase --delete-branch`.
8. **Update branch protection on `main`**: add `build (tau-runtime no-default-features)` to required-status-checks via `gh api repos/LEBOCQTitouan/tau/branches/main/protection -X PUT ...`. Use the established pattern from sub-project 3's Task 20 (memory entry: feedback_branch_protection_workflow). The current required-status-checks list has 10 entries; the new total is 11.
9. **Verify** post-merge: `git checkout main && git pull && git log --oneline origin/main | head -25 && gh run list --workflow ci.yml --branch main --limit 3`.

Commit message:

```bash
git commit -m "docs: sign off Plan 5 (tau-runtime)

Plan 5 (sub-project 4, tau-runtime) is complete: all 22 tasks ticked,
CI green on PR #5, ADR-0006 accepted, QG22 review elapsed (24h wait
or self-review per the maintainer's choice).

Adds the tau-runtime kernel: builder pattern with build-time
validation, multi-turn batch agent loop, typed capability enforcement
via the additive Tool::capabilities() amendment to ADR-0003, ~45-event
tracing vocabulary across 9 subsystems, Outcome/Error dichotomy.

ROADMAP marks sub-project 4 complete.
Plan 5 checkboxes (Task 1-3 step-level) all ticked.
ADR-0006 status: Proposed -> Accepted.

Branch protection on main updated: 'build (tau-runtime no-default-features)'
added to required-status-checks. Total: 11 required checks.

Next: sub-project 5 (tau-cli) — real subcommands (tau install, tau run,
tau ls). Wires concrete LlmBackend / Tool plugins into the runtime;
exposes streaming UX for terminal rendering.

Refs: PG4 (phase boundary), QG22, ADR-0003 (amended), ADR-0006"
```

Sub-project 4 is closed. Begin sub-project 5's brainstorm cycle when ready.

---

## Risks & rollbacks

| Risk | Mitigation |
|---|---|
| Plan delegates to spec for Tasks 4-22; if executor doesn't follow patterns from prior plans closely, drift accumulates | Each task references the spec section explicitly; the table prescribes the commit message; the executor surfaces deviations to the controller. |
| `Tool::capabilities()` amendment (Task 2) breaks downstream impls in tau-ports' fixtures | Default `&[]` keeps existing impls compiling. CI verifies via `cargo test -p tau-ports --all-targets --all-features` in Task 2. |
| Capability satisfies-relation has glob-matching bugs | Strict TDD in Task 8 with ≥10 unit tests; proptest in Task 17 covers generative pairs. ADR-0006 documents simplifications (`**`/`*`/exact only). |
| Tracing event vocabulary drifts between docs and code | Task 16 (`tracing_emission`) asserts a known-good event set fires on a happy-path run. Drift triggers test failure. ADR-0006 freezes the vocabulary; additive changes are non-breaking. |
| Async public API forces tokio dep into every downstream | tokio is dev-dep ONLY in tau-runtime. Downstream callers (tau-cli) bring tokio at the binary level. The library is async-runtime-agnostic in code (uses `async fn` + `.await`, no tokio primitives). |
| `Box<dyn Tool<Session = ()>>` registration excludes stateful tools at v0.1 | Documented limitation; `StatelessAdapter` wraps stateless tools; ADR-0006 records the additive `DynTool` extension when stateful tools land (sub-project 5+). |
| Branch protection on `main` blocks merge if CI registry test fails after Tasks 3 or 4 | Run the test locally (`cargo test -p tau-domain --test escape_hatch_registry --all-features`) before pushing each commit. Same-commit-as-introduction policy applied per the carry-over from sub-project 3. |
| Integration tests need real `Capability` / `Address` / `Message` constructors from tau-domain | tau-domain's public API is stable; constructors are documented. The executor reads tau-domain's lib.rs to find them. If a needed constructor doesn't exist, surface to the controller before adding one (would be a tau-domain change). |
| `MockLlmBackend` per-call response configuration may be limited (sub-project 2's mocks may not support arbitrary multi-turn sequences) | Verify by reading `tau_ports::fixtures` source. If `MockLlmBackend::with_response(req_pattern, resp)` doesn't support sequential responses, surface to controller. The integration tests may need to extend the mock OR define a local test-specific subclass. |
| `feat/tau-runtime-spec` branch becomes stale relative to `main` while sub-project 4 is in flight | Periodic `git fetch origin && git rebase origin/main` if other PRs land on `main`. tau-runtime work shouldn't need cross-cuts beyond its own crate + the workspace Cargo.toml + escape-hatches.md + decisions/ + the tau-ports `Tool::capabilities()` amendment. |
| Plugin contract violation test (Task 15) can't be implemented if v0.1 doesn't validate tool_use args at all | If schema validation isn't part of v0.1, mark the test `#[ignore]` with a comment explaining the deferral. Surface to controller during execution; ADR-0006 may need an addendum noting the v0.1 simplification. |

Rollback strategy: any single sub-task commit is independently revertable. The plan ordering (deps → tau-ports amendment → leaf errors → composing errors → data shapes → builder → satisfies-relation → dispatch → run loop → integration tests → CI → docs → ADR) is dependency-bottom-up.

---

## Handoff to executor

Execute via `superpowers:subagent-driven-development` on the existing `feat/tau-runtime-spec` branch (PR #5). Tasks 1–3 are detailed in this plan; Tasks 4–22 reference the spec sections and the established patterns from Plans 2 + 3 + 4. Tasks 20–22 are user-driven gates. The executor surfaces any plan ambiguity to the controller for clarification before committing.

After Plan 5 sign-off (Task 22), the next sub-project is `tau-cli` (sub-project 5) — real subcommands (`tau install`, `tau run`, `tau ls`). Wires concrete plugin instances into the runtime; exposes streaming UX for terminal rendering.
