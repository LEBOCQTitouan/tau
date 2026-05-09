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
