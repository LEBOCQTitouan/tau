# Tau Ports (sub-project 2) — Design Spec

**Date:** 2026-04-26
**Sub-project:** `tau-ports` plugin trait definitions (sub-project 2; second of the Phase-0 sub-projects, after `tau-domain`)
**Author:** Titouan Lebocq
**Status:** Approved for implementation planning

---

## 1. Scope & success criteria

### Scope

Land the trait surface that bridges between tau's runtime core and plugin adapters: `LlmBackend`, `Tool`, `Storage`, and a provisional `Sandbox`. tau-ports is the "ports" layer of the hexagonal architecture (Constitution §1). It depends on tau-domain for shared data types but adds no I/O of its own — it defines what plugins implement and what tau-runtime calls.

### Done when

- `crates/tau-ports/` exposes the traits and supporting types listed in §3.
- `cargo build -p tau-ports --no-default-features` succeeds locally and in CI.
- `cargo build -p tau-ports --all-features` succeeds locally and in CI.
- `cargo build -p tau-ports --features test-fixtures` succeeds locally and in CI.
- `cargo clippy -p tau-ports --all-targets --all-features -- -D warnings` succeeds.
- `cargo fmt --all -- --check` succeeds.
- `cargo test -p tau-ports --all-targets --all-features` succeeds (proptest, doctests, integration, mock fixtures).
- `cargo test -p tau-ports --features test-fixtures` succeeds — fixtures-feature-only test run is green.
- `cargo test -p tau-ports --doc --all-features` succeeds — every public item has an example.
- ADR-0003 (tau-ports trait surface + async-fn-in-trait commitment + Sandbox provisional caveat) is filed in `docs/decisions/` and accepted.
- Five new escape-hatch entries land in `docs/explanation/escape-hatches.md`:
  `llmerror-internal`, `toolerror-internal`, `storageerror-internal`, `sandboxerror-internal`, `completionrequest-provider-specific`.
- The git log on `main` contains a clean per-sub-task series of Conventional Commits.
- CI green on Linux + macOS (Windows non-blocking per G15) for `--no-default-features`, `--all-features`, and `--features test-fixtures` build modes.

### Out of scope (explicit, deferred to later sub-projects or ADRs)

| Item | Owner / Trigger |
|---|---|
| Streaming Tool result events (progress) | additive `CompletionChunk`-style enum on Tool when a UX consumer needs them |
| `LocalTool: !Send` companion trait | only if real plugins demand non-Send sessions |
| Blanket `impl<T: StatelessTool> Tool for T` | additive minor bump if `StatelessAdapter(T)` wrapping proves friction |
| Streaming tool_use deltas (`ToolUseStart` / `ToolUseInputDelta` / `ToolUseStop`) | additive `CompletionChunk` variants when a CLI consumer wants live arg-typing UX |
| Multi-modal LLM input (image, audio, document blocks) | additive `ContentBlock` variants when a vision LLM plugin lands |
| Reasoning / thinking blocks | additive `ContentBlock::Reasoning` |
| Cached-prompt fields | additive `CompletionRequest.cache_control` |
| Embeddings / image generation / audio | separate `EmbeddingBackend` / `ImageBackend` / `AudioBackend` traits — not unified into LlmBackend |
| Storage: blob / streaming get/put | additive methods returning `impl AsyncRead`/`AsyncWrite` |
| Storage: transactions / CAS / atomic multi-key | additive methods with default `Unsupported` impl |
| Storage: TTL / expiration / watch / counters / range queries | additive methods |
| Sandbox: `enter` / `invoke` / `exit` lifecycle on Handle | Phase 1 implementation defines |
| Sandbox: per-sandbox capability validation hooks, introspection, IPC primitives, snapshots | Phase 1+ |
| Sandbox: GPU / network / disk-quota limits | additive `ResourceLimits` fields |
| Top-level umbrella error type | rejected forever (per-trait — same posture as tau-domain) |
| `LlmError::Tool` / `LlmError::Storage` composition | rejected (LlmError stays a leaf; tools wrap their dependencies, not vice versa) |
| Structured `Retryability` enum return | additive — `is_retryable() -> bool` ships v0.1, structured form lands if hints are wanted |
| Error message i18n | forever-out-of-scope in core |
| `serde` feature on tau-ports | Phase 1 RPC sandbox or out-of-process plugin model triggers an additive minor |
| Dynamic plugin loading (cdylib) | Phase 1+ — tau-runtime decides ABI strategy |
| WASM Component Model integration | Phase 1+ |
| Plugin discovery / registration mechanism | tau-runtime concern (sub-project 4) |
| `fn capabilities(&self) -> &[Capability]` on traits | rejected — capabilities are manifest-only per G14 |

---

## 2. Module layout & dependencies

```
crates/tau-ports/
├── Cargo.toml
└── src/
    ├── lib.rs           # crate-level docs, lints, re-exports, fixtures gate
    ├── llm.rs           # LlmBackend trait, CompletionRequest/Response/Stream/Chunk,
    │                    #   LlmProviderMessage, ContentBlock, ToolSpec, ToolUse,
    │                    #   StopReason, TokenUsage, ToolChoice, ToolUseAccumulator,
    │                    #   batch_to_stream, stream_to_batch
    ├── tool.rs          # Tool trait + StatelessTool + StatelessAdapter
    │                    #   + ToolResult + ToolContent + SessionContext
    ├── storage.rs       # Storage trait + Namespace + Key
    ├── sandbox.rs       # Sandbox trait (provisional) + SandboxPlan
    │                    #   + WorkingContext + ResourceLimits
    ├── error.rs         # LlmError, ToolError, StorageError, SandboxError,
    │                    #   NamespaceError, KeyError + is_retryable() impls
    └── fixtures.rs      # MockLlmBackend, MockTool, MockStorage (gated)
```

### Cargo.toml

```toml
[package]
name = "tau-ports"
description = "Port (trait) definitions for tau's hexagonal architecture."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[dependencies]
tau-domain   = { workspace = true, features = ["serde"] }
thiserror    = { workspace = true }
futures-core = "0.3"            # Stream trait only (~50 LOC, zero runtime coupling)
uuid         = { workspace = true }
base64       = { workspace = true }   # for ToolUseAccumulator JSON-delta handling

[features]
default       = []
test-fixtures = []

[dev-dependencies]
tokio        = { version = "1", features = ["macros", "rt", "rt-multi-thread"] }
futures      = "0.3"
proptest     = { workspace = true }
serde_json   = "1"
```

**Key calls:**

- **No `tokio` in `[dependencies]`** — tau-ports is async-runtime-agnostic. Plugin authors return `impl Future` and tau-runtime picks the executor. dev-deps include tokio only for tests.
- **`futures-core` only**, not full `futures`. We need `Stream` for `LlmBackend::stream()`. `futures-core` is small and zero-coupling.
- **`tau-domain` is a hard dep** with the `serde` feature enabled (so `ToolSpec` round-trips to LLM providers as JSON).
- **No `serde` feature on tau-ports itself.** Trait types are runtime contracts; if a Phase-1 use case needs to serialize them (RPC marshaling), that's an additive minor bump.
- **`base64` for `ToolUseAccumulator`** — provider-specific tool-use input deltas are typically partial JSON strings; the accumulator buffers and parses at end-of-block.

---

## 3. Type-by-type design

### 3.1 LLM backend (`llm.rs`)

```rust
use std::pin::Pin;

use futures_core::Stream;

/// Trait implemented by `kind = "llm-backend"` plugins.
pub trait LlmBackend: Send + Sync {
    /// Plugin-visible name (matches the package name; for diagnostics).
    fn name(&self) -> &str;

    /// Make a batch completion request.
    /// Plugin authors with batch-only SDKs implement natively.
    /// Plugin authors with streaming SDKs call `stream_to_batch(self.stream(req).await?)`.
    async fn complete(
        &self,
        req: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError>;

    /// Make a streaming completion request.
    /// Plugin authors with streaming SDKs implement natively.
    /// Plugin authors with batch-only SDKs call `batch_to_stream(self.complete(req).await?)`.
    async fn stream(
        &self,
        req: CompletionRequest,
    ) -> Result<CompletionStream, LlmError>;
}

/// Boxed dyn-stream type at the runtime registry boundary.
pub type CompletionStream =
    Pin<Box<dyn Stream<Item = Result<CompletionChunk, LlmError>> + Send>>;
```

#### Request / response shape

```rust
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<LlmProviderMessage>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub seed: Option<u64>,
    pub tool_choice: ToolChoice,
    pub stop_sequences: Vec<String>,
    /// Provider-specific parameters not yet typed in core.
    /// See: [escape-hatches.md#completionrequest-provider-specific](../explanation/escape-hatches.md#completionrequest-provider-specific).
    pub provider_specific: std::collections::BTreeMap<String, tau_domain::Value>,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    Required,
    None,
    Specific { name: String },
}

impl Default for ToolChoice {
    fn default() -> Self { Self::Auto }
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum LlmProviderMessage {
    User      { content: Vec<ContentBlock> },
    Assistant { content: Vec<ContentBlock> },
    ToolResult {
        tool_use_id: String,
        content: Vec<ContentBlock>,
        is_error: bool,
    },
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ContentBlock {
    Text(String),
    ToolUse(ToolUse),
    // Future: Image { data, mime_type }, Document { ref }, Reasoning { text }, Audio { ref }
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub text: String,
    pub tool_uses: Vec<ToolUse>,
    pub stop_reason: StopReason,
    pub usage: Option<TokenUsage>,
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum CompletionChunk {
    /// Streamed text delta to append to the assistant response.
    Text { delta: String },
    /// Plugin emits this once a tool_use block is fully assembled.
    /// Plugin authors are responsible for buffering provider-specific
    /// tool_use input deltas (see `ToolUseAccumulator`).
    ToolUse(ToolUse),
    /// Final marker. Always emitted once at end of stream.
    Finish { stop_reason: StopReason, usage: Option<TokenUsage> },
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: tau_domain::Value,
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema describing the tool's input. Round-trips through
    /// tau_domain::Value's serde representation.
    pub input_schema: tau_domain::Value,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolUse,
    Error,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}
```

#### Helpers

```rust
/// Convert a batch response into a `CompletionStream` that yields the
/// equivalent chunks (zero or more `Text`, all `ToolUse` blocks, one
/// terminal `Finish`).
pub fn batch_to_stream(resp: CompletionResponse) -> CompletionStream;

/// Consume a `CompletionStream` and reassemble it into a
/// `CompletionResponse`. Concatenates `Text.delta`s, collects `ToolUse`
/// blocks, captures the final `Finish.stop_reason` and `usage`.
pub async fn stream_to_batch(
    stream: CompletionStream,
) -> Result<CompletionResponse, LlmError>;

/// Helper for plugin authors with streaming SDKs that emit JSON tool-use
/// input deltas. Call `append` per delta event; call `finalize` when the
/// tool-use block closes to obtain a `ToolUse`.
pub struct ToolUseAccumulator {
    id: String,
    name: String,
    input_buffer: String,
}

impl ToolUseAccumulator {
    pub fn new(id: String, name: String) -> Self;
    pub fn append(&mut self, json_delta: &str);
    pub fn finalize(self) -> Result<ToolUse, LlmError>;
}
```

#### Design calls

- **`Send + Sync` on the trait** — plugins are stored in a multi-task runtime registry; impls must be safe to call from any task.
- **Native `async fn in trait`** — Rust 1.75+ feature available at MSRV 1.91. No `async-trait` macro overhead, no per-call `Box<dyn Future>` allocation. tau-runtime boxes once at the dyn-cast boundary.
- **Both `complete` and `stream` required, no defaults** — avoids mutual-recursion footgun. Helpers (`batch_to_stream` / `stream_to_batch`) make the inverse implementation a one-liner.
- **`LlmProviderMessage` separate from `tau_domain::Message`** — different abstraction levels. `tau_domain::Message` is the agent's universal envelope; `LlmProviderMessage` is the LLM-call shape. tau-runtime mediates.
- **Multi-block `Vec<ContentBlock>`** — forward-compatible with image/audio/reasoning blocks. `ContentBlock::Text` and `ContentBlock::ToolUse` only at v0.1; non_exhaustive admits future variants.
- **`CompletionChunk::ToolUse` carries fully-assembled tool use** — plugin buffers provider-specific JSON deltas internally via `ToolUseAccumulator`. Consumer code stays simple.
- **`provider_specific: BTreeMap<String, Value>`** — escape hatch per ADR-0003 + escape-hatch policy. Registry entry: `completionrequest-provider-specific`.
- **`StopReason::Error` is distinct from `LlmError`** — Error means "stream completed but reported error"; LlmError means "the trait method itself failed."

### 3.2 Tool (`tool.rs`)

```rust
use std::time::SystemTime;
use tau_domain::{AgentInstanceId, Value};
use uuid::Uuid;

/// Trait implemented by `kind = "tool"` plugins.
/// Stateful by design — see [`StatelessAdapter`] for the common
/// stateless case.
pub trait Tool: Send + Sync {
    /// Per-session state. Use `()` for stateless tools (or use [`StatelessAdapter`]).
    type Session: Send + 'static;

    /// Stable name used for routing. SemVer-stable surface.
    fn name(&self) -> &str;

    /// JSON Schema describing the tool's input. Used both for runtime
    /// validation and for surfacing to the LLM via `CompletionRequest.tools`.
    fn schema(&self) -> ToolSpec;

    /// Open a session. Called once before any `invoke`.
    async fn init(&self, ctx: SessionContext) -> Result<Self::Session, ToolError>;

    /// Perform a single tool call within an open session.
    ///
    /// Return `Err(ToolError)` if the tool itself failed to run (session
    /// unhealthy, contract violation, internal bug). Return
    /// `Ok(ToolResult { is_error: true, ... })` if the tool ran but the
    /// operation reports an error to the LLM (file not found, HTTP
    /// failure, etc.). The runtime treats these differently: errors may
    /// trigger retry / agent-stop; semantic failures are surfaced to the
    /// agent's LLM via `MessagePayload::ToolError`.
    async fn invoke(
        &self,
        session: &mut Self::Session,
        args: Value,
    ) -> Result<ToolResult, ToolError>;

    /// Close the session gracefully. Called once when the runtime decides
    /// the session is done. If the runtime drops the session future
    /// (cancellation), `teardown` is NOT called — plugin authors put
    /// critical cleanup in `Drop` and graceful cleanup here.
    async fn teardown(&self, session: Self::Session) -> Result<(), ToolError>;
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SessionContext {
    pub agent_instance_id: AgentInstanceId,
    pub session_id: Uuid,
    pub deadline: Option<SystemTime>,
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    pub is_error: bool,
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ToolContent {
    Text { text: String },
    Json { data: Value },
    // Future: ImageRef { ... }, AudioRef { ... }, ResourceRef { ... }
}

// ---- StatelessTool + StatelessAdapter ----------------------------

/// Simpler trait for stateless tools. Implement this and wrap with
/// [`StatelessAdapter`] to satisfy [`Tool`] with `Session = ()`.
pub trait StatelessTool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolSpec;
    async fn invoke(&self, args: Value) -> Result<ToolResult, ToolError>;
}

/// Newtype that adapts a [`StatelessTool`] to satisfy [`Tool`].
pub struct StatelessAdapter<T: StatelessTool>(pub T);

impl<T: StatelessTool> Tool for StatelessAdapter<T> {
    type Session = ();
    fn name(&self) -> &str { self.0.name() }
    fn schema(&self) -> ToolSpec { self.0.schema() }
    async fn init(&self, _: SessionContext) -> Result<(), ToolError> { Ok(()) }
    async fn invoke(&self, _: &mut (), args: Value) -> Result<ToolResult, ToolError> {
        self.0.invoke(args).await
    }
    async fn teardown(&self, _: ()) -> Result<(), ToolError> { Ok(()) }
}
```

#### Design calls

- **`Tool::Session: Send + 'static`** — strict. !Send sessions wrap in Mutex / dedicated worker. Aligns with Phase-1 sandbox migration where Wasm Component Model uses Send futures.
- **`StatelessAdapter` is a newtype, not a blanket impl** — avoids coherence issues with downstream `impl Tool` for the same type. Pre-1.0 reversibility (newtype → blanket impl is non-breaking; reverse is breaking).
- **Dual error model preserved** — `ToolError` for "tool failed to run"; `ToolResult { is_error: true }` for "tool ran but reported a semantic error to the LLM." Matches MCP wire format. Documented prominently on `Tool::invoke`.
- **`ToolSpec` re-exported from `llm.rs`** — same type used in `LlmBackend::CompletionRequest.tools` and `Tool::schema()`. Consistency between LLM-side and Tool-side schemas.

### 3.3 Storage (`storage.rs`)

```rust
/// Trait implemented by `kind = "storage"` plugins.
///
/// v0.1 surface is KV-only. Per G8, the namespace carries scope
/// (e.g., global / project / agent-instance); tau-runtime composes
/// namespaces and plugins consume them opaquely.
pub trait Storage: Send + Sync {
    fn name(&self) -> &str;

    async fn get(&self, namespace: &Namespace, key: &Key)
        -> Result<Option<Vec<u8>>, StorageError>;

    async fn put(&self, namespace: &Namespace, key: &Key, value: &[u8])
        -> Result<(), StorageError>;

    async fn delete(&self, namespace: &Namespace, key: &Key)
        -> Result<bool, StorageError>;

    /// List all keys under `namespace` whose names begin with `prefix`.
    /// Use empty prefix `""` to list all keys in the namespace.
    /// Order is plugin-defined.
    async fn list(&self, namespace: &Namespace, prefix: &str)
        -> Result<Vec<Key>, StorageError>;
}

/// Validated namespace identifier. Carries scope information composed
/// by tau-runtime; opaque to Storage plugins.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Namespace(String);

impl Namespace {
    pub const MAX_LEN: usize = 1024;

    pub fn try_new(s: impl Into<String>) -> Result<Self, NamespaceError>;
    pub fn as_str(&self) -> &str;

    // Convenience constructors used by tau-runtime to compose canonical
    // namespaces:
    pub fn global(suffix: &str) -> Self;
    pub fn project(name: &tau_domain::PackageName, suffix: &str) -> Self;
    pub fn agent(id: tau_domain::AgentInstanceId, suffix: &str) -> Self;
}

/// Validated storage key. Within a namespace, opaque content.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key(String);

impl Key {
    pub const MAX_LEN: usize = 1024;

    pub fn try_new(s: impl Into<String>) -> Result<Self, KeyError>;
    pub fn as_str(&self) -> &str;
}
```

#### Design calls

- **No transactions, no atomic multi-key, no TTL, no watch** at v0.1. All additive.
- **`delete` returns `bool`** — idempotent delete; bool distinguishes "newly deleted" vs "wasn't there."
- **`list` returns `Vec<Key>`** — full validated keys, not raw strings. Plugins are trusted to round-trip without producing keys they wouldn't accept.
- **Namespace validation rejects:** empty, NUL bytes, control characters (U+0000..=U+001F, U+007F).
- **Key validation rejects:** empty, NUL bytes only. Keys can contain control chars and arbitrary UTF-8 (e.g., `"\n"`, `"foo:bar"`).
- **Both newtypes capped at 1024 bytes** — keeps wire formats and storage-backend keys predictable.
- **Convenience constructors on `Namespace`** encode scope conventions for tau-runtime; plugins consume via `.as_str()`.

### 3.4 Sandbox (`sandbox.rs`) — PROVISIONAL

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;
use tau_domain::Capability;

/// Trait implemented by `kind = "sandbox"` plugins.
///
/// **PROVISIONAL** — this trait is a v0.1 sketch for plugin authors to
/// anticipate the shape Phase-1 sandboxing will take. The actual
/// implementation (WASM, OS-native, container) is not yet picked, and
/// when it lands, breaking changes to this trait surface are likely.
/// Treat as forward-compatible documentation, not a SemVer commitment
/// beyond the major-version bump that introduces actual sandboxing.
pub trait Sandbox: Send + Sync {
    type Handle: Send + 'static;

    fn name(&self) -> &str;

    async fn create(&self, plan: SandboxPlan) -> Result<Self::Handle, SandboxError>;
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SandboxPlan {
    pub capabilities: Vec<Capability>,
    pub context: Option<WorkingContext>,
    pub limits: Option<ResourceLimits>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct WorkingContext {
    /// Working directory hint. OS-native sandboxes use; WASM ignores.
    pub working_dir: Option<PathBuf>,
    /// Environment variables to seed the sandboxed context.
    pub env: BTreeMap<String, String>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceLimits {
    pub memory_bytes: Option<u64>,
    pub cpu_seconds: Option<u32>,
    pub wall_clock_seconds: Option<u32>,
    pub max_subprocesses: Option<u32>,
}
```

#### Design calls

- **Provisional caveat is load-bearing.** rustdoc on every public item in `sandbox.rs` includes the "PROVISIONAL — Phase 1 may break" prefix. ADR-0003 explicitly disclaims SemVer for this module beyond the major-version bump.
- **`type Handle` opaque at v0.1.** No methods on Handle; Phase-1 implementations add `enter`/`invoke`/`exit` or similar based on the chosen mechanism.
- **`SandboxPlan.capabilities: Vec<tau_domain::Capability>`** — direct reuse. The plan IS the capability declaration handed to the sandbox.
- **`ResourceLimits` covers four universal axes** — memory, CPU time, wall-clock time, subprocess count. WASM impls leave `max_subprocesses` as None.

### 3.5 Errors (`error.rs`)

Per-trait, all `#[non_exhaustive]`, all derive `Debug + Error + Clone + PartialEq + Eq` (uniform with tau-domain pattern).

```rust
use thiserror::Error;

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LlmError {
    #[error("invalid request: {reason}")]
    InvalidRequest { reason: String },
    #[error("rate limited: retry after {retry_after_seconds:?}s")]
    RateLimited { retry_after_seconds: Option<u32> },
    #[error("authentication failed: {message}")]
    Auth { message: String },
    #[error("transport: {message}")]
    Transport { message: String },
    /// Mid-stream error (only emitted from CompletionStream items, never
    /// from the return of `complete()`).
    #[error("stream error: {message}")]
    Stream { message: String },
    #[error("provider error: {message}")]
    Provider { message: String },
    #[error("unsupported: {what}")]
    Unsupported { what: String },
    /// Plugin internal error.
    /// See: [escape-hatches.md#llmerror-internal](../explanation/escape-hatches.md#llmerror-internal).
    #[error("internal: {message}")]
    Internal { message: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ToolError {
    #[error("bad args: {reason}")]
    BadArgs { reason: String },
    #[error("session unusable: {reason}")]
    SessionDead { reason: String },
    #[error("deadline exceeded")]
    DeadlineExceeded,
    #[error("capability denied: {capability}")]
    CapabilityDenied { capability: String },
    /// Underlying LLM call failed (for tools that internally use an LLM).
    #[error("llm: {0}")]
    Llm(#[from] LlmError),
    /// Underlying storage operation failed.
    #[error("storage: {0}")]
    Storage(#[from] StorageError),
    /// Plugin internal error.
    /// See: [escape-hatches.md#toolerror-internal](../explanation/escape-hatches.md#toolerror-internal).
    #[error("internal: {message}")]
    Internal { message: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum StorageError {
    /// Backend rejected the namespace (length cap, reserved prefix, charset).
    #[error("invalid namespace: {reason}")]
    InvalidNamespace { reason: String },
    /// Backend rejected the key (length, charset, reserved prefix).
    #[error("invalid key: {reason}")]
    InvalidKey { reason: String },
    #[error("unavailable: {message}")]
    Unavailable { message: String },
    #[error("timeout")]
    Timeout,
    #[error("unsupported: {what}")]
    Unsupported { what: String },
    /// Plugin internal error.
    /// See: [escape-hatches.md#storageerror-internal](../explanation/escape-hatches.md#storageerror-internal).
    #[error("internal: {message}")]
    Internal { message: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SandboxError {
    #[error("unsupported: {what}")]
    Unsupported { what: String },
    #[error("limit exceeded: {limit}")]
    LimitExceeded { limit: String },
    /// Plugin internal error.
    /// See: [escape-hatches.md#sandboxerror-internal](../explanation/escape-hatches.md#sandboxerror-internal).
    #[error("internal: {message}")]
    Internal { message: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NamespaceError {
    #[error("namespace is empty")]
    Empty,
    #[error("namespace exceeds {max} bytes: got {got}")]
    TooLong { max: usize, got: usize },
    #[error("namespace contains invalid byte (NUL or control char) at position {pos}")]
    InvalidByte { pos: usize },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyError {
    #[error("key is empty")]
    Empty,
    #[error("key exceeds {max} bytes: got {got}")]
    TooLong { max: usize, got: usize },
    #[error("key contains NUL byte at position {pos}")]
    InvalidByte { pos: usize },
}
```

#### Retryability predicates

```rust
impl LlmError {
    /// Heuristic: is the error likely transient? Default-policy hint;
    /// nuanced policies should match on variants.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LlmError::RateLimited { .. }
                | LlmError::Transport { .. }
                | LlmError::Stream { .. }
                | LlmError::Provider { .. },
        )
    }
}

impl StorageError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, StorageError::Unavailable { .. } | StorageError::Timeout)
    }
}

impl ToolError {
    /// Most ToolError variants are NOT retryable — `SessionDead` means
    /// reopen the session; `BadArgs` is permanent. Composed Llm/Storage
    /// errors delegate to their inner predicate.
    pub fn is_retryable(&self) -> bool {
        match self {
            ToolError::Llm(e) => e.is_retryable(),
            ToolError::Storage(e) => e.is_retryable(),
            _ => false,
        }
    }
}

// SandboxError: no retryability predicate at v0.1 (provisional).
```

#### Design calls

- **Per-trait errors** for precise typing at the boundary; no top-level umbrella.
- **`#[from]` composition: `ToolError::Llm(LlmError)` and `ToolError::Storage(StorageError)`** — tools that internally use LLM or Storage propagate via `?`. Direction is unidirectional (Tool → LlmError/StorageError, not vice versa) to avoid cycles and preserve layering.
- **`is_retryable()` on LlmError, StorageError, ToolError** — heuristic for tau-runtime's default retry logic. Documented as a hint, not a contract.
- **`StorageError::InvalidNamespace` and `InvalidKey` separate variants** — backend-specific rejection of namespace vs key, distinct from construction-time `NamespaceError`/`KeyError`.

### 3.6 Test fixtures (`fixtures.rs`)

```rust
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures {
    //! Test fixtures for tau-ports. Gated behind the `test-fixtures` feature.
    //! Downstream crates depend via:
    //! ```toml
    //! [dev-dependencies]
    //! tau-ports = { workspace = true, features = ["test-fixtures"] }
    //! ```

    /// Mock LlmBackend with configurable canned responses.
    pub struct MockLlmBackend {
        // ... canned responses, recorded calls
    }

    /// Mock Tool that records invocations and returns canned ToolResults.
    pub struct MockTool {
        // ...
    }

    /// Mock Storage backed by in-memory BTreeMap<(Namespace, Key), Vec<u8>>.
    pub struct MockStorage {
        // ...
    }

    /// (Provisional) MockSandbox with no-op handles.
    pub struct MockSandbox {
        // ...
    }
}
```

Mocks expose enough surface for tau-runtime, tau-pkg, and future plugin-author tests to verify trait-driven behavior without spinning up real LLM providers / database backends. Production builds (without the feature) don't pull this code.

---

## 4. Parser surface

Two validating constructors qualify as parsers per QG5:

- `Namespace::try_new` — proptest covers grammar (length, NUL, control chars).
- `Key::try_new` — proptest covers grammar (length, NUL).

No other tau-ports types have parsers. Helpers like `ToolUseAccumulator` parse JSON internally but delegate to `serde_json` (covered by upstream's tests).

---

## 5. Testing strategy

Per QG5 (four mandatory layers + proptest for parsers).

### Layers

**Unit tests, inline:**
- Per error enum: `is_retryable()` returns expected truth per variant.
- `Namespace::try_new` and `Key::try_new` cover all NamespaceError / KeyError variants.
- `batch_to_stream(resp)` then `stream_to_batch(stream)` round-trip; check ordering preserved.
- `ToolUseAccumulator::append` then `finalize` reassembles canonical `ToolUse`.
- `StatelessAdapter` round-trip: invoke through both `StatelessTool` and `Tool`, identical results.

**Integration tests, in `tests/`:**
- `tests/llm_helpers.rs` — full CompletionResponse → batch_to_stream → stream_to_batch round-trip.
- `tests/tool_adapter.rs` — StatelessAdapter binds to MockTool's stateless mode.
- `tests/namespace_grammar.rs` — table-driven valid/invalid Namespace and Key inputs.
- `tests/error_composition.rs` — `?` propagation from LlmError to ToolError via `#[from]`.

**Doc tests (mandatory per QG9):**
- Every public item has at least one example.
- Examples on `#[non_exhaustive]` types are `ignore`-marked (same pattern as tau-domain).
- Runnable examples on: `Namespace::try_new`, `Key::try_new`, `is_retryable()`, `StatelessAdapter`, `batch_to_stream`/`stream_to_batch`.

**Property tests (proptest):**
- `proptest_namespace_grammar` — generated valid bytes round-trip; invalid → specific NamespaceError.
- `proptest_key_grammar` — same for Key.
- `proptest_chunk_roundtrip` — arbitrary `Vec<CompletionChunk>` collected via `stream_to_batch` reassembles deterministically.

**Mock-fixture tests:**
- `tests/mock_llm.rs` — assert MockLlmBackend's canned responses behave per docs.
- `tests/mock_tool.rs` — assert MockTool records invocations correctly.
- `tests/mock_storage.rs` — assert MockStorage's in-memory KV satisfies the Storage trait contract.

### CI implications

The existing CI matrix from sub-project 1 covers tau-ports automatically (`fmt`, `clippy`, `test (...)` matrix run workspace-wide). Two new jobs:

```yaml
no-default-features-ports:
  name: build (tau-ports no-default-features)
  steps:
    - run: cargo build -p tau-ports --no-default-features
    - run: cargo test -p tau-ports --no-default-features --lib

test-fixtures:
  name: test (tau-ports test-fixtures only)
  steps:
    - run: cargo test -p tau-ports --features test-fixtures
```

The `test-fixtures` job validates the feature compiles + runs in isolation, catching dependency mistakes that `--all-features` might mask.

### Wire-format goldens

N/A — tau-ports has no serde feature. ToolSpec's wire format (the only type that crosses to LLM providers) round-trips via `tau_domain::Value`'s serde, covered by tau-domain's existing wire-format goldens.

### Escape-hatch registry coverage

The CI registry-coverage test (`crates/tau-domain/tests/escape_hatch_registry.rs`) walks all `crates/**/*.rs` and will detect tau-ports' new `Internal` variants and `provider_specific` field. Five new registry entries land in `docs/explanation/escape-hatches.md` during sub-project 2:

- `llmerror-internal`
- `toolerror-internal`
- `storageerror-internal`
- `sandboxerror-internal`
- `completionrequest-provider-specific`

---

## 6. ADR-0003 — tau-ports trait surface

ADR-0003 lands in the same PR as the implementation, accepted before sub-project 2 closes (mirrors ADR-0001/0002 timing).

### What it records

1. **Native `async fn in trait`** as the async story (vs `async-trait` macro vs sync). MSRV 1.91 supports it. tau-ports stays runtime-agnostic.

2. **The four trait shapes:**
   - `LlmBackend` — both `complete` and `stream`, helpers for inverse implementation, multi-block `LlmProviderMessage`, plugin-buffered tool_use chunks.
   - `Tool` — stateful with `Session: Send + 'static` + init/invoke/teardown, `StatelessAdapter` newtype for stateless tools, dual error model (Result + is_error).
   - `Storage` — KV-only with typed `Namespace` + `Key` newtypes, no transactions/TTL/watch.
   - `Sandbox` — provisional stub, opaque Handle, four-axis ResourceLimits including wall-clock, optional WorkingContext.

3. **Error policy:** per-trait errors with composition (`ToolError::Llm`/`Storage`); `is_retryable()` predicate as a heuristic; each `Internal` variant is a tracked escape hatch.

4. **No serde feature at v0.1.** Trigger for adding: Phase-1 RPC sandbox or out-of-process plugin model.

5. **The `provider_specific: BTreeMap<String, Value>` escape hatch** on CompletionRequest — registered in the escape-hatch registry; promotion when a provider-specific param appears in 2+ plugins.

6. **Sandbox provisional caveat.** v0.1 sandbox trait is a sketch; SemVer is disclaimed beyond the major-version bump that introduces real sandboxing.

7. **Mocks live in tau-ports under `test-fixtures`** (vs per-consumer or separate crate).

---

## 7. Commit / sub-task strategy

The implementation plan derived from this spec follows the same one-commit-per-task pattern as Plans 1 and 2. Anticipated task ordering:

1. Workspace dep additions (`base64` already added in sub-project 1; add `futures-core` to workspace deps); update `crates/tau-ports/Cargo.toml` with deps + features + dev-deps.
2. `error.rs`: NamespaceError + KeyError (the leaves used by `try_new`).
3. `storage.rs`: `Namespace` + `Key` newtypes with validation + tests.
4. `error.rs`: full per-trait error enums (LlmError, ToolError, StorageError, SandboxError) + `is_retryable()` impls + #[from] composition.
5. `llm.rs`: data types (CompletionRequest, LlmProviderMessage, ContentBlock, CompletionResponse, CompletionChunk, ToolUse, ToolSpec, StopReason, TokenUsage, ToolChoice).
6. `llm.rs`: `LlmBackend` trait + `CompletionStream` type alias.
7. `llm.rs`: helper functions (`batch_to_stream`, `stream_to_batch`, `ToolUseAccumulator`).
8. `tool.rs`: `SessionContext` + `ToolResult` + `ToolContent`.
9. `tool.rs`: `Tool` trait.
10. `tool.rs`: `StatelessTool` + `StatelessAdapter`.
11. `storage.rs`: `Storage` trait.
12. `sandbox.rs`: `WorkingContext` + `ResourceLimits` + `SandboxPlan`.
13. `sandbox.rs`: `Sandbox` trait (with PROVISIONAL caveat in rustdoc).
14. `fixtures.rs`: MockLlmBackend + MockTool + MockStorage + MockSandbox.
15. Proptest suite (Namespace, Key, chunk-roundtrip).
16. Integration test suite (llm_helpers, tool_adapter, namespace_grammar, error_composition, mock_*).
17. CI: add `--no-default-features` and `--features test-fixtures` jobs for tau-ports.
18. Update `docs/explanation/escape-hatches.md` with 5 new entries.
19. ADR-0003.
20. Final local verification.
21. ADR-0003 sign-off (24h wait).
22. QG22 overnight checkpoint + Plan 3 sign-off.

---

## 8. Risks & rollbacks

| Risk | Mitigation |
|---|---|
| `async fn in trait` dyn-dispatch surprises | Standard pattern: dyn-cast at registry boundary; documented in ADR-0003. |
| Plugin authors confuse `Err(ToolError)` vs `Ok(ToolResult { is_error: true })` | Heavy rustdoc on `Tool::invoke` covers the distinction; integration test `tests/tool_adapter.rs` exercises both paths. |
| Sandbox provisional shape rejected by Phase-1 implementation | ADR-0003 explicitly disclaims SemVer for sandbox.rs beyond the major-version bump; rustdoc on every public item carries the PROVISIONAL caveat. |
| `provider_specific` escape hatch becomes a permanent grab-bag | Registry promotion rule: when a key appears in 2+ plugins, propose typed field via ADR. |
| `#[from] LlmError` in `ToolError` creates layering confusion | Documented direction: tools wrap their dependencies, not vice versa. ADR-0003 records the rule. |
| `is_retryable()` heuristic encourages wrong default | Documented as heuristic, not contract. Plugins misusing variants is a plugin bug, not a trait bug. |
| Mocks in `fixtures` module diverge from real plugin behavior | Mocks must satisfy the trait contracts; integration tests in `tests/mock_*.rs` enforce this. |

Rollback: any single sub-task commit is independently revertable. The plan's ordering (deps → leaf types → traits) is dependency-bottom-up; reverting a higher-numbered task doesn't break lower ones.

---

## 9. Handoff to writing-plans

Inputs to the next stage:

- **This spec.**
- **`CONSTITUTION.md`** — guidelines G1–G17, NG1–NG12, QG1–QG25, PG1–PG5.
- **`ROADMAP.md` row 2** — sub-project 2 scope summary.
- **Plan 2** (`docs/superpowers/plans/2026-04-26-tau-domain.md`) for committed format conventions.
- **ADR-0001 + ADR-0002** for the typed-error / forbid-unsafe / strict-clippy / escape-hatch posture.

The plan should:
1. Decompose §7's task list into discrete steps (each individually committable, ~22 tasks).
2. Specify the test invocations after each step that prove the step lands cleanly.
3. Include the `tau-ports` Cargo.toml setup as Task 1.
4. End with: a "final verification" task, an ADR-0003 task, and a QG22 overnight checkpoint.

After plan acceptance, hand off to `superpowers:subagent-driven-development` for execution under the same branch-protection workflow established in sub-project 1 (feat branch + PR + CI gate).
