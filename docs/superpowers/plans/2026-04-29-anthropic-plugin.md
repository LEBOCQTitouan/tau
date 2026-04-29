# Anthropic LLM-backend Plugin Implementation Plan

> **STATUS — COMPLETE.** All 16 tasks shipped via subagent-driven
> execution on branch `feat/anthropic-plugin-spec`. Per the project's
> plan-checkbox-reconciliation convention, individual `- [ ]`
> checkboxes below remain unticked — the authoritative record is the
> git log on this branch. PR #12 squash-merged into `main`
> 2026-04-29.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the first real LLM-backend plugin for tau — an in-tree Anthropic Claude Messages API client at `crates/tau-plugins/anthropic/`. Validates the IPC plugin loading mechanism (ADR-0008) end-to-end against real network traffic, real authentication, real provider error envelopes, and real SSE streaming.

**Architecture:** One new workspace member with seven Rust source files (`config.rs`, `request.rs`, `response.rs`, `error.rs`, `client.rs`, `stream.rs`, `plugin.rs` + `main.rs`). Reqwest-backed HTTP client with retry-on-transient + Retry-After honoring. Cassette-replay testing harness in `tests/common/cassette.rs` plus 10 cassette YAMLs and 2 env-gated live smoke tests.

**Tech Stack:** Rust stable (workspace MSRV 1.91 per QG7), `reqwest = "0.12"` (default-features = false, features = ["json", "rustls-tls", "stream"]), `eventsource-stream = "0.2"`, `async-stream = "0.3"`, `secrecy = "0.10"`, `tokio` (already workspace), `serde` + `serde_json` (workspace), `thiserror` (workspace).

**Spec:** `docs/superpowers/specs/2026-04-29-anthropic-plugin-design.md` (commit `5d42fcc`).

**Working directory:** `/Users/titouanlebocq/code/tau` on branch `feat/anthropic-plugin-spec`. PR opens at Task 1's first push (or before, per the established workflow). All implementation commits on this branch auto-trigger CI per branch-protection. NEVER push to `main` directly.

**Commit policy:** every task ends with a Conventional Commits-formatted commit. PR is opened as Draft and marked Ready for review at Task 14 (final local verification). Task 15 (Plan sign-off + ROADMAP + branch-protection update + merge) is a user-driven gate. **No ADR sign-off task** — this sub-project introduces no ADR (per spec §2 + §9.1; purely additive).

**Note on TDD strictness:** for tasks producing parsers, validators, or branching logic (Configure validation in Task 3, body builder in Task 4, response parser in Task 5, error mapper in Task 6, retry classifier in Task 7, SSE state machine in Task 8) follow strict red-green-refactor: write the failing test first, watch it fail, implement, watch it pass. For tasks producing pure data declarations or thin wiring (Tasks 1, 2, 9) the cycle collapses — write the type with its tests in one step, then verify all tests pass.

---

## Plan-erratum: spec drift against actual tau-ports types

**Critical:** Reading `crates/tau-ports/src/llm.rs` directly turns up several differences between the spec's pseudocode and the actual tau-ports types. The implementation MUST use the actual types below, not the spec's pseudocode shapes. Each task body uses the correct shapes; the erratum is consolidated here so the implementer doesn't get tripped up.

| Spec assumed | Actual tau-ports |
|---|---|
| System prompt extracted from `LlmProviderMessage::System` | `CompletionRequest::system: Option<String>` is a **top-level field**; no `System` variant exists. Plugin trivially maps `req.system` → Anthropic's top-level `system`. No "split system from messages" logic needed. |
| `LlmProviderMessage::User { text: String }` | `User { content: Vec<ContentBlock> }`; constructor `LlmProviderMessage::user(Vec<ContentBlock>)`. Multi-block content. |
| `ContentBlock::Text { text }` (struct variant) | `ContentBlock::Text(String)` (tuple variant). Two-block enum: `Text(String)` and `ToolUse(ToolUse)`. |
| `CompletionResponse { content: Vec<ContentBlock>, ... }` | `CompletionResponse { text: String, tool_uses: Vec<ToolUse>, stop_reason: StopReason, usage: Option<TokenUsage> }`. Flat shape — Anthropic's `content[*].text` blocks concatenate into `text`; tool_use blocks collect into `tool_uses`. **`usage` is `Option`** — plugin must produce `Some(...)` from Anthropic's `usage` field. |
| `CompletionChunk::ToolUseDelta { tool_use }` | `CompletionChunk::ToolUse(ToolUse)` — tuple variant carrying a full ToolUse, emitted once after fragment accumulation completes. |
| `ToolChoice::ForceTool { name }` | `ToolChoice::Specific { name }`. |
| `ToolSpec::parameters_json` | `ToolSpec::input_schema: Value`. |
| `StopReason::Other(String)` for forward-compat | No `Other` variant. Variants: `EndTurn`, `MaxTokens`, `StopSequence`, `ToolUse`, `Error`. Unknown stop_reasons map to `EndTurn` with a `tracing::warn!`; spec §4.3 already accepts this. |
| `tau_ports::TokenUsage::new(input_tokens, output_tokens)` | Same — verified to exist (`llm.rs:177`). |
| `tau_ports::ToolUseAccumulator::new(id, name)` | Same. |
| `tau_ports::ToolUse::new(id, name, input)` | Same. |

**Tools array + ToolChoice::None semantics:** spec is ambiguous; tau-ports doc says `ToolChoice::None` = "Model must not call any tool." Anthropic's enforcement is to omit the `tools` array entirely (not just omit `tool_choice`). Plan rule: when `req.tool_choice == ToolChoice::None`, omit BOTH `tools` and `tool_choice` from the body, even if `req.tools` is non-empty. Document this in Task 4's tests.

**`provider_specific: BTreeMap<String, Value>`** field on `CompletionRequest` is a registered escape hatch for `top_k`/`response_format`/etc. that tau-ports doesn't model. **v0.1 plugin ignores it** with a tracing::debug! note when non-empty. Future: read selected keys (`top_k`) and pass through. Documented as deferred in Task 4.

**Additional plan-erratum carry-overs from sub-projects 1+2 (apply preemptively):**

- Wire methods are `llm.complete` and `llm.stream` (NOT `llm.complete_streaming` — sub-project 1 plan-erratum). The SDK's `run_llm_backend_with_config` handles dispatch; plugin code never names these strings directly.
- `CompletionChunk::Finish { stop_reason, usage }` (NOT `Done`).
- `Tool` is stateful (init/invoke/teardown) — irrelevant to this plugin (it produces `ContentBlock::ToolUse`, doesn't impl `Tool`).
- `tau-ports` `serde` feature is enabled via `tau-ports = { workspace = true, features = ["serde", "test-fixtures"] }`. Sub-project 1 established this works.
- **Doctests on `#[non_exhaustive]` types must be `ignore`-marked** (E0639 from external doctest compilation). `AnthropicConfig`, `RetryConfig`, and the new `ConfigError::InvalidEnvVar` variant doctests must use ` ```ignore`.
- **`cargo test --all-targets` does NOT include doctests**; verification must explicitly run `cargo test --doc` separately.
- For tests destructuring `#[non_exhaustive]` enums: use `let X { fields, .. } = value else { panic!() };` for multi-variant enums; `assert!(matches!(...))` for single-variant or in-crate same-module patterns.
- **NO new `Internal`/`Custom` variants** ship in this sub-project. The mechanical CI registry test (`crates/tau-domain/tests/escape_hatch_registry.rs`) continues to gate.
- **Cassette replayer crate vs hand-rolled** is decided in Task 10 with a 5-minute survey of the 2026 Rust ecosystem. If no maintained crate exists, hand-roll ~200 LOC.

---

## File Structure

| Path | Responsibility | Created/Modified in |
|---|---|---|
| `Cargo.toml` (workspace root) | Add `crates/tau-plugins/anthropic` to `members`; add new workspace deps (`reqwest`, `eventsource-stream`, `async-stream`, `secrecy`) | Task 1 |
| `crates/tau-plugin-sdk/src/configure.rs` | Add `ConfigError::InvalidEnvVar { name: String, detail: String }` variant | Task 2 |
| `crates/tau-plugin-sdk/src/lib.rs` | Re-export updated `ConfigError` (no surface change; the `pub use` is already in place) | (no change in Task 2 if re-export exists) |
| `crates/tau-plugins/anthropic/Cargo.toml` | New crate manifest; bin target `anthropic-plugin` | Task 1 |
| `crates/tau-plugins/anthropic/tau.toml` | Plugin manifest with `[plugin]` table | Task 1 |
| `crates/tau-plugins/anthropic/src/main.rs` | `#[tokio::main]` entrypoint calling `run_llm_backend_with_config` | Tasks 1, 9 |
| `crates/tau-plugins/anthropic/src/config.rs` | `AnthropicConfig` + `RetryConfig` + `Configure` impl + `from_config` validation | Task 3 |
| `crates/tau-plugins/anthropic/src/request.rs` | `build_messages_body` + per-message + tool + tool_choice translation | Task 4 |
| `crates/tau-plugins/anthropic/src/response.rs` | Anthropic JSON → `CompletionResponse`; `text` concatenation; tool_uses collection; stop_reason mapping | Task 5 |
| `crates/tau-plugins/anthropic/src/error.rs` | HTTP status + Anthropic error JSON → `LlmError` | Task 6 |
| `crates/tau-plugins/anthropic/src/client.rs` | `AnthropicClient` + `post_messages` + retry classifier + Retry-After honoring | Task 7 |
| `crates/tau-plugins/anthropic/src/stream.rs` | SSE event parser + `BlockState` machine + tool_use accumulation → `CompletionStream` | Task 8 |
| `crates/tau-plugins/anthropic/src/plugin.rs` | `AnthropicPlugin` struct + `LlmBackend` impl wiring all the above | Task 9 |
| `crates/tau-plugins/anthropic/src/lib.rs` | Module declarations; pulls together internals so unit tests + binary share the same surface | Tasks 1, 9 |
| `crates/tau-plugins/anthropic/tests/common/mod.rs` | Test helpers: `sample_request()`, `extract_text()`, `test_config()` | Task 10 |
| `crates/tau-plugins/anthropic/tests/common/cassette.rs` | Cassette replayer (chosen crate or hand-rolled) | Task 10 |
| `crates/tau-plugins/anthropic/tests/cassettes/*.yaml` | 10 recorded cassette files | Tasks 11, 12 |
| `crates/tau-plugins/anthropic/tests/complete.rs` | 7 batch-mode integration tests against cassettes | Task 11 |
| `crates/tau-plugins/anthropic/tests/streaming.rs` | 3 streaming integration tests against cassettes | Task 12 |
| `crates/tau-plugins/anthropic/tests/live.rs` | 2 env-gated live smoke tests (`#[ignore]` by default) | Task 13 |
| `scripts/rerecord-anthropic-cassettes.sh` | Cassette re-recording helper script | Task 13 |
| `.github/workflows/ci.yml` | Add `build (anthropic-plugin)` job (release-build only) | Task 14 |
| `ROADMAP.md` | Mark Phase 1 priority 2a complete | Task 15 |
| `docs/superpowers/plans/2026-04-29-anthropic-plugin.md` | This plan; checkboxes ticked at sign-off | Task 15 |

---

## Tasks 1-3: detailed (Plan-2 fidelity)

### Task 1: Workspace scaffold + crate Cargo.toml + new workspace deps

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/tau-plugins/anthropic/Cargo.toml`
- Create: `crates/tau-plugins/anthropic/tau.toml`
- Create: `crates/tau-plugins/anthropic/src/main.rs` (stub)
- Create: `crates/tau-plugins/anthropic/src/lib.rs` (stub)

- [ ] **Step 1.1: Inspect current workspace `Cargo.toml`**

```bash
grep -n "^members\|^reqwest\|^eventsource\|^async-stream\|^secrecy" /Users/titouanlebocq/code/tau/Cargo.toml
```

Expected: `members = [...]` line with the existing 12 entries; no matches for the four new deps.

- [ ] **Step 1.2: Update workspace `Cargo.toml`**

Open `/Users/titouanlebocq/code/tau/Cargo.toml`. In the `[workspace] members = [...]` array, add a new line at the bottom (after `crates/tau-plugins/echo-tool`):

```toml
    "crates/tau-plugins/anthropic",
```

In the `[workspace.dependencies]` block, append:

```toml
reqwest             = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "stream"] }
eventsource-stream  = "0.2"
async-stream        = "0.3"
secrecy             = "0.10"
```

(Place these at the end, after the existing deps. Maintain whatever ordering convention the file already uses.)

- [ ] **Step 1.3: Create `crates/tau-plugins/anthropic/Cargo.toml`**

Create `/Users/titouanlebocq/code/tau/crates/tau-plugins/anthropic/Cargo.toml`:

```toml
[package]
name = "anthropic"
description = "Anthropic Claude (Messages API) LLM-backend plugin for tau."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[[bin]]
name = "anthropic-plugin"
path = "src/main.rs"

[lib]
name = "anthropic_plugin_lib"
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
tokio        = { workspace = true, features = ["macros", "rt-multi-thread", "io-util", "net"] }
tempfile     = { workspace = true }
serde_yaml   = "0.9"
```

- [ ] **Step 1.4: Create `crates/tau-plugins/anthropic/tau.toml`** (the plugin package manifest)

```toml
name = "anthropic"
version = "0.1.0"
description = "Anthropic Claude (Messages API) backend for tau."

[plugin]
provides = "llm_backend"
kind     = "rust-cargo"
bin      = "anthropic-plugin"
```

- [ ] **Step 1.5: Create stub `src/lib.rs`**

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Anthropic Claude (Messages API) plugin internals.
//!
//! The binary entrypoint at `src/main.rs` calls
//! `tau_plugin_sdk::run_llm_backend_with_config::<AnthropicPlugin>(...)`.
//! Modules below populate as Tasks 3 — 9 land.
//!
//! See `docs/superpowers/specs/2026-04-29-anthropic-plugin-design.md`
//! for the design rationale.

// Modules populate progressively across Tasks 3-9. For Task 1 the
// crate just compiles to an empty library + stub binary.
```

- [ ] **Step 1.6: Create stub `src/main.rs`**

```rust
//! `anthropic-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! Task 1 stub — populated in Task 9.

fn main() {}
```

- [ ] **Step 1.7: Verify the workspace builds + lints clean**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: all three exit 0. The new crate's binary builds as a no-op `fn main()`; the lib is empty.

- [ ] **Step 1.8: Verify per-crate doctest harness wires up**

```bash
cargo test -p anthropic --doc
```

Expected: `0 passed; 0 failed; 0 ignored`.

- [ ] **Step 1.9: Commit and push**

```bash
cd /Users/titouanlebocq/code/tau
git add Cargo.toml Cargo.lock crates/tau-plugins/anthropic
git commit -m "$(cat <<'EOF'
build(anthropic): scaffold tau-plugins/anthropic crate

Adds a new in-tree workspace member at crates/tau-plugins/anthropic
for the first real LLM-backend plugin (Phase 1 priority 2a). Bin
target `anthropic-plugin` and parallel lib `anthropic_plugin_lib`
share the same crate so unit tests and the binary observe the same
internal surface. Plugin manifest tau.toml declares
provides = "llm_backend", kind = "rust-cargo".

Adds reqwest (0.12, rustls-tls + stream), eventsource-stream,
async-stream, and secrecy to workspace deps.

Refs: spec §3.1 / §3.2, Task 1 of plan
EOF
)"
git push -u origin feat/anthropic-plugin-spec
```

---

### Task 2: tau-plugin-sdk `ConfigError::InvalidEnvVar` variant

**Files:**
- Modify: `crates/tau-plugin-sdk/src/configure.rs`

**Why this task is in the Anthropic-plugin sub-project**: the existing `ConfigError::InvalidValue { field: &'static str, detail: String }` requires the `field` name to be a `&'static str` literal at compile time. The Anthropic plugin's customizable env-var-name use case (a runtime-decided env var like `MY_ORG_ANTHROPIC_KEY`) needs to surface the actual env var name in the error. Adding a typed variant is additive (no escape-hatch impact), preserves the typed-error model, and matches the existing variant-style at module level.

This is a tightly-coupled tau-plugin-sdk amendment. Per the established Phase 0 + sub-project 1 pattern (e.g., the Task 8 `RpcErrorEnvelope::new` constructor that landed in tau-plugin-protocol when the SDK runner needed it), bundling it into Task 2 of this sub-project is correct.

- [ ] **Step 2.1: Read the current ConfigError shape**

```bash
sed -n '/pub enum ConfigError/,/^}/p' /Users/titouanlebocq/code/tau/crates/tau-plugin-sdk/src/configure.rs
```

Expected: enum with three variants — `Decode`, `MissingField`, `InvalidValue`.

- [ ] **Step 2.2: Append the new variant**

Open `/Users/titouanlebocq/code/tau/crates/tau-plugin-sdk/src/configure.rs`. In the `pub enum ConfigError` block, append a fourth variant just before the closing `}`:

```rust
    /// A required environment variable was missing or malformed.
    /// Distinct from [`ConfigError::MissingField`]: the variant carries
    /// the env-var name as a runtime `String` so plugins with
    /// customizable env-var-name configuration (e.g. an Anthropic
    /// plugin reading `api_key_env: String` from handshake config) can
    /// surface the actual name in the error message.
    #[error("env var {name} unusable: {detail}")]
    InvalidEnvVar {
        /// Name of the environment variable that was checked.
        name: String,
        /// Human-readable explanation of why it was unusable.
        detail: String,
    },
```

- [ ] **Step 2.3: Add 2 unit tests inside the existing `#[cfg(test)] mod tests` block (or create one)**

If a `#[cfg(test)] mod tests` block does not exist in `configure.rs`, append one at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_env_var_displays_name_and_detail() {
        let err = ConfigError::InvalidEnvVar {
            name: "MY_ORG_ANTHROPIC_KEY".into(),
            detail: "not set in environment".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("MY_ORG_ANTHROPIC_KEY"));
        assert!(s.contains("not set in environment"));
    }

    #[test]
    fn invalid_env_var_pattern_matches() {
        let err = ConfigError::InvalidEnvVar {
            name: "FOO".into(),
            detail: "bar".into(),
        };
        assert!(matches!(
            err,
            ConfigError::InvalidEnvVar { ref name, .. } if name == "FOO"
        ));
    }
}
```

- [ ] **Step 2.4: Verify**

```bash
cd /Users/titouanlebocq/code/tau
cargo test -p tau-plugin-sdk --all-targets
cargo test -p tau-plugin-sdk --doc
cargo clippy -p tau-plugin-sdk --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo test -p tau-domain --test escape_hatch_registry
```

Expected: all green. Escape-hatch registry test still passes — `InvalidEnvVar` is a typed variant, no `Internal` / `Custom` involved.

- [ ] **Step 2.5: Commit and push**

```bash
cd /Users/titouanlebocq/code/tau
git add crates/tau-plugin-sdk/src/configure.rs
git commit -m "$(cat <<'EOF'
feat(tau-plugin-sdk): add ConfigError::InvalidEnvVar variant

Surfaces a runtime-named env var in config errors, separate from
the &'static str-named field that `MissingField` and `InvalidValue`
carry. Anthropic plugin's customizable api_key_env (per spec §6.1)
needs to report the actual env var name (e.g. MY_ORG_ANTHROPIC_KEY)
when it isn't set, which the existing variants can't express
without `Box::leak`.

Typed variant; no Internal/Custom escape hatch — escape-hatch
registry test continues to gate. Two unit tests cover Display + match.

Refs: spec §6.2 plan-erratum, Task 2 of plan
EOF
)"
git push
```

---

### Task 3: `AnthropicConfig` + `RetryConfig` + `Configure` impl

**Files:**
- Create: `crates/tau-plugins/anthropic/src/config.rs`
- Modify: `crates/tau-plugins/anthropic/src/lib.rs` (declare `pub mod config;`)

This task introduces the deserializable config shape and validates it. Strict TDD: write the from_config validation paths as failing tests first, then make them pass.

- [ ] **Step 3.1: Create `src/config.rs`**

Create `/Users/titouanlebocq/code/tau/crates/tau-plugins/anthropic/src/config.rs`:

```rust
//! Anthropic plugin configuration.
//!
//! Deserialized from the handshake `config` field by
//! [`tau_plugin_sdk::run_llm_backend_with_config`]. Two nested
//! concerns: API auth and retry tuning.
//!
//! See `docs/superpowers/specs/2026-04-29-anthropic-plugin-design.md`
//! §6.1.

use serde::Deserialize;
use std::time::Duration;
use tau_plugin_sdk::ConfigError;

/// Top-level config for the Anthropic plugin.
///
/// Deserialized from the handshake `config: serde_json::Value`. All
/// fields have defaults so a project tau.toml `[agents.<id>.config]`
/// section can be empty.
///
/// `#[non_exhaustive]`: additive fields are non-breaking.
///
/// # Example
///
/// ```ignore
/// // `AnthropicConfig` is `#[non_exhaustive]`; external callers
/// // construct via serde or Default.
/// use anthropic_plugin_lib::config::AnthropicConfig;
/// let cfg = AnthropicConfig::default();
/// assert_eq!(cfg.api_key_env, "ANTHROPIC_API_KEY");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnthropicConfig {
    /// Override env var name for the API key. Default: `ANTHROPIC_API_KEY`.
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,

    /// Direct API key override. **Test-only** — never put a real key
    /// in project tau.toml. If both `api_key` and `api_key_env` are
    /// present, `api_key` wins and a `tracing::warn!` is emitted.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Override base URL. Default: <https://api.anthropic.com>. Tests
    /// use this to point at the cassette replayer.
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// Anthropic API version header. Default: `"2023-06-01"`.
    #[serde(default = "default_api_version")]
    pub api_version: String,

    /// Per-request HTTP timeout in seconds. Default: 600 (Anthropic
    /// streaming can run minutes).
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,

    /// Retry behavior. Defaults match the design spec §Q8.
    #[serde(default)]
    pub retry: RetryConfig,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            api_key_env: default_api_key_env(),
            api_key: None,
            base_url: default_base_url(),
            api_version: default_api_version(),
            request_timeout_secs: default_request_timeout_secs(),
            retry: RetryConfig::default(),
        }
    }
}

impl AnthropicConfig {
    /// Per-request HTTP timeout as a `Duration`.
    pub fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.request_timeout_secs)
    }
}

/// Retry behavior for transient errors (429, 503, network timeouts).
///
/// `#[non_exhaustive]`: additive fields are non-breaking.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryConfig {
    /// Maximum total attempts including the initial request. `1`
    /// disables retry (one-shot). Default: 3.
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,

    /// Base delay in milliseconds for exponential backoff. Default: 1000.
    /// Effective delay = `base_delay_ms * 2^(attempt-1)`, capped at 60s.
    #[serde(default = "default_base_delay_ms")]
    pub base_delay_ms: u64,

    /// Honor the `Retry-After` response header when present (parsed as
    /// integer seconds). Default: true.
    #[serde(default = "default_respect_retry_after")]
    pub respect_retry_after: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            base_delay_ms: default_base_delay_ms(),
            respect_retry_after: default_respect_retry_after(),
        }
    }
}

fn default_api_key_env() -> String { "ANTHROPIC_API_KEY".into() }
fn default_base_url() -> String { "https://api.anthropic.com".into() }
fn default_api_version() -> String { "2023-06-01".into() }
fn default_request_timeout_secs() -> u64 { 600 }
fn default_max_attempts() -> u32 { 3 }
fn default_base_delay_ms() -> u64 { 1_000 }
fn default_respect_retry_after() -> bool { true }

/// Validate + resolve the API key from config or env.
///
/// Returns the resolved API key on success. Errors map to [`ConfigError`]
/// variants per spec §6.2 / Task 2 plan-erratum (`InvalidEnvVar` for
/// missing env var, `InvalidValue` for malformed key shape).
pub(crate) fn resolve_api_key(cfg: &AnthropicConfig) -> Result<String, ConfigError> {
    let key = if let Some(direct) = cfg.api_key.as_ref() {
        tracing::warn!(
            target: "anthropic_plugin::config",
            "config.api_key set directly — recommended only for tests"
        );
        direct.clone()
    } else {
        std::env::var(&cfg.api_key_env).map_err(|_| ConfigError::InvalidEnvVar {
            name: cfg.api_key_env.clone(),
            detail: "env var is not set; set it or use config.api_key (test-only)".into(),
        })?
    };

    if !key.starts_with("sk-ant-") {
        return Err(ConfigError::InvalidValue {
            field: "api_key",
            detail: "Anthropic API keys start with `sk-ant-`".into(),
        });
    }
    Ok(key)
}

/// Validate retry-config invariants beyond what serde catches.
pub(crate) fn validate_retry(retry: &RetryConfig) -> Result<(), ConfigError> {
    if retry.max_attempts == 0 {
        return Err(ConfigError::InvalidValue {
            field: "retry.max_attempts",
            detail: "must be >= 1 (use 1 for no-retry semantics)".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_production_ready() {
        let cfg = AnthropicConfig::default();
        assert_eq!(cfg.api_key_env, "ANTHROPIC_API_KEY");
        assert_eq!(cfg.base_url, "https://api.anthropic.com");
        assert_eq!(cfg.api_version, "2023-06-01");
        assert_eq!(cfg.request_timeout_secs, 600);
        assert_eq!(cfg.retry.max_attempts, 3);
        assert_eq!(cfg.retry.base_delay_ms, 1000);
        assert!(cfg.retry.respect_retry_after);
    }

    #[test]
    fn deserializes_empty_object_as_defaults() {
        let cfg: AnthropicConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.api_key_env, "ANTHROPIC_API_KEY");
        assert_eq!(cfg.retry.max_attempts, 3);
    }

    #[test]
    fn rejects_unknown_fields() {
        let result: Result<AnthropicConfig, _> = serde_json::from_str(
            r#"{"unknown_key": "value"}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn resolve_api_key_uses_config_override() {
        let mut cfg = AnthropicConfig::default();
        cfg.api_key = Some("sk-ant-test123".into());
        let key = resolve_api_key(&cfg).unwrap();
        assert_eq!(key, "sk-ant-test123");
    }

    #[test]
    fn resolve_api_key_reads_env_var() {
        // Set a unique env var name to avoid clobbering across tests
        let env_name = "TEST_RESOLVE_KEY_FROM_ENV";
        std::env::set_var(env_name, "sk-ant-fromenv");
        let cfg = AnthropicConfig {
            api_key_env: env_name.into(),
            ..AnthropicConfig::default()
        };
        let key = resolve_api_key(&cfg).unwrap();
        assert_eq!(key, "sk-ant-fromenv");
        std::env::remove_var(env_name);
    }

    #[test]
    fn resolve_api_key_missing_env_returns_invalid_env_var() {
        let cfg = AnthropicConfig {
            api_key_env: "DEFINITELY_NOT_SET_OPDIQWXZ".into(),
            ..AnthropicConfig::default()
        };
        let err = resolve_api_key(&cfg).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidEnvVar { ref name, .. }
                if name == "DEFINITELY_NOT_SET_OPDIQWXZ"
        ));
    }

    #[test]
    fn resolve_api_key_malformed_prefix_returns_invalid_value() {
        let mut cfg = AnthropicConfig::default();
        cfg.api_key = Some("nope-not-a-real-key".into());
        let err = resolve_api_key(&cfg).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidValue { field: "api_key", .. }
        ));
    }

    #[test]
    fn validate_retry_zero_attempts_rejected() {
        let retry = RetryConfig { max_attempts: 0, base_delay_ms: 100, respect_retry_after: true };
        let err = validate_retry(&retry).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidValue { field: "retry.max_attempts", .. }
        ));
    }

    #[test]
    fn validate_retry_one_attempt_ok() {
        let retry = RetryConfig { max_attempts: 1, base_delay_ms: 100, respect_retry_after: true };
        validate_retry(&retry).unwrap();
    }
}
```

- [ ] **Step 3.2: Wire it up in `lib.rs`**

Open `/Users/titouanlebocq/code/tau/crates/tau-plugins/anthropic/src/lib.rs`. Replace the placeholder body with:

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Anthropic Claude (Messages API) plugin internals.
//!
//! The binary entrypoint at `src/main.rs` calls
//! `tau_plugin_sdk::run_llm_backend_with_config::<AnthropicPlugin>(...)`.
//!
//! See `docs/superpowers/specs/2026-04-29-anthropic-plugin-design.md`
//! for the design rationale.

pub mod config;
```

- [ ] **Step 3.3: Run unit tests**

```bash
cd /Users/titouanlebocq/code/tau
cargo test -p anthropic --lib config
```

Expected: 9 tests pass (defaults, deserialize empty, rejects unknown, key override, env var, missing env, malformed prefix, retry zero, retry one).

- [ ] **Step 3.4: Run all anthropic tests + doctests**

```bash
cargo test -p anthropic --all-targets
cargo test -p anthropic --doc
```

Expected: 9 unit tests pass; doctest on `AnthropicConfig` is `ignore`-marked.

- [ ] **Step 3.5: Lint + format**

```bash
cargo clippy -p anthropic --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: both exit 0.

- [ ] **Step 3.6: Commit and push**

```bash
cd /Users/titouanlebocq/code/tau
git add crates/tau-plugins/anthropic
git commit -m "$(cat <<'EOF'
feat(anthropic): AnthropicConfig + RetryConfig + Configure validators

Defines the handshake-config shape for the Anthropic plugin. Two
non_exhaustive structs with serde + Default + #[serde(deny_unknown_fields)]
to catch typos at handshake time. resolve_api_key validates env-var
fallback / direct override / sk-ant- prefix; validate_retry checks
max_attempts >= 1.

Nine unit tests cover defaults, deserialize-empty, unknown-field
rejection, key resolution paths (override, env var, missing env,
malformed prefix), and retry validation. ConfigError::InvalidEnvVar
(added in Task 2) is exercised by the missing-env-var test.

Refs: spec §6.1 / §6.2, Task 3 of plan
EOF
)"
git push
```

---

## Tasks 4-13: hybrid (per-task summary + spec references)

The remaining tasks follow the patterns established in Tasks 1-3 (Cargo.toml deltas where required, types per spec, full unit + integration tests, doctest discipline, conventional-commits per task, `cargo build` + `cargo test --all-targets` + `cargo test --doc` + `cargo clippy -- -D warnings` + `cargo fmt --all -- --check` before each commit, push after).

Spec references are hyperlinks to the design spec at `docs/superpowers/specs/2026-04-29-anthropic-plugin-design.md`.

---

### Task 4: `request.rs` — body builder + tool/tool_choice translation

**Spec:** §4.2, §7.1, §7.2. **File created:** `crates/tau-plugins/anthropic/src/request.rs`. **Files modified:** `crates/tau-plugins/anthropic/src/lib.rs` (`pub mod request;`).

**Summary.** Define `pub(crate) fn build_messages_body(req: &CompletionRequest, stream: bool) -> Result<serde_json::Value, BuildError>`. The function:

1. Maps `req.system: Option<String>` → top-level `system` field (omit if `None`). **No "split system from messages" logic** — `LlmProviderMessage::System` does not exist (per plan-erratum).
2. Translates `req.messages` (each `LlmProviderMessage`) to Anthropic's per-message JSON:
   - `User { content }` → `{"role": "user", "content": <translated content blocks>}`
   - `Assistant { content }` → `{"role": "assistant", "content": <translated content blocks>}`
   - `ToolResult { tool_use_id, content, is_error }` → `{"role": "user", "content": [{"type": "tool_result", "tool_use_id": ..., "content": ..., "is_error": ...}]}`
3. Maps each `ContentBlock`: `Text(s)` → `{"type": "text", "text": s}`; `ToolUse(tu)` → `{"type": "tool_use", "id": tu.id, "name": tu.name, "input": tu.input}` (use `serde_json::to_value(&tu.input)` for the `Value` → JSON conversion).
4. Tool-choice translation:
   - `Auto` → `{"type": "auto"}`
   - `Required` → `{"type": "any"}`
   - `Specific { name }` → `{"type": "tool", "name": name}`
   - `None` → omit `tool_choice` AND omit `tools` array entirely (per plan-erratum: tau-ports doc enforces "no tool calls"; Anthropic's enforcement is to not advertise tools).
5. Tools array: `req.tools` mapped to `[{"name", "description", "input_schema"}]` only when `req.tool_choice != ToolChoice::None` AND `!req.tools.is_empty()`.
6. `max_tokens`: `req.max_tokens.unwrap_or(4096)`.
7. Sampling overrides (`temperature`, `top_p`): pass through if `Some`. `seed` and `stop_sequences` similarly.
8. `req.provider_specific`: emit `tracing::debug!(target: "anthropic_plugin::request", keys = ?provider_specific.keys().collect::<Vec<_>>(), "ignoring provider_specific keys")` and skip — defer to a future plugin version.
9. `stream: bool` parameter sets `body["stream"] = true` when streaming.

`BuildError` is a plugin-internal error type (typed, not Internal escape-hatch); converted to `LlmError::Internal { message }` in `plugin.rs`. Reasonable variants: `BuildError::SerializationFailed(serde_json::Error)`.

**Tests** (10+ unit tests):

- `builds_minimal_body`: just model + one user message.
- `omits_system_when_none`.
- `includes_system_when_some`.
- `omits_tools_array_when_empty`.
- `omits_tools_array_when_tool_choice_is_none` (even with non-empty `req.tools`).
- `tool_choice_auto_round_trips`.
- `tool_choice_required_maps_to_any`.
- `tool_choice_specific_includes_name`.
- `translates_user_message_text_block`.
- `translates_assistant_message_with_tool_use_block`.
- `translates_tool_result_message`.
- `passes_through_sampling_overrides`.
- `sets_stream_true_when_requested`.
- `default_max_tokens_is_4096`.
- `provider_specific_logs_debug_and_does_not_emit` (use `tracing-test` or assert via `tracing-subscriber`'s test layer; alternative — just verify the body doesn't carry the keys).

Doctest on `build_messages_body` marked `ignore` if it constructs `#[non_exhaustive]` types externally.

**Verification.** Per-task: `cargo test -p anthropic --all-targets`, `cargo test -p anthropic --doc`, `cargo clippy -p anthropic --all-targets --all-features -- -D warnings`, `cargo fmt --all -- --check`.

**Commit:** `feat(anthropic): request body builder + tool/tool_choice translation`.

---

### Task 5: `response.rs` — Anthropic JSON → `CompletionResponse`

**Spec:** §4.3, §7.3. **File created:** `crates/tau-plugins/anthropic/src/response.rs`. **Files modified:** `lib.rs` (`pub mod response;`).

**Summary.** Define `pub(crate) fn parse_messages_response(body: &str) -> Result<CompletionResponse, ParseError>` per the **actual `CompletionResponse` shape** (per plan-erratum: flat `text: String`, `tool_uses: Vec<ToolUse>`, `stop_reason`, `usage: Option<TokenUsage>`).

1. Deserialize Anthropic's response into a private `AnthropicMessagesResponse` struct.
2. Walk `content` array:
   - `text` blocks → concatenate into a single `text: String`.
   - `tool_use` blocks → push to `tool_uses: Vec<ToolUse>` via `ToolUse::new(id, name, input)`. The `input` field is a JSON object; convert to `tau_domain::Value` via `serde_json::from_value(input)`.
   - Unknown block types → `tracing::warn!(target: "anthropic_plugin::response", block_type = %t, "dropped unknown content block type")` and skip.
3. Map `stop_reason: String`:
   - `"end_turn"` → `StopReason::EndTurn`
   - `"tool_use"` → `StopReason::ToolUse`
   - `"max_tokens"` → `StopReason::MaxTokens`
   - `"stop_sequence"` → `StopReason::StopSequence`
   - any other → `StopReason::EndTurn` + `tracing::warn!` ("unknown stop_reason; defaulting to EndTurn"). **Do not** map to `StopReason::Error` — that variant signals mid-stream error per tau-ports docs, not unknown stop_reasons.
4. `usage` → `Some(TokenUsage::new(input_tokens, output_tokens))`. Anthropic always returns usage in batch responses; `None` is reserved for when the field is absent (defensive: parse as `Option<AnthropicUsage>`).

`ParseError`: typed, e.g., `ParseError::Json(serde_json::Error)`, `ParseError::ToolUseInputDecode { name: String, source: serde_json::Error }`. No `Internal` escape hatch.

**Tests** (~5 unit tests):

- `parses_text_only_response` — single text block + EndTurn + usage.
- `parses_tool_use_response` — text + tool_use blocks; `tool_uses` has one entry.
- `parses_multiple_text_blocks_concatenated` — Anthropic can return multiple text blocks; verify concatenation.
- `maps_unknown_stop_reason_to_end_turn` — synthetic `"stop_reason": "frobnicated"` → `EndTurn`.
- `drops_unknown_block_type_with_warning` — synthetic `{"type": "image_url", ...}` → not in output.
- `parses_tool_use_input_as_value` — verify `tau_domain::Value::Object` round-trip.

**Verification.** Per-task.

**Commit:** `feat(anthropic): response parser + content block + stop reason mapping`.

---

### Task 6: `error.rs` — HTTP + Anthropic error JSON → `LlmError`

**Spec:** §4.4. **File created:** `crates/tau-plugins/anthropic/src/error.rs`. **Files modified:** `lib.rs` (`pub mod error;`).

**Summary.** Two responsibilities:

1. `pub(crate) fn map_response_error(status: reqwest::StatusCode, body: &str) -> LlmError`: parse `body` as Anthropic's error envelope (`{"type": "error", "error": {"type": ..., "message": ...}}`) if possible; fall back to raw body. Build a category string per status range (400 → "bad request", 401/403 → "auth failure", 429 → "rate limited (retries exhausted)", 5xx → "server error", other → "unexpected status"). Returns `LlmError::Internal { message: format!("anthropic {category} ({status}): {detail}") }`.

2. `pub(crate) fn map_client_error(err: ClientError) -> LlmError`: maps the typed `ClientError` from `client.rs` (Task 7) — for now declare a forward-ref placeholder; Task 7 wires in the real type.

   ClientError variants the mapper handles (per spec §4.1):
   - `Transport(reqwest::Error)` → `LlmError::Internal { message: format!("transport error: {e}") }`
   - `Exhausted { status, attempts }` → `LlmError::Internal { message: format!("retries exhausted: {status} after {attempts} attempts") }`

**Tests** (~4 unit tests):

- `maps_429_to_rate_limited_internal` — body with structured `rate_limit_error`.
- `maps_401_to_auth_failure_internal`.
- `maps_500_to_server_error_internal`.
- `falls_back_to_raw_body_on_unparseable_json` — body that isn't an Anthropic error envelope.

**Verification.** Per-task.

**Commit:** `feat(anthropic): map HTTP + Anthropic error JSON to LlmError`.

---

### Task 7: `client.rs` — HTTP client + retry loop

**Spec:** §4.1. **File created:** `crates/tau-plugins/anthropic/src/client.rs`. **Files modified:** `lib.rs` (`pub mod client;`).

**Summary.** Per spec §4.1 + the actual code skeleton in spec body:

- `pub(crate) struct AnthropicClient` holding `reqwest::Client`, `base_url: String`, `api_key: secrecy::SecretString`, `api_version: String`, `retry: RetryConfig` (from `config.rs`).
- `pub(crate) async fn post_messages(&self, body: &serde_json::Value, stream: bool) -> Result<reqwest::Response, ClientError>`. Loop with retry classifier per spec §4.1 retry decisions table:
  - 2xx → `Decision::Return(resp)` (caller maps non-success-mapped statuses; client doesn't peek at body for non-retry decisions).
  - 429 / 503 → `Decision::Retry { delay_ms }` if attempts left; else `Decision::Error(ClientError::Exhausted { status, attempts })`.
  - Other 4xx → `Decision::Return(resp)` (caller maps to LlmError).
  - 5xx other than 503 → `Decision::Retry` (treated as transient).
  - Network timeout → `Decision::Retry`.
  - Other transport error → `Decision::Error(ClientError::Transport(e))`.
- Retry delay: honor `Retry-After` header when `respect_retry_after`; else exponential `base_delay_ms * 2^(attempt-1)` capped at 60000ms.
- Tracing event `anthropic_plugin::retry` with `attempt`, `max`, `delay_ms` fields per retry.
- `ClientError` is typed: `Transport(reqwest::Error)`, `Exhausted { status: reqwest::StatusCode, attempts: u32 }`.

**Tests** (~4 unit tests using a small in-process server via `tokio::net::TcpListener` accepting raw HTTP, OR using `reqwest::Client::builder().build()` against a `tokio::sync::oneshot` orchestrator):

- `successful_request_returns_response`.
- `retries_429_with_retry_after_header`.
- `retries_429_with_exponential_backoff_when_no_retry_after`.
- `gives_up_after_max_attempts_with_exhausted_error`.
- `does_not_retry_400`.

These tests are nontrivial (need a tiny test HTTP server). The cassette replayer (Task 10) eventually subsumes some of this, but unit-level retry tests are valuable in `client.rs` because they're isolated from cassette parsing.

**Implementation note for unit tests**: spawn a `tokio::net::TcpListener` bound to `127.0.0.1:0`, accept connections, write canned HTTP responses. Or use a minimal embedded test server crate if one is acceptable per workspace dep policy. Hand-rolled is fine — ~50 LOC per test scenario.

**Verification.** Per-task.

**Commit:** `feat(anthropic): HTTP client with retry + Retry-After honoring`.

---

### Task 8: `stream.rs` — SSE parser + `BlockState` machine

**Spec:** §5. **File created:** `crates/tau-plugins/anthropic/src/stream.rs`. **Files modified:** `lib.rs` (`pub mod stream;`).

**Summary.** `pub(crate) async fn parse_sse(body: reqwest::Response) -> Result<CompletionStream, LlmError>`. Per spec §5.2 with **type corrections from plan-erratum**:

- Use `eventsource_stream::Eventsource` to convert the response body to typed events.
- Maintain `blocks: HashMap<u64, BlockState>` keyed on `content_block` index from Anthropic's SSE events.
- `enum BlockState { Text(String), ToolUse(tau_ports::ToolUseAccumulator) }`.
- For each event:
  - `message_start { message: { id, model, usage: { input_tokens, output_tokens } } }`: capture initial `final_usage`.
  - `content_block_start { index, content_block }`: insert new `BlockState` into the map.
  - `content_block_delta { index, delta }`:
    - `text_delta { text }` on a `BlockState::Text`: append to local buffer; **yield `CompletionChunk::Text { delta: text }`** (each text fragment is a chunk per the tau-ports streaming contract).
    - `input_json_delta { partial_json }` on a `BlockState::ToolUse(acc)`: `acc.append(&partial_json)`; do not yield (accumulating).
    - mismatch: yield `Err(LlmError::Stream { message: "delta/block kind mismatch" })`.
  - `content_block_stop { index }`:
    - For text blocks: nothing to do.
    - For tool_use blocks: `acc.finalize_with(|s| serde_json::from_str::<tau_domain::Value>(s).map_err(|e| e.to_string()))` produces a complete `ToolUse`. **Yield `CompletionChunk::ToolUse(tool_use)`** (NOT `ToolUseDelta` — per plan-erratum, the actual variant is the tuple `ToolUse(ToolUse)`).
  - `message_delta { delta: { stop_reason, ... }, usage: { output_tokens } }`: capture `final_stop_reason` and update `final_usage.output_tokens`.
  - `message_stop`: yield `CompletionChunk::Finish { stop_reason: final_stop_reason.unwrap_or(StopReason::EndTurn), usage: Some(final_usage) }` and terminate.
  - `ping`: ignore (heartbeat).
  - `error { error: { type, message } }`: yield `Err(LlmError::Stream { message: format!("anthropic stream error ({}): {}", error.type, error.message) })` and terminate.
- Build the stream via `async_stream::try_stream!`.

**Mid-stream errors do NOT retry** (spec §5.3). The retry layer in `client.rs` only retries the initial request.

**Tests** (~6 unit tests, all using hand-fed SSE event streams via a `Vec<&str>` → `bytes::Bytes` adapter):

- `parses_text_only_stream` — 3 text_delta events + message_stop → 3 Text chunks + Finish.
- `accumulates_tool_use_input_json` — `input_json_delta` fragments concatenate; final `content_block_stop` emits `CompletionChunk::ToolUse(tu)`.
- `propagates_mid_stream_error_event` — `event: error` mid-stream → final item is `Err(LlmError::Stream)`, then stream terminates.
- `ignores_ping_events` — interleaved pings don't perturb output.
- `tracks_usage_across_message_start_and_message_delta` — `final_usage.output_tokens` reflects `message_delta`'s value.
- `unknown_event_kind_logs_warn_and_continues` — synthesize an unknown `event:` name; verify stream continues.

**Verification.** Per-task.

**Commit:** `feat(anthropic): SSE stream parser + BlockState machine`.

---

### Task 9: `plugin.rs` + `main.rs` — `AnthropicPlugin` + `LlmBackend` impl

**Spec:** §6.3, §6.4. **Files created:** `crates/tau-plugins/anthropic/src/plugin.rs`. **Files modified:** `crates/tau-plugins/anthropic/src/lib.rs` (`pub mod plugin;` and add `Configure` impl module integration), `crates/tau-plugins/anthropic/src/main.rs` (real entrypoint).

**Summary.** Per spec §6.3:

- `pub struct AnthropicPlugin { client: AnthropicClient }`.
- `impl Configure for AnthropicPlugin { type Config = AnthropicConfig; fn from_config(cfg) -> Result<Self, ConfigError> }`. Implementation:
  1. `let api_key = config::resolve_api_key(&cfg)?;`
  2. `config::validate_retry(&cfg.retry)?;`
  3. Build `reqwest::Client::builder().timeout(cfg.request_timeout()).user_agent("tau-anthropic-plugin/0.1.0").build()` — on error, `ConfigError::InvalidValue { field: "request_timeout", detail: format!("could not build HTTP client: {e}") }`.
  4. Construct `AnthropicClient { inner, base_url: cfg.base_url, api_key: SecretString::new(api_key), api_version: cfg.api_version, retry: cfg.retry }`.
  5. Return `AnthropicPlugin { client }`.
- `impl tau_ports::LlmBackend for AnthropicPlugin`:
  - `fn name(&self) -> &str { "anthropic" }`
  - `async fn complete(&self, req) -> Result<CompletionResponse, LlmError>`: build body (`stream=false`), `client.post_messages`, check status, read body text, `parse_messages_response`. Errors map via `error::map_client_error` and `error::map_response_error`.
  - `async fn stream(&self, req) -> Result<CompletionStream, LlmError>`: build body (`stream=true`), `client.post_messages`, check status (non-success → read body and `map_response_error`), `stream::parse_sse(resp)`.

`main.rs` becomes:

```rust
//! `anthropic-plugin` binary.

use anthropic_plugin_lib::plugin::AnthropicPlugin;
use tau_plugin_sdk::{run_llm_backend_with_config, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_llm_backend_with_config::<AnthropicPlugin>(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    ).await
}
```

**Tests** (in `plugin.rs::tests`, ~3 unit tests):

- `from_config_with_valid_config_constructs_plugin` — uses a test env var.
- `from_config_with_missing_api_key_returns_invalid_env_var` — pattern-match the variant.
- `name_returns_anthropic` — sanity.

End-to-end LlmBackend tests live in Task 11/12 (cassette tests).

**Verification.** Per-task. Plus: confirm `cargo build --release -p anthropic` produces `target/release/anthropic-plugin`.

**Commit:** `feat(anthropic): AnthropicPlugin + LlmBackend impl + entrypoint`.

---

### Task 10: Cassette replayer (`tests/common/cassette.rs`)

**Spec:** §8.3. **Files created:** `crates/tau-plugins/anthropic/tests/common/mod.rs`, `crates/tau-plugins/anthropic/tests/common/cassette.rs`.

**Summary.** Two parts.

**Part A — choose replayer**: 5-minute survey via `cargo search` or web of these candidates:
- `rvcr` — most-mentioned in 2024-25 but small ecosystem.
- `vcr-cassette` — older, less maintained.
- `mockito` — alive but doesn't match the cassette concept exactly.

**If no maintained crate is found, hand-roll**.

**Part B — hand-rolled replayer** (~200 LOC):

```rust
//! Cassette replayer: a tiny HTTP server that serves recorded
//! responses in order from a YAML cassette file.

use serde::Deserialize;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Deserialize)]
struct CassetteEntry {
    request: RecordedRequest,
    response: RecordedResponse,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RecordedRequest {
    pub method: String,
    pub uri: String,
    pub headers: std::collections::HashMap<String, String>,
    pub body: String,
}

#[derive(Debug, Deserialize, Clone)]
struct RecordedResponse {
    status: u16,
    headers: std::collections::HashMap<String, String>,
    body: String,
}

pub struct CassetteServer {
    base_url: String,
    received: Arc<Mutex<Vec<RecordedRequest>>>,
    _handle: tokio::task::JoinHandle<()>,
}

impl CassetteServer {
    pub fn uri(&self) -> &str { &self.base_url }
    pub fn received_requests(&self) -> Vec<RecordedRequest> {
        self.received.lock().unwrap().clone()
    }
}

pub async fn replay(path: impl AsRef<Path>) -> CassetteServer {
    let yaml = std::fs::read_to_string(path).unwrap();
    let entries: Vec<CassetteEntry> = serde_yaml::from_str(&yaml).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let received = Arc::new(Mutex::new(Vec::<RecordedRequest>::new()));

    let received_clone = received.clone();
    let handle = tokio::spawn(async move {
        let mut idx = 0;
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let entry = entries.get(idx).cloned();
            idx += 1;

            let received_clone = received_clone.clone();
            tokio::spawn(async move {
                // Parse the incoming request line + headers + body
                let mut buf = vec![0u8; 16 * 1024];
                let n = stream.read(&mut buf).await.unwrap();
                let request_text = String::from_utf8_lossy(&buf[..n]).to_string();
                // Crude HTTP parse: first line "METHOD URI HTTP/1.1"
                let (method_line, _) = request_text.split_once("\r\n").unwrap_or((&request_text, ""));
                let parts: Vec<_> = method_line.split_whitespace().collect();
                let recorded = RecordedRequest {
                    method: parts.first().copied().unwrap_or("").to_string(),
                    uri: parts.get(1).copied().unwrap_or("").to_string(),
                    headers: Default::default(),  // omitted for brevity; populated below
                    body: extract_body(&request_text).to_string(),
                };
                received_clone.lock().unwrap().push(recorded);

                let entry = entry.unwrap();
                let mut response = format!(
                    "HTTP/1.1 {} {}\r\n",
                    entry.response.status,
                    status_text(entry.response.status),
                );
                for (k, v) in &entry.response.headers {
                    response.push_str(&format!("{k}: {v}\r\n"));
                }
                response.push_str(&format!("content-length: {}\r\n\r\n", entry.response.body.len()));
                response.push_str(&entry.response.body);
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.flush().await.unwrap();
            });
        }
    });

    CassetteServer {
        base_url: format!("http://127.0.0.1:{port}"),
        received,
        _handle: handle,
    }
}

fn extract_body(req: &str) -> &str {
    if let Some(idx) = req.find("\r\n\r\n") {
        &req[idx + 4..]
    } else { "" }
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK", 400 => "Bad Request", 401 => "Unauthorized",
        403 => "Forbidden", 404 => "Not Found", 429 => "Too Many Requests",
        500 => "Internal Server Error", 503 => "Service Unavailable",
        _ => "Unknown",
    }
}
```

> The exact replayer code adapts at impl time — the snippet above is a starting sketch. Key invariants the implementer must preserve:
> 1. Returns recorded responses in cassette-file order.
> 2. Captures incoming requests in `received_requests()` for assertion.
> 3. Binds to `127.0.0.1:0` so tests get an ephemeral port (parallel-safe).
> 4. Single connection per request (HTTP/1.1 connection: close); serve N entries → handle N connections.
> 5. Streaming SSE responses — for cassettes that have `content-type: text/event-stream`, serve the body as-is (no chunked encoding handling needed since we set `content-length`).

**Test helpers in `tests/common/mod.rs`:**

```rust
pub mod cassette;

pub fn sample_request() -> tau_ports::CompletionRequest {
    let mut req = tau_ports::CompletionRequest::new("claude-3-5-haiku-latest".into());
    req.messages.push(tau_ports::LlmProviderMessage::user(vec![
        tau_ports::ContentBlock::Text("say hi".into()),
    ]));
    req.max_tokens = Some(20);
    req
}

pub fn extract_text(resp: &tau_ports::CompletionResponse) -> &str { &resp.text }

pub fn test_config(base_url: String) -> anthropic_plugin_lib::config::AnthropicConfig {
    let mut cfg = anthropic_plugin_lib::config::AnthropicConfig::default();
    cfg.api_key = Some("sk-ant-test".into());
    cfg.base_url = base_url;
    cfg
}

pub fn test_config_with_retry(
    base_url: String,
    max_attempts: u32,
    base_delay_ms: u64,
) -> anthropic_plugin_lib::config::AnthropicConfig {
    let mut cfg = test_config(base_url);
    cfg.retry.max_attempts = max_attempts;
    cfg.retry.base_delay_ms = base_delay_ms;
    cfg
}
```

**Verification.** Per-task. Plus: a `cassette::tests::serves_recorded_response` self-test in `cassette.rs` that uses an inline cassette + reqwest call to verify the replayer round-trips.

**Commit:** `test(anthropic): cassette replayer for HTTP integration tests`.

---

### Task 11: Cassette files (7 batch) + `tests/complete.rs` integration tests

**Spec:** §8.1, §8.4. **Files created:** `crates/tau-plugins/anthropic/tests/cassettes/{complete_happy_path,complete_with_system_prompt,complete_with_tools,complete_429_then_success,complete_429_exhausted,complete_401_auth_failure,complete_400_bad_request}.yaml`, `crates/tau-plugins/anthropic/tests/complete.rs`.

**Summary.** Author the 7 batch cassettes from spec §8.2's format. Write integration tests that drive `AnthropicPlugin::complete()` against each cassette and assert on the parsed response or error. Tests follow the spec §8.4 layout:

```rust
mod common;
use common::cassette;

#[tokio::test]
async fn complete_happy_path() {
    let server = cassette::replay("tests/cassettes/complete_happy_path.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config(server.uri().into())).unwrap();
    let resp = plugin.complete(common::sample_request()).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Hi there");
    let usage = resp.usage.expect("Anthropic always returns usage");
    assert_eq!(usage.output_tokens, 3);
}

#[tokio::test]
async fn complete_with_system_prompt() {
    let server = cassette::replay("tests/cassettes/complete_with_system_prompt.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config(server.uri().into())).unwrap();
    let mut req = common::sample_request();
    req.system = Some("you are concise".into());
    let resp = plugin.complete(req).await.unwrap();
    /* verify text + that the request body the cassette captured had top-level "system" field */
    let received = server.received_requests();
    assert!(received[0].body.contains(r#""system":"you are concise""#));
}

#[tokio::test]
async fn complete_with_tools() {
    /* drive with req.tools = [echo_tool_spec()]; assert resp.tool_uses has 1 entry */
}

#[tokio::test]
async fn complete_429_then_success() {
    let server = cassette::replay("tests/cassettes/complete_429_then_success.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config_with_retry(
        server.uri().into(), 3, 0,
    )).unwrap();
    let resp = plugin.complete(common::sample_request()).await.unwrap();
    assert!(common::extract_text(&resp).contains("Hi"));
    assert_eq!(server.received_requests().len(), 3);
}

#[tokio::test]
async fn complete_429_exhausted_returns_internal_error() {
    let server = cassette::replay("tests/cassettes/complete_429_exhausted.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config_with_retry(
        server.uri().into(), 3, 0,
    )).unwrap();
    let err = plugin.complete(common::sample_request()).await.unwrap_err();
    let LlmError::Internal { ref message } = err else { panic!("expected Internal: {err:?}") };
    assert!(message.contains("rate limited") || message.contains("retries exhausted"));
    assert_eq!(server.received_requests().len(), 3);
}

#[tokio::test]
async fn complete_401_auth_failure_does_not_retry() {
    let server = cassette::replay("tests/cassettes/complete_401_auth_failure.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config_with_retry(
        server.uri().into(), 3, 0,
    )).unwrap();
    let err = plugin.complete(common::sample_request()).await.unwrap_err();
    let LlmError::Internal { ref message } = err else { panic!() };
    assert!(message.contains("auth failure"));
    assert_eq!(server.received_requests().len(), 1);
}

#[tokio::test]
async fn complete_400_bad_request_does_not_retry() {
    /* verify same: 1 received request, Internal with "bad request" */
}
```

**Cassette authorship**: each YAML carries one or more entries per the spec §8.2 format. For multi-attempt scenarios (429-then-success, 429-exhausted), add multiple entries. Sample cassette content for `complete_happy_path.yaml`:

```yaml
- request:
    method: POST
    uri: /v1/messages
    headers:
      x-api-key: sk-ant-test
      anthropic-version: "2023-06-01"
    body: |-
      {"model":"claude-3-5-haiku-latest","messages":[{"role":"user","content":[{"type":"text","text":"say hi"}]}],"max_tokens":20,"tool_choice":{"type":"auto"}}
  response:
    status: 200
    headers:
      content-type: application/json
    body: |-
      {"id":"msg_01ABC","type":"message","role":"assistant","content":[{"type":"text","text":"Hi there"}],"model":"claude-3-5-haiku-latest","stop_reason":"end_turn","usage":{"input_tokens":12,"output_tokens":3}}
```

> The exact request body the plugin emits depends on Task 4's body builder; the cassette doesn't need to exactly-match the request body if the replayer only sequences responses (which the Task 10 sketch does). If the implementer chooses a request-matching replayer, the cassette `request` field is reference-only.

**Verification.** Per-task. Plus: `cargo build --release -p anthropic` to ensure the binary still builds.

**Commit:** `test(anthropic): batch-mode cassette integration tests`.

---

### Task 12: Cassette files (3 streaming) + `tests/streaming.rs`

**Spec:** §8.1, §5. **Files created:** `crates/tau-plugins/anthropic/tests/cassettes/{stream_text_only,stream_with_tool_use,stream_error_mid_stream}.yaml`, `crates/tau-plugins/anthropic/tests/streaming.rs`.

**Summary.** Author 3 SSE cassettes. Each cassette's `response.body` is the raw SSE text Anthropic would emit. Example `stream_text_only.yaml`:

```yaml
- request:
    method: POST
    uri: /v1/messages
    headers:
      x-api-key: sk-ant-test
      anthropic-version: "2023-06-01"
      accept: text/event-stream
    body: |-
      {"model":"claude-3-5-haiku-latest","messages":[...],"stream":true}
  response:
    status: 200
    headers:
      content-type: text/event-stream
    body: |-
      event: message_start
      data: {"type":"message_start","message":{"id":"msg_01","model":"claude-3-5-haiku-latest","usage":{"input_tokens":10,"output_tokens":1}}}

      event: content_block_start
      data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

      event: content_block_delta
      data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}

      event: content_block_delta
      data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}

      event: content_block_stop
      data: {"type":"content_block_stop","index":0}

      event: message_delta
      data: {"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":3}}

      event: message_stop
      data: {"type":"message_stop"}
```

(YAML-block formatting subtlety: SSE blank-lines-between-events must survive YAML parsing; use `|-` block-literal indicator to preserve newlines.)

**Tests:**

```rust
mod common;
use common::cassette;
use futures_util::StreamExt;
use tau_ports::CompletionChunk;

#[tokio::test]
async fn stream_text_only_yields_chunks_then_finish() {
    let server = cassette::replay("tests/cassettes/stream_text_only.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config(server.uri().into())).unwrap();

    let mut stream = plugin.stream(common::sample_request()).await.unwrap();
    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item.unwrap());
    }
    // 2 text deltas + 1 finish.
    assert_eq!(chunks.len(), 3);
    assert!(matches!(chunks[0], CompletionChunk::Text { ref delta } if delta == "Hello"));
    assert!(matches!(chunks[1], CompletionChunk::Text { ref delta } if delta == " world"));
    let CompletionChunk::Finish { ref usage, .. } = chunks[2] else { panic!() };
    let usage = usage.as_ref().unwrap();
    assert_eq!(usage.output_tokens, 3);
}

#[tokio::test]
async fn stream_with_tool_use_emits_full_tool_use_chunk() {
    /* verify that input_json_delta fragments accumulate and a single
       CompletionChunk::ToolUse(tu) with the parsed input is emitted */
}

#[tokio::test]
async fn stream_error_mid_stream_terminates_with_err() {
    /* verify final stream item is Err(LlmError::Stream { ... }) and
       stream then ends */
}
```

**Verification.** Per-task.

**Commit:** `test(anthropic): streaming cassette integration tests`.

---

### Task 13: Live smoke tests (`tests/live.rs`) + re-record helper

**Spec:** §8.5, §8.6. **Files created:** `crates/tau-plugins/anthropic/tests/live.rs`, `scripts/rerecord-anthropic-cassettes.sh`.

**Summary.** Two `#[ignore]`-by-default tests gated by `TAU_ANTHROPIC_LIVE_TESTS=1` AND `ANTHROPIC_API_KEY=sk-ant-...`:

```rust
//! Live smoke tests. Run with:
//!   TAU_ANTHROPIC_LIVE_TESTS=1 ANTHROPIC_API_KEY=sk-ant-... \
//!     cargo test -p anthropic --test live -- --ignored
//! Costs: ~$0.001 per smoke run on claude-3-5-haiku-latest.

use anthropic_plugin_lib::{config::AnthropicConfig, plugin::AnthropicPlugin};
use tau_plugin_sdk::Configure;
use tau_ports::{CompletionRequest, ContentBlock, LlmBackend, LlmProviderMessage};

#[tokio::test]
#[ignore = "live: requires TAU_ANTHROPIC_LIVE_TESTS=1 and ANTHROPIC_API_KEY"]
async fn live_complete_smoke() {
    if std::env::var("TAU_ANTHROPIC_LIVE_TESTS").is_err() { return; }
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .expect("ANTHROPIC_API_KEY required for live tests");
    let mut cfg = AnthropicConfig::default();
    cfg.api_key = Some(api_key);
    let plugin = AnthropicPlugin::from_config(cfg).unwrap();
    let mut req = CompletionRequest::new("claude-3-5-haiku-latest".into());
    req.messages.push(LlmProviderMessage::user(vec![
        ContentBlock::Text("say hi in exactly 3 words".into()),
    ]));
    req.max_tokens = Some(20);
    let resp = plugin.complete(req).await.unwrap();
    assert!(!resp.text.is_empty());
    eprintln!("live response text: {:?}", resp.text);
    eprintln!("live usage: {:?}", resp.usage);
}

#[tokio::test]
#[ignore = "live: requires TAU_ANTHROPIC_LIVE_TESTS=1 and ANTHROPIC_API_KEY"]
async fn live_stream_smoke() {
    /* same setup; iterate stream; assert at least one Text chunk + Finish */
}
```

**Re-record helper script** (`scripts/rerecord-anthropic-cassettes.sh`):

```bash
#!/usr/bin/env bash
# Re-record Anthropic cassettes against the live API.
# Costs ~$0.05 per full re-record.
#
# Usage:
#   ANTHROPIC_API_KEY=sk-ant-... ./scripts/rerecord-anthropic-cassettes.sh
#
# This script does NOT yet automate cassette regeneration; the
# cassette format is hand-written in v0.1. When automated, this
# will set TAU_RECORD_CASSETTES=1 and run the cassette-aware tests.
set -euo pipefail
: "${ANTHROPIC_API_KEY:?required}"

echo "Cassette files in: crates/tau-plugins/anthropic/tests/cassettes/"
echo "v0.1: cassettes are hand-authored. To verify the live API still"
echo "matches them, run:"
echo
echo "  TAU_ANTHROPIC_LIVE_TESTS=1 ANTHROPIC_API_KEY=\$ANTHROPIC_API_KEY \\"
echo "    cargo test -p anthropic --test live -- --ignored --nocapture"
echo
echo "If the live response shape diverges, manually update the YAMLs."
```

> Automated cassette regeneration (record-mode replayer) is deferred to a future sub-project — flag in the script's body.

`chmod +x scripts/rerecord-anthropic-cassettes.sh` after creation.

**Verification.** Per-task. Live tests are NOT run in CI (the `#[ignore]` skips them).

**Commit:** `test(anthropic): live smoke tests + re-record helper script`.

---

### Task 14: CI — 1 new build job

**Spec:** §10 row 13. **File modified:** `.github/workflows/ci.yml`.

**Summary.** Append after the existing `build-tau-plugins` job:

```yaml
  build-anthropic-plugin:
    name: build (anthropic-plugin)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Build anthropic-plugin (release)
        run: cargo build --release -p anthropic
```

This job verifies the binary builds clean on Ubuntu in release mode. The plugin's tests (cassette + unit) run in the workspace test job (`test (ubuntu-latest / stable)` etc.); they don't need a separate CI job.

**Verification.** Locally:

```bash
cargo build --release -p anthropic
```

Confirm `target/release/anthropic-plugin` exists.

**Commit:** `ci(tau): add build (anthropic-plugin) job`.

---

## Tasks 15-16: user-driven gates

### Task 15: Final local verification + mark PR ready

**Files modified:** none (gate task).

- [ ] Confirm all of the following pass locally on the latest commit:

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace --all-features
cargo test --workspace --all-targets --all-features
cargo test --workspace --doc
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo build -p anthropic --no-default-features
cargo build --release -p anthropic
cargo test -p tau-domain --all-features --test escape_hatch_registry
```

- [ ] Confirm CI on the PR is fully green (all 16 required checks).
- [ ] Mark the PR Ready for review (`gh pr ready` if drafted).

**No commit at this task.**

---

### Task 16: Plan sign-off + ROADMAP + branch protection update + squash merge

- [ ] **Step 16.1: Tick checkboxes in this plan**

Edit `docs/superpowers/plans/2026-04-29-anthropic-plugin.md`: convert all `- [ ]` checkboxes for completed tasks (Tasks 1-15) to `- [x]`. Per the established convention, individual checkboxes may remain unticked — git log on this branch is the authoritative record. Add a top-of-plan `STATUS — COMPLETE` note matching sub-project 1's pattern.

- [ ] **Step 16.2: Update ROADMAP**

Edit `ROADMAP.md`:
- Mark Phase 1 priority 2 (the parent priority — split into 2a/2b/2c sub-projects) — but specifically annotate that **2a (Anthropic plugin) is shipped**, with 2b (Ollama) and 2c (OpenAI) remaining.
- Add a row to the "Phase 1 sub-projects shipped" sub-table for the Anthropic plugin with the merge date.
- Note the CI required-check count is now 16 (was 15).

```bash
cd /Users/titouanlebocq/code/tau
git add ROADMAP.md docs/superpowers/plans/2026-04-29-anthropic-plugin.md
git commit -m "docs(plan): tick off Anthropic plugin sub-project + update ROADMAP

Refs: Task 16 of plan
"
git push
```

- [ ] **Step 16.3: Update branch protection — add 1 new required check**

```bash
gh api repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks/contexts \
  -X POST \
  -f 'contexts[]=build (anthropic-plugin)'
```

This appends `build (anthropic-plugin)` to the existing 15-check list (after sub-project 1's branch-protection update brought the count from 12 to 15). Total: 16 required checks.

- [ ] **Step 16.4: Squash merge PR to main**

After CI is green on the latest commit:

```bash
gh pr merge --squash --delete-branch
```

Squash commit message:

```
feat(anthropic): Phase 1 sub-project 2a — Anthropic LLM-backend plugin

First real LLM-backend plugin for tau. In-tree at
crates/tau-plugins/anthropic/. Validates the plugin loading
mechanism (ADR-0008) end-to-end against real network traffic, real
authentication via API key, real Anthropic error envelopes mapped
to LlmError, and real SSE streaming via eventsource-stream.

Plugin features: Anthropic Messages API, day-1 streaming + tool-use,
exponential retry honoring Retry-After, secret-bearing API key via
env var or handshake config override, full validation in Configure.
Ten cassette-replay integration tests + 2 env-gated live smoke tests
+ ~50 unit tests.

tau-plugin-sdk amendment: ConfigError::InvalidEnvVar variant for
runtime-named env vars.

Branch protection on main now requires 16 status checks (was 15).

Refs: ROADMAP Phase 1 priority 2a
```

- [ ] **Step 16.5: Verify main is clean**

```bash
git checkout main
git pull
git log --oneline -5
```

Confirm the squash commit is at HEAD and branch protection didn't block the merge.

---

## Self-review notes (for the plan author)

Spec coverage check (cross-check against spec §1.1 — "Ships"):

| Spec deliverable | Implementing task |
|---|---|
| New workspace member `crates/tau-plugins/anthropic/` | Task 1 |
| Real HTTP client backed by reqwest | Task 7 |
| Cassette-replay test harness + 10 cassettes | Tasks 10, 11, 12 |
| 2 env-gated live smoke tests | Task 13 |
| Plugin manifest declaring `provides = "llm_backend"` | Task 1 |
| Anthropic Messages API only (Q3) | Tasks 4, 5 |
| Day-1 streaming + tool-use streaming (Q3) | Task 8 |
| Tool-use mapping (request + response, Q4) | Tasks 4, 5 |
| System prompt → top-level `system` field (Q6 corrected per plan-erratum) | Task 4 |
| Model pass-through (Q5) | Task 4 |
| Vision out-of-scope (Q7) | (no task — explicit deferral in §11) |
| Credentials via env var + handshake config override (Q9) | Tasks 2, 3 |
| Token usage pass-through (Q10) | Task 5 |
| Retry with exponential backoff + Retry-After (Q8 → A) | Task 7 |
| All errors collapse to `LlmError::Internal` (decision #16) | Task 6 |
| Cassette + env-gated live testing (Q9 → B) | Tasks 10-13 |
| Hand-rolled replayer if no maintained crate (decision #15) | Task 10 |
| ConfigError::InvalidEnvVar amendment (spec §6.2 plan-erratum) | Task 2 |
| CI: 1 new job `build (anthropic-plugin)` | Task 14 |
| Branch protection update 15 → 16 | Task 16 |

Spec §1.1 "does NOT ship" — all explicit deferrals (Ollama/OpenAI plugins, vision, prompt caching, citations, batches, computer-use, RateLimited variant, auto-reconnect, multi-vendor failover, cost telemetry, anthropic-sdk-rust wrapper) match this plan's scope. No deferred item is accidentally implemented.

Plan-erratum carry-overs accounted for: doctest `ignore`, separate `cargo test --doc`, let-else for `#[non_exhaustive]` destructure, no new `Internal` variants, type-shape corrections (`CompletionChunk::ToolUse(ToolUse)`, `ToolChoice::Specific`, `CompletionResponse::text`/`tool_uses`/`usage: Option<...>`, `ContentBlock::Text(String)` tuple, `req.system: Option<String>`, `ToolSpec::input_schema`).

Type consistency check:

- `AnthropicConfig` defined Task 3; consumed Tasks 7, 9, 10, 11, 12, 13.
- `RetryConfig` defined Task 3; consumed Task 7, test helpers.
- `AnthropicClient` defined Task 7; consumed Task 9.
- `AnthropicPlugin` defined Task 9; consumed Tasks 11, 12, 13.
- `BuildError`, `ParseError`, `ClientError` are plugin-internal typed errors; mapped to `LlmError::Internal` in `error.rs` (Task 6). No leakage to public surface.
- `ConfigError::InvalidEnvVar` defined Task 2; consumed Task 3, 11 (in test assertions).

All consistent. No unresolved type-name drift.

Three impl-time ambiguities the spec flagged are addressed:

1. **`CompletionChunk::ToolUseDelta` variant**: resolved by reading actual `tau-ports` types — the variant is `CompletionChunk::ToolUse(ToolUse)`. Plan-erratum at top documents this; Task 8 uses the correct variant.
2. **`ConfigError::InvalidValue { field: &'static str }` extension**: resolved as Task 2 — add a new typed variant `InvalidEnvVar { name: String, detail: String }`.
3. **Cassette replayer crate vs hand-rolled**: Task 10 starts with a 5-minute survey; if no maintained crate, hand-roll per the sketch.
