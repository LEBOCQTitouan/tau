# Logging Sub-project A — `tau-observe` Canonical Init Crate

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `tau-observe` the one place that builds and installs the tracing subscriber, and absorb the duplicated install logic from `tau-cli` and `tau-plugin-sdk`.

**Architecture:** `tau-observe` exposes `install(opts) -> Result<InstallGuard, InstallError>` plus a `vocabulary` module of `&'static str` constants for every §3.9 span/event name. `tau-cli` keeps a thin CLI-specific filter builder that maps `clap` flags onto `EnvFilter`, then calls `tau_observe::install`. `tau-plugin-sdk::tracing_layer::install` becomes a one-line delegate to `tau_observe::install(Format::Json)`.

**Tech Stack:** Rust 2021, `tracing = "0.1"`, `tracing-subscriber = "0.3"` (features: `fmt`, `env-filter`, `json`), `thiserror`. No new transitive deps.

**Cargo rules:** This plan obeys `CLAUDE.md`. Every `cargo` invocation in this plan uses the shape `timeout <T> env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo <verb> -p <crate>`. The `agent-impl` slug is conventional for execution sessions; substitute another role-specific slug if your runner uses one.

---

## File Structure

**Created:**
- `crates/tau-observe/src/install.rs` — `InstallOptions`, `Format`, `Writer`, `install`, `InstallError`, `InstallGuard`.
- `crates/tau-observe/src/filter.rs` — `EnvFilter` builder helpers shared by all callers.
- `crates/tau-observe/src/vocabulary.rs` — `&'static str` constants for every §3.9 span and event name. Stubs here (one constant per name); sub-projects B/C fill the rest of the work that *uses* these constants.
- `crates/tau-observe/tests/install_smoke.rs` — integration test asserting `install` returns a working guard for each `Format`/`Writer` permutation.

**Modified:**
- `Cargo.toml` (workspace) — add `tau-observe = { path = ..., version = "0.0.0" }` under `[workspace.dependencies]`, and `tracing-subscriber = { version = "0.3", features = ["env-filter"] }` (so all crates inherit the same baseline).
- `crates/tau-observe/Cargo.toml` — gain `tracing`, `tracing-subscriber` (with `fmt`, `env-filter`, `json`), `thiserror` dependencies.
- `crates/tau-observe/src/lib.rs` — module decls + re-exports.
- `crates/tau-cli/Cargo.toml` — add `tau-observe = { workspace = true }`.
- `crates/tau-cli/src/tracing.rs` — `install(cli)` delegates to `tau_observe::install`; `build_filter(cli)` keeps its CLI-flag mapping but builds the final `EnvFilter` via `tau_observe::filter::env_or_directive`.
- `crates/tau-plugin-sdk/Cargo.toml` — add `tau-observe = { workspace = true }`.
- `crates/tau-plugin-sdk/src/tracing_layer.rs` — `install()` delegates to `tau_observe::install(InstallOptions::plugin_sdk())`.

**Untouched (verified at end):**
- Every call site of `tau_cli::tracing::install(&cli)` (1 in `tau-cli/src/lib.rs:35`).
- Every call site of `tracing_layer::install()` in `tau-plugin-sdk/src/runners/{tool,storage,llm_backend}.rs` (6 call sites).

---

## Task 1: Workspace plumbing — add `tau-observe` and `tracing-subscriber` to workspace deps

**Files:**
- Modify: `Cargo.toml` (workspace root, `[workspace.dependencies]` block at line 44)

- [ ] **Step 1: Add the two workspace dependencies**

In `Cargo.toml`, insert two lines into `[workspace.dependencies]`. Insert `tau-observe` next to the other `tau-*` deps (line 58 area, after `tau-workflow`), and `tracing-subscriber` next to `tracing` (line 78 area).

```toml
# After line 58 (under the tau-* block):
tau-observe             = { path = "crates/tau-observe",             version = "0.0.0" }

# After the line `tracing = "0.1"` (line 78):
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

- [ ] **Step 2: Verify workspace still parses**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo metadata --format-version 1 --no-deps > /dev/null`
Expected: exit 0, no output.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "build(deps): add tau-observe + tracing-subscriber as workspace deps"
```

---

## Task 2: `tau-observe` Cargo.toml dependencies

**Files:**
- Modify: `crates/tau-observe/Cargo.toml`

- [ ] **Step 1: Add the dependencies block**

Replace the entire contents of `crates/tau-observe/Cargo.toml` with:

```toml
[package]
name = "tau-observe"
description = "Observability primitives for tau (structured logging, tracing)."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true

[dependencies]
tracing            = { workspace = true }
tracing-subscriber = { workspace = true, features = ["fmt", "env-filter", "json"] }
thiserror          = { workspace = true }

[features]
default = []
```

- [ ] **Step 2: Verify the crate still compiles (it has no code yet — only `lib.rs` doc comment)**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo check -p tau-observe`
Expected: warnings only (unused deps), no errors.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-observe/Cargo.toml
git commit -m "build(tau-observe): declare tracing / tracing-subscriber / thiserror deps"
```

---

## Task 3: `tau-observe::vocabulary` — span/event name constants

**Files:**
- Create: `crates/tau-observe/src/vocabulary.rs`
- Modify: `crates/tau-observe/src/lib.rs`

This task seeds the constants in their final shape so sub-projects B/C can begin importing them immediately. Filling the implementation that *uses* each constant is sub-project B.

- [ ] **Step 1: Write the failing test**

Create `crates/tau-observe/src/vocabulary.rs` with only this test for now to drive the constant naming:

```rust
//! Fixed `&'static str` names for every span and event in ADR-0006 §3.9.
//!
//! Importing from this module instead of writing string literals keeps
//! the §3.9 vocabulary discoverable by `grep` and prevents drift when
//! sub-projects B/C wire the actual emit sites.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_match_adr_0006_section_3_9() {
        assert_eq!(SPAN_RUNTIME_AGENT_RUN, "runtime.agent_run");
        assert_eq!(SPAN_RUNTIME_TURN, "runtime.turn");
        assert_eq!(SPAN_LLM_COMPLETE, "llm.complete");
        assert_eq!(SPAN_DISPATCH_TOOL, "dispatch.tool");
        assert_eq!(SPAN_CAPABILITY_CHECK, "capability.check");
        assert_eq!(SPAN_TOOL_SESSION_OPEN, "tool.session_open");
        assert_eq!(SPAN_TOOL_INVOKE, "tool.invoke");
        assert_eq!(SPAN_TOOL_SESSION_CLOSE, "tool.session_close");
    }

    #[test]
    fn runtime_events_match_adr_0006_section_3_9() {
        assert_eq!(EV_RUNTIME_RUN_STARTED, "runtime.run_started");
        assert_eq!(EV_RUNTIME_COMPLETED, "runtime.completed");
        assert_eq!(EV_RUNTIME_FAILED, "runtime.failed");
        assert_eq!(EV_RUNTIME_LOOP_TERMINATED, "runtime.loop_terminated");
        assert_eq!(EV_RUNTIME_MAX_TURNS_REACHED, "runtime.max_turns_reached");
        assert_eq!(EV_RUNTIME_TURN_STARTED, "runtime.turn_started");
    }

    #[test]
    fn llm_events_match_adr_0006_section_3_9() {
        assert_eq!(EV_LLM_REQUEST_BUILT, "llm.request_built");
        assert_eq!(EV_LLM_RESPONSE_RECEIVED, "llm.response_received");
        assert_eq!(EV_LLM_TOKEN_USAGE, "llm.token_usage");
        assert_eq!(EV_LLM_STOP_REASON, "llm.stop_reason");
        assert_eq!(EV_LLM_TOOL_USE_EMITTED, "llm.tool_use_emitted");
    }

    #[test]
    fn dispatch_events_match_adr_0006_section_3_9() {
        assert_eq!(EV_DISPATCH_TOOL_RESOLVED, "dispatch.tool_resolved");
    }

    #[test]
    fn capability_events_match_adr_0006_section_3_9() {
        assert_eq!(EV_CAPABILITY_REQUIRED_LOADED, "capability.required_loaded");
        assert_eq!(EV_CAPABILITY_GRANTED_LOADED, "capability.granted_loaded");
        assert_eq!(EV_CAPABILITY_SATISFIES_CHECK, "capability.satisfies_check");
        assert_eq!(EV_CAPABILITY_ALLOW, "capability.allow");
        assert_eq!(EV_CAPABILITY_DENY, "capability.deny");
    }

    #[test]
    fn tool_events_match_adr_0006_section_3_9() {
        assert_eq!(EV_TOOL_ARGS_RECEIVED, "tool.args_received");
        assert_eq!(EV_TOOL_RESULT_RECEIVED, "tool.result_received");
        assert_eq!(EV_TOOL_INVOKE_FAILED, "tool.invoke_failed");
        assert_eq!(EV_TOOL_SESSION_OPEN_FAILED, "tool.session_open_failed");
        assert_eq!(EV_TOOL_SESSION_CLOSE_FAILED, "tool.session_close_failed");
    }

    #[test]
    fn message_events_match_adr_0006_section_3_9() {
        assert_eq!(EV_MESSAGE_ADDED, "message.added");
    }
}
```

Add `pub mod vocabulary;` to `crates/tau-observe/src/lib.rs` (after the `//!` doc comment, before the existing end-of-file):

```rust
// crates/tau-observe/src/lib.rs

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Observability primitives for tau: structured logging, tracing, and
//! the "observe" verb of the four-verb core (G1).

pub mod vocabulary;
```

- [ ] **Step 2: Run the tests, confirm they fail**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --lib vocabulary::`
Expected: compile error — constants are not defined.

- [ ] **Step 3: Add the constants**

At the top of `crates/tau-observe/src/vocabulary.rs`, *above* the `#[cfg(test)] mod tests {`, add:

```rust
// --- Spans (§3.9) ---

/// Span around an entire agent run (`Runtime::run_with_history`).
pub const SPAN_RUNTIME_AGENT_RUN: &str = "runtime.agent_run";
/// Span around one turn of the agent loop.
pub const SPAN_RUNTIME_TURN: &str = "runtime.turn";
/// Span around an LLM completion call.
pub const SPAN_LLM_COMPLETE: &str = "llm.complete";
/// Span around the tool-dispatch decision and invocation.
pub const SPAN_DISPATCH_TOOL: &str = "dispatch.tool";
/// Span around a capability check before a tool invocation.
pub const SPAN_CAPABILITY_CHECK: &str = "capability.check";
/// Span around a tool plugin's `Open` request path.
pub const SPAN_TOOL_SESSION_OPEN: &str = "tool.session_open";
/// Span around a tool plugin's `Invoke` request path.
pub const SPAN_TOOL_INVOKE: &str = "tool.invoke";
/// Span around a tool plugin's `Close` request path.
pub const SPAN_TOOL_SESSION_CLOSE: &str = "tool.session_close";

// --- Runtime events ---

/// Emitted when a run begins.
pub const EV_RUNTIME_RUN_STARTED: &str = "runtime.run_started";
/// Emitted when a run terminates normally.
pub const EV_RUNTIME_COMPLETED: &str = "runtime.completed";
/// Emitted when a run terminates abnormally (status = Failed).
pub const EV_RUNTIME_FAILED: &str = "runtime.failed";
/// Emitted when the run loop exits without producing tool calls.
pub const EV_RUNTIME_LOOP_TERMINATED: &str = "runtime.loop_terminated";
/// Emitted when the run loop hits `RunOptions::max_turns`.
pub const EV_RUNTIME_MAX_TURNS_REACHED: &str = "runtime.max_turns_reached";
/// Emitted at the start of each turn inside the run loop.
pub const EV_RUNTIME_TURN_STARTED: &str = "runtime.turn_started";

// --- LLM events ---

/// Emitted after the kernel builds a `CompletionRequest` for the backend.
pub const EV_LLM_REQUEST_BUILT: &str = "llm.request_built";
/// Emitted after the backend returns a completion.
pub const EV_LLM_RESPONSE_RECEIVED: &str = "llm.response_received";
/// Emitted with the token-usage fields parsed from the response.
pub const EV_LLM_TOKEN_USAGE: &str = "llm.token_usage";
/// Emitted with the stop-reason field parsed from the response.
pub const EV_LLM_STOP_REASON: &str = "llm.stop_reason";
/// Emitted for each `ToolUse` block the LLM emitted on this turn.
pub const EV_LLM_TOOL_USE_EMITTED: &str = "llm.tool_use_emitted";

// --- Dispatch events ---

/// Emitted after the kernel resolves a `tool_use` to a registered plugin.
pub const EV_DISPATCH_TOOL_RESOLVED: &str = "dispatch.tool_resolved";

// --- Capability events ---

/// Emitted after the kernel loads the `required_capabilities` for a tool.
pub const EV_CAPABILITY_REQUIRED_LOADED: &str = "capability.required_loaded";
/// Emitted after the kernel loads the agent's `granted_capabilities`.
pub const EV_CAPABILITY_GRANTED_LOADED: &str = "capability.granted_loaded";
/// Emitted after the kernel computes the satisfies-check result.
pub const EV_CAPABILITY_SATISFIES_CHECK: &str = "capability.satisfies_check";
/// Emitted on the allow branch of the capability check.
pub const EV_CAPABILITY_ALLOW: &str = "capability.allow";
/// Emitted on the deny branch of the capability check.
pub const EV_CAPABILITY_DENY: &str = "capability.deny";

// --- Tool events ---

/// Emitted when the kernel forwards args to the tool plugin.
pub const EV_TOOL_ARGS_RECEIVED: &str = "tool.args_received";
/// Emitted when the kernel receives an Invoke response from the plugin.
pub const EV_TOOL_RESULT_RECEIVED: &str = "tool.result_received";
/// Emitted when the kernel observes a tool Invoke failure.
pub const EV_TOOL_INVOKE_FAILED: &str = "tool.invoke_failed";
/// Emitted when a tool's `Open` request returns an error.
pub const EV_TOOL_SESSION_OPEN_FAILED: &str = "tool.session_open_failed";
/// Emitted when a tool's `Close` request returns an error.
pub const EV_TOOL_SESSION_CLOSE_FAILED: &str = "tool.session_close_failed";

// --- Message events ---

/// Emitted when a message is appended to the run history.
pub const EV_MESSAGE_ADDED: &str = "message.added";
```

- [ ] **Step 4: Run the tests, confirm they pass**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --lib vocabulary::`
Expected: 7 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-observe/src/vocabulary.rs crates/tau-observe/src/lib.rs
git commit -m "feat(tau-observe): expose §3.9 span + event name constants"
```

---

## Task 4: `tau-observe::filter` — shared `EnvFilter` helpers

**Files:**
- Create: `crates/tau-observe/src/filter.rs`
- Modify: `crates/tau-observe/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/tau-observe/src/filter.rs`:

```rust
//! Shared `EnvFilter` builders.
//!
//! Every tau binary or library that initializes a tracing subscriber
//! goes through these helpers so the resolution order (RUST_LOG > caller
//! directive > default) is identical everywhere.

use tracing_subscriber::filter::EnvFilter;

/// Build an `EnvFilter` from the `RUST_LOG` env var if set, otherwise
/// from the `fallback` directive (e.g. `"tau=info"`).
///
/// `RUST_LOG` is parsed verbatim. The fallback is *not* a default for a
/// missing var key — it is the entire filter, used only when `RUST_LOG`
/// is unset.
pub fn env_or_directive(fallback: &str) -> EnvFilter {
    if let Ok(env) = std::env::var("RUST_LOG") {
        return EnvFilter::new(env);
    }
    EnvFilter::new(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn rust_log_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn fallback_used_when_rust_log_unset() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let f = env_or_directive("tau=info");
        assert!(f.to_string().contains("tau=info"), "got: {f}");
    }

    #[test]
    fn rust_log_overrides_fallback() {
        let _g = rust_log_lock();
        std::env::set_var("RUST_LOG", "my_plugin=trace");
        let f = env_or_directive("tau=info");
        assert!(f.to_string().contains("my_plugin=trace"), "got: {f}");
        std::env::remove_var("RUST_LOG");
    }

    #[test]
    fn empty_rust_log_still_overrides() {
        let _g = rust_log_lock();
        std::env::set_var("RUST_LOG", "");
        let f = env_or_directive("tau=info");
        // EnvFilter::new("") yields an empty filter — that's the intent
        // when the user explicitly clears RUST_LOG.
        assert!(!f.to_string().contains("tau=info"), "got: {f}");
        std::env::remove_var("RUST_LOG");
    }
}
```

Add `pub mod filter;` to `crates/tau-observe/src/lib.rs`:

```rust
pub mod filter;
pub mod vocabulary;
```

- [ ] **Step 2: Run the tests**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --lib filter::`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-observe/src/filter.rs crates/tau-observe/src/lib.rs
git commit -m "feat(tau-observe): shared EnvFilter env_or_directive helper"
```

---

## Task 5: `tau-observe::install` — the canonical subscriber installer

**Files:**
- Create: `crates/tau-observe/src/install.rs`
- Create: `crates/tau-observe/tests/install_smoke.rs`
- Modify: `crates/tau-observe/src/lib.rs`

- [ ] **Step 1: Write the unit + integration tests first**

Create `crates/tau-observe/src/install.rs`:

```rust
//! Canonical tracing-subscriber installer.
//!
//! Two install paths supported at v1: human-readable to stderr (CLI),
//! and JSON to stderr (plugin SDK). Both go through [`install`] so the
//! filter-resolution and idempotency behavior are identical.

use std::sync::{Mutex, OnceLock};
use thiserror::Error;
use tracing_subscriber::filter::EnvFilter;

/// Output format for the fmt layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Human-readable (timestamp + level + target + fields + message).
    Human,
    /// JSON Lines, one event per line.
    Json,
}

/// Where the subscriber writes serialized events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writer {
    /// Standard error.
    Stderr,
    /// Standard output.
    Stdout,
}

/// All knobs the canonical installer accepts.
#[derive(Debug)]
pub struct InstallOptions {
    /// Filter to apply. Build via `tau_observe::filter::env_or_directive`.
    pub filter: EnvFilter,
    /// Serialization format.
    pub format: Format,
    /// Sink.
    pub writer: Writer,
}

impl InstallOptions {
    /// Default options for the `tau` CLI: human format on stderr,
    /// `tau=info` fallback filter.
    pub fn cli_default() -> Self {
        Self {
            filter: crate::filter::env_or_directive("tau=info"),
            format: Format::Human,
            writer: Writer::Stderr,
        }
    }

    /// Default options for plugins authored against `tau-plugin-sdk`:
    /// JSON to stderr (read by the host), `info` fallback filter.
    pub fn plugin_sdk() -> Self {
        Self {
            filter: crate::filter::env_or_directive("info"),
            format: Format::Json,
            writer: Writer::Stderr,
        }
    }
}

/// Errors from [`install`].
#[derive(Debug, Error)]
pub enum InstallError {
    /// A subscriber is already installed in this process and the global
    /// init was attempted a second time. Calls that want idempotent
    /// install go through [`install`] (which short-circuits) — this
    /// error is reserved for explicit `install_unique`-style entry
    /// points that may be added later.
    #[error("a tracing subscriber is already installed for this process")]
    AlreadyInstalled,
}

/// Guard returned by [`install`]. Drop runs after-effects (currently
/// none; reserved for sub-project E's non-blocking writer flush).
#[derive(Debug)]
pub struct InstallGuard {
    _private: (),
}

static INSTALL_ONCE: OnceLock<Mutex<bool>> = OnceLock::new();

/// Install the global tracing subscriber. Idempotent: subsequent calls
/// after a successful install are no-ops that return a fresh guard
/// without re-installing.
pub fn install(opts: InstallOptions) -> Result<InstallGuard, InstallError> {
    let cell = INSTALL_ONCE.get_or_init(|| Mutex::new(false));
    let mut installed = cell.lock().unwrap_or_else(|p| p.into_inner());
    if *installed {
        return Ok(InstallGuard { _private: () });
    }

    use tracing_subscriber::fmt;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let registry = tracing_subscriber::registry().with(opts.filter);

    let result = match (opts.format, opts.writer) {
        (Format::Human, Writer::Stderr) => registry
            .with(fmt::layer().with_writer(std::io::stderr))
            .try_init(),
        (Format::Human, Writer::Stdout) => registry
            .with(fmt::layer().with_writer(std::io::stdout))
            .try_init(),
        (Format::Json, Writer::Stderr) => registry
            .with(
                fmt::layer()
                    .json()
                    .with_writer(std::io::stderr)
                    .with_current_span(true)
                    .with_span_list(false),
            )
            .try_init(),
        (Format::Json, Writer::Stdout) => registry
            .with(
                fmt::layer()
                    .json()
                    .with_writer(std::io::stdout)
                    .with_current_span(true)
                    .with_span_list(false),
            )
            .try_init(),
    };

    match result {
        Ok(()) => {
            *installed = true;
            Ok(InstallGuard { _private: () })
        }
        // `try_init` returns Err when a subscriber is already installed.
        // We treat that as success because another part of the process
        // (e.g. a foreign test harness) has already initialized one.
        // The guard the caller receives is a no-op.
        Err(_) => {
            *installed = true;
            Ok(InstallGuard { _private: () })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_default_uses_human_stderr() {
        let opts = InstallOptions::cli_default();
        assert_eq!(opts.format, Format::Human);
        assert_eq!(opts.writer, Writer::Stderr);
    }

    #[test]
    fn plugin_sdk_uses_json_stderr() {
        let opts = InstallOptions::plugin_sdk();
        assert_eq!(opts.format, Format::Json);
        assert_eq!(opts.writer, Writer::Stderr);
    }

    #[test]
    fn install_is_idempotent() {
        // Two installs in the same test binary must both succeed.
        let _g1 = install(InstallOptions::cli_default()).unwrap();
        let _g2 = install(InstallOptions::cli_default()).unwrap();
    }
}
```

Create `crates/tau-observe/tests/install_smoke.rs`:

```rust
//! Smoke test: each combination of Format + Writer at least *parses*
//! and returns a guard. Cannot assert on the registry state directly
//! (tracing-subscriber doesn't expose it), so this is a minimum-bar
//! check that the global init path doesn't panic.

use tau_observe::install::{install, Format, InstallOptions, Writer};
use tau_observe::filter::env_or_directive;

#[test]
fn each_format_writer_combination_installs_without_panic() {
    let combos = [
        (Format::Human, Writer::Stderr),
        (Format::Human, Writer::Stdout),
        (Format::Json, Writer::Stderr),
        (Format::Json, Writer::Stdout),
    ];
    for (format, writer) in combos {
        let opts = InstallOptions {
            filter: env_or_directive("tau=info"),
            format,
            writer,
        };
        let _g = install(opts).expect("install returned err");
    }
}
```

Add `pub mod install;` to `crates/tau-observe/src/lib.rs` so it now reads:

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Observability primitives for tau: structured logging, tracing, and
//! the "observe" verb of the four-verb core (G1).

pub mod filter;
pub mod install;
pub mod vocabulary;
```

- [ ] **Step 2: Run the tests**

Run: `timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe`
Expected: all unit tests pass, integration test `install_smoke` passes (1 test).

- [ ] **Step 3: Commit**

```bash
git add crates/tau-observe/src/install.rs crates/tau-observe/src/lib.rs crates/tau-observe/tests/install_smoke.rs
git commit -m "feat(tau-observe): canonical install() + InstallOptions"
```

---

## Task 6: Migrate `tau-cli` to call `tau_observe::install`

**Files:**
- Modify: `crates/tau-cli/Cargo.toml`
- Modify: `crates/tau-cli/src/tracing.rs` (full rewrite — kept as adapter)
- Untouched: `crates/tau-cli/src/lib.rs:35` (caller signature unchanged)

- [ ] **Step 1: Add `tau-observe` to `tau-cli`'s deps**

In `crates/tau-cli/Cargo.toml`, add after line 21 (`tau-workflow`):

```toml
tau-observe         = { workspace = true }
```

- [ ] **Step 2: Rewrite `crates/tau-cli/src/tracing.rs`**

Replace the file's contents (the public surface — `build_filter` and `install` — must stay so `lib.rs:35` keeps compiling, but internals delegate to `tau-observe`):

```rust
//! Tracing-subscriber configuration for tau-cli.
//!
//! The CLI's job is to map clap flags onto a final filter directive.
//! Subscriber install itself lives in [`tau_observe::install`] so the
//! CLI and plugin SDK share one code path (sub-project A consolidation).
//!
//! Per spec §3.7: stderr-targeted subscriber, default level INFO scoped
//! to `tau=*`, verbosity flags promote (-v: DEBUG, -vv: TRACE), --quiet
//! demotes to WARN, --debug behaves as -v plus expanded error chain at
//! print time, RUST_LOG overrides everything.

use tau_observe::filter::env_or_directive;
use tau_observe::install::{install as observe_install, Format, InstallOptions, Writer};
use tracing_subscriber::filter::EnvFilter;

use crate::cli::Cli;

/// Compute the `EnvFilter` from CLI flags + `RUST_LOG` env.
///
/// Resolution order:
/// 1. `RUST_LOG` (if set) — used verbatim, overrides flags.
/// 2. `--verbose` count >= 2 → `"tau=trace"`.
/// 3. `--debug` OR `--verbose` count >= 1 → `"tau=debug"`.
/// 4. `--quiet` → `"tau=warn"`.
/// 5. Default → `"tau=info"`.
pub fn build_filter(cli: &Cli) -> EnvFilter {
    let directive = if cli.verbose >= 2 {
        "tau=trace"
    } else if cli.debug || cli.verbose >= 1 {
        "tau=debug"
    } else if cli.quiet {
        "tau=warn"
    } else {
        "tau=info"
    };
    env_or_directive(directive)
}

/// Install the global tracing subscriber for the `tau` CLI.
///
/// Delegates to [`tau_observe::install::install`] with the CLI's
/// human-format, stderr-writer configuration. Idempotent — the
/// underlying installer short-circuits second calls.
pub fn install(cli: &Cli) {
    let opts = InstallOptions {
        filter: build_filter(cli),
        format: Format::Human,
        writer: Writer::Stderr,
    };
    // The CLI does not propagate install errors; the only failure mode
    // is "already installed", which the underlying installer maps to a
    // no-op guard.
    let _guard = observe_install(opts).expect("tau_observe::install never returns Err in current impl");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, ColorMode, Command, ListArgs, ListResource};

    fn make_cli(verbose: u8, quiet: bool, debug: bool) -> Cli {
        Cli {
            command: Command::List(ListArgs {
                resource: ListResource::Packages,
                global: false,
                all: false,
                capabilities: false,
                dry_run: false,
            }),
            verbose,
            quiet,
            debug,
            color: ColorMode::Auto,
            json: false,
            record_protocol: None,
            no_sandbox: false,
            sandbox: None,
        }
    }

    /// Serialize tests that mutate `RUST_LOG` against each other in the
    /// same process (cargo test runs unit tests in one binary, so env
    /// state is shared).
    fn rust_log_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn build_filter_default_is_info() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(0, false, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=info"), "got: {filter}");
    }

    #[test]
    fn build_filter_minus_v_is_debug() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(1, false, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=debug"), "got: {filter}");
    }

    #[test]
    fn build_filter_minus_vv_is_trace() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(2, false, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=trace"), "got: {filter}");
    }

    #[test]
    fn build_filter_quiet_is_warn() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(0, true, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=warn"), "got: {filter}");
    }

    #[test]
    fn build_filter_debug_is_debug() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(0, false, true);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=debug"), "got: {filter}");
    }

    #[test]
    fn build_filter_rust_log_overrides_flags() {
        let _g = rust_log_lock();
        std::env::set_var("RUST_LOG", "my_plugin=trace");
        let cli = make_cli(0, true, false);
        let filter = build_filter(&cli);
        assert!(
            filter.to_string().contains("my_plugin=trace"),
            "got: {filter}"
        );
        std::env::remove_var("RUST_LOG");
    }

    #[test]
    fn build_filter_scopes_to_tau_when_no_rust_log() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(0, false, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().starts_with("tau="), "got: {filter}");
    }

    #[test]
    fn build_filter_minus_vv_takes_precedence_over_minus_v_logic() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(2, false, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=trace"));
        assert!(!filter.to_string().contains("tau=debug"));
    }
}
```

- [ ] **Step 3: Verify `tau-cli` builds and its tests still pass**

Run: `timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-cli --lib tracing::`
Expected: 8 tests pass (same as before — behavior is byte-identical).

Run: `timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo build -p tau-cli`
Expected: clean build, no errors.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-cli/Cargo.toml crates/tau-cli/src/tracing.rs
git commit -m "refactor(tau-cli): route subscriber install through tau-observe"
```

---

## Task 7: Migrate `tau-plugin-sdk` to call `tau_observe::install`

**Files:**
- Modify: `crates/tau-plugin-sdk/Cargo.toml`
- Modify: `crates/tau-plugin-sdk/src/tracing_layer.rs` (full rewrite — kept as adapter)
- Untouched: all 6 call sites in `crates/tau-plugin-sdk/src/runners/{tool,storage,llm_backend}.rs`

- [ ] **Step 1: Add `tau-observe` to the dependencies**

In `crates/tau-plugin-sdk/Cargo.toml`, after line 14 (`tau-plugin-protocol`):

```toml
tau-observe         = { workspace = true }
```

- [ ] **Step 2: Rewrite `crates/tau-plugin-sdk/src/tracing_layer.rs`**

Replace the file with:

```rust
//! tracing-subscriber JSON layer that writes structured events to
//! stderr. The host (in `tau-runtime::plugin_host`) reads each line,
//! decodes the JSON, and re-emits as a `tracing::Event` on
//! `target = "plugin::<plugin_name>"`.
//!
//! Internals delegate to [`tau_observe::install`] so all tau crates
//! share one subscriber-init code path.

/// Install the SDK's stderr-JSON tracing layer.
///
/// Idempotent: subsequent calls are no-ops. Plugin authors should
/// call this once at the start of `main()`, OR call one of the
/// `run_*` runners (which install it internally).
///
/// The default filter level is `info`; override via `RUST_LOG` env var
/// (e.g., `RUST_LOG=tau_plugin_sdk=debug,my_plugin=trace`).
pub fn install() {
    let _guard = tau_observe::install::install(tau_observe::install::InstallOptions::plugin_sdk())
        .expect("tau_observe::install never returns Err in current impl");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent() {
        // Call twice; second call should be a no-op (no panic).
        install();
        install();
    }
}
```

- [ ] **Step 3: Verify `tau-plugin-sdk` builds and tests pass**

Run: `timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-plugin-sdk --lib tracing_layer::`
Expected: 1 test passes (`install_is_idempotent`).

Run: `timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo build -p tau-plugin-sdk`
Expected: clean build, no errors.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-plugin-sdk/Cargo.toml crates/tau-plugin-sdk/src/tracing_layer.rs
git commit -m "refactor(tau-plugin-sdk): route subscriber install through tau-observe"
```

---

## Task 8: Repo-wide build + lint sweep

**Files:** none modified — verification only.

- [ ] **Step 1: Build every directly-affected crate together**

Run: `timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo build -p tau-observe -p tau-cli -p tau-plugin-sdk`
Expected: clean build, no errors.

- [ ] **Step 2: Run clippy on every directly-affected crate**

Run: `timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-observe -- -D warnings`
Run: `timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-cli -- -D warnings`
Run: `timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-plugin-sdk -- -D warnings`
Expected: each exits 0 with no warnings.

- [ ] **Step 3: Run nextest on each crate**

Run: `timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-observe`
Expected: all tau-observe tests pass.

Run: `timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-cli`
Expected: same green count as before this plan started.

Run: `timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-plugin-sdk`
Expected: same green count as before this plan started.

- [ ] **Step 4: Confirm no stale `tracing-subscriber` direct uses leak**

Run: `grep -rn "tracing_subscriber::fmt()\|FmtSubscriber::" crates/tau-cli/src crates/tau-plugin-sdk/src`
Expected: zero matches (everything routes through `tau_observe::install`).

- [ ] **Step 5: No commit needed** — this task is verification only. If any check fails, fix and re-commit under the appropriate earlier task's scope.

---

## Task 9: Draft ADR-0031 entry

**Files:**
- Create: `docs/decisions/0031-tau-observe-consolidation.md`

- [ ] **Step 1: Write the ADR**

Follow the style of existing entries (look at `docs/decisions/0030-skills-reference-packages.md` for the most recent example). Sections: Context, Decision, Consequences, Trigger to revisit.

Minimum content:

```markdown
# ADR-0031: `tau-observe` as the canonical tracing-subscriber init crate

## Status

Accepted. Implemented in PR <number-to-fill-at-merge-time>.

## Context

`tracing` adoption (ADR-0006 §3.9, NG9) leaves subscriber install to the
caller. In practice every tau binary/library that produces logs has
re-implemented the same `tracing_subscriber::fmt()` + `EnvFilter` dance.
Two near-identical implementations exist today:

- `crates/tau-cli/src/tracing.rs` — human format to stderr, CLI-flag-to-
  filter mapping, panicking `init()`.
- `crates/tau-plugin-sdk/src/tracing_layer.rs` — JSON to stderr,
  idempotent `Once`-gated install.

Sub-projects B (§3.9 span vocabulary), C (preview helpers), D (workflow
+ recording as `Layer`s), E (`tracing-appender`), F (OTLP export) all
need a single place that owns the subscriber registry. Continuing to
add layers from two different crates with two different init policies
would lock in the divergence.

## Decision

Promote the existing `tau-observe` crate (currently a stub) to the
canonical owner of:

- `tau_observe::install::install(InstallOptions) -> Result<InstallGuard, InstallError>`
  with idempotent global init and an `InstallGuard` that future sub-
  projects can hang flush behavior off (sub-project E).
- `tau_observe::filter::env_or_directive(&str) -> EnvFilter` — the only
  place that interprets `RUST_LOG`.
- `tau_observe::vocabulary` — `&'static str` constants for every §3.9
  span and event name.

`tau-cli` and `tau-plugin-sdk` keep their public `install` functions
(signatures unchanged) but their bodies become one-liners that build
the appropriate `InstallOptions` and delegate to `tau_observe::install`.

## Consequences

- One subscriber init code path. Sub-projects B/C/D/E/F each layer onto
  this surface without further divergence.
- `tau-observe` becomes a workspace dependency. Build time impact is
  negligible (the crate has three direct deps; all are already in the
  workspace).
- Plugin authors who previously imported `tau_plugin_sdk::tracing_layer`
  see no source-level change; the layer continues to be re-exported.
- NG9 still holds: `tau-observe` exposes helpers but does not enforce
  any redaction policy on the caller.

## Trigger to revisit

A second subscriber init pattern lands that doesn't fit `InstallOptions`
(e.g. multi-sink, dynamic reconfiguration). At that point reconsider
whether `tau-observe::install` should grow or whether a layered API
(`tau_observe::build_layers() -> impl Layer<S>`) is a better surface.
```

- [ ] **Step 2: Commit the ADR**

```bash
git add docs/decisions/0031-tau-observe-consolidation.md
git commit -m "docs(adr): ADR-0031 — tau-observe as canonical tracing init crate"
```

---

## Task 10: Final pre-push verification

**Files:** none modified — verification only.

- [ ] **Step 1: Run the pre-push deep gate**

Per `CLAUDE.md` "AGENT PUSH RULES": do not run `git push` directly. Run the gate as a standalone command first:

Run: `timeout 1800 lefthook run pre-push`
Expected: green. Cold-start can take 30-50 min (per memory `tau gate cold-start ~30-50min 2026-05-17`); warm runs are ~3-4 min. Silent output during execution is normal.

- [ ] **Step 2: Push via the safe path**

Run: `scripts/agent-push.sh -u origin HEAD`
Expected: branch pushed; gate already passed so the inline push is fast.

- [ ] **Step 3: Open the PR**

Use `gh pr create` with title `feat(tau-observe): canonical tracing-subscriber init crate (Sub-project A)`. Body should reference the design doc at `docs/superpowers/specs/2026-05-17-logging-upgrades-design.md` and note "Sub-project A of 6. B–F land in follow-up PRs."

---

## Spec coverage check

- Spec sub-project A "canonical subscriber-init crate" → Tasks 2, 5, 6, 7.
- Spec sub-project A "`vocabulary` module of `&'static str` constants" → Task 3.
- Spec sub-project A "`tau_observe::install(InstallOptions)`" → Task 5.
- Spec sub-project A "deletes duplicated `install` from tau-cli and tau-plugin-sdk" → Tasks 6, 7 (replaced with thin delegates; the public surface is preserved so the 7 internal callers don't need to change).
- Spec sub-project A "Cargo.toml adds tracing-subscriber features" → Task 2 (`fmt`, `env-filter`, `json`).
- Spec sub-project A "ADR-0031 lands with sub-project A" → Task 9.
- Spec section "Migration plan" — A ships first, byte-identical user behavior → Task 8 (verifies test counts unchanged for tau-cli + tau-plugin-sdk).
- Spec section "Testing — A" → Task 5's `install_smoke.rs` covers each `InstallOptions` permutation.

Sub-projects B–F intentionally out of scope for this plan — each gets its own plan once A lands.
