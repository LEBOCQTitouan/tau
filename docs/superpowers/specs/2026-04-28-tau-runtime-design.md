# Tau Runtime (sub-project 4) — Design Spec

**Date:** 2026-04-28
**Sub-project:** `tau-runtime` agent lifecycle + message passing (sub-project 4; fourth of the Phase-0 sub-projects, after `tau-domain`, `tau-ports`, and `tau-pkg`)
**Author:** Titouan Lebocq
**Status:** Approved for implementation planning

---

## 1. Scope & success criteria

### Scope

Implement the kernel that loads plugins, runs an agent, dispatches messages to LLM backends and tools, enforces capability declarations at runtime, and emits structured logs. tau-runtime is the **embeddable Rust API surface** for tau (G6, QG12) — sub-project 5 (tau-cli) is a thin shim over it.

Per ROADMAP row 4: "Spawn an agent, deliver messages, observe via structured logs (solo path only)." This is the **solo path** only — orchestration of multiple agents (G10) is sub-project 5+.

The kernel does not load plugins from disk at v0.1. tau-pkg installs packages; tau-runtime accepts pre-constructed plugin instances via a builder API and dispatches against them. Sub-project 5 (tau-cli) wires concrete LLM/Tool/Storage plugins into the runtime; sub-project 4 ships a kernel testable in isolation against tau-ports' mock plugins.

### Done when

- `crates/tau-runtime/` exposes the public API enumerated in §3.10.
- `cargo build -p tau-runtime --no-default-features` succeeds locally and in CI.
- `cargo build -p tau-runtime --all-features` succeeds locally and in CI.
- `cargo clippy -p tau-runtime --all-targets --all-features -- -D warnings` succeeds.
- `cargo fmt --all -- --check` succeeds.
- `cargo test -p tau-runtime --all-targets --all-features` succeeds (unit + integration).
- `cargo test -p tau-runtime --doc --all-features` succeeds — every public item has an example.
- `cargo test -p tau-ports --all-targets --all-features` succeeds (Tool trait gains an additive method; pre-existing impls and the four mocks must still compile and pass).
- ADR-0006 (tau-runtime kernel + Tool capabilities additive amendment) is filed in `docs/decisions/` and accepted.
- Full integration test exercises: register mock LLM + mock Tool + mock Storage → run agent → multi-turn loop → tool dispatch → capability check → structured logs captured → assert on outcome.
- The git log on `main` contains a clean per-sub-task series of Conventional Commits.
- CI green on Linux + macOS (Windows non-blocking per G15).
- New escape-hatch entries (`builderror-internal`, `runtimeerror-internal`) registered in `docs/explanation/escape-hatches.md`.
- ROADMAP marks sub-project 4 complete.

### Out of scope (explicit, deferred to later sub-projects or ADRs)

| Item | Owner / Trigger |
|---|---|
| Streaming agent loop (`Runtime::run_streaming`) | Sub-project 5+ when tau-cli renders tokens to a terminal. Additive minor; existing `run` API stays. |
| Stateful tools (`Tool::Session ≠ ()`) | Sub-project 5+ when a real stateful tool plugin lands. Additive `DynTool` / `DynToolSession` extension to tau-ports per ADR-0006. |
| Dynamic plugin loading from disk (cdylib / dlopen) | Phase-1+ when shipping pre-built tau binaries to non-developers. ADR-0006 documents the deferral rationale. Sub-project 4: callers register pre-constructed `Box<dyn LlmBackend>`. |
| Sandbox enforcement | Phase-1+. Trait stays in tau-ports as a forward-compat anchor (ADR-0003 provisional caveat); `Runtime` doesn't wire `Sandbox::create` at v0.1. |
| Soft-fail tool errors (`RunOptions { soft_fail_tool_errors: true }`) | Phase-1+. v0.1 plugin errors terminate the run via `Err(RuntimeError::*)`. |
| LLM retry policy (`RunOptions { llm_retry_policy }`) | Phase-1+. v0.1 doesn't retry; caller wraps with `tokio::time::timeout` if desired. |
| Overall run timeout (`RunOptions { overall_timeout: Duration }`) | Phase-1+. v0.1 caller wraps with `tokio::time::timeout(...)`. |
| Concurrent / parallel tool calls | Phase-1+. v0.1 sequential agent loop. |
| Multi-agent orchestration | Sub-project 5+ (G10). Sub-project 4 is solo path only. |
| Persistent agent memory | NG6 forever. Storage plugins are caller-owned; the kernel doesn't auto-persist anything. |
| `Runtime::shutdown(self)` consuming method | Sub-project 5+ if a real consumer needs explicit drop ordering. v0.1 relies on `drop`. |
| Allocator-level memory tracking (G16 50MB budget) | Phase-1+ perf focus. v0.1 trusts the implementation's modest footprint. |

---

## 2. Module layout & dependencies

```
crates/tau-runtime/
├── Cargo.toml
└── src/
    ├── lib.rs           # crate-level docs, lints, re-exports
    ├── error.rs         # BuildError, CapabilityDenial, RuntimeError
    ├── builder.rs       # Runtime, RuntimeBuilder, PluginKind, plugin registries
    ├── options.rs       # RunOptions, TokenUsage
    ├── outcome.rs       # RunOutcome
    ├── capability.rs    # satisfies-relation + helpers (typed cap matching)
    ├── dispatch.rs      # Address routing, tool/llm resolution by name
    └── run.rs           # agent multi-turn loop + tracing instrumentation
```

### Cargo.toml additions

`[workspace.dependencies]` gains:
- `tracing = "0.1"` (used by tau-runtime's structured-log vocabulary).

`crates/tau-runtime/Cargo.toml`:

```toml
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

**Key calls:**

- **`tokio` is a dev-dep, NOT a runtime dep.** tau-runtime's library code uses native `async fn` and `.await` only — no tokio-specific primitives (`tokio::sync`, `tokio::spawn`, `tokio::select!`). Callers (tau-cli) bring tokio at the binary level. This matches Q2's intent ("async public API"); the kernel is agnostic about WHICH async runtime drives it. Tests use `#[tokio::test]` so tokio is dev-only.
- **`tau-ports` enabled with `test-fixtures` feature in dev-deps** so integration tests can use `MockLlmBackend`, `MockTool`, `MockStorage`.
- **`tracing-subscriber` is dev-only.** Production callers compose their own subscriber.
- **No `tau-pkg` dep.** Caller (tau-cli) reads the manifest via `tau_pkg::read_manifest` and passes it to `Runtime::run` (Q5a=A). Kernel stays decoupled from tau-pkg's I/O.
- **`proptest` is dev-only** — covers the typed capability satisfies-relation (the only logic in tau-runtime that warrants generative testing).

### tau-ports additive change

This sub-project includes a tightly-motivated additive amendment to tau-ports' `Tool` trait: a new method `fn capabilities(&self) -> &[Capability] { &[] }` with a default. Backwards-compatible (existing impls don't need to change). Lands as commit 1 of the tau-runtime PR; ADR-0006 covers both the kernel and the trait amendment. See §3.6 for usage and §6 for ADR rationale.

---

## 3. Type-by-type design

### 3.1 Errors (`error.rs`)

Per the dichotomy established at Q9: `Err(RuntimeError)` for kernel-level operational failures (the kernel itself can't continue); `Ok(RunOutcome::Failed)` for agent-level failures (agent ran but couldn't complete its task within the rules).

```rust
use thiserror::Error;

/// Tag identifying a plugin kind in error messages and tracing fields.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginKind {
    LlmBackend,
    Tool,
    Storage,
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

/// Errors from `RuntimeBuilder::build()`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BuildError {
    #[error("no LLM backends registered; at least one is required")]
    NoLlmBackend,
    #[error("name collision: two {kind}s registered as {name:?}")]
    NameCollision { kind: PluginKind, name: String },
    /// Catch-all.
    /// See: [escape-hatches.md#builderror-internal](../docs/explanation/escape-hatches.md#builderror-internal).
    #[error("internal: {message}")]
    Internal { message: String },
}

/// Helper type carrying capability-denial detail. Embedded as the
/// `detail` string of `AgentStatus::Failed { kind: PolicyDenied, .. }`
/// when capability denial occurs. NOT a variant of `RuntimeError`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityDenial {
    pub agent_id: String,
    pub package_id: String,
    pub tool_name: String,
    pub required_kind: String,
    pub required_detail: String,
}

impl std::fmt::Display for CapabilityDenial {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "agent {} (package {}) lacks capability `{}` ({}) required to call tool `{}`",
            self.agent_id, self.package_id,
            self.required_kind, self.required_detail,
            self.tool_name,
        )
    }
}

/// Errors from `Runtime::run` — kernel-level operational failures.
/// Agent-level failures (capability denied, max turns) flow through
/// `Ok(RunOutcome::Failed { status: AgentStatus::Failed{..} })` instead.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RuntimeError {
    /// Agent's `llm_backend` references a backend that wasn't registered.
    #[error("LLM backend `{backend}` not registered (agent {agent_id} requested it)")]
    LlmBackendNotRegistered { agent_id: String, backend: String },

    /// LLM emitted a tool_use targeting a tool not in the registry.
    #[error("tool `{tool_name}` not registered; registered: {registered:?}")]
    ToolNotRegistered { tool_name: String, registered: Vec<String> },

    /// Plugin returned successfully but its output violates the contract
    /// (malformed JSON args from LLM, undeserializable content, etc.).
    #[error("plugin contract violation: {plugin_kind} `{plugin_name}` returned malformed {what}: {detail}")]
    PluginContractViolation {
        plugin_kind: String,
        plugin_name: String,
        what: String,
        detail: String,
    },

    #[error("llm: {0}")]
    Llm(#[from] tau_ports::LlmError),

    #[error("tool: {0}")]
    Tool(#[from] tau_ports::ToolError),

    #[error("storage: {0}")]
    Storage(#[from] tau_ports::StorageError),

    /// Reserved for forward compat (Q7=A skips Sandbox at v0.1).
    #[error("sandbox: {0}")]
    Sandbox(#[from] tau_ports::SandboxError),

    #[error("manifest validation: {0}")]
    Manifest(#[from] tau_domain::PackageManifestError),

    /// Catch-all for invariant violations / unexpected states.
    /// See: [escape-hatches.md#runtimeerror-internal](../docs/explanation/escape-hatches.md#runtimeerror-internal).
    #[error("internal: {message}")]
    Internal { message: String },
}
```

Two new escape-hatch entries register: `builderror-internal`, `runtimeerror-internal`. Plugin errors keep their existing tau-ports registrations from sub-project 2.

### 3.2 RunOptions (`options.rs`)

```rust
use std::time::Duration;

/// Token usage reported by the LLM backend, summed across the run.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Some backends report; some don't.
    pub total_tokens: Option<u64>,
}

/// Options for `Runtime::run`.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Hard cap on agent loop iterations. Hitting this returns
    /// `Ok(RunOutcome::Failed { kind: OutOfResources, .. })`.
    /// Default: 16.
    pub max_turns: u32,

    /// Optional caller-supplied label included in tracing spans for
    /// log correlation (e.g. session UUID from a TUI).
    pub trace_label: Option<String>,

    // Future-extension space; #[non_exhaustive] enables additive options:
    //   pub soft_fail_tool_errors: bool      (Phase-1+)
    //   pub llm_retry_policy: RetryPolicy    (Phase-1+)
    //   pub overall_timeout: Option<Duration> (Phase-1+)
}

impl Default for RunOptions {
    fn default() -> Self {
        Self { max_turns: 16, trace_label: None }
    }
}
```

The `Duration` import is reserved for the Phase-1 `overall_timeout` option but unused at v0.1. Keep the import gated behind `#[allow(unused_imports)]` or land it later — implementation choice.

### 3.3 RunOutcome (`outcome.rs`)

```rust
use tau_domain::{AgentStatus, Message};

use crate::options::TokenUsage;

/// Outcome of a `Runtime::run` call. Distinguishes successful completion
/// from agent-level failures (which are NOT errors — see `RuntimeError`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum RunOutcome {
    /// Agent completed and produced a final response.
    Completed {
        final_message: Message,
        all_messages: Vec<Message>,
        total_turns: u32,
        token_usage: TokenUsage,
    },
    /// Agent ran but failed via a typed `FailureKind`. Partial conversation
    /// preserved for inspection.
    Failed {
        /// Always `AgentStatus::Failed { kind, detail }`.
        status: AgentStatus,
        all_messages: Vec<Message>,
        total_turns: u32,
        token_usage: TokenUsage,
    },
}
```

`all_messages` includes the initial message + every LLM response + every tool_use / tool_result. Long conversations carry memory cost; future option `RunOptions { include_full_history: bool }` can let callers opt out at the cost of losing inspectable history.

### 3.4 Builder + Runtime (`builder.rs`)

```rust
use std::collections::HashMap;
use std::sync::Arc;

use tau_ports::{LlmBackend, Sandbox, Storage, Tool};

use crate::error::{BuildError, PluginKind};

/// The kernel. Build with `Runtime::builder()`.
///
/// Plugin registries are immutable post-`build()`. To add or remove
/// plugins, construct a new Runtime.
pub struct Runtime {
    llm_backends: HashMap<String, Arc<dyn LlmBackend>>,
    tools: HashMap<String, Arc<dyn Tool<Session = ()>>>,
    storages: HashMap<String, Arc<dyn Storage>>,
    // sandboxes reserved for forward compat (Q7=A — not used at v0.1)
    // Per-run options live on RunOptions, not on the Runtime itself.
}

impl Runtime {
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::default()
    }

    // run / run_default — see §3.7
}

/// Builder for `Runtime`. Plugin instances accumulate; `build()`
/// validates invariants and finalizes the registries.
#[derive(Default)]
pub struct RuntimeBuilder {
    llm_backends: Vec<Arc<dyn LlmBackend>>,
    tools: Vec<Arc<dyn Tool<Session = ()>>>,
    storages: Vec<Arc<dyn Storage>>,
    // sandboxes (reserved for forward compat)
}

impl RuntimeBuilder {
    pub fn with_llm_backend(mut self, backend: Box<dyn LlmBackend>) -> Self {
        self.llm_backends.push(backend.into());
        self
    }

    pub fn with_tool(mut self, tool: Box<dyn Tool<Session = ()>>) -> Self {
        self.tools.push(tool.into());
        self
    }

    pub fn with_storage(mut self, storage: Box<dyn Storage>) -> Self {
        self.storages.push(storage.into());
        self
    }

    /// Validate registrations and produce a `Runtime`.
    ///
    /// Validation:
    /// - At least one LLM backend must be registered.
    /// - No name collisions within a kind.
    pub fn build(self) -> Result<Runtime, BuildError> {
        if self.llm_backends.is_empty() {
            return Err(BuildError::NoLlmBackend);
        }
        let llm_backends = collect_by_name(self.llm_backends, PluginKind::LlmBackend, |p| p.name())?;
        let tools = collect_by_name(self.tools, PluginKind::Tool, |p| p.name())?;
        let storages = collect_by_name(self.storages, PluginKind::Storage, |p| p.name())?;
        Ok(Runtime { llm_backends, tools, storages })
    }
}
```

`collect_by_name` is a private helper that builds a `HashMap` from a `Vec` and returns `BuildError::NameCollision` on duplicates. `Box<dyn T> -> Arc<dyn T>` conversion via `Box::into` is straightforward.

The `Box<dyn Tool<Session = ()>>` type signature locks in the v0.1 limitation: stateful tools require `tau_ports::StatelessAdapter` wrapping. ADR-0006 documents the additive `DynTool` extension when stateful tools land.

### 3.5 Capability satisfies-relation (`capability.rs`)

The agent's package declares typed capabilities (e.g., `Filesystem::Read { paths: ["/tmp/**"] }`); a tool declares typed capabilities it requires (e.g., `Filesystem::Read { paths: ["/tmp/foo.txt"] }`). The runtime checks: for every `required` capability, at least one `granted` capability must satisfy it.

```rust
use tau_domain::{Capability, FsCapability, NetCapability, ProcessCapability, AgentCapability};

/// Returns `true` if `granted` is a superset/match of `required`.
///
/// The relation is variant-by-variant: a `Filesystem::Read` grant only
/// satisfies a `Filesystem::Read` requirement (never a `Write` or `Network`).
/// Path / host / process patterns are matched via glob (granted patterns
/// must cover the required pattern).
pub(crate) fn capability_satisfies(granted: &Capability, required: &Capability) -> bool {
    use Capability::*;
    match (granted, required) {
        (Filesystem(g), Filesystem(r)) => fs_satisfies(g, r),
        (Network(g), Network(r))       => net_satisfies(g, r),
        (Process(g), Process(r))       => process_satisfies(g, r),
        (Agent(g), Agent(r))           => agent_satisfies(g, r),
        (Custom { name: gn, params: gp }, Custom { name: rn, params: rp }) => {
            gn == rn && custom_params_satisfy(gp, rp)
        }
        _ => false,
    }
}

/// Top-level check: every required cap is satisfied by at least one grant.
pub(crate) fn check_capabilities(
    granted: &[Capability],
    required: &[Capability],
) -> Option<&Capability> /* missing */ {
    for req in required {
        if !granted.iter().any(|g| capability_satisfies(g, req)) {
            return Some(req);
        }
    }
    None
}
```

Per-variant satisfies functions:

- `fs_satisfies`: `Read{paths}` granted satisfies `Read{paths_required}` if every required path matches at least one grant pattern (glob via `globset` or simple `**`/`*` substring matching at v0.1).
- `net_satisfies`: `Http{hosts, methods}` granted satisfies `Http{hosts_req, methods_req}` if hosts and methods are subsets.
- `process_satisfies`: similar pattern over executable paths and arg patterns.
- `agent_satisfies`: `Spawn{kinds, packages}` granted satisfies the requested kinds/packages by subset.
- `custom_params_satisfy`: `Custom`'s `params: BTreeMap<String, Value>` — every required key must exist in the grant with an equal value. Conservative for v0.1 (exact-match); ADR-0006 leaves room for richer matching later.

Glob matching at v0.1: the simplest correct implementation. For paths, accept `**` (any depth), `*` (single segment), exact match. Add `globset = "0.4"` to deps if needed. Decision deferred to writing-plans phase — could land as an inline matcher to keep deps minimal.

### 3.6 Tool capabilities (additive tau-ports change)

```rust
// crates/tau-ports/src/tool.rs (additive amendment)

pub trait Tool: Send + Sync {
    type Session: Send + 'static;

    fn name(&self) -> &str;
    fn schema(&self) -> ToolSpec;

    /// Capabilities this tool requires the calling agent's package to declare.
    /// Default: empty (tool is unrestricted; any agent can call it).
    ///
    /// The runtime checks: for every capability in this list, the agent's
    /// package manifest must contain at least one capability that satisfies
    /// it (see `tau_runtime::capability::check_capabilities`).
    fn capabilities(&self) -> &[tau_domain::Capability] { &[] }

    async fn init(&self, ctx: SessionContext) -> Result<Self::Session, ToolError>;
    async fn invoke(&self, session: &mut Self::Session, args: Value) -> Result<ToolResult, ToolError>;
    async fn teardown(&self, session: Self::Session) -> Result<(), ToolError>;
}
```

The default `&[]` keeps existing impls (the four mocks in `tau-ports::fixtures`) compiling without changes. `MockTool` continues to declare no capabilities — any agent can call it in tests.

This is the only tau-ports modification in sub-project 4. Lands as commit 1 of the tau-runtime PR; ADR-0006 records the rationale.

### 3.7 Run signature + agent loop (`run.rs`)

```rust
use tau_domain::{AgentDefinition, Message, PackageManifest};
use tracing::{instrument, Span};

use crate::{
    options::{RunOptions, TokenUsage},
    outcome::RunOutcome,
    error::RuntimeError,
};

impl Runtime {
    /// Run an agent through one solo-path iteration: receive the initial
    /// message, dispatch to LLM and tools per the multi-turn loop, return
    /// the outcome.
    ///
    /// `package_manifest` must be the manifest of `agent_def.package`.
    /// Caller is responsible for fetching it (typically via
    /// `tau_pkg::read_manifest`); the kernel uses its `capabilities()`
    /// for runtime enforcement.
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
        // 1. Load capabilities from the package manifest.
        // 2. Resolve the LLM backend by name (agent_def.llm_backend).
        // 3. Initialize the messages history with the initial message.
        // 4. Multi-turn loop:
        //    - Build CompletionRequest from messages + registered tools' schemas.
        //    - Call backend.complete(req) → CompletionResponse.
        //    - For each tool_use in the response:
        //       - Resolve the tool by name.
        //       - Check capabilities (granted vs required).
        //       - On denial: return Ok(RunOutcome::Failed{kind: PolicyDenied, ..}).
        //       - On allow: invoke the tool, append tool_use + tool_result to messages.
        //    - If response.tool_uses is empty: terminate, return Completed.
        //    - If turn count == options.max_turns: return Ok(RunOutcome::Failed{kind: OutOfResources, ..}).
        //    - Otherwise: increment turn, loop.
    }

    /// Convenience: `run` with `RunOptions::default()`.
    pub async fn run_default(
        &self,
        agent_def: AgentDefinition,
        package_manifest: PackageManifest,
        initial_message: Message,
    ) -> Result<RunOutcome, RuntimeError> {
        self.run(agent_def, package_manifest, initial_message, RunOptions::default()).await
    }
}
```

Implementation details land in writing-plans. Tracing instrumentation per the §3.9 vocabulary is wrapped around the relevant call sites.

### 3.8 Dispatch helpers (`dispatch.rs`)

```rust
use tau_domain::Address;
use tau_ports::{LlmBackend, Tool};

use crate::{Runtime, RuntimeError};

impl Runtime {
    /// Resolve an LLM backend by name. Returns `LlmBackendNotRegistered` if absent.
    pub(crate) fn resolve_llm_backend(
        &self,
        agent_id: &str,
        backend_name: &str,
    ) -> Result<&Arc<dyn LlmBackend>, RuntimeError>;

    /// Resolve a tool by name. Returns `ToolNotRegistered` if absent
    /// (with the registered names list for diagnostics).
    pub(crate) fn resolve_tool(
        &self,
        tool_name: &str,
    ) -> Result<&Arc<dyn Tool<Session = ()>>, RuntimeError>;

    /// Resolve a recipient `Address` to a tool name (returns `None`
    /// for non-Tool addresses; the loop only routes to tools at v0.1).
    pub(crate) fn address_to_tool_name(addr: &Address) -> Option<&str>;
}
```

Internal helpers; not part of the public API.

### 3.9 Tracing instrumentation vocabulary

Per Q6, ~45 events/spans across 9 subsystems. The full vocabulary is documented in ADR-0006 and implemented inline at the relevant call sites. Summary by subsystem:

| Subsystem | Spans | Events |
|---|---|---|
| `builder` | `builder.build` | `plugin_registered`, `validation_started/failed`, `build_succeeded` |
| `runtime` | `runtime.agent_run`, `runtime.turn` | `run_started`, `capability_set_loaded`, `turn_started/completed`, `max_turns_reached`, `loop_terminated`, `run_completed`, `run_failed`, `error_classified`, `invariant_violated` |
| `llm` | `llm.complete` | `request_built`, `request_sent`, `response_received`, `token_usage`, `stop_reason`, `tool_use_emitted`, `error` |
| `capability` | `capability.check` | `required_loaded`, `granted_loaded`, `satisfies_check`, `allow`, `deny` |
| `tool` | `tool.session_open`, `tool.invoke`, `tool.session_close` | `args_received`, `args_schema_validated`, `result_received`, `invoke_failed`, `session_open_failed`, `session_close_failed` |
| `dispatch` | `dispatch.tool` | `tool_resolved`, `tool_not_found` |
| `storage` | `storage.op` | `get`, `put`, `delete`, `list`, `op_failed` |
| `sandbox` | `sandbox.create` | `created`, `create_failed`, `handle_dropped` |
| `message` | (none) | `added`, `payload_preview` |

**Level discipline**:
- `TRACE`: per-iteration internals, capability satisfies-check per pair.
- `DEBUG`: per-operation detail (args, message previews, request IDs, internal state).
- `INFO`: significant state changes, lifecycle landmarks, run summary.
- `WARN`: recoverable issues, denials, non-terminal plugin errors.
- `ERROR`: hard failures (dispatch-not-found, validation, panics-caught-as-bugs).

**Sensitive-data discipline**: args + message contents never above `DEBUG`. Previews truncated to 256 chars at `DEBUG`; full content only at `TRACE`. API keys / credentials never logged by the kernel (NG9). Payload sizes always logged for capacity awareness.

### 3.10 Public API surface (re-exports in `lib.rs`)

```rust
// Errors
pub use error::{BuildError, CapabilityDenial, PluginKind, RuntimeError};

// Construction
pub use builder::{Runtime, RuntimeBuilder};

// Run inputs
pub use options::{RunOptions, TokenUsage};

// Run outputs
pub use outcome::{RunOutcome};
```

The trait surface (`tau_ports::*`) and data shapes (`tau_domain::*`) are NOT re-exported through tau-runtime; callers depend on those crates directly.

---

## 4. Parsers

Per QG5, parsers of external input earn proptest coverage. tau-runtime has no external-input parsers — every input it consumes (`AgentDefinition`, `PackageManifest`, `Message`, `CompletionResponse`, `ToolResult`) is already typed, validated, and proptested in tau-domain or tau-ports.

The one piece of internal logic worth proptesting is the **capability satisfies-relation** in `capability.rs`. Generative coverage validates the variant-by-variant matching, glob-pattern subset checks, and the top-level "all required must be satisfied" predicate.

| Parser / logic | What | Proptest coverage |
|---|---|---|
| `capability_satisfies` (per-variant) | Filesystem / Network / Process / Agent / Custom | Generated typed `Capability` pairs; assertion: identical pairs always satisfy; disjoint pairs never satisfy; subset-of-broader-grant always satisfies. |
| `check_capabilities` (top-level) | List of required vs. list of granted | Generated `Vec<Capability>` pairs; assertion: empty required → always satisfied; required containing one cap not in granted → returns `Some(missing)`. |

---

## 5. Testing strategy

Per QG5 + the Phase-0 testing layers established in tau-domain / tau-ports / tau-pkg.

### Layers

**Unit tests, inline in `src/*.rs`:**
- `error.rs` — display rendering, `#[from]` composition for each `RuntimeError` arrow.
- `builder.rs` — `Runtime::builder().build()` validates: no LLM backends → `BuildError::NoLlmBackend`; duplicate names within a kind → `BuildError::NameCollision`.
- `options.rs` — `RunOptions::default()` field values; `#[non_exhaustive]` enforced.
- `outcome.rs` — variant construction; `Completed` vs `Failed` discrimination.
- `capability.rs` — per-variant satisfies tests covering exact-match, subset, disjoint, mismatched-variant cases.
- `dispatch.rs` — resolution helpers return correct registry entries; missing entries produce `RuntimeError::*NotRegistered`.

**Integration tests, in `tests/`:**
- `tests/run_completed.rs` — register `MockLlmBackend` + `MockTool` (with stateless adapter) + `MockStorage`. `MockLlmBackend` configured to emit text response with no tool_uses; assert `Ok(RunOutcome::Completed { final_message, total_turns: 1 })`.
- `tests/run_with_tool_calls.rs` — `MockLlmBackend` configured to emit a tool_use on turn 1, then text on turn 2; assert `Completed { total_turns: 2, all_messages.len() ≥ 4 }` (initial + tool_use + tool_result + final).
- `tests/run_capability_denied.rs` — agent's manifest declares insufficient caps; tool requires more; assert `Ok(RunOutcome::Failed { status: AgentStatus::Failed { kind: FailureKind::PolicyDenied, .. } })`.
- `tests/run_max_turns.rs` — `MockLlmBackend` always emits a tool_use; assert `Ok(RunOutcome::Failed { kind: OutOfResources })` after `max_turns` iterations.
- `tests/run_llm_backend_not_registered.rs` — `agent_def.llm_backend` references an unregistered backend; assert `Err(RuntimeError::LlmBackendNotRegistered { .. })`.
- `tests/run_tool_not_registered.rs` — `MockLlmBackend` emits a tool_use for an unregistered tool; assert `Err(RuntimeError::ToolNotRegistered { .. })`.
- `tests/run_plugin_contract_violation.rs` — `MockLlmBackend` emits malformed tool_use args; assert `Err(RuntimeError::PluginContractViolation { .. })`.
- `tests/builder_validation.rs` — exhaustive build-validation cases (no LLM, name collision, etc.).
- `tests/tracing_emission.rs` — uses `tracing-subscriber` with a custom layer to capture events; asserts that running an agent emits the expected event set per the §3.9 vocabulary.

**Proptest:**
- `tests/proptest_capability_satisfies.rs` — generative cases over `Capability` pairs. Strategies cover all five variants with realistic field shapes.

**Doc tests:**
- Every public item has at least one example (per the established pattern from tau-domain / tau-ports / tau-pkg).
- Examples on `#[non_exhaustive]` types are `ignore`-marked (per the plan-erratum carry-over from prior sub-projects).
- Runnable examples on free functions where possible (e.g., the `check_capabilities` helper if exported as a `pub(crate)` test utility — likely not exported, just used).

**Test fixtures:**
- Reuse `tau_ports::fixtures` (`MockLlmBackend`, `MockTool`, `MockStorage`) under the `test-fixtures` feature. No new fixture types in tau-runtime.

### CI implications

One new CI job in `.github/workflows/ci.yml`:
- `no-default-features-runtime`: `cargo build -p tau-runtime --no-default-features` + `cargo test -p tau-runtime --no-default-features --lib`.

The existing matrix jobs (test on Linux + macOS + Windows × stable + 1.91) automatically cover tau-runtime with default features. Branch-protection on `main` gains one new required check (`build (tau-runtime no-default-features)`), updated via the established `gh api ... -X PUT ...` pattern at sub-project sign-off.

---

## 6. ADR-0006 — tau-runtime kernel + Tool capabilities amendment

ADR-0006 is filed at `docs/decisions/0006-tau-runtime.md` as part of this sub-project. Records the kernel decisions AND the additive `Tool::capabilities()` amendment to ADR-0003. Bundling both into one ADR is justified by their tight coupling: typed enforcement at runtime requires the trait-level declaration, and the amendment exists solely because tau-runtime needs it.

ADR-0006 records:

1. **Pure kernel skeleton scope** — sub-project 4 ships a kernel testable against tau-ports' mock plugins; real plugins land in sub-project 5+.
2. **Async public API** — `Runtime::run` is async; callers pick the async runtime. tokio is a dev-dep in tau-runtime, not a runtime dep.
3. **Builder pattern construction** — `Runtime::builder().with_*(Box<dyn Plugin>).build()`; build validates "≥1 LLM backend" and "no name collisions per kind".
4. **`Session = ()` v0.1 tool limitation** — stateful tools require `tau_ports::StatelessAdapter` wrapping. ADR-0006 documents the future `DynTool`/`DynToolSession` extension for when stateful tools land.
5. **Multi-turn batch loop** — uses `LlmBackend::complete` (not `stream`); streaming added later as additive `run_streaming`.
6. **Caller-supplied manifest** — `Runtime::run` takes `PackageManifest`; tau-runtime has no tau-pkg dep.
7. **Typed capability enforcement (Q5b=A)** — additive `Tool::capabilities() -> &[Capability]` on the tau-ports `Tool` trait (default `&[]`); tau-runtime implements the satisfies-relation; mismatch produces `RunOutcome::Failed { PolicyDenied }`.
8. **Hard-fail on denial** — capability denial terminates the run with `FailureKind::PolicyDenied`. Soft-fail deferred to Phase-1+ via `RunOptions { soft_fail_*: true }`.
9. **Outcome / Error dichotomy** — `Ok(RunOutcome::Failed)` for agent-level failures (policy denial, max turns); `Err(RuntimeError)` for kernel-level errors (plugin errors, dispatch errors, contract violations).
10. **No retries at v0.1** — plugin errors terminate the run. Caller wraps with `tokio::time::timeout` and external retry logic if needed.
11. **`tracing` for structured logs** — caller composes the subscriber (NG9 — tau doesn't manage credentials and doesn't redact for the caller).
12. **Per-runtime storage scoping** — single `Box<dyn Storage>` instance shared across runs; agent-instance-scoped namespaces isolate data.
13. **Sandbox skipped at v0.1** — trait stays in tau-ports as forward-compat anchor; runtime never invokes `Sandbox::create`.
14. **No `Runtime::shutdown`** — `drop` is sufficient; explicit shutdown deferred until a real consumer needs it.
15. **`max_turns` default = 16** — empirical range for typical agentic loops; configurable per-run.
16. **`all_messages` always included in `RunOutcome`** — full conversation visible to caller; opt-out deferred.
17. **Structured-log vocabulary frozen at v0.1** — ~45 events across 9 subsystems (see §3.9). Additive vocabulary changes don't break callers (tracing event names are not API).

Each section includes: rationale, alternatives considered, and the trigger that would prompt a revisit. Status starts as **Proposed**; flips to **Accepted** after the QG22 fresh-eyes review at sub-project sign-off.

---

## 7. Commit / sub-task strategy

The plan derived from this spec follows the per-task commit pattern from Plans 1–4. Anticipated task ordering:

1. Workspace + crate `Cargo.toml` updates (add `tracing` to workspace deps; populate `crates/tau-runtime/Cargo.toml`).
2. `tau-ports` additive amendment: `Tool::capabilities() -> &[Capability]` default method.
3. `error.rs`: `PluginKind` + `BuildError` + `CapabilityDenial`.
4. `error.rs`: `RuntimeError` with `#[from]` composition + 7 unit tests for arrows and display.
5. `options.rs`: `RunOptions` + `TokenUsage` + `Default` + tests.
6. `outcome.rs`: `RunOutcome` enum + tests.
7. `builder.rs`: `Runtime` + `RuntimeBuilder` + plugin registries + `build()` validation + tests.
8. `capability.rs`: per-variant satisfies functions + `check_capabilities` + unit tests.
9. `dispatch.rs`: `resolve_llm_backend`, `resolve_tool`, address-to-tool resolution + tests.
10. `run.rs`: agent multi-turn loop with tracing instrumentation per §3.9 vocabulary.
11. `lib.rs`: re-exports, crate-level rustdoc, lints.
12. Integration test: `run_completed` (happy path with no tool_uses).
13. Integration test: `run_with_tool_calls` (multi-turn with tool dispatch).
14. Integration test: `run_capability_denied` (PolicyDenied path).
15. Integration test: `run_max_turns` (OutOfResources path).
16. Integration test: `run_llm_backend_not_registered` + `run_tool_not_registered` + `run_plugin_contract_violation`.
17. Integration test: `tracing_emission` (assert events emitted per vocabulary).
18. Proptest: `proptest_capability_satisfies`.
19. CI: `no-default-features-runtime` job + branch-protection update queued for sign-off.
20. Update `docs/explanation/escape-hatches.md` with 2 new entries (`builderror-internal`, `runtimeerror-internal`).
21. ADR-0006.
22. Final local verification.
23. ADR-0006 sign-off (24h fresh-eyes review per QG22, or self-review-checklist alternative).
24. QG22 overnight checkpoint + Plan 5 sign-off + ROADMAP + plan tick-off + branch protection update + merge.

---

## 8. Risks & rollbacks

| Risk | Mitigation |
|---|---|
| `Box<dyn Tool<Session = ()>>` registration excludes stateful tools at v0.1 | Documented limitation; `StatelessAdapter` wraps stateless tools; ADR-0006 records the additive `DynTool` extension when stateful tools land. |
| `Tool::capabilities()` additive method breaks downstream impls | Default `&[]` keeps existing impls compiling. The four mocks in `tau-ports::fixtures` continue to work without changes. CI verifies. |
| Capability satisfies-relation has bugs in glob matching | Variant-specific unit tests cover normal cases; proptest covers generative pairs; v0.1 uses simple matching (`**`/`*`/exact) and ADR-0006 documents the simplification. |
| Tracing event vocabulary drifts between docs and code | ADR-0006 freezes the vocabulary; `tests/tracing_emission.rs` asserts a known-good event set fires on a happy-path run. Drift triggers test failure. |
| Async public API forces tokio dep into every downstream | tokio is dev-dep only in tau-runtime; downstream callers (tau-cli) bring tokio at their level. The library is async-runtime-agnostic in code. |
| Caller forgets to pass the right `PackageManifest` for an agent | Documented in `Runtime::run`'s rustdoc; convention is "manifest of `agent_def.package`". Future `RunOptions::strict_manifest_check` could verify package_id matches. |
| Concurrent calls to `Runtime::run` race on a shared `Storage` plugin | Storage plugin authors must implement `Send + Sync` and handle internal concurrency; tau-runtime doesn't serialize runs. ADR-0006 documents this expectation. |
| `MaxTurnsExceeded` arises during a long real run; user wants to inspect partial history | `RunOutcome::Failed` includes `all_messages` + `total_turns` + `token_usage` for inspection. Caller can re-run with higher `max_turns`. |
| Plugin error during a run that callers want to recover from | v0.1: hard fail via `Err(RuntimeError::*)`. ADR-0006 documents the future `RunOptions { soft_fail_tool_errors: bool }` extension for opt-in soft-fail. Caller wraps with their own retry logic at v0.1. |
| Sandbox trait usage breaks before Phase-1 implementation lands | tau-runtime never invokes `Sandbox::create`; the `RuntimeError::Sandbox` variant exists only for forward compat. CI verifies the variant is unused at v0.1. |

Rollback strategy: any single sub-task commit is independently revertable. The plan ordering (deps → tau-ports amendment → leaf errors → composing errors → data shapes → builder → satisfies-relation → dispatch → run loop → tests → CI → docs → ADR) is dependency-bottom-up.

---

## 9. Handoff to writing-plans

Inputs to the next stage:

- **This spec.**
- **`CONSTITUTION.md`** — guidelines G6, G9, G10, G11, G14, G16, NG6, NG12, QG2, QG3, QG5, QG18.
- **`ROADMAP.md` row 4** — sub-project 4 scope summary.
- **Plans 2 + 3 + 4** (`docs/superpowers/plans/2026-04-{26,26,27}-*.md`) for the established commit/test pattern.
- **ADR-0001 through ADR-0005** for the typed-error / forbid-unsafe / strict-clippy / escape-hatch / plugin-trait / manifest-format posture.

The plan should:
1. Decompose §7's 24-step task list into discrete commit-sized steps.
2. Specify the test invocations after each step that prove the step lands cleanly.
3. Include the workspace-deps step + tau-ports amendment as Tasks 1 + 2 (gates everything below).
4. Include integration tests (Tasks 12–17) after the kernel surface lands (Tasks 3–11) so they have something to test against.
5. End with: a "final verification" task, an ADR-0006 sign-off task, and a Plan 5 sign-off task that bundles ROADMAP + plan tick-off + branch-protection update + merge.

After plan acceptance, hand off to `superpowers:subagent-driven-development` for execution under the same branch-protection workflow established in sub-projects 1–3 (feat branch + PR + CI gate, branch protection on main).
