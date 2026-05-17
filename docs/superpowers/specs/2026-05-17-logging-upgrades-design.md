# Logging Upgrades Design

**Date:** 2026-05-17
**Status:** Draft
**Scope:** ROADMAP — observability hardening (post-Skills-6)
**Supersedes:** none. Companion ADR will be drafted at implementation time (likely ADR-0031).

## Goal

Bring tau's structured logging up to the level the kernel already advertises in ADR-0006 §3.9 ("11. `tracing` for structured logs"): a fixed vocabulary of named spans and events with disciplined sensitive-data previews, a single canonical subscriber-init path, and dual-purpose persistence (workflow run logs + plugin protocol recording) flowing through the same `tracing` infrastructure that powers everything else.

## Background

`tracing` is already the workspace standard (`tracing = "0.1"` in workspace deps, `tau-cli` and `tau-plugin-sdk` both install `tracing_subscriber::fmt`). What is missing relative to the spec:

| §3.9 declares | Today |
|---|---|
| Spans: `runtime.agent_run`, `runtime.turn`, `llm.complete`, `dispatch.tool`, `capability.check`, `tool.session_open`, `tool.invoke`, `tool.session_close` | Only `runtime.agent_run` (`run.rs:93`) and `llm.complete` (`stream.rs:196`) exist. |
| ~22 events across 9 subsystems | ~7 are present in `crates/tau-runtime/src/` (mostly in `stream.rs` and `builder.rs`); the rest are aspirational. |
| Sensitive-data discipline: 256-char preview at DEBUG, full content TRACE-only | Documented in `run.rs:24-26` but not enforced by any helper. Several call sites format full message bodies directly. |
| `tracing` as single observability surface | Two parallel JSONL writers exist (`tau-workflow/src/persistence.rs`, `tau-runtime/src/plugin_host/recording.rs`) that bypass `tracing` entirely. |
| `tau-observe` crate as the home for shared observability | `crates/tau-observe/src/lib.rs` contains only a doc comment. Zero deps in its `Cargo.toml`. |

Additionally:

- `tau-cli/src/tracing.rs:47` and `tau-plugin-sdk/src/tracing_layer.rs:19` maintain two near-identical subscriber-install functions.
- Neither `tracing-appender` nor `opentelemetry` appears in any `Cargo.toml` or `Cargo.lock`. There is no log rotation, no non-blocking writer, no OTLP export.
- ~12 `println!`/`eprintln!` calls in `tau-pkg/src/install.rs`, `tau-cli/src/cmd/run.rs`, `tau-cli/src/cmd/chat.rs`, and `tau-sandbox-native/` mix diagnostic output with intended CLI output. Some are user-facing CLI text and must stay; others are diagnostics that should move to `tracing`.

## Non-goals

- **Redaction enforced on plugins or callers.** ADR-0006 NG9 is explicit: "tau does not redact for the caller". The kernel's redaction discipline applies to events the kernel itself emits. Plugin authors get helpers but no forced policy.
- **A new persistence file format.** Workflow `StepRecord` JSONL and plugin-protocol JSONL keep their current on-disk shapes — only the *writer* changes.
- **Per-run sampling, head/tail sampling, or trace-id propagation across processes.** Single-process observability only at this stage; cross-process is a follow-on once OTLP is wired.
- **A web UI for traces.** Out of scope; OTLP export lets the user point at Jaeger/Tempo if they want one.
- **Replacing `tracing` with another framework.** `tracing` stays.

## Architecture

The design is six sub-projects that can ship independently and in approximately the order listed. Each is small enough for one PR.

### Sub-project A: `tau-observe` becomes the canonical subscriber/init crate

`tau-observe` is currently a stub. Promote it to the home for:

- `tau_observe::install(InstallOptions)` — the single subscriber-builder used by `tau-cli` and `tau-plugin-sdk`. Replaces `tau_cli::tracing::install` and `tau_plugin_sdk::tracing_layer::install`. `InstallOptions` carries: `filter: EnvFilter`, `format: Format::{Human, Json}`, `writer: Writer::{Stderr, Stdout, File(PathBuf)}`, `non_blocking: bool` (gated on Sub-project E feature), `otlp: Option<OtlpEndpoint>` (gated on Sub-project F feature).
- `tau_observe::filter::build_from_env_and_flags` — moves `tau_cli::tracing::build_filter` into the shared crate; `tau-cli` re-exports for its CLI surface but the logic lives here.
- `tau_observe::preview` — the redaction helpers used by Sub-project C.
- `tau_observe::vocabulary` — string constants for every span name and event name in §3.9. Macro call sites import `tau_observe::vocabulary as v;` and use `v::SPAN_RUNTIME_TURN`, `v::EV_LLM_REQUEST_BUILT`, etc., instead of stringly-typed names. Prevents drift; `grep` for span/event renames becomes trivial.

Cargo.toml adds: `tracing = { workspace = true }`, `tracing-subscriber = { workspace = true, features = ["fmt", "env-filter", "json"] }`. Sub-projects E and F add optional features.

Both `tau-cli/Cargo.toml` and `tau-plugin-sdk/Cargo.toml` gain `tau-observe = { workspace = true }`. Their existing `tracing_subscriber` dependencies remain (needed for re-exporting `EnvFilter` in tests) but the duplicated `install` functions delete.

### Sub-project B: Fill in the §3.9 span vocabulary

Add the missing spans to the kernel:

| Span | Location to add | Fields |
|---|---|---|
| `runtime.turn` | `stream.rs:181` (inside the `while total_turns < options.max_turns` loop) | `turn_index`, `messages_len` |
| `dispatch.tool` | wraps tool resolution + invocation in the streaming pump | `tool_name`, `tool_use_id` |
| `capability.check` | wraps the cap-check call before each tool invocation | `tool_name`, `required_caps`, `granted_caps_count` |
| `tool.session_open` | wraps the plugin-host `Open` request path | `tool_name`, `session_id` |
| `tool.invoke` | wraps the plugin-host `Invoke` request path | `tool_name`, `session_id`, `msgid` |
| `tool.session_close` | wraps the plugin-host `Close` request path | `tool_name`, `session_id` |

All spans use `#[instrument]` or `.instrument(info_span!(...))` (matching the style already established in `stream.rs:196`). All span names import from `tau_observe::vocabulary`.

Also add the missing events. The audit identified 7 events present today; §3.9 lists ~22. The gap (verbatim from §3.9):

```
runtime.completed, runtime.failed, runtime.max_turns_reached
llm.request_built, llm.response_received, llm.token_usage,
  llm.stop_reason, llm.tool_use_emitted
dispatch.tool_resolved
capability.required_loaded, capability.granted_loaded,
  capability.satisfies_check, capability.allow, capability.deny
tool.args_received, tool.result_received, tool.invoke_failed,
  tool.session_open_failed, tool.session_close_failed
message.added
```

Each event is emitted with `tracing::info!` or `tracing::debug!` per the level conventions in §3.9, structured fields only (no string interpolation of variable data into the message — the message is a fixed string, all variable data lives in `key = value` fields).

### Sub-project C: Sensitive-data preview helpers

A new module `tau_observe::preview` exposes two explicit helpers — discipline applies at the call site, not by reading the effective filter level (`tracing` does not cheaply expose the effective level per call; runtime-detecting it would defeat the macros' `enabled!` short-circuit).

```rust
/// Render a value with at most a 256-byte preview, ending at a UTF-8
/// boundary. Use at DEBUG (and below) callsites for argument / payload /
/// message content.
pub fn preview(value: &str) -> impl Display + '_;
pub fn preview_json(value: &serde_json::Value) -> impl Display + '_;

/// Render a value in full. Use ONLY at TRACE callsites; the macro layer
/// drops the entire event when the filter excludes TRACE, so the cost of
/// the full format is never paid unless the user explicitly opted into
/// TRACE. Calling this at DEBUG or above is a lint violation (see below).
pub fn full(value: &str) -> impl Display + '_;
pub fn full_json(value: &serde_json::Value) -> impl Display + '_;
```

Call sites that currently format full message bodies (e.g. tool args, message payloads, LLM responses) are migrated:

```rust
// before:
tracing::debug!(args = %args_json, "tool.args_received");

// after — DEBUG callsite, must preview:
use tau_observe::preview::preview_json;
tracing::debug!(args = %preview_json(&args_json), "tool.args_received");

// after — TRACE callsite, full content allowed:
use tau_observe::preview::full_json;
tracing::trace!(args = %full_json(&args_json), "tool.args_received_full");
```

A clippy-style lint or a workspace-level `deny(...)` rule is out of scope for v1; enforcement is by code review against the rule "`preview::full*` only appears inside `tracing::trace!`". Sub-project B's test suite asserts that no DEBUG-or-above event in the §3.9 vocabulary references `full*` helpers (`grep`-based smoke test in CI).

This is **kernel-internal discipline only** (per NG9 the kernel does not police callers). Plugins authored against `tau-plugin-sdk` may opt in by importing the helper; we document it in the rustdoc but do not force it.

### Sub-project D: Workflow + protocol persistence as `tracing` Layers

Both existing JSONL writers become custom `tracing_subscriber::Layer` impls living in `tau-observe`:

- `tau_observe::layers::WorkflowRunLogLayer` — subscribes to events tagged `target = "tau::workflow::step"` and writes one `StepRecord` JSONL line per event. The existing `tau_workflow::persistence::RunLog` becomes a thin internal writer used by the layer. The `tau workflow run` command installs this layer on its tracing stack instead of constructing a `RunLog` directly.
- `tau_observe::layers::PluginRecordingLayer` — subscribes to events tagged `target = "tau::plugin::frame"` and writes one frame JSONL line per event. `tau-runtime::plugin_host::recording::Recorder` becomes an internal writer used by the layer; the public `Recorder` API stays for backward compat but its `record()` method becomes a thin wrapper that emits a `tracing::event!` instead of writing directly.

On-disk file format is **unchanged**. Behavior change: when these layers are active, the same events are simultaneously visible to any other subscriber the user has installed (e.g. the human-readable stderr fmt layer at TRACE, or a JSON layer for log shipping). That collapses three observability surfaces (run-log file, recording file, tracing) into one.

The `info_span!("llm.complete")` already in `stream.rs:196` and the new spans from Sub-project B become the natural parents for protocol-frame events, so a single trace shows `runtime.agent_run > runtime.turn > llm.complete > [protocol frames]` with no extra plumbing.

### Sub-project E: Non-blocking writer + rotation via `tracing-appender`

Add `tracing-appender = "0.2"` as an optional dependency on `tau-observe`, feature `non_blocking`. `InstallOptions::non_blocking = true` (or `--log-non-blocking` CLI flag, or env `TAU_LOG_NON_BLOCKING=1`) routes the writer through `tracing_appender::non_blocking::NonBlocking`. `Writer::File` uses `tracing_appender::rolling::daily` (or `Rotation::Never` when the user passes a fixed path with no rotation hint).

This replaces the hand-rolled tokio `Mutex<File>` in `recording.rs` with appender's lock-free MPSC channel. Backpressure: if the channel fills (slow consumer), events drop and a warning is emitted on a separate rate-limited channel — appender's default behavior, suitable for tau because losing a few log lines under load is better than blocking the agent run.

### Sub-project F: Optional OpenTelemetry export

Add `opentelemetry = "0.21"`, `opentelemetry-otlp = "0.14"`, `tracing-opentelemetry = "0.22"` as optional deps on `tau-observe`, feature `otlp`. `InstallOptions::otlp = Some(OtlpEndpoint { endpoint, headers, protocol })` adds an `OpenTelemetryLayer` to the registry.

CLI surface: `tau --otlp-endpoint=https://otel.example.com:4317 run …`, or env `OTEL_EXPORTER_OTLP_ENDPOINT` (standard OTel env var). Off by default. When on, every span emitted by Sub-project B becomes an OTel span, and the agent orchestration patterns from PR #60 (supervisor/critic/hierarchical) become first-class distributed traces.

Resource attributes auto-populated: `service.name = "tau"`, `service.version = env!("CARGO_PKG_VERSION")`. The user adds anything else via `OTEL_RESOURCE_ATTRIBUTES` (standard).

## Data flow

```
                                  ┌────────────────────────────────────┐
                                  │  tau-cli / tau-plugin-sdk          │
                                  │       ↓ tau_observe::install(opts) │
                                  └─────────────────┬──────────────────┘
                                                    │
                                                    ▼
                              tracing_subscriber::Registry
                                                    │
       ┌────────────────────┬───────────────────────┼────────────────────┬─────────────────────┐
       ▼                    ▼                       ▼                    ▼                     ▼
  fmt::Layer         WorkflowRunLogLayer   PluginRecordingLayer    OpenTelemetryLayer    (others)
  (stderr,           (.tau/workflow-runs/   (debug-tier .jsonl     (gRPC OTLP, opt-in)
   human/json,       <name>-<id>.jsonl)     when recording flag                          (e.g. Loki,
   non-blocking via                          is set)                                       Tempo)
   appender)
       │                    │                       │                    │
       └────────────────────┴──────── one event stream from kernel ──────┘
```

The kernel (in `tau-runtime`) emits events with structured fields and the §3.9 vocabulary. The subscriber registry decides what to do with each event based on `target`/level/fields. Replacing or adding a sink does not require touching kernel code.

## Error handling

- **Subscriber install failures** (e.g. file path not writable) — `tau_observe::install` returns `Result<InstallGuard, InstallError>`. The guard's drop flushes the non-blocking writer. `tau-cli` prints the error to stderr and exits 2.
- **Layer write failures** (workflow JSONL, recording JSONL) — keep current behavior: log at WARN to the *parent* subscriber, do not propagate. The runtime continues even if the file is full or permission-denied.
- **OTLP export failures** — `opentelemetry-otlp`'s background batcher logs failures at WARN. Failure to reach the collector never blocks or fails an agent run.
- **NonBlocking channel saturation** — appender's default is to drop with a warning. We accept that; under sustained load it's the right tradeoff.

## Testing

Each sub-project ships with tests in the convention already established in the workspace (`#[test]` for unit, `tests/*.rs` for integration):

- **A** — `tau_observe::install` unit tests for each `InstallOptions` permutation; assert the resulting subscriber has the expected layers.
- **B** — for every new span, an integration test in `tau-runtime/tests/` that runs a minimal agent through a `MockLlmBackend` (the fixture from Skills-4) and asserts the expected sequence of span enter/exit events on a `tracing-test`-style capture subscriber. For every new event, a focused unit test that asserts the event fires with the expected fields and level.
- **C** — `tau_observe::preview` unit tests covering: short string (no truncation), long string (256-byte truncation at UTF-8 boundary), TRACE level passes through full, multi-byte UTF-8 edge cases (3- and 4-byte codepoints straddling byte 256).
- **D** — port the existing `tau-workflow/src/persistence.rs` tests and `tau-runtime/src/plugin_host/recording.rs` tests to drive the new layers; on-disk output must be byte-for-byte identical to the legacy writer for the same input sequence.
- **E** — integration test that emits 100k events under load and asserts none of them block the producer for >10ms (appender's contract).
- **F** — integration test using `opentelemetry-stdout` exporter (no network); assert the span graph for a minimal `runtime.agent_run > runtime.turn > llm.complete` trace matches expectation.

CI: nextest already runs everywhere except doctests (per CLAUDE.md). New optional features add CI jobs `cargo test -p tau-observe --features non_blocking` and `cargo test -p tau-observe --features otlp` to the existing matrix.

## Migration plan (rollout order)

1. **A** ships first — no behavior change for users, just consolidation. `tau-cli` and `tau-plugin-sdk` keep working byte-identically through their wrappers.
2. **B** ships next — adds spans/events; existing users see strictly more telemetry under their existing `RUST_LOG` filters.
3. **C** ships alongside or after B — kernel-internal redaction at all the call sites B touched.
4. **D** ships after A+B+C land — the consolidating step. Cuts code, doesn't change file formats.
5. **E** and **F** are feature-gated and can ship in either order. Both off-by-default.

Each sub-project is one PR. ADR-0031 lands with sub-project A and references the spec.

## Open questions

None at design time. Each sub-project's implementation plan will surface specifics.

## Out-of-scope follow-ons (noted for future, not part of this design)

- Cross-process trace propagation (W3C traceparent) — needs design once plugin-host launches OS processes.
- Per-agent log files (one file per `runtime.agent_run` span) — useful for debugging but commits to a file-naming policy.
- Log sampling — only worth doing once OTLP export proves volume is a problem.
- Migration of the ~12 `println!`/`eprintln!` diagnostic sites — straightforward but mechanical; can ship as a follow-up cleanup PR after B lands.
