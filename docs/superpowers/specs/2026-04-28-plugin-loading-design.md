# Plugin loading mechanism — sub-project 1 of Phase 1

**Status:** Draft (this spec) → Implementation plan derived → ADR-0008
filed alongside implementation.

**Sub-project scope:** Phase 1 priority 1 from the
[ROADMAP](../../../ROADMAP.md). Closes the gap left by Phase 0
[ADR-0007 §18](../../decisions/0007-tau-cli.md) ("plugin loading
deferred to Phase 1+"): turns `tau install`'s recorded source trees
into running plugin processes the runtime can dispatch through.

---

## 1. Summary

Tau v0.1 records plugin source trees on disk via `tau install` but has
no way to load them into a running runtime. This sub-project ships
the loading mechanism — out-of-process IPC over MessagePack-RPC on
stdio, with long-lived multiplexed plugin processes — and exercises
it end-to-end with two toy plugins (`echo-llm`, `echo-tool`). It
explicitly does **not** ship the first real LLM-backend plugin
(Anthropic / OpenAI HTTP) or the first real Tool plugin (`fs-read`,
`shell`); those are Phase 1 priorities 2 and 3, each their own
sub-project, building on top of the mechanism this one delivers.

The kernel surface (`tau-runtime::Runtime`) is **unchanged**. Plugin
loading produces `Arc<dyn Dyn{LlmBackend,Tool,Storage,Sandbox}>`
proxies that the existing kernel consumes without knowing they are
remote. All Phase 0 capability filtering, tracing, and dispatch logic
applies to plugin-backed implementations identically.

### 1.1 Scope confirmed

This sub-project ships:

- **Mechanism**: out-of-process IPC over MessagePack-RPC on stdio.
- **Toy plugins** for `LlmBackend` and `Tool` ports — the two ports
  the kernel exercises end-to-end in v0.1. `Storage` and `Sandbox`
  loaders are implemented (so the mechanism is complete) but no toy
  plugin ships for them; their toy plugins land alongside their host
  wiring in a future sub-project.
- **Debug tier**: protocol recording, live wire-decode tracing, and
  three new `tau plugin {describe, run, protocol decode}` subcommands
  to compensate for binary wire format opacity.
- **tau-pkg amendment**: build-on-install for `kind = "rust-cargo"`
  plugin packages.
- **tau-runtime amendment**: `plugin_host` module producing IPC-backed
  port proxies; four new `RuntimeError` variants; ten new tracing
  events.
- **tau-domain amendment**: `PluginManifest`, `PortKind`, `PluginKind`
  types parsed from `[plugin]` table in `tau.toml`.
- **Two new workspace crates**: `tau-plugin-protocol` (pure wire
  types) and `tau-plugin-sdk` (per-port plugin-author runners).

It does NOT ship:

- Real LLM-backend or Tool plugins (priorities 2 and 3, separate
  sub-projects).
- OS-level sandboxing (priority 12, separate sub-project).
- Auto-restart / circuit-breaker around crashed plugins
  (deferred — host returns a typed error and the user re-invokes).
- A plugin conformance test suite (deferred until there are at least
  two real implementations to compare against).
- Multi-port plugins (one package = one port = one binary in v0.1).

### 1.2 Constitution alignment

| Constraint | Mechanism’s answer |
|---|---|
| `forbid(unsafe_code)` workspace-wide | IPC fits without exception — process-level isolation, no FFI at the host boundary. |
| **G6** runtime not framework | Loading produces port proxies the existing kernel consumes; no new abstraction over the agent loop. |
| **G9** observable by default | Ten new tracing events; plugin-side `tracing` re-emitted via stderr; protocol recording is built in. |
| **G12** sandboxing | Out-of-scope here (priority 12), but the IPC mechanism is the *prerequisite* — process boundary already exists; priority 12 will bolt platform sandbox primitives onto the spawn path. |
| **G15** continue-on-fail | Plugin crashes resolve in-flight calls to `RuntimeError::PluginCrashed`; host stays up. |
| **NG4** no marketplace | Plugins distribute as git URLs; tau-pkg builds from source; no registry, no signed binaries. |
| **NG9** no credential management | Plugin config is a static map passed at handshake; how plugins source secrets (env, OS keychain) is each plugin's concern, not the loader's. |
| **NG10** no telemetry | Tracing events are local; recording is an explicit user opt-in via `--record-protocol`. |

---

## 2. Decisions (becomes ADR-0008)

ADR-0008 will bundle these, mirroring the ADR-0006 / ADR-0007 pattern
(Phase 0 sub-projects 4 + 5 each bundled tightly-coupled cross-crate
amendments into one ADR for design coherence). Eighteen decisions:

1. **Mechanism**: out-of-process IPC. Picked over dlopen / `abi_stable`
   / WASM after head-to-head on security, efficiency, constitution
   fit, and tooling maturity in 2026.
2. **Wire format**: MessagePack-RPC over stdio with length-prefixed
   framing. Picked for binary efficiency (~3–5× over JSON for tau
   payloads), self-describing structure (lossless round-trip to JSON
   for debug), and serde-derive ergonomics on the existing `tau-ports`
   types.
3. **Lifecycle**: long-lived multiplexed process per plugin per host
   session. Picked for TLS / HTTP keepalive amortization, REPL
   responsiveness, and amortized spawn cost; per-call spawn rejected
   for compounding overhead in chat workloads.
4. **Concurrency**: JSON-RPC `id`-correlated multiplexing. SDK
   processes requests concurrently by default; opt-in serial mode for
   plugins with non-Send state (e.g., a `&mut` SQLite connection).
5. **Streaming**: notification-based — plugin emits `stream.chunk`
   notifications referencing the originating msgid; final response
   carries `{ stop_reason, usage }`. LSP `partialResult` precedent.
6. **Discovery + build**: `tau.toml`'s `[plugin]` table declares
   `provides`, `kind`, `bin`. `kind = "rust-cargo"` is the only
   variant in v0.1; tau-pkg shells out to `cargo build --release` at
   install time. Future kinds (`python-pip`, `node-npm`, `prebuilt`)
   are additive enum variants.
7. **Handshake**: host-initiated `meta.handshake` request → plugin
   response carrying `protocol_version`, `provides`, `methods`,
   `schemas`, `plugin_name`, `plugin_version`. Host validates
   protocol version, port match, required methods; on mismatch,
   process is killed and `RuntimeError::PluginHandshakeFailed`
   returned with a structured `HandshakeFailureReason`.
8. **Shutdown**: host sends `meta.shutdown` notification on exit.
   Plugin SDK closes pending in-flight calls, runs the plugin's
   shutdown hook (if implemented), exits within
   `shutdown_timeout_ms` (default 2000). After timeout: SIGTERM,
   then SIGKILL after another 500 ms.
9. **Observability**: plugin-side `tracing` events serialize as JSON
   to stderr; host re-emits as `tracing::Event` on
   `target: "plugin::<name>"`. SDK provides the layer pre-configured.
10. **Trace context**: `meta.handshake` carries `{ run_id, agent_id,
    root_span_id }`; plugin SDK injects on every event. True
    distributed-span stitching deferred to G14 perf budgets
    (priority 13).
11. **Config delivery**: static, in `meta.handshake`. Per-call
    overrides not in v0.1 (additive future field).
12. **Sandboxing**: deferred to priority 12. ADR explicitly notes:
    plugin processes in v0.1 run with full host privileges; toy
    plugins are author-trusted; OS-level sandbox primitives
    (seccomp / landlock / sandbox-exec / AppContainer) land in their
    own sub-project.
13. **SDK shape**: per-port generic runner functions. Plugin author
    implements the *same* `tau_ports::*` trait the kernel uses; calls
    `run_llm_backend(plugin)` (or `run_tool`, etc.). No proc macros,
    no derive, no parallel trait surface.
14. **Configure trait**: plugins consuming the handshake `config`
    field implement an additional `Configure` trait. Two runner
    flavors per port: `run_llm_backend(plugin)` (no config) and
    `run_llm_backend_with_config::<T>()` (T impls Configure; runner
    constructs T from handshake).
15. **Testing**: layered. Mock stdio peer in `tau-runtime` for fast
    kernel correctness tests; real-spawn in `tau-cli` integration
    tests for end-to-end mechanism validation. The Phase 0
    `tau-cli` `test-mock` feature flag is retired.
16. **Toy plugins**: `echo-llm` (canned-response LlmBackend) and
    `echo-tool` (echoes args as text content). Both as workspace
    members under `crates/tau-plugins/`. Rebuilt once per test
    session.
17. **Debug tier**: protocol recording (`--record-protocol`), live
    wire-decode under `RUST_LOG=…wire=debug`, and three new
    `tau plugin {describe, run, protocol decode}` subcommands.
18. **Errors**: four new typed `RuntimeError` variants; one new
    `InstallError::BuildFailed` on tau-pkg. **No new `Internal`
    variants** — same-commit escape-hatch registry test continues to
    gate.

### 2.1 Decisions explicitly out of scope

| Topic | Where it lives |
|---|---|
| Real LLM-backend plugin (Anthropic / OpenAI) | Phase 1 priority 2 |
| Real Tool plugin (`fs-read`, `shell`) | Phase 1 priority 3 |
| Capability override (project tau.toml `[agents.<id>.capabilities]`) | Phase 1 priority 4 (tier 2) |
| Transitive `requires.tools` auto-install | Phase 1 priority 5 (tier 2) |
| Schema validation for tool args | Phase 1 priority 6 (tier 2) — `RuntimeError::PluginContractViolation` activates here |
| `tau update` / `tau verify` / `tau uninstall` | Phase 1 priority 7 (tier 2) |
| Streaming LLM responses end-to-end (`Runtime::run_streaming`) | Phase 1 priority 8 (tier 2) — protocol-side streaming primitive lands here, but the kernel consumer is priority 8 |
| Multi-agent orchestration | Phase 1 priority 9 (tier 3) |
| Workflow / pipeline runner | Phase 1 priority 10 (tier 3) |
| REPL persistence | Phase 1 priority 11 (tier 3) |
| OS-level sandboxing | Phase 1 priority 12 (tier 3) |
| Performance budgets in CI | Phase 1 priority 13 (tier 4) |
| `cargo audit` + `cargo-deny` in CI | Phase 1 priority 14 (tier 4) |
| Serve mode (JSON-RPC over stdio) | Phase 1 priority 15 (tier 4) — note: distinct from this sub-project's IPC; "serve mode" is the *kernel* exposing JSON-RPC to *outer* tools, not plugins talking to the kernel |
| Conformance test suite | Deferred until at least two real implementations exist |
| Auto-restart / circuit breaker around crashed plugins | Deferred indefinitely |

---

## 3. Architecture

### 3.1 Workspace layout

**New crates:**

| Crate | Purpose | Depends on |
|---|---|---|
| `tau-plugin-protocol` | Pure wire types: framing, method-name constants, handshake / shutdown payloads, error envelope. **No tokio, no tracing.** Used by both host and SDK. | `tau-domain`, `tau-ports`, `serde`, `rmp-serde` |
| `tau-plugin-sdk` | Server-side per-port runners (`run_llm_backend`, `run_tool`, `run_storage`, `run_sandbox`). Plain Rust; no proc macros. | `tau-plugin-protocol`, `tokio`, `tracing-subscriber` |
| `crates/tau-plugins/echo-llm` | Toy `LlmBackend` plugin replaying canned responses from config. | `tau-plugin-sdk`, `tau-ports` |
| `crates/tau-plugins/echo-tool` | Toy `Tool` plugin echoing args as text content. | `tau-plugin-sdk`, `tau-ports` |

**Modified crates:**

| Crate | Change |
|---|---|
| `tau-pkg` | New `[plugin]` manifest parsing path, `BuildOptions`, install-time build step for `kind = "rust-cargo"`, `LockedPlugin`, lockfile schema bump v1 → v2, new `InstallError::BuildFailed` and `InstallError::CargoNotFound`. |
| `tau-domain` | New `PluginManifest`, `PortKind`, `PluginKind` types with custom serde via Display/FromStr (ADR-0005 pattern). |
| `tau-runtime` | New `plugin_host` module producing `Arc<dyn Dyn*>` proxies; four new `RuntimeError` variants (`PluginSpawnFailed`, `PluginHandshakeFailed`, `PluginCrashed`, `PluginContractViolation`) with `HandshakeFailureReason` sub-enum; ten new tracing events. |
| `tau-cli` | `test-mock` feature retired; `cmd::run` + `cmd::chat` rewired through `plugin_host`; new global flag `--record-protocol <path>`; new subcommand group `tau plugin { describe, run, protocol decode }`. |

### 3.2 Dataflow on `tau run reviewer "review this"`

```
tau-cli
  ├─ resolves project tau.toml → AgentEntry
  ├─ asks tau-pkg.registry for each plugin (llm_backend, tools)
  │   └─ returns LockedPackage { binary_path, manifest.plugin } per plugin
  └─ asks tau-runtime to build a Runtime:
      └─ tau-runtime.plugin_host:
          ├─ spawns echo-llm via tokio::process::Command
          ├─ sends meta.handshake (version, port, trace_context, config)
          ├─ validates plugin's response
          ├─ wraps PluginProcess in IpcLlmBackend (impls DynLlmBackend)
          ├─ same for echo-tool → IpcTool
          ├─ assembles RuntimeBuilder with these IPC-backed impls
          └─ Runtime::run_with_history proceeds unchanged
```

The kernel does not know plugins are remote. Capability filtering,
tool dispatch, multi-turn loops, tracing — all unchanged.

---

## 4. Wire protocol

### 4.1 Framing

Length-prefixed MessagePack stream. Each message:

```
+--------+--------+--------+--------+================+
|  big-endian u32 length (excl PFX) | MessagePack    |
+--------+--------+--------+--------+ message body  |
                                    +================+
```

Max body size: 64 MiB, configurable via `PluginHostOptions`. Bytes
flow over stdin (host→plugin) and stdout (plugin→host); stderr is
reserved for the plugin's structured tracing output.

Length-prefix is stricter than vanilla MessagePack-RPC requires (the
format is self-delimiting), but provides three benefits:

1. Each message is a discrete chunk in a recording log.
2. Non-Rust plugin authors can read N bytes deterministically.
3. Defensive resync if the wire is ever corrupted.

### 4.2 Message types

Per MessagePack-RPC, each message is a fixed-size MessagePack array:

| Frame | Shape |
|---|---|
| Request | `[0, msgid: u32, method: str, params: array]` |
| Response | `[1, msgid: u32, error: ErrorObj \| nil, result: any]` |
| Notification | `[2, method: str, params: array]` |

Notifications have no msgid; they carry the originating request's
msgid in their `params` when they refer to one (streaming).

### 4.3 Method namespace

| Prefix | Methods |
|---|---|
| `meta.*` | `meta.handshake`, `meta.shutdown`, `meta.describe` |
| `llm.*` | `llm.complete`, `llm.stream` |
| `tool.*` | `tool.call`, `tool.describe` |
| `storage.*` | `storage.get`, `storage.put`, `storage.list`, `storage.delete` (defined; not exercised end-to-end in v0.1) |
| `sandbox.*` | `sandbox.run` (defined; not exercised in v0.1) |
| `stream.*` | `stream.chunk` (notification, plugin → host only) |

### 4.4 Handshake

Host first sends:

```
[0, 1, "meta.handshake", [{
    protocol_version: "1",
    port: "llm_backend",
    trace_context: { run_id: "01HXY…", agent_id: "reviewer", root_span_id: "abc…" },
    config: { /* plugin-specific, from project tau.toml's [agents.<id>.config] */ }
}]]
```

Plugin replies:

```
[1, 1, nil, {
    protocol_version: "1",
    provides: "llm_backend",
    plugin_name: "echo-llm",
    plugin_version: "0.1.0",
    methods: ["llm.complete", "llm.stream", "meta.describe"],
    schemas: {
        "llm.complete": { params: {...}, result: {...} },
        "llm.stream":   { params: {...}, result: {...} }
    }
}]
```

Validation rules — any failure → kill the process and return
`RuntimeError::PluginHandshakeFailed` with a structured cause:

| Rule | Failure variant |
|---|---|
| `protocol_version` matches host's | `ProtocolVersionMismatch { host, plugin }` |
| `provides` matches the manifest's `[plugin] provides` | `ProvidesMismatch { manifest, plugin_advertised }` |
| `methods` includes all required for the port | `MissingRequiredMethod { method }` |
| Reply parses as a valid `HandshakeResponse` | `Malformed { detail }` |
| Reply arrives within `handshake_timeout_ms` (default 5000) | `Timeout` |

### 4.5 Method payloads

Method params and results are MessagePack-encoded `tau-ports` types.
Existing serde derives are reused unchanged.

| Method | Params | Result |
|---|---|---|
| `llm.complete` | `[CompletionRequest]` | `CompletionResponse` |
| `llm.stream` | `[CompletionRequest]` | `CompletionChunk::Finish { stop_reason, usage }` (chunks via notifications) |
| `tool.call` | `[SessionContext, Value]` | `ToolResult` |
| `tool.describe` | `[]` | `ToolSpec` |
| `meta.describe` | `[method_name: str]` | `{ params: JSONSchema, result: JSONSchema }` |
| `storage.get` | `[Namespace, Key]` | `Option<Bytes>` |
| `storage.put` | `[Namespace, Key, Bytes]` | `nil` |
| `storage.list` | `[Namespace]` | `Vec<Key>` |
| `storage.delete` | `[Namespace, Key]` | `nil` |
| `sandbox.run` | `[SandboxPlan, WorkingContext, ResourceLimits]` | `SandboxResult` |

The wire RPC method names (`tool.call`, `tool.describe`) are stable; on
the plugin side they map to the `tau_ports::Tool` trait's `invoke` and
`schema` methods respectively. `Tool` is a stateful trait
(`init` → `invoke` → `teardown` per session, with an associated
`Session` type); the SDK's `run_tool` runner runs the full lifecycle
once per `tool.call` RPC, so each RPC is a self-contained
session from the host's point of view. Stateless tools either pick
`type Session = ()` or wrap with the `StatelessAdapter` helper.

### 4.6 Streaming primitive

Host sends `llm.stream` request with msgid=N. Plugin
emits a series of:

```
[2, "stream.chunk", [N, <CompletionChunk>]]
[2, "stream.chunk", [N, <CompletionChunk>]]
...
```

Plugin terminates by sending the regular response on msgid=N, whose
result body is a `CompletionChunk::Finish` variant:

```
[1, N, nil, { stop_reason: "end_turn", usage: { input_tokens: …, output_tokens: … }}]
```

On error mid-stream:

```
[1, N, { code: -32000, message: "rate_limited", data: {...} }, nil]
```

Host SDK's stream router collects `stream.chunk` notifications into
an mpsc channel, terminates on the final response, and exposes a
`Pin<Box<dyn Stream<Item = Result<CompletionChunk, LlmError>>>>` to
the runtime. The runtime consumes it identically to an in-process
implementation.

### 4.7 Error envelope

```
{ code: i32, message: str, data: any }
```

| Code | Meaning |
|---|---|
| `-32700` | Parse error (malformed MessagePack) |
| `-32600` | Invalid request shape |
| `-32601` | Method not found |
| `-32602` | Invalid params |
| `-32603` | Internal plugin error |
| `-32000` | Plugin contract violation (tau-specific) |
| `-32001` | Capability denied (tau-specific) |
| `-32100..-32199` | Port-specific recoverable errors; `data` carries serialized tau-ports `LlmError` / `ToolError` / etc. |

Tool **semantic** errors (file-not-found, HTTP 500 from a fetch tool)
are NOT RPC errors — they ride in `ToolResult.is_error = true`,
distinct from `tool.call` failing at the trait level. This matches
the existing `ToolResult` vs `ToolError` split in `tau-ports`.

### 4.8 Shutdown

Host sends `[2, "meta.shutdown", []]` on host exit. SDK closes
in-flight call handles, runs plugin's shutdown hook if present, exits
cleanly within `shutdown_timeout_ms` (default 2000). After timeout:
SIGTERM, then SIGKILL after another 500 ms.

---

## 5. Plugin SDK (`tau-plugin-sdk`)

Plain Rust library. Per-port generic runner functions; no proc
macros; plugin author implements the same `tau_ports::*` trait the
in-process kernel uses.

### 5.1 Crate layout

```
crates/tau-plugin-sdk/src/
├── lib.rs                    -- re-exports per-port runners
├── framer.rs                 -- length-prefixed MessagePack stdio reader/writer
├── handshake.rs              -- meta.handshake response builder per port
├── tracing_layer.rs          -- tracing-subscriber JSON layer → stderr
├── streaming.rs              -- helper: turn a Stream into stream.chunk notifications
├── runners/
│   ├── llm_backend.rs        -- pub async fn run_llm_backend<T: LlmBackend>(plugin: T)
│   ├── tool.rs               -- pub async fn run_tool<T: Tool>(plugin: T)
│   ├── storage.rs            -- pub async fn run_storage<T: Storage>(plugin: T)
│   └── sandbox.rs            -- pub async fn run_sandbox<T: Sandbox>(plugin: T)
└── error.rs                  -- SdkError (framing, ser/de, IO)
```

### 5.2 Runner signature

```rust
pub async fn run_llm_backend<T>(plugin: T) -> Result<(), SdkError>
where
    T: tau_ports::LlmBackend + Send + Sync + 'static,
{
    tracing_layer::install();

    let mut framer = framer::stdio();
    let req = framer.next_request().await?;
    handshake::respond(&mut framer, req, Port::LlmBackend, plugin_meta::<T>())?;

    let plugin = Arc::new(plugin);
    while let Some(frame) = framer.next().await? {
        match frame {
            Frame::Request { id, method, params } => {
                tokio::spawn(dispatch(plugin.clone(), framer.writer(), id, method, params));
            }
            Frame::Notification { method, .. } if method == "meta.shutdown" => break,
            _ => {} // ignore unknown notifications
        }
    }

    if let Some(hook) = plugin.as_shutdown_hook() {
        hook.on_shutdown().await;
    }
    Ok(())
}
```

### 5.3 Plugin author surface (full file)

```rust
// crates/tau-plugins/echo-llm/src/main.rs
use serde::Deserialize;
use tau_plugin_sdk::{run_llm_backend_with_config, Configure};
use tau_ports::{
    CompletionRequest, CompletionResponse, CompletionStream,
    LlmBackend, LlmError, batch_to_stream,
};

#[derive(Deserialize, Default)]
struct EchoConfig {
    #[serde(default)]
    canned_text: String,
    #[serde(default)]
    script: Vec<String>,
}

struct EchoLlm {
    config: EchoConfig,
    turn: std::sync::atomic::AtomicUsize,
}

impl Configure for EchoLlm {
    type Config = EchoConfig;
    fn from_config(config: Self::Config) -> Result<Self, tau_plugin_sdk::ConfigError> {
        Ok(EchoLlm { config, turn: 0.into() })
    }
}

impl LlmBackend for EchoLlm {
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let i = self.turn.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let text = self.config.script.get(i)
            .cloned()
            .unwrap_or_else(|| self.config.canned_text.clone());
        Ok(CompletionResponse::text(&text))
    }
    async fn stream(&self, req: CompletionRequest)
        -> Result<CompletionStream, LlmError>
    {
        Ok(batch_to_stream(self.complete(req).await?))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_llm_backend_with_config::<EchoLlm>().await?;
    Ok(())
}
```

That's the entire plugin. The same `LlmBackend` trait the in-process
kernel uses. No `#[derive(Plugin)]`. No `#[async_trait]`. No magic.

### 5.4 `Configure` hook

```rust
pub trait Configure {
    type Config: serde::de::DeserializeOwned;
    fn from_config(config: Self::Config) -> Result<Self, ConfigError>
    where
        Self: Sized;
}

#[derive(thiserror::Error, Debug)]
pub enum ConfigError {
    #[error("config decode failed: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("invalid value for field {field}: {detail}")]
    InvalidValue { field: &'static str, detail: String },
}
```

Two flavors per port runner: `run_llm_backend(plugin)` (no config —
plugin already constructed) vs
`run_llm_backend_with_config::<T>()` (T impls Configure; runner
constructs T from handshake config). Plugin author picks based on
whether they need config.

### 5.5 Excluded from v0.1 SDK

- Distributed-trace span stitching beyond the flat `trace_context`
  field (lands with priority 13).
- Auto-retry / circuit breaker (plugin authors implement against
  their own backend if needed).
- Auto-reconnect / re-spawn after plugin crash (host returns
  `Err(RuntimeError::PluginCrashed)` and bubbles up).

---

## 6. tau-pkg amendment

### 6.1 Manifest schema additions

New `[plugin]` table in plugin packages' `tau.toml` — additive,
optional. Packages without it remain non-plugin (data-only) and
skip the build step entirely.

```toml
name = "echo-llm"
version = "0.1.0"
description = "Toy LlmBackend plugin: replays canned responses."

[plugin]
provides = "llm_backend"  # llm_backend | tool | storage | sandbox
kind     = "rust-cargo"   # only kind in v0.1; #[non_exhaustive]
bin      = "echo-llm"     # cargo bin target name
```

### 6.2 New `tau-domain` types

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub provides: PortKind,
    pub kind: PluginKind,
    pub bin: String,
}

#[non_exhaustive]
pub enum PortKind { LlmBackend, Tool, Storage, Sandbox }

#[non_exhaustive]
pub enum PluginKind { RustCargo /* future: PythonPip, NodeNpm, Prebuilt */ }
```

`PortKind` and `PluginKind` use custom serde via Display/FromStr
(ADR-0005 pattern), serializing as `"llm_backend"` / `"rust-cargo"`
strings rather than adjacent-tagged objects.

### 6.3 Install lifecycle delta

Existing tau-pkg lifecycle (10 steps in ADR-0004) gains two new
steps between **clone** and **lockfile write**:

```
1-6: pre-flight, lock, clone, manifest read, scope resolve, clone-rev pin (unchanged)

7. Detect plugin manifest.
   If [plugin] absent → skip build (data-only package; behaves as today).
   If [plugin] present → branch on kind.

8. Build (kind = "rust-cargo"):
   - Spawn `cargo build --release --bin <plugin.bin>` in the cloned package dir.
   - Capture stdout+stderr, stream to host's tracing as
     target = "tau_pkg::build", level INFO for stdout, WARN for stderr.
   - On non-zero exit: Err(InstallError::BuildFailed { exit_status, stderr_tail }).
     Lockfile NOT written. Cloned source NOT removed
     (user retries with `tau install --force` or inspects the failure).
   - On success: record binary_path = <pkg_dir>/target/release/<bin> (canonicalized).

9. Lockfile write (existing, augmented):
   LockedPackage gains plugin: Option<LockedPlugin> with the resolved binary
   path and the plugin manifest contents.
```

### 6.4 `InstallOptions` additions

```rust
#[non_exhaustive]
pub struct InstallOptions {
    pub block_on_lock: bool,
    pub force: bool,
    pub build: BuildOptions,           // NEW
}

#[non_exhaustive]
pub struct BuildOptions {
    pub skip_build: bool,              // for `tau install --no-build`; CI/test
    pub cargo_path: Option<PathBuf>,   // override cargo discovery
    pub extra_args: Vec<String>,       // pass-through (e.g., --features)
}
```

`InstallOptions::default()` enables build, uses `cargo` from PATH,
no extra args.

### 6.5 `LockedPackage` and `LockFile` schema bump

```rust
#[non_exhaustive]
pub struct LockedPackage {
    // existing fields
    pub plugin: Option<LockedPlugin>,  // NEW
}

#[non_exhaustive]
pub struct LockedPlugin {
    pub manifest: PluginManifest,      // copy of [plugin] table
    pub binary_path: PathBuf,          // canonical path to built binary
    pub built_at: SystemTime,
}
```

The lockfile TOML on disk version-bumps from `version = 1` to
`version = 2`. v1 lockfiles auto-upgrade on the next `tau install`
(re-read manifests, re-build any `[plugin]` packages, write v2).
Older `tau` binaries reading v2 surface the existing
`LockfileVersionTooNew` error path — no new error variant needed.

### 6.6 New `InstallError` variants

```rust
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum InstallError {
    // existing variants

    #[error("plugin build failed: cargo exited {exit_status}")]
    BuildFailed {
        exit_status: ExitStatus,
        stderr_tail: String,           // last 4 KiB of cargo's stderr
    },

    #[error("`cargo` not found on PATH (set BuildOptions::cargo_path or install Rust)")]
    CargoNotFound,
}
```

No new `Internal` variants — escape-hatch registry test continues to
pass.

### 6.7 User-facing output

Success:
```
$ tau install https://github.com/example/echo-llm.git
✓ cloned to ~/.tau/packages/echo-llm/0.1.0 (from rev 7a3f9c2)
  building echo-llm@0.1.0 (rust-cargo)...
    Compiling tau-plugin-sdk v0.1.0
    Compiling tokio v1.x
    Compiling rmp-serde v1.x
    Compiling echo-llm v0.1.0
✓ built target/release/echo-llm (4.2 MiB) in 38s
✓ installed: echo-llm@0.1.0 [llm_backend]
```

Failure:
```
$ tau install https://github.com/example/broken-plugin.git
✓ cloned to ~/.tau/packages/broken-plugin/0.1.0
  building broken-plugin@0.1.0 (rust-cargo)...
    Compiling broken-plugin v0.1.0
error[E0425]: cannot find function `frobnicate` in this scope
   --> src/main.rs:42:5
    |
42  |     frobnicate(req).await
    |     ^^^^^^^^^^ not found in this scope

✗ build failed: cargo exited with status 101
  source tree retained at ~/.tau/packages/broken-plugin/0.1.0 — fix and retry with:
    tau install --force https://github.com/example/broken-plugin.git
```

---

## 7. tau-runtime amendment: `plugin_host` module

### 7.1 Module layout

```
crates/tau-runtime/src/plugin_host/
├── mod.rs              -- pub fn load_llm_backend / load_tool / load_storage / load_sandbox
├── process.rs          -- PluginProcess: owns Child + framer + dispatch task
├── framer.rs           -- length-prefixed MessagePack reader/writer (shares wire with SDK)
├── handshake.rs        -- host-side handshake driver
├── ipc_llm.rs          -- IpcLlmBackend: impl DynLlmBackend by RPC
├── ipc_tool.rs         -- IpcTool: impl DynTool by RPC
├── ipc_storage.rs      -- IpcStorage: impl DynStorage by RPC
├── ipc_sandbox.rs      -- IpcSandbox: impl DynSandbox by RPC
├── stream_router.rs    -- routes stream.chunk notifications to per-msgid mpsc
└── recording.rs        -- optional protocol recorder
```

### 7.2 Public API

```rust
pub async fn load_llm_backend(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    trace_context: TraceContext,
    options: PluginHostOptions,
) -> Result<Arc<dyn DynLlmBackend>, RuntimeError>;

pub async fn load_tool(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    trace_context: TraceContext,
    options: PluginHostOptions,
) -> Result<Arc<dyn DynTool>, RuntimeError>;

pub async fn load_storage(/* ditto */) -> Result<Arc<dyn DynStorage>, RuntimeError>;
pub async fn load_sandbox(/* ditto */) -> Result<Arc<dyn DynSandbox>, RuntimeError>;

#[non_exhaustive]
pub struct PluginHostOptions {
    pub handshake_timeout: Duration,         // default 5s
    pub shutdown_timeout: Duration,          // default 2s
    pub max_message_size: usize,             // default 64 MiB
    pub recording: Option<RecordingSink>,
}
```

Returns the existing `Arc<dyn Dyn*>` shim types — kernel paths
unchanged.

### 7.3 `PluginProcess` lifecycle

```rust
struct PluginProcess {
    name: String,
    child: Mutex<Option<Child>>,
    writer: Mutex<FramedWriter>,
    in_flight_responses: Mutex<HashMap<u32, oneshot::Sender<RpcResult>>>,
    in_flight_streams: Mutex<HashMap<u32, mpsc::Sender<CompletionChunk>>>,
    next_msgid: AtomicU32,
    shutdown_signal: Notify,
    _read_task: JoinHandle<()>,
    _stderr_task: JoinHandle<()>,
}
```

**Spawn**:
`tokio::process::Command::new(plugin.binary_path).stdin(piped).stdout(piped).stderr(piped).spawn()`.
Inherit nothing (clean env); pass `TAU_PLUGIN_RUN_ID` and
`TAU_PLUGIN_AGENT_ID` env vars for plugin-side correlation.

**Read loop** (single dedicated tokio task per plugin):

```
loop {
    frame = framer.next().await;
    match frame {
        Frame::Response { id, error, result } =>
            in_flight_responses.remove(&id).send((error, result));
        Frame::Notification { method: "stream.chunk", params: [id, chunk] } =>
            in_flight_streams.get(&id).send(chunk);
        Frame::Notification { .. } => /* ignored */,
        Frame::Request { .. } => /* host doesn't accept plugin-initiated requests */,
    }
}
// on EOF: drain both maps, broadcasting PluginCrashed to all waiters
```

**Stderr task**: reads lines, parses each as a JSON tracing event
matching the SDK's `tracing_layer` output, re-emits via
`tracing::Event::dispatch` with `target = format!("plugin::{}", plugin_name)`.
Lines that fail JSON parse are emitted as raw
`tracing::warn!(target = "plugin::{name}::raw", "{line}")`.

**Shutdown sequence** (on `PluginProcess` drop or explicit shutdown):
1. Send `[2, "meta.shutdown", []]` notification.
2. Wait for child exit, up to `options.shutdown_timeout` (default 2 s).
3. If still alive: SIGTERM, wait 500 ms.
4. If still alive: SIGKILL.
5. Tracing event `plugin.exited { plugin, exit_code, signal, clean }`.

### 7.4 `IpcLlmBackend` example impl

```rust
pub(crate) struct IpcLlmBackend { process: Arc<PluginProcess> }

impl DynLlmBackend for IpcLlmBackend {
    fn complete(
        &self,
        req: CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, LlmError>> + Send + '_>> {
        Box::pin(async move {
            let id = self.process.next_msgid.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            self.process.in_flight_responses.lock().insert(id, tx);
            self.process.writer.lock()
                .send_request(id, "llm.complete", &[req]).await?;
            let (error, result) = rx.await
                .map_err(|_| LlmError::Internal { /* plugin crashed during call */ })?;
            decode_llm_result(error, result)
        })
    }

    fn stream(
        &self,
        req: CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionStream, LlmError>> + Send + '_>> {
        Box::pin(async move {
            let id = self.process.next_msgid.fetch_add(1, Ordering::Relaxed);
            let (chunk_tx, chunk_rx) = mpsc::channel(64);
            let (resp_tx, resp_rx) = oneshot::channel();
            self.process.in_flight_streams.lock().insert(id, chunk_tx);
            self.process.in_flight_responses.lock().insert(id, resp_tx);
            self.process.writer.lock()
                .send_request(id, "llm.stream", &[req]).await?;
            Ok(stream_router::assemble(chunk_rx, resp_rx))
        })
    }
}
```

`IpcTool`, `IpcStorage`, `IpcSandbox` follow the same pattern (no
streaming on the latter three).

### 7.5 Capability filter under IPC

The Phase 0 sub-project 5 `tau-runtime` amendment that filters
`CompletionRequest.tools` by capability runs **before** the request
crosses the wire. With IPC plugins, the filter is therefore
unchanged, and the plugin never sees tools the agent isn't
authorized to use. Integration tests verify this property.

### 7.6 New `RuntimeError` variants

```rust
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum RuntimeError {
    // existing variants

    #[error("failed to spawn plugin {plugin}: {source}")]
    PluginSpawnFailed { plugin: String, source: io::Error },

    #[error("plugin {plugin} handshake failed: {reason}")]
    PluginHandshakeFailed { plugin: String, reason: HandshakeFailureReason },

    #[error("plugin {plugin} crashed: exit {exit_status}")]
    PluginCrashed {
        plugin: String,
        exit_status: ExitStatus,
        stderr_tail: String,
    },

    #[error("plugin {plugin} contract violation: {detail}")]
    PluginContractViolation { plugin: String, detail: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum HandshakeFailureReason {
    Timeout,
    ProtocolVersionMismatch { host: String, plugin: String },
    ProvidesMismatch { manifest: PortKind, plugin_advertised: PortKind },
    MissingRequiredMethod { method: String },
    Malformed { detail: String },
}
```

**No new `Internal` variants** — escape-hatch registry test
continues to gate.

### 7.7 New tracing events

| Event | Level | Fields |
|---|---|---|
| `plugin.spawning` | DEBUG | `plugin`, `binary_path` |
| `plugin.spawned` | INFO | `plugin`, `pid` |
| `plugin.handshake.completed` | DEBUG | `plugin`, `methods`, `duration_ms` |
| `plugin.handshake.failed` | ERROR | `plugin`, `reason` |
| `plugin.request_sent` | TRACE | `plugin`, `msgid`, `method` |
| `plugin.response_received` | TRACE | `plugin`, `msgid`, `duration_ms`, `error_code` |
| `plugin.stream_chunk` | TRACE | `plugin`, `msgid` |
| `plugin.shutdown_sent` | DEBUG | `plugin` |
| `plugin.exited` | INFO | `plugin`, `exit_code`, `clean: bool` |
| `plugin.crashed` | ERROR | `plugin`, `exit_status`, `in_flight_count` |

Plus the per-plugin re-emitted tracing events on
`target = "plugin::<name>"` (the plugin's own log lines).

### 7.8 Protocol recording

When `PluginHostOptions::recording = Some(sink)`:

- Read-loop and writer-mutex tap each frame.
- Each tap appends to `RecordingSink::write(direction, frame_bytes, timestamp)`.
- v0.1 sink: `JsonlFile` — one MessagePack frame per line,
  base64-encoded, with metadata:
  ```json
  {"ts":1714316451.123,"plugin":"echo-llm","dir":"h2p",
   "msgid":1,"method":"meta.handshake","frame":"kgGiZm9v..."}
  ```
- Replayable via `tau plugin protocol decode <path>`.

---

## 8. Toy plugins

### 8.1 `crates/tau-plugins/echo-llm`

Toy `LlmBackend` plugin returning a canned response from config.
Two modes:
1. **Static**: returns `config.canned_text`.
2. **Scripted**: returns `config.script[turn_counter]`, indexed by
   an internal atomic counter, for multi-turn tests.

Test-only modes (gated by `cfg(test)` or config flags):
`crash_after_handshake`, `delay_response_ms`, `error_on_method`. Used
to deterministically exercise host failure paths without flaky timing.

`tau.toml`:
```toml
name = "echo-llm"
version = "0.1.0"
description = "Toy LlmBackend plugin: replays canned responses for tau integration tests."

[plugin]
provides = "llm_backend"
kind     = "rust-cargo"
bin      = "echo-llm"
```

`Cargo.toml`:
```toml
[package]
name = "echo-llm"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "echo-llm"
path = "src/main.rs"

[dependencies]
tau-plugin-sdk = { path = "../../tau-plugin-sdk" }
tau-ports     = { path = "../../tau-ports" }
tokio         = { workspace = true, features = ["macros", "rt-multi-thread"] }
serde         = { workspace = true }
serde_json    = { workspace = true }
```

`src/main.rs` is the example shown in §5.3.

### 8.2 `crates/tau-plugins/echo-tool`

Toy `Tool` plugin echoing args as text.

`tau.toml`:
```toml
name = "echo-tool"
version = "0.1.0"
description = "Toy Tool plugin: echoes call args back as text content."

[plugin]
provides = "tool"
kind     = "rust-cargo"
bin      = "echo-tool"
```

`src/main.rs` (~50 LOC, complete):

```rust
use serde_json::Value;
use tau_plugin_sdk::run_tool;
use tau_ports::{
    SessionContext, Tool, ToolContent, ToolError, ToolResult, ToolSpec,
};

struct EchoTool;

impl Tool for EchoTool {
    type Session = ();

    fn name(&self) -> &str { "echo" }

    fn schema(&self) -> ToolSpec {
        ToolSpec::builder()
            .name("echo")
            .description("Echoes its arguments back as a text content block.")
            .parameters_json(serde_json::json!({
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"]
            }))
            .build()
    }

    async fn init(&self, _ctx: SessionContext) -> Result<(), ToolError> { Ok(()) }

    async fn invoke(
        &self,
        _session: &mut Self::Session,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        let text = args.get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs { detail: "missing 'text'".into() })?;
        Ok(ToolResult {
            content: vec![ToolContent::Text { text: format!("echo: {}", text) }],
            is_error: false,
        })
    }

    async fn teardown(&self, _session: Self::Session) -> Result<(), ToolError> { Ok(()) }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_tool(EchoTool).await?;
    Ok(())
}
```

### 8.3 Storage / Sandbox toy plugins — explicitly deferred

`tau-runtime` does not wire `Storage` or `Sandbox` into the agent
loop in v0.1. The mechanism's `load_storage` / `load_sandbox` paths
are still implemented (and unit-tested with a fake stdio peer), but
on-disk toy crates land alongside the host wiring in their own future
sub-project.

### 8.4 Workspace integration

`crates/tau-plugins/` is added as a workspace member in the root
`Cargo.toml`:

```toml
members = [
    "crates/tau-domain",
    "crates/tau-ports",
    "crates/tau-pkg",
    "crates/tau-runtime",
    "crates/tau-cli",
    "crates/tau-plugin-protocol",
    "crates/tau-plugin-sdk",
    "crates/tau-plugins/echo-llm",
    "crates/tau-plugins/echo-tool",
    # tau-app, tau-infra, tau-observe stubs unchanged
]
```

Toy plugins build under `cargo build --workspace`; CI catches breakage
immediately. They are excluded from any `--release` artifact tau
might publish (they're test fixtures, not products).

### 8.5 How tau-cli integration tests use them

`crates/tau-cli/tests/common/echo_plugins.rs`:

```rust
use std::path::PathBuf;
use std::sync::{Once, OnceLock};

static BUILD_ONCE: Once = Once::new();

pub fn echo_llm_binary() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        BUILD_ONCE.call_once(|| {
            let status = std::process::Command::new(env!("CARGO"))
                .args(["build", "--release", "-p", "echo-llm", "-p", "echo-tool"])
                .status()
                .expect("cargo build for test plugins");
            assert!(status.success());
        });
        target_dir().join("release/echo-llm")
    }).clone()
}
```

Tests synthesize a `tau-lock.toml` pointing `[plugin]` packages at the
pre-built binaries (skipping the actual `tau install` build step) and
exercise `tau run echo-agent "..."` end-to-end against real plugin
processes.

---

## 9. Debug tier

Five capabilities; all ship in v0.1.

### 9.1 Protocol recording — `--record-protocol <path>`

New global flag on `tau`. When set, every plugin process spawned
during this `tau` invocation has its host-side framer wrapped in a
`RecordingTap` that mirrors all frames to the path as JSONL:

```bash
$ tau --record-protocol /tmp/wire.log run reviewer "review this diff"
```

Format described in §7.8. Direction codes: `h2p` (host→plugin) /
`p2h` (plugin→host). The `method` and `msgid` fields are redundant
with the encoded frame — included as indexable shortcuts so `jq` can
filter without decoding.

### 9.2 Live decode in tracing

When `RUST_LOG=tau_runtime::plugin_host::wire=debug`, the read-loop
and writer-mutex emit a tracing event per frame with the **decoded**
body pretty-printed as JSON:

```
DEBUG plugin::wire echo-llm h2p msgid=1 method="meta.handshake"
        params={"protocol_version":"1","port":"llm_backend",...}
DEBUG plugin::wire echo-llm p2h msgid=1
        result={"protocol_version":"1","provides":"llm_backend",...}
```

Decode happens lazily; at higher log levels, the tracing subscriber's
filter rejects the event before any decode work happens
(`tracing`'s static dispatch makes this near-zero-cost when
disabled).

### 9.3 CLI inspector — `tau plugin protocol decode <path>`

Reads a recording file and emits a human-readable transcript.
Flags:

- `--filter plugin=<name>` — restrict to one plugin.
- `--filter method=<glob>` — restrict by method name.
- `--json` — emit one decoded JSON object per line.
- `--from <ts>` / `--to <ts>` — time-range slicing.

### 9.4 Standalone plugin runner — `tau plugin run <binary>`

Launches a plugin standalone and either drives it interactively or
replays a script.

**Interactive:**

```bash
$ tau plugin run ~/.tau/packages/echo-llm/0.1.0/target/release/echo-llm --interactive
✓ spawned (pid 41329)
✓ handshake: provides=llm_backend, methods=[llm.complete, llm.stream]
plugin> llm.complete {"messages":[{"role":"user","content":"hi"}],"model":"echo"}
[+0.012s] result: {"content":[{"text":"hello back"}],"stop_reason":"end_turn",...}
plugin> meta.describe llm.complete
[+0.001s] result: {"params":{...},"result":{...}}
plugin> exit
✓ shutdown clean (pid 41329 exited 0 in 18 ms)
```

The REPL parses `<method> <json-args>`, sends a Request frame, prints
the Response.

**Scripted:**

```bash
$ cat script.jsonl
{"method":"llm.complete","params":[{"messages":[...]}]}
{"method":"meta.describe","params":["llm.complete"]}

$ tau plugin run ~/echo-llm --script script.jsonl
[output: one decoded result line per input]
```

### 9.5 Schema introspection — `tau plugin describe <name>`

Resolves an installed plugin from the lockfile, spawns it, runs
`meta.handshake`, prints the advertised metadata, then runs
`meta.describe` on each method to dump schemas. Plugin is shut down
cleanly after.

```bash
$ tau plugin describe echo-llm
echo-llm 0.1.0  [llm_backend]
─ binary: ~/.tau/packages/echo-llm/0.1.0/target/release/echo-llm
─ protocol: 1
─ methods:
   llm.complete
     params:
       0: CompletionRequest
            { messages: [LlmProviderMessage], model: string, tools?: [ToolSpec], ... }
     result: CompletionResponse
            { content: [ContentBlock], stop_reason: StopReason, usage: TokenUsage }
   llm.stream
     ...
─ source: github.com/example/echo-llm @ 7a3f9c2
```

Schemas come from the plugin's own `meta.handshake` response (the
`schemas` field), so the canonical source is whatever the plugin's
SDK emitted — guaranteeing the documentation matches the running
code.

### 9.6 Total surface

| Capability | LOC delta |
|---|---|
| `--record-protocol` flag + RecordingSink | ~30 LOC |
| Live wire-decode tracing layer | ~20 LOC |
| `tau plugin protocol decode` subcommand | ~150 LOC |
| `tau plugin run --interactive` REPL | ~150 LOC |
| `tau plugin describe` subcommand | ~50 LOC |
| **Total** | ~400 LOC |

Three of the five (recording, live decode, describe) are nearly free
byproducts of the framer + handshake design. Two (decode CLI,
interactive runner) are small new subcommands. Total debug-tier
surface is small enough to ship in v0.1 without slipping the
sub-project.

---

## 10. Testing & CI

Layered: mock stdio peer in `tau-runtime` for fast kernel correctness
+ real-spawn in `tau-cli` for end-to-end mechanism validation +
per-crate unit + doctest discipline.

### 10.1 Test taxonomy

| Layer | Where | What it proves | Speed |
|---|---|---|---|
| `tau-plugin-protocol` unit + proptest | `crates/tau-plugin-protocol/tests/` | Framer round-trips arbitrary frames; length-prefix integrity; max-size rejection | <1 s |
| `tau-plugin-sdk` unit | `crates/tau-plugin-sdk/tests/` | Per-port runner dispatches correctly given a fake stdio peer; handshake response shape; tracing layer JSON output | ~2 s |
| `tau-runtime::plugin_host` unit | `crates/tau-runtime/src/plugin_host/` (in-module `#[cfg(test)]`) | `IpcLlmBackend` etc. dispatch correctly given a fake stdio peer (no real process); stream router; crash detection | ~2 s |
| `tau-runtime` integration | `crates/tau-runtime/tests/plugin_host_*.rs` | End-to-end via fake peer: handshake failure modes, capability filter on plugin tools, error propagation | ~3 s |
| **`tau-cli` integration (real spawn)** | `crates/tau-cli/tests/cmd_run_plugin.rs`, `cmd_chat_plugin.rs` | **Full IPC mechanism**: tau install → cargo build → tau run echo-agent → real plugin process | ~10 s |
| `tau-cli` snapshot | `help_snapshots.rs` (existing) + `protocol_decode_snapshots.rs` (new) | `tau plugin describe`, `tau plugin protocol decode` output stays stable | <1 s |

### 10.2 Mock stdio peer

```rust
// crates/tau-plugin-protocol/src/test_support.rs (visible behind `test-support` feature)
pub struct FakeStdioPeer {
    read_tx: tokio::sync::mpsc::Sender<Frame>,
    write_rx: tokio::sync::mpsc::Receiver<Frame>,
}

impl FakeStdioPeer {
    pub fn new() -> (Self, FramedReader, FramedWriter) { ... }
    pub async fn expect_handshake(&mut self) -> HandshakeRequest { ... }
    pub async fn send_handshake_response(&mut self, resp: HandshakeResponse) { ... }
    pub async fn expect_request(&mut self, method: &str) -> (u32, Vec<u8>) { ... }
    pub async fn send_response(&mut self, id: u32, result: impl Serialize) { ... }
    pub async fn send_stream_chunk(&mut self, id: u32, chunk: CompletionChunk) { ... }
    pub async fn send_crash(self) { /* drops; framer sees EOF */ }
}
```

Used by `tau-runtime::plugin_host` tests to construct an
`IpcLlmBackend` against a `FakeStdioPeer` instead of a real process.
Fast, deterministic, no subprocess overhead.

### 10.3 Real-spawn integration tests

```rust
#[tokio::test]
async fn tau_run_against_real_echo_llm_returns_canned_response() {
    let workspace = TempProjectFixture::new();
    workspace.write_tau_toml(/* names echo-llm + echo-tool, configures canned_text */);
    workspace.synthesize_lockfile(echo_llm_binary(), echo_tool_binary());

    let assert = AssertCmd::cargo_bin("tau").unwrap()
        .current_dir(workspace.path())
        .args(["run", "echo-agent", "say hello"])
        .assert();

    assert.success()
          .stdout(predicates::str::contains("hello back"));
}

#[tokio::test]
async fn tau_run_propagates_plugin_crash_as_exit_code_2() {
    let workspace = TempProjectFixture::new();
    workspace.write_tau_toml(/* echo-llm with crash_after_handshake = true */);
    workspace.synthesize_lockfile(echo_llm_binary(), echo_tool_binary());

    let assert = AssertCmd::cargo_bin("tau").unwrap()
        .current_dir(workspace.path())
        .args(["run", "echo-agent", "anything"])
        .assert();

    assert.code(2)  // ExitCode::Error per sub-project 5
          .stderr(predicates::str::contains("plugin echo-llm crashed"));
}
```

### 10.4 tau-pkg build-step tests

```rust
#[tokio::test]
async fn install_runs_cargo_build_for_rust_cargo_plugin() {
    let scope = TempScope::new();
    let fixture_repo = git_fixture::echo_llm_local_clone();
    let installed = tau_pkg::install_with_options(
        &PackageSource::Git { location: fixture_repo, rev: None },
        &scope,
        InstallOptions::default(),
    ).unwrap();
    assert!(installed.binary_path.exists());
    assert!(installed.binary_path.metadata().unwrap().permissions().is_executable());
}

#[tokio::test]
async fn install_surfaces_compile_error_as_build_failed() {
    /* fixture repo with `let x: i32 = "hi";` in main.rs */
    /* assert Err(InstallError::BuildFailed { stderr_tail, .. }) */
}
```

### 10.5 Doctest discipline (Phase 0 carry-over)

- All public types in `tau-plugin-protocol` and `tau-plugin-sdk` get
  doctests.
- `#[non_exhaustive]` types: doctests marked `ignore` (E0639 from
  external doctest compilation contexts).
- `cargo test --all-targets` does NOT include doctests; CI runs
  `cargo test --doc` separately for both new crates.
- Tests destructuring `#[non_exhaustive]` enums use
  `let X { fields, .. } = value else { panic!() };`.

### 10.6 CI matrix changes

Three new jobs added to `.github/workflows/ci.yml`:

| Job | Command | Why |
|---|---|---|
| `build (tau-plugin-protocol)` | `cargo build -p tau-plugin-protocol` | New crate |
| `build (tau-plugin-sdk)` | `cargo build -p tau-plugin-sdk` | New crate |
| `build (tau-plugins)` | `cargo build -p echo-llm -p echo-tool` | Toy plugins must compile across the matrix; integration tests depend on these binaries |

Existing test/lint jobs gain coverage automatically:
- `cargo test --workspace --all-targets --all-features` picks up new tests.
- `cargo test --workspace --doc` picks up new doctests.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` lints them.

Total required checks: 12 → 15. Branch protection on `main` updated
as part of Plan sign-off (matches sub-project 4 + 5 cadence).

### 10.7 Performance smoke

A single bound test in `tau-cli` integration tests:

```rust
#[tokio::test]
async fn handshake_completes_under_one_second_in_release() {
    let start = Instant::now();
    let _backend = tau_runtime::plugin_host::load_llm_backend(...).await.unwrap();
    assert!(start.elapsed() < Duration::from_secs(1),
            "handshake too slow: {:?}", start.elapsed());
}
```

Loose bound — guards against catastrophic regressions, doesn't gate
on micro-optimization. True perf budgets land with priority 13.

### 10.8 Conformance suite — explicitly deferred

Building a suite that validates one toy plugin per port is over-fitting.
Plans for a `tau-plugin-conformance` crate begin once the first real
LLM-backend plugin lands (Phase 1 priority 2), at which point a suite
has two implementations to compare against.

---

## 11. Migration

| Concern | Path |
|---|---|
| **Lockfile schema** | TOML `version = 1` → `version = 2`. v1 lockfiles auto-upgrade on the next `tau install` (re-read manifests, re-build any `[plugin]` packages, write v2). Older `tau` binaries reading v2 surface the existing `LockfileVersionTooNew` error path. |
| **Existing v0.1 installations** | `LockedPackage { plugin: None }` for legacy installs. `tau ls` shows them as data-only; `tau run` referencing them as a plugin fails with `RuntimeError::PluginContractViolation { detail: "package is not a plugin (no [plugin] manifest table)" }`. Documented in release notes. |
| **`test-mock` feature** | Removed from `tau-cli/Cargo.toml`. `cfg(feature = "test-mock")` blocks deleted. Tests previously gated on this rebuild against `echo-llm` / `echo-tool`. |
| **Existing `Runtime::run` consumers** | Unchanged. The kernel's signature is identical; the only difference is the type of `Arc<dyn DynLlmBackend>` it receives. All Phase 0 integration tests keep passing. |
| **Plugin author distribution** | Pre-amendment: no plugins to distribute. Post-amendment: `tau install <git-url>` clones + builds. No backward-incompatibility because there is no "back". |

---

## 12. Implementation plan outline (~26 tasks)

The plan derived from this spec follows the Phase 0 cadence: one
Conventional Commits commit per task, full verification (build /
clippy / test / fmt --check / doctest) before commit, push after each
task, PR auto-triggers CI.

| # | Task | Crate(s) |
|---|---|---|
| 1 | Workspace scaffold: add `tau-plugin-protocol` + `tau-plugin-sdk` crates with empty libs; register in workspace Cargo.toml | workspace, both new crates |
| 2 | `tau-domain`: PluginManifest, PortKind, PluginKind types with custom serde via Display/FromStr | tau-domain |
| 3 | `tau-plugin-protocol`: framing primitives (length-prefix MessagePack reader/writer) | tau-plugin-protocol |
| 4 | `tau-plugin-protocol`: Frame enum, message-type constants, error envelope types | tau-plugin-protocol |
| 5 | `tau-plugin-protocol`: handshake + shutdown payload types | tau-plugin-protocol |
| 6 | `tau-plugin-protocol`: FakeStdioPeer test-support module (behind `test-support` feature) | tau-plugin-protocol |
| 7 | `tau-plugin-sdk`: tracing-stderr layer + framer integration | tau-plugin-sdk |
| 8 | `tau-plugin-sdk`: handshake response builder per port | tau-plugin-sdk |
| 9 | `tau-plugin-sdk`: run_llm_backend + run_tool runners (Storage/Sandbox runners stubbed); streaming wired here | tau-plugin-sdk |
| 10 | `tau-plugin-sdk`: Configure + ConfigError; run_*_with_config flavors | tau-plugin-sdk |
| 11 | `tau-pkg`: manifest table parsing + InstallOptions::build + BuildOptions | tau-pkg, tau-domain |
| 12 | `tau-pkg`: install build step; InstallError::BuildFailed; LockedPlugin; lockfile v2 | tau-pkg |
| 13 | `tau-runtime`: plugin_host module skeleton; PluginProcess + PluginHostOptions + RuntimeError variants | tau-runtime |
| 14 | `tau-runtime`: spawn + handshake + stderr re-emit + shutdown sequence | tau-runtime |
| 15 | `tau-runtime`: IpcLlmBackend (non-streaming) + IpcTool with mock-peer unit tests | tau-runtime |
| 16 | `tau-runtime`: stream router + IpcLlmBackend::stream | tau-runtime |
| 17 | `tau-runtime`: protocol recording (RecordingSink::JsonlFile + tap wiring) | tau-runtime |
| 18 | echo-llm + echo-tool toy plugin crates | crates/tau-plugins/* |
| 19 | `tau-cli`: drop `test-mock` feature; rewire cmd::run + cmd::chat to load via plugin_host | tau-cli |
| 20 | `tau-cli`: --record-protocol global flag + tau plugin {describe,run,protocol decode} subcommands | tau-cli |
| 21 | `tau-cli` integration tests: real-spawn against echo plugins (cmd_run_plugin, cmd_chat_plugin) | tau-cli |
| 22 | CI: 3 new required jobs; branch protection PUT for required-status-checks | .github/workflows + sign-off ceremony |
| 23 | ADR-0008 file + index update | docs/decisions |
| 24 | Final local verification + mark PR ready (user-driven gate) | — |
| 25 | ADR-0008 fresh-eyes review (24 h or self-review checklist per QG22) | — |
| 26 | Plan sign-off + ROADMAP + branch-protection update + squash merge | — |

---

## 13. Cross-references

- ADR-0008 (filed alongside implementation, this spec drives it)
  supersedes [ADR-0007](../../decisions/0007-tau-cli.md) §18's "plugin
  loading deferred" note.
- ADR-0008 references [ADR-0004](../../decisions/0004-tau-pkg.md) §6
  (`tau install` from git URLs) — extended, not reversed.
- ADR-0008 references
  [ADR-0005](../../decisions/0005-package-source-and-kind-serde.md)
  (custom serde via Display/FromStr) — same pattern applied to
  `PortKind` and `PluginKind`.
- ADR-0008 references [ADR-0006](../../decisions/0006-tau-runtime.md)
  (Tool capability filter) — capability filter remains correct under
  IPC because it runs before requests cross the wire.
- [ROADMAP](../../../ROADMAP.md) Phase 1 priority 1 marked
  "in progress" → "completed" at sub-project sign-off.
- [ROADMAP](../../../ROADMAP.md) Phase 1 priority 2 (first real
  LLM-backend) becomes the natural next sub-project; v0.1's IPC
  mechanism is its substrate.

---

## 14. Open follow-ups

Items NOT in this sub-project's scope but tracked for the Phase 1
backlog so they aren't lost:

- **Plugin auto-restart / circuit breaker** around crashed plugins —
  deferred indefinitely; user re-invokes for now.
- **Per-call config override** (host can pass per-request config to
  plugin) — additive future field on RPC requests, not in v0.1.
- **Multi-port plugins** (one binary providing both an `LlmBackend`
  and a related `Tool`) — deferred; v0.1 enforces one port per package
  for simplicity.
- **Distributed-trace span stitching** beyond the flat `trace_context`
  field — lands with priority 13 (perf budgets in CI).
- **Conformance test suite** (`tau-plugin-conformance` crate) —
  starts after priority 2 produces a second implementation worth
  validating.
- **Non-Rust plugin kinds** (`python-pip`, `node-npm`, `prebuilt`) —
  additive `PluginKind` enum variants when demand materializes.
- **Plugin signing / verification** — out of scope; tau is not a
  marketplace (NG4). May be revisited if priority 2/3 plugins ship as
  binaries through anything other than `cargo build` from source.
