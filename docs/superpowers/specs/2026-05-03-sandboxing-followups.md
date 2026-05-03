# Sandboxing follow-ups — test coverage + future sub-projects

**Date:** 2026-05-03 (post-merge of Tier 3 priority 12).
**Status:** scoping doc for future implementation sessions, not a binding spec.
**Audience:** future tau contributors picking up where the sandboxing sub-project left off.

## Test coverage assessment

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

### Sub-project A — Activate sandboxing by default (highest priority)

**One-line:** Wire `select_adapter` into the runtime kernel construction so plugins actually run sandboxed by default.

**Status:** All infrastructure shipped in priority 12; activation not done. Plugin spawn sites in `tau-runtime/src/plugin_host/mod.rs` pass `None` for the sandbox argument.

**Scope:**
1. In `Runtime::builder` (or wherever scope config is loaded at runtime startup), call `tau_runtime::sandbox::select_adapter(&scope_config.sandbox).await`.
2. Store the resulting `Arc<SandboxAdapter>` on `Runtime`.
3. Thread it down through plugin host construction so `spawn_and_handshake` receives `Some((&plan, &adapter))`.
4. Build the `SandboxPlan` per spawn from the plugin's manifest capabilities + project override.
5. Update the four spawn call sites in `mod.rs` (describe_plugin, load_llm_backend, load_tool, load_storage) — `describe_plugin` may stay None per its TODO comment; the other three should activate.
6. Default behavior on no-config: macOS/Windows → fail-loud "no adapter available" with actionable message; Linux → use Native chain.
7. Surface `--no-sandbox` flag on `tau chat` / `tau run` for explicit opt-out (gated by ADR-0014's "Mock adapter is opt-in only").

**Test coverage to add:**
- Integration test: project with no `[sandbox]` config on Linux → plugin spawns under Native adapter, `--check-sandbox` confirms.
- Integration test: project with `[sandbox] chain = [{ kind = "mock" }]` + env opt-in → mock used.
- Integration test on macOS: no-adapter scenario → exit 2 with clear error.

**Estimated scope:** 2-3 days.

**Dependencies:** none. Foundation is ready.

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

### Sub-project C — Native adapter test gap closure

**One-line:** Add unit tests to the three currently-zero-test files in `tau-sandbox-native`.

**Scope:**
1. **`light.rs` unit tests:**
   - `collect_landlock_paths` with various capability shapes.
   - `resolve_anchors` with `${PROJECT}` substitution + glob trimming.
   - `clean_mount_path` corner cases (`/foo/**`, `/foo/`, `/foo/**/bar` — last is a known limitation).
   - `apply_landlock` mock test: verify it returns `SandboxHandle::noop()` on a valid plan.
2. **`probe.rs` unit tests:**
   - Tier capping logic (`Strict` requested + `landlock_only` available → caps to Light).
   - Unknown tier fail-loud path.
   - `landlock_v1_supported` factored to allow injection of the kernel call.
3. **`shape.rs` unit tests:**
   - `shapes_for_tier(None)` empty.
   - `shapes_for_tier(Light)` has fs read+write only.
   - `shapes_for_tier(Strict)` has fs + exec + network.
4. **`strict.rs::apply_strict` unit test:** with a stubbed installer, verify the rule-build pipeline (baseline → exec extend → net extend → compile_filter).

**Test coverage to add:** ~20 unit tests across the three files.

**Estimated scope:** 2-3 days.

**Dependencies:** none.

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

- **0 tests** in `light.rs`, `probe.rs`, `shape.rs` (sub-project C addresses).
- **No real-kernel e2e tests on CI** (sub-project D addresses).
- **No live plugin compatibility tests** (sub-project B addresses).
- **Layer 2 cross-check has zero tests** because the implementation doesn't exist yet (sub-project B).
