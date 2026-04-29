# Anthropic LLM-backend plugin — Phase 1 sub-project 2a

**Status:** Draft (this spec) → Implementation plan derived → no ADR
needed for this sub-project (purely additive — see §7).

**Sub-project scope:** Phase 1 priority 2a from the
[ROADMAP](../../../ROADMAP.md). The first real LLM-backend plugin;
validates the loading mechanism shipped in
[ADR-0008](../../decisions/0008-plugin-loading.md) end-to-end against
real network traffic, real authentication, and real provider error
envelopes. Ollama (priority 2b) and OpenAI (priority 2c) follow as
their own sub-projects.

---

## 1. Summary

ADR-0008 shipped the plugin loading mechanism with two toy plugins
(`echo-llm`, `echo-tool`) that prove the IPC + handshake + stream
plumbing works. This sub-project ships the **first real** LLM-backend
plugin: an Anthropic Claude Messages API client that:

- Speaks `tau_ports::LlmBackend` (native trait, IPC-erased through
  the existing `DynLlmBackend` shim).
- Authenticates via env-var or handshake-config-supplied API key.
- Translates `CompletionRequest` ↔ Anthropic Messages API JSON.
- Streams responses via SSE → `tau_ports::CompletionStream` (chunks
  yielded as `CompletionChunk::Text` and `CompletionChunk::Finish`).
- Handles tool-use round-trip (request `tools` array; response
  `tool_use` blocks; multi-turn `tool_result` echoes).
- Retries transient errors (429 / 503 / network timeouts) with
  exponential backoff that respects Anthropic's `Retry-After` header.
- Maps HTTP status + Anthropic error envelopes to the existing
  `LlmError` variants (`Internal`, `Stream`).
- Lives in-tree under `crates/tau-plugins/anthropic/` alongside the
  toy plugins.

### 1.1 Scope confirmed

**Ships:**

- One new workspace member: `crates/tau-plugins/anthropic/`.
- Real HTTP client backed by `reqwest`, `eventsource-stream`.
- Cassette-replay test harness (10 cassettes covering happy paths,
  retry, errors, streaming).
- 2 env-gated live smoke tests for drift detection.
- Plugin manifest (`tau.toml`) declaring `provides = "llm_backend"`.

**Does NOT ship:**

- Ollama or OpenAI plugins (priorities 2b, 2c — separate sub-projects).
- Vision / image inputs (Q7 — out-of-scope v0.1; additive future).
- Prompt caching (requires `TokenUsage` extension; own ADR).
- Citations, batches API, computer-use (Anthropic-specific niches).
- `LlmError::RateLimited` / `LlmError::Auth` variants (paired with
  whichever future sub-project — 2b or 2c — establishes the need for
  richer port-specific error vocabulary).
- Auto-reconnect mid-stream (Anthropic doesn't support resumption).
- Multi-vendor failover (belongs in agent loop, not plugin).
- Cost / token-pricing telemetry (belongs in observability; NG10).
- Anthropic SDK crate dependency (plugin owns its HTTP client to
  keep dep tree minimal + version-controlled).

### 1.2 Constitution alignment

| Constraint | This plugin's answer |
|---|---|
| `forbid(unsafe_code)` | Plain Rust; no FFI, no manual unsafe. |
| **G6** runtime not framework | Plugin is a thin translation layer; no abstractions over the kernel. |
| **G9** observable by default | All retries, request/response cycles, and stream events emit `tracing` events under `target = "anthropic_plugin::*"` re-emitted by the host. |
| **NG4** no marketplace | Plugin distributed in-tree for v0.1; expected to migrate to standalone repo once SDK contract has stabilized through 2-3 plugins. |
| **NG9** no credential management | Plugin reads API key from env var; never persists, never logs, never proxies. |
| **NG10** no telemetry | Tracing events are local; nothing leaves the user's machine except direct API calls to Anthropic. |

---

## 2. Decisions (pre-locked via brainstorm; no ADR needed)

This sub-project does NOT introduce its own ADR. Reasons:

1. **Purely additive** — new workspace crate; no existing API changes.
2. **No protocol changes** — uses ADR-0008's wire vocabulary unchanged.
3. **No new error variants** in tau-ports / tau-runtime / tau-plugin-sdk.
4. **No package manifest changes** — uses ADR-0008's `[plugin]` table.

The sub-project-local engineering decisions that the brainstorm settled:

| # | Decision | Rationale |
|---|---|---|
| 1 | **Provider:** Anthropic (Claude Messages API) | Cleanest API to map to tau-ports types; tool_use spec is the lingua franca; user has credentials; sets the bar for subsequent plugins |
| 2 | **Distribution:** in-tree under `crates/tau-plugins/anthropic/` | Co-located with `echo-llm`/`echo-tool` per ADR-0008 precedent; iteration on tau-plugin-sdk + plugin in lockstep; expected migration to standalone repo when SDK stabilizes |
| 3 | **API surface:** Messages API only | Completions API deprecated; Batches/Computer-Use out-of-scope |
| 4 | **Streaming:** day-1, full SSE + tool-use streaming | "Validates the mechanism end-to-end" implies stream-router from sub-project 1 (Task 16) gets exercised; existing `ToolUseAccumulator` (tau-ports) handles JSON-fragment accumulation |
| 5 | **Tool-use mapping:** direct (Anthropic ↔ tau-ports) | Anthropic's tool_use blocks map 1:1 to `ContentBlock::ToolUse` + `ToolUse`; `ToolChoice` maps directly |
| 6 | **System prompt:** extract `LlmProviderMessage::System` into Anthropic's top-level `system` field | Anthropic separates system from messages array; multiple System messages concatenated with `\n\n` |
| 7 | **Model selection:** pass-through, no validation | Plugin doesn't know about specific models; Anthropic's lineup churns; user picks model in `[agents.<id>] model = "..."` |
| 8 | **Vision / image content:** out-of-scope v0.1 | Additive future extension when tau-ports `ContentBlock::Image` lands |
| 9 | **Credentials:** env var `ANTHROPIC_API_KEY` (default); override via handshake `config.api_key_env` (different env name) or `config.api_key` (raw, test-only) | Standard env-var pattern; never logged; never persisted |
| 10 | **Token usage:** pass-through `usage.input_tokens`/`output_tokens` | Cache fields require `TokenUsage` extension (own ADR-amendment, deferred) |
| 11 | **Retry:** in-plugin, exponential, respects `Retry-After`, configurable | Anthropic 429s during peak hit constantly; without retry every multi-turn agent fails on first burst |
| 12 | **Retry exhaustion mapping:** `LlmError::Internal { message: "rate limited after N retries: ..." }` | tau-ports lacks `RateLimited` variant; ADR-amendment paired with second plugin |
| 13 | **Mid-stream errors do NOT retry** | Stream may have yielded usable chunks; Anthropic doesn't support resumption; retrying re-pays input tokens |
| 14 | **Testing:** VCR-style cassette replay + env-gated live smoke | Deterministic CI; reproducible tests; ~quarterly live smoke catches drift |
| 15 | **Cassette replayer:** hand-rolled (~200 LOC) if no maintained crate | Avoid unmaintained-dep risk for a small surface; migrate later if shared with Ollama/OpenAI plugins |
| 16 | **Error fidelity:** all 4xx/5xx collapse to `LlmError::Internal { message }` for v0.1 | Simplification; richer mapping when second plugin establishes need |
| 17 | **Validation we DO NOT do:** tool-name regex, input_schema validity, tool_use_id format | Anthropic enforces; duplicating it is brittle and forward-incompatible |

---

## 3. Architecture

### 3.1 Workspace layout

```
crates/tau-plugins/anthropic/
├── Cargo.toml                  -- bin target `anthropic-plugin`
├── tau.toml                    -- plugin manifest ([plugin] table)
├── src/
│   ├── main.rs                 -- #[tokio::main] entry → run_llm_backend_with_config
│   ├── plugin.rs               -- AnthropicPlugin struct, LlmBackend impl
│   ├── config.rs               -- AnthropicConfig + Configure impl + validation
│   ├── client.rs               -- HTTP client (reqwest), retry + auth headers
│   ├── request.rs              -- CompletionRequest → Anthropic JSON
│   ├── response.rs             -- Anthropic JSON → CompletionResponse
│   ├── stream.rs               -- SSE parser → CompletionStream
│   ├── tool_use.rs             -- Tool ↔ Anthropic tool_use translation
│   └── error.rs                -- HTTP + Anthropic error JSON → LlmError
└── tests/
    ├── cassettes/              -- VCR cassette YAMLs (10 files)
    ├── common/                 -- test helpers (cassette server, sample data)
    ├── complete.rs             -- batch-mode integration tests
    ├── streaming.rs            -- streaming integration tests
    └── live.rs                 -- env-gated smoke tests (#[ignore] by default)
```

### 3.2 Dependencies (additions to workspace)

| Dep | Why | Where |
|---|---|---|
| `reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls", "stream"] }` | HTTP client; rustls (no openssl); stream feature for SSE | new workspace dep |
| `eventsource-stream = "0.2"` | SSE event framing on top of `reqwest::Response` body | new workspace dep |
| `async-stream = "0.3"` | Macro for `try_stream!` blocks in `stream.rs` | new workspace dep |
| `secrecy = "0.10"` | `SecretString` to keep API keys out of `Debug` output | new workspace dep |

Workspace `tokio` already has the features needed (`macros`, `rt`, `rt-multi-thread`, `sync`).

### 3.3 Dataflow on `tau run reviewer "..."` against an Anthropic-backed agent

```
tau-cli (existing)
  └─ tau-runtime::plugin_host::load_llm_backend
      └─ spawns target/release/anthropic-plugin
          └─ tau-plugin-sdk::run_llm_backend_with_config::<AnthropicPlugin>
              ├─ handshake (carries config:
              │     { api_key_env: "ANTHROPIC_API_KEY", retry: { ... } })
              ├─ Configure::from_config → AnthropicPlugin { client: AnthropicClient }
              ├─ dispatch loop reads frames:
              │   ├─ llm.complete → AnthropicPlugin::complete
              │   │   ├─ request::build_messages_body(req, stream=false)
              │   │   ├─ client::post_messages(&body, stream=false)
              │   │   │   ├─ retry on 429/503/timeout
              │   │   │   └─ honor Retry-After header
              │   │   ├─ response::parse_messages_response(body)
              │   │   └─ → CompletionResponse, map error if non-2xx
              │   └─ llm.stream → AnthropicPlugin::stream
              │       ├─ request::build_messages_body(req, stream=true)
              │       ├─ client::post_messages(&body, stream=true)
              │       └─ stream::parse_sse(response) → CompletionStream
              │           (chunks yielded incrementally; tool_use accumulated
              │            via ToolUseAccumulator on content_block_stop)
              └─ frames out via stdout
```

The runtime receives `Arc<dyn DynLlmBackend>` (an `IpcLlmBackend` from
sub-project 1) wrapping the plugin process. All Phase 0 + Phase 1
sub-project 1 paths (capability filter, agent loop, tracing,
recording) continue to work unchanged.

---

## 4. HTTP layer (`client.rs`, `request.rs`, `response.rs`, `error.rs`)

### 4.1 `client.rs` — HTTP client with retry + auth

```rust
pub(crate) struct AnthropicClient {
    inner: reqwest::Client,
    base_url: String,                    // default https://api.anthropic.com
    api_key: SecretString,               // never Display-printed
    api_version: String,                 // "2023-06-01" baseline
    retry: RetryConfig,
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub(crate) struct RetryConfig {
    pub max_attempts: u32,               // default 3
    pub base_delay_ms: u64,              // default 1000
    pub respect_retry_after: bool,       // default true
}

impl AnthropicClient {
    pub(crate) async fn post_messages(
        &self,
        body: &serde_json::Value,
        stream: bool,
    ) -> Result<reqwest::Response, ClientError> {
        let url = format!("{}/v1/messages", self.base_url);
        let mut attempt = 0;
        loop {
            attempt += 1;
            let req = self.inner
                .post(&url)
                .header("x-api-key", self.api_key.expose_secret())
                .header("anthropic-version", &self.api_version)
                .header("content-type", "application/json")
                .json(body);
            let req = if stream {
                req.header("accept", "text/event-stream")
            } else { req };
            let res = req.send().await;

            match self.classify(res, attempt).await {
                Decision::Return(resp) => return Ok(resp),
                Decision::Error(err) => return Err(err),
                Decision::Retry { delay_ms } => {
                    tracing::warn!(
                        target: "anthropic_plugin::retry",
                        attempt, max = self.retry.max_attempts, delay_ms,
                        "retrying transient error",
                    );
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
            }
        }
    }
    // classify(), retry_delay(), backoff_only() per §2 design.
}

enum Decision {
    Return(reqwest::Response),
    Retry { delay_ms: u64 },
    Error(ClientError),
}
```

Retry decisions:

| Status / condition | Decision |
|---|---|
| 2xx | `Return(resp)` — caller handles |
| 429, 503 | `Retry` if attempts left; honor `Retry-After` if `respect_retry_after`; else exponential `base * 2^(n-1)` capped at 60s |
| 4xx (other than 429) | `Return(resp)` — caller maps to error; no retry |
| 5xx (other than 503) | `Retry` (treated as transient) |
| Network timeout | `Retry` |
| Other transport error | `Error(Transport)` — no retry on connection-refused etc. |
| Retries exhausted on retryable status | `Error(Exhausted)` — caller maps to `LlmError::Internal` |

### 4.2 `request.rs` — `CompletionRequest` → Anthropic Messages JSON

```rust
pub(crate) fn build_messages_body(
    req: &CompletionRequest,
    stream: bool,
) -> Result<serde_json::Value, BuildError> {
    let (system, user_messages) = split_system_and_user(&req.messages);
    let mut body = serde_json::json!({
        "model": req.model,
        "messages": user_messages.iter().map(translate_message).collect::<Vec<_>>(),
        "max_tokens": req.max_tokens.unwrap_or(4096),
    });

    if !system.is_empty() {
        body["system"] = serde_json::Value::String(system.join("\n\n"));
    }
    if !req.tools.is_empty() {
        body["tools"] = serde_json::Value::Array(
            req.tools.iter().map(translate_tool).collect()
        );
        body["tool_choice"] = translate_tool_choice(&req.tool_choice);
    }
    if stream {
        body["stream"] = serde_json::Value::Bool(true);
    }
    Ok(body)
}
```

Per-message translation (see also §5):

```rust
fn translate_message(msg: &LlmProviderMessage) -> serde_json::Value {
    match msg {
        LlmProviderMessage::User { text } => serde_json::json!({
            "role": "user",
            "content": text,
        }),
        LlmProviderMessage::Assistant { content } => serde_json::json!({
            "role": "assistant",
            "content": content.iter().map(translate_content_block).collect::<Vec<_>>(),
        }),
        LlmProviderMessage::ToolResult { tool_use_id, content, is_error } =>
            translate_tool_result(tool_use_id, content, *is_error),
        LlmProviderMessage::System { .. } =>
            unreachable!("extracted by split_system_and_user"),
    }
}
```

### 4.3 `response.rs` — Anthropic Messages JSON → `CompletionResponse`

```rust
pub(crate) fn parse_messages_response(
    body: &str,
) -> Result<CompletionResponse, ParseError> {
    let parsed: AnthropicMessagesResponse = serde_json::from_str(body)?;
    Ok(CompletionResponse {
        content: parsed.content.into_iter()
            .filter_map(map_content_block)
            .collect(),
        stop_reason: map_stop_reason(parsed.stop_reason),
        usage: tau_ports::TokenUsage::new(
            parsed.usage.input_tokens,
            parsed.usage.output_tokens,
        ),
    })
}

fn map_content_block(block: AnthropicContentBlock) -> Option<ContentBlock> {
    match block {
        AnthropicContentBlock::Text { text } => Some(ContentBlock::Text { text }),
        AnthropicContentBlock::ToolUse { id, name, input } => {
            Some(ContentBlock::ToolUse {
                id, name, input: serde_json_to_tau_value(input),
            })
        }
        AnthropicContentBlock::Unknown { r#type } => {
            tracing::warn!(
                target: "anthropic_plugin::content",
                block_type = %r#type,
                "dropped unknown content block type — plugin needs upgrade",
            );
            None
        }
    }
}

fn map_stop_reason(s: String) -> StopReason {
    match s.as_str() {
        "end_turn" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        // Forward-compat: unknown → safe default + warn
        other => {
            tracing::warn!(
                target: "anthropic_plugin::response",
                stop_reason = other,
                "unknown stop_reason; defaulting to EndTurn",
            );
            StopReason::EndTurn
        }
    }
}
```

### 4.4 `error.rs` — HTTP + Anthropic JSON → `LlmError`

```rust
pub(crate) fn map_response_error(
    status: reqwest::StatusCode,
    body: &str,
) -> LlmError {
    let parsed: Option<AnthropicErrorBody> = serde_json::from_str(body).ok();
    let detail = parsed.as_ref()
        .map(|p| format!("{}: {}", p.error.r#type, p.error.message))
        .unwrap_or_else(|| body.to_string());

    let category = match status.as_u16() {
        400 => "bad request",
        401 | 403 => "auth failure",
        404 => "not found",
        429 => "rate limited (retries exhausted)",
        500..=599 => "server error",
        _ => "unexpected status",
    };
    LlmError::Internal {
        message: format!("anthropic {category} ({status}): {detail}"),
    }
}

#[derive(serde::Deserialize)]
struct AnthropicErrorBody {
    r#type: String,        // "error"
    error: AnthropicErrorDetail,
}

#[derive(serde::Deserialize)]
struct AnthropicErrorDetail {
    r#type: String,        // "rate_limit_error", etc.
    message: String,
}
```

---

## 5. Streaming (`stream.rs`)

### 5.1 SSE event vocabulary

Anthropic's streaming wire format is Server-Sent Events
(`Content-Type: text/event-stream`). Event types the plugin parses:

```
event: message_start
data: {"type":"message_start","message":{"id":"...","model":"...",
       "usage":{"input_tokens":N,"output_tokens":N}}}

event: content_block_start
data: {"type":"content_block_start","index":0,
       "content_block":{"type":"text","text":""}
        | {"type":"tool_use","id":"...","name":"...","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,
       "delta":{"type":"text_delta","text":"..."}
        | {"type":"input_json_delta","partial_json":"..."}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"...","stop_sequence":null},
       "usage":{"output_tokens":N}}

event: message_stop
data: {"type":"message_stop"}

event: ping
data: {"type":"ping"}

event: error
data: {"type":"error","error":{"type":"...","message":"..."}}
```

### 5.2 Parser state machine

```rust
pub(crate) async fn parse_sse(
    body: reqwest::Response,
) -> Result<CompletionStream, LlmError> {
    let bytes_stream = body.bytes_stream().map(|r| {
        r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    });
    let mut events = bytes_stream.eventsource();
    let mut blocks: HashMap<u64, BlockState> = HashMap::new();
    let mut final_stop: Option<StopReason> = None;
    let mut final_usage = TokenUsage::new(0, 0);

    let stream = async_stream::try_stream! {
        while let Some(event_res) = events.next().await {
            let event = event_res.map_err(map_sse_error)?;
            let payload: AnthropicEvent = serde_json::from_str(&event.data)
                .map_err(|e| LlmError::Stream {
                    message: format!("event decode: {e}"),
                })?;

            match payload {
                AnthropicEvent::MessageStart { message } => {
                    final_usage = TokenUsage::new(
                        message.usage.input_tokens,
                        message.usage.output_tokens,
                    );
                }
                AnthropicEvent::ContentBlockStart { index, content_block } => {
                    blocks.insert(index, BlockState::from_start(content_block));
                }
                AnthropicEvent::ContentBlockDelta { index, delta } => {
                    let block = blocks.get_mut(&index).ok_or_else(|| {
                        LlmError::Stream {
                            message: format!("delta for unknown block index {index}"),
                        }
                    })?;
                    match (block, delta) {
                        (BlockState::Text(buf), Delta::TextDelta { text }) => {
                            buf.push_str(&text);
                            yield CompletionChunk::Text { delta: text };
                        }
                        (BlockState::ToolUse(acc), Delta::InputJsonDelta { partial_json }) => {
                            acc.append(&partial_json);
                        }
                        _ => yield Err(LlmError::Stream {
                            message: "delta/block kind mismatch".into(),
                        })?,
                    }
                }
                AnthropicEvent::ContentBlockStop { index } => {
                    if let Some(BlockState::ToolUse(acc)) = blocks.remove(&index) {
                        let tool_use = acc.finalize_with(|s| {
                            serde_json::from_str::<tau_domain::Value>(s)
                                .map_err(|e| e.to_string())
                        }).map_err(|e| LlmError::Stream {
                            message: format!("tool_use json: {e}"),
                        })?;
                        yield CompletionChunk::ToolUseDelta { tool_use };
                    }
                }
                AnthropicEvent::MessageDelta { delta, usage } => {
                    final_stop = Some(map_stop_reason(&delta.stop_reason));
                    final_usage = TokenUsage::new(
                        final_usage.input_tokens,
                        usage.output_tokens,
                    );
                }
                AnthropicEvent::MessageStop => {
                    yield CompletionChunk::Finish {
                        stop_reason: final_stop.clone().unwrap_or(StopReason::EndTurn),
                        usage: final_usage.clone(),
                    };
                    return;
                }
                AnthropicEvent::Ping => { /* heartbeat — ignore */ }
                AnthropicEvent::Error { error } => {
                    yield Err(LlmError::Stream {
                        message: format!(
                            "anthropic stream error ({}): {}",
                            error.r#type, error.message,
                        ),
                    })?;
                }
            }
        }
    };
    Ok(Box::pin(stream))
}

enum BlockState {
    Text(String),
    ToolUse(tau_ports::ToolUseAccumulator),
}

impl BlockState {
    fn from_start(b: AnthropicContentBlockStart) -> Self {
        match b {
            AnthropicContentBlockStart::Text { .. } => BlockState::Text(String::new()),
            AnthropicContentBlockStart::ToolUse { id, name, .. } => {
                BlockState::ToolUse(tau_ports::ToolUseAccumulator::new(id, name))
            }
        }
    }
}
```

> **Implementation note:** the `CompletionChunk::ToolUseDelta` variant
> name and constructor signature must be verified against tau-ports
> at impl time. If only `Text` and `Finish` variants exist (per Task
> 16's reading of tau-ports), tool-use accumulation folds into the
> agent's eventual `tool_use` block via the host's stream consumer
> rather than being a per-tool-use chunk on the wire.

### 5.3 Mid-stream errors do not retry

When Anthropic sends `event: error` mid-stream (overload, downstream
issues), the parser yields `Err(LlmError::Stream { ... })` and
terminates. The host's `stream_router::assemble` (sub-project 1, Task
16) propagates this as the final error item to the runtime.

The retry layer in `client.rs` only retries on **the initial
request** before any bytes have been consumed. Once a 200 OK status
arrives and SSE parsing begins, mid-stream errors do **not** restart
the request:

1. The stream may have already yielded usable chunks to the agent.
2. Anthropic doesn't support stream resumption (no idempotent token).
3. Retrying mid-stream re-pays input tokens.

### 5.4 Chunk emission model

| SSE event | Yielded chunk |
|---|---|
| `text_delta` (per `content_block_delta`) | `CompletionChunk::Text { delta }` |
| `input_json_delta` (per `content_block_delta`) | nothing (accumulated) |
| `content_block_stop` for tool_use block | `CompletionChunk::ToolUseDelta { tool_use }` (single full ToolUse) |
| `message_stop` | `CompletionChunk::Finish { stop_reason, usage }` |
| `event: error` | terminate with `Err(LlmError::Stream)` |
| `event: ping` | ignored |
| Unknown event | logged via `tracing::warn!`, ignored |

---

## 6. Configuration shape (`config.rs`) + plugin entry

### 6.1 `AnthropicConfig` + `Configure` impl

```rust
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnthropicConfig {
    /// Override env var name for the API key. Default:
    /// `ANTHROPIC_API_KEY`.
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,

    /// Direct API key override. Test-only; never in production tau.toml.
    #[serde(default)]
    pub api_key: Option<String>,

    /// Override base URL. Default: https://api.anthropic.com.
    /// Tests use this to point at the cassette replayer.
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// Anthropic API version header. Default: "2023-06-01".
    #[serde(default = "default_api_version")]
    pub api_version: String,

    /// Per-request HTTP timeout. Default: 600s.
    #[serde(default = "default_request_timeout_secs")]
    #[serde(deserialize_with = "deserialize_duration_secs")]
    pub request_timeout: Duration,

    /// Retry behavior. Defaults match the spec §Q8.
    #[serde(default)]
    pub retry: RetryConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetryConfig {
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
| `api_key_env` | `"ANTHROPIC_API_KEY"` |
| `api_key` | `None` |
| `base_url` | `"https://api.anthropic.com"` |
| `api_version` | `"2023-06-01"` |
| `request_timeout` | `600s` (Anthropic streaming can run minutes) |
| `retry.max_attempts` | `3` |
| `retry.base_delay_ms` | `1000` |
| `retry.respect_retry_after` | `true` |

### 6.2 Validation in `Configure::from_config`

```rust
impl Configure for AnthropicPlugin {
    type Config = AnthropicConfig;

    fn from_config(cfg: Self::Config) -> Result<Self, ConfigError> {
        // 1. Resolve api_key (priority: explicit > env var).
        let api_key = if let Some(direct) = cfg.api_key {
            tracing::warn!(
                target: "anthropic_plugin::config",
                "config.api_key set directly — recommended only for tests",
            );
            direct
        } else {
            std::env::var(&cfg.api_key_env).map_err(|_| {
                ConfigError::InvalidValue {
                    field: "api_key_env",
                    detail: format!(
                        "env var {} is not set; \
                         set it or use config.api_key (test-only)",
                        cfg.api_key_env,
                    ),
                }
            })?
        };

        // 2. Sanity-check API key shape.
        if !api_key.starts_with("sk-ant-") {
            return Err(ConfigError::InvalidValue {
                field: "api_key",
                detail: "Anthropic API keys start with `sk-ant-`".into(),
            });
        }

        // 3. Validate retry config.
        if cfg.retry.max_attempts == 0 {
            return Err(ConfigError::InvalidValue {
                field: "retry.max_attempts",
                detail: "must be >= 1 (use 1 for no-retry semantics)".into(),
            });
        }

        // 4. Build the HTTP client.
        let inner = reqwest::Client::builder()
            .timeout(cfg.request_timeout)
            .user_agent("tau-anthropic-plugin/0.1.0")
            .build()
            .map_err(|e| ConfigError::InvalidValue {
                field: "request_timeout",
                detail: format!("could not build HTTP client: {e}"),
            })?;

        let client = AnthropicClient {
            inner,
            base_url: cfg.base_url,
            api_key: SecretString::new(api_key),
            api_version: cfg.api_version,
            retry: cfg.retry,
        };
        Ok(AnthropicPlugin { client })
    }
}
```

> **Plan-erratum note**: `ConfigError::InvalidValue` takes
> `field: &'static str`. Constructing it with a runtime env-var name
> (Anthropic API key env can be customized) requires either
> `Box::leak` or extending `ConfigError` to take `String`. The plan
> resolves this at impl time; preferred: extend `ConfigError` to
> accept `String` (additive ADR-amendment to tau-plugin-sdk; see §7).

### 6.3 `plugin.rs` — `AnthropicPlugin` + `LlmBackend` impl

```rust
pub(crate) struct AnthropicPlugin {
    client: AnthropicClient,
}

impl tau_ports::LlmBackend for AnthropicPlugin {
    fn name(&self) -> &str { "anthropic" }

    async fn complete(
        &self,
        req: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        let body = build_messages_body(&req, false)
            .map_err(|e| LlmError::Internal {
                message: format!("build request: {e}"),
            })?;
        let resp = self.client.post_messages(&body, false).await
            .map_err(map_client_error)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(map_response_error(status, &body));
        }
        let body = resp.text().await
            .map_err(|e| LlmError::Internal {
                message: format!("read body: {e}"),
            })?;
        parse_messages_response(&body)
            .map_err(|e| LlmError::Internal {
                message: format!("parse response: {e}"),
            })
    }

    async fn stream(
        &self,
        req: CompletionRequest,
    ) -> Result<CompletionStream, LlmError> {
        let body = build_messages_body(&req, true)
            .map_err(|e| LlmError::Internal {
                message: format!("build request: {e}"),
            })?;
        let resp = self.client.post_messages(&body, true).await
            .map_err(map_client_error)?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(map_response_error(status, &body));
        }
        parse_sse(resp).await
    }
}
```

### 6.4 `main.rs` — entrypoint

```rust
mod client;
mod config;
mod error;
mod plugin;
mod request;
mod response;
mod stream;
mod tool_use;

use tau_plugin_sdk::{run_llm_backend_with_config, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_llm_backend_with_config::<plugin::AnthropicPlugin>(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    ).await
}
```

### 6.5 `tau.toml` — plugin manifest

```toml
name = "anthropic"
version = "0.1.0"
description = "Anthropic Claude (Messages API) backend for tau."

[plugin]
provides = "llm_backend"
kind     = "rust-cargo"
bin      = "anthropic-plugin"
```

### 6.6 Project tau.toml usage example

```toml
[agents.reviewer]
llm_backend = "anthropic"
model = "claude-3-5-sonnet-latest"
tools = ["fs-read"]

[agents.reviewer.config]
# api_key_env defaults to ANTHROPIC_API_KEY; uncomment to override:
# api_key_env = "MY_ORG_ANTHROPIC_KEY"

[agents.reviewer.config.retry]
max_attempts = 5            # default 3
base_delay_ms = 2000        # default 1000
```

---

## 7. Tool-use mapping (`tool_use.rs`)

### 7.1 Request-side: `Vec<ToolSpec>` → Anthropic `tools[]`

```rust
fn translate_tool(spec: &ToolSpec) -> serde_json::Value {
    serde_json::json!({
        "name": spec.name,
        "description": spec.description,
        "input_schema": spec.parameters_json,
    })
}
```

Validation Anthropic does (we don't duplicate):

- `name` regex: `^[a-zA-Z0-9_-]{1,64}$`
- `input_schema` must be a valid JSON Schema with `type: "object"` at root

If `req.tools` is empty, omit the `tools` field from the body
entirely (Anthropic rejects empty arrays).

### 7.2 Request-side: `ToolChoice` → Anthropic `tool_choice`

| `ToolChoice` | Anthropic shape |
|---|---|
| `Auto` | `{"type": "auto"}` |
| `Required` | `{"type": "any"}` |
| `ForceTool { name }` | `{"type": "tool", "name": name}` |
| `None` | omit `tool_choice` (and `tools`) |

### 7.3 Response-side: Anthropic content blocks → `ContentBlock`

| Anthropic block | tau-ports |
|---|---|
| `{"type": "text", "text": "..."}` | `ContentBlock::Text { text }` |
| `{"type": "tool_use", "id", "name", "input"}` | `ContentBlock::ToolUse { id, name, input }` |
| Unknown `type` | `tracing::warn!` and drop |

### 7.4 Response-side: tool_use streaming

Per §5.2; tool_use blocks accumulate JSON via
`ToolUseAccumulator` and yield as `CompletionChunk::ToolUseDelta` (or
fold into the final `Finish` chunk's content list — verify variant
at impl time).

### 7.5 Multi-turn tool result echoes

```rust
fn translate_tool_result(
    tool_use_id: &str,
    content: &ToolResultContent,
    is_error: bool,
) -> serde_json::Value {
    serde_json::json!({
        "role": "user",
        "content": [{
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": match content {
                ToolResultContent::Text(s) => serde_json::Value::String(s.clone()),
            },
            "is_error": is_error,
        }],
    })
}
```

Rich content arrays (text + image) are out-of-scope per Q7; additive
future extension when `ContentBlock::Image` lands.

### 7.6 Validation we DON'T do

- Tool-name uniqueness within a request → Anthropic returns 400
- Tool-name regex conformance → Anthropic returns 400
- `input_schema` validity → Anthropic returns 400
- `tool_use_id` format → opaque to plugin; pass through as-is

---

## 8. Testing tier (cassettes + live smoke)

### 8.1 Cassette catalog (10 files)

| Cassette | Scenario |
|---|---|
| `complete_happy_path.yaml` | Single-turn text response |
| `complete_with_system_prompt.yaml` | System prompt extraction → top-level `system` |
| `complete_with_tools.yaml` | Tools sent; assistant returns `tool_use` block |
| `complete_429_then_success.yaml` | 2× 429 with `retry-after: 0` then 200 |
| `complete_429_exhausted.yaml` | 3× 429 → retry exhaustion `LlmError::Internal` |
| `complete_401_auth_failure.yaml` | 401 invalid_request_error |
| `complete_400_bad_request.yaml` | 400 with structured error body |
| `stream_text_only.yaml` | SSE: 3× text_delta + message_stop |
| `stream_with_tool_use.yaml` | SSE: text_delta + tool_use deltas + JSON accumulation |
| `stream_error_mid_stream.yaml` | SSE: 1× text_delta + `event: error` overload |

Total cassette size: ~50 KB.

### 8.2 Cassette format

```yaml
- request:
    method: POST
    uri: https://api.anthropic.com/v1/messages
    headers:
      x-api-key: "<REDACTED>"
      anthropic-version: "2023-06-01"
      content-type: application/json
    body: |
      {
        "model": "claude-3-5-haiku-latest",
        "messages": [{"role":"user","content":"say hi in 2 words"}],
        "max_tokens": 1024
      }
  response:
    status: 200
    headers:
      content-type: application/json
    body: |
      { "id":"msg_01ABC", "type":"message", "role":"assistant",
        "content":[{"type":"text","text":"Hi there"}],
        "model":"claude-3-5-haiku-latest",
        "stop_reason":"end_turn",
        "usage":{"input_tokens":12,"output_tokens":3} }
```

### 8.3 Cassette replayer

Hand-rolled (~200 LOC) HTTP server that serves recorded responses in
order. Lives in `tests/common/cassette.rs`. Method:

```rust
pub(crate) async fn replay(path: &str) -> CassetteServer {
    let cassette = parse_yaml_cassette(path);
    let server = CassetteServer::start(cassette).await;
    server
}

pub struct CassetteServer {
    base_url: String,
    received_requests: Arc<Mutex<Vec<RecordedRequest>>>,
    /* ... */
}

impl CassetteServer {
    pub fn uri(&self) -> &str { &self.base_url }
    pub fn received_requests(&self) -> Vec<RecordedRequest> { /* ... */ }
}
```

### 8.4 Test layout

```rust
// tests/complete.rs
#[tokio::test]
async fn complete_happy_path() {
    let server = cassette::replay("tests/cassettes/complete_happy_path.yaml").await;
    let plugin = AnthropicPlugin::new(test_config(server.uri()));
    let resp = plugin.complete(common::sample_request()).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Hi there");
    assert_eq!(resp.usage.output_tokens, 3);
}

#[tokio::test]
async fn complete_429_then_success() {
    let server = cassette::replay("tests/cassettes/complete_429_then_success.yaml").await;
    let plugin = AnthropicPlugin::new(test_config_with_retry(server.uri(), 3, 0));
    let resp = plugin.complete(common::sample_request()).await.unwrap();
    assert_eq!(server.received_requests().len(), 3);
}

#[tokio::test]
async fn complete_401_auth_failure_does_not_retry() {
    let server = cassette::replay("tests/cassettes/complete_401_auth_failure.yaml").await;
    let plugin = AnthropicPlugin::new(test_config_with_retry(server.uri(), 3, 0));
    let err = plugin.complete(common::sample_request()).await.unwrap_err();
    assert!(matches!(
        err,
        LlmError::Internal { ref message } if message.contains("auth failure")
    ));
    assert_eq!(server.received_requests().len(), 1);
}
```

### 8.5 Live smoke tests (`tests/live.rs`)

```rust
//! Run with:
//!   TAU_ANTHROPIC_LIVE_TESTS=1 ANTHROPIC_API_KEY=sk-ant-... \
//!     cargo test -p anthropic --test live -- --ignored
//! Costs: ~$0.001 per smoke run on claude-3-5-haiku-latest.

#[tokio::test]
#[ignore = "live: requires TAU_ANTHROPIC_LIVE_TESTS=1"]
async fn live_complete_smoke() {
    if std::env::var("TAU_ANTHROPIC_LIVE_TESTS").is_err() { return; }
    let plugin = AnthropicPlugin::new(AnthropicConfig {
        api_key: Some(std::env::var("ANTHROPIC_API_KEY").unwrap()),
        base_url: "https://api.anthropic.com".into(),
        ..AnthropicConfig::default()
    });
    let req = CompletionRequest {
        model: "claude-3-5-haiku-latest".into(),
        messages: vec![user_msg("say hi in 2 words")],
        max_tokens: Some(20),
        ..Default::default()
    };
    let resp = plugin.complete(req).await.unwrap();
    assert!(!extract_text(&resp).is_empty());
}

#[tokio::test]
#[ignore = "live: requires TAU_ANTHROPIC_LIVE_TESTS=1"]
async fn live_stream_smoke() { /* same shape, exercises SSE */ }
```

CI does not run live tests. Maintainer-triggered, ~quarterly cadence.

### 8.6 Cassette re-recording protocol

```bash
#!/usr/bin/env bash
# scripts/rerecord-anthropic-cassettes.sh
# Costs ~$0.05 per full re-record.
set -e
: "${ANTHROPIC_API_KEY:?required}"
export TAU_RECORD_CASSETTES=1
cargo test -p anthropic --test complete -- --nocapture
cargo test -p anthropic --test streaming -- --nocapture
echo "diff cassettes; review + commit:"
git diff crates/tau-plugins/anthropic/tests/cassettes/
```

The replayer's record mode is gated on `TAU_RECORD_CASSETTES=1`;
default is replay-only.

### 8.7 Unit tests in source files

For pure logic that doesn't need HTTP:

- `request::tests::extracts_system_prompt` — `LlmProviderMessage::System` extraction
- `request::tests::omits_tools_array_when_empty`
- `request::tests::translates_tool_choice_force_tool`
- `response::tests::parses_text_block`
- `response::tests::parses_tool_use_block`
- `response::tests::maps_unknown_stop_reason_to_safe_default`
- `error::tests::maps_429_to_rate_limited_internal`
- `error::tests::auth_failure_does_not_retry`
- `stream::tests::accumulates_tool_use_input_json` — feed canned SSE events
- `stream::tests::propagates_mid_stream_error_event`

### 8.8 Test surface summary

| Category | Count | Runtime |
|---|---|---|
| Unit tests | ~10 | <1s |
| Cassette integration tests (batch) | 7 | ~2s |
| Cassette integration tests (streaming) | 3 | ~1s |
| Live smoke tests (`#[ignore]`) | 2 | ~2s when run |
| **Total CI runtime** | **~22 active** | **~3s** |

---

## 9. Plan-erratum carryovers

Same set as the prior sub-projects, applied to this plugin:

- **Doctests on `#[non_exhaustive]` types must be `ignore`-marked**
  (E0639). `AnthropicConfig`, `RetryConfig` get the gate; tau-ports
  types' doctests are already handled in their crate.
- **`cargo test --all-targets` does NOT include doctests**: verify
  with `cargo test -p anthropic --doc` separately.
- **Wire methods are `llm.complete` and `llm.stream`** (not
  `llm.complete_streaming` — sub-project 1 plan-erratum). The SDK's
  `run_llm_backend_with_config` handles dispatch; plugin code never
  names these strings directly.
- **`CompletionChunk::Finish`** (not `Done`).
- **`Tool::invoke` / `Tool::schema`** (not `call` / `spec`) — irrelevant
  to this plugin (it produces `ContentBlock::ToolUse`, doesn't impl
  `Tool`); listed for completeness.
- **`tau-ports` `serde` feature** is enabled via
  `tau-ports = { workspace = true, features = ["serde", "test-fixtures"] }`
  in the plugin's `Cargo.toml`. Sub-project 1 established this.
- **NO new `Internal` / `Custom` error variants** ship in this
  sub-project. All errors map through existing typed variants
  (`LlmError::Internal { message }`, `LlmError::Stream { message }`,
  `ConfigError::*`, `SdkError::*`). The escape-hatch registry test at
  `crates/tau-domain/tests/escape_hatch_registry.rs` continues to gate.
- **`ConfigError::InvalidValue { field: &'static str }`** is a known
  pinch-point for the customizable-env-var-name use case. Resolution:
  small additive amendment to `tau-plugin-sdk::ConfigError` to accept
  `String` for the `field` parameter (or add a new variant
  `ConfigError::InvalidEnvVar { name: String }`). Decided at impl time;
  the spec leans toward the variant addition since it's purer.

### 9.1 ADR not required

Per §2: this sub-project is purely additive — new workspace crate, no
existing public API changes, no protocol changes, no new error
variants beyond what ADR-0008 covers. The sub-project-local
engineering decisions (provider, distribution, retry, testing) are
not project-wide guideline changes.

If the implementation discovers that richer `LlmError` variants are
needed (`RateLimited`, `Auth`), that's its own ADR-amendment to
ADR-0006 (tau-runtime kernel) when sub-project 2b/2c establishes the
case for vocabulary expansion. Out of scope here.

---

## 10. Implementation plan outline (~15 tasks)

The plan derived from this spec follows the established cadence (one
Conventional Commits commit per task, full verification before
commit, push after each task, PR auto-triggers CI).

| # | Task | Files |
|---|---|---|
| 1 | Workspace scaffold: empty stub `crates/tau-plugins/anthropic/{Cargo.toml,tau.toml,src/main.rs}`; register in workspace `Cargo.toml` `members`; add new workspace deps (reqwest, eventsource-stream, async-stream, secrecy) | workspace + new crate |
| 2 | `config.rs`: `AnthropicConfig` + `RetryConfig` + `Configure` impl + 4 unit tests; `ConfigError` extension (additive variant or `String` field) | `src/config.rs`, `tau-plugin-sdk/src/configure.rs` |
| 3 | `request.rs`: body builder + 6 unit tests (system extraction, tools omitted, tool_choice variants, etc.) | `src/request.rs` |
| 4 | `response.rs`: parser + 4 unit tests | `src/response.rs` |
| 5 | `error.rs`: `map_response_error` + Anthropic error JSON struct + 3 unit tests | `src/error.rs` |
| 6 | `client.rs`: HTTP client + retry loop + `Retry-After` honoring + 3 unit tests | `src/client.rs` |
| 7 | `stream.rs`: SSE parser + `BlockState` machine + tool-use accumulation + 5 unit tests with hand-fed SSE | `src/stream.rs` |
| 8 | `plugin.rs`: `AnthropicPlugin` + `LlmBackend` impl; `main.rs` entrypoint | `src/plugin.rs`, `src/main.rs` |
| 9 | Cassette replayer: ~200 LOC HTTP server in `tests/common/cassette.rs` | `tests/common/{mod,cassette}.rs` |
| 10 | Cassette files (7 batch) + `tests/complete.rs` integration tests | `tests/cassettes/*.yaml`, `tests/complete.rs` |
| 11 | Cassette files (3 streaming) + `tests/streaming.rs` integration tests | `tests/cassettes/stream_*.yaml`, `tests/streaming.rs` |
| 12 | Live smoke tests (`#[ignore]`) + re-record helper script | `tests/live.rs`, `scripts/rerecord-anthropic-cassettes.sh` |
| 13 | CI: add `build (anthropic-plugin)` job to ci.yml (release-build only — no integration tests; those run in workspace test job) | `.github/workflows/ci.yml` |
| 14 | Final local verification + mark PR ready | (gate) |
| 15 | Plan sign-off + ROADMAP update + branch protection (1 new check) + squash merge | (gate) |

15 tasks. Tasks 14 + 15 are user-driven gates per the established
pattern. **No ADR sign-off step** — this sub-project introduces no
ADR.

---

## 11. Out of scope (explicit deferrals)

Items NOT in this sub-project, tracked so they aren't lost:

| Topic | Where it lives |
|---|---|
| Vision / image content blocks | Phase 1 backlog when `ContentBlock::Image` lands in tau-ports |
| Prompt caching | Own ADR (extends `TokenUsage`); future plugin version |
| Citations | Anthropic-specific; not in tau-ports vocabulary |
| Batches API | Async batch processing; separate use case |
| Computer Use | Agentic browser/computer control; separate sub-project |
| Multi-vendor failover | Belongs in agent loop, not plugin |
| `LlmError::RateLimited` / `LlmError::Auth` variants | Pair with sub-project 2b/2c when second plugin establishes need |
| Auto-reconnect mid-stream | Anthropic doesn't support resumption |
| Cost / token-pricing telemetry | Belongs in tau observability layer (NG10) |
| Anthropic SDK crate wrapper | Plugin owns its HTTP client to keep dep tree small |

---

## 12. Cross-references

- [ADR-0008](../../decisions/0008-plugin-loading.md) — this plugin is
  the first real consumer of the loading mechanism.
- [ROADMAP](../../../ROADMAP.md) Phase 1 priority 2 — marked
  complete on sub-project sign-off; priority 2b (Ollama) becomes the
  natural next sub-project.
- [Sub-project 1 plan](../plans/2026-04-28-plugin-loading.md) —
  plan-erratum carryovers from there apply here (see §9).
- [Sub-project 1 spec](2026-04-28-plugin-loading-design.md) — the
  protocol vocabulary this plugin uses (post the §5 sync from the
  spec hygiene clean-up).

## 13. Open follow-ups

- **Sub-project 2b: Ollama plugin** — natural next, gives the SDK
  its second real consumer + validates OpenAI-compat-style endpoints.
- **Sub-project 2c: OpenAI plugin** — completes the Tier-1 trio.
- **Conformance suite** (`tau-plugin-conformance` per ADR-0008
  deferred items) — becomes valuable once 2-3 LLM-backend plugins
  exist.
- **`LlmError` vocabulary expansion** — Anthropic + Ollama + OpenAI
  together establish the case for `RateLimited`, `Auth`,
  `BadRequest`, `Server` typed variants. Own ADR-amendment to
  ADR-0006.
- **Cost/observability layer** — usage tracking across providers;
  NG10-respectful (local-only).
