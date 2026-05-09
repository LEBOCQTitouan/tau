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

## Investigation findings

### PR 1 — shell + fs-read (2026-05-09)

**Investigator:** subagent (T1 implementer).

**Environment:** lefthook Podman gate (`docker.io/library/rust:1.82-bookworm`,
explicit `--arch arm64` on darwin-arm64 host so the multiarch image picks
the matching architecture). Test invocation:
`cargo test -p tau-plugin-compat --test layer4_native --features
integration-tests -- --ignored --nocapture
shell_layer4_native_runs_echo_hello fs_read_layer4_native_reads_data_file`.

**EOF symptom (shell), baseline (no edits):**

```
spawn shell-plugin under native adapter failed:
LoadFailed("PluginHandshakeFailed { plugin: \"shell-plugin\",
reason: Malformed { detail: \"EOF before handshake response\" } }")
```

**EOF symptom (fs-read), baseline (no edits):**

```
spawn fs-read-plugin under native adapter failed:
LoadFailed("PluginHandshakeFailed { plugin: \"fs-read-plugin\",
reason: Malformed { detail: \"EOF before handshake response\" } }")
```

#### Discovery: the EOF has TWO independent root causes, not one

The spec assumed startup-IO (landlock fs.read) was the sole cause. The
investigation surfaced a second, independent cause: the seccomp baseline
is missing `sched_getaffinity` (and `sched_setaffinity`), which Tokio's
multi-thread runtime calls during `Builder::new_multi_thread().build()`
to size the worker pool. With `KillProcess` as the seccomp mismatch
action, this kills the child with SIGSYS *before* it can write its
handshake response — producing exactly the same "EOF before handshake"
symptom as a landlock denial.

A diagnostic test (one-shot harness wrapping `shell-plugin` with
`NativeSandbox::new(_, Strict).wrap_spawn` and capturing exit status +
signal) revealed:

```
status: ExitStatus(unix_wait_status(159))
signal: 31 (SIGSYS (seccomp KillProcess))
stdout (0 bytes):
stderr (0 bytes):
```

`unix_wait_status(159) = 128 | 31` — SIGSYS, with no stderr because the
plugin never reached its first eprintln. strace under `seccomp=unconfined`
showed the last syscall before the kill was `sched_getaffinity(143, 32,
[...])` — confirming the gap.

**Discovered baseline paths (universal — go into
`BASELINE_SYSTEM_READ_PATHS` in T2):**

| Path | Why | Evidence |
|---|---|---|
| `/proc/self` | tokio runtime introspection — opens `/proc/self/cgroup` and `/proc/self/maps` during init (cgroup v2 quota detection, libstd backtrace setup) | strace: `openat(... "/proc/self/cgroup" ...)`, `openat(... "/proc/self/maps" ...)` |
| `/sys/fs/cgroup` | tokio reads `/sys/fs/cgroup/cpu.max` to compute available CPU quota for worker count (cgroup v2 path; on cgroup v1 the equivalent file lives elsewhere under `/sys/fs/cgroup`) | strace: `openat(... "/sys/fs/cgroup/cpu.max" ...)` |

Other candidates suggested in the prompt (`/proc/sys/kernel`,
`/sys/devices/system/cpu`, `/dev/urandom`, `/dev/null`) were NOT observed
in the strace and NOT needed for the two plugins to handshake. They are
omitted to keep the baseline minimal.

**Plugin-specific paths (go into `startup_io_paths_for` in T3):**

- shell-plugin: none — runtime baseline sufficient.
- fs-read-plugin: none — runtime baseline sufficient.

Both plugins exhibited identical strace output during startup IO. They
share the `tau-plugin-sdk` startup machinery (tokio multi-thread runtime
+ stdio handshake loop) and have no plugin-specific files of their own.

**Out-of-scope finding (seccomp gap — flag for spec authors):**

The seccomp baseline in `crates/tau-sandbox-native/src/strict.rs`'s
`baseline_syscall_map()` is missing two syscalls that Tokio's
multi-thread runtime requires unconditionally:

- `libc::SYS_sched_getaffinity` — to query CPU mask for worker sizing.
- `libc::SYS_sched_setaffinity` — used by the runtime when pinning a
  thread to a CPU subset (added defensively; observed needed only for
  `sched_getaffinity` in this investigation, but `setaffinity` is the
  natural pair and avoids a future SIGSYS for unrelated callers).

Adding these to the baseline is **outside T1 (investigation only)** and
strictly speaking outside the spec's stated scope ("startup IO catalog"
≈ landlock paths). However, **the un-`#[ignore]` step (T4) cannot
succeed without this seccomp change** — adding the two paths from the
table above is necessary but not sufficient. Recommend either:

1. Folding the seccomp baseline extension into T2 (rename the section
   to "baseline filesystem + syscall extensions") and adding a
   regression test alongside the path tests, OR
2. Splitting it into a tiny independent task that lands before T4.

**Outcome:**

After applying both fixes locally (NOT committed, reverted before this
findings commit):

- `fs_read_layer4_native_reads_data_file`: **PASS**.
- `shell_layer4_native_runs_echo_hello`: **handshake succeeds**, but
  test fails *later* during invoke with
  `Internal { message: "plugin error code -32603 message tool.invoke
  failed: internal: shell: spawn failed: Permission denied (os error
  13)" }`. This is shell-plugin's own `Command::new("echo").spawn()`
  hitting EACCES under landlock — i.e. an exec-gating issue (sub-project
  E territory: per-command exec landlock paths), NOT a startup-IO issue.
  Likely cause: PATH-search inside the sandboxed plugin walks
  `/usr/local/sbin`, `/usr/local/bin` (which landlock has not granted
  Execute) before reaching the exec-allow-listed `/usr/bin/echo`, and
  the first EACCES aborts the search in `Command::spawn`. This is
  outside the T1 charter and outside the startup-IO scope.

So: the documented startup-IO baseline is **sufficient and verified**
for both plugins to complete the plugin-handshake handshake. Whether
`shell_layer4_native_runs_echo_hello` ends up green at the end of PR 1
depends on whether T4's "un-`#[ignore]`" step also addresses the
exec-gating PATH-search issue, which the spec does not currently cover.
fs-read passes end-to-end.

**Notes / caveats:**

- strace was performed on the plugin binary running unsandboxed
  (stdin closed) to enumerate startup-time openat targets; this is a
  superset of what would be observed during a real handshake (which
  drives further IO post-handshake). For startup-IO purposes (everything
  before the plugin writes its handshake response on stdout) the trace
  is complete: the plugin reads its libs, initializes tokio (touching
  `/proc/self/cgroup`, `/proc/self/maps`, `/sys/fs/cgroup/cpu.max`),
  then blocks on stdin reading the request frame. No further IO occurs
  before the response.
- `/etc/ld.so.cache` is read at startup but is already covered by the
  existing `/etc` entry in `BASELINE_SYSTEM_READ_PATHS`. Likewise the
  shared libraries (`/lib/aarch64-linux-gnu/libc.so.6` etc) are covered
  by `/lib`.
- The investigation arch is aarch64 (Apple Silicon host via Podman).
  The path set is arch-independent (`/proc/self`, `/sys/fs/cgroup`)
  but worth a sanity-check on x86_64 CI.
- The seccomp gap was masked by Podman's `--security-opt
  seccomp=unconfined` at the *outer* container level — but the
  *in-process* seccomp filter installed by `apply_strict` still applies,
  which is what surfaced the SIGSYS. This means CI on bare Linux runners
  would have hit the same gap.

---

# Phase 0 — LLM-backend spawn fix (added 2026-05-09 evening)

> **Status:** spec amendment, post-T7 brainstorm. Cut from main at `f9c2822` (PR #48 merge). Phase 0 ships before original T7 (renumbered to T7' below) can run.

## Why this section exists

T7 (HTTP plugin startup-IO investigation) was supposed to be a clean parallel of T1 (shell + fs-read investigation): run the test, capture EOF symptom at handshake, identify missing paths, populate the helper. Reality:

**All 3 LlmBackend HTTP plugins fail at SPAWN, not at handshake.**

```
thread 'anthropic_layer4_native_completes_via_cassette' panicked at
crates/tau-plugin-compat/tests/layer4_native.rs:514:13:
  spawn anthropic-plugin under native adapter failed:
  LoadFailed("PluginSpawnFailed { plugin: \"anthropic-plugin\",
              source: Os { code: 2, kind: NotFound,
                           message: \"No such file or directory\" } }")
```

Failure shape: `Os { code: 2 }` (ENOENT) within 4 ms. All 3 HTTP plugins reproduce identically inside the lefthook Podman gate. By contrast, the analogous Tool-port plugins (shell + fs-read) **succeed** in `spawn_tool_under_sandbox` — same Podman environment, same baseline, same driver crate, same plugin binary on disk.

Hypothesis (to be verified in T0a investigation): there is a divergence between `tau_runtime::plugin_host::load_tool` and `tau_runtime::plugin_host::load_llm_backend` — a stricter binary check, an extra pre_exec step, or a path normalization difference — that produces ENOENT only on the LlmBackend port path. The bug is **not** plugin-specific (all 3 LlmBackend plugins fail uniformly) and **not** sandbox-baseline-related (Tool plugins work with the same baseline).

This blocks T7 entirely: we can't investigate startup-IO surface for plugins that never reach handshake. Phase 0 is therefore a prerequisite, not part of T7.

## Locked decisions (Phase 0)

| # | Decision |
|---|---|
| 6 | Phase 0 lands as its own PR (`feat/layer4-llm-spawn-fix`), cut from main at `f9c2822`. Merges before original T7 runs. |
| 7 | Phase 0's investigation step (renumbered T0a) emits findings as a spec amendment commit only — no code commit. Mirrors T1 / T7 pattern. |
| 8 | Phase 0's fix step (T0b) ships the minimal change to make `load_llm_backend` reach the spawn-and-handshake stage. Verification: 3 HTTP tests now fail at handshake EOF (the original PR 2 startup-IO blocker), not at spawn ENOENT. |
| 9 | Phase 0 does **not** un-`#[ignore]` the 3 HTTP tests. They stay ignored after Phase 0 with updated messages: "EOFs at handshake under strict tier; awaits startup-IO investigation per T7'." |
| 10 | The `scripts/agent-push.sh` helper + `CLAUDE.md` "AGENT PUSH RULES" section (drafted in stash during the silent-kill diagnostic that enabled this Phase 0) folds into the same Phase 0 PR. The narrative connection: silent-kill diagnostic enabled Phase 0 investigation; future agents need the helper to repeat that path safely. |

## Components (Phase 0)

**MODIFIED**

- `crates/tau-runtime/src/plugin_host/...` — minimal fix to make `load_llm_backend` reach spawn-and-handshake (specific files determined by T0a investigation; expected scope: 1–2 files).
- `crates/tau-plugin-compat/tests/layer4_native.rs` — update the `#[ignore]` messages on the 3 HTTP tests to reflect the post-fix failure shape (handshake EOF, not spawn ENOENT). Tests stay `#[ignore]`'d.
- `CLAUDE.md` — append "AGENT PUSH RULES" section documenting the silent-kill workaround (drafted in stash from the 2026-05-09 evening session).
- `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md` — this Phase 0 section + populated investigation findings (T0a output).

**NEW**

- `scripts/agent-push.sh` — runs `lefthook run pre-push` as a standalone command (which does NOT die from runtime signal-propagation), then `git push --no-verify`. Drafted in stash.

## Renumbered phasing

The original 11-task plan stays — only T7 gets a "T7'" alias to mark it as post-Phase-0:

| Original task | New label | Status |
|---|---|---|
| (none — new) | **T0a** Investigation: diff `load_tool` vs `load_llm_backend`, identify ENOENT root cause | new, blocking |
| (none — new) | **T0b** Apply spawn fix; verify 3 HTTP tests reach handshake-EOF stage | new, blocking |
| (none — new) | **T0c** Add `scripts/agent-push.sh` + CLAUDE.md AGENT PUSH RULES section | new, in same PR |
| (none — new) | **T0d** USER GATE — open Phase 0 PR, monitor CI | new |
| (none — new) | **T0e** USER GATE — squash-merge Phase 0 PR | new |
| T7 | **T7'** HTTP startup-IO investigation (unchanged scope; unblocked after T0e) | unchanged |
| T8 | T8 (unchanged) | unchanged |
| T9 | T9 (unchanged) | unchanged |
| T10 | T10 (unchanged) | unchanged |
| T11 | T11 (unchanged) | unchanged |

## Architecture (Phase 0)

```
Phase 0 PR: feat/layer4-llm-spawn-fix (cut from main @ f9c2822)
─ T0a: investigation (spec edit only, no code commit)
   ├─ read load_tool side-by-side with load_llm_backend
   ├─ reproduce in lefthook Podman gate
   └─ append findings to this spec
─ T0b: fix (single commit)
   ├─ apply minimal change to plugin_host
   ├─ verify: 3 HTTP tests fail at handshake-EOF, not spawn-ENOENT
   └─ update layer4_native.rs #[ignore] messages
─ T0c: agent infra (single commit)
   ├─ scripts/agent-push.sh (executable)
   └─ CLAUDE.md AGENT PUSH RULES section
─ T0d: USER GATE — push, monitor CI
─ T0e: USER GATE — squash-merge

Then PR 2 work (re-cut from post-Phase-0 main):
─ T7' through T11 (original scope; rescoped only by Phase 0's outcome)
```

## Verification (Phase 0)

**T0a (investigation):**
- Findings section in this spec populated with: exact diff observed between `load_tool` and `load_llm_backend`, hypothesis for the ENOENT, reproduction recipe.
- No code committed.

**T0b (fix):**
- `cargo nextest run -p tau-runtime --lib` continues to pass (no regression in existing runtime unit tests).
- Inside the lefthook Podman gate: the 3 HTTP tests run with `--include-ignored`, fail at the handshake-EOF stage (not at spawn-ENOENT). Failure shape now matches what PR 1's pre-baseline state looked like for fs-read.
- `cargo nextest run -p tau-plugin-compat --test layer4_native fs_read_layer4_native_reads_data_file` continues to pass (PR 1's win not regressed).

**T0c (agent infra):**
- `scripts/agent-push.sh` is executable (`chmod +x`).
- A no-op commit successfully pushed via `scripts/agent-push.sh` (validates the helper end-to-end).
- `CLAUDE.md` AGENT PUSH RULES section renders correctly.

**Branch protection:** No new CI jobs. Existing matrix validates the Phase 0 fix automatically.

## Risks & mitigations (Phase 0)

| Risk | Mitigation |
|---|---|
| Spawn fix is invasive (e.g., requires a `tau-runtime` API change that breaks plugin_host callers). | T0a investigation writes findings BEFORE T0b's fix. If scope balloons, pause and reassess (escalate to user). |
| T0b fix lands but T7' reveals more issues beyond startup-IO baseline. | Out of scope; tracked as follow-up to T7'. |
| Folding agent-push.sh into Phase 0 muddies the diff. | Acceptable: the connection (silent-kill diagnostic enabled Phase 0 investigation) is documented in the spec. Reviewer reads this section first. |
| `feat/layer4-startup-io-http` branch (currently empty) needs re-cut from post-Phase-0 main. | Cheap: the branch has no commits yet. After Phase 0 merges, `git branch -D feat/layer4-startup-io-http && git checkout main && git pull && git checkout -b feat/layer4-startup-io-http`. |

## Out of scope (Phase 0)

- The actual startup-IO investigation (that's T7' — unblocked by Phase 0).
- Populating `startup_io_paths_for` HTTP plugin arms (T8).
- Un-`#[ignore]`'ing the 3 HTTP tests (T9 — they stay ignored after Phase 0; just with updated messages).
- macOS sandbox-darwin equivalent gap (separate sub-project if symptoms surface).
- Sub-project E (per-command exec gating; closes shell test). Independent.

## Investigation findings (Phase 0)

### T0a — load_tool vs load_llm_backend diff (2026-05-09)

**Investigator:** subagent (T0a implementer, Claude Opus 4.7 1M context).

**Environment:** lefthook Podman gate (`docker.io/library/rust:1.82-bookworm`, aarch64) on darwin-arm64 host (Apple Silicon). Tests run with `CARGO_TARGET_DIR=/workspace/target/lefthook-podman` and `CARGO_INCREMENTAL=0`.

**Reproduction:**

```bash
podman run --rm \
  --cap-add SYS_ADMIN --cap-add NET_ADMIN \
  --security-opt seccomp=unconfined \
  --security-opt apparmor=unconfined \
  --security-opt label=disable \
  -v "$PWD:/workspace" \
  -v cargo-cache:/usr/local/cargo/registry \
  -v target-cache:/workspace/target/lefthook-podman \
  -w /workspace \
  -e CARGO_INCREMENTAL=0 \
  -e CARGO_TARGET_DIR=/workspace/target/lefthook-podman \
  docker.io/library/rust:1.82-bookworm \
  bash -c '
ARCH=$(uname -m)
case "$ARCH" in
  aarch64) NEXTEST_URL="https://get.nexte.st/latest/linux-arm" ;;
  *)       NEXTEST_URL="https://get.nexte.st/latest/linux" ;;
esac
rm -f /usr/local/cargo/bin/cargo-nextest
curl -LsSf "$NEXTEST_URL" | tar zxf - -C /usr/local/cargo/bin
mkdir -p target/release
cp -f target/lefthook-podman/release/anthropic-plugin target/release/
timeout 120 cargo nextest run -p tau-plugin-compat --test layer4_native \
  anthropic_layer4_native_completes_via_cassette \
  --features integration-tests --no-capture -- --include-ignored
'
```

Failure (within 4 ms, before handshake):

```
spawn anthropic-plugin under native adapter failed:
  LoadFailed("PluginSpawnFailed { plugin: \"anthropic-plugin\", source:
  Os { code: 2, kind: NotFound, message: \"No such file or directory\" } }")
```

The control test (`fs_read_layer4_native_reads_data_file`) passes under the same Podman invocation. Both binaries exist on disk at the same `target/release/<bin>` location.

**Diff observed:**

The two `load_*` functions in `crates/tau-runtime/src/plugin_host/mod.rs` are structurally identical at the spawn site — the only differences are:

- `PortKind::Tool` vs `PortKind::LlmBackend` (handshake assertion only — runs *after* spawn).
- Required-methods array (`["tool.call"]` vs `["llm.complete"]` — handshake-time only).
- `load_tool` performs an extra `IpcTool::fetch_schema` RPC after handshake (post-spawn, irrelevant to ENOENT).

`PluginProcess::spawn_and_handshake` (`crates/tau-runtime/src/plugin_host/process.rs:168-258`) does **not** branch on `PortKind`. The spawn path is byte-for-byte identical for both ports — same `Command::new(binary_path)`, same `env_clear`/PATH inheritance, same `validate_plan_against_adapter` → `adapter.wrap_spawn` → `command.spawn()` sequence. Test helpers `make_locked_plugin` and `make_llm_locked_plugin` (`crates/tau-plugin-compat/tests/layer4_native.rs:98-121`) differ only in `PortKind`; `binary_path` is constructed identically.

The actual divergence is **not in `tau-runtime`** — it's in the SandboxPlan capabilities the tests pass:

- fs-read test → `SandboxPlan` with `Filesystem(Read)` only.
- Anthropic / ollama / openai tests → `SandboxPlan` with `Network(Http)` (via `make_net_http_localhost_cap`).

When the plan contains `Network(Http)`, `tau_sandbox_native::strict::wrap_spawn` (`crates/tau-sandbox-native/src/strict.rs:410-423`) **rewrites the `Command`** to spawn `tau-net-bridge` instead of the plugin binary directly:

```rust
let bridge_path = std::env::var_os("TAU_NET_BRIDGE_PATH")
    .unwrap_or_else(|| std::ffi::OsString::from("tau-net-bridge"));
*cmd = std::process::Command::new(bridge_path);
cmd.arg(format!("--proxy-sock={}", proxy_sock_path.display()))
    .arg("--listen=127.0.0.1:8443")
    .arg("--")
    .arg(&original_program)
    .args(&original_args);
```

When `TAU_NET_BRIDGE_PATH` is unset, the bridge name falls back to a bare `"tau-net-bridge"`, which `Command::spawn` resolves via `execvp`/PATH. Diagnostic `eprintln` output confirms this exactly:

```
PHASE0_DEBUG[3] after adapter.wrap_spawn — plugin=anthropic-plugin program="tau-net-bridge"
PHASE0_DEBUG[4] before command.spawn — program="tau-net-bridge" args=[
  "--proxy-sock=/tmp/tau-proxy-157-0.sock", "--listen=127.0.0.1:8443",
  "--", "/workspace/target/lefthook-podman/release/anthropic-plugin"]
```

For fs-read (no `Network` capability), `wrap_spawn` does *not* rewrite the program; the diagnostic shows `program="/workspace/target/lefthook-podman/release/fs-read-plugin"` (absolute path) and the test passes.

**Root cause:**

The ENOENT is from `tokio::process::Command::spawn()` / `execvp("tau-net-bridge", ...)` — `tau-net-bridge` is not on `PATH` inside the Podman gate (or the production runtime), and `tau-plugin-compat::tests::layer4_native` does not export `TAU_NET_BRIDGE_PATH`. The `tau-runtime` and `tau-sandbox-native` unit tests work because they live in the `tau-sandbox-native` crate and Cargo automatically populates `CARGO_BIN_EXE_tau-net-bridge` for them (see `crates/tau-sandbox-native/tests/strict_proxy.rs:69`), but that env var is **not** auto-populated for downstream test crates like `tau-plugin-compat` that don't own the bin target.

The ignore-message hypothesis ("HTTP client init touches state outside plan's read paths" → handshake EOF) was **wrong**. The plugins never actually start — the spawn fails before the handshake even begins. After applying the fix locally (export `TAU_NET_BRIDGE_PATH` to a built bridge binary), spawn succeeds and the failure mode shifts to a downstream `expect("stdin piped via stdin(Stdio::piped())")` panic at `process.rs:283-287`, which is consistent with the bridge process exiting before stdin/stdout could be plumbed (likely because `tau-net-bridge` itself wants `iproute2`/`nftables`/proxy-socket setup that isn't running in the bare integration test). That downstream failure is T0b/T7 territory — the spawn-ENOENT root cause is fully isolated.

**Fix scope:**

Test-infrastructure-only. No `tau-runtime` or `tau-sandbox-native` source changes needed. Two correct options for T0b (pick one):

1. **(Preferred — minimal, ~5 LOC.)** In `crates/tau-plugin-compat/tests/layer4_native.rs`, add a helper that locates the built `tau-net-bridge` binary (mirroring `locate_plugin_bin`) and unconditionally exports `TAU_NET_BRIDGE_PATH` early in each LlmBackend test (or via a `ctor`/`once_cell`). Update CI / pre-push gate to also build `-p tau-sandbox-native --bin tau-net-bridge --release` alongside the plugin builds.

2. **(Alternative.)** Make `wrap_spawn` panic-with-clear-message when the bridge is unresolvable — keep behavior the same on the test caller, but produce a better error than `Os { code: 2 }`. Strictly worse than option 1 because it doesn't actually unblock the 3 ignored tests.

T0b should also build `tau-net-bridge --release` in the lefthook-pre-push container (or accept that the 3 HTTP tests skip when the bridge isn't built, gated like `resolve_native_or_skip`).

**Outcome (with proposed fix applied locally — NOT committed; T0b's job):**

Verified by re-running the same Podman invocation with `TAU_NET_BRIDGE_PATH=/workspace/target/lefthook-podman/release/tau-net-bridge` exported and `cargo build --release -p tau-sandbox-native --bin tau-net-bridge` first:

- ✅ Spawn-ENOENT eliminated. `Command::spawn` now resolves to the absolute bridge path.
- ⚠️ The 3 HTTP tests now fail later in `PluginProcess::spawn_and_handshake` (panic on `child.stdin.take().expect(...)` at `process.rs:283-287` — bridge child seemingly exits before its pipes are read). This is a *different* failure mode from spawn-ENOENT and is in handshake-startup-IO territory. Whether T0b should land just the test-env fix and update the 3 `#[ignore]` reasons to reflect this new mode (deferring real fix to T7-followup), or chase the bridge-startup issue too, is a scoping call for the spawn-fix task.
- ✅ fs-read test (`fs_read_layer4_native_reads_data_file`) continues to pass — confirming the fix is non-regressive.
- ✅ No `tau-runtime` source changes were made; existing tau-runtime lib tests are untouched.
