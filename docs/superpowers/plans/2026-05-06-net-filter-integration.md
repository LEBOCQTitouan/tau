# Net-Filter Integration (F task 6.5) Implementation Plan

> REQUIRED SUB-SKILL: superpowers:subagent-driven-development. Steps use `- [ ]` checkboxes.

**Goal:** Wire sub-project F's `apply_per_host_filter` machinery into the strict-tier `wrap_spawn` flow, closing the "PARTIAL" tag on F.

**Architecture:** New `Sandbox::apply_post_spawn(plan, child_pid, &mut handle)` trait method (default no-op) called by the runtime after `cmd.spawn()`. Sync-pipe barrier in pre_exec lets parent set up veth + nft inside the child's netns before it `execve`s. NetFilterHandle nested in SandboxHandle for LIFO cleanup.

**Tech Stack:** Rust 1.75+ native async-in-trait (existing pattern); `nix::unistd::pipe`; libc for raw read/write/close; existing `net_filter` module.

---

## Plan-erratum block

- **VERIFY against BASE_SHA = `9685cf1`** (spec commit) before claiming pre-existing failure.
- **Per-task focused gate.** Single-module unit tests + `--no-run` compilation.
- **Cargo.lock NOT touched.** F 6.5 adds zero new deps.
- **Use sub-project G's typed fixture helpers** in tests.

### Verified API surface

- `Sandbox` trait at `crates/tau-ports/src/sandbox.rs:156` — uses `#[allow(async_fn_in_trait)]` + native async fn (NOT async_trait macro).
- `SandboxHandle` at `crates/tau-ports/src/sandbox.rs:115` — has ONE field today: `cleanup: Option<Box<dyn FnOnce() + Send + 'static>>`.
- `net_filter::apply_per_host_filter` at `crates/tau-sandbox-native/src/net_filter/mod.rs:58` — pub async fn `(plan, child_pid)`. F 6.5 changes signature to `(plan, child_pid, subnet)`.
- Module-level `#![allow(dead_code, unused_imports, clippy::vec_init_then_push)]` at top of `net_filter/mod.rs`. F 6.5 removes it.
- `unshare_flags_for_plan` at `crates/tau-sandbox-native/src/net.rs` — currently has v0.1 fallback. F 6.5 flips to always `CLONE_NEWUSER | CLONE_NEWNET`.

### Critical plumbing

- **Commit ordering matters.** Commit 1 is foundational; do not reorder.
- **OwnedFd → RawFd**: use `.into_raw_fd()` (transfers ownership) — NOT `.as_raw_fd()`. SandboxHandle owns the closing.
- **`net_filter::apply_per_host_filter` signature change**: takes `subnet: VethSubnet` parameter (pre-allocated by `wrap_spawn`).
- **`netns::setup_veth_pair` split** into pure `allocate_subnet() -> VethSubnet` + impure `setup_veth_pair_with_subnet(exec, subnet)`.
- **`TAU_NET_PARENT_VETH_IP`** set on `cmd` BEFORE spawn via `cmd.env(...)` in apply_strict.
- **Subnet plumbing**: apply_strict returns `(SandboxHandle, Option<VethSubnet>)`; wrap_spawn stashes subnet in `NativeSandbox::veth_subnets: Mutex<HashMap<RawFd, VethSubnet>>` keyed by sync_write_fd; apply_post_spawn looks up + removes.
- **layer4_container test bind**: `0.0.0.0:0` (was `127.0.0.1:0`); plan hosts list includes `TAU_NET_PARENT_VETH_IP` value.
- **Drop module-level `#[allow]`** on `net_filter/mod.rs` — module is consumed in this PR.
- **Existing snapshots** for `tau resolve --check-sandbox` may need updating.

---

## File structure

| File | Change |
|---|---|
| `crates/tau-ports/src/sandbox.rs` | trait extension + handle fields + sync_write_fd_value accessor |
| `crates/tau-sandbox-native/src/net_filter/netns.rs` | split allocate_subnet + setup_veth_pair_with_subnet |
| `crates/tau-sandbox-native/src/net_filter/mod.rs` | drop module-level #[allow]; orchestrator takes subnet param |
| `crates/tau-sandbox-native/src/strict.rs` | sync pipe in pre_exec; pre-allocate veth; set TAU_NET_PARENT_VETH_IP; return tuple |
| `crates/tau-sandbox-native/src/net.rs` | flip unshare_flags_for_plan |
| `crates/tau-sandbox-native/src/lib.rs` | NativeSandbox::apply_post_spawn impl + cached probe + validate_plan extension |
| `crates/tau-sandbox-native/tests/strict_net_filter.rs` | flesh out 4 stubs |
| `crates/tau-plugin-compat/tests/layer4_container.rs` | un-#[ignore] 3 tests; bind 0.0.0.0; use TAU_NET_PARENT_VETH_IP |
| `crates/tau-plugin-compat/src/driver.rs` | extra-env extension |
| `crates/tau-runtime/src/plugin_host/process.rs` | call apply_post_spawn + signal_post_spawn_complete |

No new files. Phase 2 docs commit modifies ROADMAP, ADR-0019, followups, reference doc.

---

# Phase 1 — Implementation (single PR)

### Task 1: SandboxHandle extensions + apply_post_spawn trait method (FULL FIDELITY)

**Files:** `crates/tau-ports/src/sandbox.rs`, `crates/tau-ports/Cargo.toml`

- [ ] **Step 1**: Verify `libc` dep is present in `crates/tau-ports/Cargo.toml`. If not, add `libc = { workspace = true }` to `[dependencies]`.

- [ ] **Step 2**: Replace `SandboxHandle` struct + impls + Drop + Debug (lines 112-146 of sandbox.rs) with the version that adds `sync_write_fd: Option<std::os::fd::RawFd>` and `nested: Vec<Box<dyn Send>>` fields. Add methods: `with_sync_write_fd(self, RawFd) -> Self`, `nest_handle(&mut self, Box<dyn Send>)`, `signal_post_spawn_complete(&mut self) -> io::Result<()>`, `sync_write_fd_value(&self) -> Option<RawFd>`. Drop closes any remaining sync_write_fd defensively, drains nested LIFO, then runs main cleanup. `noop()` returns handle with all fields default.

- [ ] **Step 3**: Add `apply_post_spawn` trait method to `Sandbox` (after `wrap_spawn`):

```rust
    async fn apply_post_spawn(
        &self,
        plan: &SandboxPlan,
        child_pid: i32,
        handle: &mut SandboxHandle,
    ) -> Result<(), SandboxError> {
        let _ = (plan, child_pid, handle);
        Ok(())
    }
```

- [ ] **Step 4**: Add 2 unit tests in the existing `#[cfg(test)] mod tests` block: `nest_handle_drops_in_lifo_order` (uses Arc<Mutex<Vec<&str>>> + 2 Guard structs to verify LIFO + main_cleanup ordering) and `signal_post_spawn_complete_is_noop_without_fd`.

- [ ] **Step 5**: Verify:
```
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-ports --lib
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo check -p tau-sandbox-native --all-targets
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo check -p tau-sandbox-container --all-targets
```

- [ ] **Step 6**: Commit `feat(ports): Sandbox::apply_post_spawn + SandboxHandle nest/signal (F 6.5 task 1)`.

---

### Task 2: Split netns allocator (FULL FIDELITY)

**Files:** `crates/tau-sandbox-native/src/net_filter/netns.rs`, `crates/tau-sandbox-native/src/net_filter/mod.rs`

- [ ] **Step 1**: In netns.rs, add `pub(crate) struct VethSubnet { parent_ip, child_ip }` (Ipv4Addr fields, Copy + Clone). Add `pub(crate) fn allocate_subnet() -> VethSubnet` containing the existing atomic-counter + pid-modulo subnet logic (extracted from `next_subnet`).

- [ ] **Step 2**: Rename `setup_veth_pair` → `setup_veth_pair_with_subnet`, change signature to take `subnet: VethSubnet` parameter; remove the internal `next_subnet()` call in favor of using the parameter's IPs.

- [ ] **Step 3**: Update unit tests: rename `setup_veth_pair_invokes_three_ip_commands_in_order` → `setup_veth_pair_with_subnet_invokes_three_ip_commands_in_order`; same for `_propagates_ip_failure`; rename `next_subnet_returns_valid_ipv4_pair_in_10_222_range` → `allocate_subnet_returns_valid_ipv4_pair_in_10_222_range`.

- [ ] **Step 4**: Update `apply_per_host_filter` in mod.rs — change signature to `(plan, child_pid, subnet)`, replace `setup_veth_pair(&exec)` with `setup_veth_pair_with_subnet(&exec, subnet)`.

- [ ] **Step 5**: Verify:
```
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo check -p tau-sandbox-native --lib
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-sandbox-native --lib net_filter::netns
```

- [ ] **Step 6**: Commit `refactor(sandbox-native): split allocate_subnet from setup_veth_pair (F 6.5 task 2)`.

---

### Task 3: Sync pipe in apply_strict + pre-allocate veth (FULL FIDELITY)

**Files:** `crates/tau-sandbox-native/src/strict.rs`, `crates/tau-sandbox-native/src/lib.rs` (temporary call-site fix)

- [ ] **Step 1**: In strict.rs `apply_strict`, after the `unshare_flags` line, compute `has_network_http` (check capabilities for `Capability::Network(NetCapability::Http { .. })`). If has_network_http, call `crate::net_filter::netns::allocate_subnet()` and `cmd.env("TAU_NET_PARENT_VETH_IP", subnet.parent_ip.to_string())`.

- [ ] **Step 2**: If has_network_http, create sync pipe via `nix::unistd::pipe()`. Convert OwnedFds to RawFds via `.into_raw_fd()`. Capture `sync_read_raw: Option<RawFd>` and `sync_write_raw: Option<RawFd>`.

- [ ] **Step 3**: Inside the pre_exec closure (between unshare and seccomp), insert:
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

- [ ] **Step 4**: At the end of apply_strict, build the SandboxHandle with `with_sync_write_fd(fd)` if applicable. Change apply_strict's return type from `Result<SandboxHandle, SandboxError>` to `Result<(SandboxHandle, Option<crate::net_filter::netns::VethSubnet>), SandboxError>`. Return `(handle, veth_subnet)`.

- [ ] **Step 5**: Update the call site in `crates/tau-sandbox-native/src/lib.rs::wrap_spawn` (or wherever apply_strict is called) to destructure: `let (handle, _veth_subnet) = apply_strict(plan, cmd)?;`. The `_veth_subnet` is consumed by Task 4.

- [ ] **Step 6**: Verify `cargo check -p tau-sandbox-native --lib` passes.

- [ ] **Step 7**: Commit `feat(sandbox-native): sync pipe in apply_strict + pre-allocate veth (F 6.5 task 3)`. Note in commit body: this commit alone leaves Network(Http) plugins hanging in pre_exec; Tasks 4-5 fix.

---

### Task 4: NativeSandbox::apply_post_spawn + cached probe + unshare flip (HYBRID)

**Files:** `crates/tau-sandbox-native/src/lib.rs`, `crates/tau-sandbox-native/src/net.rs`, `crates/tau-sandbox-native/src/net_filter/mod.rs`

**Summary:** Implement the apply_post_spawn override; add cached probe field + veth_subnets HashMap; extend validate_plan; flip unshare flags; remove module-level #[allow].

**Key changes:**

1. NativeSandbox struct gains `net_filter_probe_cached: OnceLock<Result<(), NetFilterError>>` and `veth_subnets: Mutex<HashMap<RawFd, VethSubnet>>`.
2. Add `fn cached_net_filter_probe(&self)` using `get_or_init`.
3. Modify `wrap_spawn`: destructure `(handle, veth_subnet)` from apply_strict; if both `handle.sync_write_fd_value()` and `veth_subnet` are Some, insert into veth_subnets HashMap.
4. Implement `apply_post_spawn`: skip if no Network(Http); else look up subnet via fd → call `apply_per_host_filter(plan, child_pid, subnet)` → `handle.nest_handle(Box::new(nf_handle))`.
5. Modify `validate_plan`: if plan has Network(Http) and cached probe failed → `Err(SandboxError::NetFilter { message })`. Then call `validate_hosts`.
6. In net.rs, simplify `unshare_flags_for_plan` to always return `CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNET`. Update existing tests: rename `unshare_flags_with_http_drops_newnet` → `_includes_newnet`; flip the assertion.
7. In net_filter/mod.rs, delete the line `#![allow(dead_code, unused_imports, clippy::vec_init_then_push)]`.

**Verification:**
```
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo test -p tau-sandbox-native --lib
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-sandbox-native --all-targets -- -D warnings
```

Commit: `feat(sandbox-native): NativeSandbox::apply_post_spawn + cached probe + unshare flip (F 6.5 task 4)`.

---

### Task 5: Runtime caller (HYBRID)

**Files:** `crates/tau-runtime/src/plugin_host/process.rs`

**Summary:** After `cmd.spawn()`, call `sandbox.apply_post_spawn(plan, child.id() as i32, &mut handle).await`. On Ok: `handle.signal_post_spawn_complete()`. On Err: drop handle (defensive Drop closes write_fd → child reads EOF → exits), `child.wait()` to reap, return error.

**Verify the actual `RuntimeError::SandboxValidationFailed` variant shape** in `crates/tau-runtime/src/error.rs` before writing the match arms. Sub-project A had hallucination issues here; D + E avoided by reading first.

**Verification:**
```
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo check -p tau-runtime
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-runtime --all-targets -- -D warnings
```

Commit: `feat(runtime): call apply_post_spawn + signal_post_spawn_complete (F 6.5 task 5)`.

---

### Task 6: Un-#[ignore] strict_net_filter integration tests (HYBRID)

**Files:** `crates/tau-sandbox-native/tests/strict_net_filter.rs`, possibly `crates/tau-plugin-compat/fixtures/controlled-env-binary/src/main.rs`

**Summary:** Flesh out the 4 stubs from F partial:
- `localhost_socket_allowed_with_http_cap` — plan with `cap_net_http(&["127.0.0.1"], ...)`; spawn controlled-env in `socket-connect` mode.
- `external_host_socket_allowed_with_http_cap` — plan with `cap_net_http(&["one.one.one.one"], ...)`; verify connect succeeds.
- `no_network_cap_socket_denied_by_seccomp` — plan WITHOUT Network(Http); verify socket() syscall is killed by seccomp (SIGSYS).
- `net_filter_handle_drop_removes_parent_veth` — spawn plugin; after exit, verify `ip link show` doesn't list the `tsb*` veth.

Each test follows pattern of existing strict_seccomp.rs tests: NativeSandbox::new, wrap_spawn, spawn, apply_post_spawn, signal_post_spawn_complete, observe outcome.

If controlled-env binary doesn't have a `socket-connect` or `external-connect` mode, add it (similar to existing `open-socket` mode but actually attempts a TCP connection via libc).

**Verification:**
```
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-sandbox-native --features integration-tests --test strict_net_filter --no-run
```

Commit: `test(sandbox-native): un-#[ignore] strict_net_filter integration tests (F 6.5 task 6)`.

---

### Task 7: Flip 3 layer4_container HTTP plugin tests (HYBRID)

**Files:** `crates/tau-plugin-compat/tests/layer4_container.rs`, `crates/tau-plugin-compat/src/driver.rs`

**Summary:** Drop `#[ignore]` on `anthropic_layer4_container_completes_via_cassette`, `ollama_*`, `openai_*`. Each test:
- Bind cassette server on `0.0.0.0:0` (was `127.0.0.1:0`).
- Read `TAU_NET_PARENT_VETH_IP` env var (set by NativeSandbox in F task 6.5).
- Construct plan with `cap_net_http(&[parent_ip.as_str()], &["GET", "POST"])`.
- BASE_URL = `http://{parent_ip}:{port}`.

If the existing driver helpers don't support custom env vars, add `driver::spawn_under_sandbox_with_env(plan, binary, extra_env: I)` where `I: IntoIterator<Item = (impl AsRef<OsStr>, impl AsRef<OsStr>)>`.

**Verification:**
```
grep -c '#\[ignore\]' crates/tau-plugin-compat/tests/layer4_container.rs
# Expected: previous count - 3
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-plugin-compat --features integration-tests --test layer4_container --no-run
```

Commit: `test(plugin-compat): flip 3 layer4_container HTTP plugin tests (F 6.5 task 7)`.

---

### Task 8 (USER GATE): Open Phase 1 PR

- [ ] **Step 1**: Push branch + open PR with body summarizing all 7 commits, branch protection note (no change needed; `test-net-filter / linux` already required), test plan.

- [ ] **Step 2**: PAUSE — user reviews + merges.

---

# Phase 2 — Documentation

### Task 9: ADR-0019 addendum + ROADMAP + followups + reference

**Files:**
- Modify `docs/decisions/0019-per-host-network-filter.md` — append "F task 6.5 — integration" section (architecture α-2; nested handle; EOF-on-failure; 0.0.0.0 binds; veth subnet split; unshare flags flip; PR ref).
- Modify `ROADMAP.md` — 12-F row drops "PARTIAL" tag; body updated to reflect full integration.
- Modify `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` — F section heading flips from "✅ PARTIAL" to "✅ DONE 2026-05-XX"; outstanding-gaps row "Per-host network filtering is over-permissive" removed.
- Modify `docs/reference/sandbox-platform-support.md` — per-host filter row updates from "machinery in place; integration deferred" to "active enforcement".

**Verification:**
```
grep -c 'PARTIAL' ROADMAP.md  # 0 for 12-F
grep 'F task 6.5' docs/decisions/0019-per-host-network-filter.md  # matches
```

Commit: `docs(sandbox-net-filter): mark F task 6.5 done — drop PARTIAL (F 6.5 task 9)`.

---

### Task 10 (USER GATE): Final squash-merge

- [ ] **Step 1**: Push docs branch + open PR.

- [ ] **Step 2**: PAUSE — user merges. Sub-project F fully done.

---

## End-to-end verification

After Task 10 lands on main:

- [ ] All 15 required checks green
- [ ] `cargo nextest run -p tau-sandbox-native --features integration-tests --test strict_net_filter` runs 4 tests, all green (privileged Docker)
- [ ] 3 previously-#[ignore]'d Layer 4 container tests run green
- [ ] `cargo clippy -p tau-sandbox-native --all-targets -- -D warnings` clean (module-level #[allow] gone)
- [ ] `tau resolve --check-sandbox` on host without nft reports `network-filter: missing nft`
- [ ] ROADMAP 12-F has dropped "PARTIAL"
- [ ] Followups doc gap row "Per-host network filtering is over-permissive" closed
- [ ] ADR-0019 has the F task 6.5 addendum
