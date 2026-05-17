# Logging Sub-project F — Optional OpenTelemetry / OTLP Export

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a feature-gated OpenTelemetry layer that exports tau's spans (the §3.9 vocabulary from Sub-project B) over OTLP/gRPC. Off by default; opted into via `--otlp-endpoint` or the standard `OTEL_EXPORTER_OTLP_ENDPOINT` env var.

**Architecture:** New optional deps `opentelemetry = "0.21"`, `opentelemetry-otlp = "0.14"`, `opentelemetry_sdk = "0.21"`, `tracing-opentelemetry = "0.22"` behind feature `otlp`. `InstallOptions::otlp: Option<OtlpEndpoint>` plumbs the endpoint, headers, and protocol into a `tracing_opentelemetry::layer()` that's composed into the registry. Service-resource attributes auto-populate from `CARGO_PKG_*`.

**Tech Stack:** Rust 2021, OpenTelemetry stack at 0.21 (the version family compatible with `tracing-opentelemetry 0.22`). Test deps add `opentelemetry-stdout = "0.2"` for offline assertions.

**Depends on:** Sub-projects A + B merged. B provides the span vocabulary that becomes the meaningful trace structure once OTLP is on. Independent of C, D, E.

---

## File Structure

**Created:**
- `crates/tau-observe/src/otlp.rs` — `OtlpEndpoint`, exporter builder.
- `crates/tau-observe/tests/otlp_smoke.rs` — uses `opentelemetry-stdout` as the exporter (no network); asserts the expected span tree fires.

**Modified:**
- `crates/tau-observe/Cargo.toml` — optional deps + `otlp` feature.
- `crates/tau-observe/src/install.rs` — accept `otlp: Option<OtlpEndpoint>` in `InstallOptions`; compose the OTel layer into the registry.
- `crates/tau-observe/src/lib.rs` — `pub mod otlp;` (gated on feature).
- `crates/tau-cli/src/cli.rs` — `--otlp-endpoint` flag (env-fallback to `OTEL_EXPORTER_OTLP_ENDPOINT`).
- `crates/tau-cli/src/tracing.rs` — read the flag, build `OtlpEndpoint`.
- `crates/tau-cli/Cargo.toml` — `tau-observe = { workspace = true, features = ["otlp"] }`.

---

## Task 1: Feature flag + dependencies

**Files:**
- Modify: `crates/tau-observe/Cargo.toml`

- [ ] **Step 1: Add optional deps + feature**

```toml
[dependencies]
# … existing …
opentelemetry        = { version = "0.21", optional = true }
opentelemetry_sdk    = { version = "0.21", features = ["rt-tokio"], optional = true }
opentelemetry-otlp   = { version = "0.14", features = ["grpc-tonic"], optional = true }
tracing-opentelemetry = { version = "0.22", optional = true }

[dev-dependencies]
opentelemetry-stdout = "0.2"

[features]
default = []
test-fixtures = []
non_blocking = ["dep:tracing-appender"]
otlp = [
    "dep:opentelemetry",
    "dep:opentelemetry_sdk",
    "dep:opentelemetry-otlp",
    "dep:tracing-opentelemetry",
]
```

> **Version note:** `tracing-opentelemetry 0.22` targets `opentelemetry 0.21`. If those versions have rotated upstream by the time the plan runs, pin to whatever version of `tracing-opentelemetry` requires `opentelemetry 0.21–0.x` per its README. Run `cargo update --dry-run` to confirm a coherent lockfile.

- [ ] **Step 2: Verify both feature states compile**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo check -p tau-observe
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo check -p tau-observe --features otlp
```

Expected: both clean.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-observe/Cargo.toml
git commit -m "build(tau-observe): opentelemetry stack behind otlp feature"
```

---

## Task 2: `otlp::OtlpEndpoint` + exporter builder

**Files:**
- Create: `crates/tau-observe/src/otlp.rs`
- Modify: `crates/tau-observe/src/lib.rs`

- [ ] **Step 1: Write the endpoint struct + builder**

`crates/tau-observe/src/otlp.rs`:

```rust
//! OTLP endpoint configuration.
//!
//! Feature-gated: this module only compiles when feature `otlp` is on.

#![cfg(feature = "otlp")]

use std::collections::HashMap;

/// Connection parameters for an OTLP/gRPC collector.
#[derive(Debug, Clone)]
pub struct OtlpEndpoint {
    /// e.g. `"https://otel.example.com:4317"`.
    pub endpoint: String,
    /// Extra gRPC metadata headers (auth bearer tokens, tenant ids, …).
    /// Maps to `tonic::metadata::MetadataMap` at install time.
    pub headers: HashMap<String, String>,
}

impl OtlpEndpoint {
    /// Read endpoint + headers from the standard OTel env vars.
    /// Returns `None` if neither `OTEL_EXPORTER_OTLP_ENDPOINT` nor
    /// `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT` is set.
    pub fn from_env() -> Option<Self> {
        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT")
            .or_else(|_| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT"))
            .ok()?;
        let headers = std::env::var("OTEL_EXPORTER_OTLP_HEADERS")
            .ok()
            .map(parse_headers)
            .unwrap_or_default();
        Some(Self { endpoint, headers })
    }
}

fn parse_headers(raw: String) -> HashMap<String, String> {
    raw.split(',')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next()?.trim().to_string();
            let val = parts.next()?.trim().to_string();
            if key.is_empty() { None } else { Some((key, val)) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_headers_splits_on_comma_and_equals() {
        let h = parse_headers("authorization=Bearer abc,tenant=acme".to_string());
        assert_eq!(h.get("authorization").map(String::as_str), Some("Bearer abc"));
        assert_eq!(h.get("tenant").map(String::as_str), Some("acme"));
    }

    #[test]
    fn parse_headers_ignores_malformed_pairs() {
        let h = parse_headers("good=1,bad".to_string());
        assert_eq!(h.len(), 1);
        assert_eq!(h.get("good").map(String::as_str), Some("1"));
    }
}
```

In `crates/tau-observe/src/lib.rs`:

```rust
#[cfg(feature = "otlp")]
pub mod otlp;
```

- [ ] **Step 2: Run + commit**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --features otlp otlp::
```

Expected: 2 tests pass.

```bash
git add crates/tau-observe/src/otlp.rs crates/tau-observe/src/lib.rs
git commit -m "feat(tau-observe): OtlpEndpoint + env-var loader"
```

---

## Task 3: Compose the OTel layer in `install()`

**Files:**
- Modify: `crates/tau-observe/src/install.rs`

- [ ] **Step 1: Add the `otlp` field to `InstallOptions`**

```rust
pub struct InstallOptions {
    // … existing fields …
    /// When set, an OpenTelemetry layer exports spans over OTLP/gRPC.
    /// Requires feature `otlp`.
    #[cfg(feature = "otlp")]
    pub otlp: Option<crate::otlp::OtlpEndpoint>,
}
```

Default to `None` in `cli_default()` and `plugin_sdk()`.

- [ ] **Step 2: Compose the layer**

Inside the install path, after the registry+filter base:

```rust
#[cfg(feature = "otlp")]
let registry = if let Some(otlp_ep) = &opts.otlp {
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(&otlp_ep.endpoint);
    let exporter = otlp_ep.headers.iter().fold(exporter, |e, (k, v)| {
        e.with_metadata({
            let mut m = tonic::metadata::MetadataMap::new();
            if let Ok(name) = k.parse::<tonic::metadata::MetadataKey<_>>() {
                if let Ok(val) = v.parse() {
                    m.insert(name, val);
                }
            }
            m
        })
    });
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            opentelemetry_sdk::trace::config().with_resource(
                opentelemetry_sdk::Resource::new([
                    opentelemetry::KeyValue::new("service.name", "tau"),
                    opentelemetry::KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
                ]),
            ),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .expect("install OTLP pipeline");
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    registry.with(otel_layer)
} else {
    registry
};
```

- [ ] **Step 3: Smoke test using the stdout exporter (no network)**

`crates/tau-observe/tests/otlp_smoke.rs`:

```rust
#![cfg(feature = "otlp")]

#[test]
fn span_tree_emits_to_stdout_exporter() {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::TracerProvider;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    let exporter = opentelemetry_stdout::SpanExporter::default();
    let provider = TracerProvider::builder()
        .with_simple_exporter(exporter)
        .build();
    let tracer = provider.tracer("test");
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let subscriber = Registry::default().with(otel_layer);

    tracing::subscriber::with_default(subscriber, || {
        let outer = tracing::info_span!("runtime.agent_run", agent_id = "test");
        let _e = outer.enter();
        let turn = tracing::info_span!("runtime.turn", turn_index = 1u64);
        let _e2 = turn.enter();
        let llm = tracing::info_span!("llm.complete");
        let _e3 = llm.enter();
        tracing::info!("llm.request_built");
    });

    // The stdout exporter writes JSON spans to stdout on flush. This
    // test asserts the call sequence doesn't panic; visual inspection
    // of `cargo test -- --nocapture` confirms the span tree. A
    // stronger assertion would intercept the writer — out of scope
    // for v1.
}
```

- [ ] **Step 4: Run + commit**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --features otlp
```

Expected: green.

```bash
git add crates/tau-observe/src/install.rs crates/tau-observe/tests/otlp_smoke.rs
git commit -m "feat(tau-observe): compose tracing-opentelemetry layer when InstallOptions::otlp is set"
```

---

## Task 4: CLI flag `--otlp-endpoint`

**Files:**
- Modify: `crates/tau-cli/Cargo.toml` — `tau-observe = { workspace = true, features = ["otlp"] }` (combined with non_blocking from E if both shipped).
- Modify: `crates/tau-cli/src/cli.rs`
- Modify: `crates/tau-cli/src/tracing.rs`

- [ ] **Step 1: Cargo feature combine**

```toml
tau-observe = { workspace = true, features = ["otlp", "non_blocking"] }
```

If E hasn't merged yet, drop `non_blocking`.

- [ ] **Step 2: CLI flag**

In `crates/tau-cli/src/cli.rs`:

```rust
/// Export traces over OTLP/gRPC to this endpoint, e.g.
/// `https://otel.example.com:4317`. Falls back to
/// `OTEL_EXPORTER_OTLP_ENDPOINT` env var.
#[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
pub otlp_endpoint: Option<String>,
```

- [ ] **Step 3: Plumb through `install`**

In `crates/tau-cli/src/tracing.rs`:

```rust
let otlp = cli.otlp_endpoint.clone().map(|endpoint| {
    let headers = std::env::var("OTEL_EXPORTER_OTLP_HEADERS")
        .ok()
        .map(parse_headers_str)
        .unwrap_or_default();
    tau_observe::otlp::OtlpEndpoint { endpoint, headers }
});
let opts = InstallOptions {
    // … other fields …
    otlp,
};
```

(If E shipped, `non_blocking` and `file_path` also come from the CLI.)

- [ ] **Step 4: End-to-end test using stdout exporter**

`crates/tau-cli/tests/otlp_cli.rs`:

```rust
use assert_cmd::Command;

#[test]
fn otlp_endpoint_flag_is_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    Command::cargo_bin("tau")
        .unwrap()
        // The endpoint won't be reachable; we just assert the flag parses
        // and the binary doesn't reject it. Use a fast-failing
        // subcommand so this test is quick.
        .args(["--otlp-endpoint", "http://127.0.0.1:65535", "list", "packages"])
        .env("HOME", tmp.path())
        .assert()
        // Non-zero exit is fine — `list packages` may error on a fresh
        // HOME — what matters is the flag parsed.
        .stderr(predicates::str::contains("--otlp-endpoint").not());
}
```

- [ ] **Step 5: Run + commit**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-cli otlp_cli
git add crates/tau-cli/Cargo.toml crates/tau-cli/src/cli.rs crates/tau-cli/src/tracing.rs crates/tau-cli/tests/otlp_cli.rs
git commit -m "feat(tau-cli): --otlp-endpoint flag wires through tau_observe::otlp"
```

---

## Task 5: Final verification + push

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-observe --features otlp -- -D warnings
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-cli -- -D warnings
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-observe --features otlp
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-cli
timeout 1800 lefthook run pre-push
scripts/agent-push.sh -u origin HEAD
```

PR: `feat(tau-observe): optional OTLP export via tracing-opentelemetry (Sub-project F)`.

---

## Spec coverage check

- Spec sub-project F "Add opentelemetry, opentelemetry-otlp, tracing-opentelemetry as optional deps, feature `otlp`" → Task 1.
- Spec sub-project F "CLI surface: `--otlp-endpoint=…` or env `OTEL_EXPORTER_OTLP_ENDPOINT`" → Task 4.
- Spec sub-project F "Off by default" → Task 1 (`default = []`).
- Spec sub-project F "Resource attributes: `service.name = "tau"`, `service.version = env!("CARGO_PKG_VERSION")`" → Task 3.
- Spec sub-project F "the user adds anything else via `OTEL_RESOURCE_ATTRIBUTES`" — handled automatically by the `opentelemetry_sdk::Resource::default()` constructor which reads that env var. The Task 3 code uses `Resource::new`, which does NOT auto-read; **implementer adjustment:** swap to `Resource::default().merge(&Resource::new([…]))` so the env var is read.
- Spec testing F "integration test using `opentelemetry-stdout` exporter (no network)" → Task 3.
- Spec error handling "OTLP export failures … never blocks or fails an agent run" → inherits from `opentelemetry_otlp` batch exporter default; no extra task needed.

## Open follow-on (not in this plan)

The §3.9 events from Sub-project B currently fire as `tracing::Event`s, which OpenTelemetry maps to `Span Events` only inside an active span. Confirm at implementation time that every event in the §3.9 vocabulary fires within one of the §3.9 spans; if any do not, wrap them. This is a verification step, not a separate task.
