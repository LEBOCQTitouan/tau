# Plugin Loading (Phase 1 sub-project 1) Implementation Plan

> **STATUS — COMPLETE.** All 26 tasks shipped via subagent-driven
> execution on branch `feat/plugin-loading-spec`. ADR-0008 Accepted at
> commit `ddf8057`. Per the project's plan-checkbox-reconciliation
> convention, individual `- [ ]` checkboxes below remain unticked —
> the authoritative record is the git log on this branch. PR #9
> squash-merged into `main` 2026-04-28.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the plugin loading mechanism — out-of-process IPC over MessagePack-RPC on stdio with long-lived multiplexed plugin processes — and exercise it end-to-end with two toy plugins (`echo-llm`, `echo-tool`). Closes the [ADR-0007](../../decisions/0007-tau-cli.md) §18 "plugin loading deferred" gap. First sub-project of Phase 1.

**Architecture:** Two new workspace crates (`tau-plugin-protocol`: pure wire types and framing; `tau-plugin-sdk`: per-port plugin-author runners using the strategy / template-method pattern with no proc macros). One amendment per existing crate: `tau-domain` (`PluginManifest`, `PortKind`, `PluginKind`); `tau-pkg` (build-on-install for `kind = "rust-cargo"`); `tau-runtime` (new `plugin_host` module producing `Arc<dyn Dyn{LlmBackend,Tool,Storage,Sandbox}>` proxies, four new `RuntimeError` variants, ten new tracing events); `tau-cli` (drop `test-mock` feature, new debug-tier subcommands). Two toy plugins under `crates/tau-plugins/`. Bundled into ADR-0008 per the ADR-0006 / ADR-0007 precedent.

**Tech Stack:** Rust stable (workspace MSRV 1.91 per QG7), `rmp-serde = "1"`, `bytes = "1"`, `tokio` (process + io-util features added), `tracing-subscriber = "0.3"`, `serde = "1"`, `thiserror = "2"`. SDK-side adds nothing exotic. Host-side: `tokio::process::Command`, `parking_lot::Mutex` (already a transitive dep, but pin if needed), `dashmap` may be considered later but v0.1 uses `tokio::sync::Mutex<HashMap<…>>`.

**Spec:** `docs/superpowers/specs/2026-04-28-plugin-loading-design.md` (commit `fe1a1be`).

**Working directory:** `/Users/titouanlebocq/code/tau` on branch `feat/plugin-loading-spec`. PR opens after Task 1 (or before, per the established workflow). All implementation commits on this branch auto-trigger CI per branch-protection. NEVER push to `main` directly.

**Commit policy:** every task ends with a Conventional Commits-formatted commit. PR is opened as Draft and marked Ready for review at Task 24 (final local verification). Tasks 25 (ADR-0008 sign-off — 24h wait per QG22 or self-review) and 26 (Plan sign-off + ROADMAP + branch protection update + merge) are user-driven gates.

**Note on TDD strictness:** for tasks producing parsers, validators, or branching logic (manifest/handshake parsers in Tasks 2 + 5, framer in Task 3, FakeStdioPeer in Task 6, runner dispatch in Task 9, lockfile-v2 migration in Task 12, IpcLlmBackend dispatch in Task 15, stream router in Task 16) follow strict red-green-refactor. For tasks producing pure data shapes or thin wiring (Tasks 1, 4, 7, 8) the cycle collapses — write the type + its tests in one step, then verify the suite.

**Plan-erratum carry-overs from sub-projects 1+2+3+4+5 (apply preemptively):**

- **Doctests on `#[non_exhaustive]` types must be marked `ignore`** (E0639 from external doctest compilation contexts). Most types in `tau-plugin-protocol` (`Frame`, `ProtocolError`, `HandshakeRequest`, `HandshakeResponse`, etc.) and `tau-plugin-sdk` (`SdkError`, `ConfigError`) are `#[non_exhaustive]` per spec; their doctests must be `ignore`-marked. Same applies to new `tau-domain` types (`PluginManifest`, `PortKind`, `PluginKind`) and the new `tau-pkg::InstallError::BuildFailed` variant context, and the new `tau-runtime::RuntimeError::Plugin*` variants.
- **`cargo test --all-targets` does NOT include doctests.** Verification steps explicitly run `cargo test --doc` separately for `tau-plugin-protocol`, `tau-plugin-sdk`, `tau-domain`, `tau-pkg`, `tau-runtime` whenever the task touches those crates' public surface.
- **For struct-pattern destructuring of `#[non_exhaustive]` enums in tests:** use `let X { fields, .. } = value else { panic!() };` (let-else).
- **NO new `Internal` error variants ship in this sub-project.** Per spec §2 #18: all new variants are typed (`InstallError::BuildFailed`, `InstallError::CargoNotFound`, four `RuntimeError::Plugin*`, `HandshakeFailureReason::*`, `ProtocolError::*`, `SdkError::*`, `ConfigError::*`). The mechanical CI test `crates/tau-domain/tests/escape_hatch_registry.rs` continues to gate against accidental additions.
- **`tau-cli` `test-mock` feature retirement** (Task 19): the `cfg(feature = "test-mock")` blocks in `tau-cli` are removed; the `test-mock` feature is stripped from `tau-cli/Cargo.toml`; `cmd::run` and `cmd::chat` rewire through `plugin_host`; the `no-default-features-cli` CI job continues to run unchanged (it never relied on `test-mock`).
- **`tau-ports` has no `serde` feature** (discovered in Task 1, RESOLVED in Task 9). The plan's Cargo.toml templates initially prescribed `tau-ports = { workspace = true, features = ["serde"] }` for `tau-plugin-protocol` and `tau-plugin-sdk`, but `tau-ports/Cargo.toml` exposed only `default` and `test-fixtures` features. Task 1 dropped the non-existent feature flag from both new crates' `tau-ports` deps. Task 9 added the `serde` feature to `tau-ports/Cargo.toml` (propagating to `tau-domain/serde` + `uuid/serde`), gated all `Serialize`/`Deserialize` derives behind it, and restored the `features = ["serde"]` on tau-plugin-protocol + tau-plugin-sdk's tau-ports deps. `Namespace`/`Key` use `serde(into = "String", try_from = "String")` to preserve their validating constructors.
- **Wire method `llm.complete_streaming` is named `llm.stream` in the implementation** (Task 9 adaptation). The actual `tau_ports::LlmBackend` trait method is `stream` (not `complete_streaming`), so the SDK runner dispatches `llm.stream` over the wire. The spec at `docs/superpowers/specs/2026-04-28-plugin-loading-design.md` §4.3 / §4.4 / §4.5 / §4.6 still references `llm.complete_streaming` — those are stale and should be read as `llm.stream`. **Tasks 14, 15, 16 (host-side dispatch, IpcLlmBackend, stream_router) MUST use `llm.stream`** to interoperate with the plugin-side runner. The streaming notification method `stream.chunk` is unchanged.
- **Wire method `llm.complete_streaming` was also renamed to `llm.stream` in spec/plan**: spec §4.3 / §4.4 / §4.5 / §4.6 and plan Task 9/16 still say `llm.complete_streaming`. Read those as `llm.stream`. ADR-0008 (Task 23) should record the final wire vocabulary.
- **`CompletionChunk::Done` variant is named `CompletionChunk::Finish`** in actual tau-ports (Task 9 adaptation). The spec's `CompletionChunk::Done { stop_reason, usage }` should be read as `CompletionChunk::Finish { stop_reason, usage }`.
- **`Tool` is stateful with `init`/`invoke`/`teardown` lifecycle** (Task 9 adaptation). The SDK's `run_tool` runs the full lifecycle per `tool.call`. Host-side `IpcTool` (Task 15) maps a single `tool.call` RPC into one full plugin-side init→invoke→teardown.

---

## File Structure

| Path | Responsibility | Created/Modified in |
|---|---|---|
| `Cargo.toml` (workspace root) | Add new crates to `members`; add `rmp-serde`, `bytes` to `[workspace.dependencies]`; expand tokio features | Task 1 |
| `crates/tau-plugin-protocol/Cargo.toml` | New crate manifest | Task 1 |
| `crates/tau-plugin-protocol/src/lib.rs` | Module declarations, re-exports, `forbid(unsafe_code)`, `deny(missing_docs)` | Tasks 1, 3, 4, 5, 6 |
| `crates/tau-plugin-protocol/src/framer.rs` | `FramedReader<R>`, `FramedWriter<W>`, length-prefix framing, `FramerOptions` | Task 3 |
| `crates/tau-plugin-protocol/src/frame.rs` | `Frame` enum, message-type constants | Task 4 |
| `crates/tau-plugin-protocol/src/error.rs` | `ProtocolError` + `RpcErrorEnvelope` + RPC error code constants | Tasks 3, 4 |
| `crates/tau-plugin-protocol/src/handshake.rs` | `HandshakeRequest`, `HandshakeResponse`, `Port` enum, `MetaShutdown` | Task 5 |
| `crates/tau-plugin-protocol/src/test_support.rs` | `FakeStdioPeer` (gated behind `test-support` feature) | Task 6 |
| `crates/tau-plugin-protocol/tests/proptest_framing.rs` | Framing round-trip property tests | Task 3 |
| `crates/tau-plugin-protocol/tests/handshake_roundtrip.rs` | Handshake serialization round-trip | Task 5 |
| `crates/tau-plugin-sdk/Cargo.toml` | New crate manifest | Task 1 |
| `crates/tau-plugin-sdk/src/lib.rs` | Re-exports per-port runners, `forbid(unsafe_code)`, `deny(missing_docs)` | Tasks 1, 7, 8, 9, 10 |
| `crates/tau-plugin-sdk/src/error.rs` | `SdkError` (framing, ser/de, IO failures) | Task 7 |
| `crates/tau-plugin-sdk/src/tracing_layer.rs` | tracing-subscriber JSON layer wired to stderr | Task 7 |
| `crates/tau-plugin-sdk/src/handshake.rs` | Per-port handshake response builder | Task 8 |
| `crates/tau-plugin-sdk/src/streaming.rs` | Helper: turn a `Stream` into `stream.chunk` notifications | Task 9 |
| `crates/tau-plugin-sdk/src/runners/mod.rs` | Module declarations | Task 9 |
| `crates/tau-plugin-sdk/src/runners/llm_backend.rs` | `pub async fn run_llm_backend<T>(plugin: T)` | Task 9 |
| `crates/tau-plugin-sdk/src/runners/tool.rs` | `pub async fn run_tool<T>(plugin: T)` | Task 9 |
| `crates/tau-plugin-sdk/src/runners/storage.rs` | `pub async fn run_storage<T>(plugin: T)` (stubbed body, no end-to-end test in v0.1) | Task 9 |
| `crates/tau-plugin-sdk/src/runners/sandbox.rs` | `pub async fn run_sandbox<T>(plugin: T)` (stubbed body) | Task 9 |
| `crates/tau-plugin-sdk/src/configure.rs` | `Configure` trait + `ConfigError` + `run_*_with_config` flavors | Task 10 |
| `crates/tau-plugin-sdk/tests/run_llm_backend_via_fake_peer.rs` | Drive runner via `FakeStdioPeer`, verify handshake + complete dispatch | Task 9 |
| `crates/tau-plugin-sdk/tests/configure_roundtrip.rs` | Drive `run_llm_backend_with_config` through fake peer with config payload | Task 10 |
| `crates/tau-domain/src/package/plugin.rs` | `PluginManifest`, `PortKind`, `PluginKind` types with custom serde | Task 2 |
| `crates/tau-domain/src/package/mod.rs` | Re-export the new types | Task 2 |
| `crates/tau-domain/src/error.rs` | `PortKindError`, `PluginKindError` parse errors | Task 2 |
| `crates/tau-domain/tests/proptest_plugin_manifest.rs` | Round-trip property tests for `PortKind` / `PluginKind` parsers | Task 2 |
| `crates/tau-pkg/Cargo.toml` | Add `tracing` dep (already workspace-listed) | Task 11 |
| `crates/tau-pkg/src/manifest.rs` | Parse `[plugin]` table from `tau.toml` | Task 11 |
| `crates/tau-pkg/src/install.rs` | `InstallOptions::build`, `BuildOptions`, build-step in `install_with_options` | Task 12 |
| `crates/tau-pkg/src/lockfile.rs` | `LockedPlugin`, lockfile schema bump v1 → v2 with auto-upgrade | Task 12 |
| `crates/tau-pkg/src/error.rs` | `InstallError::BuildFailed`, `InstallError::CargoNotFound` | Task 12 |
| `crates/tau-pkg/tests/install_builds_rust_cargo_plugin.rs` | End-to-end install + build test against a fixture repo | Task 12 |
| `crates/tau-runtime/Cargo.toml` | Add `tau-plugin-protocol` dep, expand tokio features (process, io-util) | Task 13 |
| `crates/tau-runtime/src/lib.rs` | Add `pub mod plugin_host` | Task 13 |
| `crates/tau-runtime/src/plugin_host/mod.rs` | Public API: `load_*` functions, `PluginHostOptions`, `TraceContext` | Tasks 13, 14, 15, 16, 17 |
| `crates/tau-runtime/src/plugin_host/process.rs` | `PluginProcess` struct + spawn + read-loop + stderr task + shutdown sequence | Task 14 |
| `crates/tau-runtime/src/plugin_host/handshake.rs` | Host-side handshake driver | Task 14 |
| `crates/tau-runtime/src/plugin_host/ipc_llm.rs` | `IpcLlmBackend` impl `DynLlmBackend` (non-streaming Task 15; streaming Task 16) | Tasks 15, 16 |
| `crates/tau-runtime/src/plugin_host/ipc_tool.rs` | `IpcTool` impl `DynTool` | Task 15 |
| `crates/tau-runtime/src/plugin_host/ipc_storage.rs` | `IpcStorage` impl `DynStorage` (mock-peer unit-tested only) | Task 15 |
| `crates/tau-runtime/src/plugin_host/ipc_sandbox.rs` | `IpcSandbox` impl `DynSandbox` (mock-peer unit-tested only) | Task 15 |
| `crates/tau-runtime/src/plugin_host/stream_router.rs` | `stream.chunk` notification → `Stream<CompletionChunk>` assembly | Task 16 |
| `crates/tau-runtime/src/plugin_host/recording.rs` | `RecordingSink::JsonlFile` + tap wiring | Task 17 |
| `crates/tau-runtime/src/error.rs` | Add four new `RuntimeError::Plugin*` variants + `HandshakeFailureReason` | Task 13 |
| `crates/tau-runtime/tests/plugin_host_handshake.rs` | Integration: handshake failure modes via fake peer | Task 14 |
| `crates/tau-runtime/tests/plugin_host_ipc_llm.rs` | Integration: IpcLlmBackend dispatch via fake peer | Tasks 15, 16 |
| `crates/tau-runtime/tests/plugin_host_capability_filter.rs` | Integration: capability filter applies before requests cross the wire | Task 15 |
| `crates/tau-runtime/tests/plugin_host_recording.rs` | Integration: JsonlFile sink captures + replays frames | Task 17 |
| `crates/tau-plugins/echo-llm/Cargo.toml` | Toy LlmBackend plugin manifest | Task 18 |
| `crates/tau-plugins/echo-llm/src/main.rs` | Toy LlmBackend implementation with test-only modes | Task 18 |
| `crates/tau-plugins/echo-llm/tau.toml` | Plugin package manifest with `[plugin]` table | Task 18 |
| `crates/tau-plugins/echo-tool/Cargo.toml` | Toy Tool plugin manifest | Task 18 |
| `crates/tau-plugins/echo-tool/src/main.rs` | Toy Tool implementation | Task 18 |
| `crates/tau-plugins/echo-tool/tau.toml` | Plugin package manifest | Task 18 |
| `crates/tau-cli/Cargo.toml` | Drop `test-mock` feature; remove the `tau-ports/test-fixtures` dep that backs it | Task 19 |
| `crates/tau-cli/src/cmd/run.rs` | Rewire to load LLM backend + tools via `plugin_host` | Task 19 |
| `crates/tau-cli/src/cmd/chat.rs` | Rewire same way | Task 19 |
| `crates/tau-cli/src/cmd/mock_backend.rs` | DELETE (was the test-mock implementation) | Task 19 |
| `crates/tau-cli/src/cli.rs` | Add `--record-protocol <path>` global flag; add `Plugin { Describe, Run, Protocol { Decode } }` subcommand group | Task 20 |
| `crates/tau-cli/src/cmd/plugin/mod.rs` | New `cmd::plugin` module | Task 20 |
| `crates/tau-cli/src/cmd/plugin/describe.rs` | `tau plugin describe <name>` handler | Task 20 |
| `crates/tau-cli/src/cmd/plugin/run.rs` | `tau plugin run <path> [--interactive\|--script]` handler | Task 20 |
| `crates/tau-cli/src/cmd/plugin/protocol_decode.rs` | `tau plugin protocol decode <path>` handler | Task 20 |
| `crates/tau-cli/tests/common/echo_plugins.rs` | Test helper: build echo-llm + echo-tool once, return paths | Task 21 |
| `crates/tau-cli/tests/cmd_run_plugin.rs` | Real-spawn integration tests for `tau run` | Task 21 |
| `crates/tau-cli/tests/cmd_chat_plugin.rs` | Real-spawn integration tests for `tau chat` | Task 21 |
| `crates/tau-cli/tests/cmd_plugin_describe.rs` | Real-spawn integration test for `tau plugin describe` | Task 21 |
| `crates/tau-cli/tests/cmd_plugin_run_protocol.rs` | Real-spawn integration test for `tau plugin run --interactive` and `protocol decode` | Task 21 |
| `.github/workflows/ci.yml` | Add 3 new build jobs (`tau-plugin-protocol`, `tau-plugin-sdk`, `tau-plugins`) | Task 22 |
| `docs/decisions/0008-plugin-loading.md` | ADR-0008 (bundled plugin loading + tau-pkg + tau-runtime + tau-domain amendments) | Task 23 |
| `docs/decisions/README.md` | Add ADR-0008 row to index | Task 23 |
| `ROADMAP.md` | Mark Phase 1 priority 1 as completed; flag ROADMAP regen before priority 2 starts | Task 26 |
| `docs/superpowers/plans/2026-04-28-plugin-loading.md` | This plan; checkboxes ticked at sign-off | Task 26 |

---

## Tasks 1-3: detailed (Plan-2 fidelity)

### Task 1: Workspace scaffold + new crates

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/tau-plugin-protocol/Cargo.toml`
- Create: `crates/tau-plugin-protocol/src/lib.rs`
- Create: `crates/tau-plugin-sdk/Cargo.toml`
- Create: `crates/tau-plugin-sdk/src/lib.rs`

- [ ] **Step 1.1: Update workspace `Cargo.toml`**

Open `/Users/titouanlebocq/code/tau/Cargo.toml`. Replace the `[workspace]` and `[workspace.dependencies]` blocks with:

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
]

[workspace.package]
version = "0.0.0"
edition = "2021"
rust-version = "1.91"
license = "MIT OR Apache-2.0"
repository = "https://github.com/LEBOCQTitouan/tau"
authors = ["Titouan Lebocq <75916953+LEBOCQTitouan@users.noreply.github.com>"]

[workspace.dependencies]
tau-domain          = { path = "crates/tau-domain", version = "0.0.0" }
tau-ports           = { path = "crates/tau-ports",  version = "0.0.0" }
tau-pkg             = { path = "crates/tau-pkg",    version = "0.0.0" }
tau-runtime         = { path = "crates/tau-runtime", version = "0.0.0" }
tau-plugin-protocol = { path = "crates/tau-plugin-protocol", version = "0.0.0" }
tau-plugin-sdk      = { path = "crates/tau-plugin-sdk", version = "0.0.0" }
thiserror       = "2"
semver          = { version = "1" }
uuid            = { version = "1", features = ["v7"] }
url             = "2"
serde           = { version = "1", features = ["derive"] }
base64          = "0.22"
proptest        = "1"
walkdir         = "2"
futures-core    = "0.3"
toml            = "0.8"
fs4             = "0.8"
humantime-serde = "1"
tempfile        = "3"
tracing         = "0.1"
rmp-serde       = "1"
bytes           = "1"
tokio           = { version = "1", features = ["macros", "rt", "rt-multi-thread", "io-util", "process", "sync", "time"] }
```

> **NB:** Even though the toy plugin crates don't exist yet (they're created in Task 18), they must be listed in `members` here so a single `cargo check --workspace` later in this task fails fast if their paths are wrong. We'll create empty stub directories for them in Step 1.6 to make the workspace pointer valid.

- [ ] **Step 1.2: Create `tau-plugin-protocol` crate**

Create `/Users/titouanlebocq/code/tau/crates/tau-plugin-protocol/Cargo.toml`:

```toml
[package]
name = "tau-plugin-protocol"
description = "Wire-format types and framing primitives for the tau plugin protocol (MessagePack-RPC over stdio)."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[dependencies]
tau-domain = { workspace = true, features = ["serde"] }
tau-ports  = { workspace = true, features = ["serde"] }
serde      = { workspace = true }
rmp-serde  = { workspace = true }
bytes      = { workspace = true }
thiserror  = { workspace = true }
tokio      = { workspace = true }

[features]
default       = []
test-support  = ["dep:tokio-test"]

[dependencies.tokio-test]
version  = "0.4"
optional = true

[dev-dependencies]
proptest    = { workspace = true }
serde_json  = "1"
tokio       = { workspace = true, features = ["macros", "rt-multi-thread"] }
```

Create `/Users/titouanlebocq/code/tau/crates/tau-plugin-protocol/src/lib.rs`:

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Wire-format types and framing primitives for the tau plugin protocol.
//!
//! Plugins talk to the tau runtime over MessagePack-RPC on stdio with
//! length-prefixed framing. This crate is shared by the host (in
//! `tau-runtime::plugin_host`) and the SDK (in `tau-plugin-sdk`); it
//! contains pure types and IO helpers, no tracing, no process management.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-loading-design.md` §4
//! and ADR-0008 for the design rationale.

// Modules and re-exports populate as Tasks 3 — 6 land. For Task 1 we
// only assert the crate compiles.
```

- [ ] **Step 1.3: Create `tau-plugin-sdk` crate**

Create `/Users/titouanlebocq/code/tau/crates/tau-plugin-sdk/Cargo.toml`:

```toml
[package]
name = "tau-plugin-sdk"
description = "Plugin author SDK for tau: per-port generic runners over the tau plugin protocol."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[dependencies]
tau-domain          = { workspace = true, features = ["serde"] }
tau-ports           = { workspace = true, features = ["serde"] }
tau-plugin-protocol = { workspace = true }
serde              = { workspace = true }
serde_json         = "1"
rmp-serde          = { workspace = true }
thiserror          = { workspace = true }
tokio              = { workspace = true }
tracing            = { workspace = true }
tracing-subscriber = { version = "0.3", features = ["fmt", "env-filter", "json"] }

[features]
default = []

[dev-dependencies]
tau-plugin-protocol = { workspace = true, features = ["test-support"] }
proptest            = { workspace = true }
tokio               = { workspace = true, features = ["macros", "rt-multi-thread"] }
```

Create `/Users/titouanlebocq/code/tau/crates/tau-plugin-sdk/src/lib.rs`:

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Plugin author SDK for tau.
//!
//! Plugin authors implement the same `tau_ports::*` traits the in-process
//! tau runtime kernel uses, then call one of the per-port generic runner
//! functions (`run_llm_backend`, `run_tool`, `run_storage`, `run_sandbox`)
//! from their `#[tokio::main]` entry point. This crate contains the
//! tracing layer, handshake response builder, dispatch loop, and
//! streaming helper.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-loading-design.md` §5
//! and ADR-0008 for the design rationale.

// Modules and re-exports populate as Tasks 7 — 10 land.
```

- [ ] **Step 1.4: Create stub directories for toy plugin crates**

Tasks 18 will fully populate `crates/tau-plugins/echo-llm` and `crates/tau-plugins/echo-tool`. For Task 1 we only need stubs so the workspace pointer is valid.

```bash
mkdir -p /Users/titouanlebocq/code/tau/crates/tau-plugins/echo-llm/src
mkdir -p /Users/titouanlebocq/code/tau/crates/tau-plugins/echo-tool/src
```

Create `/Users/titouanlebocq/code/tau/crates/tau-plugins/echo-llm/Cargo.toml`:

```toml
[package]
name = "echo-llm"
description = "Toy LlmBackend plugin for tau integration tests. Stub — populated in Task 18."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[[bin]]
name = "echo-llm"
path = "src/main.rs"

[dependencies]
```

Create `/Users/titouanlebocq/code/tau/crates/tau-plugins/echo-llm/src/main.rs`:

```rust
//! Stub binary populated in Task 18.

fn main() {}
```

Create `/Users/titouanlebocq/code/tau/crates/tau-plugins/echo-tool/Cargo.toml`:

```toml
[package]
name = "echo-tool"
description = "Toy Tool plugin for tau integration tests. Stub — populated in Task 18."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[[bin]]
name = "echo-tool"
path = "src/main.rs"

[dependencies]
```

Create `/Users/titouanlebocq/code/tau/crates/tau-plugins/echo-tool/src/main.rs`:

```rust
//! Stub binary populated in Task 18.

fn main() {}
```

- [ ] **Step 1.5: Verify the whole workspace builds and lints cleanly**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: all three exit 0. Each new crate compiles to an empty library / stub binary.

- [ ] **Step 1.6: Verify per-crate doctest compile passes (no doctests yet but the harness must be wired)**

```bash
cargo test -p tau-plugin-protocol --doc
cargo test -p tau-plugin-sdk --doc
```

Expected: both report `test result: ok. 0 passed; 0 failed; 0 ignored`.

- [ ] **Step 1.7: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add Cargo.toml \
        crates/tau-plugin-protocol \
        crates/tau-plugin-sdk \
        crates/tau-plugins
git commit -m "$(cat <<'EOF'
build(plugin-loading): scaffold tau-plugin-protocol + tau-plugin-sdk + toy plugin stubs

Adds two new workspace crates (tau-plugin-protocol for pure wire types,
tau-plugin-sdk for plugin-author runners) plus stub directories for
crates/tau-plugins/echo-{llm,tool} that Task 18 will populate. Adds
rmp-serde, bytes, expanded tokio features to workspace deps.

Refs: spec §3.1, Task 1 of plan
EOF
)"
```

Push:

```bash
git push -u origin feat/plugin-loading-spec
```

---

### Task 2: tau-domain — `PluginManifest`, `PortKind`, `PluginKind`

**Files:**
- Create: `crates/tau-domain/src/package/plugin.rs`
- Modify: `crates/tau-domain/src/package/mod.rs`
- Modify: `crates/tau-domain/src/error.rs`
- Modify: `crates/tau-domain/src/lib.rs`
- Create: `crates/tau-domain/tests/proptest_plugin_manifest.rs`

This task introduces three new types in `tau-domain`. `PortKind` and `PluginKind` use the **ADR-0005 pattern**: custom serde via `Display` / `FromStr`, serializing as canonical strings rather than adjacent-tagged objects. `PluginManifest` is a struct with `Serialize`/`Deserialize` derived. All three are `#[non_exhaustive]`. Strict TDD because there's parsing logic.

- [ ] **Step 2.1: Add error variants to `tau-domain/src/error.rs`**

Open `/Users/titouanlebocq/code/tau/crates/tau-domain/src/error.rs`. Append:

```rust
/// Validation errors for [`crate::package::PortKind::from_str`].
///
/// # Example
///
/// ```
/// use tau_domain::PortKindError;
/// use tau_domain::PortKind;
/// use std::str::FromStr;
///
/// let err = PortKind::from_str("nonsense").unwrap_err();
/// assert!(matches!(err, PortKindError::Unknown { .. }));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PortKindError {
    /// The input did not match any known port kind.
    #[error("unknown port kind {input:?}; expected one of: llm_backend, tool, storage, sandbox")]
    Unknown {
        /// The input that did not parse.
        input: String,
    },
}

/// Validation errors for [`crate::package::PluginKind::from_str`].
///
/// # Example
///
/// ```
/// use tau_domain::PluginKindError;
/// use tau_domain::PluginKind;
/// use std::str::FromStr;
///
/// let err = PluginKind::from_str("nonsense").unwrap_err();
/// assert!(matches!(err, PluginKindError::Unknown { .. }));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PluginKindError {
    /// The input did not match any known plugin kind.
    ///
    /// v0.1 only supports `rust-cargo`. Future kinds (`python-pip`,
    /// `node-npm`, `prebuilt`) are tracked in spec §2.1.
    #[error("unknown plugin kind {input:?}; expected: rust-cargo")]
    Unknown {
        /// The input that did not parse.
        input: String,
    },
}
```

> **Same-commit escape-hatch:** these are typed errors with no `Internal` / `Custom` variant. `crates/tau-domain/tests/escape_hatch_registry.rs` continues to gate.

- [ ] **Step 2.2: Create `tau-domain/src/package/plugin.rs`**

Create `/Users/titouanlebocq/code/tau/crates/tau-domain/src/package/plugin.rs`:

```rust
//! Plugin manifest types declared in a package's `tau.toml` `[plugin]`
//! table.
//!
//! Mirrors the ADR-0005 pattern from [`PackageSource`]: enums serialize
//! as canonical strings via `Display`/`FromStr` (not as adjacent-tagged
//! objects), so a TOML `provides = "llm_backend"` round-trips cleanly.

use std::fmt;
use std::str::FromStr;

use crate::error::{PluginKindError, PortKindError};

/// Which port a plugin provides.
///
/// Serialized form (when the `serde` feature is on) is the canonical
/// string `llm_backend` / `tool` / `storage` / `sandbox`.
///
/// # Example
///
/// ```ignore
/// // `PortKind` is `#[non_exhaustive]`; doctest cannot construct via
/// // struct-literal across crate boundaries (E0639).
/// use tau_domain::PortKind;
/// use std::str::FromStr;
///
/// let kind = PortKind::from_str("llm_backend").unwrap();
/// assert_eq!(kind.to_string(), "llm_backend");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortKind {
    /// LlmBackend port: provides `llm.complete` and friends.
    LlmBackend,
    /// Tool port: provides `tool.call`.
    Tool,
    /// Storage port: provides `storage.get`/`put`/`list`/`delete`.
    Storage,
    /// Sandbox port: provides `sandbox.run`.
    Sandbox,
}

impl fmt::Display for PortKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PortKind::LlmBackend => "llm_backend",
            PortKind::Tool => "tool",
            PortKind::Storage => "storage",
            PortKind::Sandbox => "sandbox",
        })
    }
}

impl FromStr for PortKind {
    type Err = PortKindError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "llm_backend" => Ok(PortKind::LlmBackend),
            "tool" => Ok(PortKind::Tool),
            "storage" => Ok(PortKind::Storage),
            "sandbox" => Ok(PortKind::Sandbox),
            other => Err(PortKindError::Unknown {
                input: other.to_owned(),
            }),
        }
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for PortKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for PortKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(d)?;
        PortKind::from_str(s).map_err(serde::de::Error::custom)
    }
}

/// What kind of plugin distribution this package is.
///
/// v0.1: only `RustCargo` (a Rust crate built with `cargo build
/// --release --bin <bin>` at install time). Future variants
/// (`PythonPip`, `NodeNpm`, `Prebuilt`) are additive — see spec §2.1.
///
/// Serialized form: the kebab-case string `rust-cargo`.
///
/// # Example
///
/// ```ignore
/// use tau_domain::PluginKind;
/// use std::str::FromStr;
///
/// let kind = PluginKind::from_str("rust-cargo").unwrap();
/// assert_eq!(kind.to_string(), "rust-cargo");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginKind {
    /// A Rust crate built via `cargo build --release --bin <bin>`.
    RustCargo,
}

impl fmt::Display for PluginKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            PluginKind::RustCargo => "rust-cargo",
        })
    }
}

impl FromStr for PluginKind {
    type Err = PluginKindError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "rust-cargo" => Ok(PluginKind::RustCargo),
            other => Err(PluginKindError::Unknown {
                input: other.to_owned(),
            }),
        }
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for PluginKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for PluginKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(d)?;
        PluginKind::from_str(s).map_err(serde::de::Error::custom)
    }
}

/// Plugin manifest declared in a package's `tau.toml` `[plugin]` table.
///
/// Read-only at runtime; `tau-pkg` parses it during install and `tau-runtime`
/// consumes it via `LockedPlugin` (see `tau-pkg::lockfile`).
///
/// # Example
///
/// ```ignore
/// // `PluginManifest` is `#[non_exhaustive]`; constructed by tau-pkg
/// // during install. External callers (notably tau-runtime integration
/// // tests that synthesize a lockfile) build it via `serde::from_str`.
/// use tau_domain::PluginManifest;
/// let toml = r#"
///     provides = "llm_backend"
///     kind     = "rust-cargo"
///     bin      = "anthropic-plugin"
/// "#;
/// let m: PluginManifest = toml::from_str(toml).unwrap();
/// assert_eq!(m.bin, "anthropic-plugin");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PluginManifest {
    /// Which port this plugin provides.
    pub provides: PortKind,
    /// Distribution kind (build orchestration).
    pub kind: PluginKind,
    /// Cargo `[[bin]]` target name (when `kind == RustCargo`).
    pub bin: String,
}

impl PluginManifest {
    /// Construct a `PluginManifest`. `#[non_exhaustive]`; external
    /// callers (e.g. tau-runtime tests) use this constructor.
    pub fn new(provides: PortKind, kind: PluginKind, bin: String) -> Self {
        Self {
            provides,
            kind,
            bin,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_kind_round_trip_via_display_from_str() {
        for kind in [
            PortKind::LlmBackend,
            PortKind::Tool,
            PortKind::Storage,
            PortKind::Sandbox,
        ] {
            let s = kind.to_string();
            let parsed = PortKind::from_str(&s).unwrap();
            assert_eq!(kind, parsed);
        }
    }

    #[test]
    fn port_kind_unknown_input_errors() {
        let err = PortKind::from_str("nope").unwrap_err();
        let crate::error::PortKindError::Unknown { input } = err else {
            panic!("expected Unknown")
        };
        assert_eq!(input, "nope");
    }

    #[test]
    fn plugin_kind_round_trip() {
        let s = PluginKind::RustCargo.to_string();
        assert_eq!(s, "rust-cargo");
        assert_eq!(PluginKind::from_str(&s).unwrap(), PluginKind::RustCargo);
    }

    #[test]
    fn plugin_kind_unknown_input_errors() {
        let err = PluginKind::from_str("python-pip").unwrap_err();
        let crate::error::PluginKindError::Unknown { input } = err else {
            panic!("expected Unknown")
        };
        assert_eq!(input, "python-pip");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn plugin_manifest_round_trips_through_toml() {
        let m = PluginManifest::new(
            PortKind::LlmBackend,
            PluginKind::RustCargo,
            "anthropic-plugin".to_string(),
        );
        let s = toml::to_string(&m).unwrap();
        let back: PluginManifest = toml::from_str(&s).unwrap();
        assert_eq!(m, back);
    }
}
```

- [ ] **Step 2.3: Wire it up in `package/mod.rs`**

Open `/Users/titouanlebocq/code/tau/crates/tau-domain/src/package/mod.rs`. Append:

```rust
mod plugin;

pub use plugin::{PluginKind, PluginManifest, PortKind};
```

(If the file uses `pub mod ...; pub use ...;` style, follow the existing convention.)

- [ ] **Step 2.4: Re-export from `lib.rs`**

Open `/Users/titouanlebocq/code/tau/crates/tau-domain/src/lib.rs`. Find the `pub use error::{...};` block — replace with:

```rust
pub use error::{
    AgentIdError, PackageKindError, PackageManifestError, PackageNameError, PackageSourceError,
    PluginKindError, PortKindError,
};
```

Find `pub use package::{...};` and add `PluginKind, PluginManifest, PortKind` to the alphabetized list.

- [ ] **Step 2.5: Run unit tests**

```bash
cd /Users/titouanlebocq/code/tau
cargo test -p tau-domain --all-features --lib plugin
```

Expected: 5 tests pass (`port_kind_round_trip_via_display_from_str`, `port_kind_unknown_input_errors`, `plugin_kind_round_trip`, `plugin_kind_unknown_input_errors`, `plugin_manifest_round_trips_through_toml`).

- [ ] **Step 2.6: Add proptest round-trip in `tests/proptest_plugin_manifest.rs`**

Create `/Users/titouanlebocq/code/tau/crates/tau-domain/tests/proptest_plugin_manifest.rs`:

```rust
//! Proptest: PortKind / PluginKind / PluginManifest TOML round-trip.

#![cfg(feature = "serde")]

use proptest::prelude::*;
use tau_domain::{PluginKind, PluginManifest, PortKind};

prop_compose! {
    fn arb_port_kind()(idx in 0u8..4u8) -> PortKind {
        match idx {
            0 => PortKind::LlmBackend,
            1 => PortKind::Tool,
            2 => PortKind::Storage,
            _ => PortKind::Sandbox,
        }
    }
}

prop_compose! {
    fn arb_plugin_kind()(_pad in 0u8..1u8) -> PluginKind {
        PluginKind::RustCargo
    }
}

prop_compose! {
    fn arb_bin()(s in "[a-z][a-z0-9_-]{0,30}") -> String {
        s
    }
}

prop_compose! {
    fn arb_plugin_manifest()(
        provides in arb_port_kind(),
        kind in arb_plugin_kind(),
        bin in arb_bin(),
    ) -> PluginManifest {
        PluginManifest::new(provides, kind, bin)
    }
}

proptest! {
    #[test]
    fn plugin_manifest_toml_round_trip(m in arb_plugin_manifest()) {
        let s = toml::to_string(&m).unwrap();
        let back: PluginManifest = toml::from_str(&s).unwrap();
        prop_assert_eq!(m, back);
    }
}
```

- [ ] **Step 2.7: Run all tau-domain tests, including doctests**

```bash
cd /Users/titouanlebocq/code/tau
cargo test -p tau-domain --all-features --all-targets
cargo test -p tau-domain --all-features --doc
```

Expected: all green.

- [ ] **Step 2.8: Check escape-hatch registry test still passes**

```bash
cd /Users/titouanlebocq/code/tau
cargo test -p tau-domain --all-features --test escape_hatch_registry
```

Expected: green. The new variants are typed (no `Internal`/`Custom`).

- [ ] **Step 2.9: Lint and format**

```bash
cd /Users/titouanlebocq/code/tau
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: both exit 0.

- [ ] **Step 2.10: Commit + push**

```bash
cd /Users/titouanlebocq/code/tau
git add crates/tau-domain
git commit -m "$(cat <<'EOF'
feat(tau-domain): add PluginManifest, PortKind, PluginKind types

Adds the data shape for the [plugin] table in package tau.toml. Custom
serde via Display/FromStr per ADR-0005: PortKind serializes as
"llm_backend"|"tool"|"storage"|"sandbox", PluginKind as "rust-cargo".
All three types are #[non_exhaustive]; doctests are ignore-marked per
sub-project 4 erratum. Two new typed parse-error variants
(PortKindError::Unknown, PluginKindError::Unknown); no new Internal
variants — escape-hatch registry test continues to gate.

Refs: spec §6.2, ADR-0008 §14, Task 2 of plan
EOF
)"
git push
```

---

### Task 3: tau-plugin-protocol — framing primitives

**Files:**
- Create: `crates/tau-plugin-protocol/src/error.rs`
- Create: `crates/tau-plugin-protocol/src/framer.rs`
- Modify: `crates/tau-plugin-protocol/src/lib.rs`
- Create: `crates/tau-plugin-protocol/tests/proptest_framing.rs`

The framer is the bottom of the protocol stack: a length-prefixed reader/writer over `tokio::io::AsyncRead`/`AsyncWrite`. 4-byte big-endian `u32` length prefix; max body size configurable via `FramerOptions` (default 64 MiB per spec §4.1). Strict TDD because framing has tricky edge cases (truncated headers, max-size enforcement, partial reads).

- [ ] **Step 3.1: Define `ProtocolError` in `error.rs`**

Create `/Users/titouanlebocq/code/tau/crates/tau-plugin-protocol/src/error.rs`:

```rust
//! Errors emitted by the framing and codec layers.

use thiserror::Error;

/// Failures from the framing and codec layers.
///
/// `#[non_exhaustive]`: additive variants do not break callers.
///
/// # Example
///
/// ```ignore
/// use tau_plugin_protocol::ProtocolError;
/// let err = ProtocolError::FrameTooLarge { len: 1, max: 0 };
/// assert!(format!("{err}").contains("frame too large"));
/// ```
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// Underlying IO error from the transport.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The receiving side observed end-of-stream while expecting more
    /// bytes. For host-side framers, this typically means the plugin
    /// process exited.
    #[error("frame truncated: expected {expected} more bytes, got EOF")]
    FrameTruncated {
        /// How many more bytes were expected.
        expected: usize,
    },

    /// A frame's length-prefix exceeded the configured max.
    #[error("frame too large: {len} bytes (max {max})")]
    FrameTooLarge {
        /// Reported length from the prefix.
        len: usize,
        /// Configured maximum.
        max: usize,
    },

    /// The frame body failed to decode as MessagePack.
    #[error("body decode failed: {0}")]
    BodyDecodeFailed(#[from] rmp_serde::decode::Error),

    /// Body encoding failed.
    #[error("body encode failed: {0}")]
    BodyEncodeFailed(#[from] rmp_serde::encode::Error),
}
```

- [ ] **Step 3.2: Define `FramerOptions` and the framer types in `framer.rs`**

Create `/Users/titouanlebocq/code/tau/crates/tau-plugin-protocol/src/framer.rs`:

```rust
//! Length-prefixed MessagePack frame reader and writer.
//!
//! Each frame on the wire is:
//!
//! ```text
//! +--------+--------+--------+--------+================+
//! |  big-endian u32 length (excl PFX) | MessagePack    |
//! +--------+--------+--------+--------+ message body  |
//!                                     +================+
//! ```

use bytes::BytesMut;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::ProtocolError;

const PREFIX_LEN: usize = 4;

/// Tunables for the framer. Default `max_message_size = 64 MiB`.
///
/// # Example
///
/// ```ignore
/// use tau_plugin_protocol::FramerOptions;
/// let opts = FramerOptions::default();
/// assert_eq!(opts.max_message_size, 64 * 1024 * 1024);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct FramerOptions {
    /// Reject frames whose length-prefix exceeds this many bytes.
    pub max_message_size: usize,
}

impl Default for FramerOptions {
    fn default() -> Self {
        Self {
            max_message_size: 64 * 1024 * 1024,
        }
    }
}

/// Async reader for length-prefixed MessagePack frames.
pub struct FramedReader<R> {
    inner: R,
    options: FramerOptions,
    buf: BytesMut,
}

impl<R> FramedReader<R>
where
    R: AsyncRead + Unpin,
{
    /// Construct a new reader.
    pub fn new(inner: R, options: FramerOptions) -> Self {
        Self {
            inner,
            options,
            buf: BytesMut::with_capacity(8192),
        }
    }

    /// Read the next frame body from the underlying transport. Returns
    /// `Ok(None)` on clean EOF (zero bytes when no frame is in
    /// progress).
    pub async fn next_frame(&mut self) -> Result<Option<Vec<u8>>, ProtocolError> {
        let mut prefix = [0u8; PREFIX_LEN];
        match self.inner.read_exact(&mut prefix).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(ProtocolError::Io(e)),
        }
        let len = u32::from_be_bytes(prefix) as usize;
        if len > self.options.max_message_size {
            return Err(ProtocolError::FrameTooLarge {
                len,
                max: self.options.max_message_size,
            });
        }
        self.buf.clear();
        self.buf.resize(len, 0);
        if let Err(e) = self.inner.read_exact(&mut self.buf[..]).await {
            return Err(if e.kind() == std::io::ErrorKind::UnexpectedEof {
                ProtocolError::FrameTruncated { expected: len }
            } else {
                ProtocolError::Io(e)
            });
        }
        Ok(Some(self.buf[..len].to_vec()))
    }
}

/// Async writer for length-prefixed MessagePack frames.
pub struct FramedWriter<W> {
    inner: W,
}

impl<W> FramedWriter<W>
where
    W: AsyncWrite + Unpin,
{
    /// Construct a new writer.
    pub fn new(inner: W) -> Self {
        Self { inner }
    }

    /// Write a frame body. The length prefix is computed automatically.
    pub async fn write_frame(&mut self, body: &[u8]) -> Result<(), ProtocolError> {
        let len = body.len() as u32;
        self.inner.write_all(&len.to_be_bytes()).await?;
        self.inner.write_all(body).await?;
        self.inner.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn round_trip_small_frame() {
        let (a, b) = duplex(8192);
        let mut writer = FramedWriter::new(a);
        let mut reader = FramedReader::new(b, FramerOptions::default());
        writer.write_frame(b"hello").await.unwrap();
        let frame = reader.next_frame().await.unwrap().unwrap();
        assert_eq!(frame, b"hello");
    }

    #[tokio::test]
    async fn read_returns_none_on_clean_eof() {
        let (a, b) = duplex(8);
        drop(a); // close write side
        let mut reader = FramedReader::new(b, FramerOptions::default());
        let result = reader.next_frame().await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn frame_too_large_rejected() {
        let (a, b) = duplex(64);
        let opts = FramerOptions {
            max_message_size: 4,
        };
        let mut writer = FramedWriter::new(a);
        let mut reader = FramedReader::new(b, opts);
        writer.write_frame(b"hello").await.unwrap();
        let err = reader.next_frame().await.unwrap_err();
        let ProtocolError::FrameTooLarge { len, max } = err else {
            panic!("expected FrameTooLarge")
        };
        assert_eq!(len, 5);
        assert_eq!(max, 4);
    }

    #[tokio::test]
    async fn truncated_body_returns_frame_truncated() {
        let (mut a, b) = duplex(64);
        // Write the prefix claiming 100 bytes, then close before
        // sending body.
        let prefix = (100u32).to_be_bytes();
        a.write_all(&prefix).await.unwrap();
        drop(a);
        let mut reader = FramedReader::new(b, FramerOptions::default());
        let err = reader.next_frame().await.unwrap_err();
        let ProtocolError::FrameTruncated { expected } = err else {
            panic!("expected FrameTruncated")
        };
        assert_eq!(expected, 100);
    }
}
```

- [ ] **Step 3.3: Update `lib.rs` to expose framer + error**

Replace `/Users/titouanlebocq/code/tau/crates/tau-plugin-protocol/src/lib.rs`:

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Wire-format types and framing primitives for the tau plugin protocol.
//!
//! Plugins talk to the tau runtime over MessagePack-RPC on stdio with
//! length-prefixed framing. This crate is shared by the host (in
//! `tau-runtime::plugin_host`) and the SDK (in `tau-plugin-sdk`); it
//! contains pure types and IO helpers, no tracing, no process management.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-loading-design.md` §4
//! and ADR-0008 for the design rationale.

pub mod error;
pub mod framer;

pub use error::ProtocolError;
pub use framer::{FramedReader, FramedWriter, FramerOptions};
```

- [ ] **Step 3.4: Run unit tests in the framer module**

```bash
cd /Users/titouanlebocq/code/tau
cargo test -p tau-plugin-protocol --lib framer
```

Expected: 4 tests pass.

- [ ] **Step 3.5: Add proptest round-trip**

Create `/Users/titouanlebocq/code/tau/crates/tau-plugin-protocol/tests/proptest_framing.rs`:

```rust
//! Proptest: arbitrary frame bytes round-trip through writer → reader.

use proptest::prelude::*;
use tau_plugin_protocol::{FramedReader, FramedWriter, FramerOptions};
use tokio::io::duplex;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn round_trip_arbitrary_bytes(payload in proptest::collection::vec(any::<u8>(), 0..16384)) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let (a, b) = duplex(32 * 1024);
            let mut writer = FramedWriter::new(a);
            let mut reader = FramedReader::new(b, FramerOptions::default());
            writer.write_frame(&payload).await.unwrap();
            let got = reader.next_frame().await.unwrap().unwrap();
            prop_assert_eq!(got, payload);
            Ok(())
        }).unwrap();
    }
}
```

- [ ] **Step 3.6: Verify proptest passes**

```bash
cd /Users/titouanlebocq/code/tau
cargo test -p tau-plugin-protocol --test proptest_framing
```

Expected: green; 64 cases each.

- [ ] **Step 3.7: Verify doctests compile**

```bash
cd /Users/titouanlebocq/code/tau
cargo test -p tau-plugin-protocol --doc
```

Expected: green.

- [ ] **Step 3.8: Lint and format**

```bash
cd /Users/titouanlebocq/code/tau
cargo clippy -p tau-plugin-protocol --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

- [ ] **Step 3.9: Commit + push**

```bash
cd /Users/titouanlebocq/code/tau
git add crates/tau-plugin-protocol
git commit -m "$(cat <<'EOF'
feat(tau-plugin-protocol): length-prefixed MessagePack framer

Adds FramedReader and FramedWriter over tokio::io::AsyncRead/AsyncWrite
with a 4-byte big-endian u32 length prefix. Default max body 64 MiB,
configurable via FramerOptions. Five typed error variants on
ProtocolError (Io, FrameTruncated, FrameTooLarge, BodyDecodeFailed,
BodyEncodeFailed) — no Internal escape hatch. Tested with unit tests
covering round-trip / clean EOF / size enforcement / truncation, plus
a proptest round-trip over arbitrary byte payloads.

Refs: spec §4.1, Task 3 of plan
EOF
)"
git push
```

---

## Tasks 4-23: hybrid (per-task summary + spec references)

The remaining tasks follow the patterns established in Tasks 1-3 (Cargo.toml deltas where required, types per spec, full unit + integration tests, doctest discipline, conventional-commits per task, `cargo build` + `cargo test --all-targets` + `cargo test --doc` + `cargo clippy -- -D warnings` + `cargo fmt --all -- --check` before each commit, push after).

Spec references are hyperlinks to the design spec at
`docs/superpowers/specs/2026-04-28-plugin-loading-design.md`.

---

### Task 4: Frame enum + RPC error envelope

**Spec:** §4.2, §4.7. **File created:** `crates/tau-plugin-protocol/src/frame.rs`. **Files modified:** `error.rs` (extended), `lib.rs` (re-exports).

**Summary.** Define the `Frame` enum with three variants — `Request { id: u32, method: String, params: Vec<u8> }`, `Response { id: u32, error: Option<RpcErrorEnvelope>, result: Option<Vec<u8>> }`, `Notification { method: String, params: Vec<u8> }` — that decode from a MessagePack array per the MessagePack-RPC spec. Implement `Frame::decode(body: &[u8]) -> Result<Frame, ProtocolError>` and `Frame::encode(self) -> Result<Vec<u8>, ProtocolError>`. Define `RpcErrorEnvelope { code: i32, message: String, data: Option<rmpv::Value> }` and the RPC error code constants (`PARSE_ERROR = -32700`, `METHOD_NOT_FOUND = -32601`, `INVALID_PARAMS = -32602`, `INTERNAL_ERROR = -32603`, `PLUGIN_CONTRACT_VIOLATION = -32000`, `CAPABILITY_DENIED = -32001`).

**Tests.** Unit tests for each frame variant: round-trip via `Frame::encode` → `Frame::decode`; rejection of non-array bodies, wrong arity, wrong type-discriminator. Doctest on `Frame` marked `ignore` (`#[non_exhaustive]`).

**Verification.** `cargo test -p tau-plugin-protocol --all-targets`, `cargo test -p tau-plugin-protocol --doc`, `cargo clippy -p tau-plugin-protocol --all-targets -- -D warnings`, `cargo fmt --all -- --check`.

**Commit:** `feat(tau-plugin-protocol): Frame enum + RPC error envelope`.

---

### Task 5: Handshake + shutdown payload types

**Spec:** §4.4, §4.8. **Files created:** `crates/tau-plugin-protocol/src/handshake.rs`. **Files modified:** `lib.rs` (re-exports).

**Summary.** Define `HandshakeRequest { protocol_version: String, port: PortKind, trace_context: TraceContext, config: serde_json::Value }`. Define `HandshakeResponse { protocol_version: String, provides: PortKind, plugin_name: String, plugin_version: String, methods: Vec<String>, schemas: BTreeMap<String, MethodSchema> }`. Define `MethodSchema { params: serde_json::Value, result: serde_json::Value }`. Define `TraceContext { run_id: String, agent_id: String, root_span_id: String }`. Define a `meta::SHUTDOWN_METHOD: &str = "meta.shutdown"` constant.

**Tests.** Unit + integration: serialize/deserialize each type via `rmp-serde`. Verify `port: PortKind` round-trips through MessagePack as a string (custom serde from Task 2). Doctests `ignore`-marked.

**Verification.** Per-task verification commands.

**Commit:** `feat(tau-plugin-protocol): handshake + shutdown payload types`.

---

### Task 6: FakeStdioPeer (test-support feature)

**Spec:** §10.2. **Files created:** `crates/tau-plugin-protocol/src/test_support.rs`. **Files modified:** `lib.rs`, `Cargo.toml` (already has the `test-support` feature from Task 1).

**Summary.** Behind `#[cfg(feature = "test-support")]`, expose `FakeStdioPeer` per spec §10.2 — a struct that, given a pair of `tokio::io::duplex` halves, lets a test drive both sides of the protocol synchronously. Methods: `expect_handshake() -> HandshakeRequest`, `send_handshake_response(resp: HandshakeResponse)`, `expect_request(method: &str) -> (u32, Vec<u8>)`, `send_response(id: u32, result: impl Serialize)`, `send_response_error(id: u32, code: i32, message: &str)`, `send_stream_chunk(id: u32, chunk: CompletionChunk)`, `send_crash(self)` (drops; framer sees EOF on next read).

**Tests.** Self-test (`#[cfg(test)]` inside `test_support.rs`): drive a fake peer through a handshake + one request/response cycle.

**Verification.** `cargo test -p tau-plugin-protocol --features test-support --all-targets`, `cargo test -p tau-plugin-protocol --features test-support --doc`, lint + fmt.

**Commit:** `feat(tau-plugin-protocol): FakeStdioPeer test-support helper`.

---

### Task 7: tau-plugin-sdk tracing-stderr layer + framer integration

**Spec:** §5.1 (crate layout, error.rs, tracing_layer.rs, framer wiring). **Files created:** `crates/tau-plugin-sdk/src/error.rs`, `crates/tau-plugin-sdk/src/tracing_layer.rs`. **Files modified:** `crates/tau-plugin-sdk/src/lib.rs`.

**Summary.** Define `SdkError` (typed wrapper over `ProtocolError`, plus IO and serde variants — no `Internal` variant). Define `tracing_layer::install()` that builds a `tracing-subscriber` `Registry` with a `tracing_subscriber::fmt::layer().json().with_writer(std::io::stderr)` configuration plus an `EnvFilter` (default `info`, override `RUST_LOG`). Re-export `tau_plugin_protocol::FramedReader`/`FramedWriter` so plugin authors and the runner internals share one framer type.

**Tests.** Unit test for `tracing_layer::install`: assert idempotent re-install behavior (or that double-install errors). Doctest on `SdkError` marked `ignore`.

**Verification.** Per-task. Includes `cargo test -p tau-plugin-sdk --doc`.

**Commit:** `feat(tau-plugin-sdk): tracing-stderr layer + framer integration`.

---

### Task 8: tau-plugin-sdk handshake response builder

**Spec:** §4.4. **Files created:** `crates/tau-plugin-sdk/src/handshake.rs`. **Files modified:** `lib.rs`.

**Summary.** `pub fn respond_for_port(framer: &mut FramedWriter, request_id: u32, port: PortKind, plugin_meta: PluginMeta) -> Result<(), SdkError>`. `PluginMeta { name, version, methods, schemas }` carries plugin-author-supplied facts; the runners (Task 9) build it from a const lookup table per port. Reads handshake config from the host's `HandshakeRequest::config` field and stashes it for the runner to feed to a `Configure::from_config` call (Task 10).

**Tests.** Drive the function via `FakeStdioPeer`: verify the response has the right shape; verify validation errors when `provides` mismatches.

**Commit:** `feat(tau-plugin-sdk): per-port handshake response builder`.

---

### Task 9: tau-plugin-sdk runners (LlmBackend + Tool, with streaming)

**Spec:** §5.2, §4.6. **Files created:** `crates/tau-plugin-sdk/src/runners/{mod.rs,llm_backend.rs,tool.rs,storage.rs,sandbox.rs}`, `crates/tau-plugin-sdk/src/streaming.rs`. **Files modified:** `lib.rs`.

**Summary.** Implement `pub async fn run_llm_backend<T: LlmBackend + Send + Sync + 'static>(plugin: T) -> Result<(), SdkError>` per spec §5.2. The function: installs the tracing layer (Task 7), reads the handshake request (Task 5), responds via Task 8's builder, then runs the dispatch loop matching `method` against the LlmBackend method set (`llm.complete`, `llm.complete_streaming`, `meta.describe`). For `complete_streaming`, the streaming helper turns the `CompletionStream` returned by the plugin's trait method into `stream.chunk` notifications carrying the originating msgid. `run_tool` follows the same pattern but only handles `tool.call` and `tool.describe`. `run_storage` and `run_sandbox` are stubbed bodies (define the signature; emit a clear error if invoked) — they're loadable by the host but no toy plugin exercises them in v0.1 per spec §1.1.

**Tests.** Two integration test files:
- `crates/tau-plugin-sdk/tests/run_llm_backend_via_fake_peer.rs`: spawn `run_llm_backend(EchoLlm)` on a tokio task; drive the fake peer through handshake + one `complete` call + one streaming call. Verify the chunks arrive in order and the final response carries the right stop_reason.
- `crates/tau-plugin-sdk/tests/run_tool_via_fake_peer.rs`: same but for `run_tool(EchoTool)` exercising one `tool.call`.

**Verification.** `cargo test -p tau-plugin-sdk --all-features --all-targets`, `cargo test -p tau-plugin-sdk --doc`, lint, fmt.

**Commit:** `feat(tau-plugin-sdk): per-port runners with streaming`.

---

### Task 10: Configure trait + ConfigError + run_*_with_config

**Spec:** §5.4. **Files created:** `crates/tau-plugin-sdk/src/configure.rs`. **Files modified:** `lib.rs`, `runners/{llm_backend,tool,storage,sandbox}.rs`.

**Summary.** Add `pub trait Configure { type Config: serde::de::DeserializeOwned; fn from_config(config: Self::Config) -> Result<Self, ConfigError> where Self: Sized; }` and `ConfigError` per spec §5.4. Add `pub async fn run_llm_backend_with_config<T>() -> Result<(), SdkError> where T: LlmBackend + Configure + Send + Sync + 'static`. The function does the handshake first (without an instance), then deserializes the config field via `T::Config::deserialize`, calls `T::from_config`, and continues into the regular dispatch loop. Same for `run_tool_with_config`. (`Storage` / `Sandbox` similar — stubbed.)

**Tests.** `crates/tau-plugin-sdk/tests/configure_roundtrip.rs`: drive `run_llm_backend_with_config::<EchoLlm>()` through a fake peer with `config = { canned_text: "hi" }`; verify the plugin receives the deserialized config and uses it.

**Verification.** Per-task.

**Commit:** `feat(tau-plugin-sdk): Configure trait + run_*_with_config flavors`.

---

### Task 11: tau-pkg manifest table parsing + InstallOptions::build + BuildOptions

**Spec:** §6.1, §6.4. **Files modified:** `crates/tau-pkg/src/manifest.rs`, `crates/tau-pkg/src/install.rs`, `crates/tau-pkg/src/lib.rs`.

**Summary.** Extend the existing `read_manifest` flow to parse the optional `[plugin]` table and surface it as `Option<PluginManifest>` on the `PackageManifest`. (If `PackageManifest` is `#[non_exhaustive]` and a new field would break struct-literal construction, add a `with_plugin` builder method on `PackageManifest` and use it during validation.) Add `InstallOptions::build: BuildOptions` and `BuildOptions { skip_build: bool, cargo_path: Option<PathBuf>, extra_args: Vec<String> }` — both `#[non_exhaustive]`. Default: build enabled, cargo from PATH, no extra args.

**Tests.** Unit + integration: parse a manifest with `[plugin]`, verify the values; parse one without, verify `plugin = None`. Verify `InstallOptions::default().build.skip_build == false`.

**Verification.** Per-task. Run `cargo test -p tau-pkg --doc`.

**Commit:** `feat(tau-pkg): parse [plugin] manifest table + BuildOptions`.

---

### Task 12: tau-pkg install build step + InstallError::BuildFailed + LockedPlugin + lockfile v2

**Spec:** §6.3, §6.5, §6.6. **Files modified:** `crates/tau-pkg/src/install.rs`, `crates/tau-pkg/src/lockfile.rs`, `crates/tau-pkg/src/error.rs`. **File created:** `crates/tau-pkg/tests/install_builds_rust_cargo_plugin.rs`.

**Summary.** Insert two new steps between **clone** and **lockfile write** in `install_with_options`:

1. Detect plugin manifest. If `[plugin]` absent → skip build.
2. Build (`kind = RustCargo`): spawn `cargo build --release --bin <plugin.bin>` in the cloned package dir. Capture stdout/stderr. Stream stdout to `tracing::info!(target = "tau_pkg::build", ...)`, stderr to `tracing::warn!(target = "tau_pkg::build", ...)`. On non-zero exit: `Err(InstallError::BuildFailed { exit_status, stderr_tail })` (last 4 KiB of stderr). Lockfile NOT written; cloned source NOT removed (user retries with `tau install --force`). On success: `binary_path = canonicalize(<pkg_dir>/target/release/<bin>)`.

Add `InstallError::BuildFailed` and `InstallError::CargoNotFound`. Add `LockedPlugin { manifest: PluginManifest, binary_path: PathBuf, built_at: SystemTime }` to `LockedPackage`. Bump lockfile schema TOML field from `version = 1` to `version = 2`. v1 lockfiles auto-upgrade on next `tau install` (re-read manifests, re-build any `[plugin]` packages, write v2). Older `tau` reading v2 surfaces existing `LockfileVersionTooNew` error path.

**Tests.** Three new tests:
- `install_runs_cargo_build_for_rust_cargo_plugin`: fixture repo cloned + built; assert `binary_path` exists and is executable.
- `install_surfaces_compile_error_as_build_failed`: fixture repo with a deliberate compile error; assert `Err(InstallError::BuildFailed { stderr_tail, .. })` matches an expected substring (`error[E0`).
- `lockfile_v1_auto_upgrades_to_v2_on_next_install`: write a v1 lockfile by hand; run install; assert the file is now `version = 2` and contains the LockedPlugin entry.

**Verification.** Per-task. The fixture repos use `git_fixture` patterns from sub-project 3.

**Commit:** `feat(tau-pkg): build-on-install + LockedPlugin + lockfile v2`.

---

### Task 13: tau-runtime plugin_host module skeleton + RuntimeError variants

**Spec:** §7.1, §7.2, §7.6. **Files created:** `crates/tau-runtime/src/plugin_host/mod.rs`. **Files modified:** `crates/tau-runtime/src/lib.rs`, `crates/tau-runtime/src/error.rs`, `crates/tau-runtime/Cargo.toml`.

**Summary.** Add `tau-plugin-protocol` to `tau-runtime` deps; expand `tokio` features to include `process` + `io-util` (already in workspace deps from Task 1, so just `tokio.workspace = true` if not already). Declare `pub mod plugin_host` in `lib.rs`. Define `plugin_host::PluginHostOptions` (handshake_timeout, shutdown_timeout, max_message_size, recording: `Option<RecordingSink>` — `RecordingSink` defined as a `#[non_exhaustive]` enum with one variant `JsonlFile { path: PathBuf }` for v0.1). Define `plugin_host::TraceContext` mirroring `tau_plugin_protocol::handshake::TraceContext`. Add four new `RuntimeError` variants per spec §7.6: `PluginSpawnFailed`, `PluginHandshakeFailed`, `PluginCrashed`, `PluginContractViolation`. Add `HandshakeFailureReason` sub-enum (`#[non_exhaustive]` with five variants). Stub the four `load_*` public functions returning `unimplemented!()` for now — Task 14 fills them in.

**Tests.** Compile-only at this stage; the new variants need triggering codepaths to be testable. Add a `cargo build` smoke test.

**Verification.** Per-task. `cargo test -p tau-runtime --all-targets` (existing tests must still pass; new variants don't break anything).

**Commit:** `feat(tau-runtime): plugin_host module skeleton + 4 RuntimeError variants`.

---

### Task 14: tau-runtime spawn + handshake + stderr re-emit + shutdown sequence

**Spec:** §7.3, §9.2 (wire-decode tracing). **Files created:** `crates/tau-runtime/src/plugin_host/process.rs`, `crates/tau-runtime/src/plugin_host/handshake.rs`. **File created (test):** `crates/tau-runtime/tests/plugin_host_handshake.rs`.

**Summary.** Implement `PluginProcess` per spec §7.3:

- Spawn: `tokio::process::Command::new(plugin.binary_path).stdin(piped).stdout(piped).stderr(piped).env_clear().env("TAU_PLUGIN_RUN_ID", run_id).env("TAU_PLUGIN_AGENT_ID", agent_id).spawn()`.
- Read loop (single tokio task per plugin): reads frames from stdout, dispatches Response/Notification (no Request handling — host doesn't accept plugin-initiated requests).
- Stderr task: line-reads stderr, parses each line as JSON tracing event, re-emits via `tracing::Event` on `target = format!("plugin::{}", plugin_name)`. Lines that fail JSON parse → `tracing::warn!(target = "plugin::{name}::raw", "{line}")`.
- Shutdown sequence: `meta.shutdown` notification → wait `shutdown_timeout` → SIGTERM → 500 ms → SIGKILL. Tracing event `plugin.exited { plugin, exit_code, signal, clean }`.
- Wire-decode tracing: emit `tracing::event!(target: "tau_runtime::plugin_host::wire", Level::DEBUG, ...)` per frame on read and write paths.

Implement `handshake::drive_handshake(framer: &mut Framer, port: PortKind, trace_context, config) -> Result<HandshakeResponse, RuntimeError>`: sends `meta.handshake` request with msgid=1; awaits response with `handshake_timeout`; validates `protocol_version`, `provides`, required methods. Returns the fully-validated `HandshakeResponse` or the typed `PluginHandshakeFailed { plugin, reason: HandshakeFailureReason }` error.

Add ten new tracing events per spec §7.7.

**Tests.** Drive the handshake via `FakeStdioPeer` (Task 6) for each `HandshakeFailureReason::*` variant. Spawn a real toy binary (a 10-line `tokio::main` that emits a fixed handshake response then exits) — verify the host successfully completes a handshake against it. (Use `crates/tau-runtime/tests/fixtures/handshake_only_plugin/` for this — populated in this task.)

**Verification.** `cargo test -p tau-runtime --all-targets --all-features`, `cargo test -p tau-runtime --doc`, lint, fmt.

**Commit:** `feat(tau-runtime): plugin spawn + handshake + stderr re-emit`.

---

### Task 15: IpcLlmBackend (non-streaming) + IpcTool with mock-peer unit tests

**Spec:** §7.4. **Files created:** `crates/tau-runtime/src/plugin_host/ipc_llm.rs`, `ipc_tool.rs`, `ipc_storage.rs`, `ipc_sandbox.rs`. **File created (test):** `crates/tau-runtime/tests/plugin_host_ipc_llm.rs`, `plugin_host_capability_filter.rs`.

**Summary.** Implement `IpcLlmBackend::complete` per spec §7.4: increment msgid, register oneshot in `in_flight_responses`, send request frame, await response. Implement `IpcLlmBackend::complete_streaming` body returning `Err(LlmError::Unsupported)` for now — Task 16 wires it. Implement `IpcTool` (`call`, `describe`), `IpcStorage` (all four CRUD methods), `IpcSandbox` (`run`). Storage and Sandbox have unit tests via `FakeStdioPeer` only — they're not wired into the kernel end-to-end in v0.1.

Wire `load_llm_backend`, `load_tool`, `load_storage`, `load_sandbox` (in `mod.rs`) to spawn a `PluginProcess`, drive the handshake, and return `Arc::new(IpcLlmBackend { process }) as Arc<dyn DynLlmBackend>` (etc).

**Tests.** Three integration test files:
- `plugin_host_ipc_llm.rs`: drive `IpcLlmBackend::complete` through a `FakeStdioPeer`; verify request shape (method, params), inject response, verify caller receives it.
- `plugin_host_capability_filter.rs`: build a `RuntimeBuilder` with an `IpcLlmBackend` + an `IpcTool`; run a `Runtime::run` call; assert the existing capability filter (sub-project 5 amendment) still applies — i.e., the wire-recorded `CompletionRequest.tools` excludes filtered tools. This is the integration-level proof that capability filter is unchanged under IPC.
- One test for each of `IpcStorage` / `IpcSandbox` in a single `plugin_host_ipc_storage_sandbox.rs` to cover the fake-peer-only path.

**Verification.** Per-task.

**Commit:** `feat(tau-runtime): IpcLlmBackend (non-streaming) + IpcTool + IpcStorage + IpcSandbox`.

---

### Task 16: stream_router + IpcLlmBackend::complete_streaming

**Spec:** §7.4 streaming half + §4.6. **Files created:** `crates/tau-runtime/src/plugin_host/stream_router.rs`. **Files modified:** `ipc_llm.rs` (replace `Unsupported` body).

**Summary.** Implement `stream_router::assemble(chunk_rx: mpsc::Receiver<CompletionChunk>, resp_rx: oneshot::Receiver<RpcResult>) -> CompletionStream` — a `Pin<Box<dyn Stream<...>>>` that yields chunks until the response oneshot fires, at which point it terminates (with a final `LlmError` if the response is an error). Hook `PluginProcess::read_loop` to dispatch `stream.chunk` notifications matching `params[0]: msgid` to the per-msgid `mpsc::Sender<CompletionChunk>` registered in `in_flight_streams`. Update `IpcLlmBackend::complete_streaming` to register both an mpsc and a oneshot before sending the request, then call `stream_router::assemble` on the receivers.

**Tests.** Extend `plugin_host_ipc_llm.rs` with a streaming test: drive `complete_streaming` through `FakeStdioPeer`; inject 3 chunks then a final response; assert the consumer Stream yields exactly those 3 chunks then terminates.

**Commit:** `feat(tau-runtime): IpcLlmBackend::complete_streaming + stream_router`.

---

### Task 17: protocol recording (RecordingSink::JsonlFile + tap wiring)

**Spec:** §7.8, §9.1. **Files created:** `crates/tau-runtime/src/plugin_host/recording.rs`. **Files modified:** `process.rs` (read-loop and writer-mutex tap points). **File created (test):** `crates/tau-runtime/tests/plugin_host_recording.rs`.

**Summary.** Implement `RecordingSink::JsonlFile { path: PathBuf }`. When `PluginHostOptions::recording = Some(JsonlFile { path })`, the read-loop and writer-mutex tap each frame and append a line to the file:

```json
{"ts":1714316451.123,"plugin":"echo-llm","dir":"h2p","msgid":1,"method":"meta.handshake","frame":"<base64>"}
```

The `method` and `msgid` are decoded from the frame body for indexing convenience but are also redundantly encoded in `frame`. Use `base64 = "0.22"` (already in workspace deps) for the body encoding. Open the file in append mode; flush after each line. Tap is best-effort: a recording-side error (file full, permission denied) → emit `tracing::warn!(target = "tau_runtime::plugin_host::recording", ...)` and continue (recording is observability, not correctness).

**Tests.** Drive a single complete + 1 streaming call against a fake peer with recording enabled; read back the JSONL file; assert: 4 frames captured (h2p handshake, p2h handshake, h2p complete, p2h complete) plus 3 stream.chunk notifications + 1 final response for the streaming call.

**Commit:** `feat(tau-runtime): protocol recording (JsonlFile sink)`.

---

### Task 18: echo-llm + echo-tool toy plugin crates

**Spec:** §8.1, §8.2. **Files created:** `crates/tau-plugins/echo-llm/{Cargo.toml,src/main.rs,tau.toml}` and `crates/tau-plugins/echo-tool/{Cargo.toml,src/main.rs,tau.toml}`. **NOTE:** The Cargo.toml stubs and src/main.rs stubs already exist from Task 1; this task replaces them with full implementations.

**Summary.** Replace the stub `Cargo.toml` files with full ones (deps on `tau-plugin-sdk`, `tau-ports`, `tokio`, `serde`, `serde_json`). Replace `src/main.rs` for echo-llm with the full implementation per spec §5.3 / §8.1, including:

- `EchoConfig { canned_text: String, script: Vec<String>, crash_after_handshake: bool, delay_response_ms: Option<u64>, error_on_method: Option<String> }` — the test-only modes per spec §10.3.
- `EchoLlm` impl `LlmBackend` honoring those modes: if `crash_after_handshake`, panic at the start of any method call (handshake completes first because the crash check is in `complete`, not `from_config`). If `delay_response_ms`, `tokio::time::sleep` before responding. If `error_on_method == "complete"`, return `Err(LlmError::Internal)`.
- `EchoLlm` also implements `tau_plugin_sdk::Configure`.
- `#[tokio::main]` entry calls `run_llm_backend_with_config::<EchoLlm>()`.

Replace `src/main.rs` for echo-tool similarly: `EchoConfig { error_on_invoke: bool }`; `EchoTool` impl `Tool` honoring it. `run_tool_with_config::<EchoTool>()`.

Create the `tau.toml` per-package files (the **plugin manifest**, distinct from the workspace Cargo.toml): `provides = "llm_backend"` / `"tool"`, `kind = "rust-cargo"`, `bin = "echo-llm"` / `"echo-tool"`.

**Tests.** Build both binaries (`cargo build --release -p echo-llm -p echo-tool`); spawn each by hand and verify it accepts a handshake (via a small test harness in `crates/tau-plugins/echo-llm/tests/handshake.rs`).

**Verification.** Per-task. Includes `cargo build --release -p echo-llm -p echo-tool` (release build because integration tests use the release binary path).

**Commit:** `feat(tau-plugins): echo-llm + echo-tool toy plugins`.

---

### Task 19: tau-cli — drop test-mock + rewire cmd::run + cmd::chat to plugin_host

**Spec:** §11. **Files modified:** `crates/tau-cli/Cargo.toml` (remove `test-mock` feature + the `tau-ports/test-fixtures` dev-dep that backed it), `crates/tau-cli/src/cmd/run.rs`, `crates/tau-cli/src/cmd/chat.rs`. **File deleted:** `crates/tau-cli/src/cmd/mock_backend.rs`.

**Summary.** Per spec §11: remove the `[features] test-mock = [...]` block from `tau-cli/Cargo.toml`. Delete `mock_backend.rs`. In `cmd::run`:

- Resolve the agent's required plugins via `tau_pkg::registry::list(&scope)`.
- For each `LockedPackage` with `Some(LockedPlugin)`, branch on `plugin.manifest.provides`:
  - `LlmBackend` → `tau_runtime::plugin_host::load_llm_backend(...)`. Bail with `RuntimeError::ConfigurationError` if more than one LLM backend resolves (agents have exactly one).
  - `Tool` → `tau_runtime::plugin_host::load_tool(...)`. Add to a `ToolRegistry`.
  - `Storage` / `Sandbox` → loaded but no end-to-end wiring in v0.1; ignore for now (a later sub-project wires them).
- Build the `Runtime` with these IPC-backed implementations and run `Runtime::run_with_history`.

For `cmd::chat`: same plugin-resolution, but the plugin processes are kept alive across REPL iterations (the long-lived multiplexed lifecycle paying off — TLS reuse, etc.). On `/exit`, shut down all plugin processes via `meta.shutdown` notification.

If `--record-protocol <path>` is set (Task 20 adds the flag), pass `PluginHostOptions::recording = Some(JsonlFile { path })` to all `load_*` calls.

**Tests.** No new tests in this task — Task 21 adds the real-spawn integration tests. This task only verifies the existing snapshot tests still pass (since CLI shape didn't change beyond the new global flag) and that `cargo build -p tau-cli --no-default-features` still passes the existing `no-default-features-cli` CI job.

**Verification.** Per-task. Confirm no remaining `cfg(feature = "test-mock")` reference: `! grep -r 'test-mock' crates/tau-cli/`.

**Commit:** `refactor(tau-cli): drop test-mock feature; rewire cmd::run + cmd::chat to plugin_host`.

---

### Task 20: tau-cli `--record-protocol` flag + `tau plugin {describe,run,protocol decode}` subcommands

**Spec:** §9. **Files modified:** `crates/tau-cli/src/cli.rs` (add global flag + new subcommand group). **Files created:** `crates/tau-cli/src/cmd/plugin/{mod.rs,describe.rs,run.rs,protocol_decode.rs}`.

**Summary.** Add a global `--record-protocol <path>` flag to `Cli` (clap derive). Add a new top-level `Plugin` enum variant on `Command`: `Plugin { #[command(subcommand)] action: PluginAction }` where `PluginAction` is `Describe { name: String }` | `Run { binary: PathBuf, #[arg(long, conflicts_with = "script")] interactive: bool, #[arg(long)] script: Option<PathBuf> }` | `Protocol { #[command(subcommand)] action: PluginProtocolAction }` and `PluginProtocolAction` is `Decode { path: PathBuf, #[arg(long)] filter: Vec<String>, #[arg(long)] from: Option<f64>, #[arg(long)] to: Option<f64>, #[arg(long)] json: bool }`.

Implement each handler per spec §9:
- `describe::run(name)`: resolves plugin from lockfile, spawns it via `plugin_host::load_*` (selecting the right `load_` based on manifest `provides`), reads `meta.describe` per method (advertised in handshake), prints structured output. Cleanly shuts down.
- `plugin::run::interactive(binary)`: spawns the binary, drives a REPL prompting `plugin> `, parses `<method> <json-args>` per line, sends as Request frame, prints decoded Response.
- `plugin::run::script(binary, path)`: same but reads inputs from JSONL file, prints one decoded result line per input.
- `protocol_decode::run(path, filter, from, to, json)`: parses the JsonlFile recording, decodes each frame's MessagePack body via `rmp-serde` to `serde_json::Value`, prints either the human-readable transcript (default) or one JSON line per frame (`--json`).

Add `insta` snapshot tests for `--help` of the new subcommands in `crates/tau-cli/tests/help_snapshots.rs` (extend the existing file).

**Tests.** New snapshots in this task. Real-spawn integration tests for these subcommands land in Task 21.

**Commit:** `feat(tau-cli): --record-protocol flag + tau plugin {describe,run,protocol decode}`.

---

### Task 21: tau-cli integration tests — real-spawn against echo plugins

**Spec:** §10.3. **Files created:** `crates/tau-cli/tests/common/echo_plugins.rs`, `crates/tau-cli/tests/cmd_run_plugin.rs`, `cmd_chat_plugin.rs`, `cmd_plugin_describe.rs`, `cmd_plugin_run_protocol.rs`.

**Summary.** Implement `echo_plugins.rs` per spec §8.5: a `OnceLock<PathBuf>`-cached helper that runs `cargo build --release -p echo-llm -p echo-tool` once per test session, then returns the binary paths. Use `env!("CARGO")` to invoke cargo so the test inherits the workspace toolchain; `target_dir()` resolved from `env!("CARGO_TARGET_DIR")` (or default `target/`).

Implement integration tests (per spec §10.3):

- `cmd_run_plugin.rs`:
  - `tau_run_against_real_echo_llm_returns_canned_response`: spawn temp project, configure `[agents.echo.config]` with `canned_text = "hello back"`, synthesize lockfile pointing at echo-llm/echo-tool binaries, run `tau run echo "say hello"`, assert exit 0 + stdout contains "hello back".
  - `tau_run_propagates_plugin_crash_as_exit_code_2`: same setup but `crash_after_handshake = true`. Assert exit 2 + stderr contains "plugin echo-llm crashed".
  - `tau_run_propagates_plugin_handshake_timeout`: configure echo-llm binary to a fake binary that sleeps 30s before any output. Use `PluginHostOptions { handshake_timeout: 1s, ... }`. Assert exit 2 + stderr contains "handshake failed: Timeout".
- `cmd_chat_plugin.rs`: drive a 3-turn REPL via stdin pipe through echo-llm with `script = ["one", "two", "three"]`; assert each turn's output.
- `cmd_plugin_describe.rs`: install echo-llm to a temp scope; run `tau plugin describe echo-llm`; assert stdout contains "echo-llm 0.1.0  [llm_backend]" + the methods list.
- `cmd_plugin_run_protocol.rs`: spawn echo-llm via `tau plugin run --interactive` with a piped script (one line per command); assert each response line matches expected. Then run a `tau --record-protocol /tmp/wire.log run ...` invocation; verify `wire.log` exists and `tau plugin protocol decode /tmp/wire.log --json` emits the expected handshake + complete frames.

**Tests.** Performance smoke per spec §10.7: one test asserts `handshake_completes_under_one_second_in_release` against echo-llm.

**Verification.** Per-task. Includes the workspace `cargo test --workspace --all-targets`.

**Commit:** `test(tau-cli): real-spawn integration against echo plugins`.

---

### Task 22: CI — 3 new required jobs

**Spec:** §10.6. **File modified:** `.github/workflows/ci.yml`.

**Summary.** Add three new jobs after the existing `no-default-features-cli` job:

```yaml
  no-default-features-protocol:
    name: build (tau-plugin-protocol)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Build tau-plugin-protocol (no default features)
        run: cargo build -p tau-plugin-protocol --no-default-features
      - name: Test tau-plugin-protocol (no default features)
        run: cargo test -p tau-plugin-protocol --no-default-features --lib

  no-default-features-sdk:
    name: build (tau-plugin-sdk)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Build tau-plugin-sdk (no default features)
        run: cargo build -p tau-plugin-sdk --no-default-features
      - name: Test tau-plugin-sdk (no default features)
        run: cargo test -p tau-plugin-sdk --no-default-features --lib

  build-tau-plugins:
    name: build (tau-plugins)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Build toy plugins (release)
        run: cargo build --release -p echo-llm -p echo-tool
```

> The job *names* (the `name:` field) — `build (tau-plugin-protocol)`, `build (tau-plugin-sdk)`, `build (tau-plugins)` — are what GitHub branch protection sees and what we'll need to add to required-status-checks in Task 26.

**Verification.** `cargo build -p tau-plugin-protocol --no-default-features`, `cargo build -p tau-plugin-sdk --no-default-features`, `cargo build --release -p echo-llm -p echo-tool` all pass locally before pushing.

**Commit:** `ci(tau): add tau-plugin-protocol + tau-plugin-sdk + tau-plugins build jobs`.

---

### Task 23: ADR-0008 + index update

**Spec:** §2 (decisions). **Files created:** `docs/decisions/0008-plugin-loading.md`. **File modified:** `docs/decisions/README.md`.

**Summary.** Write ADR-0008 in the MADR style enforced by the existing `docs/decisions/template.md`. Title: "Plugin loading mechanism — IPC over MessagePack-RPC + tau-pkg, tau-runtime, tau-domain amendments". Status: **Proposed** (flips to Accepted in Task 25). Bundle the 18 decisions from spec §2 plus the four amendment summaries (tau-pkg build-on-install, tau-runtime plugin_host, tau-domain PluginManifest types, workspace additions tau-plugin-protocol + tau-plugin-sdk). Add cross-refs to ADR-0004, ADR-0005, ADR-0006, ADR-0007. Document the alternatives considered (dlopen / abi_stable / WASM) per spec §1.2. Document the deferred items (priorities 2-15) per spec §2.1.

Add ADR-0008 row to `docs/decisions/README.md` index (Proposed status; flips to Accepted in Task 25).

**Verification.** Manual review for completeness. `git diff docs/decisions/` to confirm only the two files touched.

**Commit:** `docs(decisions): add ADR-0008 — plugin loading mechanism (Proposed)`.

---

## Tasks 24-26: user-driven gates

### Task 24: Final local verification + mark PR ready

**Files modified:** none (gate task).

- [ ] Confirm all of the following pass locally on the latest commit:

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace --all-features
cargo test --workspace --all-targets --all-features
cargo test --workspace --doc
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo build -p tau-domain --no-default-features
cargo build -p tau-ports --no-default-features
cargo build -p tau-pkg --no-default-features
cargo build -p tau-runtime --no-default-features
cargo build -p tau-cli --no-default-features
cargo build -p tau-plugin-protocol --no-default-features
cargo build -p tau-plugin-sdk --no-default-features
cargo build --release -p echo-llm -p echo-tool
cargo test -p tau-domain --all-features --test escape_hatch_registry
```

- [ ] Confirm CI on the PR is fully green (all 15 required checks).
- [ ] Mark the PR Ready for review (`gh pr ready` if drafted).

**No commit at this task.**

---

### Task 25: ADR-0008 fresh-eyes review (24h or self-review checklist)

Per QG22, ADRs that touch project guidelines wait at least 24 hours between draft and accept, OR pass an inline self-review checklist.

If using the 24h wait: skip ahead to step 25.2.

If using the self-review checklist:

- [ ] **Step 25.1: Self-review checklist (alternative to 24h wait)**

Re-read ADR-0008 from end to start. Confirm:
- [ ] Every decision in spec §2 has a numbered ADR entry.
- [ ] Every alternative considered (dlopen, abi_stable, WASM) is listed with the reason for rejection.
- [ ] Cross-refs to ADR-0004, 0005, 0006, 0007 are correct.
- [ ] Deferred items map to ROADMAP priorities 2-15.
- [ ] No new `Internal` error variants are introduced in this sub-project.
- [ ] The migration path (lockfile v1 → v2; test-mock retirement) is unambiguous.
- [ ] The phase 0 amendments (capability filter, run_with_history) remain correct under IPC.

- [ ] **Step 25.2: Flip ADR-0008 status to Accepted**

Edit `docs/decisions/0008-plugin-loading.md`: change `Status: Proposed` → `Status: Accepted`. Edit `docs/decisions/README.md`: change `Proposed` → `Accepted` in the index row.

```bash
git add docs/decisions/
git commit -m "$(cat <<'EOF'
docs(decisions): accept ADR-0008 — plugin loading mechanism

Self-review (or 24h wait per QG22) complete; no objections found.

Refs: QG22, Task 25 of plan
EOF
)"
git push
```

---

### Task 26: Plan sign-off + ROADMAP + branch protection update + squash merge

- [ ] **Step 26.1: Tick checkboxes in this plan**

Edit `docs/superpowers/plans/2026-04-28-plugin-loading.md`: convert all `- [ ]` checkboxes for completed tasks (Tasks 1-25) to `- [x]`. (Tasks 26's own checkboxes can stay unticked or be ticked at the end.)

- [ ] **Step 26.2: Update ROADMAP**

Edit `ROADMAP.md`:
- Mark Phase 1 priority 1 (plugin loading mechanism) as **completed** in the Tier 1 list.
- Add a new "Phase 1 sub-projects shipped" table row for the plugin loading sub-project with date.
- Update the "Current phase" section if appropriate.

```bash
git add ROADMAP.md docs/superpowers/plans/2026-04-28-plugin-loading.md
git commit -m "docs(plan): tick off plugin loading sub-project + update ROADMAP

Refs: Task 26 of plan
"
git push
```

- [ ] **Step 26.3: Update branch protection — add 3 new required checks**

```bash
gh api repos/LEBOCQTitouan/tau/branches/main/protection -X GET --jq '.required_status_checks.contexts'
```

Capture the existing list. Add the 3 new check names: `build (tau-plugin-protocol)`, `build (tau-plugin-sdk)`, `build (tau-plugins)`. Push the updated list:

```bash
gh api repos/LEBOCQTitouan/tau/branches/main/protection \
  -X PUT \
  -F required_status_checks.strict=true \
  -F required_status_checks.contexts[]='rustfmt' \
  -F required_status_checks.contexts[]='clippy' \
  -F 'required_status_checks.contexts[]=test (ubuntu-latest / stable)' \
  -F 'required_status_checks.contexts[]=test (ubuntu-latest / 1.91)' \
  -F 'required_status_checks.contexts[]=test (macos-latest / stable)' \
  -F 'required_status_checks.contexts[]=test (macos-latest / 1.91)' \
  -F 'required_status_checks.contexts[]=build (no-default-features)' \
  -F 'required_status_checks.contexts[]=build (tau-ports no-default-features)' \
  -F 'required_status_checks.contexts[]=test (tau-ports test-fixtures only)' \
  -F 'required_status_checks.contexts[]=build (tau-pkg no-default-features)' \
  -F 'required_status_checks.contexts[]=build (tau-runtime no-default-features)' \
  -F 'required_status_checks.contexts[]=build (tau-cli no-default-features)' \
  -F 'required_status_checks.contexts[]=build (tau-plugin-protocol)' \
  -F 'required_status_checks.contexts[]=build (tau-plugin-sdk)' \
  -F 'required_status_checks.contexts[]=build (tau-plugins)' \
  -F enforce_admins=true \
  -F required_pull_request_reviews=null \
  -F restrictions=null
```

(Adjust the existing `contexts[]=...` entries to match the actual current list returned in the GET above; the example shows the expected end state with 15 checks.)

- [ ] **Step 26.4: Squash merge PR to main**

After CI is green on the latest commit:

```bash
gh pr merge --squash --delete-branch
```

Use a squash commit message of the form:

```
feat(plugin-loading): Phase 1 sub-project 1 — out-of-process IPC plugin loading

Lands the plugin loading mechanism: out-of-process IPC over
MessagePack-RPC on stdio, with long-lived multiplexed plugin
processes. Two new workspace crates (tau-plugin-protocol,
tau-plugin-sdk); amendments to tau-domain (PluginManifest, PortKind,
PluginKind), tau-pkg (build-on-install, lockfile v2), tau-runtime
(plugin_host module, 4 new RuntimeError variants, 10 new tracing
events), tau-cli (test-mock retired, --record-protocol global flag,
tau plugin {describe,run,protocol decode} subcommands). Two toy
plugins (echo-llm, echo-tool). ADR-0008 bundles all 18 design
decisions.

Closes ADR-0007 §18 (plugin loading deferred to Phase 1+).

Refs: ROADMAP Phase 1 priority 1
```

- [ ] **Step 26.5: Verify main is clean**

```bash
git checkout main
git pull
git log --oneline -5
```

Confirm the squash commit is at HEAD and the branch protection update did not block the merge.

---

## Self-review notes (for the plan author)

Spec coverage check (cross-check against spec §1.1 — "v0.1 ships"):

| Spec deliverable | Implementing task |
|---|---|
| Mechanism (IPC) | Tasks 3, 4, 5, 13, 14, 15 |
| Toy plugins (echo-llm, echo-tool) | Task 18 |
| Storage / Sandbox loaders (mechanism only) | Task 15 |
| Debug tier (recording, live decode, 3 subcommands) | Tasks 17, 20 |
| tau-pkg amendment (build-on-install) | Tasks 11, 12 |
| tau-runtime amendment (plugin_host, 4 RuntimeError variants, 10 tracing events) | Tasks 13, 14, 15, 16, 17 |
| tau-domain amendment (PluginManifest, PortKind, PluginKind) | Task 2 |
| tau-plugin-protocol crate | Tasks 1, 3, 4, 5, 6 |
| tau-plugin-sdk crate | Tasks 1, 7, 8, 9, 10 |
| ADR-0008 | Tasks 23, 25 |
| CI 3 new jobs | Task 22 |
| Branch protection update | Task 26 |
| `test-mock` retirement | Task 19 |

Spec §1.1 "does NOT ship":

- Real LLM-backend / Tool plugins → out of scope (priorities 2 + 3).
- OS sandboxing → out of scope (priority 12).
- Auto-restart → out of scope (deferred indefinitely).
- Conformance suite → out of scope (deferred until 2 implementations exist).
- Multi-port plugins → out of scope.

All match the plan's scope. No gaps.

Plan-erratum carry-overs accounted for in every task that touches affected types (doctest `ignore`, separate `cargo test --doc`, let-else for `#[non_exhaustive]` destructure, no new `Internal` variants).

Type consistency: `PluginManifest`, `PortKind`, `PluginKind` defined Task 2; consumed Task 5 (handshake), Task 11 (manifest parsing), Task 12 (LockedPlugin), Task 13 (load_* signatures). `PluginProcess`, `IpcLlmBackend`, `PluginHostOptions`, `RecordingSink` defined Task 13/14, consumed Tasks 15-21. `Configure`, `ConfigError`, `run_*_with_config` defined Task 10, consumed Task 18 (toy plugins). All type names consistent across tasks.
