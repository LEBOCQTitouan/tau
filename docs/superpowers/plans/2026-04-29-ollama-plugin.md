# Ollama LLM-backend plugin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `crates/tau-plugins/ollama/`, the second real LLM-backend plugin: an Ollama (local LLM runner) client targeting the native `POST /api/chat` endpoint with NDJSON streaming, optional bearer-token auth, in-plugin retry honoring `Retry-After`, and a cassette-replay test harness.

**Architecture:** Out-of-process plugin spawned by `tau-runtime::plugin_host` per ADR-0008. Talks MessagePack-RPC over stdio via `tau-plugin-sdk::run_llm_backend_with_config`. HTTP layer built on `reqwest`; streaming hand-rolled (~50 LOC split-on-`\n`) — **no `eventsource-stream`** because Ollama emits NDJSON, not SSE. Code is **duplicated** from `crates/tau-plugins/anthropic/` (cassette replayer, retry-loop shape, error mapping); rule-of-three refactor deferred to sub-project 2c (OpenAI).

**Tech Stack:** Rust 1.91, `reqwest 0.12` (rustls + json + stream), `async-stream`, `secrecy`, `tokio` (multi-thread), `serde` + `serde_json`, `tracing`, `serde_yaml` (cassettes, dev-only). **No new workspace deps** — all required deps already present from sub-project 2a.

**Sub-project scope:** Phase 1 priority 2b. Spec at [`docs/superpowers/specs/2026-04-29-ollama-plugin-design.md`](../specs/2026-04-29-ollama-plugin-design.md) (commit `14a3f56`).

---

## Plan-erratum: types, conventions, and traps

These are pre-known invariants from sub-projects 1 + 2a. Apply them
verbatim — do NOT re-derive them by reading the spec.

### Actual `tau-ports` types (read these once, trust thereafter)

The spec uses idiomatic Rust prose; the runner subagent must use the
**actual** type shapes:

| Concern | Actual type / shape |
|---|---|
| Streaming chunk variants | `CompletionChunk::Text { delta: String }` / `CompletionChunk::ToolUse(ToolUse)` (tuple variant!) / `CompletionChunk::Finish { stop_reason: StopReason, usage: Option<TokenUsage> }` |
| Batch response | `CompletionResponse { text: String, tool_uses: Vec<ToolUse>, stop_reason: StopReason, usage: Option<TokenUsage> }` — **flat** shape, `#[non_exhaustive]`. Construct via `tau_ports::fixtures::make_completion_response(text, tool_uses, stop_reason, usage)` (the `test-fixtures` feature is enabled in this crate's deps). |
| System prompt | `CompletionRequest::system: Option<String>` — **top-level** field. Ollama mapping: prepend a leading `{role:"system", content}` message to the `messages` array. |
| Content blocks | `ContentBlock::Text(String)` is a **tuple** variant (NOT `Text { text: String }`). `ContentBlock::ToolUse(ToolUse)` is also tuple. |
| Tool choice | `ToolChoice::Auto` / `ToolChoice::None` / `ToolChoice::Required` / `ToolChoice::Specific { name: String }` (NOT `ForceTool`) |
| Tool spec | `ToolSpec { name: String, description: String, input_schema: serde_json::Value }` (field is `input_schema`, NOT `parameters_json`) |
| Stop reasons | `StopReason::{EndTurn, MaxTokens, StopSequence, ToolUse, Error}` — NO `Other(String)` variant. Unknown `done_reason` strings map to `EndTurn` with a `tracing::warn!`. |
| Token usage | `TokenUsage::new(input_tokens: u32, output_tokens: u32)` — both `u32`. Defensive parse: when either Ollama field is absent, set `usage = None`. |
| Tool use construction | `ToolUse::new(id: String, name: String, input: tau_domain::Value)` |

### Wire-protocol carryovers (handled by the SDK, but plugin code must match)

- Wire methods are `llm.complete` and `llm.stream` (SDK names them; plugin code never names these strings).
- `CompletionChunk::Finish` (NOT `Done`) terminates a stream.

### `#[non_exhaustive]` discipline

- Doctests on `#[non_exhaustive]` types must use ` ```ignore ` fences (else E0639 from external doctest compilation).
- Cross-crate destructuring of `#[non_exhaustive]` enums: prefer `let X { fields, .. } = value else { panic!() };` for multi-variant enums; `assert!(matches!(...))` for single-variant or in-crate same-module patterns.
- Cross-crate struct construction: use `Default::default()` then field assignment (NOT struct-literal `..Default::default()` shorthand on foreign types).

### Verification protocol

`cargo test --all-targets` does **not** run doctests. Each task's
verification block runs `cargo test --doc` separately when the task
adds public items.

### Same-commit escape-hatch registry

The mechanical CI test at `crates/tau-domain/tests/escape_hatch_registry.rs` continues to gate against accidental additions of `Internal`/`Custom` variants. **No new such variants ship in this sub-project.** All Ollama errors map to existing `LlmError::Internal { message }` per spec §4.4.

### What this sub-project does NOT introduce

- No new ADR (purely additive).
- No new workspace deps (`reqwest`, `secrecy`, `async-stream`, etc. all from sub-project 2a).
- No new `tau-plugin-sdk` types (`ConfigError::InvalidEnvVar` already shipped in sub-project 2a; reused here for the bearer-token-env case).
- No new `LlmError` / `StopReason` / `ContentBlock` variants.
- No `eventsource-stream` dep — NDJSON is hand-rolled.

---

## File Structure

```
crates/tau-plugins/ollama/
├── Cargo.toml                    -- bin: ollama-plugin; lib: ollama_plugin_lib
├── tau.toml                      -- plugin manifest (provides=llm_backend)
├── src/
│   ├── main.rs                   -- #[tokio::main] → run_llm_backend_with_config
│   ├── lib.rs                    -- pub modules; crate-level docs
│   ├── plugin.rs                 -- OllamaPlugin + LlmBackend impl + Configure
│   ├── config.rs                 -- OllamaConfig + RetryConfig + resolve_bearer_token + validate_retry
│   ├── client.rs                 -- OllamaClient (reqwest) + post_chat + retry loop
│   ├── request.rs                -- CompletionRequest → /api/chat JSON
│   ├── response.rs               -- /api/chat JSON → CompletionResponse
│   ├── stream.rs                 -- NDJSON parser → CompletionStream (~50 LOC)
│   └── error.rs                  -- HTTP status + Ollama error envelope → LlmError
└── tests/
    ├── cassettes/                -- 9 cassette YAMLs (6 batch + 3 streaming)
    │   ├── complete_happy_path.yaml
    │   ├── complete_with_system_prompt.yaml
    │   ├── complete_with_tools.yaml
    │   ├── complete_503_model_loading_then_success.yaml
    │   ├── complete_404_model_not_pulled.yaml
    │   ├── complete_400_bad_request.yaml
    │   ├── stream_text_only.yaml
    │   ├── stream_with_tool_use.yaml
    │   └── stream_truncated_response.yaml
    ├── common/
    │   ├── mod.rs                -- helpers (DUPLICATED from anthropic)
    │   └── cassette.rs           -- TCP server replayer (DUPLICATED)
    ├── complete.rs               -- batch tests via cassette replay
    ├── streaming.rs              -- streaming tests via cassette replay
    └── live.rs                   -- env-gated smoke tests (#[ignore])

scripts/rerecord-ollama-cassettes.sh  -- live re-record helper

.github/workflows/ci.yml          -- + 1 new job: build (ollama-plugin)
Cargo.toml                        -- + workspace member
```

---

## Tasks 1-3: detailed (Plan-2 fidelity)

The first three tasks are documented at full fidelity (every code
snippet, every step, every verification command). Tasks 4-13 follow
the hybrid format (per-task summary + spec section references).

---

### Task 1: Workspace scaffold

Create the empty crate skeleton, register it in the workspace, verify
it builds. **No new workspace deps** — the crate's `Cargo.toml` only
references items already present from sub-project 2a.

**Files:**
- Create: `crates/tau-plugins/ollama/Cargo.toml`
- Create: `crates/tau-plugins/ollama/tau.toml`
- Create: `crates/tau-plugins/ollama/src/main.rs`
- Create: `crates/tau-plugins/ollama/src/lib.rs`
- Modify: `Cargo.toml` (workspace root, add member)

- [ ] **Step 1.1: Add the workspace member**

Edit `Cargo.toml` (workspace root) — append `crates/tau-plugins/ollama` to `workspace.members`:

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
    "crates/tau-plugins/echo-llm",
    "crates/tau-plugins/echo-tool",
    "crates/tau-plugins/anthropic",
    "crates/tau-plugins/ollama",
]
```

(NO new `[workspace.dependencies]` entries — the Anthropic plugin already added `reqwest`, `secrecy`, `async-stream` to the workspace dep table.)

- [ ] **Step 1.2: Create `crates/tau-plugins/ollama/Cargo.toml`**

```toml
[package]
name = "ollama"
description = "Ollama (local LLM runner) backend for tau."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[[bin]]
name = "ollama-plugin"
path = "src/main.rs"

[lib]
name = "ollama_plugin_lib"
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
async-stream        = { workspace = true }
secrecy             = { workspace = true }
futures-core        = { workspace = true }
futures-util        = "0.3"

[dev-dependencies]
tokio        = { workspace = true, features = ["macros", "rt-multi-thread", "io-util", "net"] }
tempfile     = { workspace = true }
serde_yaml   = "0.9"
```

> Differences from `crates/tau-plugins/anthropic/Cargo.toml`: package
> name `"ollama"` (binary `ollama-plugin`, lib `ollama_plugin_lib`),
> description, **no `eventsource-stream` dep** (NDJSON parser is
> hand-rolled). `serde_yaml` is present in dev-deps for cassette
> parsing. The `test-fixtures` feature on `tau-ports` is required
> because `parse_chat_response` constructs `CompletionResponse` via
> `tau_ports::fixtures::make_completion_response` (Task 4).

- [ ] **Step 1.3: Create `crates/tau-plugins/ollama/tau.toml`**

```toml
name = "ollama"
version = "0.1.0"
description = "Ollama (local LLM runner) backend for tau."

[plugin]
provides = "llm_backend"
kind     = "rust-cargo"
bin      = "ollama-plugin"
```

- [ ] **Step 1.4: Create `crates/tau-plugins/ollama/src/lib.rs` (empty stub)**

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Ollama (local LLM runner) plugin internals.
//!
//! The binary entrypoint at `src/main.rs` calls
//! `tau_plugin_sdk::run_llm_backend_with_config::<OllamaPlugin>(...)`.
//!
//! See `docs/superpowers/specs/2026-04-29-ollama-plugin-design.md`
//! for the design rationale.

// Modules will be added in subsequent tasks (config, request, response,
// error, client, stream, plugin).
```

- [ ] **Step 1.5: Create `crates/tau-plugins/ollama/src/main.rs` (placeholder stub)**

The dispatch wiring lands in Task 8. For Task 1, the binary simply needs to compile.

```rust
//! `ollama-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! The full implementation (handshake + dispatch loop) lands in Task 8.
//! For Task 1, this stub exists only so that `cargo build` succeeds.

fn main() {
    eprintln!("ollama-plugin: not yet wired (placeholder; see Task 8)");
    std::process::exit(1);
}
```

- [ ] **Step 1.6: Verify the workspace builds**

Run: `cargo build --workspace`
Expected: PASS — new `ollama` crate is recognized; `target/debug/ollama-plugin` is produced.

Run: `cargo build -p ollama`
Expected: PASS.

- [ ] **Step 1.7: Verify fmt + clippy + tests + doctests on the new crate**

Run: `cargo fmt --all -- --check`
Expected: PASS.

Run: `cargo clippy -p ollama --all-targets --all-features -- -D warnings`
Expected: PASS (no clippy lints on the empty stub; `-A unused_imports` is NOT used).

Run: `cargo test -p ollama --all-targets`
Expected: PASS with `0 tests`.

Run: `cargo test -p ollama --doc`
Expected: PASS with `0 tests`.

Run: `cargo test --workspace --all-targets`
Expected: PASS — pre-existing tests continue to pass.

- [ ] **Step 1.8: Commit**

```bash
git add Cargo.toml crates/tau-plugins/ollama/
git commit -m "feat(ollama): scaffold workspace member crate

Empty stub for Phase 1 sub-project 2b (Ollama LLM-backend plugin).
Registers crates/tau-plugins/ollama/ as a workspace member; binary
target ollama-plugin (placeholder); lib target ollama_plugin_lib
(empty modules to follow). tau.toml manifest declares
provides=llm_backend.

NO new workspace deps — reqwest, secrecy, async-stream were added
in sub-project 2a (Anthropic plugin); reused here. NO eventsource-
stream dep — Ollama uses NDJSON, hand-rolled in Task 7.

Refs: docs/superpowers/specs/2026-04-29-ollama-plugin-design.md §3.1"
```

- [ ] **Step 1.9: Push**

```bash
git push
```

PR auto-triggers CI. Wait for CI green before Task 2.

---

### Task 2: `OllamaConfig` + `RetryConfig` + `Configure` impl

Add the configuration shape, the env-or-direct bearer-token resolver,
and retry-config validation. Reuses
`ConfigError::InvalidEnvVar { name, detail }` already shipped in
sub-project 2a — **no SDK amendment needed**. The bearer token is
**optional**: missing config + missing env var → `Ok(None)` (the common
case for local Ollama at `http://localhost:11434`).

**Files:**
- Create: `crates/tau-plugins/ollama/src/config.rs`
- Modify: `crates/tau-plugins/ollama/src/lib.rs` (re-export `pub mod config`)

- [ ] **Step 2.1: Add `pub mod config` to `lib.rs`**

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Ollama (local LLM runner) plugin internals.
//!
//! The binary entrypoint at `src/main.rs` calls
//! `tau_plugin_sdk::run_llm_backend_with_config::<OllamaPlugin>(...)`.
//!
//! See `docs/superpowers/specs/2026-04-29-ollama-plugin-design.md`
//! for the design rationale.

pub mod config;
```

- [ ] **Step 2.2: Write the failing tests in `src/config.rs`**

The test list (9 tests, ordered to drive the implementation):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_production_ready() {
        let cfg = OllamaConfig::default();
        assert_eq!(cfg.base_url, "http://localhost:11434");
        assert_eq!(cfg.bearer_token_env, "OLLAMA_BEARER_TOKEN");
        assert!(cfg.bearer_token.is_none());
        assert_eq!(cfg.request_timeout_secs, 900);
        assert_eq!(cfg.retry.max_attempts, 3);
        assert_eq!(cfg.retry.base_delay_ms, 1000);
        assert!(cfg.retry.respect_retry_after);
    }

    #[test]
    fn deserializes_empty_object_as_defaults() {
        let cfg: OllamaConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.base_url, "http://localhost:11434");
        assert_eq!(cfg.retry.max_attempts, 3);
    }

    #[test]
    fn rejects_unknown_fields() {
        let result: Result<OllamaConfig, _> =
            serde_json::from_str(r#"{"unknown_key": "value"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn resolve_bearer_token_uses_config_override() {
        let cfg = OllamaConfig {
            bearer_token: Some("hosted-token-xyz".into()),
            ..OllamaConfig::default()
        };
        let token = resolve_bearer_token(&cfg).unwrap();
        assert_eq!(token.as_deref(), Some("hosted-token-xyz"));
    }

    #[test]
    fn resolve_bearer_token_reads_env_var() {
        let env_name = "TEST_OLLAMA_RESOLVE_TOKEN_FROM_ENV";
        std::env::set_var(env_name, "envtoken123");
        let cfg = OllamaConfig {
            bearer_token_env: env_name.into(),
            ..OllamaConfig::default()
        };
        let token = resolve_bearer_token(&cfg).unwrap();
        assert_eq!(token.as_deref(), Some("envtoken123"));
        std::env::remove_var(env_name);
    }

    #[test]
    fn resolve_bearer_token_missing_env_returns_none() {
        // Distinct from Anthropic: Ollama auth is OPTIONAL. A missing
        // env var is not an error; it means "no auth header sent".
        let cfg = OllamaConfig {
            bearer_token_env: "DEFINITELY_NOT_SET_OLLAMA_TOK_QXZ".into(),
            ..OllamaConfig::default()
        };
        let token = resolve_bearer_token(&cfg).unwrap();
        assert!(token.is_none());
    }

    #[test]
    fn resolve_bearer_token_empty_env_treated_as_none() {
        // Defensive: an empty-string env var is treated the same as
        // unset. Avoids spurious `Authorization: Bearer ` headers.
        let env_name = "TEST_OLLAMA_EMPTY_TOKEN";
        std::env::set_var(env_name, "");
        let cfg = OllamaConfig {
            bearer_token_env: env_name.into(),
            ..OllamaConfig::default()
        };
        let token = resolve_bearer_token(&cfg).unwrap();
        assert!(token.is_none());
        std::env::remove_var(env_name);
    }

    #[test]
    fn validate_retry_zero_attempts_rejected() {
        let retry = RetryConfig {
            max_attempts: 0,
            base_delay_ms: 100,
            respect_retry_after: true,
        };
        let err = validate_retry(&retry).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::InvalidValue {
                field: "retry.max_attempts",
                ..
            }
        ));
    }

    #[test]
    fn validate_retry_one_attempt_ok() {
        let retry = RetryConfig {
            max_attempts: 1,
            base_delay_ms: 100,
            respect_retry_after: true,
        };
        validate_retry(&retry).unwrap();
    }
}
```

- [ ] **Step 2.3: Verify the tests fail to compile**

Run: `cargo test -p ollama --all-targets`
Expected: FAIL with `OllamaConfig`, `RetryConfig`, `resolve_bearer_token`, `validate_retry` not found.

- [ ] **Step 2.4: Write the minimal implementation in `src/config.rs`**

```rust
//! Ollama plugin configuration.
//!
//! Deserialized from the handshake `config` field by
//! [`tau_plugin_sdk::run_llm_backend_with_config`]. Three concerns:
//! base URL, optional bearer-token auth, and retry tuning.
//!
//! See `docs/superpowers/specs/2026-04-29-ollama-plugin-design.md` §6.

use serde::Deserialize;
use std::time::Duration;
use tau_plugin_sdk::ConfigError;

/// Top-level config for the Ollama plugin.
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
/// // `OllamaConfig` is `#[non_exhaustive]`; external callers
/// // construct via serde or Default.
/// use ollama_plugin_lib::config::OllamaConfig;
/// let cfg = OllamaConfig::default();
/// assert_eq!(cfg.base_url, "http://localhost:11434");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OllamaConfig {
    /// Override base URL. Default: <http://localhost:11434>.
    /// Tests use this to point at the cassette replayer.
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// Override env var name for an optional bearer token.
    /// Default: `OLLAMA_BEARER_TOKEN`. Unset env var → no
    /// `Authorization` header sent (correct for local Ollama).
    #[serde(default = "default_bearer_token_env")]
    pub bearer_token_env: String,

    /// Direct bearer-token override. **Test-only** — never put a real
    /// token in project tau.toml. If both `bearer_token` and
    /// `bearer_token_env` are present, `bearer_token` wins and a
    /// `tracing::warn!` is emitted.
    #[serde(default)]
    pub bearer_token: Option<String>,

    /// Per-request HTTP timeout in seconds. Default: 900 (15 min).
    /// Local Ollama can take 30–60s to load a model on first call.
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,

    /// Retry behavior. Defaults match the Anthropic plugin.
    #[serde(default)]
    pub retry: RetryConfig,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            bearer_token_env: default_bearer_token_env(),
            bearer_token: None,
            request_timeout_secs: default_request_timeout_secs(),
            retry: RetryConfig::default(),
        }
    }
}

impl OllamaConfig {
    /// Per-request HTTP timeout as a `Duration`.
    pub fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.request_timeout_secs)
    }
}

/// Retry behavior for transient errors (429, 503-on-model-load,
/// network timeouts).
///
/// 503 is the load-bearing case for Ollama: returned during model
/// load, which can take 10–60s. Standard exponential backoff
/// (1s, 2s, 4s) handles short loads.
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

    /// Base delay in milliseconds for exponential backoff.
    /// Effective delay = `base_delay_ms * 2^(attempt-1)`, capped at 60s.
    /// Default: 1000.
    #[serde(default = "default_base_delay_ms")]
    pub base_delay_ms: u64,

    /// Honor the `Retry-After` response header when present (parsed
    /// as integer seconds). Default: true.
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

fn default_base_url() -> String {
    "http://localhost:11434".into()
}
fn default_bearer_token_env() -> String {
    "OLLAMA_BEARER_TOKEN".into()
}
fn default_request_timeout_secs() -> u64 {
    900
}
fn default_max_attempts() -> u32 {
    3
}
fn default_base_delay_ms() -> u64 {
    1_000
}
fn default_respect_retry_after() -> bool {
    true
}

/// Resolve an optional bearer token from config or env.
///
/// Returns `Ok(None)` when neither is set — the common case for local
/// Ollama. **Distinct from the Anthropic plugin's `resolve_api_key`,
/// which errors on missing env var because Anthropic auth is required.**
///
/// Wired into `Configure::from_config` in Task 8.
pub(crate) fn resolve_bearer_token(cfg: &OllamaConfig) -> Result<Option<String>, ConfigError> {
    if let Some(direct) = cfg.bearer_token.as_ref() {
        tracing::warn!(
            target: "ollama_plugin::config",
            "config.bearer_token set directly — recommended only for tests",
        );
        return Ok(Some(direct.clone()));
    }
    match std::env::var(&cfg.bearer_token_env) {
        Ok(v) if v.is_empty() => Ok(None),
        Ok(v) => Ok(Some(v)),
        Err(_) => Ok(None),
    }
}

/// Validate retry-config invariants beyond what serde catches.
///
/// Wired into `Configure::from_config` in Task 8.
pub(crate) fn validate_retry(retry: &RetryConfig) -> Result<(), ConfigError> {
    if retry.max_attempts == 0 {
        return Err(ConfigError::InvalidValue {
            field: "retry.max_attempts",
            detail: "must be >= 1 (use 1 for no-retry semantics)".into(),
        });
    }
    Ok(())
}
```

- [ ] **Step 2.5: Run the tests**

Run: `cargo test -p ollama --all-targets`
Expected: PASS — 9 tests pass.

- [ ] **Step 2.6: Verify doctests + fmt + clippy**

Run: `cargo test -p ollama --doc`
Expected: PASS — doctest on `OllamaConfig` is `ignore`-marked so it doesn't run; counts as 0 active.

Run: `cargo fmt --all -- --check`
Expected: PASS.

Run: `cargo clippy -p ollama --all-targets --all-features -- -D warnings`
Expected: PASS.

Run: `cargo build --workspace`
Expected: PASS.

- [ ] **Step 2.7: Commit**

```bash
git add crates/tau-plugins/ollama/src/config.rs crates/tau-plugins/ollama/src/lib.rs
git commit -m "feat(ollama): add OllamaConfig + RetryConfig + token resolver

Configuration shape for the Ollama plugin (spec §6.1):
- base_url default http://localhost:11434
- bearer_token_env default OLLAMA_BEARER_TOKEN (optional auth)
- request_timeout_secs default 900 (15 min; first-call model loads)
- retry: same defaults as Anthropic plugin

resolve_bearer_token returns Ok(None) when neither config nor env
provides a token — local Ollama needs no auth. Distinct from the
Anthropic plugin's resolve_api_key, which errors on missing env.

Reuses ConfigError::InvalidValue and ConfigError::InvalidEnvVar
from sub-project 2a; no new SDK amendment.

9 unit tests covering defaults, deserialization, env precedence,
empty-env-treated-as-none, retry validation.

Refs: docs/superpowers/specs/2026-04-29-ollama-plugin-design.md §6.1, §6.2"
```

- [ ] **Step 2.8: Push**

```bash
git push
```

Wait for CI green before Task 3.

---

### Task 3: `request.rs` — `CompletionRequest` → Ollama `/api/chat` JSON

Build the request body. Ollama's `/api/chat` shape differs from
Anthropic's Messages API in several specific ways:

1. **System prompt as a leading `role:system` message** in `messages`
   (Anthropic uses a top-level `system` field).
2. **Multi-block content concatenated to a flat string** (Ollama
   content is `String`, not array of typed blocks).
3. **Assistant `ToolUse` blocks split into a separate `tool_calls`
   array**; remaining text concatenated into `content`.
4. **Sampling overrides go inside an `options` sub-object**, with
   Ollama-specific names (`num_predict` not `max_tokens`).
5. **`tool_choice` dropped** (Ollama's `/api/chat` doesn't accept it);
   `Required` and `Specific` log a `tracing::debug!`.
6. **`tool_use_id` not round-tripped** to Ollama (its tool message has
   no such field; ordering pairs them).

**Files:**
- Create: `crates/tau-plugins/ollama/src/request.rs`
- Modify: `crates/tau-plugins/ollama/src/lib.rs` (`pub(crate) mod request`)

- [ ] **Step 3.1: Add the module to `lib.rs`**

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Ollama (local LLM runner) plugin internals.
//!
//! The binary entrypoint at `src/main.rs` calls
//! `tau_plugin_sdk::run_llm_backend_with_config::<OllamaPlugin>(...)`.
//!
//! See `docs/superpowers/specs/2026-04-29-ollama-plugin-design.md`
//! for the design rationale.

pub mod config;
pub(crate) mod request;
```

- [ ] **Step 3.2: Write the failing tests (10 tests)**

Create `crates/tau-plugins/ollama/src/request.rs` with the test module first:

```rust
//! Translate `tau_ports::CompletionRequest` to Ollama's `/api/chat`
//! JSON body.
//!
//! See `docs/superpowers/specs/2026-04-29-ollama-plugin-design.md`
//! §4.2, §7.1, §7.2.

use serde_json::Value;
use tau_domain::Value as DomainValue;
use tau_ports::{
    ContentBlock, CompletionRequest, LlmProviderMessage, ToolChoice, ToolSpec, ToolUse,
};
use thiserror::Error;

// ... implementation goes below ...

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tau_ports::fixtures::make_completion_request;

    fn req_with_user_text(text: &str) -> CompletionRequest {
        // make_completion_request(model, messages, ...)
        let mut req = make_completion_request(
            "llama3.2",
            vec![LlmProviderMessage::User {
                content: vec![ContentBlock::Text(text.into())],
            }],
        );
        req
    }

    #[test]
    fn happy_path_user_text_only() {
        let req = req_with_user_text("hello");
        let body = build_chat_body(&req, false).unwrap();
        assert_eq!(body["model"], "llama3.2");
        assert_eq!(body["stream"], false);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "hello");
        assert!(body.get("tools").is_none());
        assert!(body.get("options").is_none());
    }

    #[test]
    fn streaming_flag_propagates() {
        let req = req_with_user_text("hi");
        let body = build_chat_body(&req, true).unwrap();
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn system_prompt_emitted_as_leading_role_system_message() {
        let mut req = req_with_user_text("hi");
        req.system = Some("you are concise".into());
        let body = build_chat_body(&req, false).unwrap();
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "you are concise");
        assert_eq!(messages[1]["role"], "user");
        // Critical: NO top-level `system` field at body root.
        assert!(body.get("system").is_none());
    }

    #[test]
    fn multi_block_user_content_concatenated_to_string() {
        let mut req = req_with_user_text("ignored");
        req.messages = vec![LlmProviderMessage::User {
            content: vec![
                ContentBlock::Text("part one ".into()),
                ContentBlock::Text("part two".into()),
            ],
        }];
        let body = build_chat_body(&req, false).unwrap();
        assert_eq!(body["messages"][0]["content"], "part one part two");
    }

    #[test]
    fn assistant_tool_use_splits_into_tool_calls_array() {
        let tu = ToolUse::new(
            "ollama-tool-0".into(),
            "echo".into(),
            DomainValue::Object(
                vec![("text".into(), DomainValue::String("hi".into()))]
                    .into_iter()
                    .collect(),
            ),
        );
        let mut req = req_with_user_text("ignored");
        req.messages = vec![LlmProviderMessage::Assistant {
            content: vec![
                ContentBlock::Text("ok let me ".into()),
                ContentBlock::ToolUse(tu),
            ],
        }];
        let body = build_chat_body(&req, false).unwrap();
        let asst = &body["messages"][0];
        assert_eq!(asst["role"], "assistant");
        assert_eq!(asst["content"], "ok let me ");
        let calls = asst["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["function"]["name"], "echo");
        assert_eq!(calls[0]["function"]["arguments"]["text"], "hi");
    }

    #[test]
    fn tool_result_message_has_no_tool_use_id_field() {
        let mut req = req_with_user_text("ignored");
        req.messages = vec![LlmProviderMessage::ToolResult {
            tool_use_id: "ignored-by-ollama".into(),
            content: vec![ContentBlock::Text("42".into())],
            is_error: false,
        }];
        let body = build_chat_body(&req, false).unwrap();
        let msg = &body["messages"][0];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["content"], "42");
        // Critical: ordering pairs tool calls/results, not ids.
        assert!(msg.get("tool_use_id").is_none());
    }

    #[test]
    fn tools_array_emitted_when_non_empty() {
        let mut req = req_with_user_text("hi");
        req.tools = vec![ToolSpec {
            name: "echo".into(),
            description: "echo back".into(),
            input_schema: json!({"type": "object"}),
        }];
        let body = build_chat_body(&req, false).unwrap();
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "echo");
        assert_eq!(tools[0]["function"]["description"], "echo back");
        assert_eq!(tools[0]["function"]["parameters"], json!({"type": "object"}));
    }

    #[test]
    fn tool_choice_none_omits_tools_array_entirely() {
        let mut req = req_with_user_text("hi");
        req.tools = vec![ToolSpec {
            name: "echo".into(),
            description: "".into(),
            input_schema: json!({"type": "object"}),
        }];
        req.tool_choice = ToolChoice::None;
        let body = build_chat_body(&req, false).unwrap();
        // Critical: ToolChoice::None drops `tools` even when present.
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn tool_choice_specific_dropped_with_no_field_emitted() {
        let mut req = req_with_user_text("hi");
        req.tools = vec![ToolSpec {
            name: "echo".into(),
            description: "".into(),
            input_schema: json!({"type": "object"}),
        }];
        req.tool_choice = ToolChoice::Specific {
            name: "echo".into(),
        };
        let body = build_chat_body(&req, false).unwrap();
        // tools still emitted (caller wants tools available)…
        assert!(body.get("tools").is_some());
        // …but tool_choice never gets sent (Ollama doesn't accept it).
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn sampling_overrides_go_into_options_subobject_with_ollama_names() {
        let mut req = req_with_user_text("hi");
        req.max_tokens = Some(100);
        req.temperature = Some(0.7);
        req.top_p = Some(0.9);
        req.seed = Some(42);
        req.stop_sequences = vec!["END".into()];
        let body = build_chat_body(&req, false).unwrap();
        let opts = body["options"].as_object().unwrap();
        // num_predict NOT max_tokens — Ollama-specific name.
        assert_eq!(opts["num_predict"], 100);
        assert_eq!(opts["temperature"], 0.7);
        assert_eq!(opts["top_p"], 0.9);
        assert_eq!(opts["seed"], 42);
        assert_eq!(opts["stop"], json!(["END"]));
    }
}
```

- [ ] **Step 3.3: Verify the tests fail to compile**

Run: `cargo test -p ollama --all-targets`
Expected: FAIL with `build_chat_body`, `BuildError`, helpers not found.

- [ ] **Step 3.4: Write the implementation**

Append the implementation to `crates/tau-plugins/ollama/src/request.rs` (above the `#[cfg(test)]` block):

```rust
/// Errors raised while building the Ollama request body.
#[derive(Debug, Error)]
pub(crate) enum BuildError {
    /// A `LlmProviderMessage` variant wasn't recognized — possible if
    /// `tau-ports` adds a new variant before the plugin is updated.
    #[error("unknown LlmProviderMessage variant")]
    UnknownMessageVariant,

    /// A `ContentBlock` variant inside an Assistant message wasn't
    /// recognized — possible if `tau-ports` adds e.g. an `Image` block
    /// before the plugin is updated.
    #[error("unknown ContentBlock variant in assistant content")]
    UnknownContentBlock,

    /// Failed to convert a `tau_domain::Value` to JSON — should be
    /// infallible in practice but propagated as a typed error.
    #[error("could not serialize tool input as JSON: {0}")]
    JsonSerialize(#[from] serde_json::Error),
}

/// Build the JSON body for a `POST /api/chat` request.
///
/// `stream` controls the `"stream"` field (false for batch, true for
/// NDJSON streaming).
pub(crate) fn build_chat_body(
    req: &CompletionRequest,
    stream: bool,
) -> Result<Value, BuildError> {
    let mut body = serde_json::json!({
        "model": req.model,
        "messages": translate_messages(req)?,
        "stream": stream,
    });

    // Tools array. Omit entirely when:
    // - no tools provided, OR
    // - tool_choice == None (caller explicitly disabled tools).
    if !req.tools.is_empty() && !matches!(req.tool_choice, ToolChoice::None) {
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .map(translate_tool)
                .collect::<Result<Vec<_>, _>>()?,
        );
    }

    // Ollama's /api/chat does NOT accept `tool_choice`. Drop it; warn
    // at debug for Required/Specific so caller knows what happened.
    if matches!(
        req.tool_choice,
        ToolChoice::Required | ToolChoice::Specific { .. }
    ) {
        tracing::debug!(
            target: "ollama_plugin::request",
            tool_choice = ?req.tool_choice,
            "tool_choice unsupported by Ollama /api/chat; ignoring",
        );
    }

    // Sampling overrides → options sub-object with Ollama-specific
    // field names (num_predict, NOT max_tokens).
    let mut options = serde_json::Map::new();
    if let Some(max) = req.max_tokens {
        options.insert("num_predict".into(), serde_json::json!(max));
    }
    if let Some(t) = req.temperature {
        options.insert("temperature".into(), serde_json::json!(t));
    }
    if let Some(p) = req.top_p {
        options.insert("top_p".into(), serde_json::json!(p));
    }
    if let Some(s) = req.seed {
        options.insert("seed".into(), serde_json::json!(s));
    }
    if !req.stop_sequences.is_empty() {
        options.insert(
            "stop".into(),
            serde_json::json!(req.stop_sequences),
        );
    }
    if !options.is_empty() {
        body["options"] = Value::Object(options);
    }

    if !req.provider_specific.is_empty() {
        tracing::debug!(
            target: "ollama_plugin::request",
            keys = ?req.provider_specific.keys().collect::<Vec<_>>(),
            "ignoring provider_specific keys",
        );
    }

    Ok(body)
}

fn translate_messages(req: &CompletionRequest) -> Result<Value, BuildError> {
    let mut out: Vec<Value> = Vec::new();

    // System prompt: Ollama places it as a leading role:system message
    // (NOT a top-level field like Anthropic).
    if let Some(system) = req.system.as_ref() {
        out.push(serde_json::json!({
            "role": "system",
            "content": system,
        }));
    }

    for msg in &req.messages {
        match msg {
            LlmProviderMessage::User { content } => {
                out.push(serde_json::json!({
                    "role": "user",
                    "content": flatten_text(content),
                }));
            }
            LlmProviderMessage::Assistant { content } => {
                let (text, tool_calls) = split_assistant_content(content)?;
                let mut entry = serde_json::json!({
                    "role": "assistant",
                    "content": text,
                });
                if !tool_calls.is_empty() {
                    entry["tool_calls"] = Value::Array(tool_calls);
                }
                out.push(entry);
            }
            LlmProviderMessage::ToolResult {
                tool_use_id: _,
                content,
                is_error: _,
            } => {
                // Ollama's tool message has no tool_use_id field; the
                // kernel pairs results to calls by message order.
                // is_error is also dropped — tools encode errors in
                // the content payload.
                out.push(serde_json::json!({
                    "role": "tool",
                    "content": flatten_text(content),
                }));
            }
            _ => return Err(BuildError::UnknownMessageVariant),
        }
    }
    Ok(Value::Array(out))
}

fn flatten_text(content: &[ContentBlock]) -> String {
    let mut out = String::new();
    for block in content {
        if let ContentBlock::Text(s) = block {
            out.push_str(s);
        }
    }
    out
}

fn split_assistant_content(
    content: &[ContentBlock],
) -> Result<(String, Vec<Value>), BuildError> {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text(s) => text.push_str(s),
            ContentBlock::ToolUse(tu) => {
                tool_calls.push(tool_use_to_call(tu)?);
            }
            _ => return Err(BuildError::UnknownContentBlock),
        }
    }
    Ok((text, tool_calls))
}

fn tool_use_to_call(tu: &ToolUse) -> Result<Value, BuildError> {
    Ok(serde_json::json!({
        "function": {
            "name": tu.name,
            "arguments": serde_json::to_value(&tu.input)?,
        },
    }))
}

fn translate_tool(spec: &ToolSpec) -> Result<Value, BuildError> {
    Ok(serde_json::json!({
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": spec.input_schema,
        },
    }))
}
```

- [ ] **Step 3.5: Run the tests**

Run: `cargo test -p ollama --all-targets`
Expected: PASS — all 10 tests + 9 from Task 2 = 19 tests pass.

- [ ] **Step 3.6: Verify doctests + fmt + clippy + workspace build**

Run: `cargo test -p ollama --doc`
Expected: PASS.

Run: `cargo fmt --all -- --check`
Expected: PASS.

Run: `cargo clippy -p ollama --all-targets --all-features -- -D warnings`
Expected: PASS.

Run: `cargo build --workspace`
Expected: PASS.

- [ ] **Step 3.7: Commit**

```bash
git add crates/tau-plugins/ollama/src/request.rs crates/tau-plugins/ollama/src/lib.rs
git commit -m "feat(ollama): translate CompletionRequest to /api/chat body

build_chat_body translates tau_ports::CompletionRequest to Ollama's
JSON shape (spec §4.2, §7.1):

- System prompt → leading role:system message in messages array
  (NOT top-level field like Anthropic).
- Multi-block User/Assistant content concatenated to flat string
  (Ollama content is String, not array of typed blocks).
- Assistant ToolUse blocks split into tool_calls array; remaining
  text concatenated into content.
- ToolResult message drops tool_use_id and is_error (Ollama pairs
  by ordering; errors live in content payload).
- ToolChoice::None → omit tools entirely.
- ToolChoice::Required and Specific → drop tool_choice with debug
  warn (Ollama /api/chat doesn't accept it).
- Sampling overrides → options sub-object with num_predict
  (NOT max_tokens), temperature, top_p, seed, stop.

10 unit tests cover every translation rule.

Refs: docs/superpowers/specs/2026-04-29-ollama-plugin-design.md §4.2"
```

- [ ] **Step 3.8: Push**

```bash
git push
```

Wait for CI green before Task 4.

---

## Tasks 4-13: hybrid (per-task summary + spec references)

Per the established sub-project 2a pattern: Tasks 4-13 use a hybrid
format — a per-task summary, file list, key code skeleton, test
inventory, and verification commands, with cross-references to the
spec sections that contain the full code.

Each task ends with the same verification protocol:
```
cargo test -p ollama --all-targets
cargo test -p ollama --doc
cargo fmt --all -- --check
cargo clippy -p ollama --all-targets --all-features -- -D warnings
cargo build --workspace
```
And one Conventional Commits commit + push. Wait for CI green between tasks.

---

### Task 4: `response.rs` — `/api/chat` JSON → `CompletionResponse`

**Files:** Create `crates/tau-plugins/ollama/src/response.rs`; add `pub(crate) mod response;` to `lib.rs`.

**Public surface (crate-private):**

```rust
pub(crate) fn parse_chat_response(body: &str) -> Result<tau_ports::CompletionResponse, ParseError>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ParseError {
    #[error("could not decode response JSON: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("tool_call {name} arguments could not decode: {source}")]
    ToolUseInput { name: String, #[source] source: serde_json::Error },
}
```

**Implementation skeleton (full code at spec §4.3):**

```rust
#[derive(serde::Deserialize)]
struct OllamaChatResponse {
    message: OllamaMessage,
    done: bool,
    done_reason: Option<String>,
    prompt_eval_count: Option<u32>,
    eval_count: Option<u32>,
}
#[derive(serde::Deserialize)]
struct OllamaMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OllamaToolCall>>,
}
#[derive(serde::Deserialize)]
struct OllamaToolCall {
    id: Option<String>,
    function: OllamaToolFn,
}
#[derive(serde::Deserialize)]
struct OllamaToolFn {
    name: String,
    arguments: serde_json::Value,
}
```

`parse_chat_response`:
1. `let parsed: OllamaChatResponse = serde_json::from_str(body)?;`
2. `text = parsed.message.content.unwrap_or_default();`
3. Map `tool_calls` (if any), synthesizing `id = tc.id.unwrap_or_else(|| format!("ollama-tool-{i}"))`.
4. Map `done_reason` via `map_done_reason("stop") = EndTurn; "length" = MaxTokens; other = warn + EndTurn`.
5. `usage = match (prompt_eval_count, eval_count) { (Some(i), Some(o)) => Some(TokenUsage::new(i, o)), _ => None };`
6. Return via `tau_ports::fixtures::make_completion_response(text, tool_uses, stop_reason, usage)`.

**Test inventory (5 tests):**
- `parse_text_only_response` — `done: true, done_reason: "stop", message.content: "hello"` → text=hello, no tool_uses, EndTurn, no usage (when counts absent).
- `parse_response_with_tool_call_synthesizes_id` — single tool_call without `id` → `tool_uses[0].id == "ollama-tool-0"`.
- `parse_response_with_two_tool_calls_synthesizes_sequential_ids` — `ollama-tool-0`, `ollama-tool-1`.
- `parse_response_maps_done_reason_length_to_max_tokens` — `done_reason: "length"` → `StopReason::MaxTokens`.
- `parse_response_maps_unknown_done_reason_to_end_turn_with_warn` — `done_reason: "weird"` → `StopReason::EndTurn` (warn check is implicit).
- `parse_response_with_usage_counts` — both counts present → `Some(TokenUsage::new(input, output))`.

(Six tests total; the inventory above lists six.)

**Refs:** Spec §4.3, §7.2, §7.3.

**Commit subject:** `feat(ollama): parse /api/chat batch response`

---

### Task 5: `error.rs` — HTTP status + Ollama error envelope → `LlmError`

**Files:** Create `crates/tau-plugins/ollama/src/error.rs`; add `pub(crate) mod error;` to `lib.rs`.

**Public surface (crate-private):**

```rust
pub(crate) fn map_response_error(
    status: reqwest::StatusCode,
    body: &str,
) -> tau_ports::LlmError;

pub(crate) fn map_client_error(err: crate::client::ClientError) -> tau_ports::LlmError;
```

(`ClientError` is defined in Task 6; this task's `map_client_error` is added in Task 6 as part of `client.rs`. For Task 5, only `map_response_error` lands. The Task 5 commit should include a stub `pub(crate) fn map_client_error` only if needed for compilation — otherwise defer to Task 6.)

**Implementation skeleton (full code at spec §4.4):**

```rust
#[derive(serde::Deserialize)]
struct OllamaErrorBody {
    error: String,
}

pub(crate) fn map_response_error(
    status: reqwest::StatusCode,
    body: &str,
) -> tau_ports::LlmError {
    let detail = serde_json::from_str::<OllamaErrorBody>(body)
        .ok()
        .map(|p| p.error)
        .unwrap_or_else(|| body.to_string());

    let category = match status.as_u16() {
        400 => "bad request",
        401 | 403 => "auth failure",
        404 => "model not found (run `ollama pull <model>` first)",
        429 => "rate limited (retries exhausted)",
        500..=599 => "server error",
        _ => "unexpected status",
    };
    tau_ports::LlmError::Internal {
        message: format!("ollama {category} ({status}): {detail}"),
    }
}
```

**Test inventory (4 tests):**
- `map_404_includes_ollama_pull_remediation_hint` — assert message contains `"ollama pull"`.
- `map_400_with_structured_error_body_extracts_message` — body `{"error":"bad name"}` → message ends with `: bad name`.
- `map_500_unstructured_body_falls_back_to_raw` — body `<html>err</html>` → message ends with `: <html>err</html>`.
- `map_503_categorized_as_server_error` — message contains `"server error"`.

**Refs:** Spec §4.4.

**Commit subject:** `feat(ollama): map HTTP errors to LlmError::Internal`

---

### Task 6: `client.rs` — HTTP client + retry loop + optional bearer auth

**Files:** Create `crates/tau-plugins/ollama/src/client.rs`; add `pub(crate) mod client;` to `lib.rs`. Add `map_client_error` to `error.rs`.

**Public surface (crate-private):**

```rust
pub(crate) struct OllamaClient { /* inner: reqwest::Client, base_url, bearer_token: Option<SecretString>, retry: RetryConfig */ }

impl OllamaClient {
    pub(crate) fn new(
        inner: reqwest::Client,
        base_url: String,
        bearer_token: Option<secrecy::SecretString>,
        retry: crate::config::RetryConfig,
    ) -> Self;

    pub(crate) async fn post_chat(
        &self,
        body: &serde_json::Value,
        stream: bool,
    ) -> Result<reqwest::Response, ClientError>;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ClientError {
    #[error("transport: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("retries exhausted after {attempts} attempts (last status: {status})")]
    Exhausted { status: reqwest::StatusCode, attempts: u32 },
}
```

**Implementation skeleton (full code at spec §4.1):**

- `post_chat`: build URL `{base_url}/api/chat`, add `content-type: application/json`, conditionally add `authorization: Bearer {token}` when `bearer_token.is_some()`, send.
- Classify response → `Decision::{Return(resp), Error(err), Retry { delay_ms, status }}`.
- Retryable statuses: `is_retryable_status(s)` matches 429, 503, OR (5xx and not 501).
- On retry: respect `Retry-After` header when `cfg.retry.respect_retry_after` and parseable as integer seconds; otherwise exponential backoff = `min(60_000, base_delay_ms * 2^(attempt-1))`.
- Retry budget: `attempt >= max_attempts` → `Err(ClientError::Exhausted { status, attempts: attempt })`.
- `tracing::warn!(target: "ollama_plugin::retry", attempt, max, delay_ms, status, "retrying transient error")` per spec.

**Special case — 503 model loading (the load-bearing case):** treated identically to other retryable errors (just exponential backoff). Tests in Task 10 (cassette `complete_503_model_loading_then_success.yaml`) prove the path works end-to-end.

`map_client_error` (added to `error.rs`):

```rust
pub(crate) fn map_client_error(err: crate::client::ClientError) -> tau_ports::LlmError {
    use crate::client::ClientError;
    match err {
        ClientError::Transport(e) => tau_ports::LlmError::Internal {
            message: format!("ollama transport: {e}"),
        },
        ClientError::Exhausted { status, attempts } => tau_ports::LlmError::Internal {
            message: format!(
                "ollama retries exhausted ({attempts} attempts, last status {status})",
            ),
        },
    }
}
```

**Test inventory (5 tests via in-process `tokio::net::TcpListener`):**
- `post_chat_happy_path_no_bearer_token` — 200 response; assert no `authorization` header on the request.
- `post_chat_with_bearer_token_sends_authorization_header` — `bearer_token = Some("xyz")`; assert request had `Authorization: Bearer xyz`.
- `post_chat_503_then_200_succeeds_after_retry` — first attempt 503, second 200; assert 2 attempts, returns Ok. **The load-bearing model-load case.**
- `post_chat_429_with_retry_after_honors_header` — first attempt 429 + `Retry-After: 1`, second 200; measure elapsed >= 1000ms.
- `post_chat_exhausts_after_max_attempts` — all 503; assert `ClientError::Exhausted { attempts == 3 }`.

(Use a small `RetryConfig { max_attempts: 3, base_delay_ms: 10, respect_retry_after: true }` in tests to keep elapsed time short except for the explicit Retry-After test.)

**Refs:** Spec §4.1.

**Commit subject:** `feat(ollama): HTTP client with retry + optional bearer auth`

---

### Task 7: `stream.rs` — NDJSON parser → `CompletionStream`

**Files:** Create `crates/tau-plugins/ollama/src/stream.rs`; add `pub(crate) mod stream;` to `lib.rs`.

**Public surface (crate-private):**

```rust
pub(crate) async fn parse_ndjson(
    body: reqwest::Response,
) -> Result<tau_ports::CompletionStream, tau_ports::LlmError>;
```

**Implementation skeleton (full code at spec §5.2):**

Hand-rolled, ~50 LOC. **No `eventsource-stream` dep.** Uses
`async_stream::try_stream!` over `body.bytes_stream()`, accumulates a
`Vec<u8>` buffer, drains complete lines on each `\n`, parses each line
as a `StreamLine` typed JSON object, yields `CompletionChunk::Text { delta }`
for non-empty content, `CompletionChunk::ToolUse(ToolUse)` for each
entry in `tool_calls`, and `CompletionChunk::Finish { stop_reason, usage }`
when `done == true`. Stream ends without `done:true` line → final
yield is `Err(LlmError::Stream { message: "ollama stream ended before done:true line" })`.

```rust
#[derive(serde::Deserialize)]
struct StreamLine {
    message: Option<StreamMessage>,
    done: bool,
    done_reason: Option<String>,
    prompt_eval_count: Option<u32>,
    eval_count: Option<u32>,
}
#[derive(serde::Deserialize)]
struct StreamMessage {
    content: Option<String>,
    tool_calls: Option<Vec<StreamToolCall>>,
}
#[derive(serde::Deserialize)]
struct StreamToolCall {
    id: Option<String>,
    function: StreamToolFn,
}
#[derive(serde::Deserialize)]
struct StreamToolFn {
    name: String,
    arguments: serde_json::Value,
}
```

Tool-call id synthesis: a `tool_call_index: usize` counter increments
per emitted ToolUse chunk (NOT per stream line); when `tc.id` is None,
synthesize `format!("ollama-tool-{tool_call_index}")`.

Reuse `map_done_reason` from `response.rs` (re-export it as
`pub(crate)` if needed; otherwise duplicate the small match).

**Test inventory (6 tests via hand-fed byte streams):**
- `stream_text_only_yields_chunks_then_finish` — 3 lines (2 content deltas + done) → `Text("Hello")`, `Text(" world")`, `Finish { EndTurn, None }`.
- `stream_with_tool_use_emits_tool_use_chunk_with_synthesized_id` — single tool_calls line → `ToolUse(ToolUse { id: "ollama-tool-0", name: "echo", input: {text:"hi"} })`.
- `stream_two_tool_calls_synthesize_sequential_ids` — line with 2 tool_calls → `ollama-tool-0`, `ollama-tool-1`.
- `stream_truncated_returns_stream_error` — bytes ending without `\n{...,"done":true}` line → final chunk is `Err(LlmError::Stream { message contains "ended before done:true" })`.
- `stream_skips_empty_lines` — input has `\n\n` between valid lines → no spurious chunks.
- `stream_chunked_lines_assembled_across_byte_boundaries` — feed `{"message":{"content":"hello"},` and `"done":false}\n` as separate `Bytes` items → still yields one `Text("hello")`.

**Refs:** Spec §5.

**Commit subject:** `feat(ollama): NDJSON stream parser`

---

### Task 8: `plugin.rs` + `main.rs` — `OllamaPlugin` + `LlmBackend` impl

**Files:** Create `crates/tau-plugins/ollama/src/plugin.rs`; rewrite `crates/tau-plugins/ollama/src/main.rs`; add `pub mod plugin;` to `lib.rs`.

**Public surface:**

```rust
pub struct OllamaPlugin { client: OllamaClient }

impl tau_plugin_sdk::Configure for OllamaPlugin {
    type Config = crate::config::OllamaConfig;
    fn from_config(cfg: Self::Config) -> Result<Self, tau_plugin_sdk::ConfigError>;
}

impl tau_ports::LlmBackend for OllamaPlugin {
    fn name(&self) -> &str { "ollama" }
    async fn complete(&self, req: tau_ports::CompletionRequest)
        -> Result<tau_ports::CompletionResponse, tau_ports::LlmError>;
    async fn stream(&self, req: tau_ports::CompletionRequest)
        -> Result<tau_ports::CompletionStream, tau_ports::LlmError>;
}
```

**Implementation skeleton (full code at spec §6.2, §6.3, §6.4):**

`from_config`:
1. `let bearer_token = resolve_bearer_token(&cfg)?;`
2. `validate_retry(&cfg.retry)?;`
3. Build `reqwest::Client` with `.timeout(cfg.request_timeout())` + `.user_agent(format!("tau-ollama-plugin/{}", env!("CARGO_PKG_VERSION")))`. Map `reqwest::Error` → `ConfigError::InvalidValue { field: "request_timeout", detail: ... }`.
4. `let client = OllamaClient::new(inner, cfg.base_url, bearer_token.map(|t| SecretString::new(t.into())), cfg.retry);`
5. `Ok(OllamaPlugin { client })`.

`complete`:
1. `build_chat_body(&req, false)` (map error → `LlmError::Internal`).
2. `client.post_chat(&body, false).await` (map `ClientError` → `LlmError`).
3. If `!status.is_success()`: read body, return `map_response_error(status, &body)`.
4. Else: read body string, `parse_chat_response(&body)` (map error → `LlmError::Internal`).

`stream`:
1. Same up to `post_chat(&body, true).await`.
2. If `!status.is_success()`: read body, return `map_response_error`.
3. Else: `parse_ndjson(resp).await`.

`main.rs`:

```rust
//! `ollama-plugin` binary. Spawned by tau-runtime::plugin_host as a
//! subprocess; talks MessagePack-RPC over stdio per ADR-0008.
//!
//! Thin shim over [`tau_plugin_sdk::run_llm_backend_with_config`]:
//! the SDK runner drives the handshake, deserializes [`OllamaConfig`]
//! from the handshake `config` field, constructs the plugin via
//! [`OllamaPlugin::from_config`], and runs the dispatch loop.
//!
//! [`OllamaConfig`]: ollama_plugin_lib::config::OllamaConfig
//! [`OllamaPlugin::from_config`]: ollama_plugin_lib::plugin::OllamaPlugin

use ollama_plugin_lib::plugin::OllamaPlugin;
use tau_plugin_sdk::{run_llm_backend_with_config, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_llm_backend_with_config::<OllamaPlugin>(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    )
    .await
}
```

**Test inventory (3 unit tests in `plugin.rs`):**
- `from_config_default_succeeds_with_no_bearer_token` — default config builds, plugin name `"ollama"`.
- `from_config_invalid_retry_max_attempts_zero_returns_invalid_value` — `retry.max_attempts = 0` → `ConfigError::InvalidValue`.
- `name_returns_ollama` — trivial.

(End-to-end testing of `complete` and `stream` happens in Tasks 10-11 via cassette replay; this task's unit tests just exercise the `from_config` glue.)

**Cleanup:** Once `plugin.rs` is in place, remove any `#![allow(dead_code)]` attributes that may have been added to earlier modules to suppress warnings during scaffolding. Verify with `cargo clippy -p ollama --all-targets --all-features -- -D warnings`.

**Refs:** Spec §6.2, §6.3, §6.4.

**Commit subject:** `feat(ollama): OllamaPlugin LlmBackend impl + main entrypoint`

---

### Task 9: Cassette replayer + common helpers (DUPLICATED from Anthropic)

**Files:** Create `crates/tau-plugins/ollama/tests/common/{mod.rs, cassette.rs}`.

**Strategy:** Per spec §2 decision #2 and §8.3 — verbatim copy of
`crates/tau-plugins/anthropic/tests/common/cassette.rs` (~250 LOC).
Adapt only the helper(s) in `mod.rs` that build the plugin's `Config`
struct and start the replayer with a base URL fed back into
`OllamaConfig`.

**Concrete steps:**

1. `cp crates/tau-plugins/anthropic/tests/common/cassette.rs crates/tau-plugins/ollama/tests/common/cassette.rs` — verbatim. The cassette format and the in-process TCP server are wire-format-agnostic.
2. `cp crates/tau-plugins/anthropic/tests/common/mod.rs crates/tau-plugins/ollama/tests/common/mod.rs`.
3. Edit `mod.rs` to:
   - Replace any `AnthropicConfig` references with `OllamaConfig`.
   - Replace any `api_key` / `api_key_env` references with `bearer_token` (and recall: Ollama auth is **optional** — the helper should default to no bearer token unless a test opts in).
   - The "build the plugin and point it at the replayer's base_url" helper should produce an `OllamaPlugin`.
4. Verify both files compile with `cargo build -p ollama --tests`.

**Verification additions for this task:**
- `cargo test -p ollama --tests --no-run` (compiles tests without running).
- Run `cargo clippy -p ollama --tests --all-features -- -D warnings`.

(No new unit tests in this task; tests come in Tasks 10-12. The
replayer itself is exercised by those.)

**Refs:** Spec §8.3. Source: `crates/tau-plugins/anthropic/tests/common/{mod.rs,cassette.rs}` at the latest commit on `main`.

**Commit subject:** `test(ollama): duplicate cassette replayer from anthropic plugin`

---

### Task 10: 6 batch cassettes + `tests/complete.rs`

**Files:**
- Create: `crates/tau-plugins/ollama/tests/cassettes/complete_happy_path.yaml`
- Create: `crates/tau-plugins/ollama/tests/cassettes/complete_with_system_prompt.yaml`
- Create: `crates/tau-plugins/ollama/tests/cassettes/complete_with_tools.yaml`
- Create: `crates/tau-plugins/ollama/tests/cassettes/complete_503_model_loading_then_success.yaml`
- Create: `crates/tau-plugins/ollama/tests/cassettes/complete_404_model_not_pulled.yaml`
- Create: `crates/tau-plugins/ollama/tests/cassettes/complete_400_bad_request.yaml`
- Create: `crates/tau-plugins/ollama/tests/complete.rs`

**Cassette format reference (spec §8.2):**

```yaml
- request:
    method: POST
    uri: /api/chat
  response:
    status: 200
    headers:
      content-type: application/json
    body: |
      {"model":"llama3.2","created_at":"...","message":{"role":"assistant","content":"hi"},"done":true,"done_reason":"stop","prompt_eval_count":10,"eval_count":2}
```

For multi-step retry cassettes (e.g. `503_model_loading_then_success`),
emit multiple `- request: ... response: ...` entries; the replayer
serves them in order to successive requests.

**Test inventory (6 tests in `complete.rs`):**

```rust
#[tokio::test]
async fn complete_happy_path() { /* basic 200 + text response */ }

#[tokio::test]
async fn complete_with_system_prompt() {
    // Verify request body contained {"role":"system","content":"..."}
    // as messages[0]. Use server.received_requests() per cassette
    // helper.
}

#[tokio::test]
async fn complete_with_tools_synthesizes_tool_use_id() {
    // Cassette returns a tool_call without id; assert
    // resp.tool_uses[0].id == "ollama-tool-0".
}

#[tokio::test]
async fn complete_503_model_loading_then_success_retries() {
    // Cassette: 503, 503, 200. Assert 3 attempts, returns Ok with
    // expected text. THIS IS THE LOAD-BEARING OLLAMA RETRY CASE.
}

#[tokio::test]
async fn complete_404_model_not_pulled_includes_remediation_hint() {
    // Cassette: 404 with body {"error":"model 'x' not found"}.
    // Assert returned LlmError::Internal { message contains
    // "ollama pull" }.
}

#[tokio::test]
async fn complete_400_bad_request_does_not_retry() {
    // Cassette: 400 with structured error body. Assert exactly 1
    // attempt (server.received_requests().len() == 1) and returned
    // LlmError::Internal contains "bad request".
}
```

**Refs:** Spec §8.1, §8.4.

**Commit subject:** `test(ollama): batch cassettes + integration tests`

---

### Task 11: 3 streaming cassettes + `tests/streaming.rs`

**Files:**
- Create: `crates/tau-plugins/ollama/tests/cassettes/stream_text_only.yaml`
- Create: `crates/tau-plugins/ollama/tests/cassettes/stream_with_tool_use.yaml`
- Create: `crates/tau-plugins/ollama/tests/cassettes/stream_truncated_response.yaml`
- Create: `crates/tau-plugins/ollama/tests/streaming.rs`

**NDJSON cassette format (spec §8.2):**

YAML's `|` block-literal preserves trailing `\n` per the spec; for
NDJSON each line ends with `\n`, so use `body: |` (literal-keep-newline-at-end). Verify line-count and trailing `\n` after writing
each cassette by `wc -l <file>` and `xxd <file> | tail -3`.

```yaml
- request:
    method: POST
    uri: /api/chat
  response:
    status: 200
    headers:
      content-type: application/x-ndjson
    body: |
      {"model":"llama3.2","created_at":"...","message":{"role":"assistant","content":"Hello"},"done":false}
      {"model":"llama3.2","created_at":"...","message":{"role":"assistant","content":" world"},"done":false}
      {"model":"llama3.2","created_at":"...","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":10,"eval_count":3}
```

For `stream_truncated_response.yaml`: omit the trailing
`done:true` line — body ends after the last delta with no terminator.
This exercises the `LlmError::Stream { message: "...ended before done:true..." }`
path.

**Test inventory (3 tests in `streaming.rs`):**

```rust
#[tokio::test]
async fn stream_text_only_yields_text_chunks_then_finish() {
    // Drain CompletionStream; assert sequence:
    // [Text { delta: "Hello" }, Text { delta: " world" },
    //  Finish { stop_reason: EndTurn, usage: Some(_) }].
}

#[tokio::test]
async fn stream_with_tool_use_emits_full_tool_use_chunk() {
    // Assert chunks include
    // ToolUse(ToolUse { id: "ollama-tool-0", name: "echo",
    //                   input: Object {text: "hi"} })
    // before the Finish.
}

#[tokio::test]
async fn stream_truncated_response_yields_stream_error_at_end() {
    // Drain stream; last yielded item is Err(LlmError::Stream {
    //   message contains "ended before done:true"
    // }).
}
```

**Refs:** Spec §8.1, §8.5.

**Commit subject:** `test(ollama): streaming cassettes + integration tests`

---

### Task 12: Live smoke tests + re-record helper

**Files:**
- Create: `crates/tau-plugins/ollama/tests/live.rs`
- Create: `scripts/rerecord-ollama-cassettes.sh` (executable: `chmod +x`)

**`live.rs` skeleton (spec §8.6):**

```rust
//! Live smoke tests. Always #[ignore]'d. Maintainer-run only.
//!
//! Setup:
//!   brew install ollama
//!   ollama serve &
//!   ollama pull llama3.2
//!
//! Run:
//!   TAU_OLLAMA_LIVE_TESTS=1 cargo test -p ollama --test live -- \
//!     --ignored --nocapture

use ollama_plugin_lib::{config::OllamaConfig, plugin::OllamaPlugin};
use tau_plugin_sdk::Configure;
use tau_ports::LlmBackend;

fn live_enabled() -> bool {
    std::env::var("TAU_OLLAMA_LIVE_TESTS").is_ok()
}
fn live_model() -> String {
    std::env::var("TAU_OLLAMA_LIVE_MODEL").unwrap_or_else(|_| "llama3.2".into())
}

#[tokio::test]
#[ignore = "live: requires TAU_OLLAMA_LIVE_TESTS=1 and a running Ollama instance"]
async fn live_complete_smoke() {
    if !live_enabled() { return; }
    let plugin = OllamaPlugin::from_config(OllamaConfig::default()).unwrap();
    // build a minimal CompletionRequest; assert resp.text non-empty.
}

#[tokio::test]
#[ignore = "live: requires TAU_OLLAMA_LIVE_TESTS=1 and a running Ollama instance"]
async fn live_stream_smoke() {
    if !live_enabled() { return; }
    // exercise plugin.stream(req); drain stream; assert at least one
    // Text chunk and a final Finish.
}
```

**`scripts/rerecord-ollama-cassettes.sh` (spec §8.7):**

```bash
#!/usr/bin/env bash
# Cassette re-record helper for the Ollama plugin.
#
# v0.1: cassettes are hand-authored. The live test suite is the
# drift-detection mechanism. This script informs the operator how
# to run the live tests against a real Ollama instance.

set -euo pipefail

if ! command -v ollama >/dev/null 2>&1; then
    echo "ollama CLI not found. Install via 'brew install ollama' or" \
         "https://ollama.com/download." >&2
    exit 1
fi

MODEL="${TAU_OLLAMA_LIVE_MODEL:-llama3.2}"

echo "Pulling model: $MODEL"
ollama pull "$MODEL"

echo "Running Ollama plugin live smoke tests..."
TAU_OLLAMA_LIVE_TESTS=1 \
TAU_OLLAMA_LIVE_MODEL="$MODEL" \
    cargo test -p ollama --test live -- --ignored --nocapture

echo
echo "v0.1 note: cassettes under crates/tau-plugins/ollama/tests/cassettes/"
echo "are hand-authored; the live tests above are the drift-detection"
echo "mechanism. If responses change shape, update the cassette files"
echo "directly to match real Ollama output."
```

**Verification additions for this task:**

- `cargo test -p ollama --test live` (no `--ignored`) → 0 tests run.
- `bash scripts/rerecord-ollama-cassettes.sh` is NOT run as part of normal verification (requires live Ollama).

**Refs:** Spec §8.6, §8.7.

**Commit subject:** `test(ollama): live smoke tests + re-record helper`

---

### Task 13: CI — 1 new build job

**Files:** Modify `.github/workflows/ci.yml`.

**Add a new job after `build-anthropic-plugin`:**

```yaml
  build-ollama-plugin:
    name: build (ollama-plugin)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Build ollama-plugin (release)
        run: cargo build --release -p ollama
```

(Integration tests for the Ollama plugin run in the existing
`test (ubuntu-latest / stable)` job via `cargo test --workspace --all-targets`. This new job exists to ensure the release-mode build
stays green in case integration tests skip when the target binary
is needed at runtime.)

**Verification:**
- After commit + push: confirm the new job appears in PR CI runs.
- Job name `build (ollama-plugin)` must match exactly — sub-project
  sign-off (Task 15) will reference it when updating branch protection.

**Refs:** Spec §10 row 13.

**Commit subject:** `ci(ollama): add build (ollama-plugin) release job`

---

## Tasks 14-15: user-driven gates

These are checkpoints, not implementation steps. The runner subagent
should pause at Task 14 and surface to the user; the user signs off.

---

### Task 14: Final local verification + mark PR ready

- [ ] **Step 14.1: Run the full local verification matrix**

```bash
cargo build --workspace
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

All must pass.

- [ ] **Step 14.2: Verify PR is up-to-date with `main`**

```bash
git fetch origin
git log --oneline origin/main..HEAD
git status
```

If main has advanced, rebase; otherwise proceed.

- [ ] **Step 14.3: Verify the new CI job is green on PR**

```bash
gh pr checks
```

Confirm `build (ollama-plugin)` is among the green checks alongside
the pre-existing 16 required checks.

- [ ] **Step 14.4: Mark the PR ready**

```bash
gh pr ready
```

(If the PR was opened directly as ready, this is a no-op.)

- [ ] **Step 14.5: Wait for user sign-off before Task 15.**

Surface to the user:
> "Sub-project 2b implementation complete; all 13 work tasks shipped, all CI checks green on PR. Awaiting your sign-off to (a) update ROADMAP, (b) update branch protection (16→17 required checks), and (c) squash-merge."

---

### Task 15: Plan sign-off + ROADMAP + branch protection update + squash merge

User-driven gate. The subagent runner should NOT perform these steps
without explicit user instruction (they affect main, branch
protection, and merge state).

- [ ] **Step 15.1: Update ROADMAP.md**

Edit `ROADMAP.md` to mark Phase 1 sub-project 2b complete. Add a new
row to the Phase 1 table (after the 2a row):

```markdown
| 2b | Ollama LLM-backend plugin ✅ | Second real LLM-backend plugin: Ollama (local LLM runner) at `crates/tau-plugins/ollama/`; native `/api/chat` over NDJSON streaming; optional bearer-token auth; cassette-replay test harness duplicated from Anthropic; in-plugin retry honoring 503-on-model-load case | <DATE-OF-MERGE> |
```

Update the **Status** line under "Current phase: 1" to reflect that
sub-project 2c (OpenAI) is the natural next sub-project.

Update **Tier 1, item 2** in the Phase 1 priorities section: change
"Ollama (priority 2b) and OpenAI (priority 2c) follow as their own
sub-projects" to indicate Ollama shipped, OpenAI remains. Bump the
required-CI-checks count: "17 required CI checks gating `main` (was 16)."

- [ ] **Step 15.2: Commit the ROADMAP update**

```bash
git add ROADMAP.md
git commit -m "docs(roadmap): mark Phase 1 sub-project 2b (Ollama plugin) complete

Second real LLM-backend plugin shipped. Local-first / no-cost demo
path validated. Native /api/chat endpoint, NDJSON streaming,
optional bearer-token auth, cassette-replay testing.

17 required CI checks gating main (was 16). Sub-project 2c
(OpenAI) is the natural next sub-project; pairs with conformance
suite design at three-implementation milestone.

Refs: docs/superpowers/specs/2026-04-29-ollama-plugin-design.md"
git push
```

- [ ] **Step 15.3: Update branch protection — add `build (ollama-plugin)` check**

```bash
# Read current required checks:
gh api repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks \
    --jq '.contexts'

# Build the new list (existing 16 + "build (ollama-plugin)"):
# Update the contexts array in a PUT call. Use the GitHub web UI as
# fallback if the API path is fiddly.

gh api -X PATCH repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks \
    -f 'contexts[]=...the existing 16 entries...' \
    -f 'contexts[]=build (ollama-plugin)'
```

(Exact API call depends on whether `required_status_checks` uses
`contexts` or `checks` shape. Confirm via `gh api` GET first.
Established pattern from sub-project 2a Task 16.)

- [ ] **Step 15.4: Squash-merge the PR**

```bash
gh pr merge --squash --delete-branch
```

- [ ] **Step 15.5: Verify post-merge state**

```bash
git fetch origin
git checkout main
git pull
git log --oneline -5
gh api repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks --jq '.contexts | length'  # expect 17
```

Sub-project 2b complete. The natural next sub-project is 2c (OpenAI
LLM-backend plugin), at which point the rule-of-three trigger
justifies extracting a `tau-plugin-test-support` crate (cassette
replayer + retry client + ClientError) per spec §11 and §13.

---

## Self-review notes (for the plan author)

**Spec coverage check:**

| Spec section | Covered by task |
|---|---|
| §1 Summary, §1.1 Scope confirmed | Task 1 (scaffold), entire plan |
| §1.2 Constitution alignment | All tasks (forbid unsafe_code, deny missing_docs, clippy -D, thiserror, doctests) |
| §2 Decisions table (17 rows) | Encoded in implementation: §2.1 endpoint (Tasks 3, 6, 8), §2.2 duplicate code (Task 9), §2.3 distribution (Task 1), §2.4-5 base url + bearer (Task 2), §2.6 timeout (Task 2/8), §2.7 retry (Tasks 2, 6), §2.8 model selection (Task 3), §2.9-10 tool-use (Tasks 3, 4, 7), §2.11 tool_choice drop (Task 3), §2.13 token usage (Tasks 4, 7), §2.14 error fidelity (Task 5), §2.15 NDJSON (Task 7), §2.16 truncated stream (Task 7), §2.17 testing (Tasks 9-12) |
| §3 Architecture / workspace layout | Task 1 |
| §4.1 client.rs | Task 6 |
| §4.2 request.rs | Task 3 |
| §4.3 response.rs | Task 4 |
| §4.4 error.rs | Task 5 |
| §5 Streaming | Task 7 |
| §6.1 OllamaConfig + RetryConfig | Task 2 |
| §6.2 Configure impl | Task 8 (with config validation in Task 2) |
| §6.3 plugin.rs | Task 8 |
| §6.4 main.rs | Task 8 |
| §6.5 tau.toml | Task 1 |
| §6.6 Project tau.toml usage examples | Documentation in spec; no implementation needed |
| §7 Tool-use mapping | Tasks 3 (request), 4 (response), 7 (streaming) |
| §8.1 Cassette catalog | Tasks 10-11 |
| §8.2 Cassette format | Tasks 10-11 |
| §8.3 Cassette replayer | Task 9 |
| §8.4 tests/complete.rs | Task 10 |
| §8.5 tests/streaming.rs | Task 11 |
| §8.6 Live smoke tests | Task 12 |
| §8.7 Re-record helper | Task 12 |
| §8.8 Test surface summary | Aggregate across Tasks 2-12 |
| §9 Plan-erratum carryovers | Plan-erratum block at top |
| §9.1 ADR not required | Documented; no task |
| §10 Implementation plan outline | This plan IS the expansion |
| §11 Out of scope | No tasks (intentional non-goals) |
| §12 Cross-references | Documented in plan header |
| §13 Open follow-ups | Documented in Task 15 (sub-project 2c queued) |

**No spec gaps found.**

**Placeholder scan:** No `TBD`, `TODO`, `implement later`, `Add appropriate error handling`, or `Similar to Task N` patterns in this plan. All code blocks are concrete; tests are specified by name and assertion content.

**Type consistency check:**
- `OllamaConfig` / `RetryConfig` — defined in Task 2, used in Tasks 6, 8.
- `OllamaClient::new(inner, base_url, bearer_token, retry)` — defined in Task 6, used in Task 8.
- `OllamaPlugin { client }` — defined in Task 8.
- `parse_chat_response(body) -> Result<CompletionResponse, ParseError>` — defined in Task 4, used in Task 8.
- `parse_ndjson(body) -> Result<CompletionStream, LlmError>` — defined in Task 7, used in Task 8.
- `map_response_error(status, body) -> LlmError` — defined in Task 5, used in Task 8.
- `map_client_error(err) -> LlmError` — added in Task 6 (or as a Task 5 stub if needed for compilation).
- `build_chat_body(req, stream) -> Result<Value, BuildError>` — defined in Task 3, used in Task 8.
- `resolve_bearer_token(cfg) -> Result<Option<String>, ConfigError>` — defined in Task 2, used in Task 8.
- `validate_retry(retry) -> Result<(), ConfigError>` — defined in Task 2, used in Task 8.
- `ClientError::{Transport, Exhausted}` — defined in Task 6, mapped in Task 6's `map_client_error`.
- `BuildError::{UnknownMessageVariant, UnknownContentBlock, JsonSerialize}` — defined in Task 3, mapped in Task 8's `complete`/`stream` (`map_err(|e| LlmError::Internal { message: format!("ollama: build request body: {e}") })`).
- `ParseError::{Decode, ToolUseInput}` — defined in Task 4, mapped in Task 8.
- Synthesized tool-call ids: `"ollama-tool-{n}"` consistent across Tasks 4 (batch index per response) and 7 (`tool_call_index` counter per stream).

**No type-consistency drift found.**

---

## Plan complete and saved to `docs/superpowers/plans/2026-04-29-ollama-plugin.md`.

Two execution options:

**1. Subagent-Driven (recommended)** — fresh subagent per task, two-stage review (spec compliance + code quality) between tasks, fast iteration on the existing `feat/ollama-plugin-spec` branch.

**2. Inline Execution** — execute tasks in this session using `superpowers:executing-plans`, batch execution with checkpoints.

Which approach?
