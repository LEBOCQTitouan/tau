# Tool plugins (fs-read + shell) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land real capability enforcement for IPC-backed Tool plugins (closing Gap 1 + Gap 2 from spec §5.2) and ship the first two real Tool plugins (`fs-read` + `shell`).

**Architecture:** Two minimal Tool plugins implementing `tau_ports::Tool` via `tau_plugin_sdk::run_tool_with_config`, spawned as out-of-process subprocesses by `tau-runtime::plugin_host`. The plugins enforce `FsCapability::Read.paths` (glob) and `ProcessCapability::Spawn.commands` (allowlist) at invoke time. To make this work end-to-end, the IPC layer gains: (a) a `tool.describe_capabilities` wire method so plugin-declared capabilities surface to the kernel for the existing `run.rs:272` check; (b) `SessionContext.granted_capabilities` additive field plus threading the kernel's ctx through `DynTool::invoke` so plugins receive the agent's grant.

**Tech Stack:** Rust 1.91, `tokio` (process + time), `globset` (glob matching, fs-read only), `serde` + `serde_json`, `tracing`, `tau-plugin-sdk` (Tool runner). **No new workspace deps** — `globset` lives in fs-read's per-crate deps.

**Sub-project scope:** Phase 1 priority 3. Spec at [`docs/superpowers/specs/2026-04-29-tool-plugins-design.md`](../specs/2026-04-29-tool-plugins-design.md) (commit `d84c0e0`).

---

## Plan-erratum: types, conventions, and traps

These are pre-known invariants from sub-projects 1 + 2a + 2b + 2c. Apply
them verbatim — do NOT re-derive them by reading the spec.

### Tool trait + adjacent types

| Concern | Actual type / shape |
|---|---|
| `tau_ports::Tool` trait | Native async-fn-in-trait. `type Session: Send + 'static; fn name(&self) -> &str; fn schema(&self) -> ToolSpec; fn capabilities(&self) -> &[Capability] { &[] }; async fn init(&self, ctx: SessionContext) -> Result<Self::Session, ToolError>; async fn invoke(&self, session: &mut Self::Session, args: Value) -> Result<ToolResult, ToolError>; async fn teardown(&self, session: Self::Session) -> Result<(), ToolError>;`. Both fs-read + shell use `Session = ()` for in-process compatibility, OR a typed Session for stateful per-call grant — see Task 9 design discussion. |
| `tau_ports::SessionContext` | `#[non_exhaustive]` with fields `agent_instance_id`, `session_id`, `deadline`. Task 2 ADDS `granted_capabilities: Vec<Capability>` (additive). Constructor `SessionContext::new(agent_instance_id, session_id, deadline)` stays 3-arg; new builder method `with_granted_capabilities(self, caps) -> Self` chains for clarity. |
| `tau_ports::ToolResult` | `#[non_exhaustive] { content: Vec<ToolContent>, is_error: bool }`. Construct via `tau_ports::fixtures::make_tool_result(content, is_error)`. Plugin Cargo.toml needs `tau-ports = { workspace = true, features = ["serde", "test-fixtures"] }`. |
| `tau_ports::ToolContent` | `#[non_exhaustive]` with **struct variants**: `Text { text: String }`, `Json { data: tau_domain::Value }`. NOT tuple variants. |
| `tau_ports::ToolError` | Per ADR-0009: `BadArgs { reason }`, `SessionDead { reason }`, `DeadlineExceeded`, `CapabilityDenied { capability }`, `Llm(LlmError)`, `Storage(StorageError)`, `Internal { message }`. Plugins emit `BadArgs` and `Internal` only; the kernel emits `CapabilityDenied` at `run.rs:272`. |
| `tau_ports::ToolSpec` | `#[non_exhaustive] { name, description, input_schema: tau_domain::Value }`. Construct via `tau_ports::fixtures::make_tool_spec(name, description, input_schema)`. Schema is `tau_domain::Value`, NOT `serde_json::Value` — convert via `serde_json::from_str(&serde_json::to_string(&json_value).unwrap()).unwrap()` for static literals. |

### Capability vocabulary

`tau_domain::Capability` is `#[non_exhaustive]`:
- `Filesystem(FsCapability)` where `FsCapability::Read { paths: Vec<String> }` (also `#[non_exhaustive]`).
- `Process(ProcessCapability)` where `ProcessCapability::Spawn { commands: Vec<String> }` (also `#[non_exhaustive]`).
- `Network(NetCapability)`, `Agent(AgentCapability)`, `Custom { name, params }` — not used by these plugins.

### `DynTool` trait (object-safe wrapper for Tool)

Defined at `crates/tau-runtime/src/builder.rs:99`. The current invoke method takes `(&'a mut (), tau_domain::Value)`. Task 3 changes it to take `(&'a SessionContext, &'a mut (), tau_domain::Value)` so the IPC adapter can encode it into the `tool.call` RPC params. The in-process blanket `impl<T: Tool<Session = ()> + 'static> DynTool for T` adapts.

### Wire-protocol carryovers

- Wire methods for tool port: `tool.describe`, `tool.call`. Constants in `tau-plugin-protocol`. Task 4 adds `tool.describe_capabilities`.
- Wire shape for `tool.call` params: `(SessionContext, tau_domain::Value)` — already encodes via existing serde impls. The `granted_capabilities` field rides along once added to `SessionContext`.

### `#[non_exhaustive]` discipline

- Doctests on `#[non_exhaustive]` types must use ` ```ignore ` fences (else E0639).
- Cross-crate destructuring of `#[non_exhaustive]` enums: prefer `let X { fields, .. } = value else { panic!() };`.
- Cross-crate struct construction: use the fixtures helper (`make_tool_result`, `make_tool_spec`, `make_session_context`) — never struct-literal on `#[non_exhaustive]` types from outside the defining crate.

### Verification protocol

`cargo test --all-targets` does NOT run doctests. Each task's
verification block runs `cargo test --doc` separately when the task
adds public items.

### Same-commit escape-hatch registry

The mechanical CI test at `crates/tau-domain/tests/escape_hatch_registry.rs` continues to gate against accidental NEW `Internal` / `Custom` variants. **No new such variants ship in this sub-project.** Plugins use existing typed `ToolError` and `Capability` variants only.

### What this sub-project does NOT introduce

- No new `LlmError` / `ToolError` / `Capability` variants.
- No new ADR — non-breaking amendments to ADR-0008 §5 documented in the sign-off commit.
- No new workspace deps.
- No new SDK amendments (just a new method handler in the existing tool runner).

---

## File Structure

```
crates/
├── tau-ports/src/tool.rs                     -- + granted_capabilities field on SessionContext
├── tau-ports/src/fixtures.rs                 -- update make_session_context helper signature
├── tau-runtime/src/builder.rs                -- DynTool::invoke signature change
├── tau-runtime/src/run.rs                    -- populate granted_capabilities at dispatch
├── tau-runtime/src/plugin_host/ipc_tool.rs   -- call describe_capabilities; thread ctx through invoke
├── tau-plugin-protocol/src/lib.rs            -- + TOOL_DESCRIBE_CAPABILITIES_METHOD constant
├── tau-plugin-sdk/src/runners/tool.rs        -- handle the new wire method
│
├── tau-plugins/fs-read/                      -- NEW plugin
│   ├── Cargo.toml                            -- bin: fs-read-plugin; deps: globset
│   ├── tau.toml                              -- provides=tool; requires fs.read
│   ├── README.md                             -- trust model insert
│   └── src/
│       ├── main.rs                           -- #[tokio::main] → run_tool_with_config
│       ├── lib.rs                            -- pub modules + crate docs
│       ├── plugin.rs                         -- FsReadPlugin: Tool impl
│       ├── config.rs                         -- FsReadConfig (no-op v0.1; reserved)
│       └── path_check.rs                     -- validate_path + admit
│
└── tau-plugins/shell/                        -- NEW plugin
    ├── Cargo.toml                            -- bin: shell-plugin
    ├── tau.toml                              -- provides=tool; requires process.spawn
    ├── README.md                             -- trust model insert
    └── src/
        ├── main.rs                           -- #[tokio::main] → run_tool_with_config
        ├── lib.rs                            -- pub modules + crate docs
        ├── plugin.rs                         -- ShellPlugin: Tool impl
        ├── config.rs                         -- ShellConfig (default/max timeout)
        ├── runner.rs                         -- tokio::process::Command + timeout + capping
        └── command_check.rs                  -- command-name allowlist check

.github/workflows/ci.yml                      -- + 2 new jobs
Cargo.toml                                    -- + 2 workspace members
```

---

## Tasks 1-3: detailed (Plan-2 fidelity)

The first three tasks are documented at full fidelity. Tasks 1-3 cover
workspace scaffold + the load-bearing infrastructure changes
(`SessionContext.granted_capabilities`, `DynTool::invoke` signature).
Tasks 4-19 follow the hybrid format.

---

### Task 1: Workspace scaffold (2 new crates)

Create empty crate skeletons for fs-read + shell. **No functionality
yet** — modules + Tool impls land in subsequent tasks.

**Files:**
- Modify: `Cargo.toml` (workspace root) — append 2 new members.
- Create: `crates/tau-plugins/fs-read/Cargo.toml`
- Create: `crates/tau-plugins/fs-read/tau.toml`
- Create: `crates/tau-plugins/fs-read/src/main.rs` (placeholder stub)
- Create: `crates/tau-plugins/fs-read/src/lib.rs` (empty crate docs)
- Create: `crates/tau-plugins/shell/Cargo.toml`
- Create: `crates/tau-plugins/shell/tau.toml`
- Create: `crates/tau-plugins/shell/src/main.rs` (placeholder stub)
- Create: `crates/tau-plugins/shell/src/lib.rs` (empty crate docs)

Cargo.toml workspace: append the two new members. NO new workspace deps.

fs-read Cargo.toml: bin `fs-read-plugin`, lib `fs_read_plugin_lib`. Standard tau-plugin deps (tau-domain/tau-ports/tau-plugin-protocol/tau-plugin-sdk/serde/serde_json/thiserror/tokio/tracing) plus `globset = "0.4"` (per-crate, NOT workspace) and `tau-ports = { workspace = true, features = ["serde", "test-fixtures"] }`. Dev-deps: `tempfile = { workspace = true }`.

fs-read tau.toml:
```toml
name = "fs-read"
version = "0.1.0"
description = "Read bytes from a single absolute path under fs.read capability scope."

[plugin]
provides = "tool"
kind     = "rust-cargo"
bin      = "fs-read-plugin"

[[capabilities]]
kind = "fs.read"
paths = []
```

shell Cargo.toml: bin `shell-plugin`, lib `shell_plugin_lib`. Standard deps with `tokio` features `["macros", "rt", "rt-multi-thread", "process", "time", "io-util"]`. NO `globset`. Dev-deps: `tempfile`.

shell tau.toml: same pattern, `provides = "tool"`, `[[capabilities]] kind = "process.spawn" commands = []`.

Both lib.rs files: lints (`#![forbid(unsafe_code)] #![deny(missing_docs)] #![deny(rustdoc::broken_intra_doc_links)]`) + crate-level `//!` doc; NO module declarations yet.

Both main.rs files: placeholder `fn main()` that prints to stderr and exits 1, matching the pattern from prior sub-projects' Task 1.

**Verification:** `cargo build --workspace`, `cargo test --workspace --all-targets`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`. ALL PASS; pre-existing tests continue to pass; new crates report 0 tests.

**Commit subject:** `feat(tools): scaffold fs-read + shell plugin crates`

**Refs:** Spec §3.1.

---

### Task 2: `SessionContext.granted_capabilities` additive field

Extend `tau_ports::SessionContext` (which is `#[non_exhaustive]`) with
an additive field `granted_capabilities: Vec<tau_domain::Capability>`. Update
the `make_session_context` fixtures helper. All existing call sites
adapt by either using the constructor (which stays 3-arg with
`granted_capabilities = vec![]` default) or the new
`with_granted_capabilities` builder method.

**Files:**
- Modify: `crates/tau-ports/src/tool.rs` — add field + builder method.
- Modify: `crates/tau-ports/src/fixtures.rs` — update helper to delegate to `SessionContext::new`.

Required changes to `tool.rs`'s SessionContext block:

```rust
#[non_exhaustive]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SessionContext {
    pub agent_instance_id: AgentInstanceId,
    pub session_id: Uuid,
    pub deadline: Option<SystemTime>,
    /// Capabilities the calling agent has been granted by its package
    /// manifest. Plugins use this for finer-grained scope checks
    /// beyond the kernel's structural capability check at
    /// `tau-runtime::run.rs:272`. Defaults to empty.
    #[cfg_attr(feature = "serde", serde(default))]
    pub granted_capabilities: Vec<tau_domain::Capability>,
}

impl SessionContext {
    pub fn new(
        agent_instance_id: AgentInstanceId,
        session_id: Uuid,
        deadline: Option<SystemTime>,
    ) -> Self {
        Self {
            agent_instance_id,
            session_id,
            deadline,
            granted_capabilities: Vec::new(),
        }
    }

    /// Replace the `granted_capabilities` list. Builder pattern.
    pub fn with_granted_capabilities(
        mut self,
        granted_capabilities: Vec<tau_domain::Capability>,
    ) -> Self {
        self.granted_capabilities = granted_capabilities;
        self
    }
}
```

`make_session_context` in `fixtures.rs`: signature unchanged (still 3-arg); body delegates to `SessionContext::new(...)` instead of struct-literal.

**Verification:** `cargo build --workspace`, `cargo test --workspace --all-targets`, `cargo test --workspace --doc`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`. The additive field with `#[serde(default)]` admits older serialized inputs.

**Commit subject:** `feat(tau-ports): add SessionContext.granted_capabilities (additive)`

**Refs:** Spec §5.2 Gap 2.

---

### Task 3: `DynTool::invoke` signature — thread `SessionContext`

Change `tau_runtime::builder::DynTool::invoke` to take
`&'a SessionContext` so the IPC adapter can encode the kernel's ctx
into the `tool.call` RPC params (instead of synthesizing a fresh
ctx, which loses `granted_capabilities`).

**Files:**
- Modify: `crates/tau-runtime/src/builder.rs` — `DynTool::invoke` trait signature + the blanket impl.
- Modify: `crates/tau-runtime/src/run.rs` — call site of the invoke method (after the `init` is reached); reuse the existing ctx via `&ctx` (`SessionContext` derives `Clone`).
- Modify: `crates/tau-runtime/src/plugin_host/ipc_tool.rs` — `IpcTool::invoke` signature; remove the synthesizing block (line 161 area). Use the passed ctx instead.
- Modify: any test files that mock `DynTool` directly — adapt to new signature.

Required signature change in `builder.rs:99` block:

```rust
pub trait DynTool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolSpec;
    fn capabilities(&self) -> &[tau_domain::Capability];
    fn init<'a>(&'a self, ctx: SessionContext) -> BoxFuture<'a, Result<(), ToolError>>;
    fn invoke<'a>(
        &'a self,
        ctx: &'a SessionContext,                       // NEW
        session: &'a mut (),
        args: tau_domain::Value,
    ) -> BoxFuture<'a, Result<ToolResult, ToolError>>;
    fn teardown<'a>(&'a self, session: ()) -> BoxFuture<'a, Result<(), ToolError>>;
}
```

Blanket impl: ignore `_ctx` (the in-process blanket is for `Session = ()` plugins where ctx flows via `init → Session`):

```rust
fn invoke<'a>(
    &'a self,
    _ctx: &'a SessionContext,
    session: &'a mut (),
    args: tau_domain::Value,
) -> BoxFuture<'a, Result<ToolResult, ToolError>> {
    Box::pin(Tool::invoke(self, session, args))
}
```

`run.rs:334` site: bind ctx earlier, pass `ctx.clone()` to `init` and `&ctx` to invoke. (Task 7 fills in the actual `granted_capabilities` data; Task 3 leaves it empty.)

`plugin_host/ipc_tool.rs:155-175`: remove the synthesizing block. Use the `ctx` parameter passed into `invoke`. Clone it inside the async block so it can move across the await:

```rust
fn invoke<'a>(
    &'a self,
    ctx: &'a SessionContext,
    _session: &'a mut (),
    args: Value,
) -> Pin<Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + 'a>> {
    let process = self.process.clone();
    let ctx_owned = ctx.clone();
    Box::pin(async move {
        // ... encode (ctx_owned, &args) into params_bytes ...
    })
}
```

Update the IpcTool struct's docstring to reflect that capabilities are populated via `tool.describe_capabilities` (the new method lands in Task 6).

Find any test mock of `DynTool` (search `grep -rn "fn invoke\(.*DynTool\|impl DynTool" /Users/titouanlebocq/code/tau/crates/`). Update mock signatures.

**Verification:** Full workspace verification matrix. The cascading change touches multiple call sites; clippy may surface stale-argument warnings — fix with underscore-prefix on unused trait params or `#[allow(unused_variables)]` if needed.

**Commit subject:** `feat(tau-runtime): thread SessionContext through DynTool::invoke`

**Refs:** Spec §5.2 Gap 2.

---

## Tasks 4-19: hybrid (per-task summary + spec references)

Each task ends with the same verification protocol and one Conventional
Commits commit + push. Wait for CI green between tasks.

---

### Task 4: `tau-plugin-protocol` — `tool.describe_capabilities` method

**Files:** Modify `crates/tau-plugin-protocol/src/lib.rs` (or wherever the wire method strings live; grep `TOOL_DESCRIBE_METHOD`).

Add a public constant:

```rust
/// Wire method name for describing a Tool plugin's required
/// capabilities. Called once during plugin loading by the host;
/// returns `Vec<tau_domain::Capability>` from `Tool::capabilities()`.
pub const TOOL_DESCRIBE_CAPABILITIES_METHOD: &str = "tool.describe_capabilities";
```

Add 1 unit test asserting the string value (matches existing test patterns in the crate).

**Refs:** Spec §5.2 Gap 1.

**Commit subject:** `feat(plugin-protocol): add tool.describe_capabilities wire method`

---

### Task 5: `tau-plugin-sdk` — handle `tool.describe_capabilities`

**Files:** Modify `crates/tau-plugin-sdk/src/runners/tool.rs`.

Add a new arm in the runner's dispatch loop matching `TOOL_DESCRIBE_CAPABILITIES_METHOD`:

```rust
Some(TOOL_DESCRIBE_CAPABILITIES_METHOD) => {
    let caps: Vec<tau_domain::Capability> = plugin.capabilities().to_vec();
    let response_bytes = rmp_serde::to_vec(&caps)
        .map_err(|e| SdkError::Encode { detail: format!("encode capabilities: {e}") })?;
    let response = Frame::Response { id, result: Ok(response_bytes) };
    write_frame(writer, &response).await?;
}
```

(Adjust to match the actual dispatch pattern in the file — find the existing `TOOL_DESCRIBE_METHOD` arm and mirror its shape.)

Add 2 unit tests via the existing `FakeStdioPeer` test infrastructure:
- `runner_handles_describe_capabilities_for_tool_with_caps` — plugin with `Tool::capabilities() = &[Capability::Filesystem(FsCapability::Read{paths: vec![]})]`; assert response decodes to that vec.
- `runner_handles_describe_capabilities_for_tool_without_caps` — default `capabilities() = &[]`; assert response decodes to empty vec.

**Refs:** Spec §5.2 Gap 1.

**Commit subject:** `feat(plugin-sdk): handle tool.describe_capabilities wire method`

---

### Task 6: `tau-runtime::plugin_host::ipc_tool` — surface caps + thread ctx

**Files:** Modify `crates/tau-runtime/src/plugin_host/ipc_tool.rs`.

**(a) Call `tool.describe_capabilities` during plugin loading.**

The IpcTool is constructed by `plugin_host` after the handshake completes. Currently the `capabilities` field is initialized to `Vec::new()`. Modify the loading path (find `IpcTool {` block construction; likely in a `load` or factory function) to make a `tool.describe_capabilities` RPC call and populate the field from the response.

Pattern: send a Frame::Request with the new method, await the response via the existing in_flight oneshot mechanism, decode `Vec<tau_domain::Capability>` from the response bytes via `rmp_serde::from_slice`. On error or absent response, default to `Vec::new()` and `tracing::warn!` (keeps backward-compat with the toy `echo-tool` which doesn't declare capabilities).

**(b) Already done in Task 3:** `IpcTool::invoke` uses the passed ctx. Verify Task 3's change is in place.

Replace the IpcTool struct's docstring at line 25:

```rust
/// Populated during plugin loading via the `tool.describe_capabilities`
/// wire method. The kernel's capability filter at `run.rs:272`
/// enforces this against the calling agent's package grants.
pub(crate) capabilities: Vec<tau_domain::Capability>,
```

Add 1-2 integration tests in `crates/tau-runtime/tests/`:
- A plugin that declares `[Capability::Filesystem(FsCapability::Read{paths:vec![]})]` — assert IpcTool.capabilities matches after load.
- A plugin that declares no capabilities — assert IpcTool.capabilities is empty.

**Refs:** Spec §5.2 Gap 1 + Gap 2.

**Commit subject:** `feat(plugin-host): surface plugin capabilities + thread kernel ctx`

---

### Task 7: `tau-runtime::run.rs` — populate `granted_capabilities` at dispatch

**Files:** Modify `crates/tau-runtime/src/run.rs` around line 334.

Find the `let ctx = SessionContext::new(...)` site. Replace the empty-grant placeholder (set up in Task 3) with the actual lookup from the agent's package manifest. The agent's package manifest is reachable from the dispatch context — already used at `run.rs:272` for the structural capability check (`run.rs:120: let granted: &[Capability] = package_manifest.capabilities();`).

```rust
let granted_caps: Vec<tau_domain::Capability> = package_manifest.capabilities().to_vec();
let ctx = SessionContext::new(agent_instance_id, uuid::Uuid::new_v4(), None)
    .with_granted_capabilities(granted_caps);
```

Add 1-2 unit tests in `crates/tau-runtime/tests/` exercising:
- An agent with `paths = ["/tmp/**"]` granted; assert the SessionContext passed to `Tool::init` has `granted_capabilities` containing the correct entry.
- An agent with no grants; assert empty.

(Use a custom DynTool mock that captures SessionContext.)

**Refs:** Spec §5.2 Gap 2.

**Commit subject:** `feat(runtime): populate SessionContext.granted_capabilities from package manifest`

---

### Task 8: `fs-read` config + path_check module

**Files:** Create `crates/tau-plugins/fs-read/src/config.rs` + `crates/tau-plugins/fs-read/src/path_check.rs`. Update lib.rs to declare both modules.

**`config.rs`:** `FsReadConfig` with `#[non_exhaustive]`, `Default`, `Deserialize`, `#[serde(deny_unknown_fields)]`. No fields at v0.1; reserved for future expansion.

**`path_check.rs`:** Pure functions for path validation + glob admission.

```rust
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BadArgs {
    Empty,
    NullByte,
    NotAbsolute,
    Traversal,
    NotInScope,
}

impl BadArgs {
    pub(crate) fn reason(&self) -> String {
        match self {
            BadArgs::Empty => "fs-read: path is empty".into(),
            BadArgs::NullByte => "fs-read: path contains a NUL byte".into(),
            BadArgs::NotAbsolute => "fs-read: path is not absolute".into(),
            BadArgs::Traversal => "fs-read: path contains a `..` segment".into(),
            BadArgs::NotInScope => "fs-read: path is not in capability scope".into(),
        }
    }
}

pub(crate) fn validate_path(path: &str) -> Result<&str, BadArgs> {
    if path.is_empty() { return Err(BadArgs::Empty); }
    if path.bytes().any(|b| b == 0) { return Err(BadArgs::NullByte); }
    if !std::path::Path::new(path).is_absolute() { return Err(BadArgs::NotAbsolute); }
    if path.split(std::path::MAIN_SEPARATOR).any(|seg| seg == "..") {
        return Err(BadArgs::Traversal);
    }
    Ok(path)
}

pub(crate) fn admit(path: &str, allowed_globs: &[String]) -> bool {
    use globset::Glob;
    allowed_globs.iter().any(|g| {
        Glob::new(g)
            .ok()
            .map(|gl| gl.compile_matcher().is_match(path))
            .unwrap_or(false)
    })
}
```

**Test inventory (~10 unit tests in path_check.rs `#[cfg(test)] mod tests`):** validate_path empty/null-byte/relative/traversal-dotdot/traversal-middle/happy; admit matches-simple-glob/no-match-outside-scope/invalid-glob-defensive/empty-glob-list. Plus 1-2 tests in config.rs for `Default::default()` + `serde_json::from_str("{}")`.

**Refs:** Spec §4, §6.1.

**Commit subject:** `feat(fs-read): config + path_check (validation + glob admission)`

---

### Task 9: `fs-read` plugin.rs `Tool` impl + main.rs entrypoint

**Files:** Create `crates/tau-plugins/fs-read/src/plugin.rs`. Replace placeholder `crates/tau-plugins/fs-read/src/main.rs`. Update lib.rs to declare `pub mod plugin;`.

**Session design:** `FsReadPlugin` uses `Session = FsReadSession { allowed_globs: Vec<String> }` (NOT `()`). The SDK runner calls `init(ctx)` then `invoke(&mut session, args)` then `teardown(session)` per `tool.call`; the session carries the agent's grant from init to invoke. The `DynTool: Tool<Session = ()>` restriction only matters for IN-process plugins; out-of-process plugins use the SDK runner directly and aren't restricted.

```rust
pub struct FsReadSession {
    allowed_globs: Vec<String>,
}

pub struct FsReadPlugin {
    #[allow(dead_code)]
    config: FsReadConfig,
}

impl Configure for FsReadPlugin { /* ... */ }

impl Tool for FsReadPlugin {
    type Session = FsReadSession;
    fn name(&self) -> &str { "fs-read" }
    fn schema(&self) -> ToolSpec { /* JSON Schema with `path` property */ }
    fn capabilities(&self) -> &[Capability] {
        // OnceLock pattern — Vec::new() isn't const-evaluable.
        static CAPS: std::sync::OnceLock<Vec<Capability>> = std::sync::OnceLock::new();
        CAPS.get_or_init(|| {
            vec![Capability::Filesystem(FsCapability::Read { paths: vec![] })]
        })
    }
    async fn init(&self, ctx: SessionContext) -> Result<Self::Session, ToolError> {
        Ok(FsReadSession {
            allowed_globs: extract_fs_read_paths(&ctx.granted_capabilities),
        })
    }
    async fn invoke(&self, session: &mut Self::Session, args: Value)
        -> Result<ToolResult, ToolError>
    {
        let path_str = parse_path_arg(&args)?;
        let path = validate_path(path_str).map_err(|e| ToolError::BadArgs { reason: e.reason() })?;
        if !admit(path, &session.allowed_globs) {
            return Err(ToolError::BadArgs { reason: BadArgs::NotInScope.reason() });
        }
        match tokio::fs::read(path).await {
            Ok(bytes) => {
                // Build {contents: <base64>, size: <u64>} as ToolContent::Json
                // (binary not natively representable in tau_domain::Value)
                let len = bytes.len() as u64;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                /* ... encode as Json content ... */
                Ok(make_tool_result(vec![ToolContent::Json { data: result_value }], false))
            }
            Err(io_err) => Ok(make_tool_result(
                vec![ToolContent::Text { text: format!("fs-read: {io_err}") }],
                true,  // semantic error to LLM, NOT a ToolError
            )),
        }
    }
    async fn teardown(&self, _session: Self::Session) -> Result<(), ToolError> { Ok(()) }
}

fn parse_path_arg(args: &Value) -> Result<&str, ToolError> { /* extract args.path */ }
fn extract_fs_read_paths(granted: &[Capability]) -> Vec<String> {
    granted.iter().filter_map(|c| match c {
        Capability::Filesystem(FsCapability::Read { paths }) => Some(paths.clone()),
        _ => None,
    }).flatten().collect()
}
```

Cargo.toml: add `base64 = { workspace = true }` (workspace already has it from Anthropic plugin).

`main.rs`: tokio main shim invoking `run_tool_with_config::<FsReadPlugin>(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))`.

**Test inventory (1 unit test in plugin.rs):** `extract_fs_read_paths_collects_from_multiple_grants` — given a `Vec<Capability>` with two `Read{paths}` entries and one `Process::Spawn` entry, assert returns the concatenated `paths` strings, ignoring the spawn entry. Other invoke behavior tested at integration in Task 10.

**Refs:** Spec §4, §6.

**Commit subject:** `feat(fs-read): plugin Tool impl + main entrypoint`

---

### Task 10: `fs-read` integration tests

**Files:** Create `crates/tau-plugins/fs-read/tests/invoke.rs`. Use the SDK test harness `tau_plugin_protocol::test_support::FakeStdioPeer` (similar to anthropic/ollama integration tests).

**3 integration tests:**

1. `integration_read_tempfile_succeeds`:
   - Use `tempfile::NamedTempFile` to create a file with known content.
   - Build a SessionContext with `granted_capabilities = [Capability::Filesystem(FsCapability::Read{paths: vec![<glob covering tempfile>]})]`.
   - Drive the plugin via the SDK harness: send `tool.call((ctx, {"path": <tempfile path>}))`.
   - Assert the response is `ToolResult { is_error: false, content: [Json{data}] }` where `data` decodes to the expected `{contents, size}`.
   - Decode `contents` as base64 and assert equality with the file's bytes.

2. `integration_read_outside_glob_scope_bad_args`:
   - Tempfile at `/tmp/foo.txt`.
   - SessionContext grants `paths: vec!["/var/**"]`.
   - Call with `{"path": "/tmp/foo.txt"}`.
   - Assert `Err(ToolError::BadArgs { reason })` where `reason.contains("not in capability scope")`.

3. `integration_traversal_rejected`:
   - SessionContext grants `paths: vec!["/**"]`.
   - Call with `{"path": "/tmp/../etc/passwd"}`.
   - Assert `Err(ToolError::BadArgs { reason })` where `reason.contains("contains a `..` segment")`.

**Refs:** Spec §8.1.

**Commit subject:** `test(fs-read): integration tests via SDK test harness`

---

### Task 11: `shell` config + command_check

**Files:** Create `crates/tau-plugins/shell/src/config.rs` + `crates/tau-plugins/shell/src/command_check.rs`. Update lib.rs to declare both modules.

**`config.rs`:** `ShellConfig { default_timeout_secs: u64, max_timeout_secs: u64 }`, `#[non_exhaustive]`, defaults 30/600. Add `pub(crate) fn validate(cfg) -> Result<(), ConfigError>` rejecting `default > max` or `0` values.

**`command_check.rs`:**
```rust
pub(crate) fn extract_allowed_commands(granted: &[Capability]) -> Vec<String> {
    granted.iter().filter_map(|c| match c {
        Capability::Process(ProcessCapability::Spawn { commands }) => Some(commands.clone()),
        _ => None,
    }).flatten().collect()
}

pub(crate) fn admit(command: &str, allow_list: &[String]) -> bool {
    allow_list.iter().any(|allowed| allowed == command)
}
```

**Test inventory (~6 unit tests):** default values; validate rejects default>max; validate rejects zero default; extract_allowed_commands concatenates; admit matches; admit no-match.

**Refs:** Spec §5, §6.2.

**Commit subject:** `feat(shell): config + command_check`

---

### Task 12: `shell` runner.rs (subprocess + timeout + capping)

**Files:** Create `crates/tau-plugins/shell/src/runner.rs`. Add `pub(crate) mod runner;` to lib.rs.

Public surface (crate-private):

```rust
pub(crate) struct RunResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
    pub timed_out: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

pub(crate) async fn run_subprocess(
    command: &str,
    args: &[String],
    timeout_secs: u64,
    cwd: Option<&str>,
) -> std::io::Result<RunResult>;
```

Implementation pattern (spec §5.3, §5.4, §5.5):

1. Build `tokio::process::Command::new(command)` with `.args(args)`, `.env_clear()` (no env inheritance), `.stdin(Stdio::null())` (no stdin), `.stdout(Stdio::piped())`, `.stderr(Stdio::piped())`. If `cwd` is Some, `.current_dir(p)`.
2. Spawn the child.
3. Take `stdout = child.stdout.take().unwrap()` and `stderr = child.stderr.take().unwrap()`.
4. Spawn two tokio tasks reading stdout/stderr concurrently into `Vec<u8>` buffers.
5. `tokio::select!` between `child.wait()` and `tokio::time::sleep(Duration::from_secs(timeout_secs))`.
6. On timeout: `child.kill().await?; child.wait().await?;` to reap. Best-effort join the buffer tasks (now finished because child is dead).
7. Truncate each buffer to `MAX_OUTPUT_BYTES = 1024 * 1024` and set the corresponding `*_truncated: bool`.
8. Return `RunResult` with the partial buffers + `timed_out: true, exit_code: -1` on timeout.

`cap_and_flag(buf: Vec<u8>) -> (Vec<u8>, bool)` is a small helper.

**Test inventory (~8 unit tests, gated `#[cfg(unix)]` for OS-specific binaries):**

- `run_echo_returns_stdout` (uses `/bin/echo`)
- `run_nonzero_exit_returns_exit_code` (uses `sh -c "exit 7"`)
- `run_timeout_kills_and_flags_timed_out` (uses `sh -c "sleep 5"` with 1s timeout)
- `run_command_not_found_returns_io_err` (uses `"definitely-not-a-real-command-xyz"`)
- `run_with_cwd_runs_in_directory` (uses `pwd` with cwd = tempfile::tempdir())
- `cap_and_flag_under_limit_no_flag`
- `cap_and_flag_at_exact_limit_no_flag`
- `cap_and_flag_over_limit_truncates`

Windows can be added later via `#[cfg(windows)]` equivalents.

**Refs:** Spec §5.3, §5.4, §5.5.

**Commit subject:** `feat(shell): runner.rs subprocess + wall-clock timeout + 1 MiB output cap`

---

### Task 13: `shell` plugin.rs `Tool` impl + main.rs entrypoint

**Files:** Create `crates/tau-plugins/shell/src/plugin.rs`. Replace placeholder `crates/tau-plugins/shell/src/main.rs`. Update lib.rs to declare `pub mod plugin;`.

Mirror fs-read's plugin.rs structure with these specifics:

- `Session = ShellSession { allowed_commands: Vec<String> }`.
- `init` extracts allowed_commands via `command_check::extract_allowed_commands(&ctx.granted_capabilities)`.
- `invoke` parses args `{command: String, args: Vec<String>, timeout_secs: Option<u64>, cwd: Option<String>}`, validates via `command_check::admit`, validates `cwd` is absolute when set, clamps `timeout_secs` to `[1, max_timeout_secs]` falling back to `config.default_timeout_secs`, calls `runner::run_subprocess(...)`, returns `ToolResult` with structured response (Json content) and `is_error: result.exit_code != 0`.

`schema()` describes the input JSON Schema with `command` (required), `args`, `timeout_secs`, `cwd` properties.

`capabilities()` returns `&[Capability::Process(ProcessCapability::Spawn { commands: vec![] })]` via `OnceLock`.

`main.rs`: tokio main shim invoking `run_tool_with_config::<ShellPlugin>(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))`.

**Test inventory (3 unit tests in plugin.rs):**

- `parse_args_extracts_command_args_timeout_cwd`
- `parse_args_missing_command_returns_bad_args`
- `clamp_timeout_to_max`

**Refs:** Spec §5.

**Commit subject:** `feat(shell): plugin Tool impl + main entrypoint`

---

### Task 14: `shell` integration tests

**Files:** Create `crates/tau-plugins/shell/tests/invoke.rs`. Gated `#[cfg(unix)]`.

**4 integration tests:**

1. `integration_echo_returns_expected_stdout` — grant `commands = ["echo"]`; call `{"command":"echo","args":["hi"]}`; assert stdout decodes to `"hi\n"`, exit_code 0.
2. `integration_long_running_killed_by_timeout` — grant `commands = ["sh"]`; call `{"command":"sh","args":["-c","sleep 5"], "timeout_secs": 1}`; assert `timed_out: true, exit_code: -1`.
3. `integration_command_outside_allowlist_bad_args` — grant `commands = ["echo"]`; call `{"command":"cat","args":["/etc/hostname"]}`; assert `BadArgs` reason contains "command not in capability scope".
4. `integration_large_stdout_truncated_and_flagged` — grant `commands = ["yes"]`; call `{"command":"yes","args":["a"], "timeout_secs":2}`; assert stdout exactly `MAX_OUTPUT_BYTES` long, `stdout_truncated: true`, `timed_out: true` (yes runs forever; timeout fires).

**Refs:** Spec §8.2.

**Commit subject:** `test(shell): integration tests via SDK test harness`

---

### Task 15: Per-plugin README.md trust-model inserts

**Files:** Create `crates/tau-plugins/fs-read/README.md` + `crates/tau-plugins/shell/README.md`.

Each README contains the trust-model insert from spec §10:

> **Trust model (v0.1, sandboxing deferred):** This plugin runs **unsandboxed** on the host process. The runtime enforces capability checks at dispatch (`run.rs:272`); the plugin enforces glob-allowlist / command-allowlist scoping at invoke time. Beyond that, there is NO memory / CPU / network isolation. Constitution G12 + ROADMAP Tier 3 priority 12 will add OS-level sandboxing in a future sub-project. Until then, operators MUST treat installed plugins as host-equivalent code.

Plus a brief usage section showing the relevant tau.toml capability declaration the agent needs.

**Refs:** Spec §10.

**Commit subject:** `docs(tools): add fs-read + shell README trust-model inserts`

---

### Task 16: End-to-end smoke test

**Files:** Create `crates/tau-runtime/tests/tool_plugin_e2e.rs` (or equivalent — check existing test layout).

A single integration test that:

1. Builds a Runtime with both `fs-read-plugin` and `shell-plugin` loaded as out-of-process plugins (use the existing `plugin_host` test helpers; see `crates/tau-runtime/tests/plugin_host_ipc_llm.rs` for the LLM equivalent).
2. Spawns an agent with package manifest declaring grants:
   ```toml
   [[capabilities]] kind = "fs.read" paths = ["${TMPDIR}/**"]
   [[capabilities]] kind = "process.spawn" commands = ["echo"]
   ```
3. Issues a tool call to fs-read with a tempfile in TMPDIR — assert success.
4. Issues a tool call to fs-read with a path OUTSIDE TMPDIR — assert `BadArgs` (plugin-side scope check).
5. Issues a tool call to shell with `command: "echo"` — assert success.
6. Issues a tool call to shell with `command: "ls"` (NOT in allow-list) — assert `BadArgs` (plugin-side scope check).
7. Issues a tool call to fs-read with an agent that has NO `fs.read` capability — assert `RuntimeError::CapabilityDenied` (kernel-side check at `run.rs:272`).

This validates BOTH gaps closed end-to-end:
- Gap 1: kernel rejects step 7 (an agent without fs.read).
- Gap 2: plugin rejects steps 4 and 6 (in-scope agent, out-of-scope target).

**Refs:** Spec §1.2 (G14 enforcement), §5.2 Gap 1 + Gap 2.

**Commit subject:** `test(runtime): e2e smoke for fs-read + shell with capability enforcement`

---

### Task 17: CI — 2 new build jobs

**Files:** Modify `.github/workflows/ci.yml`. Add 2 jobs after the last existing `build-*-plugin` job:

```yaml
  build-fs-read-plugin:
    name: build (fs-read-plugin)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release -p fs-read

  build-shell-plugin:
    name: build (shell-plugin)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release -p shell
```

Job names must match exactly — Task 19's branch protection update queues these into required-status-checks.

**Refs:** Spec §9.

**Commit subject:** `ci(tools): add fs-read + shell build jobs`

---

### Task 18: Final local verification + mark PR ready (gate)

User-driven gate.

- [ ] **Step 18.1:** Full local verification matrix.
- [ ] **Step 18.2:** Verify branch state (up-to-date with main, clean tree).
- [ ] **Step 18.3:** Verify CI green on PR (`gh pr checks <PR#>`).
- [ ] **Step 18.4:** Mark PR ready (`gh pr ready <PR#>`).
- [ ] **Step 18.5:** Surface to user — wait for sign-off.

> "Sub-project tool-plugins implementation complete: 16 work tasks shipped, 23 CI checks green on PR. Awaiting your sign-off to update ROADMAP, branch protection (21→23), and squash-merge."

---

### Task 19: Plan sign-off + ROADMAP + branch protection 21→23 + squash merge (gate)

User-driven gate.

- [ ] **Step 19.1:** Update ROADMAP.md. Add a row to the Phase 1 table after the 2c row marking priority 3 ✅. Update Status line and Tier 1 item 3.
- [ ] **Step 19.2:** Commit + push the ROADMAP update.
- [ ] **Step 19.3:** Update branch protection 21 → 23 (add `build (fs-read-plugin)` + `build (shell-plugin)`).
- [ ] **Step 19.4:** Squash-merge the PR.
- [ ] **Step 19.5:** Verify post-merge state: `gh api ... required_status_checks --jq '.contexts | length'` returns 23.

Sub-project complete. Tier 1 priority 3 done.

---

## Self-review notes (for the plan author)

**Spec coverage check:**

| Spec section | Covered by task |
|---|---|
| §1 / §1.1 / §1.2 | All tasks |
| §2 (settled decisions) | Encoded in implementation |
| §3.1 (workspace layout) | Task 1 |
| §3.2 (deps) | Task 1 |
| §4 fs-read | Tasks 8, 9, 10 |
| §5.1/§5.3/§5.4/§5.5 shell | Tasks 11, 12, 13, 14 |
| §5.2 Gap 1 + Gap 2 | Tasks 2, 3, 4, 5, 6, 7 |
| §6 Configuration | Tasks 8, 11 |
| §7 Error model | Plugin invoke implementations (Tasks 9, 13) |
| §8 Testing | Per-task test inventories |
| §9 CI | Task 17 |
| §10 Trust model | Task 15 (READMEs) |
| §11 Out of scope | No tasks (intentional) |
| §12 Implementation plan outline | This plan IS the expansion |
| §13 Cross-references | Documented in plan header |
| §14 Open follow-ups | Documented in Task 19 |

No spec gaps found.

**Placeholder scan:** No `TBD`/`fill in details`/`Similar to Task N` patterns. Task 12's runner skeleton has explicit "implementation pattern" notes for the kill+drain split-spawn — this is design clarity, not a placeholder.

**Type consistency check:**

- `SessionContext` gains `granted_capabilities` in Task 2; consumed in Tasks 6, 7, 9, 13.
- `DynTool::invoke(&'a SessionContext, &'a mut Session, Value)` in Task 3; called in Task 6.
- `FsReadSession { allowed_globs: Vec<String> }` defined in Task 9.
- `ShellSession { allowed_commands: Vec<String> }` defined in Task 13.
- `RunResult` defined in Task 12; consumed in Task 13.
- `extract_fs_read_paths` (Task 9) and `extract_allowed_commands` (Task 11) — symmetric helpers.

No type-consistency drift.

**Cross-task ordering:**

- Infrastructure (Tasks 1-7) lands before plugins (Tasks 8-15).
- `SessionContext` field (Task 2) before DynTool signature (Task 3) so existing tests adapt incrementally.
- Plugin protocol method (Task 4) before SDK handler (Task 5) before runtime caller (Task 6).
- E2E smoke (Task 16) lands after both plugins exist.
- CI (Task 17) lands last among the work tasks.

---

## Plan complete and saved to `docs/superpowers/plans/2026-04-29-tool-plugins.md`.

Two execution options:

**1. Subagent-Driven (recommended)** — fresh subagent per task, two-stage review (spec compliance + code quality) between tasks, fast iteration on the existing `feat/tool-plugins-spec` branch.

**2. Inline Execution** — execute tasks in this session using `superpowers:executing-plans`, batch execution with checkpoints.

Which approach?
