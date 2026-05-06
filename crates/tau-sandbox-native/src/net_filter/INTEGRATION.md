# net_filter + strict.rs integration: post-spawn hook problem

## Status

Deferred to sub-project F task 6.5. The per-host filter orchestrator
(`apply_per_host_filter`) and all supporting infrastructure (handle.rs,
mod.rs, veth setup, nftables apply) are fully implemented. The missing piece
is **wiring the orchestrator call into the spawn lifecycle**.

## The architectural problem

`apply_per_host_filter(plan, child_pid)` must run in the **parent** process
after `fork()` but before the child proceeds past the sync-pipe barrier into
`seccomp`. It needs `child_pid`, which is only available after `Command::spawn()`
returns the `Child` handle.

However, `apply_strict` does not call `spawn()`. It installs a `pre_exec`
closure on `cmd` and returns a `SandboxHandle`. The runtime layer calls
`spawn()` later (via `wrap_spawn` on the `Sandbox` trait).

This means F's integration requires the runtime to take a new action between
the existing steps:

```
[existing]  sandbox.wrap_spawn(plan, cmd) -> SandboxHandle
[existing]  cmd.spawn() -> Child               (child forks, runs pre_exec, blocks on pipe)
[NEW F 6.5] apply_per_host_filter(plan, child.id()) -> NetFilterHandle
[NEW F 6.5] write byte to sync pipe           (child unblocks, runs seccomp, execs)
```

## Sync-pipe design

The sync-pipe is a Unix anonymous pipe created before spawn:

- **Read end** (`read_fd`): captured in the `pre_exec` closure. The child
  blocks on `libc::read(read_fd, ...)` between `unshare` and `seccomp`.
  It is read before exec, so `O_CLOEXEC` is fine (the fd is read, then exec
  closes it automatically).
- **Write end** (`write_fd`): held by the parent. After
  `apply_per_host_filter` succeeds, the parent writes 1 byte. If
  `apply_per_host_filter` fails, the parent closes `write_fd` without
  writing; the child reads 0 bytes (EOF) → returns `Err` from `pre_exec` →
  exits with error → `cmd.spawn()` … actually, the child has already been
  spawned by this point: `spawn()` returned. The child exits independently;
  the caller detects it via `Child::wait()`.

## Option α — extend `Sandbox::wrap_spawn` signature

Add a `post_fork_hook: Option<Box<dyn FnOnce(u32) -> Pin<Box<dyn Future<...>>>>>` parameter
to `wrap_spawn`. The hook receives the child PID and is called by the runtime
after `spawn()`. The `NativeSandbox` implementation supplies the hook when the
plan has `Network(Http)`.

**Pros**: clean; hook is co-located with the sandbox adapter.
**Cons**: trait signature change; all existing impls (Container, Mock) must add a parameter.

## Option β — add `Sandbox::apply_post_spawn` (recommended)

Add a new async method to the `Sandbox` trait with a default no-op:

```rust
async fn apply_post_spawn(
    &self,
    plan: &SandboxPlan,
    child_pid: u32,
) -> Result<Box<dyn Any + Send>, SandboxError> {
    let _ = (plan, child_pid);
    Ok(Box::new(()))
}
```

`NativeSandbox` overrides it to call `apply_per_host_filter` and return the
`NetFilterHandle` (boxed). The runtime calls `apply_post_spawn` immediately
after `spawn()`, before waiting on the child.

**Pros**: backward-compatible; existing impls work unchanged; clean separation.
**Cons**: `Box<dyn Any>` for the handle is slightly awkward; requires downcasting
in the runtime if the handle needs to be stored. Alternatively, the trait can return
a `SandboxHandle` variant that wraps it.

## Option γ — runtime-layer extension specific to NativeSandbox

The runtime detects (via `TypeId` or an extension trait) that the active adapter
is `NativeSandbox` and calls a separate `NativeSandbox::apply_net_filter` method
directly. Other adapters are not touched.

**Pros**: no trait surgery.
**Cons**: breaks the adapter abstraction; runtime must know about concrete types.

## Recommendation

**Option β**. It is the most idiomatic Rust approach: backward-compatible
trait extension with a default no-op, concrete override in `NativeSandbox`.
The `SandboxHandle` type (from `tau-ports`) can be extended to carry an
optional `NetFilterHandle`, or `apply_post_spawn` can return a new
`PostSpawnHandle` newtype that the runtime stores alongside the `SandboxHandle`.

## Files to modify for F task 6.5

1. `crates/tau-ports/src/sandbox.rs` — add `apply_post_spawn` to `Sandbox` trait.
2. `crates/tau-sandbox-native/src/lib.rs` (or `strict.rs`) — implement
   `apply_post_spawn` on `NativeSandbox`; create sync pipe in `apply_strict` and
   expose the write fd for the parent-side signal step.
3. `crates/tau-runtime/src/plugin_host/process.rs` (or wherever `spawn()` is called) —
   call `adapter.apply_post_spawn(plan, child.id()).await` after `spawn()`.
4. `crates/tau-sandbox-native/tests/strict_net_filter.rs` — flip `#[ignore]` once
   the integration is wired.
5. `crates/tau-plugin-compat/tests/layer4_container.rs` — flip the 3 Tier B
   `#[ignore]`'d tests once the veth+nftables path works end-to-end.
