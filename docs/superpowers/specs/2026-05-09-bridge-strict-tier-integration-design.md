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

To be populated by T0a. Template:

```markdown
### T0a — bridge syscall + path enumeration (DATE)

**Investigator:** [agent-id or human].

**Environment:** lefthook Podman gate (`docker.io/library/rust:1.82-bookworm`) on darwin-arm64 host.

**Analytical candidate set (from reading tau-net-bridge source):**
[bullet list of syscalls + paths derived from reading the bridge code]

**Strace-confirmed denials (from running bridge under candidate strict-tier filter):**
[bullet list of EACCES + EPERM + SIGSYS denials observed]

**Final additional syscall set (goes into `extend_with_network_rules`):**
[final list, with justification per syscall]

**Final additional path set (goes into `BASELINE_SYSTEM_READ_PATHS` or per-plan):**
[final list, with justification per path; note universal vs bridge-specific]

**Outcome:**
[with the proposed extensions applied locally: bridge launches under strict tier, listens on 127.0.0.1:8443, accepts a connection, exits cleanly when child closes. fs-read still passes; existing tau-sandbox-native lib + integration tests still pass.]

**Surprises / caveats:**
[anything noteworthy — e.g. netlink turned out to be unnecessary because tokio doesn't bring loopback up; or a path showed up in strace but the bridge worked without it]
```
