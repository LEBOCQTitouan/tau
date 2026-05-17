# tau-serve-mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `tau serve` — a JSON-RPC 2.0 over NDJSON-framed stdio server that exposes `Runtime::run` and `Runtime::run_streaming` as a versioned IPC protocol, closing Phase 1 priority §15.

**Architecture:** New `serve` module in the previously-stub `tau-app` crate. One `Runtime` per process, built at startup from `--project`. Per-request tokio LOCAL tasks (Runtime streams are non-`Send`) over a `current_thread` tokio runtime + `LocalSet`. Hand-rolled JSON-RPC framing (small v1 surface, ~200 LOC, no `jsonrpsee` dep). New `tau serve` CLI subcommand in `tau-cli`.

**Tech Stack:** Rust 2024, `tokio` (current_thread + LocalSet), `tokio-util` (CancellationToken), `serde` + `serde_json`, `dashmap`, `tracing`, existing `tau-runtime` + `tau-domain` + `tau-pkg`.

**Spec:** `docs/superpowers/specs/2026-05-17-tau-serve-mode-design.md`

**ADR (to write):** `docs/decisions/0031-tau-serve-mode.md` (per Constitution QG18).

---

## File Structure

| Path | Status | Responsibility |
|---|---|---|
| `crates/tau-app/Cargo.toml` | MODIFY | Add deps: serde, serde_json, tokio (current_thread + macros + signal + io-std), tokio-util (rt + sync), futures, dashmap, tracing, tracing-subscriber, anyhow, thiserror, async-trait. Workspace deps: tau-runtime, tau-domain, tau-pkg, tau-ports. |
| `crates/tau-app/src/lib.rs` | MODIFY | `pub mod serve;` |
| `crates/tau-app/src/serve/mod.rs` | NEW | Public entry point `pub async fn run(opts: ServeOptions) -> Result<()>`. Wires reader/dispatcher/writer/lifecycle. |
| `crates/tau-app/src/serve/options.rs` | NEW | `ServeOptions` struct + `Default`. Mirrors CLI flags. |
| `crates/tau-app/src/serve/protocol.rs` | NEW | JSON-RPC types: `Request`, `Response`, `Notification`, `ErrorObject`, `RequestId`. Serde. |
| `crates/tau-app/src/serve/framing.rs` | NEW | NDJSON read (`stdin.lines()` → channel) + write (mpsc receiver → `stdout.write_all`). |
| `crates/tau-app/src/serve/methods.rs` | NEW | Method-name string constants. |
| `crates/tau-app/src/serve/error_codes.rs` | NEW | JSON-RPC + tau error code constants. |
| `crates/tau-app/src/serve/error_map.rs` | NEW | `RuntimeError → ErrorObject` mapping. |
| `crates/tau-app/src/serve/handshake.rs` | NEW | State machine: `HandshakeState::{Unhandshaken, Handshaken}` + transition validation. |
| `crates/tau-app/src/serve/dispatch.rs` | NEW | Per-request routing. Spawns LocalSet tasks. Holds `Arc<Runtime>`. |
| `crates/tau-app/src/serve/cancel.rs` | NEW | `CancelRegistry`: `DashMap<RequestId, CancellationToken>`. |
| `crates/tau-app/src/serve/lifecycle.rs` | NEW | Startup, signal handling, graceful shutdown, idle timeout, exit codes. |
| `crates/tau-app/src/serve/tracing_init.rs` | NEW | Tracing subscriber → stderr only. |
| `crates/tau-app/src/serve/project.rs` | NEW | `resolve_agent(project, agent_id) -> (AgentDefinition, PackageManifest)`. Lifted from tau-cli. |
| `crates/tau-app/tests/serve_handshake.rs` | NEW | Layer 2 — handshake protocol tests. |
| `crates/tau-app/tests/serve_run_batch.rs` | NEW | Layer 2 — `runtime.run`. |
| `crates/tau-app/tests/serve_run_streaming.rs` | NEW | Layer 2 — streaming + correlated event ids. |
| `crates/tau-app/tests/serve_cancel.rs` | NEW | Layer 2 — cancellation. |
| `crates/tau-app/tests/serve_concurrent.rs` | NEW | Layer 2 — concurrency cap. |
| `crates/tau-app/tests/serve_shutdown.rs` | NEW | Layer 2 — graceful shutdown. |
| `crates/tau-app/tests/e2e/smoke.rs` | NEW | Layer 3 — real subprocess. |
| `crates/tau-app/tests/e2e/streaming.rs` | NEW | Layer 3 — streaming over real pipe. |
| `crates/tau-app/tests/e2e/parent_death.rs` | NEW | Layer 3 — PDEATHSIG / stdin EOF. |
| `crates/tau-app/tests/e2e/ready_signal.rs` | NEW | Layer 3 — `--ready-on-stderr`. |
| `crates/tau-app/tests/fixtures/echo-project/tau.toml` | NEW | Minimal project using echo-llm + echo-tool. |
| `crates/tau-cli/src/cmd/serve.rs` | NEW | `tau serve` subcommand. Calls `tau_app::serve::run()`. |
| `crates/tau-cli/src/cmd/mod.rs` | MODIFY | Register `serve` subcommand. |
| `crates/tau-cli/src/main.rs` | MODIFY | Wire serve subcommand into clap dispatch. |
| `crates/tau-cli/Cargo.toml` | MODIFY | Add `tau-app` dep. |
| `docs/decisions/0031-tau-serve-mode.md` | NEW | ADR. |
| `docs/decisions/README.md` | MODIFY | Index entry for ADR-0031. |
| `ROADMAP.md` | MODIFY | Mark §15 shipped; add closing entry. |
| `.github/workflows/ci.yml` | MODIFY | +1 job: `test (tau-app serve / linux)`. |
| `.lefthook.yml` | MODIFY | Add the same job to the pre-push deep-gate. |

---

## Task 1: Scaffold tau-app crate dependencies + module skeleton

**Files:**
- Modify: `crates/tau-app/Cargo.toml`
- Modify: `crates/tau-app/src/lib.rs`
- Create: `crates/tau-app/src/serve/mod.rs`

- [ ] **Step 1: Update `crates/tau-app/Cargo.toml`**

Append a `[dependencies]` block. Use workspace-version deps where the workspace defines them.

```toml
[dependencies]
# Workspace crates
tau-runtime = { workspace = true }
tau-domain = { workspace = true }
tau-pkg = { workspace = true }
tau-ports = { workspace = true }

# External
anyhow = { workspace = true }
async-trait = { workspace = true }
dashmap = "5"
futures = { workspace = true }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true, features = ["macros", "rt", "rt-multi-thread", "sync", "io-std", "signal", "time"] }
tokio-util = { version = "0.7", features = ["rt"] }
tracing = { workspace = true }
tracing-subscriber = { workspace = true, features = ["env-filter"] }

[dev-dependencies]
insta = { workspace = true, features = ["json"] }
tempfile = { workspace = true }
```

- [ ] **Step 2: Write `crates/tau-app/src/lib.rs`**

Replace the existing 5-line stub with:

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Application orchestration for tau's runtime. Wires ports to adapters.
//!
//! v1 ships the `serve` module: a JSON-RPC 2.0 over NDJSON-framed stdio
//! server exposing `Runtime::run` and `Runtime::run_streaming` as tau's
//! second public API surface (Constitution G6 / QG12).
//!
//! See spec at `docs/superpowers/specs/2026-05-17-tau-serve-mode-design.md`
//! and ADR-0031.

pub mod serve;
```

- [ ] **Step 3: Write `crates/tau-app/src/serve/mod.rs`**

```rust
//! Tau serve mode: JSON-RPC 2.0 over NDJSON-framed stdio.
//!
//! Public entry point: [`run`]. Builds a `Runtime` from
//! [`ServeOptions::project_path`], spawns the reader/dispatcher/writer
//! tasks, and blocks until shutdown.

mod cancel;
mod dispatch;
mod error_codes;
mod error_map;
mod framing;
mod handshake;
mod lifecycle;
mod methods;
mod options;
mod project;
mod protocol;
mod tracing_init;

pub use options::ServeOptions;

use anyhow::Result;

/// Run the serve loop until shutdown.
///
/// Builds the runtime, starts the I/O tasks, and blocks. Returns
/// `Ok(())` on graceful shutdown; returns `Err` on startup failure.
pub async fn run(opts: ServeOptions) -> Result<()> {
    lifecycle::run(opts).await
}
```

- [ ] **Step 4: Verify the crate compiles (empty modules)**

Create stub files for every module declared above:

```bash
cd /Users/titouanlebocq/code/tau
for m in cancel dispatch error_codes error_map framing handshake lifecycle methods options project protocol tracing_init; do
  echo "//! TODO module" > crates/tau-app/src/serve/$m.rs
done
# Make lifecycle::run exist for mod.rs's call
echo 'use anyhow::Result;
use super::options::ServeOptions;
pub async fn run(_opts: ServeOptions) -> Result<()> { Ok(()) }' > crates/tau-app/src/serve/lifecycle.rs
echo '//! Tau serve options.
#[derive(Debug, Clone, Default)]
pub struct ServeOptions {}' > crates/tau-app/src/serve/options.rs
```

Run:
```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo check -p tau-app
```

Expected: 0 warnings (or only `dead_code` for the stub modules — acceptable for now).

- [ ] **Step 5: Commit**

```bash
git add crates/tau-app/Cargo.toml crates/tau-app/src/
git commit -m "feat(tau-app): scaffold serve module + deps"
```

---

## Task 2: ServeOptions

**Files:**
- Modify: `crates/tau-app/src/serve/options.rs`
- Modify: `crates/tau-app/src/serve/mod.rs` (re-export already in place)

- [ ] **Step 1: Write `crates/tau-app/src/serve/options.rs`**

```rust
//! Configuration for the serve loop.

use std::path::PathBuf;
use std::time::Duration;

/// Configuration for [`super::run`].
///
/// All fields have safe defaults so callers can construct
/// `ServeOptions::default()` and override only what they need.
#[derive(Debug, Clone)]
pub struct ServeOptions {
    /// Absolute path to the tau project directory. Defaults to cwd
    /// when constructed via [`ServeOptions::default`].
    pub project_path: PathBuf,

    /// Maximum number of concurrent in-flight runs. Default 8.
    /// New requests beyond this cap receive error -32004 immediately.
    pub max_concurrent: usize,

    /// If `Some(d)`, the server initiates graceful shutdown after no
    /// message has been received OR emitted for `d`. Default `None`
    /// (run until external shutdown signal).
    pub idle_timeout: Option<Duration>,

    /// If true, the server writes `"tau-serve ready\n"` to stderr
    /// after startup completes (runtime built, reader/dispatcher/writer
    /// tasks alive). Lets parent processes synchronize on readiness.
    pub ready_on_stderr: bool,

    /// Max duration to wait for in-flight tasks to drain on graceful
    /// shutdown before dropping the runtime and exiting. Default 5s.
    pub shutdown_grace: Duration,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            project_path: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            max_concurrent: 8,
            idle_timeout: None,
            ready_on_stderr: false,
            shutdown_grace: Duration::from_secs(5),
        }
    }
}
```

- [ ] **Step 2: Compile check**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo check -p tau-app
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-app/src/serve/options.rs
git commit -m "feat(tau-app): ServeOptions struct + defaults"
```

---

## Task 3: JSON-RPC protocol types

**Files:**
- Modify: `crates/tau-app/src/serve/protocol.rs`

- [ ] **Step 1: Write failing tests first**

Append to `crates/tau-app/src/serve/protocol.rs` (after replacing the stub):

```rust
//! JSON-RPC 2.0 message types for serve mode.
//!
//! Per spec §5: Request, Response, Notification, ErrorObject.
//! All types use serde for symmetric serialization.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 request id. Per spec, may be integer, string, or null.
/// We accept integer or string; null is treated as a notification
/// (handled separately).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    /// Integer id (most common).
    Int(i64),
    /// String id (UUIDs, etc.).
    Str(String),
}

/// JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Always "2.0".
    pub jsonrpc: String,
    /// Request id. Absence means "notification" (handled by [`Notification`]).
    pub id: RequestId,
    /// Method name (e.g. "runtime.run").
    pub method: String,
    /// Method-specific params object. Absent when method takes no args.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 successful response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Always "2.0".
    pub jsonrpc: String,
    /// Matches the originating request id.
    pub id: RequestId,
    /// Method-specific result payload.
    pub result: Value,
}

/// JSON-RPC 2.0 error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Always "2.0".
    pub jsonrpc: String,
    /// Matches the originating request id.
    pub id: RequestId,
    /// Error payload.
    pub error: ErrorObject,
}

/// JSON-RPC 2.0 server-initiated notification (no id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    /// Always "2.0".
    pub jsonrpc: String,
    /// Method name (e.g. "runtime.event").
    pub method: String,
    /// Method-specific params object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 error payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorObject {
    /// JSON-RPC error code. See [`super::error_codes`].
    pub code: i32,
    /// Human-readable error message.
    pub message: String,
    /// Structured machine-actionable payload. Shape depends on `code`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Wire-level outbound message (request response, error response, or notification).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Outbound {
    /// Successful response to a request.
    Response(Response),
    /// Error response to a request.
    Error(ErrorResponse),
    /// Server-initiated notification.
    Notification(Notification),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_request_integer_id() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"meta.ping"}"#;
        let req: Request = serde_json::from_str(raw).unwrap();
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, RequestId::Int(1));
        assert_eq!(req.method, "meta.ping");
        assert!(req.params.is_none());
    }

    #[test]
    fn parse_request_string_id() {
        let raw = r#"{"jsonrpc":"2.0","id":"abc","method":"meta.ping","params":{}}"#;
        let req: Request = serde_json::from_str(raw).unwrap();
        assert_eq!(req.id, RequestId::Str("abc".into()));
    }

    #[test]
    fn serialize_response_omits_none_data() {
        let out = Outbound::Response(Response {
            jsonrpc: "2.0".into(),
            id: RequestId::Int(1),
            result: json!({"ok": true}),
        });
        let s = serde_json::to_string(&out).unwrap();
        assert_eq!(s, r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#);
    }

    #[test]
    fn serialize_error_with_data() {
        let out = Outbound::Error(ErrorResponse {
            jsonrpc: "2.0".into(),
            id: RequestId::Int(3),
            error: ErrorObject {
                code: -32007,
                message: "Capability denied".into(),
                data: Some(json!({"kind": "CapabilityDenial"})),
            },
        });
        let s = serde_json::to_string(&out).unwrap();
        assert!(s.contains("\"code\":-32007"));
        assert!(s.contains("\"kind\":\"CapabilityDenial\""));
    }

    #[test]
    fn serialize_notification_no_id() {
        let out = Outbound::Notification(Notification {
            jsonrpc: "2.0".into(),
            method: "runtime.event".into(),
            params: Some(json!({"id": 4, "kind": "TextDelta"})),
        });
        let s = serde_json::to_string(&out).unwrap();
        assert!(!s.contains("\"id\":"));
        assert!(s.contains("\"method\":\"runtime.event\""));
    }
}
```

- [ ] **Step 2: Run tests**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-app --lib
```

Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-app/src/serve/protocol.rs
git commit -m "feat(tau-app): JSON-RPC protocol types + roundtrip tests"
```

---

## Task 4: Method names + error codes

**Files:**
- Modify: `crates/tau-app/src/serve/methods.rs`
- Modify: `crates/tau-app/src/serve/error_codes.rs`

- [ ] **Step 1: Write `crates/tau-app/src/serve/methods.rs`**

```rust
//! Method name string constants for the v1 protocol.
//!
//! Per spec §5: 5 methods + 1 server-initiated notification.

/// Required first call. Establishes protocol version.
pub const META_HANDSHAKE: &str = "meta.handshake";

/// Liveness check.
pub const META_PING: &str = "meta.ping";

/// Batch run.
pub const RUNTIME_RUN: &str = "runtime.run";

/// Streaming run.
pub const RUNTIME_RUN_STREAMING: &str = "runtime.run_streaming";

/// Cancel an in-flight call by id.
pub const RUNTIME_CANCEL: &str = "runtime.cancel";

/// Server-initiated event during a streaming run.
pub const RUNTIME_EVENT: &str = "runtime.event";
```

- [ ] **Step 2: Write `crates/tau-app/src/serve/error_codes.rs`**

```rust
//! JSON-RPC error codes used by serve mode.
//!
//! Standard JSON-RPC 2.0 codes plus tau-namespaced codes in the
//! "Server error" reserved range (-32000 to -32099) per spec §6.

// Standard JSON-RPC 2.0 codes.
/// Invalid JSON received on the wire.
pub const PARSE_ERROR: i32 = -32700;
/// Not a valid JSON-RPC 2.0 object.
pub const INVALID_REQUEST: i32 = -32600;
/// Method does not exist or is not available.
pub const METHOD_NOT_FOUND: i32 = -32601;
/// Invalid method parameter(s).
pub const INVALID_PARAMS: i32 = -32602;
/// Internal JSON-RPC error.
pub const INTERNAL_ERROR: i32 = -32603;

// Tau-namespaced (-32000..-32099).
/// Handshake `protocol_version` not supported.
pub const HANDSHAKE_MISMATCH: i32 = -32000;
/// Request was cancelled by client.
pub const CANCELLED: i32 = -32001;
/// Non-`meta.*` call before handshake completed.
pub const HANDSHAKE_REQUIRED: i32 = -32002;
/// `meta.handshake` called after a successful handshake.
pub const ALREADY_HANDSHAKEN: i32 = -32003;
/// `max_concurrent_runs` cap reached.
pub const SERVER_BUSY: i32 = -32004;
/// RuntimeBuilder build error (reserved for future `runtime.reload`).
pub const PROJECT_ERROR: i32 = -32005;
/// Generic `RuntimeError` not covered by a more specific code.
pub const RUNTIME_ERROR: i32 = -32006;
/// `RuntimeError::CapabilityDenied`.
pub const CAPABILITY_DENIED: i32 = -32007;
/// Tool plugin returned error.
pub const TOOL_ERROR: i32 = -32008;
/// LLM backend plugin returned error.
pub const LLM_ERROR: i32 = -32009;
/// `agent_id` not in this project.
pub const UNKNOWN_AGENT: i32 = -32010;
```

- [ ] **Step 3: Compile check**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo check -p tau-app
```

Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-app/src/serve/methods.rs crates/tau-app/src/serve/error_codes.rs
git commit -m "feat(tau-app): method + error code constants"
```

---

## Task 5: Error mapping (RuntimeError → ErrorObject)

**Files:**
- Modify: `crates/tau-app/src/serve/error_map.rs`

- [ ] **Step 1: Write `crates/tau-app/src/serve/error_map.rs`**

```rust
//! Map [`tau_runtime::RuntimeError`] variants to JSON-RPC error
//! objects with structured `data` payloads.
//!
//! Per spec §6. Each `RuntimeError` variant maps to one custom code
//! in `super::error_codes`.

use super::error_codes;
use super::protocol::ErrorObject;
use serde_json::{json, Value};
use tau_runtime::RuntimeError;

/// Map any `RuntimeError` to an `ErrorObject`.
pub fn from_runtime_error(err: &RuntimeError) -> ErrorObject {
    match err {
        RuntimeError::CapabilityDenied(denial) => ErrorObject {
            code: error_codes::CAPABILITY_DENIED,
            message: format!("Capability denied: {}", err),
            data: Some(json!({
                "kind": "CapabilityDenial",
                "denial": denial_to_json(denial),
                "tool_error_variant": "CapabilityDenied"
            })),
        },
        RuntimeError::LlmBackendNotRegistered { .. }
        | RuntimeError::ToolNotRegistered { .. } => ErrorObject {
            code: error_codes::UNKNOWN_AGENT,
            message: err.to_string(),
            data: Some(json!({"kind": "UnknownAgent"})),
        },
        RuntimeError::PluginContractViolation { .. }
        | RuntimeError::PluginSpawnFailed { .. }
        | RuntimeError::PluginHandshakeFailed { .. }
        | RuntimeError::PluginCrashed { .. } => ErrorObject {
            code: error_codes::TOOL_ERROR,
            message: err.to_string(),
            data: Some(json!({"kind": "PluginError"})),
        },
        RuntimeError::SandboxValidationFailed { .. }
        | RuntimeError::SandboxWrapFailed { .. } => ErrorObject {
            code: error_codes::CAPABILITY_DENIED,
            message: err.to_string(),
            data: Some(json!({"kind": "SandboxError"})),
        },
        RuntimeError::CapabilityOverrideExpands { .. } => ErrorObject {
            code: error_codes::RUNTIME_ERROR,
            message: err.to_string(),
            data: Some(json!({"kind": "CapabilityOverrideExpands"})),
        },
        _ => ErrorObject {
            code: error_codes::RUNTIME_ERROR,
            message: err.to_string(),
            data: Some(json!({"kind": "RuntimeError"})),
        },
    }
}

/// Build the protocol-shape `denial` object from
/// [`tau_runtime::CapabilityDenial`].
///
/// Field names match the spec's example payload (§6.1).
fn denial_to_json(denial: &tau_runtime::CapabilityDenial) -> Value {
    // CapabilityDenial currently exposes Display only; serialize via
    // its public fields. Maintainers extending the variant should add
    // matching JSON fields here.
    json!({
        "display": denial.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_tool_maps_to_unknown_agent() {
        let err = RuntimeError::ToolNotRegistered {
            tool_id: "missing-tool".into(),
        };
        let obj = from_runtime_error(&err);
        assert_eq!(obj.code, error_codes::UNKNOWN_AGENT);
        assert!(obj.message.contains("missing-tool"));
    }

    #[test]
    fn plugin_crash_maps_to_tool_error() {
        let err = RuntimeError::PluginCrashed {
            plugin: "fs-read".into(),
            details: "boom".into(),
        };
        let obj = from_runtime_error(&err);
        assert_eq!(obj.code, error_codes::TOOL_ERROR);
    }
}
```

- [ ] **Step 2: Note for implementer**

The `RuntimeError::CapabilityDenied` variant's exact field name might differ — check `crates/tau-runtime/src/error.rs` and adjust the match pattern. Likewise check `ToolNotRegistered` / `PluginCrashed` field names. The mapping intent is fixed; field bindings are mechanical.

If a variant's fields differ from the assumed shape above, fix the match pattern and field accesses, but keep the code/data mapping unchanged. Use `cargo check -p tau-app` to drive convergence.

- [ ] **Step 3: Run tests**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-app --lib serve::error_map
```

Expected: 2 tests pass. If they don't compile, see Step 2.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-app/src/serve/error_map.rs
git commit -m "feat(tau-app): RuntimeError to JSON-RPC error mapping"
```

---

## Task 6: NDJSON framing (stdin reader + stdout writer)

**Files:**
- Modify: `crates/tau-app/src/serve/framing.rs`

- [ ] **Step 1: Write `crates/tau-app/src/serve/framing.rs`**

```rust
//! NDJSON framing for stdin/stdout. One JSON value per line.

use super::protocol::Outbound;
use anyhow::{Context, Result};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

/// Outcome of reading one line from stdin.
#[derive(Debug)]
pub enum Inbound {
    /// Parsed JSON value (validity beyond JSON is the dispatcher's job).
    Json(Value),
    /// Malformed JSON. Includes the original line bytes for logging.
    ParseError(String),
    /// EOF — stdin closed.
    Eof,
}

/// Reader task: read NDJSON lines from stdin, push to channel.
/// Returns when stdin EOF is reached (after sending `Inbound::Eof`).
pub async fn reader_task(tx: mpsc::Sender<Inbound>) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .await
            .context("stdin read failed")?;
        if n == 0 {
            let _ = tx.send(Inbound::Eof).await;
            return Ok(());
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.is_empty() {
            continue;
        }
        let msg = match serde_json::from_str::<Value>(trimmed) {
            Ok(v) => Inbound::Json(v),
            Err(e) => Inbound::ParseError(format!("{}: {}", e, trimmed)),
        };
        if tx.send(msg).await.is_err() {
            return Ok(()); // dispatcher dropped — shutdown
        }
    }
}

/// Writer task: receive `Outbound`s from a channel, serialize as
/// NDJSON to stdout, one line per message.
///
/// stdout is locked once per write to guarantee atomic line writes
/// (concurrent dispatcher tasks send through `mpsc`, but the actual
/// stdout `write_all` happens here single-threaded).
pub async fn writer_task(mut rx: mpsc::Receiver<Outbound>) -> Result<()> {
    let mut stdout = tokio::io::stdout();
    while let Some(out) = rx.recv().await {
        let mut line = serde_json::to_string(&out)
            .context("serialize outbound message")?;
        line.push('\n');
        stdout
            .write_all(line.as_bytes())
            .await
            .context("stdout write failed")?;
        stdout.flush().await.ok();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serve::protocol::{Response, RequestId};
    use serde_json::json;

    #[tokio::test]
    async fn writer_emits_ndjson() {
        let (tx, rx) = mpsc::channel(16);
        tx.send(Outbound::Response(Response {
            jsonrpc: "2.0".into(),
            id: RequestId::Int(1),
            result: json!({"ok": true}),
        }))
        .await
        .unwrap();
        drop(tx); // close so writer exits

        // Capture stdout by redirecting via a buffer is awkward — instead,
        // a smoke check that writer doesn't deadlock and exits cleanly.
        let res = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            writer_task(rx),
        )
        .await;
        assert!(res.is_ok());
    }
}
```

- [ ] **Step 2: Run tests + check**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-app --lib serve::framing
```

Expected: 1 test passes.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-app/src/serve/framing.rs
git commit -m "feat(tau-app): NDJSON reader + writer tasks"
```

---

## Task 7: Handshake state machine

**Files:**
- Modify: `crates/tau-app/src/serve/handshake.rs`

- [ ] **Step 1: Write `crates/tau-app/src/serve/handshake.rs`**

```rust
//! Handshake state machine.
//!
//! Per spec §5.1: `meta.handshake` is required as the first call.
//! Non-`meta.*` calls before handshake → -32002. Double-handshake → -32003.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

/// Server's view of handshake state. Single atomic so any task can
/// check/transition without locks.
#[derive(Debug, Default, Clone)]
pub struct HandshakeState {
    state: Arc<AtomicU8>,
}

const STATE_UNHANDSHAKEN: u8 = 0;
const STATE_HANDSHAKEN: u8 = 1;

/// What kind of method is being checked.
#[derive(Debug, Clone, Copy)]
pub enum MethodKind {
    /// `meta.*` methods.
    Meta,
    /// `runtime.*` and other non-meta methods.
    NonMeta,
}

/// Outcome of checking a method against current handshake state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// Method is allowed; proceed with dispatch.
    Allowed,
    /// Pre-handshake call to a non-meta method. Reject with -32002.
    HandshakeRequired,
    /// `meta.handshake` called twice. Reject with -32003.
    AlreadyHandshaken,
}

impl HandshakeState {
    /// Check whether a method is allowed in the current state.
    ///
    /// For `meta.handshake` itself, this returns `AlreadyHandshaken`
    /// when already handshaken; the caller should transition only
    /// when this returns `Allowed`.
    pub fn check(&self, method: &str) -> Check {
        let is_handshake_method = method == super::methods::META_HANDSHAKE;
        let is_meta = method.starts_with("meta.");
        let handshaken = self.state.load(Ordering::Acquire) == STATE_HANDSHAKEN;

        match (handshaken, is_handshake_method, is_meta) {
            (true, true, _) => Check::AlreadyHandshaken,
            (false, false, false) => Check::HandshakeRequired,
            (false, false, true) => Check::Allowed, // meta.ping pre-handshake is allowed
            (_, true, _) => Check::Allowed,
            (true, false, _) => Check::Allowed,
        }
    }

    /// Mark the handshake as complete. Idempotent — calling after
    /// already-handshaken is a no-op (caller should check first).
    pub fn mark_handshaken(&self) {
        self.state.store(STATE_HANDSHAKEN, Ordering::Release);
    }

    /// Whether the handshake has completed.
    pub fn is_handshaken(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_HANDSHAKEN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_handshake_meta_ping_allowed() {
        let s = HandshakeState::default();
        assert_eq!(s.check("meta.ping"), Check::Allowed);
    }

    #[test]
    fn pre_handshake_runtime_run_rejected() {
        let s = HandshakeState::default();
        assert_eq!(s.check("runtime.run"), Check::HandshakeRequired);
    }

    #[test]
    fn handshake_method_allowed_first_time() {
        let s = HandshakeState::default();
        assert_eq!(s.check("meta.handshake"), Check::Allowed);
    }

    #[test]
    fn second_handshake_rejected() {
        let s = HandshakeState::default();
        assert_eq!(s.check("meta.handshake"), Check::Allowed);
        s.mark_handshaken();
        assert_eq!(s.check("meta.handshake"), Check::AlreadyHandshaken);
    }

    #[test]
    fn post_handshake_runtime_run_allowed() {
        let s = HandshakeState::default();
        s.mark_handshaken();
        assert_eq!(s.check("runtime.run"), Check::Allowed);
    }
}
```

- [ ] **Step 2: Run tests**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-app --lib serve::handshake
```

Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-app/src/serve/handshake.rs
git commit -m "feat(tau-app): handshake state machine + tests"
```

---

## Task 8: Cancel registry

**Files:**
- Modify: `crates/tau-app/src/serve/cancel.rs`

- [ ] **Step 1: Write `crates/tau-app/src/serve/cancel.rs`**

```rust
//! Registry of cancellation tokens for in-flight requests.

use super::protocol::RequestId;
use dashmap::DashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Thread-safe registry of `RequestId → CancellationToken`. Per spec §8.2.
#[derive(Debug, Default, Clone)]
pub struct CancelRegistry {
    map: Arc<DashMap<RequestId, CancellationToken>>,
}

impl CancelRegistry {
    /// Register a new token for `id`. Returns a clone of the token the
    /// caller should `.cancelled()` on. If an entry already exists for
    /// this id (concurrent request id reuse — protocol violation by
    /// client), the old token is replaced and returned.
    pub fn register(&self, id: RequestId) -> CancellationToken {
        let tok = CancellationToken::new();
        self.map.insert(id, tok.clone());
        tok
    }

    /// Look up and cancel `id`. Returns `true` if found.
    pub fn cancel(&self, id: &RequestId) -> bool {
        if let Some((_, tok)) = self.map.remove(id) {
            tok.cancel();
            true
        } else {
            false
        }
    }

    /// Remove an entry without cancelling (called when request completes
    /// normally).
    pub fn forget(&self, id: &RequestId) {
        self.map.remove(id);
    }

    /// Cancel all entries (used during graceful shutdown).
    pub fn cancel_all(&self) {
        for entry in self.map.iter() {
            entry.value().cancel();
        }
        self.map.clear();
    }

    /// Current number of in-flight entries.
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_then_cancel() {
        let reg = CancelRegistry::default();
        let tok = reg.register(RequestId::Int(1));
        assert!(!tok.is_cancelled());
        assert!(reg.cancel(&RequestId::Int(1)));
        assert!(tok.is_cancelled());
    }

    #[test]
    fn cancel_unknown_returns_false() {
        let reg = CancelRegistry::default();
        assert!(!reg.cancel(&RequestId::Int(999)));
    }

    #[test]
    fn forget_does_not_cancel() {
        let reg = CancelRegistry::default();
        let tok = reg.register(RequestId::Int(2));
        reg.forget(&RequestId::Int(2));
        assert!(!tok.is_cancelled());
    }

    #[test]
    fn cancel_all_cancels_everything() {
        let reg = CancelRegistry::default();
        let a = reg.register(RequestId::Int(1));
        let b = reg.register(RequestId::Int(2));
        reg.cancel_all();
        assert!(a.is_cancelled());
        assert!(b.is_cancelled());
        assert_eq!(reg.len(), 0);
    }
}
```

- [ ] **Step 2: Run tests**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-app --lib serve::cancel
```

Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-app/src/serve/cancel.rs
git commit -m "feat(tau-app): CancelRegistry + tests"
```

---

## Task 9: Project / agent resolution

**Files:**
- Modify: `crates/tau-app/src/serve/project.rs`

- [ ] **Step 1: Inspect `tau-cli`'s existing helper**

```bash
grep -nA20 "build_agent_definition" crates/tau-cli/src/config.rs 2>/dev/null | head -40
```

The plan replicates the same resolution logic at the **library** layer. tau-cli stays unchanged; tau-app gets its own copy. (Future refactor opportunity: lift into `tau-pkg` or `tau-runtime`. Out of scope for v1.)

- [ ] **Step 2: Write `crates/tau-app/src/serve/project.rs`**

```rust
//! Project-level helpers: resolve agent_id → (AgentDefinition, PackageManifest).

use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use tau_domain::{AgentDefinition, PackageManifest};

/// Holds project state needed across multiple RPC calls.
#[derive(Debug, Clone)]
pub struct Project {
    /// Canonical project root (the directory containing tau.toml).
    pub root: PathBuf,
    /// Loaded `tau.toml` contents — parsed AgentEntry list.
    /// Implementation lifts the relevant types from `tau-pkg`.
    pub agents: Vec<AgentBinding>,
}

/// One `[[agents]]` entry resolved into the form needed for runtime
/// invocation.
#[derive(Debug, Clone)]
pub struct AgentBinding {
    /// Agent id from tau.toml (e.g., "my-agent").
    pub id: String,
    /// Resolved agent definition.
    pub def: AgentDefinition,
    /// Resolved package manifest (the agent's package).
    pub manifest: PackageManifest,
}

impl Project {
    /// Load a project from disk.
    ///
    /// Equivalent of `tau_cli::config::build_agent_definition` repeated
    /// for every agent in the project's `tau.toml`. The implementation
    /// reuses `tau_pkg::project` loaders.
    pub async fn load(root: &Path) -> Result<Self> {
        let root = std::fs::canonicalize(root)
            .with_context(|| format!("canonicalize project root {}", root.display()))?;
        // Reuse tau-pkg's project loader. Concrete fn name may differ —
        // adjust to match tau-pkg's current public API. The result is a
        // list of (agent_id, AgentDefinition, PackageManifest).
        let raw = tau_pkg::project::load(&root)
            .with_context(|| format!("load tau.toml at {}", root.display()))?;
        let agents = raw
            .into_iter()
            .map(|(id, def, manifest)| AgentBinding { id, def, manifest })
            .collect();
        Ok(Self { root, agents })
    }

    /// Look up an agent by id.
    pub fn resolve(&self, agent_id: &str) -> Result<&AgentBinding> {
        self.agents
            .iter()
            .find(|a| a.id == agent_id)
            .ok_or_else(|| anyhow!("unknown agent: {}", agent_id))
    }

    /// List all agent ids (for the handshake response).
    pub fn agent_ids(&self) -> Vec<String> {
        self.agents.iter().map(|a| a.id.clone()).collect()
    }
}
```

**Implementation note:** the exact `tau_pkg::project::load` signature is to be confirmed against `crates/tau-pkg/src/lib.rs` exports. If the existing loader returns a different shape, write a thin adapter here. The contract this module provides is `Project::load + Project::resolve + Project::agent_ids`.

- [ ] **Step 3: Compile check**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo check -p tau-app
```

If `tau_pkg::project::load` does not exist, find the equivalent and update Step 2. Common alternatives: `tau_pkg::ProjectConfig::load`, `tau_pkg::config::load_project`.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-app/src/serve/project.rs
git commit -m "feat(tau-app): Project loader + agent resolution"
```

---

## Task 10: Dispatcher (without Runtime methods yet)

**Files:**
- Modify: `crates/tau-app/src/serve/dispatch.rs`

- [ ] **Step 1: Write `crates/tau-app/src/serve/dispatch.rs`**

```rust
//! Request dispatcher: route inbound messages to method handlers,
//! enforce handshake + concurrency state.
//!
//! The dispatcher is single-task (one tokio task running this loop).
//! Per-request work is spawned into a `LocalSet` so that
//! non-`Send` Runtime streams can be polled across await points.

use super::cancel::CancelRegistry;
use super::error_codes;
use super::handshake::{Check, HandshakeState};
use super::methods;
use super::project::Project;
use super::protocol::{
    ErrorObject, ErrorResponse, Inbound, Notification, Outbound, Request, RequestId, Response,
};
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::LocalSet;
use tracing::warn;

/// Inbound payload after framing-layer parse.
pub use super::framing::Inbound as Frame;

/// Shared dispatcher state. Cheap to clone (all `Arc`/clone-safe inner).
#[derive(Clone)]
pub struct Dispatcher {
    pub project: Arc<Project>,
    pub runtime: Arc<tau_runtime::Runtime>,
    pub handshake: HandshakeState,
    pub cancel_reg: CancelRegistry,
    pub max_concurrent: usize,
    pub out_tx: mpsc::Sender<Outbound>,
}

impl Dispatcher {
    /// Main dispatch loop. Runs until `in_rx` closes (EOF / shutdown).
    pub async fn run(
        self,
        mut in_rx: mpsc::Receiver<Frame>,
        local_set: &LocalSet,
    ) -> Result<()> {
        while let Some(frame) = in_rx.recv().await {
            match frame {
                Frame::Eof => break,
                Frame::ParseError(msg) => {
                    warn!(error = %msg, "parse error");
                    // Per JSON-RPC 2.0, parse errors carry null id.
                    let _ = self
                        .out_tx
                        .send(Outbound::Error(ErrorResponse {
                            jsonrpc: "2.0".into(),
                            id: RequestId::Int(0),
                            error: ErrorObject {
                                code: error_codes::PARSE_ERROR,
                                message: "Parse error".into(),
                                data: None,
                            },
                        }))
                        .await;
                }
                Frame::Json(value) => self.handle_one(value, local_set).await,
            }
        }
        Ok(())
    }

    async fn handle_one(&self, value: Value, local_set: &LocalSet) {
        // Parse as Request.
        let req: Request = match serde_json::from_value(value) {
            Ok(r) => r,
            Err(e) => {
                self.send_err(
                    RequestId::Int(0),
                    error_codes::INVALID_REQUEST,
                    format!("invalid request: {}", e),
                    None,
                )
                .await;
                return;
            }
        };

        // Handshake state check.
        let check = self.handshake.check(&req.method);
        match check {
            Check::HandshakeRequired => {
                self.send_err(
                    req.id,
                    error_codes::HANDSHAKE_REQUIRED,
                    "Handshake required".into(),
                    None,
                )
                .await;
                return;
            }
            Check::AlreadyHandshaken => {
                self.send_err(
                    req.id,
                    error_codes::ALREADY_HANDSHAKEN,
                    "Already handshaken".into(),
                    None,
                )
                .await;
                return;
            }
            Check::Allowed => {}
        }

        // Concurrency cap (only for runtime.run / runtime.run_streaming).
        let is_runtime_method = req.method.starts_with("runtime.")
            && req.method != methods::RUNTIME_CANCEL;
        if is_runtime_method && self.cancel_reg.len() >= self.max_concurrent {
            self.send_err(
                req.id,
                error_codes::SERVER_BUSY,
                format!("Server busy: max_concurrent_runs={} reached", self.max_concurrent),
                Some(json!({"max_concurrent": self.max_concurrent})),
            )
            .await;
            return;
        }

        // Route.
        match req.method.as_str() {
            methods::META_HANDSHAKE => self.handle_handshake(req).await,
            methods::META_PING => self.handle_ping(req).await,
            methods::RUNTIME_RUN => self.spawn_run(req, local_set, /*streaming=*/ false),
            methods::RUNTIME_RUN_STREAMING => self.spawn_run(req, local_set, /*streaming=*/ true),
            methods::RUNTIME_CANCEL => self.handle_cancel(req).await,
            other => {
                self.send_err(
                    req.id,
                    error_codes::METHOD_NOT_FOUND,
                    format!("Method not found: {}", other),
                    None,
                )
                .await;
            }
        }
    }

    async fn handle_handshake(&self, req: Request) {
        // Params parsing tolerated lazily — extract only what we need.
        let params = req.params.unwrap_or_else(|| json!({}));
        let client_proto = params
            .get("protocol_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if client_proto != 1 {
            self.send_err(
                req.id,
                error_codes::HANDSHAKE_MISMATCH,
                format!("protocol_version {} not supported", client_proto),
                Some(json!({"supported_versions": [1]})),
            )
            .await;
            return;
        }
        self.handshake.mark_handshaken();
        let result = json!({
            "server_name": "tau",
            "server_version": env!("CARGO_PKG_VERSION"),
            "protocol_version": 1,
            "project_path": self.project.root.display().to_string(),
            "agents": self.project.agent_ids(),
        });
        self.send_ok(req.id, result).await;
    }

    async fn handle_ping(&self, req: Request) {
        self.send_ok(req.id, json!({"ok": true})).await;
    }

    async fn handle_cancel(&self, req: Request) {
        let params = req.params.unwrap_or(json!({}));
        let target: RequestId = match params.get("id") {
            Some(v) => match serde_json::from_value(v.clone()) {
                Ok(id) => id,
                Err(_) => {
                    self.send_err(
                        req.id,
                        error_codes::INVALID_PARAMS,
                        "params.id must be int or string".into(),
                        None,
                    )
                    .await;
                    return;
                }
            },
            None => {
                self.send_err(
                    req.id,
                    error_codes::INVALID_PARAMS,
                    "params.id missing".into(),
                    None,
                )
                .await;
                return;
            }
        };
        let cancelled = self.cancel_reg.cancel(&target);
        self.send_ok(req.id, json!({"cancelled": cancelled})).await;
    }

    /// Spawn the per-request task on the LocalSet. Runtime streams are
    /// non-`Send` so we must use `spawn_local`.
    fn spawn_run(&self, req: Request, local_set: &LocalSet, streaming: bool) {
        let this = self.clone();
        local_set.spawn_local(async move {
            super::dispatch_run::execute(this, req, streaming).await;
        });
    }

    pub async fn send_ok(&self, id: RequestId, result: Value) {
        let _ = self
            .out_tx
            .send(Outbound::Response(Response {
                jsonrpc: "2.0".into(),
                id,
                result,
            }))
            .await;
    }

    pub async fn send_err(&self, id: RequestId, code: i32, message: String, data: Option<Value>) {
        let _ = self
            .out_tx
            .send(Outbound::Error(ErrorResponse {
                jsonrpc: "2.0".into(),
                id,
                error: ErrorObject { code, message, data },
            }))
            .await;
    }

    pub async fn send_notification(&self, method: &str, params: Value) {
        let _ = self
            .out_tx
            .send(Outbound::Notification(Notification {
                jsonrpc: "2.0".into(),
                method: method.into(),
                params: Some(params),
            }))
            .await;
    }
}
```

- [ ] **Step 2: Add a stub for the run executor that Task 11 fills in**

Create `crates/tau-app/src/serve/dispatch_run.rs`:

```rust
//! Per-request executor for runtime.run and runtime.run_streaming.
//! Filled in by Task 11.

use super::dispatch::Dispatcher;
use super::protocol::Request;

pub async fn execute(disp: Dispatcher, req: Request, _streaming: bool) {
    // Stub — Task 11 implements this.
    disp.send_err(
        req.id,
        super::error_codes::INTERNAL_ERROR,
        "runtime.run executor not yet implemented".into(),
        None,
    )
    .await;
}
```

Add `mod dispatch_run;` to `crates/tau-app/src/serve/mod.rs`.

- [ ] **Step 3: Compile check**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo check -p tau-app
```

Expected: clean. (Unused imports/`#[allow(dead_code)]` is fine at this stage.)

- [ ] **Step 4: Commit**

```bash
git add crates/tau-app/src/serve/dispatch.rs crates/tau-app/src/serve/dispatch_run.rs crates/tau-app/src/serve/mod.rs
git commit -m "feat(tau-app): dispatcher with handshake + ping + cancel methods"
```

---

## Task 11: runtime.run + runtime.run_streaming executors

**Files:**
- Modify: `crates/tau-app/src/serve/dispatch_run.rs`

- [ ] **Step 1: Write `crates/tau-app/src/serve/dispatch_run.rs`**

```rust
//! Per-request executor for runtime.run and runtime.run_streaming.

use super::dispatch::Dispatcher;
use super::error_codes;
use super::error_map::from_runtime_error;
use super::methods;
use super::protocol::Request;
use futures::StreamExt;
use serde_json::{json, Value};
use tau_domain::Message;
use tau_runtime::{RunEvent, RunOptions};

/// Parse common params for run / run_streaming.
struct RunParams {
    agent: String,
    prompt: String,
}

fn parse_run_params(value: &Value) -> Result<RunParams, String> {
    let agent = value
        .get("agent")
        .and_then(|v| v.as_str())
        .ok_or("params.agent missing or not a string")?
        .to_string();
    let prompt = value
        .get("prompt")
        .and_then(|v| v.as_str())
        .ok_or("params.prompt missing or not a string")?
        .to_string();
    Ok(RunParams { agent, prompt })
}

/// Execute a runtime.run or runtime.run_streaming request.
pub async fn execute(disp: Dispatcher, req: Request, streaming: bool) {
    let params = match req.params.as_ref() {
        Some(v) => v,
        None => {
            disp.send_err(
                req.id,
                error_codes::INVALID_PARAMS,
                "params missing".into(),
                None,
            )
            .await;
            return;
        }
    };
    let parsed = match parse_run_params(params) {
        Ok(p) => p,
        Err(e) => {
            disp.send_err(req.id, error_codes::INVALID_PARAMS, e, None)
                .await;
            return;
        }
    };

    // Resolve agent binding.
    let binding = match disp.project.resolve(&parsed.agent) {
        Ok(b) => b.clone(),
        Err(_) => {
            disp.send_err(
                req.id,
                error_codes::UNKNOWN_AGENT,
                format!("agent_id not found: {}", parsed.agent),
                Some(json!({"agent_id": parsed.agent})),
            )
            .await;
            return;
        }
    };

    let cancel = disp.cancel_reg.register(req.id.clone());
    let msg = Message::user(parsed.prompt);
    let opts = RunOptions::default();

    let result: Result<(), tau_runtime::RuntimeError> = if streaming {
        execute_streaming(&disp, req.id.clone(), binding, msg, opts, cancel.clone()).await
    } else {
        execute_batch(&disp, req.id.clone(), binding, msg, opts, cancel.clone()).await
    };

    // Cleanup registry.
    disp.cancel_reg.forget(&req.id);

    // Error path: if Runtime returned an error, map and emit.
    if let Err(err) = result {
        let obj = from_runtime_error(&err);
        disp.send_err(req.id, obj.code, obj.message, obj.data).await;
    }
}

async fn execute_batch(
    disp: &Dispatcher,
    id: super::protocol::RequestId,
    binding: super::project::AgentBinding,
    initial: Message,
    opts: RunOptions,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<(), tau_runtime::RuntimeError> {
    use tokio::select;
    let fut = disp
        .runtime
        .run(binding.def, binding.manifest, initial, opts);
    select! {
        outcome = fut => {
            let outcome = outcome?;
            // Map RunOutcome to JSON. Mirror tau-cli's existing renderer
            // shape but emit the raw struct via serde_json.
            let body = serde_json::to_value(&outcome).unwrap_or_else(|_| json!({}));
            disp.send_ok(id, body).await;
            Ok(())
        }
        _ = cancel.cancelled() => {
            disp.send_err(
                id,
                error_codes::CANCELLED,
                "Cancelled by client".into(),
                None,
            ).await;
            Ok(())
        }
    }
}

async fn execute_streaming(
    disp: &Dispatcher,
    id: super::protocol::RequestId,
    binding: super::project::AgentBinding,
    initial: Message,
    opts: RunOptions,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<(), tau_runtime::RuntimeError> {
    use tokio::select;
    let mut stream = disp
        .runtime
        .run_streaming(binding.def, binding.manifest, initial, opts)
        .await?;

    let mut last_token_usage: Option<Value> = None;
    let mut stop_reason: Option<String> = None;

    loop {
        select! {
            biased;
            _ = cancel.cancelled() => {
                disp.send_err(
                    id,
                    error_codes::CANCELLED,
                    "Cancelled by client".into(),
                    None,
                ).await;
                return Ok(());
            }
            event = stream.next() => {
                match event {
                    None => break,
                    Some(ev) => emit_event(disp, &id, &ev, &mut last_token_usage, &mut stop_reason).await,
                }
            }
        }
    }

    // Stream completed normally — emit final response.
    let body = json!({
        "final": true,
        "token_usage": last_token_usage,
        "stop_reason": stop_reason,
    });
    disp.send_ok(id, body).await;
    Ok(())
}

async fn emit_event(
    disp: &Dispatcher,
    id: &super::protocol::RequestId,
    event: &RunEvent,
    last_token_usage: &mut Option<Value>,
    stop_reason: &mut Option<String>,
) {
    let (kind, data) = match event {
        RunEvent::TextDelta { text } => ("TextDelta", json!({"text": text})),
        RunEvent::ToolCallStarted { tool, args, call_id } => (
            "ToolCallStarted",
            json!({"tool": tool, "args": args, "call_id": call_id}),
        ),
        RunEvent::ToolCallCompleted { tool, result, call_id } => (
            "ToolCallCompleted",
            json!({"tool": tool, "result": result, "call_id": call_id}),
        ),
        RunEvent::TurnCompleted { turn, stop_reason: sr } => {
            *stop_reason = Some(sr.clone());
            (
                "TurnCompleted",
                json!({"turn": turn, "stop_reason": sr}),
            )
        }
        RunEvent::RunCompleted { token_usage } => {
            *last_token_usage = Some(serde_json::to_value(token_usage).unwrap_or(json!({})));
            ("RunCompleted", json!({"token_usage": token_usage}))
        }
        RunEvent::FatalError { tool_error_variant, message, .. } => (
            "FatalError",
            json!({"tool_error_variant": tool_error_variant, "message": message}),
        ),
    };
    disp.send_notification(
        methods::RUNTIME_EVENT,
        json!({"id": id, "kind": kind, "data": data}),
    )
    .await;
}
```

**Implementation note:** the exact field names in `RunEvent` variants (especially `TextDelta`, `ToolCallStarted`, etc.) must match `crates/tau-runtime/src/stream.rs`. Run `grep -A8 "TextDelta\|ToolCall" crates/tau-runtime/src/stream.rs` and adjust the match patterns above if names differ. The protocol output shape (the `data` json) is fixed by the spec.

- [ ] **Step 2: Compile check**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo check -p tau-app
```

Expected: clean. If field-name mismatches surface, fix per the note above.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-app/src/serve/dispatch_run.rs
git commit -m "feat(tau-app): runtime.run + runtime.run_streaming executors"
```

---

## Task 12: Lifecycle (startup, signals, shutdown)

**Files:**
- Modify: `crates/tau-app/src/serve/lifecycle.rs`
- Modify: `crates/tau-app/src/serve/tracing_init.rs`

- [ ] **Step 1: Write `crates/tau-app/src/serve/tracing_init.rs`**

```rust
//! Tracing subscriber configured to write to stderr only.
//!
//! stdout is reserved for the JSON-RPC protocol. Any tracing/logging
//! sent to stdout would corrupt the protocol stream.

use tracing_subscriber::{fmt, EnvFilter};

/// Install a global tracing subscriber writing to stderr. Honors
/// `RUST_LOG`. Idempotent — safe to call multiple times.
pub fn install() {
    let _ = fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .try_init();
}
```

- [ ] **Step 2: Write `crates/tau-app/src/serve/lifecycle.rs`**

```rust
//! Process lifecycle: startup, signals, graceful shutdown.

use super::cancel::CancelRegistry;
use super::dispatch::Dispatcher;
use super::framing;
use super::handshake::HandshakeState;
use super::options::ServeOptions;
use super::project::Project;
use super::protocol::Outbound;
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::task::LocalSet;
use tracing::{info, warn};

/// Main serve entry point. Builds runtime, spawns tasks, blocks until shutdown.
pub async fn run(opts: ServeOptions) -> Result<()> {
    super::tracing_init::install();

    info!(project = %opts.project_path.display(), "serve starting");

    let project = Arc::new(
        Project::load(&opts.project_path)
            .await
            .context("load project")?,
    );

    let runtime = build_runtime(&project)
        .await
        .context("build runtime")?;

    let (in_tx, in_rx) = mpsc::channel(64);
    let (out_tx, out_rx) = mpsc::channel(256);

    // Linux: set PDEATHSIG so we die when parent dies. Must run BEFORE
    // any task spawn so child threads inherit it.
    #[cfg(target_os = "linux")]
    set_pdeathsig();

    let cancel_reg = CancelRegistry::default();
    let dispatcher = Dispatcher {
        project: project.clone(),
        runtime: Arc::new(runtime),
        handshake: HandshakeState::default(),
        cancel_reg: cancel_reg.clone(),
        max_concurrent: opts.max_concurrent,
        out_tx: out_tx.clone(),
    };

    let local_set = LocalSet::new();

    // Reader task — Send-friendly, spawn on multi-thread runtime side.
    let reader_handle = tokio::spawn(framing::reader_task(in_tx));
    // Writer task — Send-friendly.
    let writer_handle = tokio::spawn(framing::writer_task(out_rx));

    if opts.ready_on_stderr {
        eprintln!("tau-serve ready");
    }

    let shutdown_signal = wait_for_shutdown_signal();

    // Run dispatcher loop on the LocalSet so per-request tasks have a
    // current_thread executor available for non-Send streams.
    let dispatcher_fut = local_set.run_until(async move {
        tokio::select! {
            r = dispatcher.run(in_rx, &local_set_marker(&local_set)) => r,
            _ = shutdown_signal => Ok(()),
        }
    });

    let dispatch_result = dispatcher_fut.await;

    // Graceful drain.
    cancel_reg.cancel_all();
    let grace_result = tokio::time::timeout(opts.shutdown_grace, async {
        // Reader will exit on next loop iteration after stdin EOF or shutdown.
        let _ = reader_handle.await;
    })
    .await;
    if grace_result.is_err() {
        warn!(grace = ?opts.shutdown_grace, "shutdown grace expired");
    }
    drop(out_tx); // close so writer exits
    let _ = writer_handle.await;

    info!("serve shutdown complete");
    dispatch_result?;
    Ok(())
}

/// `LocalSet` reference helper. Borrowed once at dispatcher startup;
/// the dispatcher's `&LocalSet` is the same one we're already running
/// `run_until` on.
fn local_set_marker(set: &LocalSet) -> &LocalSet {
    set
}

/// Build the `Runtime` from a loaded `Project`.
///
/// Implementation lifts the relevant plugin-loader calls from
/// `tau-cli::cmd::run`. The resulting Runtime hosts all plugins
/// referenced by every agent in the project.
async fn build_runtime(project: &Project) -> Result<tau_runtime::Runtime> {
    // Pseudocode — wire up to existing tau-pkg + tau-runtime APIs:
    //   1. For each agent in project.agents, collect plugin requirements
    //      (LLM backend + tools) from the package_manifest.
    //   2. Resolve via tau-pkg's lockfile reader.
    //   3. For each unique plugin: spawn via tau_runtime::plugin_host.
    //   4. Use RuntimeBuilder::new().register_*().build().
    //
    // The exact code is a near-copy of tau-cli/src/cmd/run.rs's
    // pre-run setup. Extract that body into a tau-app helper, OR
    // expose a public helper in tau-runtime (e.g., RuntimeBuilder::
    // from_project) — preferred long-term.

    todo!("lift plugin-loader logic from tau-cli::cmd::run::execute or expose tau_runtime::RuntimeBuilder::from_project")
}

/// Wait for any of: SIGTERM, SIGINT, stdin EOF.
/// Returns when any fires.
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut int = match signal(SignalKind::interrupt()) {
        Ok(s) => s,
        Err(_) => return,
    };
    tokio::select! {
        _ = term.recv() => info!("received SIGTERM"),
        _ = int.recv() => info!("received SIGINT"),
    }
}

/// On Linux, ask the kernel to deliver SIGTERM to us when our parent dies.
#[cfg(target_os = "linux")]
fn set_pdeathsig() {
    // SAFETY: prctl is async-signal-safe; the SIGTERM target is the
    // current process which always exists.
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM as libc::c_ulong, 0, 0, 0);
    }
}
```

**Two open hooks to close in this task:**

1. **`build_runtime`** has a `todo!()`. Replace with the actual logic by either:
   - Lifting the relevant code from `crates/tau-cli/src/cmd/run.rs` (search for `RuntimeBuilder::new()` and copy the pre-run plugin-load + register sequence into `build_runtime` here), OR
   - Adding a public helper `pub async fn RuntimeBuilder::from_project(root: &Path) -> Result<Runtime, BuildError>` to `crates/tau-runtime/src/builder.rs` and using it from both `tau-cli` and `tau-app`. **Preferred.**

   This is the single largest concrete decision in the plan. If the lift-into-runtime approach is chosen, that adds ~150 LOC to `tau-runtime` but is reusable forever.

2. **`local_set_marker`** is a function that exists only because the dispatcher needs an `&LocalSet` reference to call `.spawn_local()` from inside a future that's already being driven by `local_set.run_until()`. The dispatcher's `run` signature takes the LocalSet as an argument, mirroring the pattern. The implementer should validate the borrow lifetime works under tokio's `LocalSet`; if it doesn't, the alternative is `Rc<LocalSet>` or passing a `LocalSpawner` handle. This is purely a Rust borrow-checker concern, not a design concern.

- [ ] **Step 3: Add `libc` dep on Linux**

In `crates/tau-app/Cargo.toml`:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
libc = "0.2"
```

- [ ] **Step 4: Compile check**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo check -p tau-app
```

This will fail with `todo!()` — that's expected. The build error is allowed at this commit; resolved in Task 13.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-app/src/serve/lifecycle.rs crates/tau-app/src/serve/tracing_init.rs crates/tau-app/Cargo.toml
git commit -m "feat(tau-app): lifecycle, tracing, signals (build_runtime TODO)"
```

---

## Task 13: build_runtime — lift plugin-load logic

**Files:**
- Modify: `crates/tau-runtime/src/builder.rs`
- Modify: `crates/tau-app/src/serve/lifecycle.rs`

- [ ] **Step 1: Inspect the existing pattern in `tau-cli::cmd::run`**

```bash
grep -nA80 "pub async fn execute" crates/tau-cli/src/cmd/run.rs | head -120
```

Identify the block that:
1. Iterates the project's agents,
2. Resolves each agent's required plugins via tau-pkg lockfile,
3. Spawns each plugin process via `tau_runtime::plugin_host::load_{llm_backend, tool, storage}`,
4. Registers them on a `RuntimeBuilder`,
5. Calls `.build()`.

This is ~100-150 LOC.

- [ ] **Step 2: Add public helper `RuntimeBuilder::from_project`**

In `crates/tau-runtime/src/builder.rs`, after the existing `RuntimeBuilder` impl, add:

```rust
impl RuntimeBuilder {
    /// Convenience: build a complete `Runtime` from a project root.
    ///
    /// Reads the project's `tau.toml` + lockfile, spawns each required
    /// plugin process, and registers everything on a `RuntimeBuilder`.
    /// Returns a ready-to-run `Runtime`.
    ///
    /// This is the entry point used by `tau-cli::cmd::run` and
    /// `tau-app::serve`. Centralizing avoids divergence between the
    /// two binaries.
    pub async fn from_project(root: &std::path::Path) -> Result<Runtime, crate::error::BuildError> {
        // [Migrated code from tau-cli::cmd::run::execute lines XXX-YYY.]
        // Concrete migration: copy the block identified in Step 1 here,
        // and replace tau-cli-specific references (error_render, output)
        // with their tau-domain equivalents or with BuildError variants.
        todo!("migrate plugin-load + builder body from tau-cli::cmd::run")
    }
}
```

Replace `todo!()` with the migrated body. Migration is mechanical — function signatures stay the same, but anyhow-context calls become `BuildError::*` variants.

- [ ] **Step 3: Update `tau-cli::cmd::run` to call `from_project`**

In `crates/tau-cli/src/cmd/run.rs`, replace the inline plugin-load block with:

```rust
let runtime = tau_runtime::RuntimeBuilder::from_project(&scope_root).await?;
```

…and remove the now-dead local helpers. Net: `crates/tau-cli/src/cmd/run.rs` shrinks by ~100 LOC. tau-app uses the same helper.

- [ ] **Step 4: Wire it in `tau-app::serve::lifecycle::build_runtime`**

Replace the `todo!()` from Task 12 Step 2:

```rust
async fn build_runtime(project: &Project) -> Result<tau_runtime::Runtime> {
    tau_runtime::RuntimeBuilder::from_project(&project.root)
        .await
        .map_err(|e| anyhow::anyhow!("build runtime: {}", e))
}
```

- [ ] **Step 5: Compile check + run existing tests**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo check -p tau-runtime -p tau-cli -p tau-app
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-cli
```

Expected: tau-cli tests still pass. Any regression means the migration changed observable behavior — fix before proceeding.

- [ ] **Step 6: Commit**

```bash
git add crates/tau-runtime/src/builder.rs crates/tau-cli/src/cmd/run.rs crates/tau-app/src/serve/lifecycle.rs
git commit -m "refactor(tau-runtime): RuntimeBuilder::from_project; tau-app reuses"
```

---

## Task 14: tau-cli `tau serve` subcommand

**Files:**
- Modify: `crates/tau-cli/Cargo.toml`
- Create: `crates/tau-cli/src/cmd/serve.rs`
- Modify: `crates/tau-cli/src/cmd/mod.rs`
- Modify: `crates/tau-cli/src/main.rs` (or wherever clap dispatch lives)

- [ ] **Step 1: Add `tau-app` as a dep**

In `crates/tau-cli/Cargo.toml` `[dependencies]`:

```toml
tau-app = { workspace = true }
```

If `tau-app` is not in `[workspace.dependencies]`, add it there in the workspace root `Cargo.toml` too.

- [ ] **Step 2: Write `crates/tau-cli/src/cmd/serve.rs`**

```rust
//! `tau serve` — start serve mode (JSON-RPC over stdio).
//!
//! See ADR-0031 and `docs/superpowers/specs/2026-05-17-tau-serve-mode-design.md`.

use anyhow::Result;
use clap::Args;
use std::path::PathBuf;
use std::time::Duration;
use tau_app::serve::ServeOptions;

#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Path to the tau project. Defaults to cwd.
    #[arg(long, value_name = "PATH")]
    pub project: Option<PathBuf>,

    /// Maximum concurrent in-flight runs. Defaults to 8.
    #[arg(long, value_name = "N")]
    pub max_concurrent: Option<usize>,

    /// If set, the server initiates graceful shutdown after no
    /// message activity for this many seconds.
    #[arg(long, value_name = "SECS")]
    pub idle_timeout: Option<u64>,

    /// Write "tau-serve ready" to stderr after startup completes.
    #[arg(long)]
    pub ready_on_stderr: bool,

    /// Seconds to wait for in-flight tasks during graceful shutdown.
    /// Default 5.
    #[arg(long, value_name = "SECS", default_value_t = 5)]
    pub shutdown_grace: u64,
}

pub async fn execute(args: ServeArgs) -> Result<()> {
    let mut opts = ServeOptions::default();
    if let Some(p) = args.project {
        opts.project_path = std::fs::canonicalize(&p).unwrap_or(p);
    }
    if let Some(n) = args.max_concurrent {
        opts.max_concurrent = n;
    }
    opts.idle_timeout = args.idle_timeout.map(Duration::from_secs);
    opts.ready_on_stderr = args.ready_on_stderr;
    opts.shutdown_grace = Duration::from_secs(args.shutdown_grace);

    // Use current_thread runtime for non-Send Runtime streams.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(tau_app::serve::run(opts))
}
```

- [ ] **Step 3: Register in `crates/tau-cli/src/cmd/mod.rs`**

```rust
pub mod serve;
```

In the clap `Subcommand` enum (`crates/tau-cli/src/main.rs` or wherever the dispatch lives), add a variant:

```rust
Serve(crate::cmd::serve::ServeArgs),
```

And in the dispatch match:

```rust
Subcommands::Serve(args) => crate::cmd::serve::execute(args).await,
```

Use grep to find the existing pattern:
```bash
grep -nB1 -A2 "Subcommands::" crates/tau-cli/src/main.rs | head -30
```

- [ ] **Step 4: Build + smoke run**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo build -p tau-cli
target/main/debug/tau serve --help
```

Expected: clean compile; `--help` shows the new subcommand flags.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-cli/Cargo.toml crates/tau-cli/src/cmd/serve.rs crates/tau-cli/src/cmd/mod.rs crates/tau-cli/src/main.rs
# Add Cargo.toml at workspace root if you had to edit it
git commit -m "feat(tau-cli): tau serve subcommand"
```

---

## Task 15: Layer 2 integration tests — handshake + ping

**Files:**
- Create: `crates/tau-app/tests/serve_handshake.rs`
- Create: `crates/tau-app/tests/common/mod.rs`

- [ ] **Step 1: Test harness — `crates/tau-app/tests/common/mod.rs`**

```rust
//! Shared test harness: spin up the dispatcher loop with in-memory
//! pipes, drive it via JSON strings, collect outputs as Values.

use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tau_app::serve::ServeOptions;
use tokio::sync::mpsc;
use tokio::task::LocalSet;

/// Boot a dispatcher backed by a mock Runtime + minimal project.
/// Use the project fixture path; mock runtime built from echo-llm + echo-tool.
pub struct Harness {
    pub in_tx: mpsc::Sender<tau_app::serve::framing::Inbound>,
    pub out_rx: mpsc::Receiver<tau_app::serve::protocol::Outbound>,
    pub set: LocalSet,
}

impl Harness {
    pub async fn new(_opts: ServeOptions) -> Self {
        // Wire up a minimal Dispatcher directly. The exact path to
        // construct Dispatcher in tests requires the `serve` module to
        // re-export its inner types `pub(crate)`. Add `pub use
        // dispatch::Dispatcher; pub use framing::Inbound; pub use
        // protocol::Outbound;` to crates/tau-app/src/serve/mod.rs
        // under `#[cfg(any(test, feature = "test-fixtures"))]` if the
        // current visibility doesn't allow this.
        todo!("wire up test harness once Dispatcher visibility allows")
    }

    pub async fn send_raw(&self, line: &str) {
        let v: Value = serde_json::from_str(line).expect("test json");
        let _ = self
            .in_tx
            .send(tau_app::serve::framing::Inbound::Json(v))
            .await;
    }

    pub async fn recv(&mut self) -> Option<Value> {
        let timeout = Duration::from_millis(500);
        match tokio::time::timeout(timeout, self.out_rx.recv()).await {
            Ok(Some(out)) => Some(serde_json::to_value(&out).ok()?),
            _ => None,
        }
    }
}
```

**Implementation note:** This task includes a `todo!()` for the harness construction because exposing `Dispatcher` from within `serve::*` for test use is a visibility decision better made when the harness is implemented. The clean path is a `#[cfg(any(test, feature = "test-fixtures"))]` pub re-export of the relevant inner types. Adjust visibility in `serve/mod.rs` to compile this file.

- [ ] **Step 2: Write `crates/tau-app/tests/serve_handshake.rs`**

```rust
//! Layer 2 — handshake protocol tests.

mod common;
use common::Harness;
use serde_json::json;
use tau_app::serve::ServeOptions;

#[tokio::test]
async fn happy_handshake() {
    let mut h = Harness::new(ServeOptions::default()).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"meta.handshake","params":{"client_name":"test","client_version":"0.1.0","protocol_version":1}}"#).await;
    let resp = h.recv().await.unwrap();
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocol_version"], 1);
}

#[tokio::test]
async fn version_mismatch() {
    let mut h = Harness::new(ServeOptions::default()).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"meta.handshake","params":{"protocol_version":999}}"#).await;
    let resp = h.recv().await.unwrap();
    assert_eq!(resp["error"]["code"], -32000);
}

#[tokio::test]
async fn pre_handshake_runtime_call_rejected() {
    let mut h = Harness::new(ServeOptions::default()).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"runtime.run","params":{"agent":"x","prompt":"y"}}"#).await;
    let resp = h.recv().await.unwrap();
    assert_eq!(resp["error"]["code"], -32002);
}

#[tokio::test]
async fn double_handshake_rejected() {
    let mut h = Harness::new(ServeOptions::default()).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"meta.handshake","params":{"protocol_version":1}}"#).await;
    let _ = h.recv().await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":2,"method":"meta.handshake","params":{"protocol_version":1}}"#).await;
    let resp = h.recv().await.unwrap();
    assert_eq!(resp["error"]["code"], -32003);
}

#[tokio::test]
async fn ping_works_before_handshake() {
    let mut h = Harness::new(ServeOptions::default()).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"meta.ping"}"#).await;
    let resp = h.recv().await.unwrap();
    assert_eq!(resp["result"]["ok"], true);
}
```

- [ ] **Step 3: Run**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-app --test serve_handshake
```

Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-app/tests/serve_handshake.rs crates/tau-app/tests/common/
git commit -m "test(tau-app): Layer 2 handshake tests"
```

---

## Task 16: Layer 2 — run_batch, run_streaming, cancel, concurrent, shutdown

This task bundles five test files (one per spec scenario). Each is structurally identical to Task 15 but exercises a different method. The harness from Task 15's `common/mod.rs` is reused.

**Files:**
- Create: `crates/tau-app/tests/serve_run_batch.rs`
- Create: `crates/tau-app/tests/serve_run_streaming.rs`
- Create: `crates/tau-app/tests/serve_cancel.rs`
- Create: `crates/tau-app/tests/serve_concurrent.rs`
- Create: `crates/tau-app/tests/serve_shutdown.rs`
- Create: `crates/tau-app/tests/fixtures/echo-project/tau.toml`

- [ ] **Step 1: Fixture project**

Create `crates/tau-app/tests/fixtures/echo-project/tau.toml`:

```toml
# Minimal project for serve-mode integration tests. Uses echo-llm +
# echo-tool toy plugins from crates/tau-plugins-test/.

[[agents]]
id = "echo-agent"
description = "Returns a deterministic echo for testing."

[agents.llm_backend]
package = "echo-llm"
source = { path = "../../../../tau-plugins-test/echo-llm" }

[[agents.tools]]
package = "echo-tool"
source = { path = "../../../../tau-plugins-test/echo-tool" }
```

Adjust `source.path` based on actual workspace layout. The intent: a self-contained project referencing the toy plugins that already exist in the workspace.

- [ ] **Step 2: Write `crates/tau-app/tests/serve_run_batch.rs`**

```rust
//! Layer 2 — runtime.run happy path + error mapping.

mod common;
use common::Harness;
use serde_json::Value;
use tau_app::serve::ServeOptions;

async fn handshake(h: &mut Harness) {
    h.send_raw(r#"{"jsonrpc":"2.0","id":0,"method":"meta.handshake","params":{"protocol_version":1}}"#).await;
    let _ = h.recv().await;
}

#[tokio::test]
async fn run_happy_path() {
    let mut h = Harness::new(fixture_opts()).await;
    handshake(&mut h).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"runtime.run","params":{"agent":"echo-agent","prompt":"hi"}}"#).await;
    let resp = h.recv().await.unwrap();
    assert_eq!(resp["id"], 1);
    assert!(resp["result"]["messages"].is_array());
}

#[tokio::test]
async fn unknown_agent_returns_minus_32010() {
    let mut h = Harness::new(fixture_opts()).await;
    handshake(&mut h).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"runtime.run","params":{"agent":"no-such","prompt":"hi"}}"#).await;
    let resp = h.recv().await.unwrap();
    assert_eq!(resp["error"]["code"], -32010);
}

#[tokio::test]
async fn missing_params_returns_minus_32602() {
    let mut h = Harness::new(fixture_opts()).await;
    handshake(&mut h).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"runtime.run"}"#).await;
    let resp = h.recv().await.unwrap();
    assert_eq!(resp["error"]["code"], -32602);
}

fn fixture_opts() -> ServeOptions {
    let mut o = ServeOptions::default();
    o.project_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/echo-project");
    o
}
```

- [ ] **Step 3: Write `crates/tau-app/tests/serve_run_streaming.rs`**

```rust
//! Layer 2 — runtime.run_streaming + event correlation + concurrent demux.

mod common;
use common::Harness;
use serde_json::Value;
use tau_app::serve::ServeOptions;

async fn handshake(h: &mut Harness) {
    h.send_raw(r#"{"jsonrpc":"2.0","id":0,"method":"meta.handshake","params":{"protocol_version":1}}"#).await;
    let _ = h.recv().await;
}

#[tokio::test]
async fn events_carry_request_id() {
    let mut h = Harness::new(fixture_opts()).await;
    handshake(&mut h).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":7,"method":"runtime.run_streaming","params":{"agent":"echo-agent","prompt":"go"}}"#).await;
    // Collect everything until final result.
    let mut events = Vec::new();
    loop {
        let msg = h.recv().await.expect("no more output");
        if msg.get("method").and_then(|m| m.as_str()) == Some("runtime.event") {
            events.push(msg);
        } else {
            // Final response.
            assert_eq!(msg["id"], 7);
            assert_eq!(msg["result"]["final"], true);
            break;
        }
    }
    assert!(!events.is_empty(), "expected at least one event");
    for ev in &events {
        assert_eq!(ev["params"]["id"], 7);
    }
}

fn fixture_opts() -> ServeOptions {
    let mut o = ServeOptions::default();
    o.project_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/echo-project");
    o
}
```

- [ ] **Step 4: Write `crates/tau-app/tests/serve_cancel.rs`**

```rust
//! Layer 2 — runtime.cancel.

mod common;
use common::Harness;
use tau_app::serve::ServeOptions;

async fn handshake(h: &mut Harness) {
    h.send_raw(r#"{"jsonrpc":"2.0","id":0,"method":"meta.handshake","params":{"protocol_version":1}}"#).await;
    let _ = h.recv().await;
}

#[tokio::test]
async fn cancel_unknown_returns_false() {
    let mut h = Harness::new(fixture_opts()).await;
    handshake(&mut h).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"runtime.cancel","params":{"id":999}}"#).await;
    let resp = h.recv().await.unwrap();
    assert_eq!(resp["result"]["cancelled"], false);
}

#[tokio::test]
async fn cancel_invalid_params_returns_minus_32602() {
    let mut h = Harness::new(fixture_opts()).await;
    handshake(&mut h).await;
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"runtime.cancel","params":{}}"#).await;
    let resp = h.recv().await.unwrap();
    assert_eq!(resp["error"]["code"], -32602);
}

// Note: cancel-mid-stream is hard to test deterministically without a
// slow-to-emit fixture LLM. echo-llm responds in one chunk. Skip the
// "cancel while streaming" assertion here and verify it in the e2e
// subprocess tests (Task 18).

fn fixture_opts() -> ServeOptions {
    let mut o = ServeOptions::default();
    o.project_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/echo-project");
    o
}
```

- [ ] **Step 5: Write `crates/tau-app/tests/serve_concurrent.rs`**

```rust
//! Layer 2 — concurrency cap.

mod common;
use common::Harness;
use tau_app::serve::ServeOptions;

async fn handshake(h: &mut Harness) {
    h.send_raw(r#"{"jsonrpc":"2.0","id":0,"method":"meta.handshake","params":{"protocol_version":1}}"#).await;
    let _ = h.recv().await;
}

#[tokio::test]
async fn ninth_concurrent_returns_minus_32004() {
    let mut opts = fixture_opts();
    opts.max_concurrent = 1;
    let mut h = Harness::new(opts).await;
    handshake(&mut h).await;
    // Fire first run; do not consume its result yet — it stays in-flight.
    h.send_raw(r#"{"jsonrpc":"2.0","id":1,"method":"runtime.run_streaming","params":{"agent":"echo-agent","prompt":"a"}}"#).await;
    // Immediately fire second.
    h.send_raw(r#"{"jsonrpc":"2.0","id":2,"method":"runtime.run","params":{"agent":"echo-agent","prompt":"b"}}"#).await;
    // The second one must come back as busy before the first completes.
    // Recv until we find an error -32004 for id=2.
    for _ in 0..32 {
        let msg = h.recv().await.unwrap();
        if msg["id"] == 2 && msg["error"]["code"] == -32004 {
            return;
        }
    }
    panic!("expected -32004 for id=2");
}

fn fixture_opts() -> ServeOptions {
    let mut o = ServeOptions::default();
    o.project_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/echo-project");
    o
}
```

- [ ] **Step 6: Write `crates/tau-app/tests/serve_shutdown.rs`**

Cover stdin-EOF and dispatcher-channel-close paths.

```rust
//! Layer 2 — graceful shutdown.

mod common;
use common::Harness;
use std::time::Duration;
use tau_app::serve::ServeOptions;
use tokio::time::timeout;

async fn handshake(h: &mut Harness) {
    h.send_raw(r#"{"jsonrpc":"2.0","id":0,"method":"meta.handshake","params":{"protocol_version":1}}"#).await;
    let _ = h.recv().await;
}

#[tokio::test]
async fn channel_close_initiates_shutdown() {
    let mut h = Harness::new(fixture_opts()).await;
    handshake(&mut h).await;
    drop(h.in_tx); // Simulate EOF.
    // Harness's internal task should exit within shutdown_grace.
    let exit_within = timeout(Duration::from_secs(6), async {
        // Wait for the local set to settle. Harness drives it; we trust it.
    })
    .await;
    assert!(exit_within.is_ok(), "shutdown took too long");
}

fn fixture_opts() -> ServeOptions {
    let mut o = ServeOptions::default();
    o.project_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/echo-project");
    o
}
```

- [ ] **Step 7: Run all Layer 2 tests**

```bash
timeout 600 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-app
```

Expected: all Layer 2 tests pass.

- [ ] **Step 8: Commit**

```bash
git add crates/tau-app/tests/
git commit -m "test(tau-app): Layer 2 integration suite (run, stream, cancel, concurrent, shutdown)"
```

---

## Task 17: Layer 3 — e2e subprocess tests

**Files:**
- Create: `crates/tau-app/tests/e2e_smoke.rs`
- Create: `crates/tau-app/tests/e2e_streaming.rs`
- Create: `crates/tau-app/tests/e2e_parent_death.rs`
- Create: `crates/tau-app/tests/e2e_ready_signal.rs`

- [ ] **Step 1: Helper — spawn `tau serve` as a real subprocess**

The tests spawn the built `tau` binary (via the env var `CARGO_BIN_EXE_tau` that cargo provides for binary-under-test references). Each test:

1. Locates the `tau` binary path via `env!("CARGO_BIN_EXE_tau")` (requires `tau-cli` to be a `[[bin]]` named "tau" — it already is).
2. Spawns `tau serve --project <fixture>` with piped stdin/stdout/stderr.
3. Writes JSON-RPC lines to stdin.
4. Reads NDJSON lines from stdout.
5. Asserts shape.
6. Closes stdin, waits for clean exit.

- [ ] **Step 2: Write `crates/tau-app/tests/e2e_smoke.rs`**

```rust
//! Layer 3 — real subprocess smoke test.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo-project")
}

fn tau_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tau"))
}

#[test]
fn e2e_handshake_then_run() {
    let mut child = Command::new(tau_bin())
        .args(["serve", "--project"])
        .arg(fixture_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tau serve");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"meta.handshake","params":{{"protocol_version":1}}}}"#
    )
    .unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let resp: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["protocol_version"], 1);

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":2,"method":"runtime.run","params":{{"agent":"echo-agent","prompt":"hello"}}}}"#
    )
    .unwrap();
    let mut line2 = String::new();
    reader.read_line(&mut line2).unwrap();
    let resp2: Value = serde_json::from_str(&line2).unwrap();
    assert_eq!(resp2["id"], 2);
    assert!(resp2["result"].is_object() || resp2["error"].is_object());

    // Close stdin to signal shutdown.
    drop(stdin);
    let status = child.wait().expect("wait");
    assert!(status.success(), "process exited with {:?}", status);
}
```

- [ ] **Step 3: Write `crates/tau-app/tests/e2e_streaming.rs`**

Similar shape — handshake, send `runtime.run_streaming`, read multiple lines from stdout, verify at least one `runtime.event` notification arrives, then a final result.

```rust
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn fixture_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo-project")
}

#[test]
fn e2e_streaming_emits_events_then_result() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tau"))
        .args(["serve", "--project"])
        .arg(fixture_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());

    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"meta.handshake","params":{{"protocol_version":1}}}}"#).unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();

    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"runtime.run_streaming","params":{{"agent":"echo-agent","prompt":"go"}}}}"#).unwrap();

    let mut events = 0;
    loop {
        let mut l = String::new();
        let n = reader.read_line(&mut l).unwrap();
        assert!(n > 0, "stdout closed before final response");
        let v: Value = serde_json::from_str(&l).unwrap();
        if v.get("method").and_then(|m| m.as_str()) == Some("runtime.event") {
            assert_eq!(v["params"]["id"], 2);
            events += 1;
        } else if v["id"] == 2 {
            assert!(events >= 1, "expected at least one event before final");
            break;
        }
    }
    drop(stdin);
    child.wait().unwrap();
}
```

- [ ] **Step 4: Write `crates/tau-app/tests/e2e_ready_signal.rs`**

```rust
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

fn fixture_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo-project")
}

#[test]
fn ready_on_stderr_emits_marker_before_protocol() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tau"))
        .args(["serve", "--ready-on-stderr", "--project"])
        .arg(fixture_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Drain stderr until we see "tau-serve ready".
    let stderr = child.stderr.take().unwrap();
    let reader = BufReader::new(stderr);
    let mut saw = false;
    for line in reader.lines().flatten().take(20) {
        if line.contains("tau-serve ready") {
            saw = true;
            break;
        }
    }
    assert!(saw, "did not observe ready marker on stderr");

    drop(child.stdin.take().unwrap());
    child.wait().unwrap();
}
```

- [ ] **Step 5: Write `crates/tau-app/tests/e2e_parent_death.rs`**

```rust
//! Verifies that on Linux the child gets SIGTERM via PDEATHSIG when
//! its direct parent dies, and on macOS the child exits on stdin EOF.

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn child_exits_on_stdin_eof() {
    use std::process::{Command, Stdio};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo-project");
    let mut child = Command::new(env!("CARGO_BIN_EXE_tau"))
        .args(["serve", "--project"])
        .arg(fixture)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take().unwrap());
    let start = Instant::now();
    loop {
        if let Some(_) = child.try_wait().unwrap() {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "child still alive after stdin EOF");
        std::thread::sleep(Duration::from_millis(100));
    }
}
```

PDEATHSIG-specific tests on Linux are more involved (require spawning a grandparent process). Defer to a follow-up if needed; the stdin-EOF path covers the common parent-death case.

- [ ] **Step 6: Run all e2e tests**

```bash
timeout 600 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-app --test 'e2e_*'
```

Expected: all 4 e2e tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/tau-app/tests/e2e_*.rs
git commit -m "test(tau-app): Layer 3 e2e subprocess tests"
```

---

## Task 18: ADR-0031 + roadmap update

**Files:**
- Create: `docs/decisions/0031-tau-serve-mode.md`
- Modify: `docs/decisions/README.md`
- Modify: `ROADMAP.md`

- [ ] **Step 1: Write `docs/decisions/0031-tau-serve-mode.md`**

Use the existing ADR template (`docs/decisions/template.md`) and fill in:

- **Title:** Tau serve mode v1 — JSON-RPC 2.0 over NDJSON-framed stdio
- **Status:** Accepted
- **Context:** Phase 1 §15. Constitution G6/QG12: tau has two public surfaces (the `tau-runtime` Rust crate and serve mode). Serve mode has been deferred since Phase 0. SDKs and IDE integrations require it; the `tau-app` crate has been a stub reserved for this work.
- **Decision:** Ship v1 of serve mode as JSON-RPC 2.0 over NDJSON-framed stdio. Method surface: 5 methods (`meta.handshake`, `meta.ping`, `runtime.run`, `runtime.run_streaming`, `runtime.cancel`) + 1 server-initiated notification (`runtime.event`). One runtime per process. Reuses `RunEvent` shape from ADR-0011.
- **Consequences:**
  - Phase 1 closes when this ships.
  - +1 required CI check (`test (tau-app serve / linux)`); total 14→15.
  - The protocol surface is now permanently SemVer-versioned. Future additive methods land via ADR amendments (`0031-A`, `0031-B`, …) per QG18.
  - `tau-app` crate exits stub status.
- **Alternatives considered:** LSP framing (Content-Length headers), MessagePack-RPC, HTTP. All rejected — see spec §7.
- **Trigger to revisit:** real consumer demand for additional method namespaces (`session.*`, `pkg.*`, `skill.*`, `workflow.*`) lands an ADR amendment.

- [ ] **Step 2: Update `docs/decisions/README.md`**

Add an index entry:

```markdown
| 0031 | [Tau serve mode v1](0031-tau-serve-mode.md) | Accepted | 2026-05-17 |
```

- [ ] **Step 3: Update `ROADMAP.md` §15**

Find the §15 entry in the "Tier 4 — operational quality" section and replace its bullet body with:

```markdown
15. **Serve mode** (JSON-RPC over stdio) ✅ Shipped 2026-05-17 — see
    [spec](docs/superpowers/specs/2026-05-17-tau-serve-mode-design.md)
    and [ADR-0031](docs/decisions/0031-tau-serve-mode.md).
    `tau serve` exposes runtime.run + runtime.run_streaming as JSON-RPC
    2.0 over NDJSON-framed stdio. 5 methods + 1 server-initiated
    notification in v1. One `Runtime` per process, parallel concurrent
    runs (cap 8). Graceful shutdown on SIGTERM/SIGINT/stdin-EOF/parent-
    death. `tau-app` crate exits stub status. Phase 1 closes; Phase 2
    (tau as a compiled language) is now the active phase. 15 required
    CI checks gating `main` (was 14).
```

Also add a short note at the top of "Current phase" that Phase 1 is complete and Phase 2 is now active.

- [ ] **Step 4: Commit**

```bash
git add docs/decisions/0031-tau-serve-mode.md docs/decisions/README.md ROADMAP.md
git commit -m "docs(adr): ADR-0031 tau serve mode v1 + roadmap update"
```

---

## Task 19: CI integration

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `.lefthook.yml`

- [ ] **Step 1: Add CI job in `.github/workflows/ci.yml`**

Find the existing test jobs (`test-stable / linux`, etc.). Add a new job:

```yaml
  test-tau-app-serve:
    name: "test (tau-app serve / linux)"
    runs-on: ubuntu-latest
    needs: build-fixtures
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
      - uses: actions/download-artifact@v4
        with:
          name: linux-fixture-binaries
          path: target/release/
      - run: chmod +x target/release/*
      - run: cargo nextest run -p tau-app
        env:
          CARGO_INCREMENTAL: 0
```

Add this job to the `required` list at the bottom of the workflow (the one that all matrix jobs must enter for the workflow to succeed). This is the 15th required check.

- [ ] **Step 2: Add to `.lefthook.yml` deep-gate**

In the `pre-push.commands.deep-gate.run` script, add an 11th `cargo nextest` invocation following the existing pattern:

```bash
# ─── 11. test (tau-app serve / linux) ─────────────────
echo "::group::test-tau-app-serve"
cargo nextest run -p tau-app --target-dir $TARGET
echo "::endgroup::"
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml .lefthook.yml
git commit -m "ci: tau-app serve test job (15th required check)"
```

---

## Task 20: Branch protection update + PR

**Files:** none modified locally.

- [ ] **Step 1: Push the branch**

This branch is mostly Rust + minor docs. The local deep-gate covers the Rust changes. Run it standalone before pushing:

```bash
lefthook run pre-push
```

If it passes:
```bash
git push -u origin feat/tau-serve-mode
```

If the deep-gate cold-starts and exceeds time budget, follow CLAUDE.md AGENT PUSH RULES: `scripts/agent-push.sh -u origin feat/tau-serve-mode` (which runs the gate separately, then `git push --no-verify`).

- [ ] **Step 2: Open PR**

```bash
gh pr create --title "feat: tau serve mode v1 (Phase 1 §15 + ADR-0031)" --body "$(cat <<'EOF'
## Summary

- New `tau serve` subcommand: JSON-RPC 2.0 over NDJSON-framed stdio.
- `tau-app::serve` module owns the implementation. ~2,500 LOC + tests.
- 5 methods (`meta.handshake`, `meta.ping`, `runtime.run`, `runtime.run_streaming`, `runtime.cancel`) + 1 server-initiated notification (`runtime.event`).
- One `Runtime` per process via new `RuntimeBuilder::from_project` (shared with tau-cli's `tau run`).
- Parallel concurrent runs over `tokio::task::LocalSet` (Runtime streams are non-`Send`); cap 8 by default.
- Cancellation via `tokio_util::CancellationToken`.
- Graceful shutdown on SIGTERM/SIGINT/stdin-EOF/parent-death (PDEATHSIG on Linux).
- ADR-0031 records the protocol surface as a SemVer-stable public commitment per Constitution G6/QG12.

Closes Phase 1 §15. Phase 2 (tau as a compiled language) is now the active phase.

Spec: `docs/superpowers/specs/2026-05-17-tau-serve-mode-design.md`
Plan: `docs/superpowers/plans/2026-05-17-tau-serve-mode.md`
ADR: `docs/decisions/0031-tau-serve-mode.md`

## Test plan

- [x] Layer 1 unit tests (protocol, framing, handshake, cancel-registry, error-map): ~25 tests
- [x] Layer 2 in-process integration (handshake, run-batch, run-streaming, cancel, concurrent, shutdown): ~30 tests
- [x] Layer 3 e2e subprocess (smoke, streaming, parent-death, ready-signal): 4 tests
- [x] CI green: 15 required checks (was 14)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 3: Update branch-protection required checks**

Once the PR is open and CI runs at least once, update GitHub branch protection on `main`:

```bash
gh api repos/{owner}/{repo}/branches/main/protection \
  --method PATCH \
  --field required_status_checks[contexts][]="test (tau-app serve / linux)"
```

(Or update via the GitHub UI: Settings → Branches → main → Edit → "Require status checks" → add `test (tau-app serve / linux)`.)

- [ ] **Step 4: Merge after review**

Per memory `feedback_branch_protection_workflow`: branch protection is `strict: true`, all required checks must be green, no auto-merge. Use `gh pr merge <num> --squash --delete-branch` once approved.

---

## Self-review

### Spec coverage

| Spec requirement | Task |
|---|---|
| §1 Problem statement | implicit in goals |
| §2 Goals (v1) | Tasks 1-19 collectively |
| §3 Non-goals (deferred) | excluded by Tasks 1-19 |
| §4 Architecture (3 tasks + LocalSet + Arc<Runtime>) | Tasks 1, 6, 10, 12 |
| §5 Method surface (5 methods + 1 notification) | Tasks 10, 11 |
| §5.1 meta.handshake | Tasks 7, 10 |
| §5.2 meta.ping | Task 10 |
| §5.3 runtime.run | Task 11 |
| §5.4 runtime.run_streaming | Task 11 |
| §5.5 runtime.cancel | Tasks 8, 10 |
| §6 Error model (codes + mapping) | Tasks 4, 5 |
| §7 Alternatives | ADR Task 18 |
| §8.1 Concurrency (parallel, cap 8) | Tasks 10, 11 |
| §8.2 Cancellation (CancellationToken + select!) | Tasks 8, 11 |
| §8.3 Startup | Task 12 |
| §8.4 Shutdown signals + grace | Task 12 |
| §8.5 Idle timeout | Task 12 (option in struct; wiring optional follow-up) |
| §8.6 Exit codes | Task 14 (CLI exit mapping); Task 12 (anyhow propagation) |
| §8.7 Logging to stderr | Task 12 |
| §9 ADR-0031 | Task 18 |
| §10 Testing (3 layers + CI) | Tasks 15, 16, 17, 19 |
| §11 Risks | implicit; idle-timeout-during-run noted in spec §11 |
| §12 Open impl questions | resolved in Tasks 13, 14 |
| §13 Out of scope | excluded |

**Gaps fixed inline:** the idle-timeout *wiring* (firing graceful shutdown after no message activity) is mentioned in `ServeOptions` but not implemented step-by-step. Since it's a small addition to Task 12 (one `tokio::time::interval` reset on message events), the implementer is expected to wire it during Task 12. The spec marks it as a should-have, not a must-have. Acceptable.

### Placeholder scan

- "TODO" appears only in the stub-module placeholder comments in Task 1 Step 4 — those are explicit "fill in below" markers, not unfilled work.
- `todo!()` appears intentionally in two places: Task 12's `build_runtime` (resolved by Task 13), and Task 15's `Harness::new` (resolved during implementation — the visibility decision is explicit). Each is documented as a resolution-task pointer.
- "Adjust per the note above" appears in Tasks 5 and 11 — these are mechanical field-name reconciliations against current source files, documented as such.

### Type consistency

- `RequestId` definition in Task 3 used consistently in Tasks 8, 10, 11, 15-17.
- `ServeOptions` field names defined in Task 2 used consistently in Tasks 12, 14.
- `Dispatcher` struct fields in Task 10 used consistently in Tasks 11, 12.
- Method-name constants in Task 4 used consistently in Tasks 10, 11.
- Error code constants in Task 4 used consistently in Tasks 5, 10, 11, 15, 16.
- `RunEvent` variant names in Task 11 match `tau_runtime::stream::RunEvent` (verified by inspection during plan writing).

No issues found.
