# ADR-0003: tau-ports trait surface

**Status:** Accepted
**Date:** 2026-04-26
**Supersedes:** —

## Context

tau-ports (sub-project 2) defines the four plugin trait boundaries that
every tau plugin author programs against: `LlmBackend`, `Tool`, `Storage`,
and `Sandbox`. These traits, together with their request/response data
shapes, are the most permanent surface in the workspace — once a plugin
ecosystem starts, breaking them invalidates every out-of-tree plugin.

Per QG18, plugin trait boundaries require an ADR. Per Constitution §1
crate scope, tau-ports is `no_std`-leaning (currently `std`-only but
runtime-agnostic) and depends only on `tau-domain` plus `thiserror`.
This ADR records the decisions taken across spec §3 (the type-by-type
design) and locks them at v0.1: async story, trait shapes, error policy,
serde stance, escape-hatch registration, sandbox provisionality, and
where mocks live.

## Decision

### 1. Native `async fn in trait`

tau-ports uses native `async fn in trait` (stabilized in Rust 1.75,
covered by MSRV 1.91 from ADR-0001) for every async trait method.
The `async-trait` macro is not used; a sync-only API is rejected.
Each public trait carries `#[allow(async_fn_in_trait)]` to suppress
the "may have undesirable side-effects" clippy warning, since
tau-ports does not require `Send` futures at the trait level —
runtimes that need `Send` impose it at the bound site.

The crate stays runtime-agnostic: no `tokio`, `async-std`, or
`futures-executor` dependency. Returned futures are `impl Future`
in trait position, bound by the trait's lifetime parameters.

### 2. Four trait shapes

#### `LlmBackend`

- Two methods: `complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError>`
  and `stream(&self, req: CompletionRequest) -> Result<CompletionStream, LlmError>`.
- A blanket helper provides "implement one, get the other" via the
  `BackendCompletionExt` extension trait (drains a stream into a
  response; or upgrades a `complete` to a one-shot stream).
- Messages are multi-block: `LlmProviderMessage` carries
  `Vec<LlmContentBlock>` so a single assistant turn can mix text,
  tool-use, and tool-result blocks.
- Streaming chunks for `tool_use` are plugin-buffered: the backend
  accumulates the JSON until the block closes, then emits one
  `StreamEvent::ToolUse` rather than partial-JSON chunks. Consumers
  do not see incomplete tool calls.

#### `Tool`

- Stateful: associated type `Session: Send + 'static`, with
  `init(&self) -> Result<Session, ToolError>`,
  `invoke(&self, session: &mut Session, args: Value) -> Result<ToolResult, ToolError>`,
  and `teardown(&self, session: Session) -> Result<(), ToolError>`.
- Stateless tools use a `StatelessAdapter<F>` newtype that implements
  `Tool` with `Session = ()`; authors write a single async closure.
- Dual error model: `Result<ToolResult, ToolError>`, where `ToolResult`
  itself carries an `is_error: bool` flag. Hard errors (the tool
  itself failed to execute) return `Err(ToolError)`; soft errors
  (the tool ran but the model should see a failure message) return
  `Ok(ToolResult { is_error: true, .. })`.

#### `Storage`

- KV-only at v0.1: `get`, `put`, `delete`, `list_prefix`. No
  transactions, no TTL, no watch/subscribe.
- Typed newtypes `Namespace(String)` and `Key(String)` prevent
  string-typing collisions and namespace forgery; both validate
  non-empty and reject `\0` on construction.
- Values are `Vec<u8>`. Serialization is the caller's responsibility.

#### `Sandbox`

- Provisional stub at v0.1 (see decision 6).
- Opaque `Handle` newtype: implementations may park a PID, container
  id, or VM handle behind it; consumers may only `wait`, `kill`,
  or query status.
- `ResourceLimits` is four-axis: `cpu_seconds`, `memory_bytes`,
  `file_descriptors`, `wall_clock`. The wall-clock axis is
  belt-and-suspenders against runaway sandboxed work.
- `WorkingContext` (cwd, env, stdin handle) is optional on `spawn` —
  some sandbox kinds (e.g. WASI) do not honor a host cwd.

### 3. Error policy

Each trait has its own concrete error enum: `LlmError`, `ToolError`,
`StorageError`, `SandboxError`. There is no top-level `PortsError`
umbrella.

`ToolError` composes the others via `#[from]` conversions:
`ToolError::Llm(LlmError)` and `ToolError::Storage(StorageError)`.
Tools that delegate to a backend or read from storage propagate
errors with `?` without manual mapping.

Every error enum exposes `is_retryable(&self) -> bool` — a
*heuristic* predicate for transient-vs-permanent classification
(rate limits, transient network, broken pipe → true; bad request,
schema error, missing key → false). Consumers may build retry
loops on it but should not treat the answer as authoritative.

Each enum carries an `Internal { source: anyhow::Error }` (or
`InternalError`) variant as a typed escape hatch. Per ADR-0002
decision 5, every `Internal*` variant is registered in
`docs/explanation/escape-hatches.md` with reason and promotion
trigger; the CI registry test enforces this.

### 4. No serde feature at v0.1

tau-ports does not expose a `serde` feature on any of its types.
Plugin trait shapes are in-process Rust values; (de)serialization
is unnecessary for v0.1 dispatch.

The trigger for adding a `serde` feature is the first cross-process
consumer: either a Phase-1 RPC sandbox kind (where requests/responses
cross a process boundary) or the eventual out-of-process plugin
model. Adding the feature is non-breaking (gated behind
`#[cfg(feature = "serde")]`).

### 5. `provider_specific` escape hatch on `CompletionRequest`

`CompletionRequest` carries a `provider_specific: BTreeMap<String, Value>`
field for vendor-specific parameters that have no typed equivalent
(e.g. Anthropic-only `extended_thinking_budget_tokens`,
OpenAI-only `seed`). The `BTreeMap` deterministic ordering matters
for snapshot tests and idempotent retry signatures.

This is a registered escape hatch: it lives in
`docs/explanation/escape-hatches.md` with the **promotion rule**
that any key appearing in two or more independent plugins is
promoted to a typed field on `CompletionRequest` (or a typed sub-struct
under it) in the next minor.

### 6. Sandbox provisional caveat

The v0.1 `Sandbox` trait is a **provisional sketch**. Real sandboxing
work (Linux namespaces, seccomp, WASI capabilities, Firecracker VM,
or the eventual choice) lives in Phase 1+ and will likely require
shape changes — additional methods, different `Handle` semantics,
or splitting `Sandbox` into multiple traits per kind.

SemVer is **disclaimed** for the `Sandbox` trait beyond the
major-version bump that introduces production sandboxing. The
trait's rustdoc carries a "Stability: provisional" notice. Plugin
authors targeting v0.1 sandbox may need to rewrite at the major
bump.

The other three traits (`LlmBackend`, `Tool`, `Storage`) are not
provisional; they follow normal pre-1.0 SemVer (QG11).

### 7. Mocks live in tau-ports under `test-fixtures`

Test fixtures (`MockLlmBackend`, `MockTool`, `MockStorage`,
`MockSandbox`) live in tau-ports itself, gated behind a
`test-fixtures` cargo feature. Consumer crates depend on tau-ports
with `features = ["test-fixtures"]` in their `[dev-dependencies]`.

Alternatives — per-consumer mocks or a separate `tau-ports-test`
crate — were rejected. Per-consumer duplicates the same mock five
times across the workspace and drifts. A separate crate adds a
publish target and a SemVer surface for what is conceptually one
trait surface; the cargo-feature pattern colocates the mock with
the trait it mocks, so a trait change updates the mock in the
same diff.

## Consequences

- The trait shapes and request/response types are the v0.1 public
  surface for plugin authors. Adding methods is a breaking minor
  pre-1.0 (QG11); adding fields to `#[non_exhaustive]` request/response
  types is non-breaking.
- `async fn in trait` without `Send` bounds means runtimes that need
  `Send` futures (most multi-threaded executors) impose `Send` at the
  bound site. tau-runtime does this; standalone consumers may not need
  to.
- `is_retryable()` is heuristic — consumers building retry loops
  should expect occasional misclassification and complement with
  retry budgets / circuit breakers in their own layer.
- The `provider_specific` escape hatch puts pressure on the registry:
  every entry there is a candidate for promotion; the registry needs
  periodic review (no automated mechanism at v0.1).
- The `Sandbox` provisional caveat means downstream plugins targeting
  v0.1 sandbox accept rewrite cost at the next major. Acceptable
  because no real sandbox plugins exist at v0.1.
- The `test-fixtures` feature means tau-ports has two compile
  configurations CI must exercise (default + `test-fixtures`); the
  CI matrix already runs `--all-features`.
- `ToolError::Llm` / `ToolError::Storage` composition means tool
  authors get one error type to thread, not three. The `From`
  impls keep `?` propagation ergonomic.

## Alternatives considered

- **`async-trait` macro** for trait methods. Rejected: adds a
  proc-macro dependency, boxes every future (`Pin<Box<dyn Future>>`),
  and was the workaround for the missing language feature that 1.91
  now provides natively. No reason to take the dependency.
- **Sync trait methods, with consumers spawning their own tasks.**
  Rejected: most LLM and storage backends are inherently async (HTTP,
  file I/O); forcing a sync façade pessimizes the common path.
- **Top-level `PortsError` umbrella.** Rejected (same reasoning as
  ADR-0002 decision on `DomainError`): per-trait enums forever;
  consumers wanting "any tau-ports error" wrap their own.
- **Stateless `Tool` only** (no `Session` associated type). Rejected:
  long-running stateful tools (database connection, MCP session,
  subprocess pool) are real; forcing them to thread state through
  `Storage` is awkward and pays a serialization cost per call.
- **Single-block `LlmProviderMessage`** (one content per message).
  Rejected: Anthropic and OpenAI both emit multi-block assistant
  turns (text + tool_use); a single-block shape would lose
  fidelity at the wire.
- **Streaming `tool_use` JSON to consumers as it arrives.** Rejected:
  consumers (agents, UIs) cannot safely act on partial JSON; every
  consumer would have to buffer. Buffering at the plugin removes
  the duplication.
- **`Storage` with TTL / transactions / watch at v0.1.** Rejected:
  no v0.1 consumer needs them; adding now commits an API surface
  that may not match real backends. Additive in a later minor.
- **Production sandbox at v0.1.** Rejected: a real sandbox is its
  own multi-week sub-project (Linux capabilities, seccomp, WASI
  research). Provisional stub keeps the trait slot reserved.
- **Serde-derive everything from day one.** Rejected: no in-process
  consumer needs it; gated additive feature is the cheap path when
  cross-process arrives.
- **Mocks in a separate `tau-ports-test` crate.** Rejected: extra
  publish target, extra SemVer surface, drift between mock and
  trait. The `test-fixtures` cargo feature colocates them.
- **No `provider_specific` escape hatch; force every param to be
  typed.** Rejected: every new vendor would block on a tau-ports
  PR before plugin authors could wire up the param. The escape
  hatch + promotion rule keeps tau-ports out of the critical path
  while still curating the typed surface.
