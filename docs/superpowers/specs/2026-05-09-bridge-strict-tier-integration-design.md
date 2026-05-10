# Bridge ↔ strict-tier integration completion — design

> **Status:** spec, executing inline. Cuts from main at `91d827c` (PR #50 — T7 findings docs).

## Goal

Make `tau-net-bridge` actually work end-to-end under real strict-tier sandboxing (landlock + seccomp + empty netns). ADR-0020 shipped the proxy + bridge architecture but its strict-tier integration was never tested with a real bridge process running under the full filter — `strict_proxy.rs` tests cover the seccomp-denial path only.

T7 (PR #50 spec edit) verified that `extend_with_network_rules` allows only client-side syscalls, so the bridge — which is server-side (`bind` + `listen` + `accept` on `127.0.0.1:8443`) and inherits the seccomp filter via `execve` — gets SIGSYS-killed before it can plumb stdio. Adding `bind/listen/accept/accept4` is necessary but not sufficient (verified by local edit + Podman re-run). At least one more layer (likely netlink syscalls for `ip link set lo up` inside the empty netns) is missing. This sub-project enumerates the full set, ships the fix, and adds a regression test that catches future drift.

Closes the bridge prerequisite for the 3 `#[ignore]`'d HTTP layer4 tests in `tau-plugin-compat` (un-`#[ignore]`'ing them is a separate next-day chore after this lands; they may need additional plugin-specific path work depending on what surfaces).

## Locked decisions

| # | Decision |
|---|---|
| 1 | **Hybrid investigation method.** Read `tau-net-bridge` source first (~200 LOC), derive a candidate syscall + path set analytically. Then strace inside the lefthook Podman gate to verify and close gaps. Iteration converges fast because the bridge code surface is small. |
| 2 | **Extensions land in existing functions.** New seccomp syscalls go into `tau-sandbox-native::net::extend_with_network_rules` (already gated by `Network(Http)` capability); new path entries go into `tau-sandbox-native::light::BASELINE_SYSTEM_READ_PATHS` only if every Rust binary needs them, otherwise per-plan. No new public API. |
| 3 | **End-to-end test in `tau-sandbox-native`** as a new file `tests/strict_bridge.rs`, mirroring `strict_proxy.rs` patterns. Spawns a real `tau-net-bridge` under the full strict-tier filter with `/bin/cat` as the stub child; asserts the bridge reaches `accept` without SIGSYS and tears down cleanly. Linux-only. Picked up by the existing `test (tau-sandbox-native e2e / linux)` CI job — no new branch protection required. |
| 4 | **One PR closes Phase 0** (`feat/bridge-strict-tier-integration`). Investigation findings + seccomp/landlock fix + new integration test. Does NOT un-`#[ignore]` the 3 HTTP layer4 tests — they may still need plugin-specific paths beyond what this sub-project covers; a 5-minute next-day chore re-checks them and ships their un-ignore as a tiny follow-up. |
| 5 | **Constitution G12 narrowness.** New seccomp entries go in the HTTP-cap-gated branch (only when `Network(Http)` is in the plan). Universal-need paths only get added to baseline if EVERY Rust binary needs them; bridge-specific paths stay in the plan/scope. Each new path entry has a one-line justifying comment. |

## Components

**MODIFIED**

- `crates/tau-sandbox-native/src/net.rs` — extend `extend_with_network_rules` with the additional syscalls discovered in investigation. Verified-needed (per T7): `bind`, `listen`, `accept`, `accept4`. Likely-needed (per analytical pass on bridge source): netlink syscalls (`socket(AF_NETLINK)`, `sendto`/`recvfrom` on netlink — these may already be allowed if the baseline includes them; verify), `ioctl` for `SIOCSIFFLAGS`. Update the doc comment + module-level `//!` block to reflect bridge-aware rules ("client-side OR bridge server-side"). Existing 7 unit tests stay; add 2-3 new tests asserting the new entries are present when `Network(Http)` is.
- `crates/tau-sandbox-native/src/light.rs` — extend `BASELINE_SYSTEM_READ_PATHS` if strace surfaces additional bridge-needed paths (e.g. `/proc/sys/net/ipv4/tcp_*` for tokio runtime tuning). Per-path one-line justifying comment per Constitution G12. Existing 3 regression tests stay; extend `baseline_system_read_paths_includes_runtime_mechanics` to include any new entries.
- `docs/superpowers/specs/2026-05-09-bridge-strict-tier-integration-design.md` (this file) — append "Investigation findings" section at the bottom with concrete data once T0a runs.

**NEW**

- `crates/tau-sandbox-native/tests/strict_bridge.rs` — Linux-only e2e integration test. Single test function `bridge_reaches_listen_under_strict_tier()` that:
  1. Builds a strict-tier `SandboxPlan` with `Network(Http)` capability for `127.0.0.1`.
  2. Resolves the bridge binary via `env!("CARGO_BIN_EXE_tau-net-bridge")`.
  3. Spawns a host-side proxy task on a temp Unix socket (mirrors `strict_proxy.rs`).
  4. Calls `wrap_spawn(&plan, &mut cmd)` where `cmd` is `Command::new(/bin/cat)` with stdin piped.
  5. Spawns the wrapped command; expects bridge to start successfully (no SIGSYS within 1 sec).
  6. Asserts a TCP `connect("127.0.0.1:8443")` from a separate thread succeeds (bridge accepted).
  7. Closes the child's stdin to trigger clean teardown; expects bridge to exit cleanly.
  Bounded by `tokio::time::timeout` (10s total). `#[cfg(target_os = "linux")]` gated; uses the existing `integration-tests` Cargo feature for proper enable/disable.

## Architecture

```
Phase 0 PR: feat/bridge-strict-tier-integration
─ T0a: investigation (spec edit only, no code commit)
   ├─ read tau-net-bridge source (~200 LOC)
   ├─ derive candidate syscall + path set
   ├─ strace inside Podman gate to verify
   └─ append findings to this spec
─ T0b: code fix (single commit)
   ├─ extend extend_with_network_rules with new syscalls
   ├─ extend BASELINE_SYSTEM_READ_PATHS if needed
   └─ update doc comments + add unit tests
─ T0c: e2e test (single commit)
   ├─ create strict_bridge.rs integration test
   └─ verify it passes inside lefthook Podman gate
─ T0d: USER GATE — push, monitor CI
─ T0e: USER GATE — squash-merge

Then a tiny follow-up (NOT this PR):
─ Re-check the 3 HTTP layer4 tests after this merges
─ If they pass: un-#[ignore] them in a 1-commit PR
─ If they still fail: open another sub-project for plugin-specific paths
```

## Verification

**T0a (investigation):**
- "Investigation findings" section in this spec populated with concrete data: candidate set from analytical pass, strace-confirmed denials, full additional syscall + path list.
- No code committed.

**T0b (code fix):**
- `cargo nextest run -p tau-sandbox-native --lib` continues to pass (no regression in existing seccomp/landlock unit tests). New unit tests pass.
- Clippy + fmt clean for `tau-sandbox-native`.

**T0c (e2e test):**
- `cargo nextest run -p tau-sandbox-native --features integration-tests --tests strict_bridge` passes inside the lefthook Podman gate.
- Existing `strict_proxy.rs` tests continue to pass (no regression in the seccomp-denial path).

**T0d (USER GATE — full PR verification):**
- Lefthook gate green (push via `scripts/agent-push.sh` for the silent-kill workaround).
- CI green on the 14 required checks (especially `test (tau-sandbox-native e2e / linux)`).
- The 3 HTTP layer4 tests in `tau-plugin-compat` STILL fail when run with `--include-ignored` (they remain `#[ignore]`'d), but the failure shape should move past the bridge SIGSYS — should now be either passing OR plugin-specific TLS issues. This is informational, not a gate.
- `fs_read_layer4_native_reads_data_file` continues to pass (PR 1 win not regressed).

**Branch protection:** No new CI jobs.

## Risks & mitigations

| Risk | Mitigation |
|---|---|
| Bridge needs syscalls that Podman gate's outer seccomp filter blocks (e.g. specific netlink subset). | Investigation runs inside the gate. If a syscall is OS-blocked rather than tau-filter-blocked, escalate to user — may need a different bridge approach. |
| Netlink syscall numbers vary by arch. | Use `libc::SYS_*` constants when available; raw-number `#[cfg(target_arch)]` blocks for newer syscalls (existing pattern in `strict.rs`). |
| `ioctl(SIOCSIFFLAGS)` to bring loopback up needs `CAP_NET_ADMIN` inside the netns AND the seccomp allow. Bridge unshares with `CLONE_NEWUSER` so it should have the cap, but verify. | Investigation verifies. If capability-related (not syscall-related), document as a separate gap; may need unshare/setup-path code change. |
| New `strict_bridge.rs` test flakes from socket timing. | `tempfile` for proxy socket path; `tokio::time::timeout(10s)` overall bound; mirror the patterns from the working `strict_proxy.rs`. |
| Strace misses conditional paths only triggered by specific traffic. | Strace covers the startup path; runtime traffic-handling is exercised by the actual layer4 HTTP tests as a downstream safety net. |
| Investigation reveals the fix is invasive (new public API, multiple-crate change). | T0a HARD GATE: if scope balloons, escalate to user before T0b commits any code. |

## Out of scope

- Plugin-specific `startup_io_paths_for` HTTP arms (T8 of the original layer4-startup-io plan; defer to a follow-up after this lands and we re-check the 3 HTTP layer4 tests).
- Un-`#[ignore]`'ing the 3 HTTP layer4 tests in `tau-plugin-compat`.
- Sub-project E (per-command exec gating; closes shell layer4 test). Independent.
- macOS sandbox-darwin SBPL bridge equivalent (different proxy mechanics on darwin; ADR-0022 doesn't currently use a bridge, but if it adds one this design becomes a template).

## Investigation findings

### T0a — bridge syscall + path enumeration (2026-05-10)

**Investigator:** subagent (Claude Opus 4.7, T0a implementer).

**Environment:** lefthook Podman gate (`docker.io/library/rust:1.82-bookworm`)
on darwin-arm64 host. Container launched with `--cap-add SYS_ADMIN
--cap-add NET_ADMIN --security-opt seccomp=unconfined --security-opt
apparmor=unconfined --security-opt label=disable`, mounting
`cargo-cache` + `target-cache` named volumes, with
`CARGO_INCREMENTAL=0` and
`CARGO_TARGET_DIR=/workspace/target/lefthook-podman`. nextest
installed via the arch-detected `https://get.nexte.st/latest/linux-arm`
binary (lefthook.yml pattern).

**Analytical candidate set (from reading
`crates/tau-sandbox-native/src/bin/tau-net-bridge.rs`, 245 LOC):**

The bridge does:
1. `bring_lo_up()` — opens an AF_NETLINK socket via `rtnetlink::new_connection`
   to bring `lo` up. Tokio runtime spawns a tiny event loop.
2. `TcpListener::bind 127.0.0.1:8443` then `set_nonblocking(false)`.
3. `unsafe { libc::fork() }`; child execve's the plugin; parent runs
   the proxy loop.
4. Parent thread accepts TCP connections, dials the proxy via
   `UnixStream::connect`, and `std::io::copy` splices bytes
   bidirectionally between the two endpoints across two threads.
5. Parent waits on the plugin pid via `libc::waitpid` and propagates
   the exit code.

Mapping operations to syscalls and cross-referencing against
`baseline_syscall_map` (`crates/tau-sandbox-native/src/strict.rs`) +
`extend_with_network_rules` (`net.rs`):

- Already in baseline: `read`, `write`, `openat`, `close`, `fstat`,
  `fcntl`, `mmap`, `munmap`, `mprotect`, `madvise`, `brk`,
  `arch_prctl`, `prctl`, `getpid`, `gettid`, `set_tid_address`,
  `set_robust_list`, `rt_sigaction`, `futex`, `epoll_*`, `eventfd2`,
  `nanosleep`, `clock_gettime`, `sched_yield`, `rseq`, `clone`,
  `wait4`, `waitid`, `execve`, `exit`, `exit_group`, `dup`, `dup3`,
  `pipe2`, `socketpair`, `sendmsg`, `recvmsg`, `sendto`, `recvfrom`,
  `setsockopt`, `getsockopt`, `getrandom`.
- Already in `extend_with_network_rules` when `Network(Http)`:
  `socket`, `connect`, `getpeername`, `getsockname`.
- **Candidate additions** (server-side, not yet in any rule set):
  `bind` (TCP listener + netlink bind),
  `listen` (TCP listener),
  `accept` (legacy; not actually used by Rust 1.82 std but cheap to
   include for ABI compat),
  `accept4` (Rust std `TcpListener::incoming`).

No filesystem path additions appeared analytically: the bridge only
opens dynamic libraries (`/etc/ld.so.cache`, `libc.so.6`, etc.) and
`/proc/self/maps`, all of which are inside `BASELINE_SYSTEM_READ_PATHS`
already.

**Strace-confirmed denials (running the bridge bare with
`-e trace=network,openat`):**

The bridge bare (no sandbox) issues exactly the following
network-family syscalls, deduplicated:

```
accept4
bind
listen
recvfrom
sendto
setsockopt
socket
socketpair
```

Of these, `accept4`, `bind`, `listen` are absent from both baseline
and `extend_with_network_rules` and would be killed by seccomp
`KillProcess` under the current filter. `socket`, `setsockopt`,
`sendto`, `recvfrom`, `socketpair` are already covered. `accept`
(legacy) does not appear in the strace because Rust std uses
`accept4`, but it is still added defensively.

The strace also confirmed expected non-denial behaviour:
- The netlink socket is opened with `AF_NETLINK, NETLINK_ROUTE` and
  immediately uses `sendto`/`recvfrom` (not `bind`!). Surprise: the
  rtnetlink crate does NOT bind the netlink socket; it uses
  unconnected datagram I/O. So `bind` is needed only for the TCP
  listener, not for netlink.
- Inside the Container test environment (Podman with CAP_NET_ADMIN
  available), `bring_lo_up()` succeeds. Per the bridge's own comment
  (lines 118-124), it gracefully falls back to "lo already up" if
  CAP_NET_ADMIN is unavailable — so this code path tolerates
  EPERM-on-netlink without failing.
- No filesystem-path EACCES denials surfaced — bridge openat targets
  are all in BASELINE_SYSTEM_READ_PATHS.

**Final additional syscall set (goes into
`extend_with_network_rules`):**

```rust
libc::SYS_bind,     // TCP listener bind to 127.0.0.1:8443
libc::SYS_listen,   // TcpListener after bind
libc::SYS_accept,   // legacy accept; defensive (not used by Rust 1.82
                    // std but small ABI-compat cushion)
libc::SYS_accept4,  // Rust std TcpListener::incoming
```

These are added unconditionally when `Capability::Network(Http)` is
present, because:
- The bridge is the load-bearing path for ALL `Network(Http)` plans
  under the strict tier (`strict::wrap_spawn` always wraps the plugin
  in `tau-net-bridge` once `TAU_NET_BRIDGE_PATH` is set).
- The four syscalls are exclusively used by the bridge process, NOT
  by the plugin process — but seccomp filters apply per-thread and
  the bridge inherits the filter via `execve`. There's no
  cross-process distinction to make.
- Adding them to a non-bridge-using HTTP plan is harmless because
  the netns is empty (no upstream peer to listen for) and the
  per-host nftables filter blocks egress to non-allowlisted IPs;
  `bind`/`listen` on 127.0.0.1 inside an empty netns is not a
  meaningful capability uplift.

The module-level `//!` doc block on `net.rs` (lines 14-22) and the
docstring above `extend_with_network_rules` (lines 53-71) BOTH
currently assert that server-side syscalls are "intentionally
omitted". T0b must update both to reflect the bridge architecture.

**Final additional path set (goes into `BASELINE_SYSTEM_READ_PATHS`
or per-plan):**

Empty. The bridge's openat targets are all already in baseline:
- `/etc/ld.so.cache`
- `/lib/aarch64-linux-gnu/libgcc_s.so.1`
- `/lib/aarch64-linux-gnu/libm.so.6`
- `/lib/aarch64-linux-gnu/libc.so.6`
- `/proc/self/maps`

No bridge-specific path additions are required.

**Outcome:**

With the proposed 4-syscall extension applied locally to
`extend_with_network_rules`, the 3 HTTP layer4 cassette tests in
`crates/tau-plugin-compat/tests/layer4_native.rs` (anthropic, ollama,
openai) progress past the bridge's `bind`/`listen`/`accept4` SIGSYS
and now fail with a strictly downstream error:

```
spawn anthropic-plugin under native adapter failed:
  LoadFailed("PluginHandshakeFailed { plugin: \"anthropic-plugin\",
             reason: Malformed { detail: \"EOF before handshake response\" } }")
```

The `EOF before handshake response` failure mode matches the
`#[ignore]` annotation on the 3 tests at HEAD: "Plugin now reaches
handshake but EOFs there because reqwest TLS init touches paths
beyond BASELINE_SYSTEM_READ_PATHS. Awaits T7' HTTP startup-IO
investigation per docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md."

This confirms scenario 2 from the implementer prompt: bridge is
unblocked, downstream plugin TLS/reqwest startup-IO remains the
gating factor for the 3 HTTP tests. The bridge sub-project T0b
delivers the fix; the test un-ignore is the explicit non-goal of
this sub-project (per spec §0 row 4 and the 5-minute Phase 0
follow-up chore).

**Surprises / caveats:**

1. **Netlink does NOT need `bind`.** The rtnetlink crate uses an
   unconnected AF_NETLINK datagram socket via `sendto`/`recvfrom`.
   This makes the additional syscall set strictly TCP-listener-driven.
   If a future rtnetlink version were to adopt `bind` for netlink,
   it would already be covered by the same `SYS_bind` allow-list.
2. **Rust std uses `accept4`, not `accept`.** Linux's glibc accept
   wrapper invokes `accept4` directly. `SYS_accept` is added
   defensively for older toolchains/ABI compat, but it cannot fire
   in current Rust. Keeping it in the allow-list is zero-cost
   (single i64 entry in a BTreeMap) and reduces fragility.
3. **`bring_lo_up()` is best-effort and that's fine.** Inside the
   Container adapter the netns has lo already up; inside the Native
   adapter we have CAP_NET_ADMIN and the call succeeds. The bind on
   127.0.0.1:8443 is the load-bearing check either way — the bridge
   fails loud-and-clear if loopback isn't usable.
4. **The `//!` module doc and `extend_with_network_rules` docstring
   in `net.rs` both currently say server-side syscalls are
   "intentionally omitted".** T0b must rewrite them — the bridge
   architecture inverts this premise; the bridge is server-side AND
   inherits the seccomp filter.
5. **No path-set additions needed.** Earlier T7 work already brought
   the necessary system libraries into `BASELINE_SYSTEM_READ_PATHS`.
   The bridge is a pure-Rust binary linking against the same glibc
   the plugin uses, so its dynamic linker openats are already
   covered.
