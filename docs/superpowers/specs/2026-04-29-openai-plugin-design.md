# OpenAI LLM-backend plugin + supporting infrastructure — Phase 1 sub-project 2c

**Status:** Draft (this spec) → Implementation plan derived → ADR-0009 (LlmError migration policy) accompanies the implementation merge.

**Sub-project scope:** Phase 1 priority 2c. Closes out Tier 1 priority 2 (first real LLM-backend plugins). Three real LLM-backend plugins now exist; this sub-project bundles four deliverables that the rule-of-three trigger justifies in one sub-project:

1. **OpenAI plugin** at `crates/tau-plugins/openai/` (the third real LLM-backend plugin).
2. **Shared test-support crate** at `crates/tau-plugin-test-support/` (rule-of-three refactor of cassette replayer + ClientError + helpers, currently duplicated across anthropic + ollama).
3. **Conformance test suite** at `crates/tau-plugin-conformance/` (parameterized behavioral suite, deferred from ADR-0008 §17 until ≥2 real implementations existed).
4. **Error vocabulary migration** of all three plugins from blanket `LlmError::Internal` to the existing typed variants (`RateLimited`, `Auth`, `InvalidRequest`, `Provider`, `Transport`).

---

## 1. Summary

Sub-project 2a shipped Anthropic; 2b shipped Ollama. Both currently
collapse all non-2xx responses to `LlmError::Internal { message }`,
even though `tau_ports::LlmError` has typed variants for the common
cases (`RateLimited { retry_after_seconds }`, `Auth { message }`,
`InvalidRequest { reason }`, `Provider { message }`,
`Transport { message }`). This was acceptable at N=1 and N=2 (small
caller surface, deferred per spec §1.1 of both prior sub-projects).
At N=3 the value of typed variants is concrete:

- A caller can `match err { RateLimited { retry_after_seconds } => sleep(...); ... }` — today the caller must string-grep `Internal.message`.
- `Runtime::run` (and tools that drive an LLM call) can use `LlmError::is_retryable()` for honest retry decisions; today every variant including bad-request errors looks like `Internal`, which `is_retryable()` correctly returns `false` for — meaning we silently lose the retry-eligibility signal.
- Error envelope shapes differ across providers (Anthropic typed, OpenAI typed, Ollama bare-string); a uniform mapping is what users expect.

The OpenAI plugin lands as the fresh greenfield consumer of the new
infrastructure; anthropic + ollama migrate to it during the same
sub-project so the shared crates are validated against three
implementations.

### 1.1 Scope confirmed

**Ships:**

- One new workspace member: `crates/tau-plugins/openai/`. Targets
  `POST /v1/chat/completions` (de-facto-standard endpoint, supported
  by every OpenAI-compat provider), SSE streaming via
  `eventsource-stream` (reuses the workspace dep from sub-project 2a),
  required `Authorization: Bearer {OPENAI_API_KEY}` auth, real
  `tool_call_id` round-trip (NOT positional like Ollama), full
  `tool_choice` round-trip (Auto / None / Required / Specific), retry
  on 429/5xx with `Retry-After` honoring.
- One new workspace member: `crates/tau-plugin-test-support/` (lib
  crate, dev-only role). Lifts `tests/common/cassette.rs` (~323 LOC
  duplicated verbatim across anthropic + ollama) plus generic test
  helpers (`drain_stream`, byte-level fixtures). Each existing plugin's
  `tests/common/cassette.rs` is deleted; tests import from the shared
  crate. The provider-specific `test_config(base_url)` builder STAYS
  per-plugin (each has its own Config struct).
- One new workspace member: `crates/tau-plugin-conformance/` (lib
  crate, dev-only role). Parameterized behavioral test suite. Takes a
  `Box<dyn LlmBackend>` plus a path to a directory of cassettes (one
  per behavioral test) and runs the battery against any plugin. Each
  plugin's `tests/conformance.rs` is a 5-15 line shim wiring its
  configured plugin into the suite. Initial catalog: 6 tests covering
  request/response/streaming/tool-use/error-typing.
- Migration of all three plugins (anthropic + ollama + new openai) to
  emit typed `LlmError` variants instead of blanket `Internal`.
- 4 new CI build jobs: `build (openai-plugin)`,
  `build (tau-plugin-test-support)`,
  `build (tau-plugin-conformance)`, plus the new
  `test (conformance)` job that runs the parameterized suite against
  all three plugins. **17 → 21 required CI checks gating `main`.**
- One new ADR: ADR-0009 documenting the typed-error migration policy
  and the conformance-suite charter (governs which behaviors the suite
  is allowed to mandate vs. leave plugin-specific).
- ~9 cassette files for the OpenAI plugin (matches the 6+3 batch+stream split from prior sub-projects).
- ~25 unit tests in the OpenAI plugin + 6 conformance tests + 2 cassette self-tests in `tau-plugin-test-support`.

**Does NOT ship:**

- New `LlmError` variants. Existing variants suffice. (Open question
  flagged for future: `LlmError::ModelNotFound { model: String }` —
  defer; `InvalidRequest { reason }` covers the case adequately for v0.1
  with the remediation hint embedded in the reason string.)
- OpenAI's newer `/v1/responses` API (the chat-completions API is the
  de-facto standard).
- Multi-modal (image inputs via `content` array elements) — deferred
  until `tau_ports::ContentBlock::Image` lands.
- Function-calling-via-name-only (the deprecated `function_call` field) —
  use modern `tool_calls` only.
- `logprobs`, `n>1`, `response_format` (structured outputs), `service_tier`
  — out of scope; could land via `provider_specific` if needed.
- Embeddings, fine-tuning, file-uploads — different ports / different
  sub-projects.
- Live re-record automation. Same v0.1 stance as 2a/2b: cassettes are
  hand-authored; a future sub-project introduces record-mode replayer.

### 1.2 Constitution alignment

| Constraint | This sub-project's answer |
|---|---|
| `forbid(unsafe_code)` | Plain Rust; no FFI, no manual unsafe. |
| **G6** runtime not framework | OpenAI plugin is a thin translation layer; conformance suite is a test-only library. |
| **G9** observable by default | All retries, error mappings, and stream-parser decisions emit `tracing` events under `target = "openai_plugin::*"`. |
| **NG4** no marketplace | All three crates are in-tree for v0.1; standalone-repo migration deferred. |
| **NG7** does not evaluate quality | The conformance suite tests **mechanical correctness** (request shape, error type, etc.), NOT response quality. Renaming this concern was a Q3 design decision: "conformance" not "quality". |
| **NG9** no credential management | OpenAI plugin reads `OPENAI_API_KEY` from env; never persists, never logs the key value. |
| **NG10** no telemetry | Tracing is local; only outbound traffic is direct calls to api.openai.com (and to the cassette replayer in tests). |

---

## 2. Decisions

### 2.1 Settled (by precedent or already specified above)

| # | Decision | Rationale |
|---|---|---|
| 1 | **OpenAI endpoint:** `POST /v1/chat/completions` (NOT `/v1/responses`) | De-facto standard supported by every OpenAI-compat provider; broader ecosystem reach. The `/v1/responses` API is OpenAI-only and adds no capability we need at v0.1. |
| 2 | **Distribution:** in-tree at `crates/tau-plugins/openai/` | Matches Anthropic + Ollama precedent. |
| 3 | **Default base URL:** `https://api.openai.com` | Standard OpenAI base. |
| 4 | **Authentication:** `Authorization: Bearer {OPENAI_API_KEY}` (required). Default env var `OPENAI_API_KEY`. | Matches OpenAI convention; matches Anthropic's required-key pattern (NOT Ollama's optional-token pattern). |
| 5 | **Streaming wire format:** SSE via `eventsource-stream` (reuses workspace dep) | Matches OpenAI's chat-completions specification; reuses the SSE machinery the Anthropic plugin already validated. |
| 6 | **Tool-use:** real `tool_call_id` round-trip (NOT synthesized like Ollama) | OpenAI provides a stable id per tool_call; preserve it through the request/response cycle. |
| 7 | **`tool_choice` field:** full round-trip — `Auto` → `"auto"`, `None` → `"none"`, `Required` → `"required"`, `Specific { name }` → `{"type":"function","function":{"name":"<n>"}}` | OpenAI accepts the field directly; emit it. Distinct from Ollama (which drops it). |
| 8 | **Retry:** same defaults as Anthropic (max_attempts=3, base_delay_ms=1000, respect_retry_after=true). 429 with `Retry-After` honoring + 5xx (≠501) exponential backoff. | OpenAI's most common transient failure is 429 with explicit `Retry-After`. |
| 9 | **Tool-call argument streaming:** accumulator pattern (matches Anthropic). OpenAI streams tool_call arguments as fragmented JSON deltas across multiple SSE events; the parser accumulates them per-id, emits one complete `CompletionChunk::ToolUse(ToolUse)` when arguments are valid JSON. | Same shape as Anthropic's `input_json_delta` accumulator; reuse the pattern. |
| 10 | **Multi-modal:** out-of-scope v0.1. | Defer until `tau_ports::ContentBlock::Image` lands. |

### 2.2 Settled (this sub-project's contested decisions, all resolved Q&A=A)

| # | Decision | Rationale |
|---|---|---|
| 11 | **Refactor extraction (Q1):** Extract `tau-plugin-test-support` aggressively now. Lifts cassette replayer + ClientError shape + generic helpers. | Rule-of-three triggered; 600 LOC of duplication eliminated. |
| 12 | **Error vocabulary (Q2):** Migrate all three plugins to emit existing typed `LlmError` variants. NO new variants needed in `tau-ports` — `RateLimited`, `Auth`, `InvalidRequest`, `Provider`, `Transport` already exist. | The 2a/2b "blanket Internal" mapping was a deferred decision; three implementations exist now to validate the typed shapes. |
| 13 | **Conformance suite (Q3):** Ship `tau-plugin-conformance` in this sub-project (the full bundle, not a scaffold). Initial catalog: 6 behavioral tests. All three plugins integrate. | Three implementations exist; the parameterized harness becomes high-leverage. The full bundle in one sub-project also provides natural integration testing of test-support + conformance + the OpenAI plugin together. |
| 14 | **ADR coverage:** Single new ADR-0009 covering: (a) the typed-error migration policy, (b) the conformance suite's charter (mechanical correctness only, not quality). | Two distinct decisions but both relate to "what we mandate of LLM-backend plugins"; one ADR keeps the policy story coherent. |

### 2.3 Decisions explicitly out of scope

| Topic | Where it lives |
|---|---|
| `LlmError::ModelNotFound` typed variant | Future sub-project; `InvalidRequest { reason }` with remediation in `reason` is sufficient at v0.1. |
| `LlmError` migration of `tau-runtime` consumers | Out of scope: callers will get richer types automatically when they upgrade past this merge. |
| OpenAI `/v1/responses` API | OpenAI-specific; no benefit until tau exposes feature parity. |
| Multi-modal | Deferred until ContentBlock::Image. |
| Embeddings, fine-tuning, batch API | Different ports or different sub-projects. |
| Live cassette re-record automation | Same v0.1 stance as 2a/2b. |
| Conformance tests for tool-use _semantics_ (does the model actually call the tool?) | NG7 says no — conformance tests **shape**, not **quality**. |
| Conformance tests for streaming chunk **ordering invariants** beyond "Text precedes Finish" | YAGNI — add when a real bug motivates. |

---

## 3. Architecture

### 3.1 Workspace layout

```
crates/
├── tau-plugin-test-support/        -- NEW: shared dev-dep crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                  -- crate-level docs; pub modules
│       ├── cassette.rs             -- LIFTED from anthropic/ollama (verbatim) ~323 LOC
│       └── helpers.rs              -- generic helpers: drain_stream, etc.
│
├── tau-plugin-conformance/         -- NEW: parameterized test suite
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                  -- crate-level docs; pub modules
│       ├── suite.rs                -- ConformanceSuite::run(plugin, cassettes_dir)
│       ├── tests/                  -- the 6 individual test functions
│       │   ├── batch_happy_path.rs
│       │   ├── batch_with_tools.rs
│       │   ├── streaming_text.rs
│       │   ├── streaming_tool_use.rs
│       │   ├── error_rate_limited.rs
│       │   └── error_auth.rs
│       └── catalog.rs              -- registers the test catalog
│
├── tau-plugins/openai/             -- NEW: third LLM-backend plugin
│   ├── Cargo.toml
│   ├── tau.toml
│   ├── src/
│   │   ├── main.rs                 -- #[tokio::main] → run_llm_backend_with_config
│   │   ├── lib.rs                  -- pub modules; crate-level docs
│   │   ├── plugin.rs               -- OpenAIPlugin + LlmBackend impl + Configure
│   │   ├── config.rs               -- OpenAIConfig + RetryConfig + resolve_api_key + validate_retry
│   │   ├── client.rs               -- OpenAIClient (reqwest) + post_chat_completions + retry loop
│   │   ├── request.rs              -- CompletionRequest → /v1/chat/completions JSON
│   │   ├── response.rs             -- /v1/chat/completions JSON → CompletionResponse
│   │   ├── stream.rs               -- SSE parser + ToolUseAccumulator → CompletionStream
│   │   └── error.rs                -- HTTP status + OpenAI error envelope → LlmError (TYPED)
│   └── tests/
│       ├── cassettes/              -- 9 cassette YAMLs (6 batch + 3 streaming)
│       ├── conformance.rs          -- shim invoking ConformanceSuite::run
│       ├── complete.rs             -- batch tests via cassette replay (provider-specific assertions)
│       ├── streaming.rs            -- streaming tests
│       └── live.rs                 -- env-gated smoke tests (#[ignore])
│
├── tau-plugins/anthropic/          -- MIGRATED
│   ├── src/error.rs                -- map_response_error TYPED (was always Internal)
│   ├── tests/common/cassette.rs    -- DELETED (now imports tau_plugin_test_support::cassette)
│   ├── tests/common/mod.rs         -- adapted to import shared helpers
│   └── tests/conformance.rs        -- NEW: shim invoking ConformanceSuite::run
│
└── tau-plugins/ollama/             -- MIGRATED
    ├── src/error.rs                -- map_response_error TYPED (was always Internal)
    ├── tests/common/cassette.rs    -- DELETED
    ├── tests/common/mod.rs         -- adapted
    └── tests/conformance.rs        -- NEW: shim invoking ConformanceSuite::run

.github/workflows/ci.yml            -- + 4 new jobs:
                                       build (openai-plugin)
                                       build (tau-plugin-test-support)
                                       build (tau-plugin-conformance)
                                       test (conformance)            <-- runs the parameterized suite

docs/decisions/0009-llm-error-typing-and-conformance.md   -- NEW ADR
ROADMAP.md                                                -- 2c row added
Cargo.toml                                                -- + 3 workspace members + workspace dep entries
```

### 3.2 Dependencies (workspace adds)

**One new workspace dep:** `wiremock = "0.6"` for the conformance suite's cassette parsing helpers. (Optional — if we extend the hand-rolled replayer instead, no new workspace dep is needed. Decided at impl time after a 5-min survey of the cassette-format helpers we've already hand-rolled.)

**No other new workspace deps.** OpenAI plugin reuses everything that
sub-projects 2a + 2b already added (`reqwest`, `secrecy`,
`async-stream`, `eventsource-stream`).

### 3.3 Dataflow

```
tau-cli (existing)
  └─ tau-runtime::plugin_host::load_llm_backend
      └─ spawns target/release/openai-plugin
          └─ tau-plugin-sdk::run_llm_backend_with_config::<OpenAIPlugin>
              ├─ handshake (config: { api_key, retry: {...} })
              ├─ Configure::from_config → OpenAIPlugin { client: OpenAIClient }
              ├─ dispatch loop:
              │   ├─ llm.complete → OpenAIPlugin::complete
              │   │   ├─ request::build_chat_completions_body(req, stream=false)
              │   │   ├─ client::post_chat_completions(&body, stream=false)
              │   │   │   ├─ retry on 429/5xx (≠501)
              │   │   │   └─ honor Retry-After
              │   │   ├─ response::parse_chat_completions_response(body) → CompletionResponse
              │   │   └─ map error → TYPED LlmError variant if non-2xx
              │   └─ llm.stream → OpenAIPlugin::stream
              │       ├─ request::build_chat_completions_body(req, stream=true)
              │       ├─ client::post_chat_completions(&body, stream=true)
              │       └─ stream::parse_sse(response) → CompletionStream
              │           (Text deltas; ToolUseAccumulator across
              │            input_json_delta-equivalents; Finish on
              │            [DONE] sentinel)
              └─ frames out via stdout
```

---

## 4. OpenAI plugin: HTTP layer

### 4.1 `client.rs` — HTTP client with retry

Same shape as the anthropic + ollama clients. Differences:

```rust
pub(crate) struct OpenAIClient {
    inner: reqwest::Client,
    base_url: String,                    // default https://api.openai.com
    api_key: SecretString,
    retry: RetryConfig,
}

impl OpenAIClient {
    pub(crate) async fn post_chat_completions(
        &self,
        body: &serde_json::Value,
        stream: bool,
    ) -> Result<reqwest::Response, ClientError> {
        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));
        // Retry loop identical in shape to ollama/anthropic.
        // Headers:
        //   authorization: Bearer {api_key}
        //   content-type: application/json
        //   accept: text/event-stream  // when stream == true
    }
}
```

`ClientError` is now imported from `tau_plugin_test_support::ClientError` (lifted in this sub-project).

### 4.2 `request.rs` — `CompletionRequest` → `/v1/chat/completions` body

OpenAI's request shape:

```json
{
  "model": "gpt-4o-mini",
  "messages": [
    {"role": "system", "content": "you are concise"},
    {"role": "user", "content": "say hi"},
    {"role": "assistant", "content": null, "tool_calls": [{"id":"call_abc","type":"function","function":{"name":"echo","arguments":"{\"text\":\"hi\"}"}}]},
    {"role": "tool", "tool_call_id": "call_abc", "content": "echoed: hi"}
  ],
  "tools": [{"type":"function","function":{"name":"echo","description":"...","parameters":{...}}}],
  "tool_choice": "auto",
  "stream": true,
  "max_tokens": 100,
  "temperature": 0.7,
  "top_p": 0.9,
  "seed": 42,
  "stop": ["END"]
}
```

Translation rules:
- `req.system: Option<String>` → leading `{role:"system",content:<system>}` message (matches Ollama; OpenAI does NOT have a top-level `system` field).
- Multi-block `User` content: if all blocks are `Text`, concatenate to flat string (OpenAI accepts `content: String` OR `content: Array`); v0.1 emits flat string. Multi-modal (when blocks include `Image`) routes through the array shape — out-of-scope here.
- `Assistant` content with `ContentBlock::Text` → `content: String`. With `ContentBlock::ToolUse(tu)` → `tool_calls: [{"id": tu.id, "type":"function","function":{"name": tu.name, "arguments": <stringified JSON>}}]`. **Note: `arguments` is a JSON-encoded string** in OpenAI's wire format, NOT a JSON object.
- `ToolResult { tool_use_id, content, is_error }` → `{role:"tool",tool_call_id:<tool_use_id>,content:<flattened text>}`. **`tool_use_id` round-trips** (distinct from Ollama). `is_error` is dropped — OpenAI's tool message has no error flag (errors live in content).
- `tool_choice` mapping: `Auto` → `"auto"`, `None` → `"none"`, `Required` → `"required"`, `Specific { name }` → `{"type":"function","function":{"name":<name>}}`.
- Sampling overrides: `max_tokens`, `temperature`, `top_p`, `seed`, `stop` (NOT `stop_sequences`) at the body's top level. OpenAI uses `max_tokens` (not Ollama's `num_predict`).

### 4.3 `response.rs` — batch JSON → `CompletionResponse`

OpenAI's batch response:

```json
{
  "id": "chatcmpl-abc",
  "object": "chat.completion",
  "created": 1731000000,
  "model": "gpt-4o-mini",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "Hi there",
      "tool_calls": [{"id":"call_abc","type":"function","function":{"name":"echo","arguments":"{\"text\":\"hi\"}"}}]
    },
    "finish_reason": "stop"
  }],
  "usage": {"prompt_tokens": 12, "completion_tokens": 3, "total_tokens": 15}
}
```

Translation:
- `text` = `choices[0].message.content.unwrap_or_default()`.
- `tool_uses` = each `choices[0].message.tool_calls[i]`:
  - `id` = `tc.id` (always present from OpenAI; if Some(id) is None defensively, synthesize `"openai-tool-{i}"`).
  - `name` = `tc.function.name`.
  - `input` = `serde_json::from_str::<tau_domain::Value>(&tc.function.arguments)?` (parse the JSON-encoded string).
- `stop_reason`: map `finish_reason`:
  - `"stop"` → `EndTurn`
  - `"length"` → `MaxTokens`
  - `"tool_calls"` → `ToolUse`
  - `"content_filter"` → `Error` (with warn)
  - `"function_call"` (deprecated) → `ToolUse` (warn that legacy field is in use)
  - other → `EndTurn` with warn.
- `usage` = `Some(TokenUsage::new(prompt_tokens, completion_tokens))` always (OpenAI always returns usage).

If `choices` is empty or has length > 1, return `LlmError::Provider { message: "unexpected choices count: ..." }` — v0.1 only handles `n=1`.

### 4.4 `error.rs` — HTTP status + OpenAI error envelope → TYPED `LlmError`

**THIS IS THE MIGRATION PATTERN that anthropic + ollama also adopt.**

OpenAI's error envelope:

```json
{"error": {"message": "...", "type": "invalid_request_error", "param": "model", "code": "model_not_found"}}
```

```rust
pub(crate) fn map_response_error(status: reqwest::StatusCode, body: &str) -> LlmError {
    let detail = serde_json::from_str::<OpenAIErrorBody>(body)
        .ok()
        .map(|p| p.error)
        .unwrap_or_else(|| OpenAIErrorDetail {
            message: body.to_string(),
            error_type: None,
            code: None,
        });

    match status.as_u16() {
        400 => LlmError::InvalidRequest {
            reason: format_invalid_request(&detail),
        },
        401 | 403 => LlmError::Auth {
            message: detail.message,
        },
        404 => LlmError::InvalidRequest {
            // OpenAI 404 → typically model_not_found. Embed remediation
            // in `reason` (typed ModelNotFound variant deferred per
            // spec §2.3).
            reason: format!("model not found: {}", detail.message),
        },
        429 => LlmError::RateLimited {
            retry_after_seconds: parse_retry_after_seconds_from_body(&detail),
        },
        500..=599 => LlmError::Provider {
            message: format!("openai server error ({status}): {}", detail.message),
        },
        _ => LlmError::Provider {
            message: format!("openai unexpected status ({status}): {}", detail.message),
        },
    }
}
```

`map_client_error` (transport layer):
- `ClientError::Transport(e)` → `LlmError::Transport { message: e.to_string() }`.
- `ClientError::Exhausted { status, attempts }` → typed by status:
  - 429 → `LlmError::RateLimited { retry_after_seconds: None }` (already exhausted).
  - 5xx → `LlmError::Provider { message: ... }`.
  - 408 (synthesized timeout) → `LlmError::Transport { message: ... }`.

### 4.5 Migration: anthropic + ollama error mappers

Anthropic's current `map_response_error` returns `LlmError::Internal` for everything. Migrate to:
- 400 → `InvalidRequest { reason }` (uses Anthropic's `error.type + error.message`).
- 401/403 → `Auth { message }`.
- 404 → `InvalidRequest { reason }` (rare).
- 429 → `RateLimited { retry_after_seconds }` (parse from `Retry-After` header — needs caller passing the header through).
- 5xx → `Provider { message }`.

Ollama's current `map_response_error` returns `Internal` for everything. Migrate to:
- 400 → `InvalidRequest { reason }`.
- 401/403 → `Auth { message }`.
- 404 → `InvalidRequest { reason: "model not found (run \`ollama pull <model>\` first): ..." }` (preserve existing remediation hint inline).
- 429 → `RateLimited { retry_after_seconds }`.
- 503 (model loading) → `Provider { message: "ollama server: \(detail)" }` (it's transient and `is_retryable()` is true for `Provider`).
- 5xx → `Provider`.

`map_client_error` for both plugins also typed: `ClientError::Exhausted { status, .. }` collapses to `RateLimited` when status==429, `Provider` for 5xx, `Transport` for synthesized 408.

---

## 5. Streaming

OpenAI's chat-completions SSE shape:

```
data: {"id":"chatcmpl-abc","choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}]}

data: {"id":"chatcmpl-abc","choices":[{"index":0,"delta":{"content":"Hi"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc","choices":[{"index":0,"delta":{"content":" there"},"finish_reason":null}]}

data: {"id":"chatcmpl-abc","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":12,"completion_tokens":3}}

data: [DONE]
```

Tool-call streaming:

```
data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","type":"function","function":{"name":"echo","arguments":""}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"te"}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"xt\":\"hi\"}"}}]}}]}

data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}],"usage":{...}}

data: [DONE]
```

### 5.1 Parser

Reuses `eventsource-stream` (workspace dep from sub-project 2a). Per-line shape:

```rust
#[derive(Deserialize)]
struct StreamEvent {
    choices: Vec<StreamChoice>,
    usage: Option<StreamUsage>,
}
#[derive(Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    finish_reason: Option<String>,
}
#[derive(Deserialize)]
struct StreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<StreamToolCallDelta>>,
}
#[derive(Deserialize)]
struct StreamToolCallDelta {
    index: u32,
    id: Option<String>,        // present on first delta only
    function: Option<StreamToolFnDelta>,
}
#[derive(Deserialize)]
struct StreamToolFnDelta {
    name: Option<String>,      // present on first delta only
    arguments: Option<String>, // accumulated across deltas
}
```

### 5.2 ToolUseAccumulator

Per `tool_calls[].index`, accumulate `name` (first delta only, kept) and `arguments` (concatenated). On finish_reason=="tool_calls" or [DONE], for each accumulator entry, parse `arguments` as JSON, emit one `CompletionChunk::ToolUse(ToolUse::new(id, name, parsed_input))`.

If `arguments` fails to parse as JSON at finish time → `LlmError::Stream { message: "openai tool_call arguments not valid JSON: ..." }`.

### 5.3 [DONE] sentinel

The terminal `data: [DONE]` line is consumed silently. Finish chunk is emitted from the last `finish_reason: "<...>"` event, NOT from [DONE].

If [DONE] arrives without a prior `finish_reason` event → `LlmError::Stream { message: "openai stream ended without finish_reason" }`.

---

## 6. Configuration shape (`config.rs`) + plugin entry

Mirrors anthropic (required api_key) with these specifics:

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenAIConfig {
    /// Override base URL. Default: https://api.openai.com.
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// Override env var name for the API key. Default: OPENAI_API_KEY.
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,

    /// Direct API key override. Test-only.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Per-request HTTP timeout in seconds. Default: 600 (matches
    /// Anthropic — OpenAI streaming can run minutes).
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,

    /// Optional organization id. Sent as `OpenAI-Organization` header.
    #[serde(default)]
    pub organization: Option<String>,

    /// Retry behavior. Same defaults as Anthropic.
    #[serde(default)]
    pub retry: RetryConfig,
}
```

`resolve_api_key` errors with `ConfigError::InvalidEnvVar` when the env var is missing (matches Anthropic — required auth). Validates the key starts with `sk-` (OpenAI keys start with `sk-` — historically `sk-...`, more recently `sk-proj-...`; the prefix check is `starts_with("sk-")`).

`OpenAIPlugin` + `LlmBackend` impl + `main.rs` mirror Ollama's shape.

---

## 7. Tool-use mapping

| Concern | OpenAI |
|---|---|
| Request: tools array | `[{type:"function",function:{name,description,parameters:<input_schema>}}]` (matches Ollama shape; same translation function lifted) |
| Request: tool_choice | Round-trip — see §4.2 |
| Request: assistant tool_calls | `[{id:<tu.id>,type:"function",function:{name:<tu.name>,arguments:<JSON-stringified input>}}]` |
| Request: tool result | `{role:"tool",tool_call_id:<tool_use_id>,content:<flattened text>}` (round-trips id) |
| Response batch | `choices[0].message.tool_calls` array; ids preserved |
| Response streaming | `delta.tool_calls[]` with `index` + `id` (first delta) + `function.{name,arguments}` accumulated per-index |
| Stop reason on tool-use turn | `finish_reason: "tool_calls"` → `StopReason::ToolUse` |
| Tool-call id semantics | OpenAI provides stable ids per turn; round-trip directly. NO synthesis needed for happy path; `"openai-tool-{i}"` is a defensive fallback if `id: None` (rare). |

---

## 8. Testing tier

### 8.1 Cassette catalog (9 files)

| Cassette | Scenario |
|---|---|
| `complete_happy_path.yaml` | Single-turn text response |
| `complete_with_system_prompt.yaml` | `req.system` → leading `role:system` message at index 0 |
| `complete_with_tools.yaml` | Tools sent; assistant returns `tool_calls` with real ids + `finish_reason:"tool_calls"` |
| `complete_429_then_success.yaml` | 1× 429 + Retry-After + 200; exercises typed `RateLimited` retry path |
| `complete_401_auth_failure.yaml` | 401 → typed `LlmError::Auth` |
| `complete_400_bad_request.yaml` | 400 → typed `LlmError::InvalidRequest` |
| `stream_text_only.yaml` | SSE: 2 content deltas + finish_reason event + `[DONE]` |
| `stream_with_tool_use.yaml` | SSE: tool_call deltas accumulated across multiple events |
| `stream_truncated_response.yaml` | SSE ends without `finish_reason` event → `LlmError::Stream` |

### 8.2 OpenAI plugin tests

- ~25 unit tests across config / request / response / error / client / stream / plugin (matches the per-module breakdown from anthropic + ollama).
- 6 cassette integration tests in `tests/complete.rs` (one per non-streaming cassette).
- 3 cassette integration tests in `tests/streaming.rs`.
- 2 env-gated live smoke tests in `tests/live.rs` (gated by `TAU_OPENAI_LIVE_TESTS=1`; cost ~$0.001/run on `gpt-4o-mini`).
- 1 conformance shim in `tests/conformance.rs` invoking the parameterized suite.

### 8.3 Conformance suite (`tau-plugin-conformance`)

Initial catalog (6 tests):

1. **`batch_happy_path`**: send a minimal request; assert non-empty `text`, valid `stop_reason`, `usage` populated when applicable.
2. **`batch_with_tools`**: send a request with one `ToolSpec`; cassette returns one `tool_call`; assert `tool_uses.len() == 1`, non-empty id+name, valid input.
3. **`streaming_text`**: drain stream; assert at least one `Text` chunk and exactly one `Finish`; `Finish` is the last chunk.
4. **`streaming_tool_use`**: assert at least one `ToolUse` chunk before `Finish`; `tu.input` parses as a JSON object.
5. **`error_rate_limited`**: 429 cassette; assert `Err(LlmError::RateLimited { retry_after_seconds })`. Plugin's retry path must have already exhausted (cassette returns 429 to all attempts).
6. **`error_auth`**: 401 cassette; assert `Err(LlmError::Auth { .. })`.

Each test takes a `&dyn LlmBackend` plus a path to its plugin's cassette file. The plugin's `tests/conformance.rs` is the wiring shim.

The suite is **not** allowed to mandate:
- specific response text (would test quality, not shape).
- specific stop_reason values beyond "valid variant".
- specific tool_use ids (Ollama synthesizes; OpenAI/Anthropic preserve provider ids).
- specific token counts (varies by provider tokenizer).

### 8.4 Cassette format unchanged

Same YAML shape as 2a/2b — the replayer is wire-format-agnostic. Lifted verbatim into `tau-plugin-test-support`.

### 8.5 Live smoke tests

Cost-aware default: live tests cost ~$0.001/run on gpt-4o-mini. Same `#[ignore]`-by-default pattern as 2a. Setup:

```bash
export OPENAI_API_KEY=sk-proj-...
TAU_OPENAI_LIVE_TESTS=1 cargo test -p openai --test live -- --ignored --nocapture
```

CI never runs live tests.

---

## 9. Migration: anthropic + ollama

### 9.1 Test-support migration

For each existing plugin (anthropic + ollama):

1. Delete `tests/common/cassette.rs` (was a verbatim copy).
2. Replace with a re-export from `tau_plugin_test_support::cassette`:
   ```rust
   // tests/common/mod.rs:
   pub use tau_plugin_test_support::cassette;
   ```
   (or `mod cassette { pub use tau_plugin_test_support::cassette::*; }` if rust forbids the direct re-export pattern in integration tests.)
3. Update integration test imports if needed (most refer to `cassette::replay` which moves transparently).
4. Add `tau-plugin-test-support = { workspace = true }` to dev-dependencies.

Net delete: ~620 LOC across both plugins (323 LOC × 2 — 26 LOC retained adapter).

### 9.2 Error mapping migration

Both plugins' `error.rs::map_response_error` and `map_client_error`
move from "everything → Internal" to typed variants per §4.5.

Existing tests that asserted `LlmError::Internal { message contains "rate limited" }` etc. are **updated** to assert `LlmError::RateLimited { retry_after_seconds: Some(_) }` etc. This is a substantive test change but a 1-for-1 mapping per cassette.

`map_response_error` signature change: gains a `headers: &HeaderMap` parameter on both plugins so it can extract `Retry-After` for `RateLimited`. (Currently the parser just sees the body; the header info is on `reqwest::Response`. Plumb it through from `client.rs`.)

### 9.3 Conformance suite integration

For each plugin:

1. Add `crates/tau-plugins/<plugin>/tests/conformance.rs`:
   ```rust
   //! Run the conformance suite against this plugin.
   use tau_plugin_conformance::ConformanceSuite;
   use <plugin>_plugin_lib::plugin::<Plugin>;

   #[tokio::test]
   async fn run_conformance_suite() {
       let plugin = ...;  // build via Configure::from_config
       let cassettes = std::path::PathBuf::from("tests/conformance-cassettes");
       ConformanceSuite::default().run(&plugin, &cassettes).await;
   }
   ```
2. Add `tests/conformance-cassettes/` directory with one cassette per behavioral test (6 files).

Cassettes for the conformance suite are **per-plugin** (each provider's wire shape differs); the suite consumes them through the `tau_plugin_test_support::cassette::replay` helper.

---

## 10. Implementation plan outline (~22 tasks)

The plan derived from this spec follows the established cadence (one
Conventional Commits commit per task; full verification before commit;
push after each task; PR auto-triggers CI). Tasks 1-3 detailed at full
Plan-2 fidelity; remaining tasks hybrid format.

| # | Task | Notes |
|---|---|---|
| 1 | Workspace scaffold: 3 new crates (`openai/`, `tau-plugin-test-support/`, `tau-plugin-conformance/`); register in workspace; placeholder `lib.rs` / `main.rs` stubs | No new workspace deps. |
| 2 | `tau-plugin-test-support`: lift `cassette.rs` verbatim; export `replay`, `RecordedRequest` types | Run anthropic + ollama integration tests against the lifted module before committing. |
| 3 | Migrate anthropic: delete `tests/common/cassette.rs`, re-export from shared crate; verify all anthropic integration tests pass | Behavior change: zero. |
| 4 | Migrate ollama: same as task 3 | Behavior change: zero. |
| 5 | OpenAI: `OpenAIConfig` + `Configure` + `resolve_api_key` + `validate_retry`; ~9 unit tests | spec §6 |
| 6 | OpenAI: `request.rs` with body builder + tool/tool_choice translation + sampling overrides; ~10 unit tests | spec §4.2, §7 |
| 7 | OpenAI: `response.rs` parser + tool_call id round-trip + finish_reason mapping; ~6 unit tests | spec §4.3 |
| 8 | OpenAI: `error.rs` TYPED `map_response_error` + `map_client_error`; ~6 unit tests | spec §4.4 |
| 9 | OpenAI: `client.rs` + retry; ~5 in-process TcpListener tests | spec §4.1 |
| 10 | OpenAI: `stream.rs` SSE parser + `ToolUseAccumulator`; ~6 unit tests | spec §5 |
| 11 | OpenAI: `plugin.rs` + `main.rs` LlmBackend impl; ~3 unit tests | spec §6 |
| 12 | OpenAI: 6 batch cassettes + `tests/complete.rs` integration tests | spec §8.1, §8.2 |
| 13 | OpenAI: 3 streaming cassettes + `tests/streaming.rs` | spec §8.1, §8.2 |
| 14 | OpenAI: `tests/live.rs` + `scripts/rerecord-openai-cassettes.sh` | spec §8.5 |
| 15 | `tau-plugin-conformance`: lib crate scaffold + `ConformanceSuite::default().run(plugin, cassettes_dir)` API + 6 test functions | spec §8.3 |
| 16 | OpenAI: conformance shim + 6 conformance cassettes + `tests/conformance.rs`; verify all 6 conformance tests pass | spec §9.3 |
| 17 | Anthropic: migrate `error.rs` to TYPED variants; update existing test assertions; add conformance shim + cassettes | spec §4.5, §9 |
| 18 | Ollama: migrate `error.rs` to TYPED variants; update existing test assertions; add conformance shim + cassettes | spec §4.5, §9 |
| 19 | ADR-0009: typed-error migration policy + conformance suite charter | spec §2.2 row 14 |
| 20 | CI: add 4 new build/test jobs in `ci.yml` | spec §1.1 |
| 21 | Final local verification + mark PR ready (user-driven gate) | (gate) |
| 22 | ROADMAP + branch protection 17 → 21 + ADR-0009 status to Accepted + squash merge (user-driven gate) | (gate) |

22 tasks. Tasks 21–22 are user-driven gates per the established pattern.

---

## 11. Out of scope (explicit deferrals)

| Topic | Where it lives |
|---|---|
| `LlmError::ModelNotFound` typed variant | Future sub-project; existing `InvalidRequest` covers v0.1. |
| OpenAI `/v1/responses` API | Future plugin extension or fresh sub-project. |
| Multi-modal | Deferred until `tau_ports::ContentBlock::Image` lands. |
| `logprobs`, `n>1`, `response_format`, `service_tier` | Provider-specific knobs; route via `provider_specific` if needed. |
| Embeddings, fine-tuning, batch API | Different ports / different sub-projects. |
| Cassette record-mode automation | Same v0.1 stance as 2a/2b. |
| Conformance tests for response quality | NG7 prohibits — quality is downstream's job. |
| Conformance tests for non-mechanical invariants | YAGNI — add when a real bug motivates. |
| `tau-runtime` migration to typed-error matching | Out of scope of this sub-project; runtime continues to format errors via `Display` and gains typed-handling opportunistically. |

---

## 12. Cross-references

- [ADR-0008](../../decisions/0008-plugin-loading.md) — third real consumer; conformance suite from §17 deferral activates.
- ADR-0009 (NEW, lands with this sub-project) — typed-error migration + conformance suite charter.
- [Anthropic plugin spec](2026-04-29-anthropic-plugin-design.md) — pattern source.
- [Ollama plugin spec](2026-04-29-ollama-plugin-design.md) — pattern source.
- [ROADMAP](../../../ROADMAP.md) Phase 1 priority 2c — marked complete on sub-project sign-off.

## 13. Open follow-ups

- **`LlmError::ModelNotFound`** typed variant (small additive change; defer until a real caller needs to branch on it).
- **Conformance catalog expansion** — add tests for sampling override propagation, system-prompt placement, multi-turn conversations, image inputs (when ContentBlock::Image lands).
- **Cassette record-mode automation** — eliminate hand-authored cassette drift across all three plugins simultaneously.
- **`tau-runtime` migration to typed-error matching** — branch on `RateLimited.retry_after_seconds` for honest backoff in the runtime's retry helper (today the runtime relies on `is_retryable()` only).
- **Standalone-repo migration** for the three plugin crates (NG4 mid-term direction).
