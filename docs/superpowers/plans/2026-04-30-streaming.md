# Streaming LLM Responses Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `Runtime::run_streaming` (and `run_streaming_with_history`) yielding a `Stream<Item = RunEvent>` as the agent loop progresses; refactor `run_with_history` as a thin consumer of the new pump (zero behavior change for batch callers); ship `tau chat` streaming-on-by-default and `tau run --stream` (human + JSON modes).

**Architecture:** New `tau-runtime::stream` module owns the `RunEvent` enum and `run_streaming_inner` async-generator (built via `async_stream::stream!`). The kernel pump translates `CompletionChunk` events from `LlmBackend::stream` into higher-level `RunEvent`s (`TextDelta`, `ToolCallStarted`, `ToolCallCompleted`, `TurnCompleted`, `RunCompleted`). The bulk of today's `run.rs:111-510` agent-loop body MOVES into `stream.rs`; existing `run_with_history` becomes a thin stream-drainer. CLI integrations: `tau chat` uses two-pass rendering (raw `print!` typewriter + termimad re-render on completion); `tau run --stream` emits text deltas to stdout and tool annotations to stderr (or per-line JSON events with `--json`).

**Tech Stack:** Rust 2021, `async-stream = "0.3"` (new workspace dep), `futures-core` (already a workspace dep), `tokio`, `termimad` (already in tau-cli), `serde_json`.

---

## Plan-erratum (carryover constraints)

Apply preemptively. Do NOT re-derive.

- **`RunEvent` is `#[non_exhaustive]`** with 5 variants (`TextDelta`, `ToolCallStarted`, `ToolCallCompleted`, `TurnCompleted`, `RunCompleted`). Doctests must be `ignore`-marked.

- **`Runtime::run_streaming` and `run_streaming_with_history`** return `Result<impl Stream<Item = RunEvent> + Send + '_, RuntimeError>`. The lifetime tie to `&self` is intentional — the stream borrows the runtime's plugin registry.

- **`Runtime::run` and `run_with_history`** retain their EXISTING public signatures (return `Result<RunOutcome, RuntimeError>`) but their bodies REFACTOR to thin stream-drainers in Task 7. The "batch wrapper invariant" is critical — the existing 100+ run-loop unit/integration tests are the regression net.

- **The bulk of today's `run.rs:111-510` agent-loop body MOVES into `crates/tau-runtime/src/stream.rs`.** This is the largest refactor of this sub-project. Sequencing reduces risk: Tasks 2-5 build `run_streaming_inner` ALONGSIDE the existing run.rs body (no refactor yet); Task 6 wires the public entry points; Task 7 deletes the bulk of run.rs:111-510 and points the existing entry points at the new pump.

- **`async-stream = "0.3"`** added to root `[workspace.dependencies]` AND `crates/tau-runtime/Cargo.toml`. Use `async_stream::stream! { ... }` macro.

- **`RunEvent` re-exports at the crate root** — `pub use stream::RunEvent` from `lib.rs`.

- **No new error variants.** Existing `RuntimeError` covers construction-time failures; mid-stream failures become `RunOutcome::Failed { status: AgentStatus::Failed { kind, .. }, .. }` via existing `FailureKind` variants.

- **`tau-cli`'s `RunArgs`** gets `--stream` (opt-in, default `false`). **`ChatArgs`** gets `--no-stream` (opt-out, default `false` ⇒ streaming on). Polarity asymmetry is intentional per spec §5.3.

- **`tau chat` two-pass rendering:** raw `print!` + flush during streaming, termimad re-render once `RunCompleted` fires (only on `RunOutcome::Completed`). Termimad rendering uses the existing `render_final_message` helper at `chat.rs:313`.

- **`tau run --stream`** human mode: text deltas to stdout, tool annotations to stderr. JSON mode: per-line objects via existing `Output::json` helper.

- **`--stream` sequences AFTER** the existing `requires.tools` resolve flow (priority 5). Resolve and run are sequential.

- **For tests destructuring `#[non_exhaustive]` enums cross-crate:** `let X { fields, .. } = value else { panic!() };`.

- **E2E tests** at `crates/tau-runtime/tests/run_streaming_e2e.rs` follow the existing convention: gated `#![cfg(unix)]`; mirror the in-process FsReadPlugin adapter pattern from `tool_plugin_e2e.rs`.

- **NO new CI jobs.** No new workspace member; no new external service in CI. Branch protection stays at 23 required checks.

---

## File structure

| Path | Status | Purpose |
|------|--------|---------|
| `Cargo.toml` (root) | Modify | Add `async-stream = "0.3"` to `[workspace.dependencies]` |
| `crates/tau-runtime/Cargo.toml` | Modify | Add `async-stream = { workspace = true }` |
| `crates/tau-runtime/src/stream.rs` | Create | `RunEvent` enum + `run_streaming_inner` async generator. Built incrementally across Tasks 2-5. ~250 LOC final. |
| `crates/tau-runtime/src/lib.rs` | Modify | Declare `pub mod stream;`; re-export `RunEvent`. |
| `crates/tau-runtime/src/run.rs` | Modify | Task 7: `run_with_history` body collapses to thin stream-drainer. Bulk of agent-loop body moves to `stream.rs`. |
| `crates/tau-runtime/src/builder.rs` | Modify | Task 6: `Runtime::run_streaming` + `run_streaming_with_history` public entry points. |
| `crates/tau-cli/src/cli.rs` | Modify | Task 9-10: `RunArgs.stream: bool`, `ChatArgs.no_stream: bool`. |
| `crates/tau-cli/src/cmd/chat.rs` | Modify | Task 9: REPL turn handler consumes the stream; two-pass render; `--no-stream` opt-out. |
| `crates/tau-cli/src/cmd/run.rs` | Modify | Task 10: `--stream` branch consumes the stream; human + JSON modes. |
| `crates/tau-cli/tests/cmd_chat.rs` | Modify | Task 9: 2 streaming tests. |
| `crates/tau-cli/tests/cmd_run.rs` | Modify | Task 10: 2 `--stream` tests (human + JSON). |
| `crates/tau-runtime/tests/run_streaming_e2e.rs` | Create | Task 8: gated `#![cfg(unix)]`; ~5 e2e scenarios via in-process FsReadPlugin. |
| `docs/decisions/0011-streaming-llm-responses.md` | Create (Task 12) | Full ADR locking the 5 design decisions. |
| `ROADMAP.md` | Modify (Task 12) | Mark Tier 2 priority 8 ✅ Shipped. |

---

## Task 1: async-stream workspace dep + tau-runtime Cargo.toml

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/Cargo.toml`
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/Cargo.toml`

### Steps

- [ ] **Step 1.1: Verify async-stream version**

```bash
cargo search async-stream 2>/dev/null | head -3
```
Expected: a line like `async-stream = "0.3.6"`. Pin to the major `"0.3"` for forward-compat.

- [ ] **Step 1.2: Add to workspace deps**

Edit `/Users/titouanlebocq/code/tau/Cargo.toml`. Find `[workspace.dependencies]`. Add a new line alphabetically (near `async-stream` or `bytes`):

```toml
async-stream    = "0.3"
```

- [ ] **Step 1.3: Add to tau-runtime's deps**

Edit `/Users/titouanlebocq/code/tau/crates/tau-runtime/Cargo.toml`. In `[dependencies]`, add (near the existing `futures-core` line):

```toml
# Async generator for `Runtime::run_streaming` (Tier 2 priority 8 / ADR-0011).
async-stream        = { workspace = true }
```

- [ ] **Step 1.4: Verify the dep compiles**

```bash
cargo build --workspace
```
Expected: PASS. The dep is added but not yet consumed.

- [ ] **Step 1.5: Run full verification**

```bash
cargo build --workspace
cargo test -p tau-runtime --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-runtime --doc
```
Expected: all PASS.

- [ ] **Step 1.6: Commit Cargo.lock too if regenerated**

```bash
git status
```
If `Cargo.lock` shows as modified, include it in the commit. Priority-6's Task 1 implementer skipped this and we had to fix it post-PR-open — don't repeat.

- [ ] **Step 1.7: Commit + push**

```bash
git add Cargo.toml Cargo.lock crates/tau-runtime/Cargo.toml
git commit -m "$(cat <<'EOF'
build(runtime): add async-stream 0.3 workspace dep

Foundation for Runtime::run_streaming (Tier 2 priority 8). The crate
is added to [workspace.dependencies] and pulled into tau-runtime; not
yet consumed at this commit (Task 2 wires it up).

Pinned to "0.3" major to allow patch upgrades within the major.

Refs: docs/superpowers/specs/2026-04-30-streaming-design.md §4

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push
```

---

## Task 2: `tau-runtime::stream` module skeleton — `RunEvent` enum

**Files:**
- Create: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/stream.rs` (skeleton only — `RunEvent` enum + ~5 unit tests; no `run_streaming_inner` yet)
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/lib.rs` — declare module + re-export.

### Steps

- [ ] **Step 2.1: Declare module + re-export in lib.rs**

In `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/lib.rs`, find existing module declarations (search for `pub mod options;` or `pub mod outcome;`). Add alphabetically:

```rust
pub mod stream;
```

In the existing `pub use` block (where `RunOptions`, `RunOutcome`, etc. are re-exported), add:

```rust
pub use stream::RunEvent;
```

- [ ] **Step 2.2: Create stream.rs skeleton**

Create `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/stream.rs`:

```rust
//! Streaming agent runs. Realizes ADR-0006 §5 deferral closure
//! (Tier 2 priority 8).
//!
//! `Runtime::run_streaming` (added in Task 6) yields a
//! `Stream<Item = RunEvent>` as the agent loop progresses — text
//! deltas as the LLM types, tool calls as the LLM commits to them,
//! tool results as dispatch finishes. The terminal `RunCompleted`
//! event carries the final `RunOutcome` (success or failure).
//!
//! See `docs/superpowers/specs/2026-04-30-streaming-design.md` and
//! ADR-0011 (added in Task 12).

use tau_domain::Value;
use tau_ports::{StopReason, TokenUsage, ToolResult};

use crate::outcome::RunOutcome;

/// Streaming event from `Runtime::run_streaming`.
///
/// Always terminates with exactly one `RunCompleted`; intermediate
/// events are unbounded per agent run. See spec §4.2 for the full
/// pump invariants.
///
/// Per ADR-0011:
/// - Every `ToolCallStarted` is followed by either a matching
///   `ToolCallCompleted` (same `id`) before the next `TurnCompleted`,
///   OR a terminal `RunCompleted { outcome: Failed }` if dispatch
///   crashed mid-flight.
/// - `TurnCompleted` arrives only after the turn's LLM `Finish` AND
///   all that turn's tool dispatches resolved.
/// - Stream order preserves LLM source order; the kernel never
///   reorders events.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum RunEvent {
    /// LLM emitted a text fragment. Concatenate with previous deltas
    /// for the running assistant message text.
    TextDelta {
        /// Text fragment to append.
        delta: String,
    },

    /// LLM emitted a complete `tool_use` block. Fires immediately
    /// when the kernel sees `CompletionChunk::ToolUse` — BEFORE the
    /// tool is dispatched. Display intent: "agent wants to call X
    /// with args Y". The matching `ToolCallCompleted` fires after
    /// dispatch finishes.
    ToolCallStarted {
        /// Provider-supplied tool-use id; correlates with
        /// `ToolCallCompleted.id`.
        id: String,
        /// Tool name.
        name: String,
        /// Args the LLM emitted.
        args: Value,
    },

    /// Tool dispatch finished. Fires after `Tool::invoke` returns,
    /// regardless of success/failure. Carries the tool result OR a
    /// validation/dispatch error message.
    ToolCallCompleted {
        /// Matches the `id` from `ToolCallStarted`.
        id: String,
        /// Tool name.
        name: String,
        /// `Ok(ToolResult)` on success; `Err(reason)` for validation
        /// failures or other recoverable errors. Plugin-crash-class
        /// errors don't surface here — they terminate the run via
        /// `RunCompleted`.
        result: Result<ToolResult, String>,
    },

    /// One turn of the agent loop completed. The LLM's `Finish`
    /// chunk arrived AND any tool calls within the turn finished
    /// dispatching.
    TurnCompleted {
        /// Why the turn ended (per LLM-reported `StopReason`).
        stop_reason: StopReason,
        /// Token usage for this turn. `None` if the provider did
        /// not report.
        usage: Option<TokenUsage>,
        /// Turn number (1-indexed) within the run.
        turn: u32,
    },

    /// Terminal event. Always exactly one per stream. After this
    /// fires, the stream returns `None`.
    RunCompleted {
        /// Final outcome — same shape as `Runtime::run` returns.
        outcome: RunOutcome,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use tau_ports::fixtures::{make_token_usage, make_tool_result};

    #[test]
    fn run_event_text_delta_clone_preserves_delta() {
        let e = RunEvent::TextDelta {
            delta: "Hello".into(),
        };
        let cloned = e.clone();
        let RunEvent::TextDelta { delta } = cloned else {
            panic!("expected TextDelta")
        };
        assert_eq!(delta, "Hello");
    }

    #[test]
    fn run_event_tool_call_started_clone_preserves_fields() {
        let e = RunEvent::ToolCallStarted {
            id: "call_1".into(),
            name: "fs-read".into(),
            args: Value::Null,
        };
        let cloned = e.clone();
        let RunEvent::ToolCallStarted { id, name, .. } = cloned else {
            panic!("expected ToolCallStarted")
        };
        assert_eq!(id, "call_1");
        assert_eq!(name, "fs-read");
    }

    #[test]
    fn run_event_tool_call_completed_carries_result() {
        let e = RunEvent::ToolCallCompleted {
            id: "call_1".into(),
            name: "fs-read".into(),
            result: Ok(make_tool_result(vec![], false)),
        };
        let RunEvent::ToolCallCompleted { result, .. } = e else {
            panic!("expected ToolCallCompleted")
        };
        assert!(result.is_ok());
    }

    #[test]
    fn run_event_tool_call_completed_carries_error_reason() {
        let e = RunEvent::ToolCallCompleted {
            id: "call_1".into(),
            name: "fs-read".into(),
            result: Err("validation failed".into()),
        };
        let RunEvent::ToolCallCompleted { result, .. } = e else {
            panic!("expected ToolCallCompleted")
        };
        let Err(reason) = result else {
            panic!("expected Err")
        };
        assert_eq!(reason, "validation failed");
    }

    #[test]
    fn run_event_turn_completed_carries_stop_reason_and_usage() {
        let e = RunEvent::TurnCompleted {
            stop_reason: StopReason::ToolUse,
            usage: Some(make_token_usage(10, 5)),
            turn: 3,
        };
        let RunEvent::TurnCompleted {
            stop_reason,
            usage,
            turn,
        } = e
        else {
            panic!("expected TurnCompleted")
        };
        assert_eq!(stop_reason, StopReason::ToolUse);
        assert_eq!(turn, 3);
        assert!(usage.is_some());
    }
}
```

- [ ] **Step 2.3: Verify**

```bash
cargo build --workspace
cargo test -p tau-runtime --all-targets stream
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-runtime --doc
```
Expected: build PASS; 5 unit tests PASS; fmt/clippy/doctest clean.

- [ ] **Step 2.4: Commit + push**

```bash
git add crates/tau-runtime/src/stream.rs crates/tau-runtime/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(runtime): add stream module skeleton — RunEvent enum

Defines the five-variant RunEvent enum (TextDelta, ToolCallStarted,
ToolCallCompleted, TurnCompleted, RunCompleted) with #[non_exhaustive]
+ Debug + Clone derives. Re-exported at the crate root.

5 unit tests covering each variant's clone semantics and field
accessibility. The async-generator body that yields these events
lands in Task 3 (happy path: Text → Finish only) and Task 4 (full
tool-dispatch flow).

Refs: docs/superpowers/specs/2026-04-30-streaming-design.md §4.2

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push
```

---

## Task 3: `run_streaming_inner` happy path — Text → Finish only

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/stream.rs` — add `run_streaming_inner` async generator (Text + Finish only; NO tool dispatch yet).

### Steps

- [ ] **Step 3.1: Add `run_streaming_inner` happy-path body**

In `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/stream.rs`, append after the `RunEvent` enum (BEFORE `#[cfg(test)] mod tests`):

```rust
use std::sync::Arc;
use futures_core::Stream;
use tau_domain::{
    AgentDefinition, AgentInstanceId, Address, Message, MessagePayload, PackageManifest,
};
use tau_ports::{
    CompletionChunk, CompletionRequest, LlmError,
};
use tracing::{debug, info, warn};

use crate::builder::DynLlmBackend;
use crate::error::RuntimeError;
use crate::options::RunOptions;

/// Build the stream of `RunEvent`s for a single agent run. Happy
/// path: drains the LLM stream, yields `TextDelta` per chunk, then
/// `TurnCompleted` + `RunCompleted` once `Finish` arrives. No tool
/// dispatch in this commit (Task 4 adds it).
///
/// Constructed inputs are pre-validated by the caller in Task 6
/// (`Runtime::run_streaming`); here we trust them.
#[allow(dead_code)] // wired up by Task 6
pub(crate) fn run_streaming_inner(
    backend: Arc<dyn DynLlmBackend>,
    agent_def: AgentDefinition,
    _package_manifest: PackageManifest,
    history: Vec<Message>,
    initial_message: Message,
    options: RunOptions,
) -> impl Stream<Item = RunEvent> + Send + 'static {
    async_stream::stream! {
        let agent_instance_id = AgentInstanceId::new();
        let mut messages: Vec<Message> = Vec::with_capacity(history.len() + 1);
        messages.extend(history);
        messages.push(initial_message);
        let mut total_turns: u32 = 0;
        let mut aggregated_tokens = TokenUsage::default();

        info!(name = "runtime.streaming_run_started");

        while total_turns < options.max_turns {
            total_turns += 1;
            debug!(name = "runtime.streaming_turn_started", turn = total_turns);

            // Build request — text-only flow at this stage.
            let mut request = CompletionRequest::new(agent_def.llm_backend.as_str().into());
            request.system = agent_def.system_prompt.clone();
            request.messages = crate::run::agent_messages_to_provider_messages(&messages);
            request.tools = Vec::new(); // Task 4 will populate with capability-filtered tool specs.

            // Open the LLM stream.
            let mut llm_stream = match backend.stream(request).await {
                Ok(s) => s,
                Err(llm_err) => {
                    warn!(name = "runtime.streaming_llm_open_failed");
                    yield make_llm_error_outcome(
                        llm_err,
                        messages,
                        total_turns,
                        aggregated_tokens,
                    );
                    return;
                }
            };

            let mut accumulated_text = String::new();
            let mut turn_stop_reason: Option<StopReason> = None;
            let mut turn_usage: Option<TokenUsage> = None;

            // Drain the LLM stream.
            use futures_core::Stream as _;
            use std::pin::Pin;
            let mut llm_stream = Pin::new(&mut llm_stream);
            // Manual `next` instead of futures::StreamExt to avoid
            // pulling in the StreamExt dep at this layer.
            loop {
                let next = std::future::poll_fn(|cx| {
                    llm_stream.as_mut().poll_next(cx)
                }).await;
                match next {
                    None => break,
                    Some(Ok(CompletionChunk::Text { delta })) => {
                        accumulated_text.push_str(&delta);
                        yield RunEvent::TextDelta { delta };
                    }
                    Some(Ok(CompletionChunk::ToolUse(_))) => {
                        // Task 4 handles this; for now treat as if it
                        // didn't happen. Plugin protocol guarantees
                        // happy-path text-only tests don't exercise this.
                        warn!(name = "runtime.streaming_tool_use_unhandled_in_task_3");
                    }
                    Some(Ok(CompletionChunk::Finish { stop_reason, usage })) => {
                        turn_stop_reason = Some(stop_reason);
                        turn_usage = usage;
                        break;
                    }
                    Some(Err(llm_err)) => {
                        warn!(name = "runtime.streaming_llm_chunk_err");
                        yield make_llm_error_outcome(
                            llm_err,
                            messages,
                            total_turns,
                            aggregated_tokens,
                        );
                        return;
                    }
                }
            }

            // Append assistant turn.
            if !accumulated_text.is_empty() {
                let agent_addr = Address::Agent(agent_instance_id);
                messages.push(Message::new(
                    agent_addr.clone(),
                    Address::User,
                    MessagePayload::Text {
                        content: accumulated_text.clone(),
                    },
                ));
            }

            // Aggregate tokens.
            if let Some(usage) = turn_usage {
                aggregated_tokens.input_tokens =
                    aggregated_tokens.input_tokens.saturating_add(u64::from(usage.input_tokens));
                aggregated_tokens.output_tokens =
                    aggregated_tokens.output_tokens.saturating_add(u64::from(usage.output_tokens));
            }

            yield RunEvent::TurnCompleted {
                stop_reason: turn_stop_reason.unwrap_or(StopReason::EndTurn),
                usage: turn_usage,
                turn: total_turns,
            };

            // No tool dispatch yet (Task 4). End the run after the
            // first turn's Finish since text-only flows don't loop.
            let final_message = messages.last().cloned().expect(
                "messages contains at least the initial user message",
            );
            yield RunEvent::RunCompleted {
                outcome: RunOutcome::Completed {
                    final_message,
                    all_messages: messages,
                    total_turns,
                    token_usage: aggregated_tokens,
                },
            };
            return;
        }

        // max_turns reached (text-only path; Task 4 adds tool-loop max_turns case).
        yield make_max_turns_outcome(messages, total_turns, aggregated_tokens, options.max_turns);
    }
}

#[allow(dead_code)] // wired up by Task 4
fn make_llm_error_outcome(
    llm_err: LlmError,
    messages: Vec<Message>,
    total_turns: u32,
    token_usage: TokenUsage,
) -> RunEvent {
    use tau_domain::{AgentStatus, FailureKind};
    let detail = format!("{llm_err}");
    RunEvent::RunCompleted {
        outcome: RunOutcome::Failed {
            status: AgentStatus::failed(FailureKind::BackendError, Some(detail)),
            all_messages: messages,
            total_turns,
            token_usage,
        },
    }
}

#[allow(dead_code)] // wired up by Task 5
fn make_max_turns_outcome(
    messages: Vec<Message>,
    total_turns: u32,
    token_usage: TokenUsage,
    max_turns: u32,
) -> RunEvent {
    use tau_domain::{AgentStatus, FailureKind};
    RunEvent::RunCompleted {
        outcome: RunOutcome::Failed {
            status: AgentStatus::failed(
                FailureKind::OutOfResources,
                Some(format!("max_turns ({max_turns}) reached")),
            ),
            all_messages: messages,
            total_turns,
            token_usage,
        },
    }
}
```

NOTE: this references `crate::run::agent_messages_to_provider_messages`. That helper exists in the existing run.rs (search for `fn agent_messages_to_provider_messages`). It must remain `pub(crate)` (or be made `pub(crate)` if currently private to run.rs).

NOTE: `DynLlmBackend` is imported from `crate::builder`. If it's currently `pub(crate)` only at the dyn-trait level, ensure it's importable from this module. Likely already is via `use crate::builder::DynLlmBackend;`.

NOTE: `TokenUsage` and `StopReason` are imported via the existing `use tau_ports::{...}`.

- [ ] **Step 3.2: Add three unit tests for the happy path**

In the `#[cfg(test)] mod tests` block (before the closing `}`), add:

```rust
    use std::sync::Arc;

    use futures_core::Stream;
    use tau_domain::{
        AgentDefinition, AgentId, Address, Message, MessagePayload, PackageId, PackageName,
        Version,
    };
    use tau_ports::{
        CompletionChunk, CompletionRequest, CompletionResponse, CompletionStream, LlmBackend,
        LlmError, StopReason, TokenUsage,
    };

    use crate::builder::DynLlmBackend;

    /// LLM that emits a fixed sequence of CompletionChunk via stream().
    /// Used to script the happy-path test without real network.
    struct ScriptedLlm {
        chunks: std::sync::Mutex<Option<Vec<Result<CompletionChunk, LlmError>>>>,
    }

    impl ScriptedLlm {
        fn new(chunks: Vec<Result<CompletionChunk, LlmError>>) -> Self {
            Self {
                chunks: std::sync::Mutex::new(Some(chunks)),
            }
        }
    }

    impl LlmBackend for ScriptedLlm {
        fn name(&self) -> &str {
            "scripted-llm"
        }

        async fn complete(
            &self,
            _req: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            unimplemented!("ScriptedLlm streams only")
        }

        async fn stream(
            &self,
            _req: CompletionRequest,
        ) -> Result<CompletionStream, LlmError> {
            let chunks = self
                .chunks
                .lock()
                .expect("lock poisoned")
                .take()
                .ok_or_else(|| LlmError::Internal {
                    message: "ScriptedLlm: stream() called twice".into(),
                })?;
            Ok(Box::pin(async_stream::stream! {
                for c in chunks {
                    yield c;
                }
            }))
        }
    }

    fn agent_def() -> AgentDefinition {
        use std::str::FromStr;
        let pkg = PackageId::new(
            PackageName::from_str("test-pkg").unwrap(),
            Version::parse("0.1.0").unwrap(),
        );
        AgentDefinition::new(
            AgentId::from_str("test-agent").unwrap(),
            "test".to_string(),
            pkg,
            PackageName::from_str("scripted-llm").unwrap(),
        )
    }

    fn manifest_with_no_capabilities() -> PackageManifest {
        use tau_domain::UncheckedManifest;
        let toml_str = r#"
            name = "test-pkg"
            version = "0.1.0"
            description = "test package"
            authors = []
            source = "https://example.com/test.git"
            kind = "tool"
            dependencies = []
            capabilities = []
        "#;
        let unchecked: UncheckedManifest = toml::from_str(toml_str).unwrap();
        unchecked.validate().unwrap()
    }

    fn user_msg(text: &str) -> Message {
        Message::new(
            Address::User,
            Address::User,
            MessagePayload::Text {
                content: text.to_string(),
            },
        )
    }

    async fn collect_events(
        mut stream: impl Stream<Item = RunEvent> + Unpin,
    ) -> Vec<RunEvent> {
        use std::pin::Pin;
        let mut out = Vec::new();
        loop {
            let next = std::future::poll_fn(|cx| {
                Pin::new(&mut stream).poll_next(cx)
            }).await;
            match next {
                None => break,
                Some(e) => out.push(e),
            }
        }
        out
    }

    #[tokio::test]
    async fn happy_path_text_only_yields_text_delta_then_turn_completed_then_run_completed() {
        let llm: Arc<dyn DynLlmBackend> = Arc::new(crate::builder::dyn_llm_adapter(
            ScriptedLlm::new(vec![
                Ok(CompletionChunk::Text {
                    delta: "Hello ".into(),
                }),
                Ok(CompletionChunk::Text {
                    delta: "world".into(),
                }),
                Ok(CompletionChunk::Finish {
                    stop_reason: StopReason::EndTurn,
                    usage: Some(TokenUsage {
                        input_tokens: 10,
                        output_tokens: 5,
                    }),
                }),
            ]),
        ));

        let stream = run_streaming_inner(
            llm,
            agent_def(),
            manifest_with_no_capabilities(),
            vec![],
            user_msg("hi"),
            RunOptions::default(),
        );
        let events = collect_events(Box::pin(stream)).await;

        // Expected: TextDelta("Hello ") → TextDelta("world") → TurnCompleted → RunCompleted
        assert_eq!(events.len(), 4, "got events: {events:#?}");
        let RunEvent::TextDelta { delta } = &events[0] else {
            panic!("expected TextDelta, got {:?}", events[0])
        };
        assert_eq!(delta, "Hello ");
        let RunEvent::TextDelta { delta } = &events[1] else {
            panic!("expected TextDelta, got {:?}", events[1])
        };
        assert_eq!(delta, "world");
        assert!(matches!(events[2], RunEvent::TurnCompleted { .. }));
        assert!(matches!(events[3], RunEvent::RunCompleted { .. }));
    }

    #[tokio::test]
    async fn llm_error_mid_stream_yields_run_completed_failed() {
        let llm: Arc<dyn DynLlmBackend> = Arc::new(crate::builder::dyn_llm_adapter(
            ScriptedLlm::new(vec![
                Ok(CompletionChunk::Text {
                    delta: "Hello".into(),
                }),
                Err(LlmError::Internal {
                    message: "provider blew up".into(),
                }),
            ]),
        ));

        let stream = run_streaming_inner(
            llm,
            agent_def(),
            manifest_with_no_capabilities(),
            vec![],
            user_msg("hi"),
            RunOptions::default(),
        );
        let events = collect_events(Box::pin(stream)).await;

        // Expected: TextDelta → RunCompleted { Failed }.
        assert_eq!(events.len(), 2, "got events: {events:#?}");
        let RunEvent::RunCompleted { outcome } = &events[1] else {
            panic!("expected RunCompleted, got {:?}", events[1])
        };
        assert!(matches!(outcome, RunOutcome::Failed { .. }));
    }

    #[tokio::test]
    async fn llm_open_failure_yields_run_completed_failed_with_no_intermediate_events() {
        struct FailingLlm;
        impl LlmBackend for FailingLlm {
            fn name(&self) -> &str {
                "failing-llm"
            }
            async fn complete(
                &self,
                _r: CompletionRequest,
            ) -> Result<CompletionResponse, LlmError> {
                unimplemented!()
            }
            async fn stream(
                &self,
                _r: CompletionRequest,
            ) -> Result<CompletionStream, LlmError> {
                Err(LlmError::Internal {
                    message: "open failed".into(),
                })
            }
        }

        let llm: Arc<dyn DynLlmBackend> =
            Arc::new(crate::builder::dyn_llm_adapter(FailingLlm));

        let stream = run_streaming_inner(
            llm,
            agent_def(),
            manifest_with_no_capabilities(),
            vec![],
            user_msg("hi"),
            RunOptions::default(),
        );
        let events = collect_events(Box::pin(stream)).await;

        // Single RunCompleted { Failed }; no intermediate events.
        assert_eq!(events.len(), 1, "got events: {events:#?}");
        assert!(matches!(events[0], RunEvent::RunCompleted { .. }));
    }
```

NOTE: the test relies on `crate::builder::dyn_llm_adapter(impl LlmBackend) -> impl DynLlmBackend` — verify this helper exists. If it doesn't, look at how priority 3's e2e test wraps an `impl LlmBackend` into a `dyn DynLlmBackend`-able type (see `tool_plugin_e2e.rs` for the established pattern). Adjust the test setup accordingly. The pattern likely uses `Box::new(...) as Arc<dyn DynLlmBackend>` directly via a manual cast.

If `dyn_llm_adapter` doesn't exist as a public helper, the test can construct via `Runtime::builder().with_llm_backend(scripted_llm).build()` and then access the `Arc<dyn DynLlmBackend>` from `runtime.llm_backends().values().next().cloned().unwrap()`.

- [ ] **Step 3.3: Verify**

```bash
cargo build --workspace
cargo test -p tau-runtime --all-targets stream
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-runtime --doc
```
Expected: build PASS; 5 (Task 2) + 3 (Task 3) = 8 tests PASS; fmt/clippy/doctest clean.

If clippy flags `#[allow(dead_code)]` on the helpers — that's intentional (Tasks 4-6 wire them up).

- [ ] **Step 3.4: Commit + push**

```bash
git add crates/tau-runtime/src/stream.rs
git commit -m "$(cat <<'EOF'
feat(runtime): run_streaming_inner happy path — text-only flow

The async generator drains the LLM's CompletionStream and yields:
  Text → TextDelta
  Finish → TurnCompleted → RunCompleted { Completed }
  Err(LlmError) mid-stream → RunCompleted { Failed }
  Err(LlmError) at stream open → RunCompleted { Failed }

ToolUse chunks are warned-and-ignored in this commit; Task 4 wires
the full tool-dispatch flow. max_turns is plumbed but the text-only
path always returns after the first turn's Finish (no looping until
Task 4 adds tool-driven turn iteration).

3 unit tests + ScriptedLlm fixture for scripted CompletionChunk
sequences.

Refs: docs/superpowers/specs/2026-04-30-streaming-design.md §4.3

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push
```

---

## Task 4: `run_streaming_inner` tool-dispatch flow

**Hybrid format.**

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/stream.rs` — add `ToolUse` chunk handling, capability check, schema validation, session open/invoke/close, `ToolCallStarted` immediate emission + `ToolCallCompleted` after dispatch.

**Spec sections:** §4.3 steps 6.

**Per-task summary:**

1. **Add request-building** that includes the capability-filtered tool specs (mirror today's `run.rs:170-198`):
   - Build `tool_specs: Vec<ToolSpec>` from `runtime.tools()` filtered by `check_capabilities(granted, required)`.
   - Note: `run_streaming_inner` doesn't receive the runtime directly — it gets the LLM backend `Arc`. Either:
     - Refactor the signature to take `runtime: &Runtime` (but that breaks the `'static` Stream bound), OR
     - Pass `tool_specs: Vec<ToolSpec>` and `tools: HashMap<String, RegisteredTool>` (cloned slices that the stream owns).
   - **Pick the second**: build the tool specs + tool registry snapshot in `Runtime::run_streaming_with_history` (Task 6), pass as owned args. Streams own their data.

2. **Update the `Some(Ok(CompletionChunk::ToolUse(tu)))` arm** to:
   - Push `tu` onto a `pending_tool_uses: Vec<ToolUse>` accumulator.
   - Yield `RunEvent::ToolCallStarted { id: tu.id.clone(), name: tu.name.clone(), args: tu.input.clone() }` IMMEDIATELY (per Q3-A).

3. **After the LLM-stream drain loop, dispatch each pending tool use** (mirror today's `run.rs:340-490`):
   - For each `tu`:
     - **Capability check** via `check_capabilities(&granted, tool.capabilities())`. Denial → yield `RunCompleted { Failed { kind: PolicyDenied } }` and `return`.
     - **Schema validation** (priority 6 carryover) via `tool_args::validate_tool_args(...)`. On `ToolError::BadArgs`: best-effort teardown, append `MessagePayload::ToolError { kind: "tool_args_validation", message: reason, details: None }` to messages, yield `RunEvent::ToolCallCompleted { result: Err(reason) }`, `continue` the inner for-loop.
     - **Session open** via `tool.init(ctx)`. Failure → yield `RunCompleted { Failed { kind: BackendError } }`, `return`.
     - **Invoke** via `tool.invoke(...)`. Success → append `MessagePayload::ToolResult` (or `MessagePayload::ToolError` if `ToolResult.is_error`), yield `ToolCallCompleted { result: Ok(...) }`. Failure (real plugin crash) → best-effort teardown, yield `RunCompleted { Failed { kind: BackendError } }`, `return`.
     - **Session close** via `tool.teardown(())`. Failure → yield `RunCompleted { Failed { kind: BackendError } }`, `return`.

4. **After all dispatches resolved**, yield `TurnCompleted` and loop back to the top of the while loop for the next turn (NOT immediately yield `RunCompleted` like Task 3's text-only path did — when there ARE tool uses, we expect another turn).

5. **`pending_tool_uses.is_empty()` branch:** unchanged from Task 3 — yield `TurnCompleted` then `RunCompleted { Completed }`, return.

6. **Helper functions to add:**
   - `make_policy_denied_outcome(...)` — same shape as `make_llm_error_outcome` but for `FailureKind::PolicyDenied`.
   - `make_backend_error_outcome(...)` — for plugin-crash terminal failures.
   - `make_tool_args_validation_msg(reason)` — builds the `MessagePayload::ToolError { kind: "tool_args_validation", ... }`.

7. **Unit tests** (~5):
   - `tool_dispatch_happy_path_yields_tool_call_started_then_completed_then_turn_completed`
   - `tool_dispatch_capability_denial_yields_run_completed_failed`
   - `tool_dispatch_schema_validation_failure_yields_tool_call_completed_with_err`
   - `tool_dispatch_plugin_crash_yields_run_completed_failed`
   - `tool_dispatch_two_tools_in_one_turn_emits_both_started_and_both_completed`

8. **Verification:** standard 5-command suite. Existing tests still pass; Task 3's three tests still pass.

9. **Commit message:** `feat(runtime): run_streaming_inner tool-dispatch flow`.

10. Push.

---

## Task 5: `run_streaming_inner` failure modes

**Hybrid format.**

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/stream.rs` — finish the failure-mode coverage.

**Spec sections:** §4.3 invariants list.

**Per-task summary:**

1. **max_turns reached path:** ensure the `while total_turns < options.max_turns` exit path emits `RunCompleted { Failed { kind: OutOfResources } }`. Already in Task 3's skeleton via `make_max_turns_outcome` but not exercised — Task 4's tool-dispatch flow now loops, so a `max_turns: 1` + tool-emitting LLM hits this case.

2. **`pending_tool_uses` partial dispatch crash:** if the second tool in a turn crashes, the first tool's `ToolCallCompleted` was already yielded; the second tool's `ToolCallStarted` was yielded but its `Completed` won't fire — instead a terminal `RunCompleted { Failed }` follows. The pump invariant at spec §4.3 documents this case explicitly.

3. **Capability-denied tool that comes AFTER a successful tool in the same turn:** capability check fires per-tool; if tool 1 succeeds and tool 2 is denied, the run terminates with `RunCompleted { Failed { kind: PolicyDenied } }` after tool 1's `ToolCallCompleted` and tool 2's `ToolCallStarted`. Document; add a test.

4. **Empty LLM stream (no chunks at all):** unlikely in practice but possible. Stream EOF without `Finish` → kernel treats as `StopReason::EndTurn` with no usage; emits `TurnCompleted` then `RunCompleted { Completed }` with whatever message accumulated.

5. **Unit tests (~3):**
   - `max_turns_reached_yields_run_completed_failed_out_of_resources`
   - `mid_dispatch_crash_after_one_success_yields_started_completed_started_then_run_completed_failed`
   - `empty_llm_stream_yields_turn_completed_then_run_completed`

6. **Verification:** standard suite.

7. **Commit message:** `feat(runtime): run_streaming_inner failure-mode coverage`.

8. Push.

---

## Task 6: `Runtime::run_streaming` + `run_streaming_with_history` public entry points

**Hybrid format.**

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/builder.rs` — add the two public entry points to `impl Runtime`.

**Spec sections:** §4.1.

**Per-task summary:**

1. **`pub async fn run_streaming_with_history`:**
   - Validate inputs the same way `run_with_history` does today (LLM backend resolution, tool resolution, capability override compute_effective). If any setup fails, return `Err(RuntimeError)` BEFORE the stream materializes.
   - Build the per-tool `granted_capabilities` snapshot (priority 4 carryover).
   - Build the `tool_specs: Vec<ToolSpec>` (capability-filtered) and the `tools: HashMap<String, RegisteredTool>` snapshot.
   - Construct and return the stream:
     ```rust
     Ok(stream::run_streaming_inner(
         llm_backend,
         agent_def,
         package_manifest,
         history,
         initial_message,
         options,
         tool_specs,
         tools,
         tool_validators,
         granted_capabilities,
         deny_entries,
     ))
     ```
   - Adjust the `run_streaming_inner` signature accordingly (Tasks 3-5 used a smaller signature; this task expands it).

2. **`pub async fn run_streaming`:** delegate to `run_streaming_with_history` with `history: Vec::new()`.

3. **Doctests** for both entry points (`ignore`-marked).

4. **Verification:** standard suite.

5. **Commit message:** `feat(runtime): Runtime::run_streaming public entry points`.

6. Push.

---

## Task 7: REFACTOR `run_with_history` → thin stream-drainer

**Hybrid format. CRITICAL: this is the largest refactor of the sub-project.**

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/run.rs` — DELETE the bulk of lines 111-510. The body becomes:

  ```rust
  pub async fn run_with_history(
      &self,
      agent_def: AgentDefinition,
      package_manifest: PackageManifest,
      history: Vec<Message>,
      initial_message: Message,
      options: RunOptions,
  ) -> Result<RunOutcome, RuntimeError> {
      use futures_core::Stream as _;
      use std::pin::Pin;
      let mut stream = self
          .run_streaming_with_history(
              agent_def, package_manifest, history, initial_message, options,
          )
          .await?;
      loop {
          let next = std::future::poll_fn(|cx| {
              Pin::new(&mut stream).poll_next(cx)
          })
          .await;
          match next {
              Some(RunEvent::RunCompleted { outcome }) => return Ok(outcome),
              Some(_) => continue,
              None => unreachable!(
                  "run_streaming_inner must yield exactly one RunCompleted before stream end"
              ),
          }
      }
  }
  ```

- The helper functions still used by `stream.rs` (`agent_messages_to_provider_messages`, `preview_value`, `flatten_content_to_string`, `content_to_value`, `build_policy_denied_outcome`, etc.) stay in `run.rs` as `pub(crate)`. Anything ONLY used by the deleted body is removed.

**Spec sections:** §4.4.

**Per-task summary:**

1. **Identify dead helpers** — anything in `run.rs` that's only called from the now-deleted body. Common candidates: `append_assistant_response`, the inline tool-dispatch helpers. Move them to `stream.rs` if needed; delete if unused.

2. **Promote shared helpers** to `pub(crate)` if they're now called from BOTH `run.rs` (the wrapper) and `stream.rs` (the pump).

3. **Run the EXISTING tests:** `cargo test -p tau-runtime --all-targets`. The 100+ existing run-loop unit/integration tests (including `tool_plugin_e2e.rs`, `capability_override_e2e.rs`, `tool_args_validation_e2e.rs`, etc.) are the regression net. ALL of them must pass.

4. **Add a "pin test"** that scripts a fixed scenario through `run_with_history` and asserts the `RunOutcome` matches a known-good fixture. This is documentation, not regression coverage (the existing tests do that).

5. **Verification (full workspace):**
   ```bash
   cargo build --workspace
   cargo test --workspace --all-targets
   cargo test --workspace --doc
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```

   Expected: ALL PASS. If any existing test fails, surface as a regression; do NOT silently rewrite test expectations.

6. **Commit message:**
   ```
   refactor(runtime): run_with_history → thin stream-drainer over run_streaming_inner

   The bulk of the agent-loop body (~400 LOC at run.rs:111-510)
   moves into stream.rs's run_streaming_inner. run_with_history
   becomes a ~20-line wrapper that drains the stream and returns
   the terminal RunCompleted.outcome.

   Public API (signature, return type, error semantics) is
   byte-identical to today's. The 100+ existing run-loop tests
   pass unchanged — they're the regression net. Plus one new
   pin-test that scripts a fixed scenario for documentation.

   Two source paths collapse into one: streaming and batch share
   the same pump; batch is just streaming + drain.

   Refs: docs/superpowers/specs/2026-04-30-streaming-design.md §4.4
   ```

7. Push.

---

## Task 8: E2E integration test

**Hybrid format.**

**Files:**
- Create: `/Users/titouanlebocq/code/tau/crates/tau-runtime/tests/run_streaming_e2e.rs` — gated `#![cfg(unix)]`.

**Spec sections:** §7 testing tier.

**Per-task summary:**

Mirror the harness from `crates/tau-runtime/tests/tool_plugin_e2e.rs` (priority 3) and `tool_args_validation_e2e.rs` (priority 6):
- `mod common;` for shared test fixtures.
- Copy the in-process `InProcessFsRead` `DynTool` adapter pattern.
- Use `ScriptedFsReadLlm` for two-turn scripted scenarios.

Five test scenarios:

1. **`text_only_run_streams_text_deltas`** — scripted LLM emits `Text("Hi") + Text(" there") + Finish(EndTurn)`. Assert events: `TextDelta("Hi") → TextDelta(" there") → TurnCompleted → RunCompleted { Completed }`.

2. **`tool_use_run_streams_tool_call_started_then_completed`** — scripted LLM emits `ToolUse(fs-read, {path:...}) + Finish(ToolUse)`, then on turn 2 emits `Text("done") + Finish(EndTurn)`. Assert events include `ToolCallStarted` BEFORE any tool dispatch begins, `ToolCallCompleted` after, `TurnCompleted` after both, and `RunCompleted { Completed }` at the end.

3. **`schema_validation_failure_emits_tool_call_completed_with_err`** — scripted LLM emits a tool_use with malformed args (e.g., `{path: 42}` for fs-read). Assert `ToolCallStarted → ToolCallCompleted { result: Err(reason) }` where the reason contains the MANDATORY-rule template substrings ("You sent:", "Expected (input_schema):", "Specific issue").

4. **`capability_denial_terminates_run`** — agent has no fs.read grant; LLM emits a fs-read tool_use. Assert events: `ToolCallStarted → RunCompleted { Failed { PolicyDenied } }`. Note: `ToolCallStarted` fired, but `ToolCallCompleted` did NOT — this is the documented terminal-failure exception.

5. **`max_turns_reached_yields_run_completed_failed_out_of_resources`** — scripted LLM keeps emitting tool_uses turn after turn; configured `max_turns: 2`. Assert run terminates with `RunCompleted { Failed { OutOfResources } }`.

**Verification:** standard 5-command suite + workspace test rollup.

**Commit message:** `test(runtime): streaming run e2e coverage`.

Push.

---

## Task 9: `tau chat` streaming integration

**Hybrid format.**

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/cli.rs` — add `ChatArgs.no_stream: bool`.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/cmd/chat.rs` — REPL turn handler at lines 250-307 consumes the stream when `--no-stream` is unset.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/tests/cmd_chat.rs` — add 2 streaming tests.

**Spec sections:** §4.5.

**Per-task summary:**

1. **Add `--no-stream` flag** to `ChatArgs` in `cli.rs`. Default `false` (streaming on).

2. **In `cmd/chat.rs:250-307`**, branch on `args.no_stream`:
   - **`true`**: call `runtime.run_with_history(...)` (existing batch flow, unchanged).
   - **`false`** (default): call `runtime.run_streaming_with_history(...)`, drain the stream with the two-pass rendering shown in spec §4.5.

3. **Two-pass rendering** (per spec §4.5):
   - During streaming: `print!` text deltas to stdout, flush after each. `eprintln!` tool annotations to stderr.
   - On `RunCompleted { Completed }`: re-render the full assistant text via the existing `render_final_message` helper at `chat.rs:313`.
   - On `RunCompleted { Failed }`: print the error via existing `output.error(...)` path.

4. **Update `history`** with `all_messages` from the terminal `RunCompleted.outcome`.

5. **Update help snapshot:** the `tau chat --help` insta snapshot will need re-acceptance because of the new `--no-stream` flag.

6. **Add 2 integration tests** in `tests/cmd_chat.rs`:
   - `chat_streaming_emits_text_deltas_inline` — scripted LLM via in-process adapter; capture stdout, assert text deltas appeared.
   - `chat_no_stream_flag_disables_streaming` — same scenario with `--no-stream`; assert the existing batch render path runs.

7. **Verification (full workspace):**
   ```bash
   cargo build --workspace
   cargo test --workspace --all-targets
   cargo test --workspace --doc
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```

8. **Commit message:** `feat(cli): tau chat streaming + --no-stream flag`.

9. Push.

---

## Task 10: `tau run --stream` integration

**Hybrid format.**

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/cli.rs` — add `RunArgs.stream: bool`.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/cmd/run.rs` — branch on `args.stream`; consume the stream in human + JSON modes.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/tests/cmd_run.rs` — add 2 `--stream` tests.

**Spec sections:** §4.6.

**Per-task summary:**

1. **Add `--stream` flag** to `RunArgs`. Default `false` (opt-in).

2. **In `cmd/run.rs`**, sequence:
   - Run the existing requires.tools resolve (priority 5) — unchanged.
   - Run the `--dry-run` check — unchanged.
   - Branch on `args.stream`:
     - **`false`**: call `runtime.run_with_history(...)` (existing batch flow, unchanged).
     - **`true`**: call `runtime.run_streaming_with_history(...)`, drain the stream:
       - **Human mode** (`!output.is_json()`): `print!` text deltas to stdout, flush; `eprintln!` tool annotations to stderr; on `RunCompleted` print closing newline and emit existing closing summary.
       - **JSON mode** (`output.is_json()`): one `Output::json(&serde_json::json!({...}))` call per `RunEvent` — the canonical event shape from spec §4.6:
         ```json
         {"event":"text_delta","delta":"..."}
         {"event":"tool_call_started","id":"...","name":"...","args":...}
         {"event":"tool_call_completed","id":"...","name":"...","result":...}
         {"event":"turn_completed","stop_reason":"...","usage":{...},"turn":N}
         {"event":"run_completed","outcome":{...}}
         ```

3. **Helper:** `fn run_event_to_json(event: &RunEvent) -> serde_json::Value` — pure function, easily unit-tested.

4. **Update help snapshot:** `tau run --help` insta snapshot needs re-acceptance for the new `--stream` flag.

5. **Add 2 integration tests** in `tests/cmd_run.rs`:
   - `run_stream_human_mode_emits_text_deltas_inline_to_stdout` — scripted LLM (via assert_cmd + a fixture project tau.toml + a mock LLM backend); `--stream` set; capture stdout, assert text deltas appeared.
   - `run_stream_json_mode_emits_one_event_per_line` — same scenario with `--stream --json`; parse stdout line-by-line, assert each is a valid JSON object with the canonical `event` discriminator.

6. **Verification (full workspace).**

7. **Commit message:** `feat(cli): tau run --stream flag (human + JSON modes)`.

8. Push.

---

## Task 11: Final verification + open PR

**User-driven gate. PAUSE before this task.**

### Steps

- [ ] **Step 11.1: Full local verification**

```bash
cargo build --workspace
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

All must pass. If anything fails, fix it before opening the PR.

- [ ] **Step 11.2: Open the PR (or mark draft → ready)**

```bash
gh pr list --head feat/streaming-spec --json number,state,isDraft
```

If empty, create:

```bash
gh pr create --title "feat: streaming LLM responses (Tier 2 priority 8)" \
  --body "$(cat <<'EOF'
## Summary

Adds `Runtime::run_streaming` and `run_streaming_with_history` — kernel API yielding a `Stream<Item = RunEvent>` as the agent loop progresses. Realizes ADR-0006 §5 deferral closure.

- New `tau-runtime::stream` module with `RunEvent` enum (5 variants) and `run_streaming_inner` async generator (built via `async-stream` crate).
- Existing `run`/`run_with_history` REFACTOR as thin consumers of the new pump (same public API; same behavior; one source of truth for the agent loop).
- `tau chat`: streaming-on-by-default (`--no-stream` opt-out); two-pass rendering (raw `print!` typewriter + termimad re-render on completion).
- `tau run --stream` flag: opt-in; human mode emits text deltas to stdout + tool annotations to stderr; JSON mode emits one `RunEvent` per stdout line.
- New ADR-0011 lands in Task 12.
- Tests: ~12 unit tests for the pump, 5 e2e scenarios via in-process FsReadPlugin, 2 tau chat REPL tests, 2 tau run --stream tests, plus the 100+ existing run-loop tests as regression net.

## Spec / Plan

- Spec: `docs/superpowers/specs/2026-04-30-streaming-design.md`
- Plan: `docs/superpowers/plans/2026-04-30-streaming.md`

## Test plan

- [x] `cargo build --workspace` green
- [x] `cargo test --workspace --all-targets` green
- [x] `cargo test --workspace --doc` green
- [x] `cargo clippy --workspace --all-targets -- -D warnings` green
- [x] `cargo fmt --all -- --check` green
- [ ] CI matrix (23 required checks) green — verifying on push

## Out of scope (deferred)

- Parallel tool dispatch (LLMs go quiet after `tool_use`; parallelism win is narrow). Phase-2 perf work.
- Mid-stream markdown rendering in `tau chat` (hard problem; ship two-pass termimad re-render instead). Future work.
- Cancellation mid-run via `Drop`. Future work.
- SSE export for a future `tau serve`. Owned by that sub-project when it lands.
- Token-usage live updates (currently end-of-turn only).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

If a draft already exists, mark ready: `gh pr ready <number>`.

- [ ] **Step 11.3: Capture PR URL**

```bash
gh pr view --json number,url --jq '{number, url}'
```

- [ ] **Step 11.4: PAUSE — wait for CI green before Task 12**

Use the same Bash + run_in_background poller pattern from priorities 5 + 6 + 7.

---

## Task 12: ADR-0011 + ROADMAP + squash merge

**User-driven gate. PAUSE before this task.**

**Files:**
- Create: `/Users/titouanlebocq/code/tau/docs/decisions/0011-streaming-llm-responses.md` — full ADR (5 sections per spec §6).
- Modify: `/Users/titouanlebocq/code/tau/ROADMAP.md` — mark Tier 2 priority 8 ✅.

### Steps

- [ ] **Step 12.1: Write ADR-0011**

Mirror the structure of ADR-0010 (the most recent ADR). Sections:

1. **API surface: `Stream<Item = RunEvent>`** (Q1-A) — pull-based async iterator.
2. **Event vocabulary: kernel-translated `RunEvent`** (Q2-B) — UI consumers shouldn't learn LLM-protocol semantics.
3. **`ToolCallStarted` fires on LLM emission** (Q3-A) — display intent before dispatch.
4. **Pure `RunEvent` items; errors flow through `RunOutcome::Failed`** (Q4-A) — single error path.
5. **Full CLI scope: kernel + tau chat + tau run --stream** (Q5-A) — bundle UX surfaces with the API.

Plus pump invariants documented (exactly one `RunCompleted` per stream; `ToolCallStarted` paired or terminal-failure-followed; pull-based backpressure; batch wrappers factor as thin consumers).

Trigger to revisit: parallel tool dispatch, mid-stream markdown rendering, async cancellation, mid-stream usage updates, SSE export for `tau serve`.

Status: Accepted, 2026-04-30.

Cross-references: ADR-0006 §5 (the deferral this closes), ADR-0003 §2 (`LlmBackend::stream` trait), ADR-0008 (`llm.stream` IPC wire method), ADR-0009 (typed-error policy), ADR-0010 (schema validation; preserved in the new pump).

- [ ] **Step 12.2: Update ROADMAP**

Find the Tier 2 priority 8 entry in `ROADMAP.md`:

```markdown
8. **Streaming LLM responses** (`Runtime::run_streaming` additive).
```

Replace with:

```markdown
8. **Streaming LLM responses** ✅ Shipped 2026-04-30 — see
   [spec](docs/superpowers/specs/2026-04-30-streaming-design.md)
   and [ADR-0011](docs/decisions/0011-streaming-llm-responses.md).
   New `Runtime::run_streaming` and `run_streaming_with_history`
   yield a `Stream<Item = RunEvent>` as the agent loop progresses.
   Existing `run`/`run_with_history` refactor as thin consumers of
   the new pump (zero behavior change; one source of truth for the
   agent loop). `tau chat` streams by default with two-pass
   termimad rendering; `tau run --stream` opt-in flag for scripts
   (human + JSON modes). No new CI jobs (23 required checks
   unchanged).
```

Add to the top-of-file shipped table:

```markdown
| 8 | Streaming LLM responses ✅ | Tier 2 priority 8 — realizes ADR-0006 §5 deferral closure. New `tau-runtime::stream` module with `RunEvent` enum + `run_streaming_inner` async generator (via `async-stream` crate). Kernel pump translates `CompletionChunk` into higher-level `RunEvent`s (`TextDelta`, `ToolCallStarted`, `ToolCallCompleted`, `TurnCompleted`, `RunCompleted`). `Runtime::run_streaming` + `run_streaming_with_history` public entry points. `Runtime::run`/`run_with_history` refactor as thin consumers of the new pump (zero behavior change; 100+ existing tests pass unchanged). `tau chat` streams by default (`--no-stream` opt-out, two-pass termimad render); `tau run --stream` opt-in flag (human + JSON modes). New ADR-0011. No new CI jobs (23 required checks unchanged). | 2026-04-30 |
```

Update the front-matter narrative paragraph to reflect that priorities 4, 5, 6, AND 8 are closed (priority 7 still open).

- [ ] **Step 12.3: Commit + push**

```bash
git add docs/decisions/0011-streaming-llm-responses.md ROADMAP.md
git commit -m "$(cat <<'EOF'
docs: ADR-0011 + ROADMAP Tier 2 priority 8 done

Locks the 5 design decisions for streaming LLM responses:
1. API: Stream<Item = RunEvent> from Runtime::run_streaming
2. Event vocabulary: kernel-translated higher-level RunEvent
3. ToolCallStarted fires on LLM emission (display intent)
4. Pure RunEvent items; errors via RunOutcome::Failed
5. Full CLI scope: kernel + tau chat + tau run --stream

Updates ROADMAP:
- Top-of-file shipped table gains a row for Tier 2 priority 8.
- Tier 2 priority 8 entry marked ✅ Shipped 2026-04-30.
- Front-matter narrative updates to reflect priorities 4, 5, 6, 8
  closed; priority 7 still open.

No new CI jobs; branch protection stays at 23 required checks.

Refs: docs/superpowers/specs/2026-04-30-streaming-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push
```

- [ ] **Step 12.4: Wait for CI green on the PR**

Same poller pattern as priority 6. 23 required checks must all pass.

- [ ] **Step 12.5: Squash merge**

```bash
gh pr merge --squash --delete-branch
```

- [ ] **Step 12.6: Verify branch protection unchanged**

```bash
gh api repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks/contexts | jq 'length'
```
Expected: `23`.

- [ ] **Step 12.7: Sync local main + report squash SHA**

```bash
git checkout main && git pull && git log --oneline -3
```

Report back to the user with the squash SHA.

---

## Verification standard (per task)

Each task ends with:

```bash
cargo build --workspace
cargo test -p tau-runtime --all-targets
cargo test -p tau-runtime --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

For tasks touching multiple crates (7, 9, 10, 11), use `cargo test --workspace --all-targets` instead.

CI continues on push; no new jobs added; branch protection stays at 23.
