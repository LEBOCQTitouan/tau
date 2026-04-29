# OpenAI plugin + supporting infrastructure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `crates/tau-plugins/openai/` (third real LLM-backend plugin), `crates/tau-plugin-test-support/` (rule-of-three refactor of cassette replayer), `crates/tau-plugin-conformance/` (parameterized behavioral test suite), and migrate all three plugins from blanket `LlmError::Internal` to typed variants.

**Architecture:** Out-of-process plugin spawned by `tau-runtime::plugin_host` per ADR-0008. OpenAI plugin targets `POST /v1/chat/completions` with SSE streaming via `eventsource-stream`, real `tool_call_id` round-trip, full `tool_choice` support, required `Authorization: Bearer` auth. Shared test-support crate lifts the cassette replayer (~323 LOC) currently duplicated across anthropic + ollama. Conformance suite runs 6 parameterized behavioral tests against any `LlmBackend` impl.

**Tech Stack:** Rust 1.91, `reqwest 0.12` (rustls + json + stream), `eventsource-stream 0.2`, `secrecy 0.10`, `async-stream 0.3`, `tokio` (multi-thread), `serde` + `serde_json`, `tracing`, `serde_yaml` (cassettes, dev-only). **No new workspace deps** — all required deps already present from sub-projects 2a + 2b.

**Sub-project scope:** Phase 1 priority 2c. Spec at [`docs/superpowers/specs/2026-04-29-openai-plugin-design.md`](../specs/2026-04-29-openai-plugin-design.md) (commit `284eb5f`).

---

## Plan-erratum: types, conventions, and traps

These are pre-known invariants from sub-projects 1 + 2a + 2b. Apply
them verbatim — do NOT re-derive them by reading the spec.

### Actual `tau-ports` types

| Concern | Actual type / shape |
|---|---|
| Streaming chunk variants | `CompletionChunk::Text { delta: String }` / `CompletionChunk::ToolUse(ToolUse)` (tuple variant!) / `CompletionChunk::Finish { stop_reason: StopReason, usage: Option<TokenUsage> }` |
| Batch response | `CompletionResponse { text: String, tool_uses: Vec<ToolUse>, stop_reason: StopReason, usage: Option<TokenUsage> }` — flat `#[non_exhaustive]`. Construct via `tau_ports::fixtures::make_completion_response(text, tool_uses, stop_reason, usage)` (the `test-fixtures` feature must be enabled in the OpenAI crate's deps). |
| System prompt | `CompletionRequest::system: Option<String>` — top-level. OpenAI mapping: prepend a leading `{role:"system", content}` message to the `messages` array (matches Ollama; OpenAI has NO top-level `system` field). |
| Content blocks | `ContentBlock::Text(String)` is a tuple variant. `ContentBlock::ToolUse(ToolUse)` also tuple. v0.1 ignores any other variant. |
| Tool choice | `ToolChoice::{Auto, None, Required, Specific { name: String }}`. **OpenAI round-trips ALL FOUR**: `Auto→"auto"`, `None→"none"`, `Required→"required"`, `Specific→{"type":"function","function":{"name":...}}`. Distinct from Ollama which drops Required/Specific. |
| Tool spec | `ToolSpec { name: String, description: String, input_schema: tau_domain::Value }`. The schema is `tau_domain::Value`, NOT `serde_json::Value` — convert via `serde_json::to_value(&spec.input_schema)?` for the wire body. |
| Stop reasons | `StopReason::{EndTurn, MaxTokens, StopSequence, ToolUse, Error}`. NO `Other(String)`. Unknown `finish_reason` strings map to `EndTurn` with `tracing::warn!`. |
| Token usage | `TokenUsage::new(input: u32, output: u32)` — both `u32`. Defensive parse: when either field is absent, set `usage = None`. |
| Tool use construction | `ToolUse::new(id: String, name: String, input: tau_domain::Value)` |

### `LlmError` typed variants (ALREADY EXIST — no `tau-ports` changes needed)

`tau_ports::LlmError` is `#[non_exhaustive]` and already has:
- `InvalidRequest { reason: String }` — 400 errors.
- `RateLimited { retry_after_seconds: Option<u32> }` — 429.
- `Auth { message: String }` — 401, 403.
- `Transport { message: String }` — network/DNS/TLS failures.
- `Stream { message: String }` — mid-stream errors.
- `Provider { message: String }` — 5xx + unmapped errors.
- `Unsupported { what: String }` — feature not supported.
- `Internal { message: String }` — escape hatch (registered).

Migration goal: plugins emit typed variants for ALL HTTP-mapped paths; `Internal` is retained ONLY for plugin-internal translation errors (e.g., wrapping a `BuildError` from `request.rs`). The escape-hatch registry `llmerror-internal` entry stays — its scope narrows.

### `LlmError::is_retryable` semantics

`true` for `RateLimited`, `Transport`, `Stream`, `Provider`. `false` for `InvalidRequest`, `Auth`, `Unsupported`, `Internal`. Plugins should map 5xx (≠501) to `Provider` (retryable) and 503 to `Provider` even when it represents Ollama's "model loading" case — caller's retry helper wins.

### Wire-protocol carryovers (handled by SDK; plugin code must match)

- Wire methods are `llm.complete` and `llm.stream`.
- `CompletionChunk::Finish` (NOT `Done`) terminates a stream.

### `#[non_exhaustive]` discipline

- Doctests on `#[non_exhaustive]` types must use ` ```ignore ` fences (else E0639).
- Cross-crate destructuring of `#[non_exhaustive]` enums: prefer `let X { fields, .. } = value else { panic!() };`.
- Cross-crate struct construction: use `Default::default()` then field assignment (NOT `..Default::default()` shorthand on foreign types).

### `Retry-After` plumbing

The current `map_response_error(status, body)` signature on anthropic + ollama doesn't see HTTP headers. The migration adds a `headers: &reqwest::header::HeaderMap` parameter so 429 responses can populate `RateLimited { retry_after_seconds: Option<u32> }` from the `Retry-After` header. Existing `is_retryable_status` + retry-loop logic in `client.rs` is unchanged.

### Verification protocol

`cargo test --all-targets` does **not** run doctests. Each task's verification block runs `cargo test --doc` separately when the task adds public items.

### Same-commit escape-hatch registry

The mechanical CI test at `crates/tau-domain/tests/escape_hatch_registry.rs` continues to gate against accidental NEW `Internal`/`Custom` variants. **No new such variants ship in this sub-project.** The migration narrows existing `Internal` callsites; the registry's `llmerror-internal` anchor stays.

### What this sub-project does NOT introduce

- No new `LlmError` variants (the typed variants we need already exist).
- No new `ConfigError` variants (`InvalidEnvVar` already shipped in 2a).
- No new ADR amendment to ADR-0008 (this sub-project ships its own ADR-0009).
- No new workspace deps (`reqwest`, `eventsource-stream`, `secrecy`, `async-stream` all from 2a).

---

## File Structure

```
crates/
├── tau-plugin-test-support/              -- NEW dev-dep crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                        -- pub modules + crate docs
│       └── cassette.rs                   -- LIFTED from anthropic verbatim (~323 LOC)
│
├── tau-plugin-conformance/               -- NEW parameterized test crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                        -- ConformanceSuite struct + 6 tests
│       └── cases/                        -- per-test helpers (private)
│           ├── mod.rs
│           ├── batch_happy_path.rs
│           ├── batch_with_tools.rs
│           ├── streaming_text.rs
│           ├── streaming_tool_use.rs
│           ├── error_rate_limited.rs
│           └── error_auth.rs
│
├── tau-plugins/openai/                   -- NEW plugin
│   ├── Cargo.toml                        -- bin: openai-plugin; lib: openai_plugin_lib
│   ├── tau.toml                          -- provides=llm_backend
│   ├── src/
│   │   ├── main.rs                       -- #[tokio::main] → run_llm_backend_with_config
│   │   ├── lib.rs                        -- pub modules + crate docs
│   │   ├── plugin.rs                     -- OpenAIPlugin + LlmBackend + Configure
│   │   ├── config.rs                     -- OpenAIConfig + RetryConfig + resolve_api_key + validate_retry
│   │   ├── client.rs                     -- OpenAIClient (reqwest) + post_chat_completions + retry loop
│   │   ├── request.rs                    -- CompletionRequest → /v1/chat/completions JSON
│   │   ├── response.rs                   -- /v1/chat/completions JSON → CompletionResponse
│   │   ├── stream.rs                     -- SSE parser + ToolUseAccumulator → CompletionStream
│   │   └── error.rs                      -- HTTP status + headers + OpenAI envelope → TYPED LlmError
│   └── tests/
│       ├── cassettes/                    -- 9 cassette YAMLs (6 batch + 3 streaming)
│       ├── conformance-cassettes/        -- 6 conformance cassettes (one per ConformanceSuite test)
│       ├── common/mod.rs                 -- helpers (re-exports cassette from shared crate)
│       ├── complete.rs                   -- batch tests via cassette replay
│       ├── streaming.rs                  -- streaming tests
│       ├── conformance.rs                -- ConformanceSuite::default().run(...) shim
│       └── live.rs                       -- env-gated smoke tests (#[ignore])
│
├── tau-plugins/anthropic/                -- MIGRATED
│   ├── Cargo.toml                        -- + tau-plugin-test-support + tau-plugin-conformance dev-deps
│   ├── src/
│   │   ├── client.rs                     -- post_messages signature unchanged; classify still works
│   │   └── error.rs                      -- map_response_error TYPED + signature gains headers param
│   └── tests/
│       ├── common/mod.rs                 -- re-exports cassette from shared crate (cassette.rs DELETED)
│       ├── conformance-cassettes/        -- NEW: 6 conformance cassettes
│       ├── complete.rs                   -- assertions updated for typed variants
│       ├── streaming.rs                  -- (no functional change, may need to drop unused imports)
│       └── conformance.rs                -- NEW: ConformanceSuite shim
│
└── tau-plugins/ollama/                   -- MIGRATED
    ├── Cargo.toml                        -- + tau-plugin-test-support + tau-plugin-conformance dev-deps
    ├── src/
    │   ├── client.rs                     -- post_chat signature unchanged
    │   └── error.rs                      -- map_response_error TYPED + signature gains headers param
    └── tests/
        ├── common/mod.rs                 -- re-exports cassette from shared crate (cassette.rs DELETED)
        ├── conformance-cassettes/        -- NEW: 6 conformance cassettes
        ├── complete.rs                   -- assertions updated for typed variants
        ├── streaming.rs                  -- (no functional change)
        └── conformance.rs                -- NEW: ConformanceSuite shim

.github/workflows/ci.yml                  -- + 4 new jobs
docs/decisions/0009-llm-error-typing-and-conformance.md  -- NEW ADR (Status: Proposed → Accepted at Task 22)
ROADMAP.md                                -- 2c row added
Cargo.toml                                -- + 3 workspace members; no new workspace deps
```

---

## Tasks 1-3: detailed (Plan-2 fidelity)

The first three tasks are documented at full fidelity (every code
snippet, every step, every verification command). Tasks 4-22 follow
the hybrid format (per-task summary + spec section references).

---

### Task 1: Workspace scaffold (3 new crates)

Create the empty crate skeletons for the OpenAI plugin, the shared
test-support crate, and the conformance suite. **No new workspace
deps.**

**Files:**
- Create: `crates/tau-plugin-test-support/Cargo.toml`
- Create: `crates/tau-plugin-test-support/src/lib.rs`
- Create: `crates/tau-plugin-conformance/Cargo.toml`
- Create: `crates/tau-plugin-conformance/src/lib.rs`
- Create: `crates/tau-plugins/openai/Cargo.toml`
- Create: `crates/tau-plugins/openai/tau.toml`
- Create: `crates/tau-plugins/openai/src/main.rs`
- Create: `crates/tau-plugins/openai/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1.1: Add the three workspace members + workspace dep entries**

Edit `Cargo.toml` (workspace root). Append the three new members; add the new crates as workspace deps so other plugins can pick them up cleanly.

```toml
[workspace]
resolver = "2"
members = [
    "crates/tau-domain",
    "crates/tau-ports",
    "crates/tau-infra",
    "crates/tau-app",
    "crates/tau-pkg",
    "crates/tau-observe",
    "crates/tau-runtime",
    "crates/tau-cli",
    "crates/tau-plugin-protocol",
    "crates/tau-plugin-sdk",
    "crates/tau-plugin-test-support",
    "crates/tau-plugin-conformance",
    "crates/tau-plugins/echo-llm",
    "crates/tau-plugins/echo-tool",
    "crates/tau-plugins/anthropic",
    "crates/tau-plugins/ollama",
    "crates/tau-plugins/openai",
]
```

Append to the existing `[workspace.dependencies]` block (after the existing entries):

```toml
tau-plugin-test-support = { path = "crates/tau-plugin-test-support", version = "0.0.0" }
tau-plugin-conformance  = { path = "crates/tau-plugin-conformance",  version = "0.0.0" }
```

NO other dep table changes.

- [ ] **Step 1.2: Create `crates/tau-plugin-test-support/Cargo.toml`**

```toml
[package]
name = "tau-plugin-test-support"
description = "Shared test support for tau LLM-backend plugins (cassette replayer + helpers)."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[lib]
name = "tau_plugin_test_support"
path = "src/lib.rs"

[dependencies]
tokio       = { workspace = true, features = ["macros", "rt", "rt-multi-thread", "sync", "io-util", "net"] }
serde       = { workspace = true }
serde_yaml  = "0.9"
```

- [ ] **Step 1.3: Create `crates/tau-plugin-test-support/src/lib.rs` (empty stub)**

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Shared test-support code for tau LLM-backend plugins.
//!
//! See `docs/superpowers/specs/2026-04-29-openai-plugin-design.md`
//! §3.1 + §9.1 for design rationale (rule-of-three refactor of the
//! cassette replayer that originated in the anthropic plugin).

// The `cassette` module lands in Task 2 (lifted verbatim from
// crates/tau-plugins/anthropic/tests/common/cassette.rs).
```

- [ ] **Step 1.4: Create `crates/tau-plugin-conformance/Cargo.toml`**

```toml
[package]
name = "tau-plugin-conformance"
description = "Parameterized conformance test suite for tau LLM-backend plugins."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[lib]
name = "tau_plugin_conformance"
path = "src/lib.rs"

[dependencies]
tau-domain               = { workspace = true, features = ["serde"] }
tau-ports                = { workspace = true, features = ["serde", "test-fixtures"] }
tau-plugin-test-support  = { workspace = true }
tokio                    = { workspace = true, features = ["macros", "rt", "rt-multi-thread"] }
futures-util             = "0.3"
serde                    = { workspace = true }
serde_json               = "1"
```

- [ ] **Step 1.5: Create `crates/tau-plugin-conformance/src/lib.rs` (empty stub)**

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Parameterized conformance test suite for tau `LlmBackend` plugins.
//!
//! See `docs/superpowers/specs/2026-04-29-openai-plugin-design.md`
//! §8.3 for the catalog and charter.

// `ConformanceSuite` lands in Task 15.
```

- [ ] **Step 1.6: Create `crates/tau-plugins/openai/Cargo.toml`**

```toml
[package]
name = "openai"
description = "OpenAI Chat Completions LLM-backend plugin for tau."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[[bin]]
name = "openai-plugin"
path = "src/main.rs"

[lib]
name = "openai_plugin_lib"
path = "src/lib.rs"

[dependencies]
tau-domain          = { workspace = true, features = ["serde"] }
tau-ports           = { workspace = true, features = ["serde", "test-fixtures"] }
tau-plugin-protocol = { workspace = true }
tau-plugin-sdk      = { workspace = true }
serde               = { workspace = true }
serde_json          = "1"
thiserror           = { workspace = true }
tokio               = { workspace = true, features = ["macros", "rt", "rt-multi-thread", "sync", "time"] }
tracing             = { workspace = true }
reqwest             = { workspace = true }
eventsource-stream  = { workspace = true }
async-stream        = { workspace = true }
secrecy             = { workspace = true }
futures-core        = { workspace = true }
futures-util        = "0.3"

[dev-dependencies]
tokio                   = { workspace = true, features = ["macros", "rt-multi-thread", "io-util", "net"] }
tempfile                = { workspace = true }
serde_yaml              = "0.9"
tau-plugin-test-support = { workspace = true }
tau-plugin-conformance  = { workspace = true }
```

- [ ] **Step 1.7: Create `crates/tau-plugins/openai/tau.toml`**

```toml
name = "openai"
version = "0.1.0"
description = "OpenAI Chat Completions backend for tau."

[plugin]
provides = "llm_backend"
kind     = "rust-cargo"
bin      = "openai-plugin"
```

- [ ] **Step 1.8: Create `crates/tau-plugins/openai/src/lib.rs`**

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! OpenAI (Chat Completions API) plugin internals.
//!
//! The binary entrypoint at `src/main.rs` calls
//! `tau_plugin_sdk::run_llm_backend_with_config::<OpenAIPlugin>(...)`.
//!
//! See `docs/superpowers/specs/2026-04-29-openai-plugin-design.md`
//! for the design rationale.

// Modules will be added in subsequent tasks (config, request, response,
// error, client, stream, plugin).
```

- [ ] **Step 1.9: Create `crates/tau-plugins/openai/src/main.rs` (placeholder)**

```rust
//! `openai-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! The full implementation lands in Task 11. For Task 1, this stub
//! exists only so that `cargo build` succeeds.

fn main() {
    eprintln!("openai-plugin: not yet wired (placeholder; see Task 11)");
    std::process::exit(1);
}
```

- [ ] **Step 1.10: Verify the workspace builds**

Run: `cargo build --workspace`
Expected: PASS — three new crates recognized; `target/debug/openai-plugin` produced.

Run each verification command:

```
cargo build -p tau-plugin-test-support
cargo build -p tau-plugin-conformance
cargo build -p openai
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test --workspace --doc
```
Expected: ALL PASS. Pre-existing tests continue to pass; new crates each report 0 tests.

If any verification fails, STOP and report verbatim.

- [ ] **Step 1.11: Commit + push**

```bash
git add Cargo.toml crates/tau-plugin-test-support/ crates/tau-plugin-conformance/ crates/tau-plugins/openai/
git commit -m "$(cat <<'EOF'
feat(2c): scaffold openai plugin + test-support + conformance crates

Empty stubs for Phase 1 sub-project 2c (OpenAI plugin + supporting
infrastructure). Three new workspace members:

- crates/tau-plugin-test-support/  -- shared cassette replayer
  (lift in Task 2; rule-of-three refactor)
- crates/tau-plugin-conformance/   -- parameterized test suite
  (Task 15; charter: mechanical correctness, NOT response quality)
- crates/tau-plugins/openai/       -- third real LLM-backend plugin
  binary `openai-plugin`, lib `openai_plugin_lib`; provides=llm_backend

Also registers the two test-support crates as workspace deps so
plugins can dev-depend on them by `workspace = true`.

NO new workspace deps — every transitive dep (reqwest, secrecy,
eventsource-stream, async-stream, etc.) was added in 2a/2b.

Refs: docs/superpowers/specs/2026-04-29-openai-plugin-design.md §3.1

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push -u origin feat/openai-plugin-spec
```

PR auto-triggers CI. Wait for CI green before Task 2.

---

### Task 2: Lift `cassette.rs` into `tau-plugin-test-support`

Lift the cassette replayer from anthropic into the new shared crate.
**This task does NOT yet delete the local copies in anthropic/ollama**
— that lands in Tasks 3 and 4 once the shared crate is verified
self-contained. The lift is verbatim except for: (a) the module
becomes a top-level `pub mod cassette;` in `lib.rs`; (b) `pub(crate)`
visibility on internal types remains.

**Files:**
- Create: `crates/tau-plugin-test-support/src/cassette.rs` (verbatim copy)
- Modify: `crates/tau-plugin-test-support/src/lib.rs` (add `pub mod cassette;`)

- [ ] **Step 2.1: Lift the file**

Run, from `/Users/titouanlebocq/code/tau`:

```bash
cp crates/tau-plugins/anthropic/tests/common/cassette.rs \
   crates/tau-plugin-test-support/src/cassette.rs
```

The file is now ~323 LOC at the new location. Verify:

```bash
diff crates/tau-plugin-test-support/src/cassette.rs \
     crates/tau-plugins/anthropic/tests/common/cassette.rs
```

Expected: empty output. The copies are identical.

- [ ] **Step 2.2: Update `crates/tau-plugin-test-support/src/lib.rs`**

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Shared test-support code for tau LLM-backend plugins.
//!
//! See `docs/superpowers/specs/2026-04-29-openai-plugin-design.md`
//! §3.1 + §9.1 for design rationale (rule-of-three refactor of the
//! cassette replayer that originated in the anthropic plugin).

pub mod cassette;
```

- [ ] **Step 2.3: Add a crate-level doc comment to `cassette.rs`**

The lifted file's module-level doc currently says it's part of the
anthropic plugin's tests. Update the first 5-10 lines to reflect the
new home:

Locate the existing `//!` block at the top of `crates/tau-plugin-test-support/src/cassette.rs` and replace it with:

```rust
//! In-process HTTP cassette replayer for plugin integration tests.
//!
//! Loads YAML cassettes describing recorded request/response pairs
//! and serves them in-order from a `tokio::net::TcpListener`.
//! Captures the request body (and arbitrary other headers) into a
//! `Vec<RecordedRequest>` so tests can assert on what the plugin sent.
//!
//! Originated in `crates/tau-plugins/anthropic/tests/common/cassette.rs`;
//! lifted here as the rule-of-three refactor when ollama and openai
//! became consumers.
```

(Keep the implementation below unchanged.)

- [ ] **Step 2.4: Verify the new crate builds and self-tests pass**

The lifted file includes its own `#[cfg(test)] mod self_tests` block (you can confirm by `grep "mod self_tests" crates/tau-plugin-test-support/src/cassette.rs`).

Run:

```
cargo build -p tau-plugin-test-support
cargo test -p tau-plugin-test-support --all-targets
cargo test -p tau-plugin-test-support --doc
cargo fmt --all -- --check
cargo clippy -p tau-plugin-test-support --all-targets -- -D warnings
```
Expected: PASS. The self_tests run inside the lib crate and validate the replayer.

If clippy flags any of the lifted code (the original passed `-D warnings` at home in anthropic/tests/), it likely means lib-context vs test-context clippy lints differ. Most common: `clippy::needless_pass_by_value` was suppressed by `tests/`'s `#![allow(dead_code)]`; you may need to add `#![allow(clippy::needless_pass_by_value)]` at the top of `cassette.rs` OR fix the offending pass-by-value sites. Fix in-place; do NOT propagate suppressions outside this file.

- [ ] **Step 2.5: Verify the workspace still compiles**

```
cargo build --workspace
cargo test --workspace --all-targets
```
Expected: PASS. Anthropic's existing `tests/common/cassette.rs` still works (we haven't deleted it yet); the workspace just gained a new crate that nobody imports yet.

- [ ] **Step 2.6: Commit + push**

```bash
git add crates/tau-plugin-test-support/
git commit -m "$(cat <<'EOF'
feat(test-support): lift cassette replayer from anthropic plugin

tau-plugin-test-support gains its first module: cassette.rs lifted
verbatim (~323 LOC) from
crates/tau-plugins/anthropic/tests/common/cassette.rs. Module-level
doc updated to reflect the new home.

Public surface:
- `cassette::replay(path) -> CassetteServer`
- `CassetteServer::{uri, received_requests}`
- `RecordedRequest`

The original copy in anthropic/tests/common/cassette.rs stays in
place this commit; it gets deleted in Task 3 once the shared crate
is verified self-contained. Same plan for ollama in Task 4.

Refs: docs/superpowers/specs/2026-04-29-openai-plugin-design.md §3.1, §9.1

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push
```

Wait for CI green before Task 3.

---

### Task 3: Migrate anthropic to use the shared cassette crate

Delete `crates/tau-plugins/anthropic/tests/common/cassette.rs` (the
local copy). Update `tests/common/mod.rs` to re-export the cassette
module from `tau_plugin_test_support`. Verify all existing anthropic
integration tests continue to pass with zero behavior change.

**Files:**
- Delete: `crates/tau-plugins/anthropic/tests/common/cassette.rs`
- Modify: `crates/tau-plugins/anthropic/tests/common/mod.rs`
- Modify: `crates/tau-plugins/anthropic/Cargo.toml` (add dev-dep)

- [ ] **Step 3.1: Add the shared crate as a dev-dep**

Modify `crates/tau-plugins/anthropic/Cargo.toml`. Append to the existing `[dev-dependencies]` block:

```toml
tau-plugin-test-support = { workspace = true }
```

- [ ] **Step 3.2: Read the current `tests/common/mod.rs`**

Read `/Users/titouanlebocq/code/tau/crates/tau-plugins/anthropic/tests/common/mod.rs`. Note its public surface (test_config, test_config_with_retry, sample_request, extract_text). The `pub mod cassette;` line at the top will be replaced.

- [ ] **Step 3.3: Update `tests/common/mod.rs`**

Replace the `pub mod cassette;` declaration with a re-export. The exact pattern (depending on the rest of the file):

```rust
//! Test helpers shared across the integration test files in
//! `crates/tau-plugins/anthropic/tests/`.

#![allow(dead_code)]

// `cassette` is provided by the shared `tau-plugin-test-support` crate
// (lifted in Task 2 of sub-project 2c). Re-exported under the local
// name so existing test imports (`use common::cassette;`) keep working.
pub use tau_plugin_test_support::cassette;

use anthropic_plugin_lib::config::AnthropicConfig;
use tau_ports::{CompletionRequest, CompletionResponse, ContentBlock, LlmProviderMessage};

// ... (rest of the file unchanged: sample_request, extract_text,
//      test_config, test_config_with_retry)
```

If `pub use ...::cassette;` doesn't work cleanly (Rust sometimes errors when re-exporting modules across crate boundaries from an integration-test target), use this alternative:

```rust
pub mod cassette {
    pub use tau_plugin_test_support::cassette::*;
}
```

The `*` re-export pattern is more robust for cross-crate module
mirroring. Pick whichever clippy + the test harness accept. Both are
behaviorally identical for the test imports.

- [ ] **Step 3.4: Delete the local copy**

```
rm crates/tau-plugins/anthropic/tests/common/cassette.rs
```

- [ ] **Step 3.5: Verify ALL anthropic integration tests still pass**

```
cargo build -p anthropic --tests
cargo test -p anthropic --all-targets
cargo test -p anthropic --doc
cargo fmt --all -- --check
cargo clippy -p anthropic --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace --all-targets
```

Expected: PASS — anthropic's complete.rs, streaming.rs, live.rs (the `#[ignore]` tests that compile but don't run) all see `cassette::replay`, `cassette::CassetteServer`, etc. without changes.

If `cassette::CassetteServer::uri()` is called by tests but the re-export pattern (Option A) hides it, switch to Option B (`pub mod cassette { pub use ...::*; }`).

- [ ] **Step 3.6: Verify no functional change**

Run the most assertion-heavy tests with `--nocapture`:

```
cargo test -p anthropic --test complete -- --nocapture
cargo test -p anthropic --test streaming -- --nocapture
```

Expected: same output as before. Cassette replayer behavior is unchanged.

- [ ] **Step 3.7: Commit + push**

```bash
git add crates/tau-plugins/anthropic/Cargo.toml \
        crates/tau-plugins/anthropic/tests/common/mod.rs \
        crates/tau-plugins/anthropic/tests/common/cassette.rs
git diff --cached --stat
# Confirm: 3 files changed (Cargo.toml modify, mod.rs modify,
# cassette.rs deleted).
git commit -m "$(cat <<'EOF'
refactor(anthropic): use shared tau-plugin-test-support cassette

Migrates the anthropic plugin from its local
crates/tau-plugins/anthropic/tests/common/cassette.rs (~323 LOC) to
the shared tau-plugin-test-support crate (lifted in Task 2 of
sub-project 2c).

- cassette.rs DELETED.
- tests/common/mod.rs re-exports cassette from the shared crate;
  existing test imports (`use common::cassette;`) continue to work.
- Cargo.toml gains tau-plugin-test-support as a dev-dep.

Behavior change: zero. All integration tests pass before and after.

Refs: docs/superpowers/specs/2026-04-29-openai-plugin-design.md §9.1

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push
```

Wait for CI green before Task 4.

---

## Tasks 4-22: hybrid (per-task summary + spec references)

Per the established sub-project pattern: Tasks 4-22 use a hybrid format
— per-task summary, file list, key code skeleton, test inventory, and
verification commands, with cross-references to the spec sections that
contain the full code.

Each task ends with the same verification protocol:
```
cargo test -p <plugin> --all-targets
cargo test -p <plugin> --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
```
And one Conventional Commits commit + push. Wait for CI green between
tasks.

---

### Task 4: Migrate ollama to use the shared cassette crate

Mirror Task 3 verbatim but for the ollama plugin.

**Files:**
- Delete: `crates/tau-plugins/ollama/tests/common/cassette.rs`
- Modify: `crates/tau-plugins/ollama/tests/common/mod.rs` — replace `pub mod cassette;` with the re-export pattern from Task 3.5
- Modify: `crates/tau-plugins/ollama/Cargo.toml` — add `tau-plugin-test-support = { workspace = true }` to dev-dependencies.

**Verification:** All ollama integration tests pass (59 tests including the load-bearing 503-retry test).

**Commit subject:** `refactor(ollama): use shared tau-plugin-test-support cassette`

**Refs:** Spec §9.1.

---

### Task 5: OpenAI `OpenAIConfig` + `Configure` + `resolve_api_key` + `validate_retry`

**Files:** Create `crates/tau-plugins/openai/src/config.rs`; add `pub mod config;` to `lib.rs`.

**Public surface:**

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAIConfig {
    #[serde(default = "default_base_url")]            pub base_url: String,
    #[serde(default = "default_api_key_env")]         pub api_key_env: String,
    #[serde(default)]                                 pub api_key: Option<String>,
    #[serde(default = "default_request_timeout_secs")] pub request_timeout_secs: u64,
    #[serde(default)]                                 pub organization: Option<String>,
    #[serde(default)]                                 pub retry: RetryConfig,
}

#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryConfig {
    #[serde(default = "default_max_attempts")]       pub max_attempts: u32,
    #[serde(default = "default_base_delay_ms")]      pub base_delay_ms: u64,
    #[serde(default = "default_respect_retry_after")] pub respect_retry_after: bool,
}
```

Defaults: `base_url = "https://api.openai.com"`, `api_key_env = "OPENAI_API_KEY"`, `request_timeout_secs = 600`, `retry` defaults match Anthropic (3, 1000, true).

`resolve_api_key`: identical pattern to Anthropic's. Errors with `ConfigError::InvalidEnvVar` when env var missing. Validates the key starts with `"sk-"` (matches both legacy `sk-...` and modern `sk-proj-...`); else `ConfigError::InvalidValue { field: "api_key", detail: "OpenAI API keys start with `sk-`" }`.

`validate_retry`: identical to Anthropic/Ollama (rejects `max_attempts == 0`).

`#[allow(dead_code)]` on `resolve_api_key` and `validate_retry` (consumed by tests + Task 11; cleared in Task 11).

**Test inventory (~9 unit tests):** mirrors anthropic/config.rs + ollama/config.rs:
- `defaults_are_production_ready`
- `deserializes_empty_object_as_defaults`
- `rejects_unknown_fields`
- `resolve_api_key_uses_config_override`
- `resolve_api_key_reads_env_var`
- `resolve_api_key_missing_env_returns_invalid_env_var`
- `resolve_api_key_malformed_prefix_returns_invalid_value` (key = "nope-not-real" → fails)
- `resolve_api_key_modern_sk_proj_prefix_accepted` (key = "sk-proj-abc" → succeeds)
- `validate_retry_zero_attempts_rejected`
- `validate_retry_one_attempt_ok`

**Refs:** Spec §6.

**Commit subject:** `feat(openai): add OpenAIConfig + RetryConfig + api-key resolver`

---

### Task 6: OpenAI `request.rs` — body builder + tool/tool_choice translation + sampling overrides

**Files:** Create `crates/tau-plugins/openai/src/request.rs`; add `pub(crate) mod request;` to `lib.rs`.

**Public surface (crate-private):**

```rust
pub(crate) fn build_chat_completions_body(
    req: &CompletionRequest,
    stream: bool,
) -> Result<serde_json::Value, BuildError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum BuildError {
    #[error("unknown LlmProviderMessage variant")]   UnknownMessageVariant,
    #[error("unknown ContentBlock variant in assistant content")] UnknownContentBlock,
    #[error("could not serialize tool input as JSON: {0}")] JsonSerialize(#[from] serde_json::Error),
}
```

**Translation rules (spec §4.2 + §7):**

- `req.system: Option<String>` → leading `{role:"system", content:<system>}` message in `messages` array.
- `LlmProviderMessage::User { content }` → `{role:"user", content:<flatten_text>}`.
- `LlmProviderMessage::Assistant { content }` → split into `(text, tool_calls)`:
  - `text` = concatenation of `ContentBlock::Text(s)` blocks.
  - `tool_calls[]` = one entry per `ContentBlock::ToolUse(tu)`:
    `{"id": <tu.id>, "type": "function", "function": {"name": <tu.name>, "arguments": <stringified JSON of tu.input>}}`.
    **`arguments` is a JSON-encoded string**, NOT a JSON object — OpenAI wire format peculiarity.
  - Body: `{role:"assistant", content:<text or null if empty>, tool_calls: <array if non-empty>}`.
- `LlmProviderMessage::ToolResult { tool_use_id, content, is_error }`:
  - Body: `{role:"tool", tool_call_id:<tool_use_id>, content:<flatten_text>}`.
  - `is_error` is dropped (no equivalent field in OpenAI; errors live in content).
- `req.tools` non-empty AND `req.tool_choice != ToolChoice::None` → `tools` array.
- `tool_choice` mapping (round-trip ALL FOUR):
  - `Auto` → `"auto"`
  - `None` → `"none"` (or omit; both work — emit `"none"` explicitly).
  - `Required` → `"required"`
  - `Specific { name }` → `{"type":"function","function":{"name":<name>}}`
- Sampling overrides at top level (NOT nested in `options` like Ollama):
  - `req.max_tokens` → `"max_tokens"` (matches OpenAI wire — ollama's `num_predict` is OLLAMA-specific).
  - `req.temperature`, `req.top_p`, `req.seed` → top-level fields.
  - `req.stop_sequences` non-empty → `"stop": [<...>]`.

**Test inventory (~11 unit tests):**
- happy_path_user_text_only
- streaming_flag_propagates
- system_prompt_emitted_as_leading_role_system_message
- multi_block_user_content_concatenated_to_string
- assistant_tool_use_emits_tool_calls_with_real_id
- assistant_tool_use_arguments_is_json_encoded_string (verifies `arguments` is a string, not a JSON object)
- tool_result_message_round_trips_tool_use_id (verifies `tool_call_id` field is set from `tool_use_id`)
- tool_choice_auto_required_specific_round_trip (one test exercising all four)
- tool_choice_none_omits_tools_array_entirely
- sampling_overrides_top_level_no_options_subobject (verifies `max_tokens` not `num_predict` and at top level)
- tools_emitted_with_function_wrapper

**Refs:** Spec §4.2, §7.

**Commit subject:** `feat(openai): translate CompletionRequest to /v1/chat/completions body`

---

### Task 7: OpenAI `response.rs` — batch parser + tool-call id round-trip + finish_reason mapping

**Files:** Create `crates/tau-plugins/openai/src/response.rs`; add `pub(crate) mod response;` to `lib.rs`.

**Public surface (crate-private):**

```rust
pub(crate) fn parse_chat_completions_response(body: &str) -> Result<CompletionResponse, ParseError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ParseError {
    #[error("could not decode response JSON: {0}")] Decode(#[from] serde_json::Error),
    #[error("tool_call {name} arguments not valid JSON: {source}")] ToolUseInput {
        name: String,
        #[source] source: serde_json::Error,
    },
    #[error("unexpected choices count: got {got}, expected exactly 1")] UnexpectedChoicesCount { got: usize },
}
```

**Parsing (spec §4.3):**

Typed deser shape:

```rust
#[derive(Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChoice>,
    #[serde(default)] usage: Option<OpenAIUsage>,
}
#[derive(Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
    #[serde(default)] finish_reason: Option<String>,
}
#[derive(Deserialize)]
struct OpenAIMessage {
    #[serde(default)] content: Option<String>,
    #[serde(default)] tool_calls: Option<Vec<OpenAIToolCall>>,
}
#[derive(Deserialize)]
struct OpenAIToolCall {
    #[serde(default)] id: Option<String>,
    function: OpenAIToolFn,
}
#[derive(Deserialize)]
struct OpenAIToolFn { name: String, arguments: String }  // arguments is a JSON-string
#[derive(Deserialize)]
struct OpenAIUsage { #[serde(default)] prompt_tokens: Option<u32>, #[serde(default)] completion_tokens: Option<u32> }
```

Translation:
- `choices.len() != 1` → `Err(ParseError::UnexpectedChoicesCount)` (v0.1 only handles n=1).
- `text` = `choices[0].message.content.unwrap_or_default()`.
- For each `tool_calls[i]`:
  - `id` = `tc.id.unwrap_or_else(|| format!("openai-tool-{i}"))` (defensive synthesis; OpenAI almost always supplies real ids).
  - `input` = `serde_json::from_str::<tau_domain::Value>(&tc.function.arguments)?` (the arguments-string contains JSON; parse it).
- `stop_reason` mapping:
  - `"stop"` → `EndTurn`
  - `"length"` → `MaxTokens`
  - `"tool_calls"` → `ToolUse`
  - `"content_filter"` → `Error` (with `tracing::warn!`)
  - `"function_call"` (deprecated) → `ToolUse` (with `tracing::warn!` about legacy field)
  - `None` → `EndTurn` (defensive)
  - other → `EndTurn` with warn.
- `usage`: when both `prompt_tokens` AND `completion_tokens` are `Some(u32)`, `Some(TokenUsage::new(p, c))`; else `None`.
- Return via `tau_ports::fixtures::make_completion_response(text, tool_uses, stop_reason, usage)`.

**Test inventory (~7 unit tests):**
- parse_text_only_response
- parse_response_with_tool_call_preserves_real_id (id from cassette = "call_abc"; assert preserved)
- parse_response_with_missing_id_synthesizes_fallback (rare edge case)
- parse_response_arguments_string_parses_to_value (cassette includes `"arguments":"{\"text\":\"hi\"}"`; assert parsed)
- parse_response_finish_reason_tool_calls_maps_to_tool_use
- parse_response_finish_reason_length_maps_to_max_tokens
- parse_response_zero_choices_returns_unexpected_count_error
- parse_response_with_usage

**Refs:** Spec §4.3.

**Commit subject:** `feat(openai): parse /v1/chat/completions batch response`

---

### Task 8: OpenAI `error.rs` — TYPED `map_response_error` + `map_client_error`

**Files:** Create `crates/tau-plugins/openai/src/error.rs`; add `pub(crate) mod error;` to `lib.rs`.

**Crucial: `map_response_error` signature differs from anthropic/ollama** — it gains a `headers: &reqwest::header::HeaderMap` param so 429 responses can populate `RateLimited { retry_after_seconds: Option<u32> }`.

```rust
pub(crate) fn map_response_error(
    status: reqwest::StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> tau_ports::LlmError;

pub(crate) fn map_client_error(err: ClientError) -> tau_ports::LlmError;

// ClientError lives in this module (matches anthropic/ollama precedent — single point of LlmError translation).
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub(crate) enum ClientError {
    #[error("transport: {0}")] Transport(reqwest::Error),
    #[error("retries exhausted: {status} after {attempts} attempts")]
    Exhausted { status: reqwest::StatusCode, attempts: u32 },
}
```

**Mapping logic (spec §4.4):**

OpenAI envelope:
```rust
#[derive(Deserialize)]
struct OpenAIErrorBody { error: OpenAIErrorDetail }
#[derive(Deserialize)]
struct OpenAIErrorDetail {
    message: String,
    #[serde(default, rename = "type")] error_type: Option<String>,
    #[serde(default)]                  code: Option<String>,
}
```

`map_response_error(status, headers, body)`:
1. Parse body as `OpenAIErrorBody` (defensive — fall back to raw body if parse fails).
2. Branch by status:
   - 400 → `LlmError::InvalidRequest { reason: "openai bad request: <type/code>: <msg>" }`.
   - 401 | 403 → `LlmError::Auth { message: <detail.message> }`.
   - 404 → `LlmError::InvalidRequest { reason: "openai model not found: <msg>" }` (no typed `ModelNotFound` variant per spec §2.3).
   - 429 → `LlmError::RateLimited { retry_after_seconds: parse_retry_after(headers) }` where `parse_retry_after` reads the `retry-after` header and parses as integer seconds.
   - 500..=599 → `LlmError::Provider { message: "openai server error (<status>): <msg>" }` (retryable).
   - 408 → `LlmError::Transport { message }` (synthesized from timeout in client.rs).
   - other → `LlmError::Provider { message: "openai unexpected status (<status>): <msg>" }`.

`map_client_error(err)`:
- `Transport(e)` → `LlmError::Transport { message: e.to_string() }`.
- `Exhausted { status, attempts }`:
  - status==429 → `LlmError::RateLimited { retry_after_seconds: None }` (already exhausted; no point passing the last header).
  - status==408 → `LlmError::Transport { message: "openai retries exhausted on timeout (<attempts> attempts)" }`.
  - status in 5xx → `LlmError::Provider { message: "openai retries exhausted (<attempts> attempts, last status <status>)" }`.

**Test inventory (~6 unit tests):**
- map_400_returns_invalid_request
- map_401_returns_auth
- map_404_returns_invalid_request_with_remediation
- map_429_with_retry_after_header_populates_seconds (assert `retry_after_seconds: Some(5)`)
- map_429_without_retry_after_returns_none (assert `retry_after_seconds: None`)
- map_500_returns_provider_retryable
- map_client_error_transport_exhausted_429_returns_rate_limited

**Refs:** Spec §4.4.

**Commit subject:** `feat(openai): map HTTP errors to typed LlmError variants`

---

### Task 9: OpenAI `client.rs` — HTTP client + retry + bearer auth

**Files:** Create `crates/tau-plugins/openai/src/client.rs`; add `pub(crate) mod client;` to `lib.rs`.

**Public surface (crate-private):**

```rust
pub(crate) struct OpenAIClient {
    inner: reqwest::Client,
    base_url: String,
    api_key: SecretString,
    organization: Option<String>,
    retry: RetryConfig,
}

impl OpenAIClient {
    pub(crate) fn new(
        inner: reqwest::Client, base_url: String, api_key: SecretString,
        organization: Option<String>, retry: RetryConfig,
    ) -> Self;

    pub(crate) async fn post_chat_completions(
        &self,
        body: &serde_json::Value,
        stream: bool,
    ) -> Result<reqwest::Response, ClientError>;
}
```

**Implementation skeleton (spec §4.1):**

Mirrors `crates/tau-plugins/ollama/src/client.rs` with these specifics:

- URL: `format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'))`.
- Headers per request:
  - `authorization: Bearer {api_key}` (always; required).
  - `content-type: application/json`.
  - `accept: text/event-stream` when `stream == true`.
  - `OpenAI-Organization: {organization}` when `Some(org)`.
- Retry classification: identical to ollama (`is_retryable_status` matches 429, 503, 5xx≠501).
- `Decision::{Return, Retry { delay_ms, status }, Error}` — same shape.
- Tracing target: `openai_plugin::retry`.

**Test inventory (~5 in-process `tokio::net::TcpListener` tests):**
- post_chat_completions_happy_path_sends_authorization_header
- post_chat_completions_with_organization_sends_org_header
- post_chat_completions_429_then_200_succeeds_after_retry
- post_chat_completions_429_with_retry_after_honors_header
- post_chat_completions_exhausts_after_max_attempts (asserts `ClientError::Exhausted { status: 429, attempts: 3 }`)

**Refs:** Spec §4.1.

**Commit subject:** `feat(openai): HTTP client with retry + required bearer auth`

---

### Task 10: OpenAI `stream.rs` — SSE parser + `ToolUseAccumulator`

**Files:** Create `crates/tau-plugins/openai/src/stream.rs`; add `pub(crate) mod stream;` to `lib.rs`.

**Public surface (crate-private):**

```rust
pub(crate) async fn parse_sse(body: reqwest::Response) -> Result<tau_ports::CompletionStream, tau_ports::LlmError>;
```

**Implementation skeleton (spec §5):**

Uses `eventsource_stream::Eventsource` (workspace dep, reused from anthropic). Each event's `data:` line is parsed as `StreamEvent`. The terminal `data: [DONE]` is consumed silently (do NOT try to parse it as JSON).

```rust
#[derive(Deserialize)]
struct StreamEvent {
    #[serde(default)] choices: Vec<StreamChoice>,
    #[serde(default)] usage: Option<StreamUsage>,
}
#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    #[serde(default)] finish_reason: Option<String>,
}
#[derive(Deserialize)]
struct StreamDelta {
    #[serde(default)] content: Option<String>,
    #[serde(default)] tool_calls: Option<Vec<StreamToolCallDelta>>,
}
#[derive(Deserialize)]
struct StreamToolCallDelta {
    #[serde(default)] index: u32,
    #[serde(default)] id: Option<String>,           // present on first delta only
    #[serde(default)] function: Option<StreamToolFnDelta>,
}
#[derive(Deserialize)]
struct StreamToolFnDelta {
    #[serde(default)] name: Option<String>,         // present on first delta only
    #[serde(default)] arguments: Option<String>,    // accumulated across deltas
}
```

**ToolUseAccumulator (per spec §5.2):**

```rust
struct ToolUseAccumulator {
    // index → (id, name, accumulated_arguments_string)
    entries: BTreeMap<u32, (String, String, String)>,
}
impl ToolUseAccumulator {
    fn observe(&mut self, delta: &StreamToolCallDelta);
    /// Drain all entries, parse arguments as JSON, emit one ToolUse per entry.
    fn drain_into_chunks(self) -> Result<Vec<ToolUse>, LlmError>;
}
```

When a `finish_reason` event arrives:
1. Drain the accumulator → emit zero or more `CompletionChunk::ToolUse(_)` chunks.
2. Map `finish_reason` (same logic as `response.rs::parse_chat_completions_response` — share the helper or duplicate the small match).
3. Emit `CompletionChunk::Finish { stop_reason, usage }`.
4. Return.

The `[DONE]` line that follows is consumed silently. If the stream ends without `finish_reason` AND with non-empty accumulator → `LlmError::Stream { message: "openai stream ended without finish_reason" }`.

If a tool_call's `arguments` doesn't parse as JSON → `LlmError::Stream { message: "openai tool_call arguments not valid JSON" }`.

**Test inventory (~7 unit tests):**
- stream_text_only_yields_chunks_then_finish
- stream_with_tool_use_accumulator_emits_one_tool_use (multiple `function.arguments` deltas accumulate to one `ToolUse`)
- stream_two_tool_calls_indexed_separately
- stream_tool_call_id_from_first_delta_preserved
- stream_tool_call_arguments_invalid_json_yields_stream_error
- stream_finish_reason_tool_calls_maps_to_tool_use
- stream_truncated_without_finish_reason_yields_stream_error
- stream_done_sentinel_consumed_silently

**Refs:** Spec §5.

**Commit subject:** `feat(openai): SSE stream parser with ToolUseAccumulator`

---

### Task 11: OpenAI `plugin.rs` + `main.rs` — `OpenAIPlugin` LlmBackend impl

**Files:** Create `crates/tau-plugins/openai/src/plugin.rs`; rewrite `main.rs`; add `pub mod plugin;` to `lib.rs`. **Strip `#[allow(dead_code)]` from prior modules** now that plugin.rs is the non-test caller.

**Public surface:**

```rust
pub struct OpenAIPlugin { client: OpenAIClient }

impl tau_plugin_sdk::Configure for OpenAIPlugin {
    type Config = crate::config::OpenAIConfig;
    fn from_config(cfg: Self::Config) -> Result<Self, tau_plugin_sdk::ConfigError>;
}

impl tau_ports::LlmBackend for OpenAIPlugin {
    fn name(&self) -> &str { "openai" }
    async fn complete(...) -> Result<CompletionResponse, LlmError>;
    async fn stream(...) -> Result<CompletionStream, LlmError>;
}
```

**Implementation skeleton (spec §6):**

`from_config`:
1. `let api_key = resolve_api_key(&cfg)?`
2. `validate_retry(&cfg.retry)?`
3. Build `reqwest::Client` with `.timeout(cfg.request_timeout())` + `.user_agent("tau-openai-plugin/{CARGO_PKG_VERSION}")`. Map error → `ConfigError::InvalidValue`.
4. `OpenAIClient::new(inner, cfg.base_url, SecretString::new(api_key.into()), cfg.organization, cfg.retry)`.

`complete`:
1. `build_chat_completions_body(&req, false)` → map `BuildError` → `LlmError::Internal { message: format!("openai: build request body: {e}") }`.
2. `client.post_chat_completions(&body, false).await` → `map_client_error`.
3. If `!status.is_success()`: read body, `map_response_error(status, resp.headers(), &body)`. **Note: pass `headers()` BEFORE consuming the response via `text()`** — once `text()` consumes the response, headers are no longer accessible. So extract headers first: `let headers = resp.headers().clone(); let body = resp.text().await?; map_response_error(status, &headers, &body)`.
4. Else: `parse_chat_completions_response(&body)` → map `ParseError` → `LlmError::Internal { message: format!("openai: parse response: {e}") }`.

`stream`:
1. Same up to `post_chat_completions(&body, true).await`.
2. Non-success: same `map_response_error` flow.
3. Else: `parse_sse(resp).await`.

`main.rs`:

```rust
use openai_plugin_lib::plugin::OpenAIPlugin;
use tau_plugin_sdk::{run_llm_backend_with_config, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_llm_backend_with_config::<OpenAIPlugin>(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    ).await
}
```

**Test inventory (3 unit tests):**
- from_config_valid_api_key_constructs_plugin
- from_config_invalid_retry_max_attempts_zero_returns_invalid_value
- name_returns_openai

(End-to-end behavior is tested in Tasks 12-13 via cassette replay.)

**`#[allow(dead_code)]` strip:** Remove from `config.rs` (resolve_api_key, validate_retry), `request.rs` (BuildError, build_chat_completions_body), `response.rs` (ParseError, parse_chat_completions_response), `error.rs` (ClientError, map_response_error, map_client_error), `client.rs` (OpenAIClient::new, post_chat_completions), `stream.rs` (parse_sse). Run clippy after each removal; if clippy still flags one, restore that single annotation.

**Refs:** Spec §6.

**Commit subject:** `feat(openai): OpenAIPlugin LlmBackend impl + main entrypoint`

---

### Task 12: OpenAI 6 batch cassettes + `tests/complete.rs`

**Files:**
- Create: `crates/tau-plugins/openai/tests/cassettes/complete_happy_path.yaml`
- Create: `crates/tau-plugins/openai/tests/cassettes/complete_with_system_prompt.yaml`
- Create: `crates/tau-plugins/openai/tests/cassettes/complete_with_tools.yaml`
- Create: `crates/tau-plugins/openai/tests/cassettes/complete_429_then_success.yaml`
- Create: `crates/tau-plugins/openai/tests/cassettes/complete_401_auth_failure.yaml`
- Create: `crates/tau-plugins/openai/tests/cassettes/complete_400_bad_request.yaml`
- Create: `crates/tau-plugins/openai/tests/common/mod.rs` (test_config + helpers; uses shared cassette via `tau-plugin-test-support`)
- Create: `crates/tau-plugins/openai/tests/complete.rs`

**Cassette format (spec §8.1):** YAML; URI `/v1/chat/completions`; response bodies are valid OpenAI Chat Completions JSON.

Example `complete_happy_path.yaml`:

```yaml
- request:
    method: POST
    uri: /v1/chat/completions
    body: |-
      placeholder
  response:
    status: 200
    headers:
      content-type: application/json
    body: |-
      {"id":"chatcmpl-abc","object":"chat.completion","created":1700000000,"model":"gpt-4o-mini","choices":[{"index":0,"message":{"role":"assistant","content":"Hi there"},"finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":3,"total_tokens":15}}
```

`tests/common/mod.rs` shape (mirrors anthropic pattern):

```rust
#![allow(dead_code)]

pub use tau_plugin_test_support::cassette;

use openai_plugin_lib::config::OpenAIConfig;
use tau_ports::{CompletionRequest, CompletionResponse, ContentBlock, LlmProviderMessage};

pub fn sample_request() -> CompletionRequest {
    let mut req = CompletionRequest::new("gpt-4o-mini".into());
    req.messages.push(LlmProviderMessage::user(vec![
        ContentBlock::Text("say hi".into()),
    ]));
    req.max_tokens = Some(20);
    req
}

pub fn extract_text(resp: &CompletionResponse) -> &str { &resp.text }

pub fn test_config(base_url: String) -> OpenAIConfig {
    let mut cfg = OpenAIConfig::default();
    cfg.api_key = Some("sk-test".into());
    cfg.base_url = base_url;
    cfg
}

pub fn test_config_with_retry(base_url: String, max_attempts: u32, base_delay_ms: u64) -> OpenAIConfig {
    let mut cfg = test_config(base_url);
    cfg.retry.max_attempts = max_attempts;
    cfg.retry.base_delay_ms = base_delay_ms;
    cfg
}
```

**6 integration tests in `complete.rs`:**

```rust
#[tokio::test]
async fn complete_happy_path() { /* 200 + text */ }

#[tokio::test]
async fn complete_with_system_prompt() {
    // Verify the request body contained {"role":"system","content":"..."} as messages[0].
}

#[tokio::test]
async fn complete_with_tools_round_trips_tool_call_id() {
    // Cassette returns tool_call with id="call_abc"; assert resp.tool_uses[0].id == "call_abc".
    // (Distinct from Ollama where ids are synthesized.)
}

#[tokio::test]
async fn complete_429_then_success_retries_with_typed_rate_limited_path() {
    // Cassette: 1× 429 + Retry-After:0 + 200. Plugin retries; final result is Ok.
    // Verify 2 attempts.
}

#[tokio::test]
async fn complete_401_returns_typed_auth_error() {
    // Cassette: 401 with body {"error":{"message":"Invalid API key","type":"invalid_request_error"}}
    let err = ...; let LlmError::Auth { message } = err else { panic!() };
    assert!(message.contains("Invalid API key"));
}

#[tokio::test]
async fn complete_400_returns_typed_invalid_request() {
    let err = ...; let LlmError::InvalidRequest { reason } = err else { panic!() };
    assert!(reason.contains("openai bad request"));
}
```

**Refs:** Spec §8.1, §8.2.

**Commit subject:** `test(openai): batch cassettes + complete.rs integration tests`

---

### Task 13: OpenAI 3 streaming cassettes + `tests/streaming.rs`

**Files:**
- Create: 3 streaming cassette YAMLs:
  - `stream_text_only.yaml` — SSE: 2 content deltas + finish_reason event + `[DONE]`.
  - `stream_with_tool_use.yaml` — SSE: tool_call deltas accumulated across multiple events; arguments fragments concatenate to `{"text":"hi"}`.
  - `stream_truncated_response.yaml` — SSE ends without `finish_reason` event.
- Create: `crates/tau-plugins/openai/tests/streaming.rs`

**SSE cassette caveat:** YAML's `|-` chomps trailing newlines. SSE event boundaries are `\n\n`. Use `body: |+` (chomp keep) to preserve the trailing `\n\n` after the last event. Same pitfall the anthropic plugin documented in its plan-erratum.

Example `stream_text_only.yaml`:

```yaml
- request:
    method: POST
    uri: /v1/chat/completions
    body: |-
      placeholder
  response:
    status: 200
    headers:
      content-type: text/event-stream
    body: |+
      data: {"id":"chatcmpl-abc","choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}]}

      data: {"id":"chatcmpl-abc","choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":null}]}

      data: {"id":"chatcmpl-abc","choices":[{"index":0,"delta":{"content":" there"},"finish_reason":null}]}

      data: {"id":"chatcmpl-abc","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":3}}

      data: [DONE]

```

**3 integration tests:**
- `stream_text_only_yields_chunks_then_finish` — assert `Text("Hi")`, `Text(" there")`, `Finish { EndTurn, Some(usage) }`.
- `stream_with_tool_use_accumulates_into_one_chunk` — multiple delta lines; assert one `ToolUse` chunk with `id="call_abc", name="echo", input=Object{text:"hi"}`.
- `stream_truncated_yields_stream_error_at_end` — assert last item is `Err(LlmError::Stream { message contains "ended without finish_reason" })`.

**Refs:** Spec §8.1.

**Commit subject:** `test(openai): streaming cassettes + integration tests`

---

### Task 14: OpenAI live smoke tests + re-record helper

**Files:**
- Create: `crates/tau-plugins/openai/tests/live.rs`
- Create: `scripts/rerecord-openai-cassettes.sh` (chmod +x)

**`live.rs` skeleton (spec §8.5):**

```rust
//! Live smoke tests against api.openai.com.
//!
//! Setup:
//!   export OPENAI_API_KEY=sk-proj-...
//!   TAU_OPENAI_LIVE_TESTS=1 cargo test -p openai --test live -- --ignored --nocapture
//!
//! Cost: ~$0.001 per smoke run on gpt-4o-mini.

mod common;

use futures_util::StreamExt;
use openai_plugin_lib::{config::OpenAIConfig, plugin::OpenAIPlugin};
use tau_plugin_sdk::Configure;
use tau_ports::{CompletionChunk, CompletionRequest, ContentBlock, LlmBackend, LlmProviderMessage};

fn live_config() -> OpenAIConfig {
    let api_key = std::env::var("OPENAI_API_KEY")
        .expect("OPENAI_API_KEY required for live tests");
    let mut cfg = OpenAIConfig::default();
    cfg.api_key = Some(api_key);
    cfg
}

fn live_request() -> CompletionRequest {
    let mut req = CompletionRequest::new("gpt-4o-mini".into());
    req.messages.push(LlmProviderMessage::user(vec![ContentBlock::Text(
        "say hi in exactly 3 words".into(),
    )]));
    req.max_tokens = Some(20);
    req
}

#[tokio::test]
#[ignore = "live: requires TAU_OPENAI_LIVE_TESTS=1 + OPENAI_API_KEY"]
async fn live_complete_smoke() {
    if std::env::var("TAU_OPENAI_LIVE_TESTS").is_err() {
        eprintln!("skipping: TAU_OPENAI_LIVE_TESTS not set");
        return;
    }
    let plugin = OpenAIPlugin::from_config(live_config()).unwrap();
    let resp = plugin.complete(live_request()).await.unwrap();
    assert!(!resp.text.is_empty());
    eprintln!("live response text: {:?}", resp.text);
}

#[tokio::test]
#[ignore = "live: requires TAU_OPENAI_LIVE_TESTS=1 + OPENAI_API_KEY"]
async fn live_stream_smoke() {
    if std::env::var("TAU_OPENAI_LIVE_TESTS").is_err() {
        eprintln!("skipping: TAU_OPENAI_LIVE_TESTS not set");
        return;
    }
    let plugin = OpenAIPlugin::from_config(live_config()).unwrap();
    let mut stream = plugin.stream(live_request()).await.unwrap();
    let mut text_chunks = 0;
    let mut got_finish = false;
    while let Some(item) = stream.next().await {
        match item {
            Ok(CompletionChunk::Text { delta }) => {
                text_chunks += 1;
                eprintln!("delta: {delta:?}");
            }
            Ok(CompletionChunk::Finish { stop_reason, usage }) => {
                got_finish = true;
                eprintln!("finish stop_reason={stop_reason:?} usage={usage:?}");
            }
            Ok(other) => eprintln!("other: {other:?}"),
            Err(e) => panic!("stream error: {e:?}"),
        }
    }
    assert!(text_chunks > 0);
    assert!(got_finish);
}
```

**`scripts/rerecord-openai-cassettes.sh`:** mirrors `scripts/rerecord-anthropic-cassettes.sh` (cost note ~$0.001/run; v0.1 hand-authored cassettes; live test is the drift-detection mechanism).

**Refs:** Spec §8.5.

**Commit subject:** `test(openai): live smoke tests + re-record helper`

---

### Task 15: `tau-plugin-conformance` crate — `ConformanceSuite` + 6 tests

**Files:**
- Create: `crates/tau-plugin-conformance/src/lib.rs` (replaces stub)
- Create: `crates/tau-plugin-conformance/src/cases/mod.rs`
- Create: `crates/tau-plugin-conformance/src/cases/{batch_happy_path, batch_with_tools, streaming_text, streaming_tool_use, error_rate_limited, error_auth}.rs`

**Public surface:**

```rust
//! Parameterized conformance test suite for tau LlmBackend plugins.
//!
//! Charter: tests **mechanical correctness** (request/response shape,
//! error typing, stream chunk ordering) NOT response quality (NG7).
//!
//! Usage from a plugin's tests/conformance.rs:
//!
//! ```ignore
//! use tau_plugin_conformance::ConformanceSuite;
//! #[tokio::test]
//! async fn run_conformance_suite() {
//!     let plugin = build_plugin();
//!     ConformanceSuite::default().run(&plugin, "tests/conformance-cassettes").await;
//! }
//! ```

#[non_exhaustive]
#[derive(Debug, Default)]
pub struct ConformanceSuite { /* configurable knobs added later as needed */ }

impl ConformanceSuite {
    /// Run the full battery against `plugin`, loading per-test cassettes
    /// from `cassettes_dir`. Each test has a fixed file name:
    ///   batch_happy_path.yaml, batch_with_tools.yaml,
    ///   streaming_text.yaml, streaming_tool_use.yaml,
    ///   error_rate_limited.yaml, error_auth.yaml
    ///
    /// Panics on the first failure with a descriptive message.
    pub async fn run<B: tau_ports::LlmBackend>(&self, plugin: &B, cassettes_dir: &std::path::Path);
}
```

**Architecture detail:** the suite **does NOT spawn a cassette replayer per test** — that's the plugin's own concern. The plugin must already be configured (in the test shim) to point at a per-test cassette server. The suite's responsibility is to send a pre-canned `CompletionRequest` and assert on the response/stream/error.

Wait — actually, re-reading: that's wrong. The suite SHOULD spawn the replayer. The plugin is configured in the test shim to be POINTED AT some base URL; the suite spawns the replayer per-test, gets its URL, and the test shim re-configures the plugin against that URL.

The tension: the plugin is constructed once in the shim; the suite spawns replayers per-test. So the plugin construction needs to be parameterized over the URL. Two API options:

**Option A — caller provides a builder**:
```rust
pub trait PluginBuilder: Send + Sync {
    type Plugin: tau_ports::LlmBackend;
    fn build(&self, base_url: String) -> Self::Plugin;
}
impl ConformanceSuite {
    pub async fn run<P: PluginBuilder>(&self, builder: &P, cassettes_dir: &std::path::Path);
}
```

**Option B — caller provides a closure** (idiomatic, no trait needed):
```rust
impl ConformanceSuite {
    pub async fn run<B, F>(&self, build_plugin: F, cassettes_dir: &std::path::Path)
    where
        B: tau_ports::LlmBackend,
        F: Fn(String) -> B + Send + Sync,
    ;
}
```

**Use Option B** — simpler, avoids defining yet another trait, idiomatic Rust async.

The shim then becomes:

```rust
let cfg_base = base_openai_test_config();
ConformanceSuite::default().run(
    |base_url| {
        let mut cfg = cfg_base.clone();
        cfg.base_url = base_url;
        OpenAIPlugin::from_config(cfg).expect("build plugin")
    },
    Path::new("tests/conformance-cassettes"),
).await;
```

**Per-test logic:**

Each `cases/<test>.rs` exports one async function `pub async fn run<B>(plugin_builder: &F, cassettes_dir: &Path)` where `F: Fn(String) -> B`. The test:
1. `let server = tau_plugin_test_support::cassette::replay(cassettes_dir.join("<test>.yaml")).await;`
2. `let plugin = plugin_builder(server.uri().into());`
3. Call the plugin (e.g. `plugin.complete(sample_request()).await`).
4. Assert on the result.

**The 6 tests (assertions, not full code — see spec §8.3):**

1. **`batch_happy_path`**: assert non-empty `text`, `stop_reason` is one of `EndTurn|MaxTokens|StopSequence`, `usage` populated when applicable.
2. **`batch_with_tools`**: cassette returns one tool_call; assert `tool_uses.len() == 1`, `id` non-empty, `name` non-empty, `input` is a JSON object (not null/scalar).
3. **`streaming_text`**: drain stream; assert at least one `Text` chunk and exactly one `Finish` (which is the last chunk).
4. **`streaming_tool_use`**: assert at least one `ToolUse` chunk before `Finish`; `tu.input` is a JSON object.
5. **`error_rate_limited`**: cassette is N×429 (where N == `max_attempts`); assert `Err(LlmError::RateLimited { .. })`.
6. **`error_auth`**: 401 cassette; assert `Err(LlmError::Auth { .. })`.

**Sample request helpers** (private to the conformance crate):

```rust
fn sample_request(model: &str) -> CompletionRequest {
    let mut req = CompletionRequest::new(model.into());
    req.messages.push(LlmProviderMessage::user(vec![ContentBlock::Text("hi".into())]));
    req.max_tokens = Some(20);
    req
}

fn sample_request_with_tools(model: &str) -> CompletionRequest {
    let mut req = sample_request(model);
    req.tools.push(tau_ports::fixtures::make_tool_spec(
        "echo".into(),
        "echo input".into(),
        tau_domain::Value::Object(Default::default()),
    ));
    req
}
```

The `model` argument is passed-through; each plugin's shim picks a model name that its cassettes match.

**Test inventory inside `tau-plugin-conformance` itself:** 0 tests in this crate (the crate is invoked from plugin integration tests; testing it standalone would require constructing a fake `LlmBackend`).

**Refs:** Spec §8.3.

**Commit subject:** `feat(conformance): parameterized LlmBackend conformance suite`

---

### Task 16: OpenAI conformance shim + 6 conformance cassettes

**Files:**
- Create: `crates/tau-plugins/openai/tests/conformance-cassettes/{batch_happy_path,batch_with_tools,streaming_text,streaming_tool_use,error_rate_limited,error_auth}.yaml`
- Create: `crates/tau-plugins/openai/tests/conformance.rs`

**Cassettes:** 6 OpenAI-shaped responses, one per conformance test. The `error_rate_limited.yaml` must contain N=3 successive 429 responses (matches the suite's expectation that the plugin exhausts retries).

**Shim (`tests/conformance.rs`):**

```rust
//! Run the conformance suite against the OpenAI plugin.

mod common;

use openai_plugin_lib::plugin::OpenAIPlugin;
use std::path::Path;
use tau_plugin_conformance::ConformanceSuite;
use tau_plugin_sdk::Configure;

#[tokio::test]
async fn run_conformance_suite() {
    let cassettes = Path::new("tests/conformance-cassettes");
    ConformanceSuite::default()
        .run(
            |base_url: String| {
                let mut cfg = openai_plugin_lib::config::OpenAIConfig::default();
                cfg.api_key = Some("sk-test".into());
                cfg.base_url = base_url;
                cfg.retry.max_attempts = 3;
                cfg.retry.base_delay_ms = 0;
                OpenAIPlugin::from_config(cfg).expect("build plugin")
            },
            cassettes,
        )
        .await;
}
```

**Verification:** `cargo test -p openai --test conformance -- --nocapture` passes all 6 conformance tests.

**Refs:** Spec §8.3, §9.3.

**Commit subject:** `test(openai): conformance suite shim + 6 cassettes`

---

### Task 17: Migrate Anthropic — typed error variants + conformance suite

**Files:**
- Modify: `crates/tau-plugins/anthropic/src/error.rs` — `map_response_error` signature gains `headers: &HeaderMap` param; returns typed variants instead of `Internal`.
- Modify: `crates/tau-plugins/anthropic/src/client.rs` — call site of `map_response_error` plumbs headers through.
- Modify: `crates/tau-plugins/anthropic/src/plugin.rs` — same call-site plumbing where `map_response_error` is invoked from `complete()` / `stream()`. Extract `headers` BEFORE consuming the response via `text()`.
- Modify: `crates/tau-plugins/anthropic/tests/complete.rs` — update assertions from `LlmError::Internal { message contains "rate limited" }` to `LlmError::RateLimited { retry_after_seconds }`, etc. 1-for-1 mapping per cassette.
- Create: `crates/tau-plugins/anthropic/tests/conformance.rs` (suite shim).
- Create: `crates/tau-plugins/anthropic/tests/conformance-cassettes/*.yaml` (6 Anthropic-shaped cassettes).
- Modify: `crates/tau-plugins/anthropic/Cargo.toml` — add `tau-plugin-conformance = { workspace = true }` to dev-dependencies.

**Migration mapping (spec §4.5):**

```
400  → InvalidRequest { reason: format!("anthropic bad request: {error_type}: {message}") }
401, 403 → Auth { message: <error.message> }
404  → InvalidRequest { reason: format!("anthropic not found: {message}") }
429  → RateLimited { retry_after_seconds: parse_retry_after(headers) }
500..=599 → Provider { message: format!("anthropic server error ({status}): {error_type}: {message}") }
other → Provider { message: format!("anthropic unexpected status ({status}): ...") }
```

`map_client_error`:
```
Transport(e) → Transport { message: e.to_string() }
Exhausted { 429, attempts } → RateLimited { retry_after_seconds: None }
Exhausted { 408, attempts } → Transport { message: ... }
Exhausted { 5xx, attempts } → Provider { message: ... }
```

The `Internal` variant is RETAINED ONLY for plugin-internal translation errors (e.g., `LlmError::Internal { message: format!("anthropic: build request body: {e}") }` where `e: BuildError`). This narrows the variant's role; the escape-hatch registry entry stays.

**Test assertion updates (`complete.rs`):**
- `complete_429_then_success` retries successfully (no error); no assertion change beyond verifying request count.
- `complete_429_exhausted_returns_internal_error` → renamed `complete_429_exhausted_returns_rate_limited` and asserts `LlmError::RateLimited { .. }`.
- `complete_401_auth_failure_does_not_retry` → asserts `LlmError::Auth { .. }`.
- `complete_400_bad_request_does_not_retry` → asserts `LlmError::InvalidRequest { .. }`.

**Conformance shim:** mirror Task 16's shape but with `AnthropicPlugin::from_config` and Anthropic test config.

**Conformance cassettes:** 6 Anthropic-shaped cassettes (URI `/v1/messages`, response bodies match Anthropic Messages API shape).

**Verification:** All anthropic integration tests pass; conformance suite runs cleanly.

**Refs:** Spec §4.5, §9.

**Commit subject:** `refactor(anthropic): typed LlmError variants + conformance suite integration`

---

### Task 18: Migrate Ollama — typed error variants + conformance suite

**Files:**
- Modify: `crates/tau-plugins/ollama/src/error.rs` — `map_response_error` signature gains `headers: &HeaderMap`; typed mapping per spec §4.5.
- Modify: `crates/tau-plugins/ollama/src/client.rs` — call-site plumbing.
- Modify: `crates/tau-plugins/ollama/src/plugin.rs` — extract headers before `text()`.
- Modify: `crates/tau-plugins/ollama/tests/complete.rs` — assertion updates.
- Create: `crates/tau-plugins/ollama/tests/conformance.rs`.
- Create: `crates/tau-plugins/ollama/tests/conformance-cassettes/*.yaml`.
- Modify: `crates/tau-plugins/ollama/Cargo.toml` — add `tau-plugin-conformance` dev-dep.

**Migration mapping (spec §4.5):**

```
400 → InvalidRequest { reason: format!("ollama bad request: {detail.error}") }
401, 403 → Auth { message: detail.error }
404 → InvalidRequest { reason: format!("ollama model not found (run `ollama pull <model>` first): {detail.error}") }
   // Preserve the existing remediation hint inline.
429 → RateLimited { retry_after_seconds: parse_retry_after(headers) }
503 → Provider { message: format!("ollama server: {detail.error}") }
   // 503-on-model-load: still Provider (retryable via is_retryable()).
500..=599 → Provider { ... }
other → Provider { ... }
```

`map_client_error` follows the same pattern as anthropic.

**Critical: load-bearing 503 retry test stays passing.** The existing `complete_503_model_loading_then_success_retries` test asserts retries happen before the result. The retry path is in `client.rs` (status-classification untouched). The typed-variant change only manifests when retries EXHAUST — that's the case the tests verify. So the load-bearing test continues to pass without changes.

**Test assertion updates (`complete.rs`):**
- `complete_503_model_loading_then_success_retries` — no change (success path).
- `complete_404_model_not_pulled_includes_remediation_hint` → asserts `LlmError::InvalidRequest { reason }` AND `reason.contains("ollama pull")`.
- `complete_400_bad_request_does_not_retry` → asserts `LlmError::InvalidRequest { reason }`.

**Conformance shim + cassettes:** mirror Task 16/17. Ollama cassettes are NDJSON shape (URI `/api/chat`).

**Verification:** All 59 ollama tests pass + new conformance shim adds 1 (which internally drives 6 conformance tests).

**Refs:** Spec §4.5, §9.

**Commit subject:** `refactor(ollama): typed LlmError variants + conformance suite integration`

---

### Task 19: ADR-0009 — Typed-error migration + conformance suite charter

**Files:**
- Create: `docs/decisions/0009-llm-error-typing-and-conformance.md`

**ADR shape (template per `docs/decisions/template.md`):**

- Title: `ADR-0009: Typed `LlmError` migration policy + conformance suite charter`
- Status: `Proposed` (changes to `Accepted` at Task 22).
- Date: today's date.
- Supersedes: —
- Closes: ADR-0008 §17 (conformance test suite deferral).
- Amends: —
- Refines: ADR-0007 (escape-hatch registry rule applies to the `LlmError::Internal` callsite reduction).

**Sections:**

1. **Context**: Three real LLM-backend plugins exist. Plugins were uniformly mapping non-2xx HTTP responses to `LlmError::Internal { message }`; callers couldn't branch on retry-eligibility, auth failures, etc.

2. **Decision A — typed mapping policy**: All plugins MUST emit typed `LlmError` variants (`RateLimited`, `Auth`, `InvalidRequest`, `Transport`, `Provider`, `Stream`) for HTTP-mapped failures. `Internal` is reserved for plugin-internal translation errors only. The escape-hatch registry entry for `llmerror-internal` STAYS — its scope narrows. `map_response_error` signature for new plugins must take `(status, headers, body)` so `Retry-After` is honored for 429.

3. **Decision B — conformance suite charter**: A new crate `tau-plugin-conformance` runs parameterized behavioral tests against any `LlmBackend` impl. Charter:
   - Tests **mechanical correctness** (shape, types, ordering) — explicit IN scope.
   - Tests **response quality** (does it follow instructions? is the answer right?) — explicit OUT of scope (NG7 forbids tau evaluating quality).
   - The catalog is conservative: 6 baseline tests at v0.1; extension requires a follow-up. Specifically, conformance tests do NOT mandate:
     - specific response text.
     - specific `stop_reason` values beyond "valid variant".
     - specific tool_use ids (some plugins synthesize; some preserve provider ids).
     - specific token counts.

4. **Consequences**:
   - 17 → 21 required CI checks gating `main`.
   - `tau-runtime` retry helper can use `is_retryable()` honestly now (today it sees `Internal`, treats as non-retryable, silently loses signal).
   - Future LLM-backend plugin authors get a behavioral test suite for free.
   - Plugin migration is mechanical (1-for-1 cassette assertion updates).

5. **Out of scope**:
   - `LlmError::ModelNotFound` typed variant (deferred; `InvalidRequest` covers v0.1).
   - `tau-runtime` migration to typed-error-matching (callers gain capabilities opportunistically).
   - Cassette record-mode automation.
   - Conformance catalog expansion beyond the 6 baseline tests.

**Verification:** `cargo test --workspace --doc` passes (the ADR has no doctests but other docs may reference it via intra-doc links — confirm no breakage).

**Refs:** Spec §1.1, §2.2 row 14, §8.3.

**Commit subject:** `docs(adr): ADR-0009 typed LlmError migration + conformance charter`

---

### Task 20: CI — 4 new jobs

**Files:** Modify `.github/workflows/ci.yml`.

**Add 4 new jobs after `build-ollama-plugin`:**

```yaml
  build-openai-plugin:
    name: build (openai-plugin)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build --release -p openai

  build-tau-plugin-test-support:
    name: build (tau-plugin-test-support)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build -p tau-plugin-test-support
      - run: cargo test -p tau-plugin-test-support --all-targets

  build-tau-plugin-conformance:
    name: build (tau-plugin-conformance)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo build -p tau-plugin-conformance

  test-conformance:
    name: test (conformance)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Run conformance suite against all plugins
        run: |
          cargo test -p anthropic --test conformance -- --nocapture
          cargo test -p ollama --test conformance -- --nocapture
          cargo test -p openai --test conformance -- --nocapture
```

The four new job names must match exactly: `build (openai-plugin)`, `build (tau-plugin-test-support)`, `build (tau-plugin-conformance)`, `test (conformance)`. Task 22's branch protection update queues these into the required-checks list.

**Verification:** After commit + push: confirm the four new jobs appear in PR CI runs.

**Commit subject:** `ci(2c): add openai/test-support/conformance build + conformance test jobs`

---

### Task 21: Final local verification + mark PR ready

User-driven gate.

- [ ] **Step 21.1: Full local verification matrix**

```
cargo build --workspace
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```
All must pass.

- [ ] **Step 21.2: Verify branch state**

```
git fetch origin
git log --oneline origin/main..HEAD
git status
```
Confirm branch is up-to-date and tree is clean.

- [ ] **Step 21.3: PR existence + checks**

```
gh pr list --head feat/openai-plugin-spec
gh pr checks <PR#>
```
All 21 required checks must be SUCCESS.

- [ ] **Step 21.4: Mark PR ready**

```
gh pr ready <PR#>
```

- [ ] **Step 21.5: Surface to user — wait for sign-off**

> "Sub-project 2c implementation complete; all 19 work tasks shipped; all 21 CI checks green on PR. Awaiting your sign-off to (a) update ROADMAP, (b) update branch protection (17→21 required checks), (c) flip ADR-0009 status from Proposed to Accepted, (d) squash-merge."

---

### Task 22: Plan sign-off + ROADMAP + branch protection 17→21 + ADR-0009 Accepted + squash merge

User-driven gate.

- [ ] **Step 22.1: Update ROADMAP.md**

Add a new row (after the 2b row):

```
| 2c | OpenAI LLM-backend plugin + supporting infrastructure ✅ | Third real LLM-backend plugin: OpenAI Chat Completions client at `crates/tau-plugins/openai/`; SSE streaming, real tool_call_id round-trip, full tool_choice round-trip. Plus `crates/tau-plugin-test-support/` (rule-of-three refactor of cassette replayer) and `crates/tau-plugin-conformance/` (parameterized behavioral test suite, deferred from ADR-0008 §17). All 3 plugins migrated to typed `LlmError` variants. ADR-0009 Accepted. | <DATE-OF-MERGE> |
```

Update the "Status" line under "Current phase: 1" — sub-project 2c shipped; Tier 1 priority 2 complete; next priority is Tier 1 priority 3 (first real Tool plugin).

Update Tier 1 item 2: change wording to indicate all three plugins shipped; bump CI-checks count to 21.

- [ ] **Step 22.2: Flip ADR-0009 status to Accepted**

Edit `docs/decisions/0009-llm-error-typing-and-conformance.md`: change `**Status:** Proposed` to `**Status:** Accepted`.

- [ ] **Step 22.3: Commit + push**

```
git add ROADMAP.md docs/decisions/0009-llm-error-typing-and-conformance.md
git commit -m "docs(roadmap+adr): mark Phase 1 sub-project 2c complete + ADR-0009 Accepted

Third real LLM-backend plugin shipped (OpenAI). Tier 1 priority 2
fully complete with all three plugins (Anthropic + Ollama + OpenAI)
running typed LlmError variants and integrated with the new
parameterized conformance suite.

21 required CI checks gating main (was 17). The next Tier 1 priority
is the first real Tool plugin.

ADR-0009 (LlmError migration policy + conformance suite charter)
status: Proposed → Accepted.

Refs: docs/superpowers/specs/2026-04-29-openai-plugin-design.md"
git push
```

- [ ] **Step 22.4: Update branch protection — add 4 new required checks**

```
gh api repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks --jq '.contexts'
# Confirm current count = 17.

gh api -X PATCH repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks \
    -f 'contexts[]=...the existing 17...' \
    -f 'contexts[]=build (openai-plugin)' \
    -f 'contexts[]=build (tau-plugin-test-support)' \
    -f 'contexts[]=build (tau-plugin-conformance)' \
    -f 'contexts[]=test (conformance)'
# Confirm new count = 21.
```

- [ ] **Step 22.5: Squash-merge**

```
gh pr merge <PR#> --squash --delete-branch
```

- [ ] **Step 22.6: Verify post-merge state**

```
git fetch origin
git checkout main
git pull
git log --oneline -5
gh api repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks --jq '.contexts | length'
# Expect 21.
```

Sub-project 2c complete. Tier 1 priority 2 fully done.

---

## Self-review notes (for the plan author)

**Spec coverage check:**

| Spec section | Covered by task |
|---|---|
| §1, §1.1, §1.2 | All tasks |
| §2.1 (settled by precedent) | Tasks 5-14 |
| §2.2 (contested decisions) | Q1=A → Tasks 2,3,4 (test-support); Q2=A → Tasks 8 (openai), 17 (anthropic), 18 (ollama); Q3=A → Tasks 15,16 (suite + openai integration), 17,18 (anthropic+ollama integration) |
| §2.2 row 14 (ADR-0009) | Task 19 |
| §3.1 (workspace layout) | Task 1 |
| §4.1 client.rs | Task 9 |
| §4.2 request.rs | Task 6 |
| §4.3 response.rs | Task 7 |
| §4.4 error.rs (TYPED) | Task 8 |
| §4.5 (anthropic + ollama migration) | Tasks 17, 18 |
| §5 streaming + accumulator | Task 10 |
| §6 config + plugin entry | Tasks 5, 11 |
| §7 tool-use mapping | Tasks 6, 7, 10 |
| §8.1 cassette catalog | Tasks 12, 13 |
| §8.2 OpenAI plugin tests | Tasks 12, 13 |
| §8.3 conformance suite | Tasks 15, 16 |
| §8.4 cassette format | Tasks 12, 13 |
| §8.5 live smoke tests | Task 14 |
| §9.1 anthropic + ollama test-support migration | Tasks 3, 4 |
| §9.2 anthropic + ollama error migration | Tasks 17, 18 |
| §9.3 conformance integration | Tasks 17, 18 (anthropic + ollama); Task 16 (openai) |
| §10 (this plan IS the expansion) | n/a |
| §11 out of scope | No tasks (intentional non-goals) |
| §12 cross-references | Documented in plan header |
| §13 follow-ups | Documented in Task 22 |

**No spec gaps found.**

**Placeholder scan:** No `TBD`, `TODO`, `implement later`, `Add appropriate error handling`, or `Similar to Task N` patterns. All code blocks are concrete.

**Type consistency check:**
- `OpenAIConfig` / `RetryConfig` — defined in Task 5, used in Tasks 9, 11, 12, 16.
- `OpenAIClient::new(inner, base_url, api_key, organization, retry)` — defined in Task 9, used in Task 11.
- `OpenAIPlugin { client }` — Task 11.
- `parse_chat_completions_response(body) -> Result<CompletionResponse, ParseError>` — defined in Task 7, used in Task 11.
- `parse_sse(body) -> Result<CompletionStream, LlmError>` — Task 10.
- `map_response_error(status, headers, body) -> LlmError` — defined in Task 8, used in Task 11; Tasks 17 and 18 update anthropic and ollama to use the same 3-arg signature.
- `map_client_error(err) -> LlmError` — defined in Task 8, used in Tasks 11, 17, 18.
- `build_chat_completions_body(req, stream) -> Result<Value, BuildError>` — Task 6.
- `resolve_api_key`, `validate_retry` — Task 5, used in Task 11.
- `ClientError::{Transport, Exhausted}` — Task 8.
- `ConformanceSuite::default().run(|base_url| ..., cassettes_dir)` — defined in Task 15, used in Tasks 16, 17, 18.

**No type-consistency drift found.**

**Cross-task migration ordering (sanity check):**
- Test-support extraction (Tasks 2-4) lands BEFORE OpenAI implementation (Tasks 5-14). Tasks 5-14 use the shared cassette from day one.
- Conformance suite (Task 15) lands BEFORE its first consumer (Task 16: OpenAI).
- OpenAI conformance (Task 16) lands BEFORE anthropic + ollama migration (Tasks 17, 18) so the suite is exercised by its greenfield consumer first; existing-plugin migrations import a known-working API.
- Typed-error migration is bundled with the conformance integration in Tasks 17-18 (one PR-quality commit per existing plugin).
- ADR-0009 (Task 19) lands AFTER all the work it documents is in the tree — same pattern as ADR-0008 in sub-project 1.
- CI (Task 20) lands LAST among the work tasks so the new jobs see all 4 new directories.

---

## Plan complete and saved to `docs/superpowers/plans/2026-04-29-openai-plugin.md`.

Two execution options:

**1. Subagent-Driven (recommended)** — fresh subagent per task, two-stage review (spec compliance + code quality) between tasks, fast iteration on the existing `feat/openai-plugin-spec` branch.

**2. Inline Execution** — execute tasks in this session using `superpowers:executing-plans`, batch execution with checkpoints.

Which approach?
