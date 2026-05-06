# Sub-project F task 6.5 — Net-filter integration

> Spec for the strict.rs post-spawn hook integration that wires sub-project F's `apply_per_host_filter` machinery into the strict-tier `wrap_spawn` flow. Closes the "PARTIAL" tag from sub-project F.

## Context

Sub-project F (PARTIAL) shipped at commit `608560b` (PR #35 + #36, merged 2026-05-06):

- `tau-sandbox-native::net_filter` module with `probe`, `validate`, `resolve`, `netns`, `rules`, `handle`, and orchestrator (`apply_per_host_filter`) — ~640 LOC + 26 unit tests + 3 insta snapshots.
- `SandboxError::NetFilter { message: String }` variant in `tau-ports::error`.
- New CI job `test-net-filter / linux` running inside privileged Docker (Phase 0 finding: GHA host runners block `uid_map` writes; privileged Docker has full root + CAP_NET_ADMIN-in-userns).
- Branch protection: 15 required checks (added `test-net-filter / linux`).

What did NOT ship in sub-project F (deferred to this task, F 6.5):

1. Wiring `apply_per_host_filter` into the strict-tier `wrap_spawn` flow. The orchestrator needs the **child PID**, which is only known after `cmd.spawn()`. Today's `Sandbox::wrap_spawn(plan, &mut Command) -> SandboxHandle` returns BEFORE spawn — no path to call the orchestrator afterwards.
2. The sync barrier between parent and child needed to coordinate F setup with the child's `pre_exec` flow (parent must finish veth + nft setup before the child runs `seccomp_apply` or `exec`).
3. The 3 currently-`#[ignore]`'d Layer 4 container × HTTP plugin tests (`anthropic_layer4_container_*`, `ollama_*`, `openai_*` in `crates/tau-plugin-compat/tests/layer4_container.rs`).
4. The 4 `#[ignore]`'d integration test stubs in `crates/tau-sandbox-native/tests/strict_net_filter.rs`.
5. Flipping `unshare_flags_for_plan` from "drop CLONE_NEWNET when Network(Http)" (v0.1 fallback) to "always include CLONE_NEWNET" (now safe with F integration).
6. Removing the module-level `#[allow(dead_code)]` on `net_filter::mod`.

This task closes all six gaps in a single PR plus a small docs follow-up.

## Goals

In priority order:

1. **Wire `apply_per_host_filter` into the strict-tier `wrap_spawn` flow.** Plugins requesting `Network(Http)` get real per-host network filtering at runtime (not v0.1 over-permissive fallback).
2. **Close the spec's hard-refuse promise.** When F prereqs are missing, `validate_plan` rejects plans containing `Network(Http)` with `SandboxError::NetFilter { ... }`.
3. **Run all F's previously-`#[ignore]`'d tests.** 4 stubs + 3 layer4_container tests = 7 tests un-`#[ignore]`'d, all green in privileged Docker CI.
4. **Ship F as fully done.** ROADMAP 12-F drops "PARTIAL"; followups doc gap row "Per-host network filtering is over-permissive" closes.

## Non-goals

- Refactoring the existing landlock/seccomp pre_exec ordering. F 6.5 inserts a sync-pipe step between unshare and seccomp; nothing else moves.
- Trait surgery for non-sandbox concerns. One new method (`apply_post_spawn`) on `Sandbox`. No other public-API changes.
- Performance optimization of veth/nft setup. Each plugin spawn pays the ~50-200ms F setup cost; profile + optimize is a separate sub-project if needed.
- DNS rotation handling (one-shot resolution at spawn per spec Q3.A).
- Container or Mock adapter changes — they use the default no-op `apply_post_spawn`; no behavioral change.

## Locked decisions

| # | Decision | Choice | Rationale |
|---|---|---|---|
| 1 | Architecture | **α-2**: `Sandbox::apply_post_spawn` trait method (default no-op) + `sync_write_fd` field on `SandboxHandle` | Keeps adapter abstraction; trait extension is additive; sync-pipe coordination via the handle that already lives across spawn lifetime |
| 2 | NetFilterHandle ownership | **A**: nested via `SandboxHandle::nest_handle` | Drop ordering is automatic; runtime struct unchanged; type-mediated (no struct-field-order foot-gun) |
| 3 | F setup failure handling | **A**: close write_fd → child sees EOF in pre_exec → child exits with error | Cleanest contract; child cooperates via existing pre_exec read; reaped via standard `cmd.spawn().wait()` |
| 4 | Layer 4 container test scope | **A**: flip all 3 tests; bind cassette server on `0.0.0.0:0`; use `TAU_NET_PARENT_VETH_IP` env var | Matches spec Q4.B2; ~30 LOC test refactor; full end-to-end confidence after merge |

## Constraints

- Linux kernel ≥ 5.13 (existing).
- nftables binary ≥ 0.9, iproute2, nsenter (already documented in F's reference doc).
- CAP_NET_ADMIN must be grantable in unprivileged user namespace (F's existing probe).
- Privileged Docker for CI-side e2e (Phase 0 finding; `test-net-filter / linux` already runs this way).
- BASE_SHA = `608560b` (F PARTIAL merge commit) for "pre-existing failure" verification.

---

## Architecture

### End-to-end flow

```
Plan with Network(Http) ──→ NativeSandbox::validate_plan ──┬─→ probe failed: SandboxError::NetFilter (hard refuse)
                                                            │
                                                            └─→ probe ok: continue
                                                                  ↓
                                                NativeSandbox::wrap_spawn(plan, cmd)
                                                  ↓
                              ┌─── strict.rs::apply_strict ─────────┐
                              │ pre-allocate veth subnet            │
                              │ set TAU_NET_PARENT_VETH_IP on cmd   │
                              │ create pipe2:                       │
                              │   read_fd captured in pre_exec      │
                              │   write_fd → SandboxHandle          │
                              │ pre_exec closure: {                 │
                              │   landlock; unshare;                │
                              │   read(read_fd);  ← blocks          │
                              │   seccomp; exec                     │
                              │ }                                   │
                              │ return SandboxHandle                │
                              │   .with_sync_write_fd(write_fd)     │
                              │   .with_pre_allocated_veth(subnet)  │
                              └─────────────────────────────────────┘
                                                  ↓
                              caller: cmd.spawn() → child PID known
                                                  ↓
                              caller: sandbox.apply_post_spawn(plan, child_pid, &mut handle)
                                                  ↓
                              ┌─── NativeSandbox::apply_post_spawn ──┐
                              │ apply_per_host_filter:               │
                              │   validate hosts                     │
                              │   resolve hostnames (DNS)            │
                              │   setup_veth_pair_with_subnet        │
                              │   move_peer_to_netns(pid)            │
                              │   nsenter+ip configure child netns   │
                              │   nsenter+nft -f -                   │
                              │ on success: handle.nest_handle(nf)  │
                              │ on failure: return Err               │
                              │   (handle drop closes write_fd       │
                              │    → child reads EOF → exits)        │
                              └──────────────────────────────────────┘
                                                  ↓
                              caller: handle.signal_post_spawn_complete()
                                       (writes 1 byte to write_fd)
                                                  ↓
                              child unblocks → seccomp install → exec plugin
                                                  ↓
                              plugin lifecycle continues normally

On plugin exit:
  drop(child)            ──→ child netns refcount → 0; child-side veth disappears
  drop(SandboxHandle)    ──→ defensive close of any remaining sync_write_fd
                         ──→ nested NetFilterHandle drops (LIFO):
                                ip link del <veth-host>
                         ──→ main cleanup closure: landlock/seccomp release (no-op)
```

### What `tau resolve --check-sandbox` reports

After F 6.5 lands, the existing report is augmented:

- For plans WITHOUT `Network(Http)`: identical output to today.
- For plans WITH `Network(Http)`:
  - On F-available hosts: ✅ all checks pass.
  - On F-unavailable hosts: validate_plan returns the `SandboxError::NetFilter { ... }` for the specific missing prereq (already wired by F partial; F 6.5 just makes the validate path actually call into the cached probe).

---

## Components

### File layout

No new files. Modifications to existing files:

| File | Type | LOC delta |
|---|---|---|
| `crates/tau-ports/src/sandbox.rs` | trait extension + handle fields | +60 |
| `crates/tau-sandbox-native/src/strict.rs` | sync pipe in pre_exec; pre-allocate veth | +50 |
| `crates/tau-sandbox-native/src/lib.rs` | NativeSandbox::apply_post_spawn impl + cached probe + validate_plan extension | +80 |
| `crates/tau-sandbox-native/src/net.rs` | flip `unshare_flags_for_plan` | -10 / +5 |
| `crates/tau-sandbox-native/src/net_filter/mod.rs` | drop module-level `#[allow(dead_code)]`; expose `apply_per_host_filter` publicly | -1 / +1 |
| `crates/tau-sandbox-native/src/net_filter/netns.rs` | split `setup_veth_pair` → `allocate_subnet` + `setup_veth_pair_with_subnet` | refactor |
| `crates/tau-sandbox-native/tests/strict_net_filter.rs` | un-`#[ignore]` 4 stubs; flesh them out | +120 |
| `crates/tau-plugin-compat/tests/layer4_container.rs` | un-`#[ignore]` 3 tests; switch to 0.0.0.0 bind + parent veth IP | +30 |
| `crates/tau-plugin-compat/src/driver.rs` | inject `TAU_NET_PARENT_VETH_IP` from NetFilterHandle | +10 |
| `crates/tau-runtime/src/plugin_host/process.rs` | call `apply_post_spawn` + `signal_post_spawn_complete` | +20 |

Total: ~250 LOC delta + ~150 LOC test refactor.

### Public API additions

```rust
// crates/tau-ports/src/sandbox.rs

pub trait Sandbox: Send + Sync {
    // ... existing methods ...

    /// Adapter-specific post-spawn setup. Called by the runtime after
    /// `cmd.spawn()` succeeds and the child PID is known.
    ///
    /// Default: no-op. Mock + Container adapters use the default.
    /// NativeSandbox (Linux) applies per-host nftables filtering inside
    /// the child's netns when the plan has `Capability::Network(Http)`.
    ///
    /// On Ok: the caller MUST call `handle.signal_post_spawn_complete()`
    /// to release the child from its sync-pipe block in pre_exec.
    /// On Err: the caller drops `handle` (which dismisses the sync_write_fd;
    /// child reads EOF and exits cleanly) and reaps the child via wait().
    async fn apply_post_spawn(
        &self,
        plan: &SandboxPlan,
        child_pid: i32,
        handle: &mut SandboxHandle,
    ) -> Result<(), SandboxError> {
        let _ = (plan, child_pid, handle);
        Ok(())
    }
}

pub struct SandboxHandle {
    cleanup: Option<Box<dyn FnOnce() + Send>>,
    sync_write_fd: Option<RawFd>,         // NEW
    nested: Vec<Box<dyn Send>>,           // NEW
}

impl SandboxHandle {
    pub fn nest_handle(&mut self, guard: Box<dyn Send>);
    pub fn signal_post_spawn_complete(&mut self) -> std::io::Result<()>;
    // crate-internal: pub(crate) fn with_sync_write_fd(self, fd: RawFd) -> Self;
}
```

`SandboxHandle::Drop` is enhanced:
1. Defensive: if `sync_write_fd` is still set, close without writing → child sees EOF.
2. Drop nested guards LIFO.
3. Run main cleanup closure.

### Sync-pipe routing

Sync pipe is created in `strict.rs::apply_strict`:

```rust
let (read_fd, write_fd) = nix::unistd::pipe()?;
// read_fd captured in pre_exec closure
// write_fd attached to SandboxHandle via .with_sync_write_fd(fd)
// std::mem::forget on the OwnedFd to transfer ownership to SandboxHandle
```

Pre_exec closure addition (between unshare and seccomp):

```rust
if let Some(fd) = sync_read_raw {
    let mut byte = [0u8; 1];
    let n = unsafe { libc::read(fd, byte.as_mut_ptr() as _, 1) };
    if n != 1 {
        return Err(std::io::Error::other("net-filter setup failed (parent closed sync pipe)"));
    }
    unsafe { libc::close(fd); }
}
```

### Pre-allocation of veth subnet

`TAU_NET_PARENT_VETH_IP` env var must be set on `cmd` BEFORE `cmd.spawn()`. But the parent IP is allocated inside `apply_per_host_filter`, which runs AFTER spawn. Resolution: split the subnet allocation (pure) from the veth-pair shell-out (impure):

```rust
// crates/tau-sandbox-native/src/net_filter/netns.rs

/// Pure: pick a /30 subnet. Used in wrap_spawn.
pub(super) fn allocate_subnet() -> VethSubnet { /* ... */ }

/// Impure: shell out to `ip link add` etc. Used in apply_post_spawn.
pub(super) fn setup_veth_pair_with_subnet(
    exec: &dyn CommandExecutor,
    subnet: VethSubnet,
) -> Result<VethPair, NetFilterError> { /* ... */ }
```

`wrap_spawn` allocates + sets env var; `apply_post_spawn` consumes the allocation via the SandboxHandle.

### NativeSandbox cached probe

```rust
pub struct NativeSandbox {
    // ... existing fields ...
    net_filter_probe_cached: std::sync::OnceLock<Result<(), NetFilterError>>,
}

impl NativeSandbox {
    fn cached_net_filter_probe(&self) -> &Result<(), NetFilterError> {
        self.net_filter_probe_cached.get_or_init(net_filter::probe_prerequisites)
    }
}
```

`validate_plan` consults the cached probe; `wrap_spawn` does NOT re-probe (validation has already gated it).

### `unshare_flags_for_plan` final form

```rust
pub(crate) fn unshare_flags_for_plan(plan: &SandboxPlan) -> CloneFlags {
    let _ = plan;
    CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNET
}
```

The v0.1 fallback (drop CLONE_NEWNET when Network(Http)) is gone. F's probe gates Network(Http) plans at validate_plan time, so reaching wrap_spawn with Network(Http) means F is available and apply_post_spawn will configure the netns.

---

## Migration plan

Single PR `feat/f6-5-net-filter-integration`, 7 commits. No phases — changes interlock.

1. **`feat(ports): add Sandbox::apply_post_spawn + SandboxHandle::nest_handle / signal_post_spawn_complete`** — trait extension with default no-op + handle field additions + Drop changes. No behavior change to existing adapters.

2. **`refactor(sandbox-native): split allocate_subnet from setup_veth_pair`** — pure subnet-pick separated from ip-shell-out. Existing tests stay green.

3. **`feat(sandbox-native): wire sync pipe in apply_strict + pre-allocate veth + set TAU_NET_PARENT_VETH_IP`** — strict.rs gains pipe + write_fd routing through SandboxHandle; cmd gets env var pre-spawn. Without `apply_post_spawn` impl, this is dead-equivalent to today (sync_read_fd is None when has_network_http is false).

4. **`feat(sandbox-native): NativeSandbox::apply_post_spawn + cached probe + validate_plan extension + unshare flag flip`** — wires `apply_per_host_filter` through the new trait method. `unshare_flags_for_plan` flipped to always-include-CLONE_NEWNET; module-level `#[allow(dead_code)]` removed from net_filter.

5. **`feat(runtime): call apply_post_spawn + signal_post_spawn_complete from plugin_host::process`** — runtime caller updated. Error path drops handle + reaps child.

6. **`test(sandbox-native): un-#[ignore] strict_net_filter integration tests (4 stubs)`** — flesh out using sub-project G's typed fixture helpers.

7. **`test(plugin-compat): flip 3 layer4_container HTTP plugin tests + inject TAU_NET_PARENT_VETH_IP`** — drop `#[ignore]`; bind cassette on `0.0.0.0:0`; update tau-plugin-compat::driver to inject the env var.

Plus a separate Phase 2 docs PR:

8. **`docs(sandbox-net-filter): mark F task 6.5 done — drop PARTIAL from ROADMAP, close gap row, ADR-0019 addendum`** — followups doc + ROADMAP 12-F flips ✅ → ✅ (no PARTIAL) + ADR-0019 amended.

### Branch protection

**No change.** `test-net-filter / linux` already in required list (added at F task 6 user gate). After F 6.5 it just runs MORE tests inside the same job.

### Open PR handling

No check-name renames. Open PRs continue passing; they don't need rebase.

### Aborting mid-implementation

If commit 4 (the integration commit) reveals a deeper issue, commits 1-3 are individually safe to land standalone — they're additive scaffolding. Revert commit 4+ if needed; F partial state remains intact.

---

## Verification

### Per-task verification gates (focused, NOT full workspace)

| Commit | Verification |
|---|---|
| 1 | `cargo test -p tau-ports --lib` (existing tests + new SandboxHandle::nest_handle test) |
| 2 | `cargo test -p tau-sandbox-native --lib net_filter::netns` (refactored allocator) |
| 3 | `cargo test -p tau-sandbox-native --lib`; `cargo check -p tau-sandbox-native --tests` |
| 4 | `cargo test -p tau-sandbox-native --lib`; `cargo nextest run -p tau-sandbox-native --features integration-tests --test strict_net_filter --no-run` |
| 5 | `cargo check -p tau-runtime`; `cargo clippy -p tau-runtime -- -D warnings` |
| 6 | `cargo nextest run -p tau-sandbox-native --features integration-tests --test strict_net_filter --no-run` (compiles); CI-side runs them in privileged Docker |
| 7 | `grep -c '#\[ignore\]' crates/tau-plugin-compat/tests/layer4_container.rs` should drop by 3 |
| 8 (docs) | sed/grep checks on ROADMAP, followups, ADR-0019 |

### End-to-end verification (after PR merges + Phase 2 docs)

- [ ] `test-net-filter / linux` CI job runs **8 strict_net_filter tests** (4 stubs un-`#[ignore]`'d + 4 fleshed out) — all green.
- [ ] `plugin-compat / linux` CI job runs the 3 previously-`#[ignore]`'d Layer 4 container HTTP plugin tests — all green.
- [ ] `cargo clippy -p tau-sandbox-native --all-targets -- -D warnings` clean (`#[allow(dead_code)]` gone).
- [ ] `tau resolve --check-sandbox` on host without `nft` reports `network-filter: missing nft` (validate_plan rejects Network(Http) plans).
- [ ] Manual smoke (Linux): `tau install <anthropic-plugin> && tau chat -m anthropic` works; manual edit of plugin manifest with bad host fails at install validate-plan time.
- [ ] ROADMAP 12-F has dropped "PARTIAL".
- [ ] Followups doc gap row "Per-host network filtering is over-permissive" closed.
- [ ] ADR-0019 has an addendum documenting the integration choices (α-2 + nested handle + EOF-on-failure + 0.0.0.0 binds).

### What's NOT verified

- Performance impact of veth + nft setup on plugin spawn latency. Tracked separately.
- Plugin behavior on hosts where nftables is present but a specific rule fails to apply (edge-case kernel quirks). The `NftApplyFailed` error path is exercised; specific kernel-version behaviors not.
- macOS / Windows. Module gated `#[cfg(target_os = "linux")]`; these platforms unchanged.

---

## Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Pre-allocated subnet collides with another concurrent spawn | low | medium | Atomic counter (existing in `next_subnet`); refactor preserves it |
| Child blocks indefinitely if signal_post_spawn_complete never called | medium | high | SandboxHandle::Drop's defensive close-without-write makes "drop without signal" still cleanly cause child cleanup (EOF path); test covers this |
| `TAU_NET_PARENT_VETH_IP` env var not propagated through layer4_container test fixtures | medium | medium | Commit 7 verifies in-test; CI catches via un-`#[ignore]`'d tests |
| veth name collision in privileged Docker (parallel test runs) | low | low | Cargo nextest serializes per-binary by default; each test-net-filter / linux job has its own container |
| Layer 4 container tests timing out due to nft setup latency | low | low | Per-test timeout configurable in nextest profile; 30s should suffice |
| Existing `tau resolve --check-sandbox` snapshot tests change output | medium | medium | Update snapshots if they fire; intended behavior change |
| Network(Http) plans become unspawnable on host runners (intended after this lands) | certain (intended) | n/a | Spec Q1.A; documented; matches hard-refuse policy |
| `nix::unistd::pipe()` ergonomics on macOS | n/a | n/a | Module is Linux-only |
| `apply_post_spawn` is a new public trait method; downstream Sandbox impls might break | low | low | Default no-op impl makes it additive; only NativeSandbox overrides |
| `cmd.spawn()` returning before child reaches read(sync_fd) | certain (this is the design) | n/a | Documented; standard fork/exec semantics |

---

## Open questions deferred to plan-time

1. `async_trait::async_trait` vs Rust 1.75+ native async-in-trait — verify against existing Sandbox trait pattern.
2. `nix::unistd::pipe()` vs `pipe2(O_CLOEXEC)` — pick based on nix API ergonomics; both are correct (read happens before exec).
3. OwnedFd vs RawFd plumbing — match existing strict.rs RawFd convention.
4. The 4 stub test bodies — names are known; plan writes test bodies using sub-project G's helpers.
5. Cargo.lock — F 6.5 doesn't add new deps. (`async_trait` if used is already present; `nix` workspace dep already has needed features.)

---

## Documentation deliverables

Captured in commit 8:

- `ROADMAP.md` — 12-F row drops "PARTIAL" tag.
- `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` — sub-project F section heading flips from "✅ PARTIAL" → "✅ DONE 2026-05-XX"; outstanding-gaps row "Per-host network filtering is over-permissive" removed.
- `docs/decisions/0019-per-host-network-filter.md` — new "F task 6.5 — integration" addendum section documenting α-2 + nested handle + EOF-on-failure + 0.0.0.0 binds.
- `docs/reference/sandbox-platform-support.md` — per-host filter row updates "machinery in place; integration deferred" → "active enforcement".

---

## Decisions recorded

1. **α-2 architecture** for the post-spawn hook (`Sandbox::apply_post_spawn` trait method + `sync_write_fd` field on SandboxHandle).
2. **A: nested ownership** of NetFilterHandle inside SandboxHandle via `nest_handle`. Drop ordering automatic; no struct-field-order foot-gun.
3. **A: EOF-on-failure** for F setup errors (close write_fd; child sees EOF; cooperative exit).
4. **A: full Layer 4 test flip** (3 tests un-`#[ignore]`'d; bind 0.0.0.0; use `TAU_NET_PARENT_VETH_IP`).
5. **Veth subnet allocator split** — pure `allocate_subnet` (in wrap_spawn) + impure `setup_veth_pair_with_subnet` (in apply_post_spawn). Allows env-var-on-cmd-before-spawn.
6. **`unshare_flags_for_plan` always returns CLONE_NEWUSER | CLONE_NEWNET** — F's probe at validate_plan time gates Network(Http) plans on F-unavailable hosts.
7. **No new CI checks**. `test-net-filter / linux` already in required list.
8. **Single PR, 7 commits** — changes interlock. Plus a small Phase 2 docs PR.

---

## References

- Sub-project F (PARTIAL) spec: `docs/superpowers/specs/2026-05-06-sandbox-net-filter-design.md`
- Sub-project F (PARTIAL) plan: `docs/superpowers/plans/2026-05-06-sandbox-net-filter.md`
- ADR-0019: `docs/decisions/0019-per-host-network-filter.md`
- INTEGRATION.md: `crates/tau-sandbox-native/src/net_filter/INTEGRATION.md`
- Sub-project F PRs: #34 (closed; Phase 0 probe), #35 (machinery), #36 (docs).
- Sub-project G fixture helpers: `tau_domain::fixtures::cap_net_http`, `tau_ports::fixtures::plan_from_capabilities`.
