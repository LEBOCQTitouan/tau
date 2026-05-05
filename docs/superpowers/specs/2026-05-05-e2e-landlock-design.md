# End-to-end landlock CI integration + port-aware Layer 4 driver — design

**Date:** 2026-05-05
**Status:** Accepted
**Branch:** `feat/e2e-landlock-spec`
**Predecessors:**
- [ADR-0014](../../decisions/0014-sandboxing.md) — sandboxing infrastructure
- [ADR-0015](../../decisions/0015-sandbox-activation.md) — sandbox activation by default
- [ADR-0016](../../decisions/0016-plugin-compat-verification.md) — plugin compatibility verification
- [Followups doc](2026-05-03-sandboxing-followups.md) — sub-project D (this work)

**Audience:** the implementer of sub-project D and any reviewer of the resulting plan + PR.

## Context

Sub-project B ([ADR-0016](../../decisions/0016-plugin-compat-verification.md)) merged at `b81de81` on 2026-05-04. It absorbed sub-project D's *foundation* (controlled-environment binary at `crates/tau-plugin-compat/fixtures/controlled-env-binary/` + landlock-symlink fix in `tau-sandbox-native::light::resolve_symlinks_for_landlock`), leaving D with a reduced scope.

Two debts remain that this sub-project retires:

1. **5 e2e kernel-enforcement test files** were removed at priority-12 ship because Ubuntu's `/bin → /usr/bin` symlinks combined with landlock V1 path-lookup returned EACCES on real binary spawns. B's symlink fix unblocked them; D re-introduces them using the controlled-env binary.

2. **10 `#[ignore]`'d Layer 4 plugin-compat tests** were scaffolded in B with rationale: the existing `tau plugin run --script` driver hardcodes the handshake port to `LlmBackend`, breaking tool-port plugins; cassette-replay-through-sandboxed-process for HTTP plugins doesn't yet exist. D builds a port-aware test driver and flips 7 of the 10 ignores.

The 3 remaining `#[ignore]`'d tests (container × HTTP plugins) need sub-project F's per-host network filter to test cleanly; deferring them keeps D's scope focused.

## Goal

Retire the kernel-enforcement debt (5 e2e files re-introduced) AND the plugin-compat debt (7 of 10 ignored Layer 4 tests flipped) with real-kernel verification on Linux CI. Establish a port-aware test-driver pattern that future sub-projects (E, F, J, K) extend.

## Design decisions

### Decision 1 — Single sub-project, both halves shipped together

**Decision:** ship the 5 e2e files AND the port-aware driver + flipped tests as one sub-project. ~10 days; one PR; one verification milestone.

**Context:** the work has two distinct flavors (kernel-enforcement tests using the controlled-env binary vs plugin-compat tests using real plugins + driver). Splitting was viable but the followups doc's framing kept them together, and treating both as one verification milestone matches sub-project A and B's pattern.

**Consequences:**
- One PR, ~16 commits across the implementation tasks.
- One CI matrix run validates both halves end-to-end.
- Test ownership stays clear: adapter tests in `tau-sandbox-native`, runtime tests in `tau-runtime`, plugin-compat tests in `tau-plugin-compat`.

### Decision 2 — Driver = thin wrapper in `tau-plugin-compat::driver` reusing `plugin_host` machinery

**Decision:** the port-aware driver is a new module `crates/tau-plugin-compat/src/lib.rs::driver` exposing `spawn_under_sandbox`, `DrivenPlugin::invoke_tool`, `DrivenPlugin::complete_llm`, etc. It internally calls `tau_runtime::plugin_host::process::PluginProcess::spawn_and_handshake` with `PluginHostOptions { sandbox_adapter: <test-supplied>, .. }`. No new public CLI surface.

**Context:** alternatives were (A) extending `tau plugin run` with `--port=<kind>` (adds public CLI surface to a debug verb) and (B) duplicating spawn-and-handshake logic in a test-only helper that doesn't reuse `plugin_host` (sub-project B's `tau-pkg::sandbox_check` pattern). Option C reuses the production code path; sub-project B's constraint (`tau-pkg` can't depend on `tau-runtime`) doesn't apply here — `tau-plugin-compat` already depends on `tau-runtime` (Task 4 of sub-project B added it).

**Consequences:**
- Driver module ~150 LOC + ~5 unit tests in `tau-plugin-compat/src/lib.rs`.
- Tests construct `Frame::Request` for `tool.call` / `llm.complete` / etc., dispatch through `DrivenPlugin::call`, decode the result.
- Future sub-projects (E, F, J, K) extend the same driver; no new test infrastructure crate needed.

### Decision 3 — Flip 7 of 10 Layer 4 tests; 3 container HTTP plugin tests stay `#[ignore]`'d

**Decision:** of the 10 currently-`#[ignore]`'d Layer 4 plugin-compat tests:
- **Flip 4 tool plugin tests** (shell + fs-read × container + native) via direct method invocation through the new driver.
- **Flip 3 native HTTP plugin tests** (anthropic + ollama + openai under native) via cassette-replay over inherited netns localhost.
- **Keep 3 container HTTP plugin tests `#[ignore]`'d** with rationale pointing to sub-project F. Container netns isolation requires sub-project F's nftables-in-netns work (or `--network=host` regression on sub-project A's "real adapter, real enforcement" promise) to test cleanly.

**Context:** alternatives were (A) flip all 10 (forces solving netns-localhost in this sub-project; bleeds scope into sub-project F's territory) and (B) flip only the 4 tool plugin tests (simplest scope; leaves 6 native+container HTTP plugin tests still ignored).

**Consequences:**
- Net: 7 of 10 ignores flipped. 3 container HTTP plugin tests stay flagged with a clear rationale string pointing to sub-project F.
- Tool plugins fully verified under both adapters.
- Native HTTP plugins verified via the v0.1 over-permissive netns inheritance (per priority-12 `tau-sandbox-native::net::unshare_flags_for_plan` — when `Network(Http)` is requested, child inherits parent netns; localhost works).
- The 3 deferred tests retire in sub-project F as that work lands proper per-host network filtering.

**Why HTTP-plugin native works today:** the v0.1 native adapter doesn't isolate the netns when `Network(Http)` is in the plan — that's the priority-12 over-permissiveness documented in `tau-sandbox-native::net`. Localhost in the child process is the host's localhost. Cassette server bound to 127.0.0.1 is reachable.

**Why HTTP-plugin container doesn't work today:** container has its own netns. Localhost INSIDE the container ≠ host localhost. Solving cleanly requires either (a) `--network=host` (defeats network isolation, regression on sub-project A's promise), or (b) the proper per-host filtering sub-project F is scheduled to ship.

### Decision 4 — 5 e2e kernel-enforcement files at original per-crate locations

**Decision:**
- 4 files at `crates/tau-sandbox-native/tests/`: `light_landlock.rs`, `strict_seccomp.rs`, `strict_net_filter.rs`, `strict_exec_gating.rs` (the last `#[ignore]`'d for sub-project E)
- 1 file at `crates/tau-runtime/tests/sandbox_native.rs`

**Context:** the alternative was centralizing all 5 in `tau-plugin-compat/tests/`. Per-crate co-location was chosen because the tests verify specific surfaces in specific crates (`light.rs` → `light_landlock.rs` etc.); centralization distances tests from the code they exercise.

**Consequences:**
- `tau-sandbox-native` and `tau-runtime` each gain a new `integration-tests` Cargo feature.
- Tests reference the controlled-env binary at `crates/tau-plugin-compat/fixtures/controlled-env-binary/` via relative path. Fixtures are data; cross-crate references are fine.
- Test ownership matches the workspace's existing pattern: each crate owns its tests.

### Decision 5 — 3 separate Linux CI jobs

**Decision:** add 2 new Linux-only CI jobs:
- `test (tau-sandbox-native e2e / linux)` — runs the 4 adapter e2e tests
- `test (tau-runtime e2e / linux)` — runs the 1 runtime e2e test

The existing `test (tau-plugin-compat / linux)` job is unchanged in shape — it runs the 7 newly-flipped Layer 4 tests as part of its existing test invocation.

**Context:** alternatives were (A) extending the existing `test (tau-plugin-compat / linux)` job to run sandbox-native + runtime e2e tests too (zero new check names but misleading job ownership when failures point at the wrong crate) and (C) bundling sandbox-native + runtime e2e under a single `test (sandbox-e2e / linux)` job (1 new check name; blurs adapter vs runtime concerns).

**Consequences:**
- Branch protection rises 27 → 29 (one GitHub-settings change after first push).
- Each test job names the crate it tests; CI failure attribution is unambiguous.
- Per-job parallelism on GH Actions can be faster than one large sequential job.

## Architecture

The work splits across three layers:

| Layer | Crate | Files modified or created |
|---|---|---|
| Adapter e2e | `tau-sandbox-native` | new `integration-tests` feature in `Cargo.toml`; 4 new test files; ~30 LOC controlled-env binary mode-flag extension |
| Runtime e2e | `tau-runtime` | new `integration-tests` feature; 1 new test file `tests/sandbox_native.rs` |
| Plugin-compat driver | `tau-plugin-compat` | new `src/lib.rs::driver` module (~150 LOC + 5 unit tests); 7 `#[ignore]` flips in `tests/layer4_*.rs`; ~30 LOC HTTP-plugin cassette-replay-on-localhost setup helpers |
| Documentation | (root) | new `docs/reference/sandbox-platform-support.md` |
| CI | `.github/workflows/ci.yml` | 2 new Linux jobs |

Total LOC delta: ~700 across all crates. ~12 new tests; 7 newly-passing previously-ignored.

## Components

### `tau-sandbox-native::tests/light_landlock.rs` (new)

```rust
#![cfg(feature = "integration-tests")]
#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::Command;
use tau_ports::SandboxPlan;

const CONTROLLED_ENV_BIN: &str = "tau-controlled-env";

fn locate_controlled_env_bin() -> PathBuf { /* fixtures/controlled-env-binary/target/release/ */ }

#[test]
fn allowed_read_succeeds() {
    let tmp = tempfile::TempDir::new().unwrap();
    let allowed = tmp.path().join("allowed.txt");
    std::fs::write(&allowed, b"OK").unwrap();

    let plan: SandboxPlan = serde_json::from_value(serde_json::json!({
        "capabilities": [{"kind": "fs.read", "paths": [tmp.path().to_str().unwrap()]}],
        "context": null, "limits": null,
    })).unwrap();

    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "read")
       .env("TAU_FIXTURE_INPUT_PATH", &allowed);

    let _handle = tau_sandbox_native::__internals::wrap_spawn(&plan, &mut cmd).unwrap();
    let output = cmd.output().expect("spawn");

    assert!(output.status.success(), "expected success, got {:?}", output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("READ_OK OK"));
}

#[test]
fn blocked_read_returns_eacces() { /* analogous; assert non-zero + EACCES */ }
```

(Adjust `tau_sandbox_native::__internals::wrap_spawn` to whichever public-test surface exists; sub-project A may have introduced a test-only export.)

### `tau-sandbox-native::tests/strict_seccomp.rs` (new)

Same pattern; binary in `open-socket` mode; without `Network(Http)` capability, kernel SIGSYSes the process. Assert `output.status.signal() == Some(libc::SIGSYS as i32)`.

### `tau-sandbox-native::tests/strict_net_filter.rs` (new)

Binary in `open-socket` mode; with `Network(Http)` capability, kernel allows the socket call. Assert exit 0.

### `tau-sandbox-native::tests/strict_exec_gating.rs` (new, stub)

`#[ignore]`'d test stub with rationale string pointing to sub-project E (landlock V2 needed).

### `tau-runtime/tests/sandbox_native.rs` (new)

Tests `plugin_host` integration: spawns the controlled-env binary as if it were a plugin (it doesn't speak the IPC protocol, so the test asserts handshake-fail-as-expected). Verifies that the runtime correctly threads the native adapter through to plugin spawn.

### `tau-plugin-compat/src/lib.rs::driver` (new module)

```rust
pub mod driver {
    use std::path::Path;
    use std::sync::Arc;
    use tau_domain::PortKind;
    use tau_plugin_protocol::Frame;
    use tau_runtime::plugin_host;

    pub async fn spawn_under_sandbox(
        plugin_path: &Path,
        port: PortKind,
        adapter: Option<Arc<plugin_host::SandboxAdapter>>,
        plan: Option<&tau_ports::SandboxPlan>,
    ) -> Result<DrivenPlugin, DriveError> { /* wraps plugin_host::process::PluginProcess::spawn_and_handshake */ }

    pub struct DrivenPlugin { process: Arc<plugin_host::process::PluginProcess> }

    impl DrivenPlugin {
        pub async fn call(&self, method: &str, params: Vec<u8>) -> Result<Vec<u8>, DriveError> { /* sends Frame::Request, awaits response */ }
        pub async fn invoke_tool(&self, args: serde_json::Value) -> Result<tau_ports::ToolResult, DriveError>;
        pub async fn complete_llm(&self, request: serde_json::Value) -> Result<serde_json::Value, DriveError>;
        pub async fn shutdown(self) -> Result<(), DriveError>;
    }

    #[non_exhaustive]
    #[derive(Debug, thiserror::Error)]
    pub enum DriveError { /* SpawnFailed, HandshakeFailed, RpcFailed, DecodeFailed, PluginCrashed */ }
}
```

### Controlled-env binary update

```
crates/tau-plugin-compat/fixtures/controlled-env-binary/src/main.rs (modify; +30 LOC)
```

Add `TAU_FIXTURE_MODE` env-var dispatch:

| Mode | Behavior |
|---|---|
| `read` (default if `TAU_FIXTURE_INPUT_PATH` set) | existing — reads file, emits `READ_OK <bytes>` |
| `open-socket` | calls `socket(AF_INET, SOCK_STREAM, 0)`; emits `SOCKET_OK` on success |
| `exec <cmd>` | `Command::new(<cmd>)` — proxy the spawn |
| `default` (or unset mode) | existing — emits `CONTROLLED_ENV_OK` |

Mode dispatch keeps the binary statically linked (no new deps) and predictable.

### `tau-plugin-compat/tests/layer4_container.rs` and `layer4_native.rs`

Flip 7 of 10 `#[ignore]` attributes per Decision 3. Test bodies use `driver::spawn_under_sandbox` + `invoke_tool` / `complete_llm`. HTTP plugin tests additionally set up a `wiremock` cassette server before spawning.

### `docs/reference/sandbox-platform-support.md` (new)

```markdown
# Sandbox platform support

## Required kernel features

- Linux kernel ≥ 5.13 (landlock V1)
- Unprivileged user namespaces (kernel ≥ 4.18; enabled by default on most distros)
- seccomp BPF (kernel ≥ 3.5; ubiquitous)

## Tested distros

- Ubuntu 22.04+ (CI primary)
- Ubuntu 24.04 (CI matrix)
- (other distros TBD as users report)

## Known limitations

- Per-host network filtering is over-permissive at v0.1 (when `Network(Http)` is in the plan, child inherits parent netns). Tracked in [sub-project F](../superpowers/specs/2026-05-03-sandboxing-followups.md#sub-project-f).
- Per-command exec gating requires landlock V2 (kernel ≥ 5.19); v0.1 has a no-op stub. Tracked in [sub-project E](../superpowers/specs/2026-05-03-sandboxing-followups.md#sub-project-e).
- macOS / Windows native adapters not yet shipped — use the container adapter on those hosts. Tracked in sub-projects J and K.

## Verification

The native adapter's kernel-enforcement is verified end-to-end on Linux CI via `cargo test -p tau-sandbox-native --features integration-tests --tests`. See ADR-0017.
```

## Data flow

(Section 3 of brainstorm verbatim — adapter e2e flow, plugin-compat driver flow, HTTP plugin cassette-replay flow, skip-with-message paths.)

## Error handling

(Section 4 of brainstorm verbatim — DriveError taxonomy, test failure rendering, controlled-env binary error semantics, cassette-replay error paths, test isolation, CI failure attribution.)

## Testing strategy

(Section 5 of brainstorm verbatim — test inventory ~16 new + 7 newly-passing, CI configuration with 2 new Linux jobs, branch protection 27 → 29.)

## Out-of-scope for sub-project D

- Per-host network filtering for container adapter — sub-project F.
- Per-command exec gating via landlock V2 — sub-project E.
- macOS / Windows native adapters — sub-projects J and K.
- Cross-port universal `meta.describe_capabilities` wire mechanism — Phase 2 hardening.
- The 3 container × HTTP plugin Layer 4 tests stay `#[ignore]`'d; they flip when sub-project F lands.

## Forward links

- **Sub-project E** — gains a verified compat baseline. Adapter e2e tests' `strict_exec_gating.rs` stub becomes a real test when E lands.
- **Sub-project F** — flips the 3 deferred container HTTP plugin tests by providing per-host network filtering that lets cassette-replay-server-on-host be reachable from the container netns.
- **Sub-projects J, K** — extend `tau-plugin-compat::driver` for macOS sandbox-exec / Windows AppContainer test coverage.
- **Phase 2 sub-project A (`tau check`)** — gains a verified production surface to re-expose; the e2e tests confirm `tau resolve --check-sandbox` matches real-kernel behavior.
