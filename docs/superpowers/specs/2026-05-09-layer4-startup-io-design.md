# Layer 4 plugin-compat startup-IO cataloging — design

> **Status:** spec, executing inline. Cuts from main at `e35880e` (PR #47 docs audit).

## Goal

Make the 5 `#[ignore]`'d tests in `crates/tau-plugin-compat/tests/layer4_native.rs` actually run. Each plugin spawned under strict-tier landlock currently EOFs before sending the meta.handshake response because the test's `SandboxPlan` only grants application-data paths and not the plugin's runtime-mechanics startup-IO surface (`/proc/self/*`, `/dev/urandom`, etc.). This is also a real production gap: any user running these plugins under strict tier hits the same EOF.

## Locked decisions

| # | Decision |
|---|---|
| 1 | **Hybrid investigation method.** Start with an analytically-derived candidate baseline (paths every Rust binary needs: tokio runtime, num_cpus, rand init). Iterate via `cargo nextest` runs; fall back to strace inside the lefthook Podman gate's Linux container if the analytical baseline isn't sufficient. |
| 2 | **Hybrid plan derivation.** Universally-needed paths extend `tau-sandbox-native::light::system_read_paths` (production-wide). Plugin-specific paths (per-plugin config dirs, etc.) live in a new `startup_io_paths_for(&plugin_kind)` helper in `tau-plugin-compat`. Mirrors the existing pattern: runtime mechanics in the runtime baseline, application data in the plan. |
| 3 | **Two PRs.** PR 1 (`feat/layer4-startup-io-baseline`) lands the `light.rs` baseline extension + un-`#[ignore]`s the 2 simple plugins (shell + fs-read). PR 2 (`feat/layer4-startup-io-http`) un-`#[ignore]`s the 3 HTTP plugins (anthropic + ollama + openai), adding per-plugin fixtures only if needed. Splits "is the baseline right?" from "do HTTP plugins need extra fixtures?" for cleaner failure isolation. |
| 4 | **Linux-only scope.** The 5 tests are Linux-only (`tau-sandbox-native` is Linux-gated). macOS sandbox-darwin SBPL baseline likely has the analogous gap; deferred to a follow-up if symptoms surface. |
| 5 | **Each baseline path gets a one-line justification comment.** Constitution G12 wants narrow defaults; the comments make it auditable why a path is in the baseline (runtime mechanics, not application data). |

## Components

**MODIFIED**

- `crates/tau-sandbox-native/src/light.rs` — extend the existing `system_read_paths` array in `install_landlock` with universally-needed paths discovered through investigation. Each entry gets a one-line comment justifying it. New unit tests for the new entries' presence + landlock-rule generation; existing baseline tests still pass.
- `crates/tau-plugin-compat/tests/layer4_native.rs` — un-`#[ignore]` the 5 tests across both PRs. Extend each test's plan with `startup_io_paths_for(&plugin_kind)` if the per-plugin helper returns non-empty.

**NEW**

- `crates/tau-plugin-compat/src/startup_io.rs` — public `startup_io_paths_for(plugin_bin: &str) -> Vec<&'static str>` returning plugin-specific paths beyond the runtime baseline. Empty match arms for shell/fs-read in PR 1; populated in PR 2 for HTTP plugins if needed. New module exported from `lib.rs`.

## Architecture

```
LAYER 4 native test execution
─ test plan = SandboxPlan {
    capabilities: [<test-specific app-data caps>],
    + (if any) startup_io_paths_for(plugin_bin) as fs.read
  }
─ resolve_adapter_forced(RegistryKind::Native)
─ NativeSandbox::wrap_spawn(plan, cmd)
   ├─ install_landlock(plan)
   │  ├─ system_read_paths       <-- baseline extension lands HERE
   │  │  (binary load + dyld + libc + /etc + /proc/self + /dev/urandom + ...)
   │  ├─ plan.fs_read_paths      <-- test-specific app-data + per-plugin extras
   │  └─ plan.fs_write_paths
   └─ install_seccomp(plan)
─ Plugin spawns, opens config + tokio runtime files within baseline
─ Plugin sends meta.handshake response (test was previously EOFing here)
─ Driver invokes high-level method (shell.call / fs_read.call / llm.complete)
```

## Investigation strategy

**Analytical candidate baseline (start here):**

| Path | Why |
|---|---|
| `/proc/self/status` | tokio runtime introspection (thread count, etc.) |
| `/proc/self/cmdline` | various crates for self-identification |
| `/proc/self/exe` | std + tokio for binary path resolution |
| `/proc/self/maps` | rand_core for entropy seeding on some platforms |
| `/sys/devices/system/cpu/online` | `num_cpus` crate (transitive dep of tokio) |
| `/dev/urandom` | rand crate, reqwest TLS bootstrap, getrandom fallback |
| `/dev/null` | std::process for closed stdio |
| `/proc/sys/kernel/...` | tokio for some kernel-feature probes |

`/etc` is already in the existing baseline → covers `/etc/resolv.conf` + `/etc/ssl/certs/*`.
`/lib`, `/usr/lib`, etc. are already in the existing baseline → covers libc + dyld.

**Strace fallback:**

If the analytical baseline isn't sufficient, run inside the lefthook Podman gate:

```bash
podman run --rm -v $PWD:/work -w /work tau-podman-gate \
  strace -f -e trace=openat -o /tmp/trace.txt \
    target/debug/<plugin>-plugin
# Then: grep "openat" /tmp/trace.txt | awk '{print $4}' | sort -u
```

This catches conditional paths the analytical pass misses (locale files, distribution-specific config locations).

## Verification

**PR 1 — baseline + simple plugins:**

- `cargo nextest run -p tau-sandbox-native` — baseline unit tests pass; new test asserting `/proc/self/status` etc. are in `system_read_paths` passes.
- `cargo nextest run -p tau-plugin-compat --test layer4_native shell_layer4_native_runs_echo_hello fs_read_layer4_native_reads_data_file` — both pass on Linux CI (no longer `#[ignore]`'d).
- Existing 5 `layer4_container.rs` tests continue passing (no regression in container path).
- `cargo nextest run -p tau-sandbox-native --features integration-tests` — Linux e2e tests pass (no regression in landlock/seccomp behaviour).

**PR 2 — HTTP plugins:**

- `cargo nextest run -p tau-plugin-compat --test layer4_native` — all 5 tests green. The 3 HTTP plugins make actual proxy-routed cassette-replay requests.
- `tau-sandbox-native::strict_proxy.rs` integration tests still pass (no regression in proxy path).

**Branch protection:** No new CI jobs. Existing `test (tau-plugin-compat / linux)` job picks up the un-`#[ignore]`'d tests automatically.

## Risks & mitigations

| Risk | Mitigation |
|---|---|
| Baseline widens default sandbox unnecessarily. | Each added path gets a one-line justifying comment. Audit-friendly: a future contributor can see why each path is there and remove unused ones. |
| Path missed in analytical pass. | Strace-in-Podman fallback per Q1's plan (d). |
| Plugin-specific need that shouldn't go in baseline (e.g. `~/.config/anthropic/`). | Covered by `startup_io_paths_for` per-plugin helper in PR 2. Helper takes the plugin's binary name to keep the API simple. |
| Adding `/tmp` to baseline hides real bugs (allows plugins to bypass plan-derived write paths). | Only add `/tmp` if a plugin actually needs it. Default: NOT in baseline. If needed, add as plugin-specific extra, not baseline. |
| macOS sandbox-darwin SBPL baseline has analogous gap. | Out of scope. macOS tests aren't gated on this work. Track as follow-up if symptoms surface. |
| Plugin EOF persists after baseline + per-plugin fixtures (deeper bug). | If PR 1's shell + fs-read fail after the baseline change, escalate to user before continuing — likely indicates a non-IO issue (seccomp, signal handling, etc.). |

## Out of scope

- Production runtime path coverage beyond what the 5 real plugins actually need at startup.
- macOS / Windows baseline parity (separate sub-project if needed).
- Layer 4 plugin-compat container tests (already closed by ADR-0021).
- Sub-project E (per-command exec gating) — independent.
- A general "what does each Rust crate need at runtime" catalog — this work catalogs only what these 5 plugins need.
- Driver-level changes beyond the per-plugin fixture helper.
