# ADR-0011: Streaming LLM responses

**Status:** Accepted
**Date:** 2026-04-30
**Deciders:** Titouan Lebocq
**Supersedes:** —
**Closes:** [ADR-0006](0006-tau-runtime.md) §5 — streaming was a v0.1
deferral: `LlmBackend::stream` existed and every shipped backend
implemented it, but no kernel-level consumer drove it.
**Amends:** —
**Refines:** [ADR-0006](0006-tau-runtime.md) §9 (outcome/error
dichotomy — this ADR refines the streaming-path classification of
fatal errors via a new `RunEvent::FatalError` terminal event that
preserves byte-identical batch-API error semantics).

## Context

Three of the LLM backends shipped in priorities 2a-2c
(Anthropic/Ollama/OpenAI) implement `LlmBackend::stream`, returning a
`CompletionStream` (`Pin<Box<dyn Stream<Item = Result<CompletionChunk,
LlmError>> + Send>>`). The trait has been there since priority 1, and
the protocol's `llm.stream` IPC wire method exists per ADR-0008. Yet
`Runtime::run_with_history` calls `LlmBackend::complete` exclusively —
the entire response materializes before the agent loop sees it.

The cost:

1. **`tau chat` REPL feels dead** during long completions. The user
   types a question, sees nothing for 5-30 seconds, then the full
   response renders at once.
2. **`tau run` cannot drive token-aware UIs.** Scripts piping
   `tau run --json` get one final blob; they can't react to text as it
   arrives.
3. **Two distinct codepaths** would emerge if streaming consumers kept
   calling `LlmBackend::stream` directly while batch consumers kept
   calling `complete`. ADR-0006 §16's "single source of truth for the
   agent loop" invariant would break.

ADR-0006 §5 reserved this work explicitly:

> Streaming is deferred to a future ADR. v0.1 ships
> `LlmBackend::stream` as a contract for backends but no
> kernel-level consumer; `Runtime::run_with_history` is the
> non-streaming entry point.

This ADR closes that reservation.

## Decision

Five inter-locking commitments:

### 1. API surface: `Stream<Item = RunEvent>`

`Runtime::run_streaming` and `run_streaming_with_history` are public
async methods on `impl Runtime` that return `Result<impl
Stream<Item = RunEvent> + 'static, RuntimeError>`. The caller drives
consumption via `.next().await` or equivalent stream adapters.

Rationale: `Stream<Item = RunEvent>` is the idiomatic Rust async
iterator. It composes naturally with `tokio_stream`, `futures-util`,
and any future SSE / WebSocket / long-poll surface. Callbacks and
mpsc channels were considered:

- **Callback** (`run_streaming(req, on_event: impl FnMut(...))`) puts
  the kernel in charge of pacing. Caller can't pause / await between
  events; `async`-flavored callbacks force the trait to be `async fn`
  in trait, which needs `async_trait` or fights the compiler.
- **mpsc channel** adds a hidden buffer and a second runtime task; the
  consumer has to learn the channel's bounded/unbounded semantics
  separately. Pull-based `Stream` is the simpler primitive.

The lifetime is `'static`, NOT `'_`. The stream owns all the data it
needs (`Arc<dyn DynLlmBackend>`, owned `HashMap<String, Arc<dyn
DynTool>>`, owned `Vec<Capability>`, etc.) so it doesn't borrow from
the runtime registry. After construction the stream is independent of
`&self`. The plan-erratum's draft signature
`+ Send + '_` was wrong on both points: the stream owns its data
(`'static`), and it cannot be `Send` because `DynLlmBackend::stream`
returns a non-`Send` `BoxFuture` per the design at
`crates/tau-runtime/src/builder.rs:45-50`.

Trigger to revisit: A `Send`-bounded variant of `DynLlmBackend` (and
sibling traits) is needed before streaming consumers can use
`tokio::spawn` (rather than running on the same task). At that point
the public signature gains `+ Send`.

### 2. Event vocabulary: kernel-translated `RunEvent`

The stream yields **kernel-translated** events, not the raw
`CompletionChunk`s from `LlmBackend::stream`. The `RunEvent` enum has
six variants:

| Variant | Carries | Fires when |
|---|---|---|
| `TextDelta { delta: String }` | one text fragment | LLM emits `CompletionChunk::Text` |
| `ToolCallStarted { id, name, args }` | the tool the LLM wants to call | LLM emits `CompletionChunk::ToolUse` (display intent BEFORE dispatch — see decision 3) |
| `ToolCallCompleted { id, name, result: Result<ToolResult, String> }` | dispatch outcome | `Tool::invoke` returns OR validation fails (BadArgs is recoverable; `result: Err(reason)` and the loop continues) |
| `TurnCompleted { stop_reason, usage, turn }` | per-turn summary | LLM `Finish` chunk arrives AND all tool dispatches resolved |
| `RunCompleted { outcome }` | final `RunOutcome` | the run completes (success or agent-level failure: `Completed`, `Failed{PolicyDenied}`, `Failed{OutOfResources}`) |
| `FatalError { kind, detail, context_json, tool_error_variant }` | structured kernel-error payload | the streaming pump hits an error that the **batch API** must surface as `Err(RuntimeError::*)` (LLM, Tool::*, ToolNotRegistered) |

`RunEvent` is `#[non_exhaustive]` + `#[derive(Debug, Clone)]`. Future
variants are additive.

Rationale: streaming consumers (REPL UIs, JSON event readers) shouldn't
have to learn LLM-protocol semantics (raw chunk types, partial-tool-use
accumulation, provider-specific finish reasons). The kernel handles
the protocol gymnastics; UIs see a clean state machine. The
`ToolCallStarted` / `ToolCallCompleted` pair is the equivalent
abstraction for tool dispatch — the consumer doesn't need to track
chunk-by-chunk tool_use accumulation.

The `FatalError` variant is the major **plan-erratum revision** vs.
the spec's original 5-variant design. The original "no new error
variants" rule (spec §5) was wrong: `Runtime::run_with_history`'s
public batch API returns `Result<RunOutcome, RuntimeError>` where
`RuntimeError::Tool(ToolError::BadArgs { reason })`,
`RuntimeError::ToolNotRegistered { ... }`, and
`RuntimeError::Llm(...)` are all **kernel-error** failures, distinct
from agent-level `RunOutcome::Failed { kind: PolicyDenied }`. To
preserve byte-identical batch behavior under Task 7's refactor (where
the batch path becomes a thin stream-drainer), the streaming pump
needed a way to communicate kernel errors back to the drainer for
typed reconstruction. `FatalError`'s `kind: String` (e.g. "Tool",
"Llm", "ToolNotRegistered") + `tool_error_variant: Option<String>`
fields together let the drainer reconstruct the precise typed
`RuntimeError::*` variant the test suite asserts on. Without this
variant, either (a) the batch API's typed error variants would have to
change (forbidden by spec §4.4's byte-identical requirement) or (b)
all errors would collapse into agent-level failures (a UX regression
in `tau run`, where exit code 1 / 2 distinguish agent failure /
kernel error).

Trigger to revisit: a third-party streaming consumer that wants
provider-specific raw chunks (e.g., for a custom debugging UI). At
that point an opt-in raw-chunk channel can be added without
disturbing the kernel-translated events.

### 3. `ToolCallStarted` fires on LLM emission, not after dispatch

When the LLM streams a `CompletionChunk::ToolUse(tu)`, the kernel pump
yields `RunEvent::ToolCallStarted { id, name, args }` **immediately**
— before any capability check, schema validation, session open, or
`Tool::invoke` call. The matching `ToolCallCompleted` fires after
dispatch resolves.

Rationale: REPL UIs need to display intent — "the agent wants to call
fs-read with path=foo.txt" — before the dispatch itself begins. If
`ToolCallStarted` fired only after dispatch, slow tools (network
calls, long shell commands) would freeze the UI for the dispatch
duration with no visible activity. The spec's Q3-A locked this in
based on the user-experience requirement.

The pump invariant becomes: **every `ToolCallStarted` is paired with
a `ToolCallCompleted` (same id) before the next `TurnCompleted`, OR
followed by a terminal `RunCompleted { Failed }` / `FatalError`**.

The "OR" is the documented exception:

- A capability denial mid-turn yields `RunCompleted { Failed {
  PolicyDenied } }` and terminates the run; the offending tool's
  `ToolCallStarted` fired but its `ToolCallCompleted` does NOT.
- A real plugin invocation crash yields `FatalError { kind: "Tool",
  ... }` and terminates the run; same exception.

Trigger to revisit: a UI that requires every Started to have a
matching Completed (e.g., a progress bar). At that point a sentinel
`Aborted` ToolCallCompleted variant can be added.

### 4. Pure `RunEvent` items, not `Result<RunEvent, ...>`

The stream's item type is `RunEvent`, not `Result<RunEvent,
StreamError>`. There is no separate "stream error" variant; instead,
errors surface through:

- **Construction-time failures** (capability override invalid, LLM
  backend not registered, tool not registered at build): the
  `Result<impl Stream, RuntimeError>` returned from
  `run_streaming(...)` is `Err(RuntimeError::*)` BEFORE the stream
  materializes.
- **Mid-stream agent-level failures** (`PolicyDenied`,
  `OutOfResources`): yielded as `RunEvent::RunCompleted { outcome:
  RunOutcome::Failed { kind, .. } }`. The agent-loop terminated
  cleanly; the agent itself failed.
- **Mid-stream kernel-level failures** (`LlmError`, `ToolError::*`,
  `ToolNotRegistered`): yielded as `RunEvent::FatalError { kind,
  detail, context_json, tool_error_variant }`. The kernel hit an
  error the batch API surfaces as `Err(RuntimeError::*)`.

Rationale: a single error path is simpler than a stream-item Result.
Construction errors are caught synchronously; mid-stream errors are
events the consumer renders alongside text. CLIs map kernel-level
fatal errors to exit code 2 (kernel error); agent-level failures map
to exit code 1 (`AgentFailed` marker error).

Trigger to revisit: a streaming consumer that wants per-chunk
recoverable errors (e.g., a partial-text-render path for OpenAI's
"content moderation" mid-stream rejections). At that point a
`RunEvent::Warning` or similar non-terminal event variant can be
added without changing the terminal event semantics.

### 5. Full CLI scope: kernel + `tau chat` + `tau run --stream`

Ship streaming-aware UI in both `tau chat` and `tau run` in the same
sub-project (not deferred to a follow-up):

- **`tau chat`** streams by default. The new `--no-stream` flag
  (`ChatArgs.no_stream: bool`, default `false`) opts out for users
  who prefer the batch render UX. Two-pass rendering: during the
  stream, raw `print!("{delta}")` + flush gives a typewriter UX; on
  `RunCompleted`, the existing `render_final_message` helper
  re-renders the assistant text via termimad (markdown) — the user
  sees text twice (rough draft → final formatting). Tool annotations
  go to stderr (`→ calling X...` / `✓ X completed`) so stdout stays
  the agent's text.
- **`tau run`** gains a `--stream` flag (`RunArgs.stream: bool`,
  default `false`) for opt-in streaming. Polarity asymmetry is
  intentional: `tau run` is script-friendly by default (one final
  blob is easier to pipe); `tau chat` is interactive by default
  (typewriter UX is the win). Human mode emits text deltas to stdout
  + tool annotations to stderr; JSON mode emits one canonical event
  per stdout line:

```json
{"event":"text_delta","delta":"..."}
{"event":"tool_call_started","id":"...","name":"...","args":...}
{"event":"tool_call_completed","id":"...","name":"...","result":{"ok":true}}
{"event":"turn_completed","stop_reason":"...","usage":{...},"turn":N}
{"event":"run_completed","outcome":{...}}
```

Rationale: bundling the UX surfaces with the kernel API in one
sub-project means the kernel pump is exercised end-to-end (CLI →
runtime → backend) before the sub-project closes. A future ADR
revising the event shape would touch all three layers consistently.
Splitting into kernel-only-now + UI-later would risk landing a
kernel API that doesn't fit any actual UI's needs.

Trigger to revisit: a third user-facing surface (TUI, web SSE,
JSON-RPC over stdio for `tau serve`). Each surface gets its own ADR
section reusing the canonical event shape from this ADR.

## Consequences

### Negative / new cost

- `tau-runtime` gains a transitive dep on `async-stream = "0.3"`
  (~50KB compiled). Negligible vs. the existing `tokio` /
  `futures-core` footprint.
- `tau-runtime` gains a `RunEvent` enum (6 variants, ~200 LOC) and a
  `run_streaming_inner` async generator (~250 LOC). The generator
  uses `async_stream::stream! { ... }` macro syntax; future
  contributors need familiarity. (`async-stream` is widely used; not
  a high adoption cost.)
- `tau-runtime`'s `run.rs` agent-loop body (~400 LOC at run.rs:111-510
  pre-refactor) MOVED into `stream.rs`'s `run_streaming_inner`. The
  batch path (`run_with_history`) is now a ~50-line stream-drainer
  with a typed-error reconstruction match for FatalError. Two
  codepaths collapsed into one source of truth.
- `RunEvent::FatalError`'s `tool_error_variant: Option<String>` is a
  string-tagged-variant pattern that's slightly less type-safe than
  carrying the original `ToolError` directly. Trade-off: passing
  `ToolError` through a `Clone`-required event variant would force
  `LlmError` and `ToolError` to derive `Clone`, expanding the public
  API surface area. The string-tag approach is additive and
  reversible.
- `RunEvent` is `Clone`, which forces every variant to carry only
  `Clone` types. This rules out future variants carrying `dyn` trait
  objects directly (they'd need to be `Arc<dyn ...>` or owned
  payloads).

### Positive

- The streaming pump is **the single source of truth** for the agent
  loop. `run_with_history` is a thin drainer that returns the
  terminal `RunCompleted.outcome` (or reconstructs `RuntimeError::*`
  from `FatalError`). The 100+ existing run-loop tests continue to
  pass unchanged — they're the regression net.
- LLM-backend plugins' existing `stream()` impls (priority 2a-2c) are
  now exercised by every batch-mode `tau run` and `tau chat` call,
  not just opt-in streaming. The 5 test-fixture LLMs that previously
  panicked on `stream()` were updated to delegate via
  `tau_ports::batch_to_stream(self.complete(req).await?)`.
- `tau chat` typewriter UX dramatically improves perceived latency
  on long completions.
- `tau run --stream --json` enables token-aware downstream tooling
  (progress bars, partial-render UIs, telemetry pipelines).

### Neutral / new obligations

- Future tau-runtime API additions involving streaming (cancellation
  via `Drop`, parallel tool dispatch, per-event token usage) require
  their own ADRs (QG18). The MANDATORY-rule template at the schema-
  validation boundary (ADR-0010) now also fires inside the streaming
  pump's tool-dispatch flow; the rule is unchanged.
- The `async-stream = "0.3"` crate version is pinned to the major in
  `[workspace.dependencies]`; major upgrades verify the public API
  surface.

## Alternatives considered

### A. Callback-based API (`Runtime::run_streaming(req, on_event: impl FnMut(...))`)

Rejected. Caller can't pause / await between events; the closure
runs synchronously inside the kernel pump's task. To support
`async fn`-flavored callbacks, the trait would need `async_trait`
desugaring. `Stream` is the more flexible, more idiomatic primitive.

### B. mpsc channel API (`Runtime::run_streaming(req) -> mpsc::Receiver<RunEvent>`)

Rejected. mpsc adds a buffer and a second runtime task; the consumer
has to learn the bounded/unbounded semantics. Pull-based `Stream` is
the simpler primitive — the consumer paces by awaiting `.next()`.

### C. Pass-through `StreamingEvent` (raw LLM events)

Rejected. Forwarding raw `CompletionChunk`s would force every
streaming consumer to learn:
- Per-provider tool_use chunk accumulation (Anthropic deltas vs. OpenAI
  function_call deltas vs. Ollama's whole-message tool_calls).
- The kernel's tool-dispatch state machine (which tool_calls the
  kernel approved, which it denied, which crashed).
- `StopReason` mapping per provider.

The kernel's job is exactly to translate this into a clean state
machine. Punting that to consumers would multiply UI complexity
across every consumer and make UI testing harder.

### D. End-of-turn-only tool dispatch (no `ToolCallStarted` mid-stream)

Rejected. UI would freeze during long tool calls with no visible
activity. `ToolCallStarted` fires immediately on LLM emission so
REPLs can show "→ calling fs-read..." while dispatch proceeds.

### E. No streaming UX (kernel-only API, defer CLI UX to a follow-up)

Rejected. See decision 5. Bundling the kernel API with at least one
real UI consumer ensures the API actually fits a UI's needs. A
kernel-only API would risk landing a shape that doesn't compose with
the eventual UI work.

### F. Drop the byte-identical batch-API requirement; collapse all errors into `RunOutcome::Failed`

Rejected. `tau run`'s exit-code semantics distinguish agent failure
(exit 1, `AgentFailed` marker) from kernel error (exit 2). Collapsing
them would make scripts that depend on this distinction silently
misbehave. The `FatalError` variant preserves the distinction.

## References

- Spec: `docs/superpowers/specs/2026-04-30-streaming-design.md`
- Plan: `docs/superpowers/plans/2026-04-30-streaming.md`
- ADR-0006 §5 — the deferral this ADR closes.
- ADR-0006 §16 — the "single source of truth for the agent loop"
  invariant this refactor preserves.
- ADR-0008 — `llm.stream` IPC wire method.
- ADR-0009 — typed-error policy; new error variants follow this.
- ADR-0010 — schema validation; the streaming pump preserves the
  MANDATORY-rule template at the validator boundary.
- `crates/tau-runtime/src/stream.rs` — the streaming module.
- `crates/tau-runtime/src/run.rs:111-196` — the post-refactor batch
  drainer (replaces the original ~400 LOC agent-loop body).
- `crates/tau-runtime/src/builder.rs::Runtime::run_streaming` — the
  public entry point.
- `crates/tau-runtime/tests/run_streaming_e2e.rs` — 5 e2e scenarios.
- `crates/tau-cli/src/cmd/chat.rs` — `tau chat` two-pass rendering.
- `crates/tau-cli/src/cmd/run.rs::run_streaming_path` — `tau run
  --stream` human + JSON modes.
