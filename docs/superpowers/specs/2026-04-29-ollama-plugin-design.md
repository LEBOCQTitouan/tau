# Ollama LLM-backend plugin — Phase 1 sub-project 2b

**Status:** Draft (this spec) → Implementation plan derived → no ADR
needed for this sub-project (purely additive — see §6).

**Sub-project scope:** Phase 1 priority 2b from the
[ROADMAP](../../../ROADMAP.md). Second real LLM-backend plugin
(Anthropic shipped as 2a). Validates the plugin loading mechanism
[ADR-0008](../../decisions/0008-plugin-loading.md) against a second
transport (NDJSON vs SSE) and a second auth model (optional bearer
vs required API key). Establishes the third real implementation
candidate (with future OpenAI plugin, priority 2c) so the deferred
conformance test suite becomes valuable.

---

## 1. Summary

Sub-project 2a shipped the Anthropic Claude (Messages API) plugin as
the first real LLM-backend plugin. This sub-project ships the second:
**Ollama** (local LLM runner). Anthropic and Ollama together exercise
two distinct transports (SSE vs NDJSON), two distinct auth models
(required API key vs optional bearer token), and two distinct retry
edge cases (Anthropic 429 vs Ollama 503-on-model-load).

The plugin:

- Speaks `tau_ports::LlmBackend` (native trait, IPC-erased through
  `DynLlmBackend`).
- Targets Ollama's native `/api/chat` endpoint (NOT the OpenAI-compat
  shim — see §2 decision #1).
- Accepts no auth by default (correct for local Ollama at
  `http://localhost:11434`); optional bearer token via env var or
  handshake config for hosted Ollama services.
- Translates `CompletionRequest` ↔ Ollama `/api/chat` JSON. System
  prompts map to a leading `{role:"system", content:"..."}` message
  (NOT a top-level `system` field — Ollama-specific shape).
- Streams responses via NDJSON → `tau_ports::CompletionStream`.
- Handles tool-use round-trip (request `tools[*].function.{name,
  description, parameters}`; response `message.tool_calls[*]`).
- Synthesizes deterministic tool_call ids per turn (`"ollama-tool-{n}"`)
  since Ollama's tool_calls don't always include an id.
- Retries transient errors with exponential backoff. 503 ("model is
  loading") is the load-bearing case for Ollama; tested explicitly.
- Maps HTTP status + Ollama error envelopes to `LlmError::Internal`.
  404 errors include the remediation hint "run `ollama pull <model>`
  first".
- Lives in-tree under `crates/tau-plugins/ollama/`, parallel to
  `crates/tau-plugins/anthropic/`.

### 1.1 Scope confirmed

**Ships:**

- One new workspace member: `crates/tau-plugins/ollama/`.
- Real HTTP client backed by `reqwest` (workspace dep, already
  present from sub-project 2a). NO new workspace deps.
- Hand-rolled NDJSON parser (~50 LOC; no `eventsource-stream`).
- Cassette-replay test harness (~250 LOC, **duplicated** from
  Anthropic plugin per §2 decision #2; rule-of-three refactor
  deferred to sub-project 2c or its own).
- 9 cassette files (6 batch + 3 streaming) + 2 env-gated live smoke
  tests + ~25 unit tests.
- Plugin manifest declaring `provides = "llm_backend"`.

**Does NOT ship:**

- OpenAI-compat shim path (`/v1/chat/completions`) — Q1 decision.
- Generate API (`/api/generate`, the older completion endpoint).
- Embeddings, model management (`/api/pull`, `/api/list`,
  `/api/show`) — different port or operator-side concerns.
- Multi-modal (image input) — additive when
  `tau_ports::ContentBlock::Image` lands.
- Structured outputs (`format` field for JSON schema-constrained
  outputs) — Ollama-specific feature; future plugin version.
- Tool-call argument streaming (fragment-by-fragment) — Ollama in
  2026 sends tool_calls on a single line; future versions may stream.
- Long-delay path for model-load 503 (5+ minute waits) — v0.1 uses
  standard exponential backoff.
- `LlmError::ModelNotFound` / `RateLimited` / `Auth` typed variants
  — pair with sub-project 2c (OpenAI) when the third consumer
  establishes the vocabulary-expansion case.
- New ADR — purely additive, no public API / protocol / error
  vocabulary changes.

### 1.2 Constitution alignment

| Constraint | This plugin's answer |
|---|---|
| `forbid(unsafe_code)` | Plain Rust; no FFI, no manual unsafe. |
| **G6** runtime not framework | Plugin is a thin translation layer. |
| **G9** observable by default | Retries, request/response cycles, stream parsing emit `tracing` events under `target = "ollama_plugin::*"` re-emitted by the host. |
| **NG4** no marketplace | In-tree for v0.1; standalone-repo migration deferred. |
| **NG9** no credential management | Plugin reads optional bearer token from env var; never persists, never logs. |
| **NG10** no telemetry | Tracing is local; only outbound traffic is direct calls to Ollama. |

---

## 2. Decisions

This sub-project does NOT introduce its own ADR. Reasons:

1. **Purely additive** — new workspace crate; no existing API changes.
2. **No protocol changes** — uses ADR-0008's wire vocabulary unchanged.
3. **No new error variants** in tau-ports / tau-runtime / tau-plugin-sdk.
4. **No package manifest changes**.
5. **`ConfigError::InvalidEnvVar`** (added in sub-project 2a) is
   reused for the bearer-token-env case; no new SDK amendment needed.

The sub-project-local engineering decisions the brainstorm settled:

| # | Decision | Rationale |
|---|---|---|
| 1 | **Endpoint:** Native `POST /api/chat` (NOT OpenAI-compat shim) | Local-first authenticity; better tool-use support; decouples from priority 2c. The shim is "OpenAI-compatible enough" — Ollama's docs warn niche features don't round-trip cleanly. |
| 2 | **Code sharing with Anthropic plugin:** **Duplicate**, not extract a shared crate | At N=2 the shared surface is one example deep. Extracting now bakes Anthropic-specific decisions (`Retry-After` semantics, error envelope shape) into the shared crate. Refactor at N=3 (after sub-project 2c) when the rule-of-three justifies it. ~300 LOC duplication acceptable. |
| 3 | **Distribution:** in-tree at `crates/tau-plugins/ollama/` | Matches Anthropic + echo plugin precedent. |
| 4 | **Default base URL:** `http://localhost:11434` | Ollama's default port. |
| 5 | **Authentication:** none by default; optional bearer token via `bearer_token_env` (default `OLLAMA_BEARER_TOKEN`) or `bearer_token` (raw, test-only). If neither set, no `Authorization` header is sent. | Local Ollama needs no auth; hosted Ollama services use bearer tokens. |
| 6 | **Default request timeout:** 900s (15 minutes) | Local Ollama can take 30–60s to load a model on first call. Generous default. |
| 7 | **Retry:** same defaults as Anthropic (max_attempts=3, base_delay_ms=1000, respect_retry_after=true). 503 is the load-bearing case (model loading); treated as transient, retried via exponential backoff. | Ollama's 503-on-model-load is more common than Anthropic's transient 5xx. |
| 8 | **Model selection:** pass-through, no validation (matches Anthropic) | User picks `model="llama3.2"` etc. |
| 9 | **Tool-use:** best-effort. Plugin doesn't maintain a known-supports-tools allowlist (brittle). User picks tool-capable model (llama3.1+, qwen2.5+, mistral); if unsupported, Ollama returns 400 → `LlmError::Internal`. | Ollama's model lineup churns; allowlist would lag. |
| 10 | **Tool-call id synthesis:** plugin synthesizes `"ollama-tool-{n}"` per turn when Ollama doesn't include an id | Required for the kernel's multi-turn loop pairing. Deterministic per turn. |
| 11 | **`tool_choice` mapping:** `Auto` (default) → no `tool_choice` field (Ollama default behavior); `Required` and `Specific` → drop with `tracing::warn!` (Ollama's `/api/chat` doesn't support tool_choice). `None` → omit `tools` array. | Ollama doesn't accept the OpenAI-compat `tool_choice` field. |
| 12 | **Vision / image content:** out-of-scope v0.1 | Additive future when tau-ports gains `ContentBlock::Image`. |
| 13 | **Token usage:** `prompt_eval_count` → `input_tokens`, `eval_count` → `output_tokens`. Both `Option<u32>` so `usage` becomes `None` when absent. | Ollama's field names differ from tau-ports'; mapping is direct when both present. |
| 14 | **Error fidelity:** all 4xx/5xx collapse to `LlmError::Internal { message }` for v0.1 (matches Anthropic). 404 includes `ollama pull` remediation hint. | Richer mapping when sub-project 2c establishes the third consumer. |
| 15 | **Streaming wire format:** NDJSON (split on `\n`); ~50 LOC parser, no `eventsource-stream` dep | Ollama's `/api/chat` with `stream:true` emits one JSON object per line. |
| 16 | **Stream-end without `done:true`:** yields `LlmError::Stream { message: "ended before done:true line" }` | Defensive: if the connection drops mid-stream, plugin signals cleanly. |
| 17 | **Testing:** cassette replay (replayer copied from Anthropic) + env-gated live smoke (`TAU_OLLAMA_LIVE_TESTS=1`, requires running Ollama instance) | Same pattern as sub-project 2a. |

### 2.1 Decisions explicitly out of scope

| Topic | Where it lives |
|---|---|
| OpenAI-compat shim path | Sub-project 2c (OpenAI plugin) targets `/v1/chat/completions` directly |
| Embeddings | Different port (Storage-adjacent); future sub-project |
| Model management endpoints (`/api/pull`, etc.) | Operator concern, not plugin |
| Multi-modal | Future when ContentBlock::Image lands |
| Structured outputs (`format` field) | Future plugin version |
| Long-delay model-load 503 path | v0.1 uses standard backoff; revisit if real-world model-load times bite |
| `LlmError` vocabulary expansion | Pair with sub-project 2c |
| Conformance test suite | Pair with sub-project 2c (third implementation makes it high-value) |
| Shared `tau-plugin-test-support` crate | Rule-of-three trigger; lift after sub-project 2c |

---

## 3. Architecture

### 3.1 Workspace layout

```
crates/tau-plugins/ollama/
├── Cargo.toml                    -- bin target `ollama-plugin`
├── tau.toml                      -- plugin manifest
├── src/
│   ├── main.rs                   -- #[tokio::main] → run_llm_backend_with_config
│   ├── plugin.rs                 -- OllamaPlugin + LlmBackend impl
│   ├── config.rs                 -- OllamaConfig + RetryConfig + Configure impl
│   ├── client.rs                 -- HTTP client (reqwest), retry; DUPLICATED from anthropic with Ollama-specific tweaks
│   ├── request.rs                -- CompletionRequest → /api/chat JSON
│   ├── response.rs               -- /api/chat JSON → CompletionResponse
│   ├── stream.rs                 -- NDJSON parser (split-on-\n) → CompletionStream
│   └── error.rs                  -- HTTP status + Ollama error JSON → LlmError
└── tests/
    ├── cassettes/                -- 9 cassette YAMLs (6 batch + 3 streaming including the truncated-stream variant)
    ├── common/                   -- DUPLICATED from anthropic: cassette replayer + helpers
    ├── complete.rs               -- batch tests via cassette replay
    ├── streaming.rs              -- streaming tests
    └── live.rs                   -- env-gated smoke tests
```

### 3.2 Dependencies

**No new workspace deps.** All required deps already present from sub-project 2a:

- `reqwest` (workspace; uses `json` + `rustls-tls` + `stream` features for `bytes_stream()`)
- `tokio` (workspace)
- `serde` + `serde_json` (workspace)
- `thiserror` (workspace)
- `secrecy` (workspace; for optional bearer token)
- `async-stream` (workspace; from sub-project 2a)
- `tau-domain`, `tau-ports`, `tau-plugin-protocol`, `tau-plugin-sdk` (workspace)
- `futures-core` + `futures-util` (workspace)

**No `eventsource-stream`** — NDJSON parser is split-on-newline.

### 3.3 Dataflow

```
tau-cli (existing)
  └─ tau-runtime::plugin_host::load_llm_backend
      └─ spawns target/release/ollama-plugin
          └─ tau-plugin-sdk::run_llm_backend_with_config::<OllamaPlugin>
              ├─ handshake (config: { base_url, retry: {...} })
              ├─ Configure::from_config → OllamaPlugin { client: OllamaClient }
              ├─ dispatch loop:
              │   ├─ llm.complete → OllamaPlugin::complete
              │   │   ├─ request::build_chat_body(req, stream=false)
              │   │   ├─ client::post_chat(&body, stream=false)
              │   │   │   ├─ retry on 429/503/timeout (503 = model loading)
              │   │   │   └─ honor Retry-After
              │   │   ├─ response::parse_chat_response(body) → CompletionResponse
              │   │   └─ map error if non-2xx
              │   └─ llm.stream → OllamaPlugin::stream
              │       ├─ request::build_chat_body(req, stream=true)
              │       ├─ client::post_chat(&body, stream=true)
              │       └─ stream::parse_ndjson(response) → CompletionStream
              │           (chunks emitted line-by-line; tool_calls on
              │            their own line; Finish on done:true)
              └─ frames out via stdout
```

---

## 4. HTTP layer (`client.rs`, `request.rs`, `response.rs`, `error.rs`)

### 4.1 `client.rs` — HTTP client with retry

Same shape as `anthropic/src/client.rs`. Differences:

```rust
pub(crate) struct OllamaClient {
    inner: reqwest::Client,
    base_url: String,                    // default http://localhost:11434
    bearer_token: Option<SecretString>,  // None for local; Some for hosted
    retry: RetryConfig,
}

impl OllamaClient {
    pub(crate) async fn post_chat(
        &self,
        body: &serde_json::Value,
        stream: bool,
    ) -> Result<reqwest::Response, ClientError> {
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));
        let mut attempt = 0;
        loop {
            attempt += 1;
            let mut req = self.inner
                .post(&url)
                .header("content-type", "application/json")
                .json(body);
            if let Some(token) = &self.bearer_token {
                req = req.header(
                    "authorization",
                    format!("Bearer {}", token.expose_secret()),
                );
            }
            // No `accept: text/event-stream` — Ollama emits NDJSON when
            // body.stream == true.
            let send_result = req.send().await;
            match self.classify(send_result, attempt) {
                Decision::Return(resp) => return Ok(resp),
                Decision::Error(err) => return Err(err),
                Decision::Retry { delay_ms, status } => {
                    if attempt >= self.retry.max_attempts {
                        return Err(ClientError::Exhausted { status, attempts: attempt });
                    }
                    tracing::warn!(
                        target: "ollama_plugin::retry",
                        attempt, max = self.retry.max_attempts, delay_ms,
                        status = status.as_u16(),
                        "retrying transient error",
                    );
                    sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }
}

// 503 retry is the load-bearing case: Ollama returns 503 during
// model load, which can take 10–60s. Standard exponential backoff
// (1s, 2s, 4s) handles short loads; longer loads exhaust the retry
// budget and surface as Exhausted.
fn is_retryable_status(status: StatusCode) -> bool {
    matches!(status.as_u16(), 429 | 503) || (status.as_u16() >= 500 && status.as_u16() != 501)
}
```

`ClientError` enum is duplicated with the same shape:
`Transport(reqwest::Error)`, `Exhausted { status: StatusCode, attempts: u32 }`.

### 4.2 `request.rs` — `CompletionRequest` → Ollama `/api/chat` JSON

Ollama's request shape:

```json
{
  "model": "llama3.2",
  "messages": [
    {"role": "system", "content": "you are concise"},
    {"role": "user", "content": "say hi"}
  ],
  "stream": false,
  "tools": [{"type": "function", "function": {"name", "description", "parameters"}}],
  "options": {"temperature": 0.7, "num_predict": 100, "top_p": 0.9, "seed": 42, "stop": ["..."]}
}
```

Translation:

```rust
pub(crate) fn build_chat_body(
    req: &CompletionRequest,
    stream: bool,
) -> Result<serde_json::Value, BuildError> {
    let mut body = serde_json::json!({
        "model": req.model,
        "messages": translate_messages(req)?,
        "stream": stream,
    });

    // Tools: omit when empty OR tool_choice is None.
    if !req.tools.is_empty() && !matches!(req.tool_choice, ToolChoice::None) {
        body["tools"] = serde_json::Value::Array(
            req.tools.iter().map(translate_tool).collect::<Result<Vec<_>, _>>()?,
        );
    }
    // Ollama doesn't support tool_choice. Warn at debug and drop.
    if matches!(req.tool_choice, ToolChoice::Required | ToolChoice::Specific { .. }) {
        tracing::debug!(
            target: "ollama_plugin::request",
            "tool_choice {:?} unsupported by Ollama /api/chat; ignoring",
            req.tool_choice,
        );
    }

    // Sampling overrides → options sub-object.
    let mut options = serde_json::Map::new();
    if let Some(max) = req.max_tokens {
        // Ollama uses num_predict, not max_tokens.
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
        options.insert("stop".into(), serde_json::json!(req.stop_sequences));
    }
    if !options.is_empty() {
        body["options"] = serde_json::Value::Object(options);
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

fn translate_messages(req: &CompletionRequest) -> Result<serde_json::Value, BuildError> {
    let mut out: Vec<serde_json::Value> = Vec::new();

    // System prompt: Ollama places it as a leading role:system message
    // (NOT a top-level field like Anthropic).
    if let Some(system) = &req.system {
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
                    entry["tool_calls"] = serde_json::Value::Array(tool_calls);
                }
                out.push(entry);
            }
            LlmProviderMessage::ToolResult { tool_use_id: _, content, is_error: _ } => {
                // Ollama's tool message doesn't carry tool_use_id; the
                // kernel pairs results to calls by message order.
                out.push(serde_json::json!({
                    "role": "tool",
                    "content": flatten_text(content),
                }));
            }
            _ => return Err(BuildError::UnknownMessageVariant),
        }
    }
    Ok(serde_json::Value::Array(out))
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
) -> Result<(String, Vec<serde_json::Value>), BuildError> {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text(s) => text.push_str(s),
            ContentBlock::ToolUse(tu) => {
                tool_calls.push(serde_json::json!({
                    "function": {
                        "name": tu.name,
                        "arguments": serde_json::to_value(&tu.input)?,
                    },
                }));
            }
            _ => return Err(BuildError::UnknownContentBlock),
        }
    }
    Ok((text, tool_calls))
}

fn translate_tool(spec: &ToolSpec) -> Result<serde_json::Value, BuildError> {
    Ok(serde_json::json!({
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": serde_json::to_value(&spec.input_schema)?,
        },
    }))
}
```

> **Differences from Anthropic plugin:**
> 1. System prompt → leading `role:system` message (NOT top-level field).
> 2. Multi-block User/Assistant content concatenated to a flat string
>    (Ollama's `/api/chat` content is `String`, not array of typed blocks).
> 3. ToolUse blocks in Assistant content split into the `tool_calls`
>    array; remaining text concatenated into `content`.
> 4. Sampling overrides go inside `options` sub-object.
> 5. `max_tokens` renamed to `num_predict`.
> 6. `tool_choice` dropped (Ollama doesn't support it on `/api/chat`).
> 7. `tool_use_id` not round-tripped to Ollama (Ollama's tool message
>    has no such field; ordering pairs them).

### 4.3 `response.rs` — `/api/chat` JSON → `CompletionResponse`

Ollama's batch response shape:

```json
{
  "model": "llama3.2",
  "created_at": "2026-04-29T...",
  "message": {
    "role": "assistant",
    "content": "Hello world",
    "tool_calls": [{"function": {"name": "echo", "arguments": {"text": "hi"}}}]
  },
  "done": true,
  "total_duration": 1234567,
  "prompt_eval_count": 12,
  "eval_count": 3,
  "done_reason": "stop"
}
```

Translation:

```rust
pub(crate) fn parse_chat_response(body: &str) -> Result<CompletionResponse, ParseError> {
    let parsed: OllamaChatResponse = serde_json::from_str(body)?;

    let text = parsed.message.content.unwrap_or_default();
    let tool_uses: Vec<ToolUse> = parsed.message.tool_calls.unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, tc)| {
            // Ollama's tool_calls don't always carry an id; synthesize.
            let id = tc.id.unwrap_or_else(|| format!("ollama-tool-{i}"));
            let input: tau_domain::Value = serde_json::from_value(tc.function.arguments)
                .map_err(|e| ParseError::ToolUseInput { name: tc.function.name.clone(), source: e })?;
            Ok(ToolUse::new(id, tc.function.name, input))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let stop_reason = parsed.done_reason.as_deref()
        .map(map_done_reason)
        .unwrap_or(StopReason::EndTurn);

    let usage = match (parsed.prompt_eval_count, parsed.eval_count) {
        (Some(input), Some(output)) => Some(TokenUsage::new(input, output)),
        _ => None,
    };

    Ok(tau_ports::fixtures::make_completion_response(text, tool_uses, stop_reason, usage))
}

fn map_done_reason(s: &str) -> StopReason {
    match s {
        "stop" => StopReason::EndTurn,
        "length" => StopReason::MaxTokens,
        // Ollama doesn't have a tool-use-specific stop reason — when a
        // model returns tool_calls, done_reason is still "stop".
        // Caller infers from non-empty tool_uses.
        other => {
            tracing::warn!(
                target: "ollama_plugin::response",
                done_reason = other,
                "unknown done_reason; defaulting to EndTurn",
            );
            StopReason::EndTurn
        }
    }
}
```

### 4.4 `error.rs` — HTTP + Ollama error JSON → `LlmError`

Ollama's error envelope is simpler than Anthropic's:

```json
{"error": "model 'llama99' not found, try pulling it first"}
```

```rust
pub(crate) fn map_response_error(status: reqwest::StatusCode, body: &str) -> LlmError {
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
    LlmError::Internal {
        message: format!("ollama {category} ({status}): {detail}"),
    }
}

#[derive(serde::Deserialize)]
struct OllamaErrorBody {
    error: String,
}
```

> **404 messaging**: includes the remediation hint inline because
> 404 is by far the most common failure mode for new Ollama users
> (model not pulled yet).

---

## 5. Streaming (`stream.rs`)

### 5.1 NDJSON wire format

Per `/api/chat` with `stream:true`:

```
{"model":"llama3.2","created_at":"...","message":{"role":"assistant","content":"Hello"},"done":false}
{"model":"llama3.2","created_at":"...","message":{"role":"assistant","content":" world"},"done":false}
{"model":"llama3.2","created_at":"...","message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"echo","arguments":{"text":"hi"}}}]},"done":false}
{"model":"llama3.2","created_at":"...","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":12,"eval_count":3}
```

Each `\n`-terminated line is one JSON object. Final line has `done:true`.

### 5.2 Parser shape

```rust
pub(crate) async fn parse_ndjson(
    body: reqwest::Response,
) -> Result<CompletionStream, LlmError> {
    let bytes_stream = body.bytes_stream();
    Ok(Box::pin(stream_from_lines(bytes_stream)))
}

fn stream_from_lines<S>(
    mut bytes: S,
) -> impl futures_core::Stream<Item = Result<CompletionChunk, LlmError>> + Send
where
    S: futures_core::Stream<Item = reqwest::Result<bytes::Bytes>> + Send + Unpin + 'static,
{
    async_stream::try_stream! {
        let mut buf = Vec::new();
        let mut tool_call_index: usize = 0;

        while let Some(chunk_res) = bytes.next().await {
            let chunk = chunk_res.map_err(|e| LlmError::Stream {
                message: format!("ollama stream transport: {e}"),
            })?;
            buf.extend_from_slice(&chunk);

            // Drain complete lines (separated by '\n').
            while let Some(nl_pos) = buf.iter().position(|&b| b == b'\n') {
                let line_bytes = buf.drain(..=nl_pos).collect::<Vec<u8>>();
                let line = match std::str::from_utf8(&line_bytes[..nl_pos]) {
                    Ok(s) => s.trim(),
                    Err(e) => {
                        yield Err(LlmError::Stream {
                            message: format!("ollama stream UTF-8: {e}"),
                        })?;
                        return;
                    }
                };
                if line.is_empty() { continue; }

                let parsed: StreamLine = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(e) => {
                        yield Err(LlmError::Stream {
                            message: format!("ollama stream line decode: {e} (raw: {line})"),
                        })?;
                        return;
                    }
                };

                // Yield text delta if non-empty.
                if let Some(msg) = parsed.message.as_ref() {
                    if let Some(text) = msg.content.as_ref() {
                        if !text.is_empty() {
                            yield CompletionChunk::Text { delta: text.clone() };
                        }
                    }
                    // Yield each tool_call as a CompletionChunk::ToolUse.
                    if let Some(calls) = msg.tool_calls.as_ref() {
                        for call in calls {
                            let id = call.id.clone()
                                .unwrap_or_else(|| format!("ollama-tool-{tool_call_index}"));
                            tool_call_index += 1;
                            let input: tau_domain::Value = serde_json::from_value(
                                call.function.arguments.clone(),
                            ).map_err(|e| LlmError::Stream {
                                message: format!("ollama stream tool_use input decode: {e}"),
                            })?;
                            yield CompletionChunk::ToolUse(ToolUse::new(
                                id,
                                call.function.name.clone(),
                                input,
                            ));
                        }
                    }
                }

                if parsed.done {
                    let stop_reason = parsed.done_reason.as_deref()
                        .map(map_done_reason)
                        .unwrap_or(StopReason::EndTurn);
                    let usage = match (parsed.prompt_eval_count, parsed.eval_count) {
                        (Some(i), Some(o)) => Some(TokenUsage::new(i, o)),
                        _ => None,
                    };
                    yield CompletionChunk::Finish { stop_reason, usage };
                    return;
                }
            }
        }

        // Stream ended without a `done: true` line — defensive.
        yield Err(LlmError::Stream {
            message: "ollama stream ended before done:true line".into(),
        })?;
    }
}
```

### 5.3 Key differences from Anthropic SSE parser

| Concern | Anthropic (`eventsource-stream`) | Ollama (custom split-on-`\n`) |
|---|---|---|
| External dep | `eventsource-stream` | None (just `async-stream`) |
| Frame boundary | `\n\n` between SSE events | `\n` between JSON lines |
| Decode unit | Typed event enum with `tag = "type"` | One typed `StreamLine` per line |
| Tool-use accumulation | `ToolUseAccumulator` over multiple `input_json_delta` fragments | Single line carries the full `tool_calls` array |
| Final chunk signal | `message_stop` event | `done: true` line |
| Mid-stream error signaling | `event: error` with structured payload | HTTP-level only; truncated stream → `LlmError::Stream` |
| Connection drops mid-stream | `eventsource-stream` yields error | Plugin yields `LlmError::Stream { "ended before done:true line" }` |

### 5.4 Tool-use streaming behavior

Per Ollama in 2026: tool_calls in streaming mode are typically delivered
**on a single line** rather than accumulated across fragments. Plugin
emits `CompletionChunk::ToolUse(ToolUse)` once per tool_call entry on
that line. No accumulator needed.

If a future Ollama version starts streaming tool-call argument
fragments, the plugin would need accumulation; that's a future
amendment, not v0.1.

---

## 6. Configuration shape (`config.rs`) + plugin entry

### 6.1 `OllamaConfig` + `Configure` impl

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OllamaConfig {
    /// Override base URL. Default: <http://localhost:11434>.
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// Override env var name for an optional bearer token. Default:
    /// `OLLAMA_BEARER_TOKEN`. Unset env var → no Authorization header.
    #[serde(default = "default_bearer_token_env")]
    pub bearer_token_env: String,

    /// Direct bearer-token override. Test-only.
    #[serde(default)]
    pub bearer_token: Option<String>,

    /// Per-request HTTP timeout in seconds. Default: 900 (15 min).
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,

    /// Retry behavior. Same defaults as anthropic plugin.
    #[serde(default)]
    pub retry: RetryConfig,
}

#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryConfig {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_base_delay_ms")]
    pub base_delay_ms: u64,
    #[serde(default = "default_respect_retry_after")]
    pub respect_retry_after: bool,
}
```

Defaults:

| Field | Default |
|---|---|
| `base_url` | `"http://localhost:11434"` |
| `bearer_token_env` | `"OLLAMA_BEARER_TOKEN"` |
| `bearer_token` | `None` |
| `request_timeout_secs` | `900` |
| `retry.max_attempts` | `3` |
| `retry.base_delay_ms` | `1000` |
| `retry.respect_retry_after` | `true` |

### 6.2 Validation in `Configure::from_config`

```rust
impl Configure for OllamaPlugin {
    type Config = OllamaConfig;

    fn from_config(cfg: Self::Config) -> Result<Self, ConfigError> {
        let bearer_token = resolve_bearer_token(&cfg)?;
        validate_retry(&cfg.retry)?;

        let inner = reqwest::Client::builder()
            .timeout(cfg.request_timeout())
            .user_agent(format!("tau-ollama-plugin/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| ConfigError::InvalidValue {
                field: "request_timeout",
                detail: format!("could not build HTTP client: {e}"),
            })?;

        let client = OllamaClient::new(
            inner,
            cfg.base_url,
            bearer_token.map(|t| SecretString::new(t.into())),
            cfg.retry,
        );
        Ok(OllamaPlugin { client })
    }
}

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

> **Auth semantics differ from Anthropic**: `resolve_bearer_token`
> returns `Ok(None)` when neither config nor env provide a token (the
> common case for local Ollama). Anthropic returns `Err` because the
> API key is required.

### 6.3 `plugin.rs` — `OllamaPlugin` + `LlmBackend` impl

```rust
pub struct OllamaPlugin {
    client: OllamaClient,
}

impl LlmBackend for OllamaPlugin {
    fn name(&self) -> &str { "ollama" }

    async fn complete(
        &self,
        req: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        let body = build_chat_body(&req, false).map_err(|e| LlmError::Internal {
            message: format!("ollama: build request body: {e}"),
        })?;
        let resp = self.client.post_chat(&body, false).await.map_err(map_client_error)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(map_response_error(status, &body));
        }
        let body = resp.text().await.map_err(|e| LlmError::Internal {
            message: format!("ollama: read response body: {e}"),
        })?;
        parse_chat_response(&body).map_err(|e| LlmError::Internal {
            message: format!("ollama: parse response: {e}"),
        })
    }

    async fn stream(
        &self,
        req: CompletionRequest,
    ) -> Result<CompletionStream, LlmError> {
        let body = build_chat_body(&req, true).map_err(|e| LlmError::Internal {
            message: format!("ollama: build request body: {e}"),
        })?;
        let resp = self.client.post_chat(&body, true).await.map_err(map_client_error)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(map_response_error(status, &body));
        }
        parse_ndjson(resp).await
    }
}
```

### 6.4 `main.rs` — entrypoint

```rust
use ollama_plugin_lib::plugin::OllamaPlugin;
use tau_plugin_sdk::{run_llm_backend_with_config, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_llm_backend_with_config::<OllamaPlugin>(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    ).await
}
```

### 6.5 `tau.toml` — plugin manifest

```toml
name = "ollama"
version = "0.1.0"
description = "Ollama (local LLM runner) backend for tau."

[plugin]
provides = "llm_backend"
kind     = "rust-cargo"
bin      = "ollama-plugin"
```

### 6.6 Project tau.toml usage examples

**Local Ollama (no auth):**

```toml
[agents.local]
llm_backend = "ollama"
model = "llama3.2"
tools = []

[agents.local.config]
# All defaults work for local Ollama.
```

**Hosted Ollama (with bearer token):**

```toml
[agents.hosted]
llm_backend = "ollama"
model = "llama3.2:70b"

[agents.hosted.config]
base_url = "https://my-ollama-gateway.example.com"
# bearer_token_env defaults to OLLAMA_BEARER_TOKEN
```

**Slow model (long timeout, patient retries):**

```toml
[agents.slow]
llm_backend = "ollama"
model = "llama3.1:405b"

[agents.slow.config]
request_timeout_secs = 1800

[agents.slow.config.retry]
max_attempts = 5
base_delay_ms = 5000
```

---

## 7. Tool-use mapping

### 7.1 Request-side: `Vec<ToolSpec>` → Ollama `tools` array

```rust
fn translate_tool(spec: &ToolSpec) -> Result<serde_json::Value, BuildError> {
    Ok(serde_json::json!({
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": serde_json::to_value(&spec.input_schema)?,
        },
    }))
}
```

Edge cases:

- Empty `req.tools` → omit `tools` field entirely.
- `req.tool_choice == ToolChoice::None` → omit `tools` (and the
  unsupported `tool_choice` field).
- `req.tool_choice == Required | Specific` → `tracing::debug!` and
  drop (Ollama's `/api/chat` doesn't accept `tool_choice`).

Validation Ollama does (we don't duplicate):

- Tool-name format: implementation-dependent; Ollama may 400 on
  invalid names.
- Schema validity: Ollama may 400 on invalid JSON Schema.

### 7.2 Response-side: Ollama `message.tool_calls` → `Vec<ToolUse>`

```rust
parsed.message.tool_calls.unwrap_or_default()
    .into_iter()
    .enumerate()
    .map(|(i, tc)| {
        let id = tc.id.unwrap_or_else(|| format!("ollama-tool-{i}"));
        let input: tau_domain::Value = serde_json::from_value(tc.function.arguments)?;
        Ok(ToolUse::new(id, tc.function.name, input))
    })
    .collect()
```

### 7.3 Synthesized tool_call ids

Ollama's tool_calls don't always include an `id` field. The plugin
synthesizes deterministic ids per turn:

- For the batch path: `"ollama-tool-{index}"` based on position in the
  `tool_calls` array.
- For the streaming path: same pattern using a `tool_call_index`
  counter that increments per emitted ToolUse chunk.

The kernel's multi-turn loop uses this id to pair the eventual
`LlmProviderMessage::ToolResult { tool_use_id }` back. Determinism per
turn is required.

### 7.4 Multi-turn tool result echoes

When the agent runs a tool and feeds the result back,
`LlmProviderMessage::ToolResult { tool_use_id, content, is_error }`
translates to:

```json
{
  "role": "tool",
  "content": "<concatenated text from content blocks>"
}
```

Note: Ollama's tool message has **no `tool_use_id` field**. The kernel's
turn structure ensures the tool message follows the assistant's
tool_call message, and Ollama infers the pairing by order. The plugin
drops `tool_use_id` (documented in plugin README).

`is_error` is also dropped — Ollama's tool message doesn't carry an
error flag. Tools that report errors should encode them in the
`content` payload.

### 7.5 Validation we DON'T do

- Tool-name conformance — Ollama returns 400.
- `parameters` JSON Schema validity — Ollama returns 400.
- Tool-call id round-trip — Ollama doesn't track ids.

---

## 8. Testing tier (cassettes + live smoke)

### 8.1 Cassette catalog (9 files)

| Cassette | Scenario |
|---|---|
| `complete_happy_path.yaml` | Single-turn text response |
| `complete_with_system_prompt.yaml` | `req.system` → leading `role:system` message at index 0 of `messages` |
| `complete_with_tools.yaml` | Tools sent; assistant returns `tool_calls` → `resp.tool_uses` with synthesized id |
| `complete_503_model_loading_then_success.yaml` | 2× 503 ("model is loading") then 200 — exercises retry on Ollama's most common transient failure |
| `complete_404_model_not_pulled.yaml` | 404 with remediation hint in error message |
| `complete_400_bad_request.yaml` | 400 with structured error body |
| `stream_text_only.yaml` | NDJSON: 3× content delta lines + done:true |
| `stream_with_tool_use.yaml` | NDJSON: text deltas + tool_calls line + done:true |
| `stream_truncated_response.yaml` | NDJSON ends without `done:true` line → `LlmError::Stream` |

### 8.2 Cassette format (NDJSON streaming)

NDJSON cassettes use YAML's `|` block-literal (preserves trailing `\n`):

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

The hand-rolled cassette replayer (duplicated from Anthropic) is
wire-format-agnostic; it serves the body verbatim.

### 8.3 Cassette replayer

Verbatim copy of `crates/tau-plugins/anthropic/tests/common/cassette.rs`
(~250 LOC). No changes needed. Per Q2 decision: this duplication is
acceptable until rule-of-three triggers a refactor at sub-project 2c.

### 8.4 Test layout (`tests/complete.rs`)

```rust
#[tokio::test]
async fn complete_happy_path() { /* 200 + text response */ }

#[tokio::test]
async fn complete_with_system_prompt() {
    // Verify the request body the plugin sent contained
    // a {"role":"system","content":"..."} entry as messages[0].
    let received = server.received_requests();
    assert!(
        received[0].body.contains(r#""role":"system","content":"you are concise""#),
        "expected leading role:system message in request body",
    );
}

#[tokio::test]
async fn complete_with_tools() {
    // resp.tool_uses[0].id == "ollama-tool-0" (synthesized).
}

#[tokio::test]
async fn complete_503_model_loading_then_success_retries() { /* 3 attempts */ }

#[tokio::test]
async fn complete_404_model_not_pulled_includes_remediation_hint() {
    // Error message contains "ollama pull".
}

#[tokio::test]
async fn complete_400_bad_request_does_not_retry() { /* 1 attempt */ }
```

### 8.5 Streaming tests (`tests/streaming.rs`)

```rust
#[tokio::test]
async fn stream_text_only_yields_chunks_then_finish() { /* 2 Text + 1 Finish */ }

#[tokio::test]
async fn stream_with_tool_use_emits_full_tool_use_chunk() {
    // Synthesized id "ollama-tool-0"; input = Object{text:"hi"}.
}

#[tokio::test]
async fn stream_truncated_response_returns_stream_error() {
    // Last chunk asserts Err(LlmError::Stream { message: "...ended before done:true..." }).
}
```

### 8.6 Live smoke tests (`tests/live.rs`)

Always `#[ignore]`. Maintainer-triggered:

```bash
# One-time setup:
brew install ollama       # or curl install on Linux
ollama serve &
ollama pull llama3.2

# Run:
TAU_OLLAMA_LIVE_TESTS=1 cargo test -p ollama --test live -- --ignored --nocapture
```

```rust
#[tokio::test]
#[ignore = "live: requires TAU_OLLAMA_LIVE_TESTS=1 and a running Ollama instance"]
async fn live_complete_smoke() {
    if std::env::var("TAU_OLLAMA_LIVE_TESTS").is_err() { return; }
    let model = std::env::var("TAU_OLLAMA_LIVE_MODEL")
        .unwrap_or_else(|_| "llama3.2".into());
    /* hits http://localhost:11434/api/chat with model */
}

#[tokio::test]
#[ignore = "live: requires TAU_OLLAMA_LIVE_TESTS=1 and a running Ollama instance"]
async fn live_stream_smoke() { /* same shape, exercises NDJSON */ }
```

CI does NOT run live tests.

### 8.7 Re-record helper

`scripts/rerecord-ollama-cassettes.sh` — same shape as
`scripts/rerecord-anthropic-cassettes.sh`. v0.1: cassettes are
hand-authored; script informs operator that the live test suite is
the drift-detection mechanism.

### 8.8 Test surface summary

| Category | Count | Runtime |
|---|---|---|
| Unit tests | ~25 (config 9 + request 8 + response 5 + error 4 + client 5 + stream 6 + plugin 3) | <1s |
| Cassette integration tests (batch) | 6 | ~2s |
| Cassette integration tests (streaming) | 3 | ~1s |
| Live smoke tests (`#[ignore]`) | 2 | ~5–15s when run |
| **Total CI runtime** | **~37 active** | **~3s** |

---

## 9. Plan-erratum carryovers

Same set as sub-projects 1 + 2a, applied here:

- **Doctests on `#[non_exhaustive]` types must be `ignore`-marked**
  (E0639). `OllamaConfig`, `RetryConfig` get the gate.
- **`cargo test --all-targets` does NOT include doctests**: verify
  with `cargo test -p ollama --doc` separately.
- **Wire methods are `llm.complete` and `llm.stream`** — handled by
  the SDK; plugin code never names these strings directly.
- **`CompletionChunk::Finish` / `CompletionChunk::ToolUse(ToolUse)`**
  — tuple variant carrying a full `ToolUse`.
- **`req.system: Option<String>`** is a top-level field. For Ollama:
  prepend as a leading `role:system` message in the `messages` array
  (NOT a top-level `system` field — Ollama-specific).
- **`ToolChoice::Specific { name }`** — drop with debug warn (Ollama
  doesn't support tool_choice on `/api/chat`).
- **`ContentBlock::Text(String)`** is a tuple variant; flatten via
  string concatenation for Ollama's `content: String` shape.
- **`tau_ports::fixtures::make_completion_response`** for
  CompletionResponse construction (`#[non_exhaustive]`; no public
  constructor or Default impl).
- **`#[non_exhaustive]` cross-crate construction**: integration tests
  must use `Default::default()` + field assignment (not struct-literal
  `..Default::default()`).
- **NO new `Internal` / `Custom` error variants** — escape-hatch
  registry continues to gate.
- **`ConfigError::InvalidEnvVar`** (added in sub-project 2a) is
  reused for the bearer-token-env case; no SDK amendment needed.

### 9.1 ADR not required

Per §2: this sub-project is purely additive — new workspace crate, no
existing public API changes, no protocol changes, no new error
variants. The sub-project-local engineering decisions (endpoint, code
sharing, retry, testing) are not project-wide guideline changes.

If `LlmError` vocabulary expansion (`RateLimited`, `Auth`,
`ModelNotFound`) is needed, that's its own ADR-amendment paired with
sub-project 2c (OpenAI), where the third consumer establishes the
case for richer typed variants. Out of scope here.

---

## 10. Implementation plan outline (~15 tasks)

The plan derived from this spec follows the established cadence (one
Conventional Commits commit per task, full verification before
commit, push after each task, PR auto-triggers CI).

| # | Task | Files |
|---|---|---|
| 1 | Workspace scaffold: empty stub `crates/tau-plugins/ollama/{Cargo.toml,tau.toml,src/main.rs,src/lib.rs}`; register in workspace `Cargo.toml` `members`. **No new workspace deps** (all needed from sub-project 2a). | workspace + new crate |
| 2 | `config.rs`: `OllamaConfig` + `RetryConfig` + `resolve_bearer_token` + `validate_retry`; ~9 unit tests | `src/config.rs`, `src/lib.rs` |
| 3 | `request.rs`: `build_chat_body` + system-as-leading-`role:system` + tool/tool_choice translation (drop `Specific`/`Required` with warn) + `options` sub-object for sampling overrides; ~10 unit tests | `src/request.rs` |
| 4 | `response.rs`: `parse_chat_response` + tool_calls collection + tool-call-id synthesis + `done_reason` mapping; ~5 unit tests | `src/response.rs` |
| 5 | `error.rs`: `map_response_error` with 404-includes-`ollama pull`-hint + `map_client_error`; ~4 unit tests | `src/error.rs` |
| 6 | `client.rs`: `OllamaClient` + `post_chat` + retry loop with optional bearer-token Auth header (no `x-api-key`); ~5 unit tests via in-process `TcpListener` (one is the 503 retry path) | `src/client.rs` |
| 7 | `stream.rs`: NDJSON parser with line-buffered `split_terminator('\n')`; ~5 unit tests with hand-fed NDJSON streams | `src/stream.rs` |
| 8 | `plugin.rs` + `main.rs`: `OllamaPlugin` + `LlmBackend` impl + entrypoint; ~3 unit tests; remove `#![allow(dead_code)]` from prior modules | `src/plugin.rs`, `src/main.rs` |
| 9 | Cassette replayer + helpers (DUPLICATED from Anthropic): copy `cassette.rs` verbatim (~250 LOC); adapt `mod.rs` helpers to use `OllamaConfig` types | `tests/common/{mod.rs, cassette.rs}` |
| 10 | 6 batch cassettes + `tests/complete.rs`: happy_path, with_system_prompt (verify role:system at index 0), with_tools, 503_model_loading_then_success, 404_model_not_pulled, 400_bad_request | `tests/cassettes/*.yaml`, `tests/complete.rs` |
| 11 | 3 streaming cassettes + `tests/streaming.rs`: stream_text_only, stream_with_tool_use, stream_truncated_response | `tests/cassettes/stream_*.yaml`, `tests/streaming.rs` |
| 12 | Live smoke tests (`#[ignore]`-by-default) + re-record helper script | `tests/live.rs`, `scripts/rerecord-ollama-cassettes.sh` |
| 13 | CI: add `build (ollama-plugin)` job to ci.yml (release-build only) | `.github/workflows/ci.yml` |
| 14 | Final local verification + mark PR ready | (gate) |
| 15 | Plan sign-off + ROADMAP + branch protection update (16 → 17) + squash merge | (gate) |

15 tasks. Tasks 14–15 are user-driven gates per the established
pattern. **No ADR sign-off step**.

---

## 11. Out of scope (explicit deferrals)

| Topic | Where it lives |
|---|---|
| OpenAI-compat shim path | Sub-project 2c targets `/v1/chat/completions` directly |
| Embeddings (`/api/embeddings`) | Future sub-project (Storage-adjacent) |
| Model management (`/api/pull`, `/api/list`, `/api/show`) | Operator concern, not plugin |
| Multi-modal | Future when `ContentBlock::Image` lands |
| Structured outputs (`format` field) | Future plugin version |
| Generate API (`/api/generate`) | Q1 chose `/api/chat` |
| `LlmError::ModelNotFound` / `RateLimited` / `Auth` typed variants | Pair with sub-project 2c |
| Tool-call argument streaming | Future amendment if Ollama starts streaming fragments |
| Long-delay model-load 503 path | v0.1 uses standard backoff |

---

## 12. Cross-references

- [ADR-0008](../../decisions/0008-plugin-loading.md) — second real consumer.
- [Anthropic plugin spec](2026-04-29-anthropic-plugin-design.md) — pattern source; ~300 LOC duplicated.
- [Anthropic plugin plan](../plans/2026-04-29-anthropic-plugin.md) — execution model carried over.
- [ROADMAP](../../../ROADMAP.md) Phase 1 priority 2b — marked complete on sub-project sign-off.

## 13. Open follow-ups

- **Sub-project 2c — OpenAI plugin** (next Tier 1 priority).
- **Conformance suite** (`tau-plugin-conformance` per ADR-0008
  deferred items) — high-leverage with three real LLM-backend
  implementations.
- **Refactor common HTTP transport to a shared crate** — rule-of-three
  trigger; lift cassette replayer + retry client + ClientError.
- **`LlmError` vocabulary expansion** — `RateLimited`, `Auth`,
  `ModelNotFound`, `BadRequest` typed variants. Own ADR-amendment.
- **Long-delay 503 retry path** — if real-world model-load times bite,
  add a longer-delay path with a dedicated config knob.
