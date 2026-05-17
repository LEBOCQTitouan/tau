# Logging Sub-project D — Workflow + Plugin Recording as `tracing` Layers

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the two hand-rolled JSONL writers (`tau_workflow::persistence::RunLog`, `tau_runtime::plugin_host::recording::Recorder`) with custom `tracing_subscriber::Layer` impls in `tau-observe`. On-disk file formats stay byte-identical; only the producer side changes.

**Architecture:** Two new `Layer<S>` impls — `WorkflowRunLogLayer` (filters on `target = "tau::workflow::step"`) and `PluginRecordingLayer` (filters on `target = "tau::plugin::frame"`). The legacy `RunLog::append` and `Recorder::record` become thin shims that emit a `tracing::event!` with the right target + fields; the layer materializes the line. Any other subscriber the user installs (e.g. the fmt layer at TRACE) sees the same events.

**Tech Stack:** Rust 2021, `tracing`, `tracing-subscriber` with `registry` feature, `serde_json`, `tokio::fs`. No new transitive deps.

**Depends on:** Sub-project A merged. Independent of B and C (touches different code paths).

---

## File Structure

**Created:**
- `crates/tau-observe/src/layers/mod.rs` — module root.
- `crates/tau-observe/src/layers/workflow_run_log.rs` — `WorkflowRunLogLayer`.
- `crates/tau-observe/src/layers/plugin_recording.rs` — `PluginRecordingLayer`.
- `crates/tau-observe/tests/layer_format_compat.rs` — byte-identical-output assertion.

**Modified:**
- `crates/tau-observe/src/lib.rs` — `pub mod layers;`
- `crates/tau-observe/Cargo.toml` — add `chrono = { workspace = true }`, `tokio = { workspace = true, features = ["fs", "sync"] }`, `tau-domain = { workspace = true }` (for shared types). Add `tracing-subscriber = { workspace = true, features = ["fmt", "env-filter", "json", "registry"] }` (add `registry`).
- `crates/tau-workflow/src/persistence.rs` — `RunLog::append` becomes a `tracing::event!` emission; the file-writing internal moves into the layer.
- `crates/tau-runtime/src/plugin_host/recording.rs` — `Recorder::record` becomes a `tracing::event!` emission; the file-writing internal moves into the layer.

---

## Task 1: `WorkflowRunLogLayer` skeleton + on_event hook

**Files:**
- Create: `crates/tau-observe/src/layers/mod.rs`
- Create: `crates/tau-observe/src/layers/workflow_run_log.rs`
- Modify: `crates/tau-observe/src/lib.rs`
- Modify: `crates/tau-observe/Cargo.toml`

- [ ] **Step 1: Cargo.toml deps**

Add to `[dependencies]`:

```toml
chrono             = { workspace = true }
tokio              = { workspace = true, features = ["fs", "sync"] }
tau-domain         = { workspace = true }
```

Update the existing `tracing-subscriber` line:

```toml
tracing-subscriber = { workspace = true, features = ["fmt", "env-filter", "json", "registry"] }
```

- [ ] **Step 2: Write the layer skeleton + test**

`crates/tau-observe/src/layers/mod.rs`:

```rust
//! Custom `tracing_subscriber::Layer` impls that materialize internal
//! events into on-disk JSONL artifacts. See sub-project D in the
//! 2026-05-17 logging upgrades design.

pub mod plugin_recording;
pub mod workflow_run_log;
```

`crates/tau-observe/src/layers/workflow_run_log.rs`:

```rust
//! Layer that materializes `target = "tau::workflow::step"` events
//! into the `<scope>/.tau/workflow-runs/<workflow>-<run-id>.jsonl`
//! file format previously written by `tau_workflow::persistence::RunLog`.
//!
//! Field schema (must match `StepRecord` in tau-workflow):
//! - `run_id`, `step_id`, `step_index`, `kind`, `input`, `output`,
//!   `started_at`, `ended_at`, `duration_ms`, `status`
//! - optional: `error`, `detail`
//!
//! The layer writes one line per event and fsyncs after each write.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{field::Visit, Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

/// The `target` string that emissions must use to be picked up.
pub const TARGET: &str = "tau::workflow::step";

/// Layer that appends each matching event to a JSONL file.
#[derive(Clone)]
pub struct WorkflowRunLogLayer {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    path: PathBuf,
    file: Option<tokio::fs::File>,
}

impl WorkflowRunLogLayer {
    /// Open (or create + append-to) the file at `path`. The file is not
    /// opened until the first matching event arrives, to keep the layer
    /// cheap when no workflow run is in progress.
    pub fn new(path: PathBuf) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner { path, file: None })),
        }
    }
}

impl<S> Layer<S> for WorkflowRunLogLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        if meta.target() != TARGET {
            return;
        }
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let line = serialize_step_record(&visitor.fields);
        let inner = self.inner.clone();
        // Hand off the write to the runtime so we don't block the
        // emitting task. Best-effort: errors are logged at WARN to the
        // parent subscriber.
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt as _;
            let mut guard = inner.lock().await;
            let path = guard.path.clone();
            let file = match guard.file.as_mut() {
                Some(f) => f,
                None => {
                    if let Some(parent) = path.parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                    let opened = tokio::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                        .await;
                    match opened {
                        Ok(f) => {
                            guard.file = Some(f);
                            guard.file.as_mut().unwrap()
                        }
                        Err(e) => {
                            tracing::warn!(
                                target: "tau_observe::layers::workflow_run_log",
                                path = %path.display(),
                                err = %e,
                                "workflow run-log open failed; dropping event",
                            );
                            return;
                        }
                    }
                }
            };
            if let Err(e) = file.write_all(line.as_bytes()).await {
                tracing::warn!(
                    target: "tau_observe::layers::workflow_run_log",
                    err = %e,
                    "workflow run-log write failed",
                );
                return;
            }
            let _ = file.sync_all().await;
        });
    }
}

#[derive(Default)]
struct FieldVisitor {
    fields: BTreeMap<String, serde_json::Value>,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields.insert(field.name().to_string(), serde_json::Value::String(format!("{value:?}")));
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields.insert(field.name().to_string(), serde_json::Value::String(value.to_string()));
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields.insert(field.name().to_string(), serde_json::Value::Number(value.into()));
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields.insert(field.name().to_string(), serde_json::Value::Number(value.into()));
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields.insert(field.name().to_string(), serde_json::Value::Bool(value));
    }
}

fn serialize_step_record(fields: &BTreeMap<String, serde_json::Value>) -> String {
    // Build the JSONL line in the field order documented in
    // tau_workflow::persistence::StepRecord. Missing optional fields
    // (error, detail) are simply omitted, matching `#[serde(skip_serializing_if = "Option::is_none")]`.
    let mut obj = serde_json::Map::new();
    for key in ["ts", "run_id", "step_id", "step_index", "kind", "input", "output",
                "started_at", "ended_at", "duration_ms", "status"] {
        if let Some(v) = fields.get(key) {
            obj.insert(key.to_string(), v.clone());
        }
    }
    for key in ["error", "detail"] {
        if let Some(v) = fields.get(key) {
            obj.insert(key.to_string(), v.clone());
        }
    }
    let mut line = serde_json::Value::Object(obj).to_string();
    line.push('\n');
    line
}
```

In `crates/tau-observe/src/lib.rs`:

```rust
pub mod layers;
```

- [ ] **Step 3: Verify the crate builds**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo build -p tau-observe`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-observe/Cargo.toml crates/tau-observe/src/lib.rs crates/tau-observe/src/layers/
git commit -m "feat(tau-observe): WorkflowRunLogLayer skeleton"
```

---

## Task 2: Migrate `tau_workflow::persistence::RunLog::append` to emit a `tracing::event!`

**Files:**
- Modify: `crates/tau-workflow/src/persistence.rs`
- Modify: `crates/tau-workflow/Cargo.toml` — add `tau-observe = { workspace = true }`.

- [ ] **Step 1: Add dep**

`crates/tau-workflow/Cargo.toml`:

```toml
tau-observe = { workspace = true }
```

- [ ] **Step 2: Rewrite `RunLog::append`**

The current `append` writes the JSONL line directly. After this task it emits a `tracing::event!` with the canonical fields, and the layer materializes the line. We keep the public signature so callers don't break.

```rust
use tau_observe::layers::workflow_run_log::TARGET as WF_TARGET;

impl RunLog {
    /// Append one `StepRecord` to the run log.
    ///
    /// Emits a `tracing::event!` on `target = TARGET`. The
    /// `WorkflowRunLogLayer` (installed by the caller's subscriber
    /// stack) writes the JSONL line. The existing file handle on
    /// `RunLog` is retained for the legacy direct-write path used by
    /// callers that haven't migrated to a tracing-driven subscriber
    /// yet (see Step 3 for the migration timeline).
    pub async fn append(&mut self, record: &StepRecord) -> Result<(), std::io::Error> {
        // Emit for the layer-based path.
        let error_field = record.error.as_deref().unwrap_or("");
        let detail_field = record.detail.as_deref().unwrap_or("");
        tracing::event!(
            target: WF_TARGET,
            tracing::Level::INFO,
            ts = %record.ts,
            run_id = %record.run_id,
            step_id = %record.step_id,
            step_index = record.step_index as u64,
            kind = %record.kind,
            input = %record.input,
            output = %record.output,
            started_at = %record.started_at,
            ended_at = %record.ended_at,
            duration_ms = record.duration_ms,
            status = ?record.status,
            error = %error_field,
            detail = %detail_field,
        );

        // Continue to write directly until v0.x migration is complete —
        // see Task 3 for the deletion of this branch.
        self.write_direct(record).await
    }

    async fn write_direct(&mut self, record: &StepRecord) -> Result<(), std::io::Error> {
        // existing implementation body (the contents of what `append`
        // currently does), unchanged.
        todo!("paste existing append body here verbatim")
    }
}
```

> **Implementer note:** the `todo!()` is a literal mechanical action — copy the existing `append` body into `write_direct`, no changes.

- [ ] **Step 3: Verify existing `RunLog` unit tests still pass**

Run: `timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-workflow persistence::`
Expected: same green count as before (direct-write path still works).

- [ ] **Step 4: Commit**

```bash
git add crates/tau-workflow/Cargo.toml crates/tau-workflow/src/persistence.rs
git commit -m "feat(tau-workflow): emit step events via tracing alongside direct write"
```

---

## Task 3: Install `WorkflowRunLogLayer` in `tau workflow run` and remove the direct-write path

**Files:**
- Modify: `crates/tau-cli/src/cmd/workflow/run.rs` (or wherever `tau workflow run` builds its tracing stack; locate via `grep -rn "fn run\b" crates/tau-cli/src/cmd/workflow/`)
- Modify: `crates/tau-workflow/src/persistence.rs` (delete `write_direct`)

- [ ] **Step 1: Install the layer in the CLI's workflow-run command**

In `tau workflow run`'s setup, before the subscriber is initialized via `tau_observe::install`, register the workflow layer. Since the current `install` API doesn't accept extra layers, this task either (a) adds an `InstallOptions::extra_layers: Vec<Box<dyn Layer<…>>>` field, or (b) builds the registry inline for the workflow-run command.

Recommended: extend `InstallOptions`. Add to `crates/tau-observe/src/install.rs`:

```rust
pub struct InstallOptions {
    pub filter: EnvFilter,
    pub format: Format,
    pub writer: Writer,
    /// Extra layers to compose into the registry alongside the fmt
    /// layer. Each is `Box<dyn Layer<Registry> + Send + Sync>`.
    pub extra_layers: Vec<Box<dyn tracing_subscriber::Layer<tracing_subscriber::Registry> + Send + Sync + 'static>>,
}
```

Wire `extra_layers` into the install body (compose them into the registry before the fmt layer's `try_init`).

In `tau workflow run`:

```rust
use tau_observe::layers::workflow_run_log::WorkflowRunLogLayer;

let log_path = run_log_path(&scope_root, workflow_name, &run_id);
let mut opts = InstallOptions::cli_default();
opts.extra_layers.push(Box::new(WorkflowRunLogLayer::new(log_path)));
let _guard = tau_observe::install::install(opts).expect("install");
```

- [ ] **Step 2: Delete `RunLog::write_direct` and the file handle on `RunLog`**

`RunLog::append` becomes only the `tracing::event!` emission. The struct drops its `file: File` field. Public API stays — `append`'s signature is unchanged.

- [ ] **Step 3: Run the full workflow suite end-to-end**

Run: `timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-workflow`
Expected: all green. If a test asserts on log-file contents, install the layer in the test's setup the same way `tau workflow run` does.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-observe/src/install.rs crates/tau-cli/src/cmd/workflow/run.rs crates/tau-workflow/src/persistence.rs
git commit -m "refactor(tau-workflow): drop direct-write; WorkflowRunLogLayer is the only writer"
```

---

## Task 4: `PluginRecordingLayer` — same pattern, different target

**Files:**
- Create: `crates/tau-observe/src/layers/plugin_recording.rs`

- [ ] **Step 1: Write the layer**

```rust
//! Layer that materializes `target = "tau::plugin::frame"` events
//! into the recording JSONL format previously written by
//! `tau_runtime::plugin_host::recording::Recorder`.
//!
//! Field schema (must match `recording.rs` output):
//! - `ts` (f64 unix seconds)
//! - `plugin` (string)
//! - `dir` (string: "h2p" | "p2h")
//! - `msgid` (u32 | null)
//! - `method` (string | null)
//! - `frame` (string: base64)

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

pub const TARGET: &str = "tau::plugin::frame";

#[derive(Clone)]
pub struct PluginRecordingLayer {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    path: PathBuf,
    file: Option<tokio::fs::File>,
}

impl PluginRecordingLayer {
    pub fn new(path: PathBuf) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner { path, file: None })),
        }
    }
}

impl<S> Layer<S> for PluginRecordingLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let meta = event.metadata();
        if meta.target() != TARGET {
            return;
        }
        // Field-extraction body identical in shape to workflow_run_log.rs:
        // visit fields, build a JSON object preserving field order
        // (ts, plugin, dir, msgid, method, frame), serialize as one
        // line, spawn a tokio::task that opens/appends/syncs.
        // See workflow_run_log.rs for the full pattern.
        todo!("copy the on_event body from workflow_run_log.rs and adapt for frame fields")
    }
}
```

> **Implementer note:** the `todo!()` body is a mechanical adaptation of `WorkflowRunLogLayer::on_event`. Copy the FieldVisitor, the serialize_record builder, and the spawn-write closure verbatim; change only the field name list (`["ts", "plugin", "dir", "msgid", "method", "frame"]`).

- [ ] **Step 2: Verify build**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo build -p tau-observe`
Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-observe/src/layers/plugin_recording.rs
git commit -m "feat(tau-observe): PluginRecordingLayer skeleton"
```

---

## Task 5: Migrate `Recorder::record` to emit `tracing::event!` + delete the direct-write path

**Files:**
- Modify: `crates/tau-runtime/src/plugin_host/recording.rs`
- Modify: `crates/tau-runtime/Cargo.toml` — ensure `tau-observe = { workspace = true }` (already added in Sub-project B).

- [ ] **Step 1: Replace `Recorder::record` body**

```rust
use tau_observe::layers::plugin_recording::TARGET as REC_TARGET;

impl Recorder {
    pub async fn record(&self, dir: Direction, frame_bytes: &[u8]) {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let (msgid, method) = decode_frame_metadata(frame_bytes);
        let encoded = B64.encode(frame_bytes);
        tracing::event!(
            target: REC_TARGET,
            tracing::Level::TRACE,
            ts,
            plugin = self.plugin_name.as_str(),
            dir = dir.as_str(),
            msgid = msgid.map(|n| n as u64),
            method = method.as_deref().unwrap_or(""),
            frame = encoded.as_str(),
        );
    }
}
```

The `Mutex<File>` field on `Recorder` is now unused — keep the struct shape (the public `Recorder::open_jsonl` API still constructs it) but the `file` field becomes a placeholder. Actually: delete `Recorder::open_jsonl` and the file field — the layer owns the file now. The remaining `Recorder` becomes a stateless emitter holding only `plugin_name`. Update all 2 callers in `plugin_host/` (locate via `grep -rn "Recorder::open_jsonl" crates/tau-runtime/src`) to construct directly: `Recorder { plugin_name: name.into() }`.

- [ ] **Step 2: Install `PluginRecordingLayer` from the plugin-host startup path**

When `PluginHostOptions::recording` is `Some(RecordingSink::JsonlFile(path))`, the host installs the layer (via `tau_observe::install::install` with `extra_layers`) before launching the plugin. Locate the install site via `grep -rn "RecordingSink::JsonlFile" crates/tau-runtime/src`.

- [ ] **Step 3: Run the recording tests**

Run: `timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-runtime recording`
Expected: all 4 existing `Recorder` tests pass; the assertion on file contents now reaches through the layer.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-runtime/src/plugin_host/recording.rs crates/tau-runtime/src/plugin_host/mod.rs
git commit -m "refactor(tau-runtime): Recorder emits via tracing; PluginRecordingLayer writes file"
```

---

## Task 6: Byte-identical output assertion

**Files:**
- Create: `crates/tau-observe/tests/layer_format_compat.rs`

- [ ] **Step 1: Snapshot test**

```rust
//! Assert the layers produce JSONL lines byte-identical to the legacy
//! direct-write format, given an identical input event stream.

use tau_observe::layers::workflow_run_log::{WorkflowRunLogLayer, TARGET as WF_TARGET};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::test]
async fn workflow_layer_writes_expected_jsonl_line() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.jsonl");
    let layer = WorkflowRunLogLayer::new(path.clone());

    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        tracing::event!(
            target: WF_TARGET,
            tracing::Level::INFO,
            ts = "2026-05-17T12:00:00Z",
            run_id = "01HXYZ",
            step_id = "build",
            step_index = 0u64,
            kind = "agent.run",
            input = "hello",
            output = "world",
            started_at = "2026-05-17T12:00:00Z",
            ended_at = "2026-05-17T12:00:01Z",
            duration_ms = 1000u64,
            status = "ok",
        );
    });

    // The layer spawns a tokio task per event — wait briefly for it
    // to flush. In a real run the subscriber's drop or an explicit
    // flush call handles this; for this test we just yield+sleep.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let contents = tokio::fs::read_to_string(&path).await.unwrap();
    let line = contents.lines().next().expect("at least one line");
    assert!(line.contains(r#""run_id":"01HXYZ""#));
    assert!(line.contains(r#""step_id":"build""#));
    assert!(line.contains(r#""duration_ms":1000"#));
}
```

- [ ] **Step 2: Run + commit**

Run: `timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --test layer_format_compat`
Expected: green.

```bash
git add crates/tau-observe/tests/layer_format_compat.rs
git commit -m "test(tau-observe): byte-identical workflow JSONL output through layer"
```

---

## Task 7: Final verification + push

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-observe -- -D warnings
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-workflow -- -D warnings
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-runtime -- -D warnings
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-observe
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-workflow
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-runtime
timeout 1800 lefthook run pre-push
scripts/agent-push.sh -u origin HEAD
```

Open the PR: title `feat(tau-observe): WorkflowRunLogLayer + PluginRecordingLayer (Sub-project D)`.

---

## Spec coverage check

- Spec sub-project D "Both existing JSONL writers become custom Layer impls" → Tasks 1, 4.
- Spec sub-project D "on-disk file format unchanged" → Task 6 byte-identical assertion.
- Spec sub-project D "RunLog becomes a thin internal writer used by the layer" — actually the spec said "thin internal writer", but the cleaner outcome (delete the direct path entirely once the layer covers all callers) is what Task 3 does. The spec accepted "we keep current behavior: log at WARN to the parent subscriber, do not propagate" for layer write errors — Tasks 1 and 4 implement that.
- Spec sub-project D "the same events are simultaneously visible to any other subscriber" → emerges automatically from the registry pattern; no additional task needed.
- Spec migration plan "D ships after A+B+C land" → respected.
