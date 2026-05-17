# `tau serve` (Phase 1 §15) — design

**Date:** 2026-05-17
**Status:** Draft (pending user review)
**ADR:** Will require ADR-0031 (per Constitution QG18) — see [§9 ADR scope](#9-adr-scope).
**Scope:** Phase 1 priority §15. The second public API surface of tau (alongside the `tau-runtime` Rust crate).
**Roadmap entry:** [ROADMAP.md §15 Serve mode](../../../ROADMAP.md#tier-4--operational-quality).

## 1. Problem

Today the only way to drive tau is via the `tau` CLI. CLI output is a human UI (colors, progress bars, formatted text) — it is not a stable API. Spawning `tau run` from another program is brittle and pays the cold-start cost (parse tau.toml, resolve deps, spawn plugins, build capability shapes) per invocation.

Constitution G6 / QG12 commit to **two** public surfaces: the `tau-runtime` Rust crate AND a serve-mode IPC protocol. The latter has been deferred since Phase 0; the empty `tau-app` crate has been reserved for it. Phase 1 cannot close without it because:

- Phase 3+ SDKs (npm/pip/etc.) cannot exist without a protocol surface to wrap.
- IDE integrations (Cursor/Copilot-style agent invocation from editor extensions) have no way to embed tau.
- High-throughput consumers (CI loops, web app backends, batch agent execution) cannot amortize tau's startup cost.
- The `tau-runtime` Rust API only works for Rust callers; the majority of application code in 2026 is in other languages.

## 2. Goals (v1)

- Ship a versioned IPC protocol — JSON-RPC 2.0 over stdio — that exposes the runtime's two execution entry points (`Runtime::run`, `Runtime::run_streaming`).
- Treat the protocol as a SemVer-stable public surface (per QG12). Subsequent changes go through ADR amendments.
- Match the existing `tau run` mental model: one tau project per process, agents are resolved within that project.
- Be debuggable from a shell at any moment (`echo '…' | tau serve | jq`).
- Reuse the `RunEvent` shape canonicalized by ADR-0011 for streaming events.
- Be embeddable as a long-lived subprocess from any language with a JSON-RPC client and a way to spawn a child process.

## 3. Non-goals (v1)

These are intentionally deferred to v2+ via additive protocol amendments:

- **Session/persistence methods** (`session.start`, `session.resume`, `session.list`). Phase 1 §11 already exposes these via CLI; serve-mode coverage waits for a concrete embedding need.
- **Package management methods** (`pkg.install`, `pkg.resolve`, `pkg.list`, `pkg.verify`). External orchestrators that need these today can shell out to `tau install`/`tau resolve` as setup steps before `tau serve`.
- **Skill / workflow methods** (`skill.*`, `workflow.*`). Same reasoning.
- **HTTP transport.** Constitution NG3 (not a hosted service) and NG9 (no auth) make HTTP a poor fit. Subprocess-over-stdio is the lightest possible IPC. If HTTP is ever wanted, it lands in a downstream project.
- **MessagePack-RPC transport.** Considered and rejected — see [§7 Alternatives](#7-alternatives).
- **LSP-style Content-Length framing.** Considered and rejected for v1; additive `--transport lsp` is possible in a future ADR amendment if IDE-extension demand materializes.
- **Per-call project switching.** One Runtime per process. To switch projects, restart the process.
- **Discovery method (`rpc.discover`).** v1 ships with a versioned ADR-documented method list; clients consult the docs, not the server.
- **Partial results on batch errors.** `runtime.run` either returns a full `RunOutcome` or errors out.
- **Warnings as a separate channel.** If non-fatal warnings become useful, they land as a `RunEvent::Warning` variant in `tau-runtime::stream` and propagate automatically through the existing notification path.

## 4. Architecture

```
                            tau-app (NEW serve module)
                            ─────────────────────────────
                           ┌───────────────────────────┐
                           │ tau_app::serve            │
                           │                           │
                           │   ┌──────────────────┐   │
   stdin ──JSON-lines──►   │   │  reader task     │   │      ┌─────────────────┐
                           │   │  (line-split,    │   │      │ tau-runtime     │
                           │   │   serde_json)    │   │      │                 │
                           │   └────────┬─────────┘   │      │  Arc<Runtime>   │
                           │            ▼             │      │                 │
                           │   ┌──────────────────┐   │      │  ::run          │
                           │   │ dispatcher task  │───┼──────┤  ::run_streaming│
                           │   │ (per-message,    │   │      │                 │
                           │   │  spawn tokio     │   │      └─────────────────┘
                           │   │  task with       │   │
                           │   │  CancelToken)    │   │
                           │   └────────┬─────────┘   │
                           │            ▼             │
                           │   ┌──────────────────┐   │
   stdout ◄─JSON-lines──   │   │  writer task     │   │
                           │   │  (mpsc rx,       │   │
                           │   │   mutex on       │   │
                           │   │   stdout write)  │   │
                           │   └──────────────────┘   │
                           │                           │
   stderr ◄─tracing──      │   tracing subscriber      │
                           │   writes to stderr only;  │
                           │   stdout reserved for     │
                           │   protocol traffic.       │
                           └───────────────────────────┘
```

**Crate layout.** `tau-app` (currently a 5-line stub) gains a `serve` module. No new top-level crate.

**Runtime lifecycle.** One `Runtime` per process. Built via existing `RuntimeBuilder` at startup from `--project <path>` (default cwd). Plugins load once. Shared via `Arc<Runtime>` across concurrent RPC calls (Runtime was made `Arc`-shareable in PR #60 for recursive agent.spawn — this is the same shape).

**Async task layout.**

| Task | Responsibility |
|---|---|
| `reader` | `io::stdin().lines()` → mpsc channel of typed `Message`. Parse errors emit a `-32700` error response and continue. |
| `dispatcher` | Per inbound `Message`: validate handshake state, look up method, enforce concurrency cap, spawn a tokio task that handles the call. Records a `CancellationToken` per in-flight request in a `DashMap<RequestId, CancellationToken>`. |
| `writer` | Single mpsc receiver pulling outbound `Message`s; calls `io::stdout().write_all()` with a mutex so one JSON line is one atomic write. |

**CLI entrypoint.** New `tau serve` subcommand in `tau-cli` that calls into `tau_app::serve::run()`. Mirrors `tau run` argument-parsing shape.

## 5. Method surface

v1 ships **five methods** in two namespaces, plus **one server-initiated notification**.

```
meta.handshake           ⟶ ServerInfo            # first call, REQUIRED
meta.ping                ⟶ Pong                  # liveness

runtime.run              ⟶ RunOutcome            # batch (blocks until done)
runtime.run_streaming    ⟶ FinalResult           # streaming (emits notifications)
runtime.cancel           ⟶ Cancelled             # cancel an in-flight call

runtime.event             [notification]         # emitted during run_streaming
```

### 5.1 `meta.handshake`

Required as the first call after process start. Any non-`meta.*` call before successful handshake → `-32002 "Handshake required"`. Calling `meta.handshake` after a successful handshake → `-32003 "Already handshaken"`.

```jsonc
→ {"jsonrpc":"2.0","id":1,"method":"meta.handshake",
   "params":{"client_name":"my-app","client_version":"0.4.0",
             "protocol_version":1}}
← {"jsonrpc":"2.0","id":1,
   "result":{"server_name":"tau","server_version":"0.7.0",
             "protocol_version":1,
             "project_path":"/abs/path/to/project",
             "agents":["my-agent","helper"]}}
```

If `params.protocol_version` is not supported by the server → `-32000 "Handshake mismatch"` with `data.supported_versions: [1]`. v1 supports only `protocol_version: 1`.

### 5.2 `meta.ping`

```jsonc
→ {"jsonrpc":"2.0","id":2,"method":"meta.ping"}
← {"jsonrpc":"2.0","id":2,"result":{"ok":true}}
```

### 5.3 `runtime.run`

Batch execution. Blocks until the agent run completes or errors. Returns the same `RunOutcome` shape exposed by `tau-runtime::outcome::RunOutcome`.

```jsonc
→ {"jsonrpc":"2.0","id":3,"method":"runtime.run",
   "params":{"agent":"my-agent","prompt":"hello"}}
← {"jsonrpc":"2.0","id":3,
   "result":{"messages":[{"role":"assistant","content":"hi"}],
             "token_usage":{"prompt":12,"completion":3},
             "stop_reason":"end_turn"}}
```

**Params schema:**

```
agent:   string             — agent id within the project
prompt:  string             — user message
options: object (optional)  — additional opts, see §5.6
```

### 5.4 `runtime.run_streaming`

Streaming execution. Server emits 0..N `runtime.event` notifications with `params.id` matching the request id, then a final response.

```jsonc
→ {"jsonrpc":"2.0","id":4,"method":"runtime.run_streaming",
   "params":{"agent":"my-agent","prompt":"go"}}

← {"jsonrpc":"2.0","method":"runtime.event",
   "params":{"id":4,"kind":"TextDelta","data":{"text":"go "}}}
← {"jsonrpc":"2.0","method":"runtime.event",
   "params":{"id":4,"kind":"ToolCallStarted","data":{"tool":"fs-read","args":{…}}}}
← {"jsonrpc":"2.0","method":"runtime.event",
   "params":{"id":4,"kind":"ToolCallCompleted","data":{"tool":"fs-read","result":{…}}}}
← {"jsonrpc":"2.0","method":"runtime.event",
   "params":{"id":4,"kind":"TurnCompleted","data":{…}}}
← {"jsonrpc":"2.0","id":4,
   "result":{"final":true,"token_usage":{…},"stop_reason":"end_turn"}}
```

**`runtime.event.params.kind`** maps directly to `tau_runtime::stream::RunEvent` variants from ADR-0011:

```
TextDelta            data: {text: string}
ToolCallStarted      data: {tool: string, args: object, call_id: string}
ToolCallCompleted    data: {tool: string, result: object, call_id: string}
TurnCompleted        data: {turn: int, stop_reason: string}
RunCompleted         data: {token_usage: object}
FatalError           data: {tool_error_variant: string, message: string, …}
```

The serve module is a thin translator: each `RunEvent` becomes one `runtime.event` notification, the final `RunCompleted` triggers the success response, and `FatalError` triggers an error response per §6.

### 5.5 `runtime.cancel`

Cancel an in-flight request by id. Cooperative — in-flight LLM HTTP requests and plugin subprocess calls run to completion (their next async await point), but no further `RunEvent`s are emitted.

```jsonc
→ {"jsonrpc":"2.0","id":5,"method":"runtime.cancel","params":{"id":4}}
← {"jsonrpc":"2.0","id":5,"result":{"cancelled":true}}
// The cancelled call gets:
← {"jsonrpc":"2.0","id":4,"error":{"code":-32001,"message":"Cancelled by client"}}
```

Cancelling an unknown id (already completed, never existed) → `{cancelled: false}`, NOT an error. Idempotent.

### 5.6 Reserved `params.options`

`runtime.run` and `runtime.run_streaming` accept an optional `options` object. v1 recognizes:

```
project_override:           string|null     — per-call CapabilityOverride source
                                              (mirrors RunOptions.project_override)
```

Unknown option keys are tolerated (forward-compat). Future ADRs add fields here.

## 6. Error model

Standard JSON-RPC 2.0 errors plus a tau-namespaced custom range in the Server error band (`-32000` to `-32099`).

| Code | Name | When |
|---|---|---|
| `-32700` | Parse error | Invalid JSON on a line |
| `-32600` | Invalid Request | Not a valid JSON-RPC 2.0 object |
| `-32601` | Method not found | Unknown method name |
| `-32602` | Invalid params | Wrong shape/types for the called method |
| `-32603` | Internal error | Unrecoverable server bug |
| `-32000` | Handshake mismatch | `protocol_version` not supported |
| `-32001` | Cancelled | Request cancelled by client |
| `-32002` | Handshake required | Non-`meta.*` call before handshake |
| `-32003` | Already handshaken | `meta.handshake` called twice |
| `-32004` | Server busy | `max_concurrent_runs` reached |
| `-32005` | Project error | Build failed (BuildError variants — future surface) |
| `-32006` | Runtime error | `Runtime::run*` returned an error |
| `-32007` | Capability denied | `RuntimeError::CapabilityDenied` |
| `-32008` | Tool error | Tool plugin returned error |
| `-32009` | LLM error | Backend plugin returned error |
| `-32010` | Unknown agent | `agent_id` not in this project |
| `-32011`..`-32099` | Reserved | Future serve-mode errors |

### 6.1 Error response shape

```jsonc
{"jsonrpc":"2.0","id":3,
 "error":{
   "code":-32007,
   "message":"Capability denied: fs.read.paths does not allow /etc/shadow",
   "data":{
     "kind":"CapabilityDenial",
     "denial":{
       "capability":"fs.read",
       "tool_id":"fs-read",
       "agent_id":"my-agent",
       "denied_path":"/etc/shadow"
     },
     "tool_error_variant":"CapabilityDenied"
   }
 }}
```

### 6.2 Mapping rules

- `RuntimeError` variants → specific custom codes per the table; original Rust error's structured fields carried in `error.data`.
- `BuildError` variants → exit with code 64 at startup (no protocol yet). Post-startup invocations (if a future `runtime.reload` is added) map to `-32005`.
- `error.message` is human-readable, stable enough to grep but not part of the SemVer contract.
- `error.data` is the machine-actionable payload. Schema is stable within the protocol version; each `code` has a known `data` shape documented alongside this spec.
- Mid-stream `RunEvent::FatalError` terminates the streaming call: server stops emitting notifications and replies with an error response using the appropriate code. Preserves ADR-0011's byte-identical batch-vs-streaming error semantics via `tool_error_variant` tagging.

## 7. Alternatives considered

| Choice | Why rejected |
|---|---|
| **JSON-RPC + LSP-style framing** (Content-Length headers) | More complex parser, harder to test manually with shell pipes. NDJSON has identical IDE-integration cost for the moment (no current IDE-extension consumer). Additive opt-in remains possible later. |
| **MessagePack-RPC** | Diverges from Constitution wording ("JSON-RPC over stdio"); requires Constitution amendment. External SDK clients in every language would need msgpack libs. Loses the operational property that anyone can debug the protocol with `cat`/`jq`. Plugin-host's msgpack is an *internal* protocol; serve mode is *external* — different ergonomic requirements. |
| **HTTP transport** | Tau is not a hosted service (NG3) and explicitly does no auth (NG9). Subprocess-over-stdio is the lightest possible IPC (zero ports, zero auth surface, parent-bound lifetime). HTTP would add attack surface that the design rejects on principle. |
| **Per-call project switching** (Runtime built per request OR explicit `runtime.attach`) | RuntimeBuilder is expensive (tau.toml parse, dep resolution, plugin spawn, capability shape build). Building per call would defeat the cold-start-amortization that is one of the two main motivations for serve mode. Multiple-runtime-handles model defers all the hard lifecycle bugs to v2 if real demand appears. |
| **Discovery via `rpc.discover`** | Adds a v1 method that locks an introspection contract forever. Versioned ADR-documented method list is sufficient. |
| **Full CLI parity** (every subcommand exposed) | ~15-method surface locked in forever. SemVer commitment is much heavier. Forces premature decisions about session/skill/pkg method shapes. v1 ships the kernel; later ADRs add increments as real consumers appear. |

## 8. Concurrency & lifecycle

### 8.1 Concurrency

- Parallel by default. Each request runs in its own tokio task. Tasks share `Arc<Runtime>`.
- Concurrent `runtime.run_streaming` calls interleave their `runtime.event` notifications on the wire; clients demultiplex by `params.id`.
- Writer mutex around `stdout.write_all()` guarantees no partial-line interleaving.
- Bounded: default **8 concurrent runs per process**. New requests beyond cap → `-32004 "Server busy"` immediately (no queueing). Configurable via `tau serve --max-concurrent <N>`. Settable to 1 for serial mode.

### 8.2 Cancellation

- Per in-flight request: `CancellationToken` (tokio-util) in `DashMap<RequestId, CancellationToken>`.
- `runtime.cancel` looks up the token, calls `.cancel()`, removes from map.
- Runtime call uses `tokio::select!` to race `Runtime::run_streaming` against `cancel_token.cancelled()`. Cancellation winning → drops the tokio task, emits `-32001`.
- Idempotent on unknown ids (returns `{cancelled: false}`).

### 8.3 Startup

```
tau serve [--project <path>] [--max-concurrent <N>] [--idle-timeout <secs>]
          [--ready-on-stderr] [--shutdown-grace <secs>]

1. Parse CLI flags (clap).
2. Resolve --project: explicit > $TAU_PROJECT > cwd.
3. RuntimeBuilder::from_project(path).build().await
   ├─ ok  → continue
   └─ err → write BuildError JSON to stderr, exit code 64.
4. Start reader/dispatch/writer tasks.
5. If --ready-on-stderr: write "tau-serve ready\n" to stderr.
6. Block on dispatch task until shutdown.
```

Runtime is built **before any RPC traffic**, including `meta.handshake`. The handshake's `result.agents` reflects the already-built runtime's known agents.

### 8.4 Shutdown

| Signal / event | Behavior |
|---|---|
| **stdin EOF** | Begin graceful shutdown. |
| **SIGTERM** | Begin graceful shutdown. |
| **SIGINT** (Ctrl-C if attached) | Begin graceful shutdown. |
| **SIGKILL** | Immediate. Plugins orphaned (parent's problem). |
| **SIGHUP** | Ignored in v1. |
| **Parent process death** | Linux: `prctl(PR_SET_PDEATHSIG, SIGTERM)` at startup. macOS: rely on stdin EOF (which happens when parent's stdout side closes). |

**Graceful shutdown sequence:**

1. Stop accepting new requests (reader closes outbound channel).
2. Drain `DashMap<id, CancellationToken>`, call `.cancel()` on each.
3. Wait up to `--shutdown-grace <secs>` (default 5s) for in-flight tasks to emit final error responses.
4. If grace expires: drop the runtime, kill plugin subprocesses (existing `kill_on_drop` tokio setup), exit 0.
5. If all in-flight finish within grace: clean exit 0.

stdout is flushed before exit.

### 8.5 Idle timeout

- Default: none.
- `--idle-timeout <secs>` arms a timer reset on every incoming message; firing initiates graceful shutdown as if SIGTERM arrived.

### 8.6 Exit codes (`sysexits.h` convention)

```
0   Clean shutdown
64  Usage / startup config error (bad --project, build failed)
70  Internal error (panic somewhere unhandled)
130 Killed by signal (SIGINT/SIGTERM)
```

### 8.7 Logging

- `tracing` subscriber → stderr.
- `RUST_LOG` env honored (workspace standard).
- Suggested defaults: INFO (startup, shutdown, ready), DEBUG (per-request), WARN (parse errors).

## 9. ADR scope

This spec requires **ADR-0031** per Constitution QG18 ("Changes to the serve-mode protocol require ADRs"). The ADR covers:

- Decision: ship serve mode v1 as JSON-RPC 2.0 over NDJSON-framed stdio with the method surface in §5.
- Status: Accepted (assuming this spec is approved).
- Context: Phase 1 §15; Constitution G6/QG12.
- Consequences:
  - 14 → 15 (or 16) required CI checks (depending on whether the new `test (tau-app serve / linux)` job is folded or separate).
  - `tau-app` crate exits stub status.
  - The protocol surface is permanently versioned. Future additive methods land via ADR amendments.
  - Phase 1 closes when this ships; Phase 2 (tau as a compiled language for agentic workflows) becomes the active phase.

## 10. Testing strategy

Four layers, fast-to-slow:

### Layer 1 — Unit tests (~30 tests)

In `crates/tau-app/src/serve/*.rs`. Per-module:

| Module | Coverage |
|---|---|
| `serve::framing` | Parse valid requests; reject malformed JSON (`-32700`); reject non-2.0 (`-32600`); roundtrip request/response/notification. |
| `serve::dispatch` | Method routing; unknown method (`-32601`); pre-handshake (`-32002`); double handshake (`-32003`); max_concurrent (`-32004`). |
| `serve::cancel` | Token registry insert/lookup/cancel/no-op on unknown id/removal after completion. |
| `serve::error_map` | Each `RuntimeError` variant → correct code + `data` shape. `insta` snapshot tests. |
| `serve::lifecycle` | Idle timer reset; graceful drain; exit-code mapping. |

### Layer 2 — In-process integration (~25 tests)

In `crates/tau-app/tests/serve_*.rs`. Drive serve via `tokio::io::duplex` without spawning a real subprocess. Use `MockLlmBackend` (PR #78 fixture).

| File | Scenario |
|---|---|
| `serve_handshake.rs` | Happy / mismatch / pre-handshake / double-handshake. |
| `serve_run_batch.rs` | `runtime.run` happy path; each error mapping. |
| `serve_run_streaming.rs` | Events carry the right `params.id`; multiple concurrent streams demuxed correctly. |
| `serve_cancel.rs` | Mid-stream cancel → `-32001`; cancel after completion / unknown id → `{cancelled:false}`. |
| `serve_concurrent.rs` | 8 in-flight, 9th gets `-32004`; resubmit succeeds. |
| `serve_shutdown.rs` | stdin EOF / SIGTERM drains in-flight within grace; grace timeout drops runtime. |

### Layer 3 — End-to-end subprocess (~10 tests)

In `crates/tau-app/tests/e2e/`. Spawn built `tau` binary, JSON-RPC over its stdio.

| File | Scenario |
|---|---|
| `e2e_smoke.rs` | Spawn `tau serve --project <fixture>`; handshake → run → response → shutdown. |
| `e2e_streaming.rs` | Same but `run_streaming`; notifications in order on the pipe. |
| `e2e_parent_death.rs` | Kill parent; child exits within 1s (PDEATHSIG/stdin-EOF). |
| `e2e_ready_signal.rs` | `--ready-on-stderr` writes `tau-serve ready\n` before any RPC. |

Fixtures: minimal tau.toml projects under `crates/tau-app/tests/fixtures/` using existing `echo-llm` + `echo-tool` toy plugins. No real LLM or plugin builds in CI.

### Layer 4 — Conformance (deferred)

Not in v1. Future ADR-versioned protocol bumps add conformance tests via the `tau-plugin-conformance/`-style pattern (rule-of-three refactor from sub-project 2c).

### CI integration

- New required check: **`test (tau-app serve / linux)`** — runs Layer 2 + Layer 3.
- Layer 1 tests run inside existing `test-stable / {linux,macos,windows}` matrix.
- Net: +1 required check (14 → 15 gating `main`).

## 11. Risks & tradeoffs

- **Permanent versioning commitment.** Once shipped, the v1 method surface is permanent. Mitigated by deliberately tiny surface (5 methods) and reserving `error.code` space for future namespaces.
- **stdout discipline.** Any accidental `println!` anywhere in tau code reachable from a serve-mode process corrupts the protocol. Mitigation: clippy lint forbidding `println!` / `eprint!` / `print!` outside `tau-cli`'s output paths; tracing subscriber configured early.
- **Concurrency surprises in plugin host.** Plugin subprocesses today are 1-call-at-a-time. Concurrent serve-mode requests will exercise the plugin host's concurrency story for the first time. Mitigation: existing tokio-mutex inside `plugin_host` already serializes per-plugin calls; concurrent runs share plugins safely.
- **Parent-death handling differs by OS.** Linux PDEATHSIG is reliable; macOS relies on stdin EOF; Windows is a future concern (no equivalent native mechanism — likely a heartbeat-from-parent pattern when Windows support lands).
- **Idle timeout interacts with in-flight runs.** If an idle-timed-out server has a long in-flight run, the timer should NOT fire while a run is in progress. Decision: timer resets on every incoming OR outgoing message, so in-flight runs hold it open. Document this.

## 12. Open implementation questions

- Exact crate for JSON-RPC: hand-roll (small, no dep) vs `jsonrpsee` (~150KB compiled, full-featured, supports HTTP/WS we don't need). Decision deferred to plan-writing — likely hand-roll given v1's small surface.
- Exact crate for `CancellationToken`: `tokio-util` already in the workspace via sandbox crates. Reuse.
- Whether `tau-cli`'s existing `--project` resolution helpers can be lifted into `tau-app` or whether to copy. Decision deferred; both crates compile against the same `tau-runtime` API.
- Whether `tau serve` should call the existing `tau-cli` clap subcommand harness or stand alone. Decision: stand alone (it's a different argument surface, different output discipline).

## 13. Out of scope for THIS SPEC (deferred to future ADR amendments)

- Sessions / persistence methods
- Package management methods
- Skill / workflow methods
- LSP-style framing transport
- MessagePack-RPC transport
- HTTP transport
- WebSocket transport
- TCP socket transport (already excluded by the subprocess-stdio architectural choice)
- `rpc.discover` method
- Streaming `runtime.run` with partial results
- Warnings as a separate channel (folds into `RunEvent::Warning` variant if added)
- Authentication (forever-deferred per NG9)
