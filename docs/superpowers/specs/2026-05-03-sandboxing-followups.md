# Sandboxing follow-ups — test coverage + future sub-projects

**Date:** 2026-05-03 (post-merge of Tier 3 priority 12).
**Status:** scoping doc for future implementation sessions, not a binding spec.
**Audience:** future tau contributors picking up where the sandboxing sub-project left off.

> **Update (2026-05-04):** Sub-project A (sandbox activation by default) shipped. See [its design doc](2026-05-04-sandbox-activation-design.md) and [ADR-0015](../../decisions/0015-sandbox-activation.md). Sub-project A's "Status" + "Scope" + "Test coverage to add" sections below are kept for historical reference but are now closed. The 9 remaining sub-projects (B, D, E, F, G, H, I, J, K — and the closed-inline C) remain as listed.

## Test coverage assessment

> **Update (post-merge):** an inline test pass added ~30 unit tests to the
> three previously-zero-test files in `tau-sandbox-native` (`light.rs`,
> `probe.rs`, `shape.rs`). The inventory below reflects pre-followup state;
> the "Coverage gaps" section near the bottom of this document tracks the
> closed gap and the items still outstanding.

Total: **76 sandbox-related tests** across 17 files. Honest read on what's covered and what isn't:

### Per-file inventory

| File | Tests | Lines | What's covered |
|---|---|---|---|
| `tau-domain/src/package/capability.rs` | 12 | — | `CapabilityShape` mappings, `CapabilityShapeSet` ops |
| `tau-ports/src/fixtures.rs` (MockSandbox) | 10 | — | Trait-shape conformance, `SandboxHandle` Drop semantics, error rendering |
| `tau-sandbox-native/src/lib.rs` | 6 | 213 | `NativeSandbox::new`, `supported_shapes`, `validate_plan` |
| `tau-sandbox-native/src/exec.rs` | 4 | 170 | `extend_with_exec_rules` no-op behavior at v0.1 |
| `tau-sandbox-native/src/net.rs` | 5 | 302 | `unshare_flags_for_plan`, `extend_with_network_rules` shape |
| `tau-sandbox-native/src/strict.rs` | 4 | 430 | `baseline_syscall_map` introspection (read/write present, socket absent) |
| `tau-sandbox-native/src/light.rs` | **0** | 177 | **None** |
| `tau-sandbox-native/src/probe.rs` | **0** | 93 | **None** |
| `tau-sandbox-native/src/shape.rs` | **0** | 31 | **None** |
| `tau-sandbox-container/src/lib.rs` | 5 | 183 | Trait-shape conformance, validate_plan rejects Custom |
| `tau-sandbox-container/src/probe.rs` | 2 | 119 | Unknown-binary fast-fail, Auto fallback |
| `tau-sandbox-container/src/runner.rs` | 16 | 409 | argv generation per shape (read/write mounts, network, hardening flags, env forwarding) |
| `tau-runtime/src/sandbox/chain.rs` | 8 | 439 | `select_adapter` (default chain, mock selection, tier mismatch, parsers) |
| `tau-runtime/src/sandbox/plan.rs` | 4 | 113 | `build_plan` (compute_effective passthrough, error propagation) |
| `tau-runtime/src/sandbox/validation.rs` | 5 | 200 | `validate_plan_against_adapter` (returns ALL errors, plan_id threading) |
| `tau-runtime/src/plugin_host/process.rs` (sandbox part) | 2 | — | `spawn_fails_on_validation_error`, `spawn_calls_validate_plan_then_wrap_spawn` |
| `tau-runtime/tests/sandbox_container.rs` | 2 | 71 | wrap_spawn structural verification (gated) |
| `tau-runtime/tests/sandbox_mismatch.rs` | 3 | 62 | Cross-platform validation paths via MockSandbox |

### Coverage gaps

**Critical gaps (zero-test files):**
- `tau-sandbox-native/src/light.rs` (177 LOC) — landlock path collection, anchor resolution, glob trim, `apply_landlock` pre_exec wiring. Zero unit tests. The function is integration-tested only on Linux, and those e2e tests were removed for ship.
- `tau-sandbox-native/src/probe.rs` (93 LOC) — kernel feature detection, tier capping logic. Zero tests.
- `tau-sandbox-native/src/shape.rs` (31 LOC) — `shapes_for_tier` mapping. Zero tests; trivially-shaped function but worth a sanity test.

**Partial gaps:**
- `light.rs::collect_paths`, `resolve_anchors`, `clean_mount_path` — pure functions, easily unit-testable, no tests. The anchor resolution (`${PROJECT}` → cwd) and glob trimming logic are real bugs-in-waiting.
- `strict.rs` covers the syscall map but the `apply_strict` function itself (the orchestrator) isn't unit-tested — only Linux-only integration tests would have exercised it, and those were removed.
- `probe.rs::landlock_v1_supported` — could be dependency-injected to test the cap logic without invoking the kernel.

**Coverage by Sandbox layer (as defined in the spec):**

| Layer | Coverage | Notes |
|---|---|---|
| Layer 1 (plugin SDK type-state) | N/A | Out of scope for v0.1 |
| Layer 2 (install cross-check) | None | Documented in plan but not implemented as tests |
| Layer 3 (pre-flight validation) | Strong | `validation.rs` + `plan.rs` + `chain.rs` well-covered |
| Layer 4 (runtime enforcement) | **Weak** | Mock-based only; no real-kernel verification in CI |

**End-to-end coverage:**
- 2 `tests/sandbox_container.rs` tests verify docker/podman argv shape but never spawn a real container.
- 3 `tests/sandbox_mismatch.rs` tests use MockSandbox cross-platform.
- 5 e2e tests that spawned real binaries under landlock were removed (CI infrastructure couldn't support them reliably).

**Plugin compatibility:** Layer 2 install-time cross-check (manifest declarations vs binary `CAPABILITIES` handshake) was described in the plan but never implemented or tested. The plan-erratum block confirmed this was deferred.

### Honest summary

Coverage is **adequate for the validation/configuration logic** (Layers 3, chain selection, argv building) but **weak for the OS enforcement primitives** (landlock + seccomp + namespace correctness). The strongest gap-closing investment is end-to-end testing on real Linux, which requires CI infrastructure work documented as a follow-up.

---

## Future tasks

Below are concrete sub-project proposals, ordered by priority. Each is sized for a single dedicated implementation session.

### ~~Sub-project A — Activate sandboxing by default~~ ✅ DONE 2026-05-04

**Status:** Shipped 2026-05-04 — see [spec](2026-05-04-sandbox-activation-design.md) and [ADR-0015](../../decisions/0015-sandbox-activation.md). PR #23.

**What landed (and why it diverged from the original scope above):**

The original scope assumed the priority-12 chain model (`select_adapter` against a `[sandbox] chain = [...]` list) would simply be activated. During fresh brainstorming, the chain model was rejected in favor of Bazel-style declarative requirements + adapter registry + resolver. The resulting scope:

1. **Architectural pivot:** removed `tau-runtime::sandbox::chain::select_adapter`; replaced with `tau-runtime::sandbox::resolver::resolve_adapter` against a static `AdapterRegistration` slice. 5-stage filter pipeline (platform → probe → tier → shape → plugin tier) + priority sort.
2. **Schema migration v2 → v3:** `[sandbox] chain` + `minimum_tier` replaced with `required_tier` + `required_shapes`. v2 lockfiles auto-load with a `tracing::warn!` (best-effort migration); v3 is canonical.
3. **`passthrough` adapter:** new ~30-LOC adapter replacing the `Option<None>` "no isolation" branch. Registered first-class in the registry with `tier = None`, `priority = 0`. `--no-sandbox` is sugar for forcing it.
4. **Plugin manifest `[sandbox]` block:** `PluginSandboxRequirements { required_tier, required_shapes }` added to `tau-domain`. Resolver's 5th filter stage rejects adapters below the maximum plugin-required tier.
5. **`PluginHostOptions` integration:** new `sandbox_adapter: Option<Arc<SandboxAdapter>>`, `force_passthrough: bool`, `force_adapter_kind: Option<SandboxAdapterKind>` fields. CLI integration via `tau-cli/src/cmd/plugin_loader.rs::load_plugins`.
6. **Hard refuse on resolution failure:** exit 2 with guided multi-option `ResolutionError::NoAdapterMatches { tried }` diagnostic. No silent fall-through.
7. **CLI surface:**
   - global `--no-sandbox` flag (forces passthrough)
   - global `--sandbox <kind>` flag (forces specific adapter)
   - `tau sandbox status` (read-only diagnostic; probes adapters, prints what would happen)
   - `tau sandbox setup [--tier ...] [--non-interactive]` (atomic write of `[sandbox]` block)
   - `tau resolve --check-sandbox` extended to surface plugin-tier mismatches even when project's `required_tier = none`
8. **Error rendering:** `crates/tau-cli/src/cmd/error_render.rs` with multi-option output and insta snapshot tests.

**What stayed unchanged:**
- The `Sandbox` port, the two concrete adapters (`tau-sandbox-native`, `tau-sandbox-container`), Layer 3 validation logic (`validate_plan_against_adapter`), `wrap_spawn` integration in plugin_host.
- The mock adapter in `tau-ports/src/fixtures.rs`. Sub-project H from this doc handles eventual cleanup.
- `TAU_TESTING_ALLOW_MOCK_SANDBOX=1` env-var injection path (preserved for CLI integration tests).
- Branch protection at 25 required checks (no new CI jobs).

**Tests added:** ~30 unit tests across the workspace (resolver, registry, passthrough, plugin manifest, schema migration, error renderer, CLI subcommands), 3 insta snapshots, 3 new CLI integration tests in `cmd_resolve_check_sandbox.rs`. All 25-job CI matrix green on PR #23.

---

### Sub-project B — Plugin compatibility verification (depends on A)

**One-line:** Verify all 5 existing plugins (anthropic, ollama, openai, fs-read, shell) work under sandbox enforcement; implement Layer 2 install-time cross-check.

**Scope:**
1. **Layer 2 cross-check:** at `tau install` time, after the plugin handshake response is received, compare its `CAPABILITIES` set against the manifest's `[capabilities]` list. Reject install on mismatch; populate `LockedPlugin.required_shapes` from the binary's actual surface.
2. **Per-plugin verification harness:** automated test that for each existing plugin, runs `tau resolve --check-sandbox` and asserts no Layer 3 violations.
3. **Live spawn test:** run each plugin under the activated sandbox (post-A) on a Linux runner and verify it functions correctly. Cover the typical golden path (e.g., for `fs-read`: actually read a file; for `shell`: run a command).
4. Document any plugin manifest discrepancies found and fix them.
5. Add the `--rehash` flag to `tau install` for refreshing v3 lockfiles to v4 (deferred from priority 12).

**Test coverage to add:**
- 5+ plugin compatibility tests on Linux CI (with the e2e infrastructure from sub-project D).

**Estimated scope:** 1 week.

**Dependencies:** Sub-project A (default activation needed for live spawn tests).

---

### ~~Sub-project C — Native adapter test gap closure~~ ✅ DONE INLINE

**Status:** completed in the same branch as priority 12 (post-merge inline pass).

**What landed:**
- `light.rs` — 12 unit tests for `collect_paths`, `resolve_anchors`, `collect_landlock_paths`, plus an `apply_landlock` structural smoke test.
- `probe.rs` — 11 unit tests for the `decide_probe` pure function (extracted via refactor) covering the full tier-capping decision matrix + monotonicity property test, plus side-effect-free smoke tests for `landlock_v1_supported` / `user_ns_supported`.
- `shape.rs` — 4 unit tests covering `None` / `Light` / `Strict` tier shape sets + Light-is-strict-subset-of-Strict invariant.

**Still gap:** `strict.rs::apply_strict` orchestrator (the rule-build pipeline). Could be unit-tested with a stubbed installer, but for v0.1 the orchestrator is essentially a `?`-chain through three pure helpers (each independently tested) plus a single `cmd.pre_exec` wiring. **Tracked in the gap list below.**

---

### Sub-project D — End-to-end landlock CI integration

**One-line:** Establish a reliable CI infrastructure for testing real-kernel landlock + seccomp + namespace behavior on Linux.

**Status:** Removed 5 e2e test files from priority 12 final ship because Ubuntu's `/bin → /usr/bin` symlinks combined with landlock V1 path-lookup returned EACCES on real binary spawns regardless of system_read_paths expansion.

**Scope:**
1. Investigate landlock + symlink resolution on modern Ubuntu — likely needs landlock V2's path resolution semantics OR explicit symlink resolution before adding to ruleset.
2. Build a controlled-environment test binary (small, statically-linked, in a known location) that the e2e tests spawn instead of `/bin/cat` etc.
3. Re-introduce the 5 removed test files using the controlled binary:
   - `tests/light_landlock.rs` (allowed read + blocked read).
   - `tests/strict_seccomp.rs` (block socket without Network capability).
   - `tests/strict_exec_gating.rs` (per-command exec — defer until landlock V2 lands).
   - `tests/strict_net_filter.rs` (allowed socket with Network capability).
   - `tests/sandbox_native.rs` (in tau-runtime, runtime e2e).
4. CI workflow: confirm the existing `cargo test ... --features integration-tests --tests -- --ignored` step passes reliably on `ubuntu-latest`.
5. Document supported kernel versions + distro testing matrix.

**Test coverage to add:** 5+ real-kernel e2e tests, gated on `integration-tests` feature, run only on Linux CI.

**Estimated scope:** 1 week (the landlock + symlinks investigation is the unknown).

**Dependencies:** ideally Sub-project A so e2e tests can also exercise the activated runtime path.

---

### Sub-project E — Per-command exec argument-filter

**One-line:** Implement true per-command exec gating using landlock V2 `AccessFs::Execute`.

**Status:** Currently a v0.1 no-op stub in `exec.rs::extend_with_exec_rules`. Documented TODO.

**Scope:**
1. Detect landlock V2 support in the probe (kernel ≥ 5.19).
2. When `Capability::Process(Spawn { commands })` or `Capability::Filesystem(Exec { paths })` is in the plan, add the listed paths to the landlock ruleset with `AccessFs::Execute` access.
3. Keep `execve` in the seccomp baseline (plugin startup must work).
4. Refuse plans with these capabilities on kernels < 5.19 with a clear message: "per-command exec gating requires landlock V2 (kernel ≥ 5.19); falling back to seccomp-only allow-all-execve".
5. Update unit tests in `exec.rs` to actually exercise the V2 path.

**Test coverage to add:** ~5 tests + e2e verification (depends on D).

**Estimated scope:** 1 week.

**Dependencies:** Sub-project D for e2e infrastructure.

---

### Sub-project F — Per-host network filtering via nftables-in-netns

**One-line:** Replace the v0.1 "inherit parent netns" fallback with real per-host egress filtering.

**Status:** v0.1 strips `CLONE_NEWNET` when `Network(Http)` is requested (over-permissive). `tracing::warn!` fires once-per-process.

**Scope:**
1. Keep `CLONE_NEWNET` always; child runs in fresh empty netns.
2. Create veth pair in parent + move one end to child netns + assign IPs.
3. Resolve hostnames in `Capability::Network(NetCapability::Http { hosts })` to IPs (DNS lookup with timeout).
4. Generate nftables ruleset in child netns: allow egress to resolved IPs + DNS; drop everything else.
5. Need `CAP_NET_ADMIN` inside the user namespace — verify this works for unprivileged users.
6. Fallback when nftables isn't available: stay with the v0.1 over-permissive behavior + warn.
7. Remove the once-per-process warn from `unshare_flags_for_plan` since it's now actually enforcing.

**Test coverage to add:** ~5 tests + e2e (depends on D).

**Estimated scope:** 1.5-2 weeks (nftables tooling complexity).

**Dependencies:** Sub-project D.

---

### Sub-project G — Test fixture helpers (debt cleanup)

**One-line:** Eliminate the JSON round-trip workaround for constructing `#[non_exhaustive]` types in tests.

**Status:** `serde_json::from_value(json!({...}))` for `Capability`, `SandboxPlan`, `WorkingContext`, `ResourceLimits` appears in 6+ test files. Flagged in two code reviews; deferred.

**Scope:**
1. Add a `tau_ports::testing` module gated `#[cfg(any(test, feature = "test-fixtures"))]`:
   - `plan_from_capabilities(caps: serde_json::Value) -> SandboxPlan` (minimal helper for callers who already use JSON).
   - `working_context(working_dir, env) -> WorkingContext`.
   - `resource_limits(memory, cpu) -> ResourceLimits`.
2. Add a `tau_domain::testing` module:
   - `cap_fs_read(paths: &[&str]) -> Capability`.
   - `cap_fs_write(paths: &[&str], max_bytes: Option<u64>) -> Capability`.
   - `cap_net_http(hosts: &[&str], methods: &[&str]) -> Capability`.
   - `cap_process_spawn(commands: &[&str]) -> Capability`.
   - `cap_custom(name: &str) -> Capability`.
3. Migrate existing test sites to use these helpers; drop `serde_json` from the affected dev-deps where possible.

**Test coverage to add:** N/A (this IS the test-coverage helper).

**Estimated scope:** 2-3 days.

**Dependencies:** none.

---

### Sub-project H — MockSandbox production-binary cleanup

**One-line:** Refactor so `MockSandbox` is no longer reachable in production builds.

**Status:** v0.1 added `tau-ports/test-fixtures` to `tau-runtime`'s production deps so `cargo_bin("tau")` can use Mock for CLI integration tests via the env var `TAU_TESTING_ALLOW_MOCK_SANDBOX=1`. This pollutes the production binary.

**Options to evaluate:**
1. Build a dedicated test-only `tau-test` binary that has Mock baked in; CLI integration tests use that instead of `tau`.
2. Move `MockSandbox` out of `tau-ports/fixtures` into a new always-available stub crate (`tau-sandbox-mock`) that the runtime always pulls — but this still ships the no-op implementation in production.
3. Replace CLI integration tests' use of Mock with the real Native adapter on Linux + the Container adapter where Docker is available; skip tests on platforms where neither works.

Option 1 is cleanest. Option 3 is most aligned with security posture (no Mock in production). Pick during the sub-project.

**Estimated scope:** 1 week.

**Dependencies:** none.

---

### Sub-project I — Fork-server pattern for async-signal-safety

**One-line:** Eliminate the `pre_exec` async-signal-safety hazard in the multi-threaded tokio runtime.

**Status:** `KNOWN-LIMITATION` comment in `light.rs` and `strict.rs` documents the malloc-during-fork deadlock window. Small but nonzero risk.

**Scope:**
1. Replace `Command::pre_exec` + `cmd.spawn()` with a fork-server pattern: a single dedicated child process (single-threaded, no malloc-holding threads) that receives spawn requests via IPC and does the actual fork+landlock+seccomp+exec.
2. The runtime sends `(SandboxPlan, Command)` over IPC to the fork-server; the fork-server returns the spawned child's PID + stdio FDs back.
3. Verify async-signal-safety of all closure body operations (no malloc, no string formatting on the failure path — convert errors to numeric codes).

**Test coverage to add:** ~5 tests covering the fork-server lifecycle + crash recovery.

**Estimated scope:** 2-3 weeks. Significant infrastructure work.

**Dependencies:** none, but high risk; sequence after the simpler items.

---

### Sub-project J — macOS sandbox-exec adapter

**One-line:** First-class macOS sandbox via `sandbox_init_with_parameters` (libsandbox FFI).

**Scope:**
1. New crate `tau-sandbox-macos`.
2. FFI bindings for `sandbox_init_with_parameters` (scarce documentation; reverse-engineer from open-source sandbox-exec users).
3. Translate `CapabilityShape` to sandbox-exec profile syntax.
4. Probe + wrap_spawn implementation.
5. Default chain on macOS becomes `[macos, container]`.

**Estimated scope:** 4-6 weeks (the libsandbox FFI is poorly documented).

---

### Sub-project K — Windows AppContainer adapter

**One-line:** First-class Windows sandbox via WinAPI AppContainer.

**Scope:**
1. New crate `tau-sandbox-windows`.
2. `windows-rs` bindings for AppContainer creation.
3. Translate `CapabilityShape` to WinRT capability strings.
4. Probe + wrap_spawn implementation.
5. Default chain on Windows becomes `[windows, container]`.

**Estimated scope:** 4-6 weeks.

---

## Recommended sequencing

```
A (activate)  ──┐
                ├──→ B (plugin compat)
C (native tests)│
                │
D (e2e CI infra)─→ E (per-command exec)
                │
                └──→ F (per-host network)
                
G (fixtures cleanup) — independent, anytime
H (MockSandbox prod cleanup) — independent
I (fork-server) — high value, high effort, sequence later
J (macOS), K (Windows) — Phase 2; cross-platform parity
```

**First-session priority:** A + C in parallel (different files, no conflict).
**Second-session priority:** B once A is done.
**Third-session priority:** D, then E and F can proceed.

## Summary of what didn't ship in priority 12

| Item | Status | Sub-project |
|---|---|---|
| Default sandbox activation | Infrastructure ready, not turned on | A |
| Plugin compatibility verification | Not done at install or runtime | B |
| Layer 2 install cross-check | Not implemented | B |
| `--rehash` flag | Not shipped (warn instead) | B |
| Real-kernel landlock e2e tests | Removed for ship; CI infrastructure needed | D |
| Per-command exec argument filter | v0.1 no-op stub | E |
| Per-host network filtering | v0.1 over-permissive fallback | F |
| `tau check` standalone command | Phase 2 sub-project A | (separate roadmap) |
| `#[capabilities(...)]` proc macro | Not implemented; manifest is authority | (deferred) |
| macOS sandbox-exec adapter | Not implemented | J |
| Windows AppContainer adapter | Not implemented | K |
| Remote sandbox backends | Not implemented | (Phase 2 F) |
| WASM target | Not implemented | (Phase 2 G) |

## Test coverage gaps to track

These are the **deferred gaps** that future sub-projects must close. Listed
explicitly so a future contributor (or future-me) doesn't lose them in the
noise of follow-up work.

### Closed gaps (no longer outstanding)

- ~~**0 tests in `light.rs`, `probe.rs`, `shape.rs`**~~ — addressed inline in the priority-12 branch (~30 unit tests added across the three files; pure-function logic is now fully covered, kernel-syscall paths are still e2e-only territory).

### Outstanding gaps — must be picked up by named sub-projects

| Gap | Sub-project | Why deferred |
|---|---|---|
| **No real-kernel e2e tests on CI.** Validation that landlock actually returns EACCES on an unlisted-path read, that seccomp actually SIGSYS-kills a denied syscall, that namespace unshare actually drops privileges — none of these are exercised by automated CI today. | **D** | Ubuntu CI's `/bin → /usr/bin` symlinks vs landlock V1 path resolution caused the v0.1 e2e tests to fail unrelated to the sandbox logic; needed a controlled-environment test binary. |
| **No live plugin compatibility tests.** `tau resolve --check-sandbox` and live spawn against the 5 existing plugins (anthropic, ollama, openai, fs-read, shell) is unverified at runtime. | **B** | Depends on sub-project A activating the sandbox; running pre-A would only test mock paths. |
| **Layer 2 cross-check is unimplemented and untested.** The plan-erratum block deferred the install-time manifest-vs-`CAPABILITIES`-handshake comparison; lockfile `required_shapes` is currently always populated empty by the install path. | **B** | The cross-check needs the install pipeline to be sandbox-aware (depends on A) and to know which adapter to validate against. |
| **`strict.rs::apply_strict` orchestrator has no direct unit test.** Each of its three helpers (`baseline_syscall_map`, `exec::extend_with_exec_rules`, `net::extend_with_network_rules` + `compile_filter`) is independently tested, but the `?`-chain that wires them isn't unit-tested as a unit. | **C extension** (or rolled into D's e2e work) | At Light tier the orchestrator is trivial; at Strict tier it does enough work that a focused integration test would be valuable. Inline coverage didn't reach this because every wired-up call in the chain is itself well-tested. |
| **Per-command exec gating is a no-op stub.** `exec::extend_with_exec_rules` does nothing at v0.1; the 4 unit tests verify the no-op behavior. The actual gating logic (landlock V2 `AccessFs::Execute`) is unwritten. | **E** | landlock V2 requires kernel ≥ 5.19; needs feature detection + fallback path. |
| **Per-host network filtering is over-permissive.** When `Network(Http)` is requested, the child inherits the parent's full netns. The 5 unit tests verify the unshare-flag decision; the actual nftables-in-netns enforcement is unwritten. | **F** | Needs `CAP_NET_ADMIN` + nftables rule generation + DNS resolution; substantial complexity. |
| **`SandboxHandle` Drop semantics are unit-tested but not integration-tested.** Container adapter relies on `--rm` for cleanup; future cgroup/cidfile-based adapters will need real Drop coverage. | **D** + container e2e | Currently no real-cleanup adapter ships, so the gap is forward-compat only. |
| **No async-signal-safety verification in `pre_exec` chain.** The KNOWN-LIMITATION comments document the malloc-during-fork hazard; no test confirms the closure body is allocation-free on the success path (and it currently isn't). | **I** | The fork-server pattern is the real fix; testing the current closure for signal-safety would just pin the bug. |
| **`tau resolve --check-sandbox` integration tests use `MockSandbox` only.** Real adapter coverage at the CLI level requires the env-var opt-in path (`TAU_TESTING_ALLOW_MOCK_SANDBOX=1`) which is itself a debt item. | **H** | Replacing Mock with real Native + Container in CLI integration tests is part of the H cleanup. |
| ~~**CI doesn't yet exercise the activated runtime path.**~~ ✅ ADDRESSED 2026-05-04 (sub-project A) — the `cmd_resolve_check_sandbox.rs` integration tests now exercise the resolver against the real registry; CLI integration tests that load plugins flow through `wrap_spawn` via the new `PluginHostOptions.sandbox_adapter` field. Real-kernel e2e validation still belongs to sub-project D. | **A** ✅ + **D** for real-kernel | Activation happened; e2e infrastructure for landlock/seccomp still pending. |

### Coverage policy going forward

When a sub-project from this doc is picked up, **its definition of done MUST include closing the corresponding gap row above.** A sub-project that ships behavior without ALSO shipping the validating tests is not finished. This is a deliberate response to the v0.1 lesson where 5 e2e test files had to be removed at the last minute because the underlying CI infrastructure wasn't ready — the gap was created at the END of the sub-project, when it should have been part of the design from the START.

Each sub-project's design doc (when written) should explicitly state:
1. **Test coverage delta** — how many tests are being added, at which layer.
2. **Gap row(s) closed** — references to this list.
3. **New gaps introduced** — added to this list when the sub-project ships.
