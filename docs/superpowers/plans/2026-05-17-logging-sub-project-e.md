# Logging Sub-project E — `tracing-appender` Non-Blocking Writer

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a feature-gated non-blocking writer + log rotation to `tau-observe::install`, backed by `tracing-appender`. Off by default; opted into via `InstallOptions::non_blocking = true` or `--log-non-blocking` / `TAU_LOG_NON_BLOCKING=1`.

**Architecture:** New optional dep `tracing-appender = "0.2"` behind feature `non_blocking`. `InstallOptions` gains a `non_blocking` bool and an optional `file_path: Option<PathBuf>` with associated `rotation: Rotation`. `InstallGuard` holds the appender's `WorkerGuard` so the writer thread flushes on drop.

**Tech Stack:** Rust 2021, `tracing-appender = "0.2"`. No other new transitive deps.

**Depends on:** Sub-project A merged. Independent of B, C, D.

---

## File Structure

**Created:**
- `crates/tau-observe/tests/non_blocking_load.rs` — emits 100k events and asserts the producer is not blocked.

**Modified:**
- `crates/tau-observe/Cargo.toml` — add optional `tracing-appender = "0.2"`; declare feature `non_blocking = ["dep:tracing-appender"]`.
- `crates/tau-observe/src/install.rs` — extend `InstallOptions` with `non_blocking`, `file_path`, `rotation`; gate the appender code on `cfg(feature = "non_blocking")`; have `InstallGuard` hold the appender `WorkerGuard`.
- `crates/tau-cli/src/cli.rs` — add `--log-non-blocking` boolean flag, plumb it into `tracing::build_filter`/`install`.
- `crates/tau-cli/src/tracing.rs` — read the flag, set `InstallOptions::non_blocking`.

---

## Task 1: Feature flag + dependency

**Files:**
- Modify: `crates/tau-observe/Cargo.toml`

- [ ] **Step 1: Add the optional dep + feature**

```toml
[dependencies]
# … existing …
tracing-appender   = { version = "0.2", optional = true }

[features]
default = []
test-fixtures = []
non_blocking = ["dep:tracing-appender"]
```

- [ ] **Step 2: Verify both feature on / off compile**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo check -p tau-observe
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo check -p tau-observe --features non_blocking
```

Expected: clean both times.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-observe/Cargo.toml
git commit -m "build(tau-observe): tracing-appender behind non_blocking feature"
```

---

## Task 2: Extend `InstallOptions` + `InstallGuard`

**Files:**
- Modify: `crates/tau-observe/src/install.rs`

- [ ] **Step 1: Add the new fields + types**

Inside `install.rs`:

```rust
/// File rotation policy when writing to a file sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    /// No rotation — append forever to a single file.
    #[default]
    Never,
    /// Roll over each day at UTC midnight (filename gains a date suffix).
    Daily,
    /// Roll over each hour (filename gains a date+hour suffix).
    Hourly,
}

pub struct InstallOptions {
    pub filter: EnvFilter,
    pub format: Format,
    pub writer: Writer,
    pub extra_layers: Vec<Box<dyn tracing_subscriber::Layer<tracing_subscriber::Registry> + Send + Sync + 'static>>,
    /// When `true`, the fmt layer writes through a non-blocking MPSC
    /// channel. Requires feature `non_blocking`.
    pub non_blocking: bool,
    /// Optional file sink. When set, overrides [`Writer`]. Requires
    /// feature `non_blocking`.
    pub file_path: Option<std::path::PathBuf>,
    /// File rotation policy. Ignored unless `file_path` is set.
    pub rotation: Rotation,
}

impl InstallOptions {
    // Existing cli_default / plugin_sdk constructors gain defaults:
    pub fn cli_default() -> Self {
        Self {
            filter: crate::filter::env_or_directive("tau=info"),
            format: Format::Human,
            writer: Writer::Stderr,
            extra_layers: Vec::new(),
            non_blocking: false,
            file_path: None,
            rotation: Rotation::Never,
        }
    }
    // … and plugin_sdk() likewise …
}
```

- [ ] **Step 2: Extend `InstallGuard` to hold the appender guard**

```rust
pub struct InstallGuard {
    #[cfg(feature = "non_blocking")]
    _appender_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
    _private: (),
}
```

- [ ] **Step 3: Wire the appender into `install()`**

The body of `install()` branches on `opts.non_blocking`:

```rust
#[cfg(feature = "non_blocking")]
{
    if opts.non_blocking {
        return install_non_blocking(opts);
    }
}
// existing blocking install body, returning InstallGuard {
//     #[cfg(feature = "non_blocking")] _appender_guard: None,
//     _private: (),
// }
```

`install_non_blocking` (also gated on `cfg(feature = "non_blocking")`) constructs an `appender` from `opts.file_path` + `opts.rotation`, wraps it in `tracing_appender::non_blocking()`, passes the resulting writer into `fmt::layer().with_writer(...)`, calls `try_init`, and stuffs the returned `WorkerGuard` into the `InstallGuard`. Sketch:

```rust
#[cfg(feature = "non_blocking")]
fn install_non_blocking(opts: InstallOptions) -> Result<InstallGuard, InstallError> {
    use tracing_appender::rolling;
    let path = opts.file_path.clone().expect("non_blocking requires file_path");
    let dir = path.parent().expect("file_path has no parent").to_path_buf();
    let prefix = path.file_name().expect("file_path has no filename").to_string_lossy().to_string();
    let file_appender = match opts.rotation {
        Rotation::Never => rolling::never(&dir, &prefix),
        Rotation::Daily => rolling::daily(&dir, &prefix),
        Rotation::Hourly => rolling::hourly(&dir, &prefix),
    };
    let (writer, worker_guard) = tracing_appender::non_blocking(file_appender);

    use tracing_subscriber::fmt;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let registry = tracing_subscriber::registry().with(opts.filter);
    let result = match opts.format {
        Format::Human => registry
            .with(fmt::layer().with_writer(writer))
            .try_init(),
        Format::Json => registry
            .with(fmt::layer().json().with_writer(writer).with_current_span(true).with_span_list(false))
            .try_init(),
    };
    let _ = result; // try_init Err means a subscriber was already installed — fall through
    Ok(InstallGuard {
        _appender_guard: Some(worker_guard),
        _private: (),
    })
}
```

- [ ] **Step 4: Unit test of the new fields' defaults**

```rust
#[test]
fn cli_default_has_non_blocking_off_and_no_file() {
    let opts = InstallOptions::cli_default();
    assert!(!opts.non_blocking);
    assert!(opts.file_path.is_none());
    assert_eq!(opts.rotation, Rotation::Never);
}
```

- [ ] **Step 5: Verify build with feature on**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --features non_blocking install::tests
```

Expected: green.

- [ ] **Step 6: Commit**

```bash
git add crates/tau-observe/src/install.rs
git commit -m "feat(tau-observe): InstallOptions { non_blocking, file_path, rotation }"
```

---

## Task 3: Load test — 100k events, producer-side latency bounded

**Files:**
- Create: `crates/tau-observe/tests/non_blocking_load.rs`

- [ ] **Step 1: Write the test (gated on the feature)**

```rust
//! Producer-side latency assertion for the non_blocking install path.
//!
//! Emits 100,000 INFO events to a file-backed subscriber and asserts
//! no single emission took longer than 10 ms. The exact bound is
//! generous; the point is to catch a regression where the writer
//! goes back to blocking semantics.

#![cfg(feature = "non_blocking")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use tau_observe::install::{install, Format, InstallOptions, Rotation, Writer};
use tau_observe::filter::env_or_directive;

#[test]
fn producer_latency_stays_under_10ms_per_event() {
    let tmp = tempfile::tempdir().unwrap();
    let log_path: PathBuf = tmp.path().join("load.log");

    let opts = InstallOptions {
        filter: env_or_directive("info"),
        format: Format::Human,
        writer: Writer::Stderr, // ignored when file_path is set
        extra_layers: Vec::new(),
        non_blocking: true,
        file_path: Some(log_path.clone()),
        rotation: Rotation::Never,
    };
    let _guard = install(opts).expect("install");

    let mut worst = Duration::ZERO;
    for i in 0..100_000 {
        let start = Instant::now();
        tracing::info!(idx = i, "load.test_event");
        let elapsed = start.elapsed();
        if elapsed > worst {
            worst = elapsed;
        }
    }
    assert!(
        worst < Duration::from_millis(10),
        "worst-case producer latency {:?} exceeded 10ms",
        worst
    );
}
```

- [ ] **Step 2: Run with the feature on**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-observe --features non_blocking --test non_blocking_load
```

Expected: passes; worst-case under 10 ms (typically <1 ms on a warm machine).

- [ ] **Step 3: Commit**

```bash
git add crates/tau-observe/tests/non_blocking_load.rs
git commit -m "test(tau-observe): producer-latency bound for non_blocking writer"
```

---

## Task 4: CLI surface — `--log-non-blocking` + `TAU_LOG_NON_BLOCKING`

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` (the `Cli` struct + clap derive)
- Modify: `crates/tau-cli/src/tracing.rs` (`install` body)
- Modify: `crates/tau-cli/Cargo.toml` — `tau-observe = { workspace = true, features = ["non_blocking"] }`.

- [ ] **Step 1: Update tau-cli's dep on tau-observe**

```toml
tau-observe = { workspace = true, features = ["non_blocking"] }
```

- [ ] **Step 2: Add the CLI flag**

In `crates/tau-cli/src/cli.rs`, add to the `Cli` struct:

```rust
/// Write logs through a non-blocking MPSC channel (high-throughput).
/// Backed by `tracing-appender`. When set, `--log-file` must also be
/// set. Env: `TAU_LOG_NON_BLOCKING=1`.
#[arg(long, env = "TAU_LOG_NON_BLOCKING")]
pub log_non_blocking: bool,

/// Write logs to the given file instead of stderr. Required when
/// `--log-non-blocking` is set.
#[arg(long, env = "TAU_LOG_FILE")]
pub log_file: Option<std::path::PathBuf>,

/// File rotation policy: never, daily, hourly. Default: never.
#[arg(long, env = "TAU_LOG_ROTATION", default_value = "never")]
pub log_rotation: tau_observe::install::Rotation,
```

Add a `clap::ValueEnum` impl for `Rotation` in `tau-observe`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Rotation { /* … */ }
```

> If adding a `clap` dep to `tau-observe` is undesirable, move the enum's `ValueEnum` impl into `tau-cli` instead and translate flag values manually. The plan recommends the inline `derive` because the tradeoff favors a single canonical type.

- [ ] **Step 3: Update `install` in `tau-cli/src/tracing.rs`**

```rust
pub fn install(cli: &Cli) {
    let mut opts = InstallOptions {
        filter: build_filter(cli),
        format: Format::Human,
        writer: Writer::Stderr,
        extra_layers: Vec::new(),
        non_blocking: cli.log_non_blocking,
        file_path: cli.log_file.clone(),
        rotation: cli.log_rotation,
    };
    if opts.non_blocking && opts.file_path.is_none() {
        eprintln!("error: --log-non-blocking requires --log-file");
        std::process::exit(2);
    }
    let _guard = observe_install(opts).expect("install");
    // NOTE: dropping the guard at end-of-function would flush and exit
    // the appender thread. We leak it deliberately for the lifetime of
    // the process — `tau`'s exit triggers a `std::process::exit` that
    // bypasses Drop. The non-blocking appender's worker thread is
    // joined by `WorkerGuard::drop`, which would deadlock at exit.
    // See tracing-appender docs.
    std::mem::forget(_guard);
}
```

- [ ] **Step 4: Test**

`crates/tau-cli/tests/log_non_blocking_cli.rs`:

```rust
use assert_cmd::Command;

#[test]
fn non_blocking_without_log_file_exits_with_error_message() {
    let tmp = tempfile::tempdir().unwrap();
    Command::cargo_bin("tau")
        .unwrap()
        .args(["--log-non-blocking", "list", "packages"])
        .env("HOME", tmp.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("--log-non-blocking requires --log-file"));
}
```

- [ ] **Step 5: Run + commit**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-cli log_non_blocking_cli
git add crates/tau-cli/Cargo.toml crates/tau-cli/src/cli.rs crates/tau-cli/src/tracing.rs crates/tau-cli/tests/log_non_blocking_cli.rs crates/tau-observe/src/install.rs
git commit -m "feat(tau-cli): --log-non-blocking + --log-file flags"
```

---

## Task 5: Final verification + push

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-observe --features non_blocking -- -D warnings
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-cli -- -D warnings
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-observe --features non_blocking
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-cli
timeout 1800 lefthook run pre-push
scripts/agent-push.sh -u origin HEAD
```

PR: `feat(tau-observe): non_blocking writer + rotation via tracing-appender (Sub-project E)`.

---

## Spec coverage check

- Spec sub-project E "`tracing-appender = "0.2"` optional, feature `non_blocking`" → Task 1.
- Spec sub-project E "`InstallOptions::non_blocking = true` or `--log-non-blocking` CLI flag or env `TAU_LOG_NON_BLOCKING=1`" → Task 4.
- Spec sub-project E "Writer::File uses tracing_appender::rolling::daily" → Task 2, with `Rotation::{Never, Daily, Hourly}` covering the common cases.
- Spec sub-project E "channel fills → events drop with warning" → inherits from `tracing-appender::non_blocking` default; no extra task needed.
- Spec testing E "100k events under load … producer not blocked >10ms" → Task 3.
