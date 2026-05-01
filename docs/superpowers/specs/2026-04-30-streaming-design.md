# Streaming LLM Responses — Design Spec

**Date:** 2026-04-30
**Status:** Approved (pending user review of this written spec)
**Sub-project:** Tier 2 priority 8 (per ROADMAP `Tier 2 — completes Phase 0 deferrals`).
**Closes deferral:** ADR-0006 §5 — `LlmBackend::stream` exists in tau-ports
but tau-runtime does not invoke it at v0.1; streaming integration is
"purely additive (a new `Runtime::run_streaming` method)."

---

## 1. Summary

Add `Runtime::run_streaming` (and `run_streaming_with_history`) — a
new kernel API that yields a `Stream<Item = RunEvent>` as the agent
loop progresses. Callers see text deltas as the LLM types, tool calls
as the LLM commits to them, and tool results as dispatch finishes,
instead of receiving the entire `RunOutcome` after the run completes.

`Runtime::run` / `run_with_history` (the existing batch entry points)
keep their current API. They become thin consumers of the new stream
under the hood — the agent-loop body factors into a single async
generator, and the batch entry points just drain the stream and return
the terminal `RunCompleted.outcome`.

CLI integrations ship in the same sub-project:
- `tau chat`: streaming-on-by-default; raw `print!` typewriter rendering
  during the stream, termimad re-render once the run completes for
  proper markdown formatting. `--no-stream` flag preserves the
  existing batch-render behavior.
- `tau run --stream`: opt-in flag for one-shot runs. Human mode prints
  text deltas to stdout + bracketed tool annotations to stderr; JSON
  mode emits one `RunEvent` per stdout line.

This sub-project closes ADR-0006 §5 and lights up the existing
`LlmBackend::stream` infrastructure that all three shipped LLM-backend
plugins (anthropic, ollama, openai) already implement.

---

## 2. Background and motivation

ADR-0006 §5 is the canonical deferral:

> `LlmBackend::complete` (batch) is the v0.1 surface used by the agent
> loop. `LlmBackend::stream` exists in tau-ports (per ADR-0003 §2) but
> tau-runtime does not invoke it at v0.1. Streaming integration is
> purely additive (a new `Runtime::run_streaming` method) and does not
> break the batch surface when it lands.
>
> Trigger to revisit: a streaming-UX-driven use case (TUI rendering
> tokens as they arrive, latency-sensitive interactive flows).

Today's `tau chat` REPL has the trigger condition: users wait for the
entire assistant turn before any output appears, even though the
underlying LLM is emitting tokens incrementally. This is bad UX for
long generations and worse for tool-using agents (the user can't see
what the agent is "thinking" or which tools it's about to call).

The plumbing on both ends is already in place:
- `LlmBackend::stream` returns `CompletionStream` (a
  `Pin<Box<dyn Stream<Item = Result<CompletionChunk, LlmError>> + Send>>`).
- `CompletionChunk` has three variants: `Text { delta }`,
  `ToolUse(ToolUse)` (the plugin pre-assembles tool_uses; the runtime
  never sees partials), and `Finish { stop_reason, usage }`.
- All three LLM-backend plugins (anthropic, ollama, openai) implement
  `stream()` natively per their priority 2a/2b/2c spec lock-in.
- IPC plugins are wired via `IpcLlmBackend::stream` and the
  `plugin_host::stream_router::assemble` adapter.

What's missing is the kernel-side consumer: the agent loop in `run.rs`
calls `backend.complete(request).await?` (line 254-255) and never
exercises `stream()`. This sub-project fills that gap.

---

## 3. Decisions table

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| Q1 | API surface | **A** — `Stream<Item = RunEvent>` returned from `Runtime::run_streaming`; pull-based async iterator | Composes with the broader Rust async ecosystem; matches existing `LlmBackend::stream()` shape; pull-based backpressure falls out naturally |
| Q2 | Event vocabulary | **B** — higher-level kernel events (`TextDelta`, `ToolCallStarted`, `ToolCallCompleted`, `TurnCompleted`, `RunCompleted`); NOT raw `CompletionChunk` passthrough | UI consumers shouldn't need to learn LLM-protocol semantics; the kernel IS the layer that knows what a tau agent run looks like |
| Q3 | `ToolCallStarted` timing | **A** — fire IMMEDIATELY on LLM emission (when kernel sees `CompletionChunk::ToolUse`) | Costs nothing at the kernel; gives REPL a measurable UX win (user sees tool intent the moment the LLM commits to it, not 200-500ms later after turn completion) |
| Q4 | Error semantics | **A** — stream items are pure `RunEvent`; mid-stream errors flow through `RunOutcome::Failed` and a terminal `RunCompleted` event; construction-time errors return `Err(RuntimeError)` from the `.await?` call | Single error path; uniform consumer loop (`while let Some(event) = stream.next()`); avoids "did the stream end because it succeeded or because it errored" ambiguity |
| Q5 | CLI scope | **A** — full scope: kernel + `tau chat` REPL streaming (default-on, `--no-stream` opt-out) + `tau run --stream` flag (human + JSON modes) | Streaming's product value is human-facing latency; bundling both UX surfaces (REPL + script) lands the API + its consumers in one cycle |

---

## 4. Architecture

### 4.1 Module layout

A new module `crates/tau-runtime/src/stream.rs` owns:
- The public `RunEvent` enum (re-exported at the crate root).
- `run_streaming_inner` — the async-generator (built via
  `async_stream::stream!`) that drives the agent loop and yields
  `RunEvent`s. ~250 LOC.

`Runtime` (in `builder.rs`) gains two new entry points:

```rust
impl Runtime {
    pub async fn run_streaming(
        &self,
        agent_def: AgentDefinition,
        package_manifest: PackageManifest,
        initial_message: Message,
        options: RunOptions,
    ) -> Result<impl Stream<Item = RunEvent> + Send + '_, RuntimeError>;

    pub async fn run_streaming_with_history(
        &self,
        agent_def: AgentDefinition,
        package_manifest: PackageManifest,
        history: Vec<Message>,
        initial_message: Message,
        options: RunOptions,
    ) -> Result<impl Stream<Item = RunEvent> + Send + '_, RuntimeError>;
```

Both are thin wrappers that perform the same setup as the existing
batch entries (resolve LLM backend, capability filtering, override
`compute_effective`, etc.) and then delegate to `run_streaming_inner`.

The existing `Runtime::run_with_history` becomes a thin consumer of
`run_streaming_inner` — drains the stream, ignores all events except
the terminal `RunCompleted`, returns its `outcome`. This eliminates
duplication between batch and streaming paths.

`async-stream = "0.3"` lands as a workspace dep.

### 4.2 `RunEvent` enum

```rust
/// Streaming event from `Runtime::run_streaming`. Always terminates
/// with exactly one `RunCompleted`; intermediate events are unbounded
/// per agent run.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum RunEvent {
    /// LLM emitted a text fragment. Concatenate with previous deltas
    /// for the running assistant message text.
    TextDelta {
        /// Text fragment to append.
        delta: String,
    },

    /// LLM emitted a complete `tool_use` block. Fires immediately when
    /// the kernel sees `CompletionChunk::ToolUse` — BEFORE the tool is
    /// dispatched. Display intent: "agent wants to call X with args Y".
    /// The matching `ToolCallCompleted` fires after dispatch finishes.
    ToolCallStarted {
        /// Provider-supplied tool-use id; correlates with
        /// `ToolCallCompleted.id`.
        id: String,
        /// Tool name.
        name: String,
        /// Args the LLM emitted.
        args: tau_domain::Value,
    },

    /// Tool dispatch finished. Fires after `Tool::invoke` returns,
    /// regardless of success/failure. Carries the tool result OR a
    /// validation/dispatch error message — same payload shape that
    /// gets written to the conversation as a `MessagePayload`.
    ToolCallCompleted {
        /// Matches the `id` from `ToolCallStarted`.
        id: String,
        /// Tool name.
        name: String,
        /// The outcome envelope: `Ok(ToolResult)` on success;
        /// `Err(reason)` for validation failures or other recoverable
        /// errors. Plugin-crash-class errors don't surface here —
        /// they terminate the run via `RunCompleted`.
        result: Result<tau_ports::ToolResult, String>,
    },

    /// One turn of the agent loop completed. The LLM's `Finish`
    /// chunk arrived AND any tool calls within the turn finished
    /// dispatching. Carries the LLM's reported stop reason and
    /// token usage.
    TurnCompleted {
        stop_reason: tau_ports::StopReason,
        usage: Option<tau_ports::TokenUsage>,
        turn: u32,  // 1-indexed
    },

    /// Terminal event. Always exactly one per stream. After this
    /// fires, the stream returns `None`. Carries the final
    /// `RunOutcome` (success or any failure mode).
    RunCompleted {
        outcome: RunOutcome,
    },
}
```

**Naming:** `ToolCallStarted` / `ToolCallCompleted` follow the
existing `MessagePayload::ToolCall` / `ToolResult` vocabulary. The
LLM-protocol layer says `tool_use`; the kernel layer says `tool_call`.

### 4.3 Kernel pump translation

`run_streaming_inner` is structured as one async-generator function
that mirrors the existing `run_with_history` flow. Per turn:

1. **Build `CompletionRequest`** (same as today's `run.rs:235-244`).
2. **Open the LLM stream:** `backend.stream(request).await?`.
   - On `Err(LlmError)`: yield `RunEvent::RunCompleted { outcome: Failed { kind: LlmError } }` and return.
3. **Drain the LLM stream into events + accumulator state:**
   ```rust
   while let Some(chunk) = llm_stream.next().await {
       match chunk {
           Ok(Text { delta }) => {
               accumulated_text.push_str(&delta);
               yield RunEvent::TextDelta { delta };
           }
           Ok(ToolUse(tu)) => {
               // Q3-A: fire ToolCallStarted IMMEDIATELY.
               yield RunEvent::ToolCallStarted {
                   id: tu.id.clone(), name: tu.name.clone(), args: tu.input.clone()
               };
               pending_tool_uses.push(tu);
           }
           Ok(Finish { stop_reason, usage }) => {
               turn_stop_reason = Some(stop_reason);
               turn_usage = usage;
               break;
           }
           Err(llm_err) => {
               yield RunEvent::RunCompleted { outcome: Failed { kind: LlmError { source: llm_err } } };
               return;
           }
       }
   }
   ```
4. **Append assistant message** to the conversation buffer (same logic
   as today's `run.rs:280-294`).
5. **If no tool_uses:** yield `TurnCompleted` then `RunCompleted` with
   the existing `RunOutcome::Completed` fields. Return.
6. **Per-tool dispatch** (same as today's `run.rs:340-490`): capability
   check (priority 4 carryover), schema validation (priority 6
   carryover), session open (`Tool::init`), invoke, session close
   (`Tool::teardown`).
   - **On `Tool::invoke` success:** append `MessagePayload::ToolResult`
     to messages; yield `RunEvent::ToolCallCompleted { result: Ok(...) }`.
   - **On schema-validation `BadArgs`:** append
     `MessagePayload::ToolError` (kind `tool_args_validation` from
     priority 6); yield
     `RunEvent::ToolCallCompleted { result: Err(reason) }`.
   - **On real plugin crash:** best-effort teardown; yield
     `RunCompleted { outcome: Failed { kind: ToolError } }` and return.
   - **On capability denial:** yield
     `RunCompleted { outcome: Failed { kind: PolicyDenied, .. } }` and return.
7. **End of turn:** yield `RunEvent::TurnCompleted { stop_reason, usage, turn }`. Loop.
8. **`max_turns` reached:** yield
   `RunCompleted { outcome: Failed { kind: OutOfResources, .. } }`.

**Invariants the pump enforces:**
- Exactly one `RunCompleted` per stream, regardless of exit path.
- For every `ToolCallStarted` in the stream, EITHER a matching
  `ToolCallCompleted` (with same `id`) follows before the next
  `TurnCompleted`, OR the next event is `RunCompleted { outcome:
  Failed }` indicating mid-dispatch terminal failure (e.g., plugin
  crash during `Tool::invoke`). The "no matching Completed" case is
  a documented terminal-failure signal, not a stream bug.
- `TurnCompleted` arrives only after the turn's LLM `Finish` AND all
  the turn's tool dispatches resolved.
- Stream ordering preserves LLM source order — kernel never reorders.

### 4.4 Batch wrapper

`Runtime::run_with_history` becomes:

```rust
pub async fn run_with_history(
    &self,
    agent_def: AgentDefinition,
    package_manifest: PackageManifest,
    history: Vec<Message>,
    initial_message: Message,
    options: RunOptions,
) -> Result<RunOutcome, RuntimeError> {
    let mut stream = self.run_streaming_with_history(
        agent_def, package_manifest, history, initial_message, options,
    ).await?;
    while let Some(event) = stream.next().await {
        if let RunEvent::RunCompleted { outcome } = event {
            return Ok(outcome);
        }
    }
    // Pump invariant guarantees we never reach here.
    unreachable!("run_streaming_inner must yield exactly one RunCompleted before stream end")
}
```

`Runtime::run` (single-shot) is unchanged in signature but its body
delegates similarly.

This is a behavior-preserving refactor with one subtle difference:
the batch wrapper's outcome is identical to today's, but the per-event
visibility (which today's batch wrapper has no use for) flows through
the new pump. Batch tests pin the unchanged outcome.

### 4.5 CLI: `tau chat` streaming

Today's REPL turn handler calls `runtime.run_with_history(...).await?`,
collects the final assistant `Message`, renders via termimad, then
loops. The new flow consumes a stream:

```rust
let mut stream = runtime.run_streaming_with_history(...).await?;
let mut accumulated_text = String::new();
let mut final_outcome: Option<RunOutcome> = None;

while let Some(event) = stream.next().await {
    match event {
        RunEvent::TextDelta { delta } => {
            // Raw print + flush — termimad can't render mid-paragraph.
            print!("{delta}");
            std::io::stdout().flush().ok();
            accumulated_text.push_str(&delta);
        }
        RunEvent::ToolCallStarted { name, args, .. } => {
            println!("\n→ {name}({})", preview_args(&args));
        }
        RunEvent::ToolCallCompleted { name, result, .. } => match result {
            Ok(_) => println!("← {name} ok"),
            Err(reason) => println!("← {name} error: {}", first_line(&reason)),
        },
        RunEvent::TurnCompleted { .. } => { /* silent */ }
        RunEvent::RunCompleted { outcome } => {
            final_outcome = Some(outcome);
            break;
        }
    }
}

// Two-pass rendering: re-render with termimad once the run completes.
println!();
if matches!(&final_outcome, Some(RunOutcome::Completed { .. })) {
    render_via_termimad(&accumulated_text);
}
```

**Two-pass rendering:** the streaming pass shows raw text immediately
(typewriter UX). After the run completes, we re-render the full text
via termimad for proper markdown (headers, code blocks, lists). This
sidesteps the hard problem of "render markdown incrementally."

**`--no-stream` flag** added to `ChatArgs`. Defaults to `false`
(streaming on). Setting `--no-stream` reverts to today's batch flow:
single termimad render after the full assistant turn completes.

### 4.6 CLI: `tau run --stream`

New flag on `RunArgs`. Default `false` (preserves the existing
one-shot behavior).

**Human mode (`--stream` without `--json`):**
- `TextDelta` → `print!` to stdout, flushed immediately.
- `ToolCallStarted` → `eprintln!` `→ {name}({preview})` to stderr (so
  stdout stays the agent's text).
- `ToolCallCompleted` → `eprintln!` `← {name} ok` or
  `← {name} error: ...` to stderr.
- `TurnCompleted` → silent.
- `RunCompleted` → trailing newline; emit any closing summary; return
  outcome via existing exit-code translation.

**JSON mode (`--stream --json`):**
- One JSON object per line via the existing `Output::json` helper.
  Same per-line precedent as priorities 5 + 6.
- Event shape (canonical):
  ```json
  {"event":"text_delta","delta":"Hello "}
  {"event":"tool_call_started","id":"call_1","name":"fs-read","args":{"path":"/etc/foo"}}
  {"event":"tool_call_completed","id":"call_1","name":"fs-read","result":{"ok":{"contents":"...","size":42}}}
  {"event":"turn_completed","stop_reason":"ToolUse","usage":{"input_tokens":150,"output_tokens":12},"turn":1}
  {"event":"run_completed","outcome":{"completed":{...}}}
  ```

**Sequencing with priority 5's resolve flow:** `tau run --stream` first
runs the existing `requires.tools` resolve (with its npm-style
progress). Streaming only covers the agent loop, not dep install.

---

## 5. Type changes

### 5.1 New types in tau-runtime

- `pub enum RunEvent` (5 variants, `#[non_exhaustive]`). Re-exported
  from the crate root.
- `pub(crate) async fn run_streaming_inner(...)` — kernel-internal
  generator.

### 5.2 New methods on `Runtime`

- `pub async fn run_streaming(...) -> Result<impl Stream<Item = RunEvent> + Send + '_, RuntimeError>`
- `pub async fn run_streaming_with_history(...) -> Result<impl Stream<Item = RunEvent> + Send + '_, RuntimeError>`

### 5.3 Modified entries

- `Runtime::run_with_history` body collapses to a thin stream-drainer.
  Public API signature unchanged (still returns
  `Result<RunOutcome, RuntimeError>`).
- `RunArgs.stream: bool` (clap-derived, default false; `--stream`
  is opt-in for `tau run` to preserve the existing one-shot script
  behavior).
- `ChatArgs.no_stream: bool` (clap-derived, default false ⇒ streaming
  on; `--no-stream` is opt-out for `tau chat` because the typewriter
  UX is the dominant chat-REPL win).
- The polarity asymmetry is intentional: each subcommand's default
  matches its dominant use case. `tau run` is script-friendly by
  default; `tau chat` is human-friendly by default.

### 5.4 No tau-ports changes

The `LlmBackend::stream` trait method already exists. The
`CompletionStream` / `CompletionChunk` types are stable. No new
errors; no new variants.

### 5.5 No new error variants

Mid-stream failures become `RunOutcome::Failed { kind: ... }` via
existing variants (`PolicyDenied`, `OutOfResources`, `LlmError`,
`Internal`). Construction-time failures continue to return
`RuntimeError` via existing variants.

---

## 6. ADR-0011 — Streaming LLM responses

A new ADR lands as part of this sub-project. Mirrors the structure of
ADR-0009 + ADR-0010. Sections:

1. **API surface: `Stream<Item = RunEvent>`** (Q1-A) — pull-based
   async iterator; composes with the broader Rust ecosystem.
2. **Event vocabulary: kernel-translated `RunEvent`** (Q2-B) — UI
   consumers shouldn't learn LLM-protocol semantics.
3. **`ToolCallStarted` fires on LLM emission** (Q3-A) — display intent
   before dispatch; UX latency win.
4. **Pure `RunEvent` items; errors flow through `RunOutcome::Failed`**
   (Q4-A) — single error path; uniform consumer loop.
5. **Full CLI scope: kernel + chat (default-on) + run --stream**
   (Q5-A) — bundle the UX surfaces with the API.

Pump invariants documented:
- Exactly one `RunCompleted` per stream.
- `ToolCallStarted` paired with exactly one `ToolCallCompleted`
  (matching `id`), with the documented exception of mid-dispatch run
  termination.
- Pull-based backpressure (no buffering).
- Batch (`run`/`run_with_history`) factored as thin wrappers — same
  pump, two presentation layers.

Trigger to revisit: parallel tool dispatch, mid-stream markdown
rendering, async cancellation, mid-stream usage updates, SSE export
for `tau serve` (none of which exist at v0.1).

---

## 7. Testing

| Tier | Coverage | Where |
|------|----------|-------|
| Unit | Happy path: scripted LLM emits `Text → Finish` → kernel yields `TextDelta → TurnCompleted → RunCompleted` | `crates/tau-runtime/src/stream.rs::tests` |
| Unit | Tool dispatch flow: `Text → ToolUse → Finish` → `TextDelta → ToolCallStarted → (turn-end dispatch) → ToolCallCompleted → TurnCompleted` | `stream.rs::tests` |
| Unit | LlmError mid-stream → `RunCompleted { outcome: Failed { kind: LlmError } }` | `stream.rs::tests` |
| Unit | Plugin crash on invoke → `RunCompleted { outcome: Failed { kind: ToolError } }` | `stream.rs::tests` |
| Unit | max_turns reached → `RunCompleted { outcome: Failed { kind: OutOfResources } }` | `stream.rs::tests` |
| Unit | Capability denial → `RunCompleted { outcome: Failed { kind: PolicyDenied } }` | `stream.rs::tests` |
| Unit | Schema validation failure → `ToolCallCompleted { result: Err(reason) }` containing the MANDATORY-rule template; loop continues | `stream.rs::tests` |
| Unit | Batch-wrapper invariant: `run_with_history` consumes the stream and returns the same `RunOutcome` as before for a fixed scripted scenario (pin-to-prevent-regression test) | `stream.rs::tests` |
| Integration | E2E via the in-process FsReadPlugin adapter — full pipeline streaming | `crates/tau-runtime/tests/run_streaming_e2e.rs` (~5 scenarios, gated `#![cfg(unix)]`) |
| Integration | `tau chat` REPL: scripted LLM emits text → REPL captures stdout, asserts deltas appear inline | `crates/tau-cli/tests/cmd_chat.rs` (2 tests) |
| Integration | `tau run --stream` human mode: deltas to stdout, tool annotations to stderr | `crates/tau-cli/tests/cmd_run.rs` |
| Integration | `tau run --stream --json` mode: per-line JSON event shape via `assert_cmd` | `crates/tau-cli/tests/cmd_run.rs` |

The batch-wrapper invariant test is critical: it pins the existing
`run_with_history` behavior so we can refactor without regressions in
any of the existing 100+ run-loop unit/integration tests.

---

## 8. Module layout

| Path | Status | Purpose |
|------|--------|---------|
| `Cargo.toml` (root) | Modify | Add `async-stream = "0.3"` to `[workspace.dependencies]` |
| `crates/tau-runtime/Cargo.toml` | Modify | Add `async-stream = { workspace = true }` |
| `crates/tau-runtime/src/stream.rs` | Create | `RunEvent` enum + `run_streaming_inner` async generator. ~250 LOC. |
| `crates/tau-runtime/src/lib.rs` | Modify | Declare `pub mod stream;`; re-export `RunEvent`. |
| `crates/tau-runtime/src/run.rs` | Modify | `run_with_history` body becomes thin stream-drainer. Bulk of the run-loop body MOVES to `stream.rs`. |
| `crates/tau-runtime/src/builder.rs` | Modify | `Runtime::run_streaming` and `Runtime::run_streaming_with_history` entry points. |
| `crates/tau-cli/src/cli.rs` | Modify | `RunArgs.stream: bool`; `ChatArgs.no_stream: bool`. |
| `crates/tau-cli/src/cmd/chat.rs` | Modify | Replace batch call with streaming consumer. Two-pass rendering (raw print → termimad re-render). |
| `crates/tau-cli/src/cmd/run.rs` | Modify | `--stream` branch consumes the stream. Human mode: stdout deltas + stderr annotations. JSON mode: per-line events. |
| `crates/tau-cli/tests/cmd_chat.rs` | Modify | Add 2 streaming tests. |
| `crates/tau-cli/tests/cmd_run.rs` | Modify | Add 2 `--stream` tests (human + JSON). |
| `crates/tau-runtime/tests/run_streaming_e2e.rs` | Create | E2E integration tests via in-process FsReadPlugin (gated `#![cfg(unix)]`). |
| `docs/decisions/0011-streaming-llm-responses.md` | Create | New ADR locking the 5 design decisions. |
| `ROADMAP.md` | Modify | Mark Tier 2 priority 8 ✅ Shipped. |

---

## 9. Implementation plan outline (~10–12 tasks)

The plan derived from this spec will have these tasks. Final wording
lives in the implementation plan.

1. `async-stream` workspace dep + tau-runtime Cargo.toml.
2. `tau-runtime::stream` module — `RunEvent` enum + module skeleton +
   per-event unit tests for the helpers (no real run-loop yet).
3. `run_streaming_inner` happy path — `Text → Finish` flow only; pump
   yields `TextDelta → TurnCompleted → RunCompleted`. Unit tests with
   a scripted LLM.
4. `run_streaming_inner` tool-dispatch flow — adds `ToolUse` handling,
   tool dispatch, `ToolCallStarted` / `ToolCallCompleted` events.
   Unit tests covering capability denial, schema validation failure,
   plugin crash, success.
5. `run_streaming_inner` failure modes — LlmError mid-stream,
   max_turns. Unit tests.
6. `Runtime::run_streaming` + `run_streaming_with_history` entry
   points in `builder.rs`.
7. **Refactor batch wrappers** — `run_with_history` becomes a thin
   stream-drainer over the new pump. Pin-test for behavior parity.
8. E2E integration test at
   `crates/tau-runtime/tests/run_streaming_e2e.rs`.
9. `tau chat` streaming integration — `--no-stream` flag, two-pass
   rendering, REPL integration tests.
10. `tau run --stream` integration — `--stream` flag, human mode,
    JSON mode, integration tests.
11. (Gate) Final verification + open PR.
12. (Gate) ADR-0011 + ROADMAP + squash merge.

Each task is a single Conventional Commits commit. Per-task
verification: `cargo build/test/doc/fmt/clippy` workspace-level. CI:
no new jobs (no new workspace member; no new external service in CI).
Branch protection stays at 23.

---

## 10. Out of scope (deferred)

- **Parallel tool dispatch** — running `Tool::invoke` concurrently
  with continued LLM streaming, or running multiple tool_uses in a
  turn in parallel. LLMs go quiet after `tool_use`; the parallelism
  win is narrow. Phase-2 perf work.
- **`tau chat` mid-stream markdown rendering** — rendering markdown
  formatting incrementally as the LLM types (rather than two-pass).
  Hard problem (line buffering vs. inline formatting); ship the
  typewriter preview + final-pass termimad re-render. Future work.
- **Cancellation mid-run via `Drop`** — caller drops the `Stream`
  handle to abort the run. Async cancellation in Rust requires
  careful design; v0.1 streaming runs to completion or runs to
  failure. Future work.
- **Server-Sent Events (SSE) export** — `tau serve` doesn't exist
  yet; when it does, SSE-encoding the `RunEvent` stream for HTTP
  clients is its concern.
- **Token-usage live updates** — current `CompletionChunk::Finish`
  reports usage at end-of-turn only; some providers (Anthropic)
  report partial usage mid-stream via `message_delta` events that
  the plugin currently coalesces. Live-usage events would be a
  tau-ports change (new `CompletionChunk::UsageUpdate` variant).
  Future work.
- **`tau resolve` streaming progress** — `tau resolve` already
  emits npm-style progress lines (priority 5); refactoring it onto
  the same `RunEvent` shape isn't useful (resolve is a fundamentally
  different lifecycle).

---

## 11. Cross-references

- ADR-0006 §5 — the deferral this spec closes.
- ADR-0003 §2 — established `LlmBackend::stream` in tau-ports.
- ADR-0008 — `llm.stream` IPC wire method (alongside `llm.complete`).
- `crates/tau-ports/src/llm.rs:280-296` — `CompletionChunk` enum
  (the streaming unit).
- `crates/tau-ports/src/llm.rs:362-364` — `CompletionStream` type
  alias.
- `crates/tau-ports/src/llm.rs:392,399` — `LlmBackend::complete` and
  `stream` trait methods.
- `crates/tau-runtime/src/run.rs:255` — current `backend.complete()`
  call site (the entry point that becomes a wrapper over the new
  pump).
- `crates/tau-runtime/src/plugin_host/ipc_llm.rs:126` — IPC plugin
  `stream` impl (already wired).
- ADR-0009 — typed-error policy; this sub-project follows it (no new
  error variants).
- ADR-0010 — schema validation; the new pump preserves its
  `BadArgs`-flows-through-conversation semantic.
- ADR-0011 (this sub-project's deliverable) — locks the 5 design
  decisions in §3.
