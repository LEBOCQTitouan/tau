# End-to-end landlock CI integration + port-aware Layer 4 driver — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** retire the `#[ignore]` debt from sub-project B (10 Layer 4 plugin-compat tests) AND the kernel-enforcement debt from priority-12 ship (5 removed e2e files). Real-kernel verification on Linux CI.

**Architecture:** three layers of work. Adapter e2e in `tau-sandbox-native::tests/` (4 files using the controlled-env binary). Runtime e2e in `tau-runtime::tests/sandbox_native.rs`. Port-aware test driver in `tau-plugin-compat::driver` wrapping the public `tau_runtime::plugin_host::load_{tool,llm_backend,storage}` functions. 7 of 10 Layer 4 ignores flipped (4 tool plugins + 3 native HTTP plugins via cassette-replay over inherited netns localhost); 3 container HTTP plugin tests stay `#[ignore]`'d pending sub-project F.

**Tech Stack:** Rust 1.91 stable / 1.91 MSRV. Existing infrastructure: `tau_runtime::plugin_host::load_*` (public), `tau_ports::Sandbox` trait, `tau-plugin-test-support` cassette infrastructure, `wiremock` for localhost cassette server. New: `integration-tests` Cargo features on `tau-sandbox-native` and `tau-runtime`; `driver` module in `tau-plugin-compat`.

---

## Plan-erratum block

Carryovers from sub-projects A & B that apply to this plan:

- **VERIFY against BASE_SHA = `0a97bcc` before claiming "pre-existing failure".** Sub-project A had 4-of-5 false-alarm "pre-existing" claims; B had 0 (improved discipline). Maintain that.
- **`#[non_exhaustive]` on every new public type.** `DriveError` is the new public enum; gets `#[non_exhaustive]`. Any new public types follow suit.
- **Per-task focused gates.** Use `cargo test -p <crate> --features integration-tests --tests`. The `--tests` flag is important for crates with new feature gating. Full workspace gate runs at Task 10's USER GATE + on CI.
- **Cargo.lock staged in same commit as new deps.** Task 7 likely adds `wiremock` if not in workspace deps; verify with `grep wiremock Cargo.toml` and stage Cargo.lock with the dep change.
- **Branch protection 27 → 29.** Task 10's PR push surfaces the 2 new check names; user makes the GitHub-settings change manually.
- **macOS dev / Linux CI gap.** All e2e tests are `cfg(target_os = "linux")` gated; they MUST compile on macOS but won't run there. Per-task gates verify compilation; CI verifies execution on Linux.
- **`TAU_TESTING_ALLOW_MOCK_SANDBOX=1` env var preserved but NOT used in these tests** — they use real adapters intentionally.
- **The driver in Task 3 wraps `tau-runtime::plugin_host::load_{tool,llm_backend,storage}`** — these are the public `pub async fn` in `crates/tau-runtime/src/plugin_host/mod.rs` (lines 343, 419, 515 at HEAD). `PluginProcess::spawn_and_handshake` is `pub(crate)`-only and NOT directly accessible from `tau-plugin-compat`; the driver uses the higher-level public surface.
- **`PluginHostOptions::sandbox_adapter`** is `Option<Arc<crate::sandbox::SandboxAdapter>>` (verified at line 291 of plugin_host/mod.rs). Driver constructs `PluginHostOptions { sandbox_adapter: Some(arc), ..PluginHostOptions::default() }`.
- **`PortKind` is `#[non_exhaustive]`** with at least Tool, LlmBackend, Storage, Sandbox variants. Driver match arms must include a wildcard catch-all.
- **`wiremock` is the cassette-replay library**. `tau-plugin-test-support` may or may not export helpers — Task 7 verifies and reuses if available, otherwise constructs `MockServer` per-test.
- **The 10 currently-`#[ignore]`'d Layer 4 tests** at `crates/tau-plugin-compat/tests/layer4_*.rs` have empty stub bodies after sub-project B's Tier-A regression fix in commit `a449c10`. Tasks 5, 6, 7 fill in real test bodies AND remove the `#[ignore]` attribute (or change its rationale for the 3 deferred container HTTP tests).
- **No new lockfile schema bump.** `LockedPlugin.required_shapes` was already populated by sub-project B's Layer 2 cross-check; D doesn't change lockfile schema.
- **Sub-project F is the unblocker** for the 3 container × HTTP plugin tests that stay ignored; Task 11's followups doc update should cross-reference.

---

## File structure

| Path | Status | Responsibility |
|---|---|---|
| `crates/tau-plugin-compat/fixtures/controlled-env-binary/src/main.rs` | Modify | Add `TAU_FIXTURE_MODE` dispatch (`read` / `open-socket` / `exec` / `default`) |
| `crates/tau-sandbox-native/Cargo.toml` | Modify | Add `[features] integration-tests = []` |
| `crates/tau-sandbox-native/tests/light_landlock.rs` | Create | 3 e2e tests (allowed_read, blocked_read, multiple_paths) |
| `crates/tau-sandbox-native/tests/strict_seccomp.rs` | Create | 3 e2e tests (socket blocked, socket allowed-with-cap, baseline syscalls) |
| `crates/tau-sandbox-native/tests/strict_net_filter.rs` | Create | 2 e2e tests (localhost reachable with Network(Http) cap) |
| `crates/tau-sandbox-native/tests/strict_exec_gating.rs` | Create | 1 stub `#[ignore]`'d test pointing to sub-project E |
| `crates/tau-runtime/Cargo.toml` | Modify | Add `[features] integration-tests = []` |
| `crates/tau-runtime/tests/sandbox_native.rs` | Create | 2 runtime-e2e tests (adapter threads through; plan validation pre-spawn) |
| `crates/tau-plugin-compat/src/lib.rs` | Modify | Add `pub mod driver;` |
| `crates/tau-plugin-compat/src/driver.rs` | Create | New driver module with `spawn_tool_under_sandbox` etc. |
| `crates/tau-plugin-compat/tests/layer4_native.rs` | Modify | Flip 5 `#[ignore]`'d tests (2 tool + 3 HTTP) to real implementations |
| `crates/tau-plugin-compat/tests/layer4_container.rs` | Modify | Flip 2 `#[ignore]`'d tool tests; update 3 HTTP rationale strings |
| `crates/tau-plugin-compat/Cargo.toml` | Modify | Add `wiremock` dev-dep if missing |
| `Cargo.lock` | Modify | Resolved transitives for `wiremock` (Task 7) |
| `docs/reference/sandbox-platform-support.md` | Create | Kernel feature requirements + tested distros + known limitations |
| `.github/workflows/ci.yml` | Modify | 2 new Linux jobs (sandbox-native e2e + runtime e2e) |
| `docs/decisions/0017-e2e-landlock-and-driver.md` | Create (Task 11) | ADR for sub-project D |
| `ROADMAP.md` | Modify (Task 11) | Mark 12-D done |
| `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` | Modify (Task 11) | Mark D done; flag F as unblocker for 3 remaining tests |

---

## Task 1: Controlled-env binary mode dispatch

**Files:**
- Modify: `crates/tau-plugin-compat/fixtures/controlled-env-binary/src/main.rs`

**Goal:** add `TAU_FIXTURE_MODE` env-var dispatch with values `read` / `open-socket` / `exec` / `default`. Preserve existing behavior when `TAU_FIXTURE_MODE` is unset (existing tests rely on the "if `TAU_FIXTURE_INPUT_PATH` is set, read it; else emit `CONTROLLED_ENV_OK`" path).

- [ ] **Step 1: Read existing `main.rs` to confirm current shape**

```bash
cat crates/tau-plugin-compat/fixtures/controlled-env-binary/src/main.rs
```

Expected: ~50 LOC with `read_and_emit` + `emit_default` per sub-project B Task 4.

- [ ] **Step 2: Replace `main.rs` with mode-dispatched version**

```rust
//! Controlled-environment test binary for sub-project B's landlock
//! e2e tests, extended in sub-project D for seccomp + exec coverage.
//!
//! # Mode dispatch
//!
//! `TAU_FIXTURE_MODE` env var selects the binary's behavior:
//!
//! - `read` (or unset, when `TAU_FIXTURE_INPUT_PATH` is set):
//!   Read up to 256 bytes from `TAU_FIXTURE_INPUT_PATH`, emit
//!   `READ_OK <bytes>\n` to stdout, exit 0.
//! - `open-socket`: call `socket(AF_INET, SOCK_STREAM, 0)`. On success,
//!   emit `SOCKET_OK\n` and exit 0. On EACCES / EPERM, emit error and
//!   exit 1. SIGSYS from seccomp → no output, signal exit.
//! - `exec`: spawn `${TAU_FIXTURE_EXEC_CMD}` with no args, proxy its
//!   stdout, exit with its exit code.
//! - `default` (or no env vars set): emit `CONTROLLED_ENV_OK\n`,
//!   exit 0.
//!
//! Statically-linked release builds avoid landlock false positives on
//! Ubuntu CI's `/bin → /usr/bin` symlink layout.

use std::io::{Read, Write};

fn main() {
    let mode = std::env::var("TAU_FIXTURE_MODE").ok();
    let path = std::env::var("TAU_FIXTURE_INPUT_PATH").ok();

    let result = match mode.as_deref() {
        Some("read") => match path {
            Some(p) => read_and_emit(&p),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "TAU_FIXTURE_MODE=read requires TAU_FIXTURE_INPUT_PATH",
            )),
        },
        Some("open-socket") => open_socket(),
        Some("exec") => exec_proxy(),
        Some("default") | None => match path {
            Some(p) => read_and_emit(&p),
            None => emit_default(),
        },
        Some(other) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unknown TAU_FIXTURE_MODE: {other}"),
        )),
    };

    if let Err(e) = result {
        eprintln!("controlled-env-binary error: {e}");
        std::process::exit(1);
    }
}

fn read_and_emit(path: &str) -> std::io::Result<()> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 256];
    let n = file.read(&mut buf)?;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(b"READ_OK ")?;
    handle.write_all(&buf[..n])?;
    handle.write_all(b"\n")?;
    handle.flush()?;
    Ok(())
}

fn emit_default() -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(b"CONTROLLED_ENV_OK\n")?;
    handle.flush()?;
    Ok(())
}

fn open_socket() -> std::io::Result<()> {
    // Use libc directly to avoid pulling std::net which may add dynamic deps.
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        let err = std::io::Error::last_os_error();
        return Err(err);
    }
    unsafe { libc::close(fd) };
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(b"SOCKET_OK\n")?;
    handle.flush()?;
    Ok(())
}

fn exec_proxy() -> std::io::Result<()> {
    let cmd = std::env::var("TAU_FIXTURE_EXEC_CMD").map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "TAU_FIXTURE_MODE=exec requires TAU_FIXTURE_EXEC_CMD",
        )
    })?;
    let output = std::process::Command::new(&cmd).output()?;
    std::io::stdout().write_all(&output.stdout)?;
    std::io::stdout().flush()?;
    if !output.status.success() {
        std::process::exit(output.status.code().unwrap_or(1));
    }
    Ok(())
}
```

- [ ] **Step 3: Add `libc` to the binary's `Cargo.toml`**

```toml
# crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml

[dependencies]
libc = "0.2"
```

- [ ] **Step 4: Verify the binary builds (release profile, statically linked)**

```bash
cargo build --manifest-path crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml --release
```

Expected: clean build. Binary at `crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env`.

- [ ] **Step 5: Smoke-test each mode**

```bash
BIN=crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env

# default mode
$BIN
# expected stdout: CONTROLLED_ENV_OK

# read mode
echo -n "hello" > /tmp/tau-test-input
TAU_FIXTURE_MODE=read TAU_FIXTURE_INPUT_PATH=/tmp/tau-test-input $BIN
# expected stdout: READ_OK hello

# open-socket mode (host has no sandbox; should succeed)
TAU_FIXTURE_MODE=open-socket $BIN
# expected stdout: SOCKET_OK

# exec mode
TAU_FIXTURE_MODE=exec TAU_FIXTURE_EXEC_CMD=/bin/echo $BIN
# expected stdout: <empty since /bin/echo with no args is empty>; exit 0

rm /tmp/tau-test-input
```

All four modes succeed.

- [ ] **Step 6: Verification gates**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --manifest-path crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml --release
cargo fmt --all -- --check
```

Both clean. (clippy + workspace tests not relevant — the binary is outside the workspace.)

- [ ] **Step 7: Commit**

```bash
git add crates/tau-plugin-compat/fixtures/controlled-env-binary/src/main.rs \
        crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml \
        crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.lock

git commit -m "feat(plugin-compat): add TAU_FIXTURE_MODE dispatch to controlled-env binary

Sub-project D Task 1. Extends the priority-B controlled-env binary with
3 new modes (open-socket, exec, default) on top of the existing read
mode. Modes selected via TAU_FIXTURE_MODE env var; default behavior
preserved when no env var set.

read mode: existing — read TAU_FIXTURE_INPUT_PATH, emit READ_OK <bytes>
open-socket mode: call socket(AF_INET, SOCK_STREAM, 0); emit SOCKET_OK
                  on success or error message on failure
exec mode: spawn TAU_FIXTURE_EXEC_CMD; proxy stdout; exit with its code
default mode: emit CONTROLLED_ENV_OK (existing fallback)

The binary is intentionally outside the workspace (statically linked
release profile per sub-project B). Adds libc dep for socket() syscall
without pulling std::net's dynamic deps.

Tasks 2 + 4 use these modes for the kernel-enforcement e2e tests.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 2: tau-sandbox-native integration-tests feature + 4 e2e files

**Files:**
- Modify: `crates/tau-sandbox-native/Cargo.toml` (add `[features] integration-tests = []`)
- Create: `crates/tau-sandbox-native/tests/light_landlock.rs` (~150 LOC, 3 tests)
- Create: `crates/tau-sandbox-native/tests/strict_seccomp.rs` (~100 LOC, 3 tests)
- Create: `crates/tau-sandbox-native/tests/strict_net_filter.rs` (~120 LOC, 2 tests)
- Create: `crates/tau-sandbox-native/tests/strict_exec_gating.rs` (~30 LOC stub, 1 ignored test)

- [ ] **Step 1: Add `integration-tests` feature to Cargo.toml**

```toml
# crates/tau-sandbox-native/Cargo.toml

[features]
integration-tests = []
```

(If `[features]` section doesn't exist, add it. If it has `default = []`, add `integration-tests = []` alongside.)

- [ ] **Step 2: Verify `tau-sandbox-native`'s public test surface**

The 4 e2e tests need to call `wrap_spawn` (or equivalent) from outside the crate. Find the public entry point:

```bash
grep -n "^pub fn\|^impl.*Sandbox.*for NativeSandbox\|pub use" crates/tau-sandbox-native/src/lib.rs | head -20
```

Expected: `NativeSandbox` implements `tau_ports::Sandbox` trait with `wrap_spawn` method. Tests construct `NativeSandbox::new()` and call `.wrap_spawn(plan, cmd).await`.

If the test surface differs, adapt the test code below to whatever the actual public API is (constructor + trait method or direct function).

- [ ] **Step 3: Create `tests/light_landlock.rs`**

```rust
//! Sub-project D Task 2 — real-kernel landlock e2e tests using the
//! controlled-env binary.
//!
//! Verifies that the native adapter installs a landlock V1 ruleset
//! that allows reads inside declared paths and blocks reads outside.
//!
//! These tests were originally drafted at priority-12 ship but removed
//! because Ubuntu's `/bin → /usr/bin` symlinks tripped landlock V1's
//! lack of symlink resolution. Sub-project B's
//! `resolve_symlinks_for_landlock` helper canonicalizes paths and adds
//! both the symlink and target to the ruleset; D re-introduces the
//! tests using the controlled-env binary for predictable I/O.

#![cfg(feature = "integration-tests")]
#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::Command;
use tau_ports::{Sandbox, SandboxPlan};
use tau_sandbox_native::NativeSandbox;
use tempfile::TempDir;

fn locate_controlled_env_bin() -> PathBuf {
    // CARGO_MANIFEST_DIR is `crates/tau-sandbox-native`; walk up to repo root.
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    let bin = workspace_root
        .join("crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env");
    if !bin.exists() {
        panic!(
            "controlled-env binary not found at {}. Run: cargo build --manifest-path crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml --release",
            bin.display()
        );
    }
    bin
}

fn plan_with_read_paths(paths: Vec<&str>) -> SandboxPlan {
    let path_array: Vec<serde_json::Value> = paths.iter().map(|p| serde_json::json!(p)).collect();
    serde_json::from_value(serde_json::json!({
        "capabilities": [{"kind": "fs.read", "paths": path_array}],
        "context": null,
        "limits": null,
    }))
    .expect("valid plan")
}

#[tokio::test]
async fn allowed_read_succeeds() {
    let tmp = TempDir::new().expect("tempdir");
    let allowed = tmp.path().join("allowed.txt");
    std::fs::write(&allowed, b"OK").unwrap();

    let plan = plan_with_read_paths(vec![tmp.path().to_str().unwrap()]);

    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "read")
       .env("TAU_FIXTURE_INPUT_PATH", &allowed);

    let sandbox = NativeSandbox::new();
    let _handle = sandbox.wrap_spawn(&plan, &mut cmd).await.expect("wrap_spawn");
    let output = cmd.output().expect("spawn");

    assert!(
        output.status.success(),
        "expected exit 0; got status={:?}, stdout={:?}, stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("READ_OK OK"),
        "expected READ_OK OK in stdout; got {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[tokio::test]
async fn blocked_read_returns_eacces() {
    let tmp = TempDir::new().expect("tempdir");
    // Write a file OUTSIDE the allowed path.
    let blocked_dir = TempDir::new().expect("blocked tempdir");
    let blocked_file = blocked_dir.path().join("secret.txt");
    std::fs::write(&blocked_file, b"SECRET").unwrap();

    // Plan only allows reads inside `tmp`, not `blocked_dir`.
    let plan = plan_with_read_paths(vec![tmp.path().to_str().unwrap()]);

    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "read")
       .env("TAU_FIXTURE_INPUT_PATH", &blocked_file);

    let sandbox = NativeSandbox::new();
    let _handle = sandbox.wrap_spawn(&plan, &mut cmd).await.expect("wrap_spawn");
    let output = cmd.output().expect("spawn");

    assert!(
        !output.status.success(),
        "expected non-zero exit; got status={:?}, stdout={:?}, stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("controlled-env-binary error"),
        "expected controlled-env error message; got stderr={stderr:?}"
    );
}

#[tokio::test]
async fn multiple_paths_all_landlocked() {
    let tmp_a = TempDir::new().expect("tempdir A");
    let tmp_b = TempDir::new().expect("tempdir B");
    let file_a = tmp_a.path().join("a.txt");
    let file_b = tmp_b.path().join("b.txt");
    std::fs::write(&file_a, b"A").unwrap();
    std::fs::write(&file_b, b"B").unwrap();

    let plan = plan_with_read_paths(vec![
        tmp_a.path().to_str().unwrap(),
        tmp_b.path().to_str().unwrap(),
    ]);

    // Read from B (second path).
    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "read")
       .env("TAU_FIXTURE_INPUT_PATH", &file_b);

    let sandbox = NativeSandbox::new();
    let _handle = sandbox.wrap_spawn(&plan, &mut cmd).await.expect("wrap_spawn");
    let output = cmd.output().expect("spawn");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("READ_OK B"));
}
```

- [ ] **Step 4: Create `tests/strict_seccomp.rs`**

```rust
//! Sub-project D Task 2 — real-kernel seccomp e2e tests.
//!
//! Verifies that the native adapter at Strict tier installs a seccomp
//! filter that SIGSYSes the child on syscalls outside the baseline +
//! capability-derived extensions.
//!
//! Reference: `crates/tau-sandbox-native/src/strict.rs::baseline_syscall_map`
//! (priority 12) for the baseline allow-list; `net.rs::extend_with_network_rules`
//! for the `Network(Http)` extension.

#![cfg(feature = "integration-tests")]
#![cfg(target_os = "linux")]

use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::Command;
use tau_ports::{Sandbox, SandboxPlan};
use tau_sandbox_native::NativeSandbox;

fn locate_controlled_env_bin() -> PathBuf {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    workspace_root
        .join("crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env")
}

fn plan_strict_no_network() -> SandboxPlan {
    serde_json::from_value(serde_json::json!({
        "capabilities": [],
        "context": null,
        "limits": null,
    }))
    .expect("valid plan")
}

fn plan_strict_with_network() -> SandboxPlan {
    serde_json::from_value(serde_json::json!({
        "capabilities": [{
            "kind": "net.http",
            "hosts": ["api.example.com"],
            "methods": ["GET"]
        }],
        "context": null,
        "limits": null,
    }))
    .expect("valid plan")
}

#[tokio::test]
async fn socket_blocked_without_network_capability() {
    let plan = plan_strict_no_network();

    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "open-socket");

    let sandbox = NativeSandbox::new();
    let _handle = sandbox.wrap_spawn(&plan, &mut cmd).await.expect("wrap_spawn");
    let output = cmd.output().expect("spawn");

    // seccomp at strict tier without Network(Http) capability should
    // SIGSYS the process on socket(). Signal exit, NO stdout.
    assert!(
        !output.status.success(),
        "expected non-zero/signal exit; got status={:?}, stdout={:?}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
    );
    assert!(
        output.stdout.is_empty() || !String::from_utf8_lossy(&output.stdout).contains("SOCKET_OK"),
        "expected no SOCKET_OK; got stdout={:?}",
        String::from_utf8_lossy(&output.stdout)
    );
    // signal() returns Some(sig) when the process was killed by a signal.
    if let Some(sig) = output.status.signal() {
        assert_eq!(
            sig,
            libc::SIGSYS,
            "expected SIGSYS (31); got signal {sig}"
        );
    }
    // Either signal-exit OR exit-1-from-EACCES is acceptable.
}

#[tokio::test]
async fn socket_allowed_with_network_capability() {
    let plan = plan_strict_with_network();

    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "open-socket");

    let sandbox = NativeSandbox::new();
    let _handle = sandbox.wrap_spawn(&plan, &mut cmd).await.expect("wrap_spawn");
    let output = cmd.output().expect("spawn");

    assert!(
        output.status.success(),
        "expected exit 0 with Network(Http) cap; got status={:?}, stdout={:?}, stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("SOCKET_OK"),
        "expected SOCKET_OK; got stdout={:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[tokio::test]
async fn baseline_syscalls_allowed() {
    // The default mode (no env vars) just emits CONTROLLED_ENV_OK,
    // exercising baseline syscalls (write, exit_group, etc.).
    // Strict tier with no extra caps must allow these.
    let plan = plan_strict_no_network();

    let mut cmd = Command::new(locate_controlled_env_bin());
    // No mode set → default; just emit CONTROLLED_ENV_OK.

    let sandbox = NativeSandbox::new();
    let _handle = sandbox.wrap_spawn(&plan, &mut cmd).await.expect("wrap_spawn");
    let output = cmd.output().expect("spawn");

    assert!(output.status.success(), "baseline syscalls must succeed");
    assert!(String::from_utf8_lossy(&output.stdout).contains("CONTROLLED_ENV_OK"));
}
```

- [ ] **Step 5: Create `tests/strict_net_filter.rs`**

```rust
//! Sub-project D Task 2 — real-kernel network-filter e2e tests.
//!
//! Verifies that with `Network(Http)` capability, the native adapter
//! allows socket creation. Per priority-12 v0.1 over-permissive design
//! (`net.rs::unshare_flags_for_plan`), the child inherits the parent's
//! netns when `Network(Http)` is present, so localhost connections work.
//!
//! True per-host filtering (nftables-in-netns) is sub-project F.
//! Until F lands, this test verifies only "socket allowed at all";
//! it does not verify that traffic is restricted to the declared hosts.

#![cfg(feature = "integration-tests")]
#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::Command;
use tau_ports::{Sandbox, SandboxPlan};
use tau_sandbox_native::NativeSandbox;

fn locate_controlled_env_bin() -> PathBuf {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir.parent().unwrap().parent().unwrap();
    workspace_root
        .join("crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env")
}

fn plan_with_network(hosts: Vec<&str>) -> SandboxPlan {
    let host_array: Vec<serde_json::Value> = hosts.iter().map(|h| serde_json::json!(h)).collect();
    serde_json::from_value(serde_json::json!({
        "capabilities": [{
            "kind": "net.http",
            "hosts": host_array,
            "methods": ["GET"]
        }],
        "context": null,
        "limits": null,
    }))
    .expect("valid plan")
}

#[tokio::test]
async fn localhost_socket_allowed_with_http_cap() {
    let plan = plan_with_network(vec!["127.0.0.1", "localhost"]);

    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "open-socket");

    let sandbox = NativeSandbox::new();
    let _handle = sandbox.wrap_spawn(&plan, &mut cmd).await.expect("wrap_spawn");
    let output = cmd.output().expect("spawn");

    assert!(
        output.status.success(),
        "socket() should succeed with Network(Http) cap; got status={:?}, stderr={:?}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("SOCKET_OK"));
}

#[tokio::test]
async fn external_host_socket_allowed_with_http_cap() {
    // V0.1 limitation: per-host filtering is not yet enforced; any host
    // in the cap list (including externally-reachable hosts) just means
    // "socket allowed at all".
    let plan = plan_with_network(vec!["api.example.com"]);

    let mut cmd = Command::new(locate_controlled_env_bin());
    cmd.env("TAU_FIXTURE_MODE", "open-socket");

    let sandbox = NativeSandbox::new();
    let _handle = sandbox.wrap_spawn(&plan, &mut cmd).await.expect("wrap_spawn");
    let output = cmd.output().expect("spawn");

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("SOCKET_OK"));
}
```

- [ ] **Step 6: Create `tests/strict_exec_gating.rs` (stub)**

```rust
//! Sub-project D Task 2 — real-kernel exec-gating e2e tests.
//!
//! `#[ignore]`'d stub. Per-command exec gating requires landlock V2
//! (kernel ≥ 5.19) which is sub-project E's scope. v0.1 has a no-op
//! stub in `exec.rs::extend_with_exec_rules` (priority 12 design).
//!
//! When sub-project E lands proper exec gating, this file gets real
//! test bodies that:
//! - Plan with `Capability::Process(Spawn { commands: ["echo"] })`
//! - Spawn controlled-env in `exec` mode with `TAU_FIXTURE_EXEC_CMD=/bin/echo`
//! - Assert the binary can exec /bin/echo
//! - Plan WITHOUT the `commands` allow-list, exec should fail

#![cfg(feature = "integration-tests")]
#![cfg(target_os = "linux")]

#[tokio::test]
#[ignore = "Per-command exec gating requires landlock V2 (kernel >= 5.19); pending sub-project E"]
async fn exec_blocked_without_process_capability() {
    // Will exercise: plan with no Process(Spawn) cap → exec fails.
}
```

- [ ] **Step 7: Verify per-task focused gates locally (macOS will SKIP runs but compile)**

```bash
cargo build -p tau-sandbox-native --features integration-tests --tests
cargo fmt --all -- --check
cargo clippy -p tau-sandbox-native --all-targets --features integration-tests -- -D warnings
```

All clean. Tests don't run on macOS (gated `cfg(target_os = "linux")`) but they compile.

- [ ] **Step 8: Commit**

```bash
git add crates/tau-sandbox-native/Cargo.toml \
        crates/tau-sandbox-native/tests/light_landlock.rs \
        crates/tau-sandbox-native/tests/strict_seccomp.rs \
        crates/tau-sandbox-native/tests/strict_net_filter.rs \
        crates/tau-sandbox-native/tests/strict_exec_gating.rs

git commit -m "test(sandbox-native): real-kernel e2e tests for landlock + seccomp + net

Sub-project D Task 2. Re-introduces the 5 e2e test files removed at
priority-12 ship, now that sub-project B's resolve_symlinks_for_landlock
helper resolved Ubuntu's /bin → /usr/bin issue.

New integration-tests Cargo feature gates the test bodies. All tests
gated cfg(feature = \"integration-tests\") + cfg(target_os = \"linux\");
compile cleanly on macOS/Windows but only run on Linux.

Tests use the controlled-env binary at
crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/
tau-controlled-env (built on demand by CI; Task 1 added mode dispatch).

Tests added (8 + 1 ignored):
- light_landlock.rs (3): allowed_read, blocked_read, multiple_paths
- strict_seccomp.rs (3): socket_blocked_without_cap, socket_allowed_with_cap,
                        baseline_syscalls_allowed
- strict_net_filter.rs (2): localhost_allowed, external_host_allowed
                            (per-host filtering deferred to sub-project F)
- strict_exec_gating.rs (1 ignored): pending landlock V2 in sub-project E

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 3: tau-plugin-compat::driver module

**Files:**
- Modify: `crates/tau-plugin-compat/src/lib.rs` (add `pub mod driver;`)
- Create: `crates/tau-plugin-compat/src/driver.rs` (~150 LOC + 5 unit tests)

**Goal:** new public driver module that wraps the high-level `tau_runtime::plugin_host::load_*` functions. Tests construct a synthetic `LockedPlugin`, call the driver, get back `Arc<dyn DynTool>` / `Arc<dyn DynLlmBackend>` / `Arc<dyn DynStorage>`, and invoke methods directly.

- [ ] **Step 1: Verify the public API of `tau_runtime::plugin_host`**

```bash
grep -n "^pub async fn load_" crates/tau-runtime/src/plugin_host/mod.rs
```

Expected output:
```
343:pub async fn load_llm_backend(
419:pub async fn load_tool(
515:pub async fn load_storage(
```

Read the function signatures to learn the exact arg types. (Implementer: do this; signatures may have evolved since plan was written.)

```bash
sed -n '343,360p' crates/tau-runtime/src/plugin_host/mod.rs
sed -n '419,436p' crates/tau-runtime/src/plugin_host/mod.rs
sed -n '515,532p' crates/tau-runtime/src/plugin_host/mod.rs
```

Each `load_*` function takes (verify exact names):
- `plugin: &LockedPlugin`
- `config: serde_json::Value`
- `trace_context: TraceContext`
- `options: PluginHostOptions`
- `sandbox_plan: Option<&SandboxPlan>`

Returns `Result<Arc<dyn DynTool>, RuntimeError>` (or DynLlmBackend / DynStorage).

- [ ] **Step 2: Add `pub mod driver;` to `crates/tau-plugin-compat/src/lib.rs`**

```rust
// Append near the end of lib.rs.
pub mod driver;
```

- [ ] **Step 3: Create `crates/tau-plugin-compat/src/driver.rs`**

```rust
//! Port-aware test driver for sub-project D's Layer 4 plugin compat
//! tests.
//!
//! Wraps the public `tau_runtime::plugin_host::load_{tool,llm_backend,storage}`
//! functions. Tests use this to spawn a real plugin under the resolved
//! sandbox adapter and invoke the high-level `DynTool` / `DynLlmBackend` /
//! `DynStorage` traits directly — no manual `Frame::Request` construction.
//!
//! # Why this exists
//!
//! Sub-project B's `tau plugin run --script` driver hardcoded the
//! handshake port to `LlmBackend`, breaking tool-port plugin tests
//! (commit a449c10 marked them `#[ignore]`'d). This module is the
//! port-aware replacement: callers specify the expected port via the
//! `LockedPlugin.manifest.provides` field (already part of the type).
//!
//! Internally this calls into `tau_runtime::plugin_host::load_*`
//! which themselves call `PluginProcess::spawn_and_handshake` (private).
//! Tests don't need raw `Frame::Request` access.

use std::sync::Arc;

use tau_domain::TraceContext;
use tau_pkg::LockedPlugin;
use tau_ports::SandboxPlan;
use tau_runtime::builder::{DynLlmBackend, DynStorage, DynTool};
use tau_runtime::plugin_host::{self, PluginHostOptions};
use tau_runtime::sandbox::SandboxAdapter;

/// Errors raised by driver helpers.
///
/// `#[non_exhaustive]`: future driver variants (e.g. for new ports) may
/// add variants without breaking callers.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum DriveError {
    /// `tau_runtime::plugin_host::load_*` returned a `RuntimeError`.
    #[error("plugin load failed: {0}")]
    LoadFailed(String),
    /// The plugin's port doesn't match what the caller expected.
    #[error("port mismatch: caller expected {expected:?}, plugin provides {actual:?}")]
    PortMismatch { expected: String, actual: String },
    /// A tool invocation returned an error.
    #[error("tool invocation failed: {0}")]
    ToolFailed(String),
    /// An LLM completion returned an error.
    #[error("llm completion failed: {0}")]
    LlmFailed(String),
    /// A storage call returned an error.
    #[error("storage call failed: {0}")]
    StorageFailed(String),
}

/// Construct test trace context. Each test call gets a fresh context;
/// use a synthetic run/agent/span ID stable enough for log correlation
/// but unique enough that parallel test runs don't conflate.
pub fn test_trace_context() -> TraceContext {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    TraceContext::new(
        format!("test-driver-run-{nanos}"),
        format!("test-agent-{nanos}"),
        format!("test-span-{nanos}"),
    )
}

/// Construct `PluginHostOptions` for a test, with the supplied sandbox
/// adapter and standard test timeouts.
pub fn test_plugin_host_options(adapter: Option<Arc<SandboxAdapter>>) -> PluginHostOptions {
    PluginHostOptions {
        sandbox_adapter: adapter,
        ..PluginHostOptions::default()
    }
}

/// Spawn a tool plugin under the given sandbox adapter and return the
/// `DynTool` handle for direct method invocation.
pub async fn spawn_tool_under_sandbox(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    adapter: Option<Arc<SandboxAdapter>>,
    sandbox_plan: Option<&SandboxPlan>,
) -> Result<Arc<dyn DynTool>, DriveError> {
    let trace = test_trace_context();
    let options = test_plugin_host_options(adapter);
    plugin_host::load_tool(plugin, config, trace, options, sandbox_plan)
        .await
        .map_err(|e| DriveError::LoadFailed(format!("{e:?}")))
}

/// Spawn an llm-backend plugin under the given sandbox adapter and
/// return the `DynLlmBackend` handle.
pub async fn spawn_llm_under_sandbox(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    adapter: Option<Arc<SandboxAdapter>>,
    sandbox_plan: Option<&SandboxPlan>,
) -> Result<Arc<dyn DynLlmBackend>, DriveError> {
    let trace = test_trace_context();
    let options = test_plugin_host_options(adapter);
    plugin_host::load_llm_backend(plugin, config, trace, options, sandbox_plan)
        .await
        .map_err(|e| DriveError::LoadFailed(format!("{e:?}")))
}

/// Spawn a storage plugin under the given sandbox adapter and return
/// the `DynStorage` handle.
pub async fn spawn_storage_under_sandbox(
    plugin: &LockedPlugin,
    config: serde_json::Value,
    adapter: Option<Arc<SandboxAdapter>>,
    sandbox_plan: Option<&SandboxPlan>,
) -> Result<Arc<dyn DynStorage>, DriveError> {
    let trace = test_trace_context();
    let options = test_plugin_host_options(adapter);
    plugin_host::load_storage(plugin, config, trace, options, sandbox_plan)
        .await
        .map_err(|e| DriveError::LoadFailed(format!("{e:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_error_display_includes_detail() {
        let err = DriveError::LoadFailed("plugin handshake timed out".to_string());
        let msg = format!("{err}");
        assert!(msg.contains("plugin handshake timed out"));
    }

    #[test]
    fn drive_error_is_non_exhaustive_via_match() {
        let err = DriveError::LoadFailed("e".to_string());
        match err {
            DriveError::LoadFailed(_)
            | DriveError::PortMismatch { .. }
            | DriveError::ToolFailed(_)
            | DriveError::LlmFailed(_)
            | DriveError::StorageFailed(_) => {}
        }
    }

    #[test]
    fn test_trace_context_unique_across_calls() {
        let a = test_trace_context();
        let b = test_trace_context();
        // run_id differs even when called close together (uses ns precision)
        // — flaky on ultra-fast systems? Acceptable for now; if flaky,
        // swap to UUID.
        assert!(
            a.run_id != b.run_id || a.span_id != b.span_id,
            "expected unique trace contexts; got a={a:?}, b={b:?}"
        );
    }

    #[test]
    fn test_plugin_host_options_carries_adapter() {
        let opts = test_plugin_host_options(None);
        assert!(opts.sandbox_adapter.is_none());
    }

    #[test]
    fn port_mismatch_error_displays_clearly() {
        let err = DriveError::PortMismatch {
            expected: "Tool".to_string(),
            actual: "LlmBackend".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("Tool"));
        assert!(msg.contains("LlmBackend"));
    }
}
```

(NOTE to implementer: if `TraceContext::new` signature differs from `(run_id, agent_id, span_id)` per HEAD, adjust `test_trace_context()`. Same for `PluginHostOptions::default()` field access — if `..default()` doesn't compile, build the struct field-by-field. Sub-project B's Task 2 had the same vigilance need.)

- [ ] **Step 4: Verify the driver builds and tests pass**

```bash
cargo build -p tau-plugin-compat
cargo test -p tau-plugin-compat --lib driver
```

Expected: 5 tests passed.

- [ ] **Step 5: Per-crate verification gates**

```bash
cargo build -p tau-plugin-compat
cargo clippy -p tau-plugin-compat --all-targets -- -D warnings
cargo fmt --all -- --check
```

All clean.

- [ ] **Step 6: Commit**

```bash
git add crates/tau-plugin-compat/src/lib.rs \
        crates/tau-plugin-compat/src/driver.rs

git commit -m "feat(plugin-compat): port-aware test driver in tau-plugin-compat::driver

Sub-project D Task 3. New public module wrapping
tau_runtime::plugin_host::load_{tool,llm_backend,storage} for use by
Layer 4 plugin compat tests.

Three spawn helpers:
- spawn_tool_under_sandbox(plugin, config, adapter, plan) -> Arc<dyn DynTool>
- spawn_llm_under_sandbox(...)                            -> Arc<dyn DynLlmBackend>
- spawn_storage_under_sandbox(...)                        -> Arc<dyn DynStorage>

Tests construct a synthetic LockedPlugin, call the appropriate spawn fn
with a SandboxAdapter, then invoke trait methods directly (no raw
Frame::Request needed; the high-level Dyn traits already provide
typed call/complete/get/put/etc. methods).

#[non_exhaustive] DriveError with 5 variants (LoadFailed, PortMismatch,
ToolFailed, LlmFailed, StorageFailed).

5 unit tests cover error display, non-exhaustive discipline, trace
context uniqueness, options shape, and port-mismatch rendering.

Tasks 5/6/7 use this driver to flip 7 of the 10 #[ignore]'d Layer 4
plugin compat tests. The 3 container × HTTP plugin tests stay ignored
(sub-project F — needs proper netns network filtering).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 4: tau-runtime integration-tests feature + sandbox_native.rs

**Files:**
- Modify: `crates/tau-runtime/Cargo.toml` (add `[features] integration-tests = []`)
- Create: `crates/tau-runtime/tests/sandbox_native.rs` (~150 LOC, 2 tests)

**Summary:** add `integration-tests` feature to `tau-runtime`. Create one new integration test file at `tests/sandbox_native.rs` that exercises `plugin_host` integration with the native adapter using the controlled-env binary as a stand-in for a real plugin.

Two tests:
1. `adapter_threads_through_to_plugin_spawn` — construct a `Runtime` with a native adapter, call `load_*` against the controlled-env binary, expect `RuntimeError::PluginHandshakeFailed` (the binary doesn't speak the IPC protocol, so handshake fails — but the failure proves the adapter ran wrap_spawn before the handshake attempt).
2. `sandbox_plan_validation_runs_pre_spawn` — construct a `SandboxPlan` with a `Custom` shape the native adapter doesn't support; call `load_*`; expect `SandboxError::ShapeUnsupported` from the validator BEFORE spawn.

Both tests gated `cfg(feature = "integration-tests")` + `cfg(target_os = "linux")`.

**Spec references:** Spec §2.2; Component 2.2; data flow §3.1.

**Verification (focused gate):**

```bash
cargo build -p tau-runtime --features integration-tests --tests
cargo clippy -p tau-runtime --all-targets --features integration-tests -- -D warnings
cargo fmt --all -- --check
```

(Tests don't run on macOS due to `cfg(target_os = "linux")`; Linux CI exercises them.)

**Commit message:**
```
test(runtime): real-kernel runtime e2e tests for plugin_host + native adapter

Sub-project D Task 4. New integration-tests Cargo feature on tau-runtime.
New tests/sandbox_native.rs with 2 tests verifying that:

1. The native sandbox adapter is correctly threaded through plugin_host's
   load_* functions; wrap_spawn fires before handshake; expected handshake
   failure (controlled-env doesn't speak IPC) confirms adapter ran first.
2. SandboxPlan validation runs pre-spawn, rejecting unsupported capability
   shapes before any process spawn.

cfg(feature = "integration-tests") + cfg(target_os = "linux") gated.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

---

## Task 5: Layer 4 native test flips — tool plugins (shell + fs-read)

**Files:**
- Modify: `crates/tau-plugin-compat/tests/layer4_native.rs` (flip 2 `#[ignore]`'d tests; add helper functions; total ~200 LOC delta)

**Summary:** flip `#[ignore]` on `shell_layer4_native_runs_echo_hello` and `fs_read_layer4_native_reads_data_file`. Test bodies use the Task 3 driver (`driver::spawn_tool_under_sandbox`) to invoke `shell.call({command: "echo", args: ["hello"]})` and `fs-read.call({path: <data.txt>})` respectively.

Each test:
1. Synthesize-install the plugin into a tempdir (mirror Task 6 of sub-project B's pattern in `crates/tau-plugin-compat/tests/layer3_check_sandbox.rs`).
2. Resolve the native adapter via `tau_runtime::sandbox::resolve_adapter_forced(SandboxAdapterKind::Native, ...)`.
3. Build a SandboxPlan from the plugin's manifest capabilities.
4. Call `driver::spawn_tool_under_sandbox(&locked_plugin, config, Some(adapter), Some(&plan))`.
5. Call `dyn_tool.call(session_context, json!({...}))` to invoke the tool method.
6. Assert exit + expected stdout substring.

Helper extracted to `tests/common/native_helpers.rs` (or just at top of `layer4_native.rs`): `build_session_context()`, `build_native_adapter_or_skip()`, `synthesize_plugin_install(scope, plugin_name)`.

**Spec references:** Spec §3.2 (test harness flow); §3.3 (adapter resolution).

**Verification (focused gate):**

```bash
cargo build -p tau-plugin-compat --features integration-tests --tests
cargo test -p tau-plugin-compat --features integration-tests --test layer4_native shell_layer4_native_ fs_read_layer4_native_
```

(2 tests run on Linux CI; macOS skips per `cfg(target_os = "linux")`.)

**Commit message:**
```
test(plugin-compat): flip Layer 4 native tests for tool plugins (shell + fs-read)

Sub-project D Task 5. Flips #[ignore] on shell_layer4_native_runs_echo_hello
and fs_read_layer4_native_reads_data_file. Tests use Task 3's
driver::spawn_tool_under_sandbox to invoke real tool methods under the
native landlock+seccomp adapter on Linux CI.

These exercise:
- resolve_adapter_forced(SandboxAdapterKind::Native, ...)
- driver::spawn_tool_under_sandbox + DynTool::call
- The native adapter's wrap_spawn pipeline end-to-end
- Sub-project B Task 3's resolve_symlinks_for_landlock fix
  (echo and the data fixture are resolved through symlinks on Ubuntu)

2 of 10 sub-project B Layer 4 #[ignore]'s flipped.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

---

## Task 6: Layer 4 container test flips — tool plugins (shell + fs-read)

**Files:**
- Modify: `crates/tau-plugin-compat/tests/layer4_container.rs` (flip 2 `#[ignore]`'d tests; ~100 LOC delta — mostly mirrors Task 5)

**Summary:** same pattern as Task 5 but using the Container adapter. Force adapter via `resolve_adapter_forced(SandboxAdapterKind::Container, ...)`. The driver and assertions are otherwise identical.

Skip-with-message if Docker isn't available on the host (already-existing `require_docker()` helper from sub-project B).

**Spec references:** Spec §3.2; §3.3.

**Verification (focused gate):**

```bash
cargo test -p tau-plugin-compat --features integration-tests --test layer4_container shell_layer4_container_ fs_read_layer4_container_
```

(2 tests run on Linux CI with Docker; skip-with-message elsewhere.)

**Commit message:**
```
test(plugin-compat): flip Layer 4 container tests for tool plugins

Sub-project D Task 6. Flips #[ignore] on shell_layer4_container_runs_echo_hello
and fs_read_layer4_container_reads_data_file. Same pattern as Task 5
but with SandboxAdapterKind::Container forced.

Skip-with-message if Docker daemon isn't running.

4 of 10 sub-project B Layer 4 #[ignore]'s flipped (Tasks 5+6).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

---

## Task 7: Layer 4 native HTTP plugin tests via cassette-replay

**Files:**
- Modify: `crates/tau-plugin-compat/tests/layer4_native.rs` (flip 3 `#[ignore]`'d HTTP plugin tests; ~250 LOC delta — cassette-replay setup + driver use)
- Modify: `crates/tau-plugin-compat/Cargo.toml` (add `wiremock` dev-dep if not already in workspace deps)
- Modify: `Cargo.lock` (transitives for `wiremock`)
- Modify: `crates/tau-plugin-compat/tests/layer4_container.rs` (update the 3 HTTP plugin test `#[ignore]` rationales to point clearly at sub-project F)

**Summary:** flip `#[ignore]` on the 3 native HTTP plugin tests (`anthropic_layer4_native_completes_via_cassette`, `ollama_*`, `openai_*`). Each test:
1. Loads the corresponding cassette from `crates/tau-plugins/<plugin>/tests/cassettes/complete_happy_path.yaml`.
2. Spins up `wiremock::MockServer::start()` on `127.0.0.1:<random_port>`; registers the cassette as a Mock.
3. Synthesize-installs the plugin into a tempdir.
4. Resolves the native adapter; builds a SandboxPlan with `Network(Http)` capability listing `127.0.0.1` and `localhost` in hosts.
5. Configures the plugin's HTTP base URL via env var on the spawn (e.g. `ANTHROPIC_API_BASE=http://127.0.0.1:<port>`) — this requires extending `PluginHostOptions` or passing through `LockedPlugin.config` (verify with implementer; existing `config` parameter on `load_*` likely accepts env vars).
6. Calls `driver::spawn_llm_under_sandbox` → gets `Arc<dyn DynLlmBackend>`.
7. Invokes `dyn_llm.complete(request).await` with a synthetic completion request.
8. Asserts the response matches the cassette's expected outcome.

The 3 container HTTP plugin tests in `layer4_container.rs` keep their `#[ignore]` attribute but their rationale string is updated:

```rust
#[test]
#[ignore = "Container netns isolation: localhost cassette server not reachable from container without sub-project F's nftables-in-netns work. See ADR-0017 Decision 3."]
fn anthropic_layer4_container_completes_via_cassette() { ... }
```

(Same for ollama and openai container variants.)

**Spec references:** Spec §3.3; Decision 3.

**Verification (focused gate):**

```bash
cargo build -p tau-plugin-compat --features integration-tests --tests
cargo test -p tau-plugin-compat --features integration-tests --test layer4_native anthropic_layer4_native_ ollama_layer4_native_ openai_layer4_native_
```

(3 tests run on Linux CI; macOS skips.)

**Commit message:**
```
test(plugin-compat): flip Layer 4 native HTTP plugin tests via cassette-replay

Sub-project D Task 7. Flips #[ignore] on the 3 native-adapter HTTP
plugin tests (anthropic + ollama + openai). Each test spins up a
wiremock::MockServer on localhost, registers the cassette from the
corresponding plugin's tests/cassettes/ directory, configures the
plugin's HTTP base URL via env var, then drives DynLlmBackend::complete
through the Task 3 driver under the native adapter.

The native adapter's v0.1 over-permissive netns inheritance (when
Network(Http) is in the plan; per priority-12 net.rs design) makes
localhost reachable from the sandboxed plugin process.

The 3 container × HTTP plugin tests stay #[ignore]'d with updated
rationale pointing to sub-project F (per-host network filtering via
nftables-in-netns is needed for cassette-server-on-host to be
reachable from the container netns).

7 of 10 sub-project B Layer 4 #[ignore]'s flipped (Tasks 5+6+7).

wiremock added as dev-dep on tau-plugin-compat. Cargo.lock staged.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

---

## Task 8: docs/reference/sandbox-platform-support.md

**Files:**
- Create: `docs/reference/sandbox-platform-support.md` (~50 LOC markdown)

**Summary:** new reference doc documenting the kernel features required by the native sandbox adapter, the tested distros, and known limitations. Spec §2's component table specifies the exact content (kernel ≥ 5.13 for landlock V1, user namespaces ≥ 4.18, seccomp BPF ubiquitous; Ubuntu 22.04+ on CI; sub-project F tracks per-host network filtering, sub-project E tracks per-command exec gating; macOS/Windows tracked as J/K).

**Spec references:** Spec §2.5 (controlled-env binary update mentions documentation deliverable); §5.6.

**Verification:** none (markdown only). `cargo fmt --all -- --check` to verify no formatting drift in adjacent code (none expected).

**Commit message:**
```
docs(reference): sandbox platform support matrix

Sub-project D Task 8. New docs/reference/sandbox-platform-support.md
documenting:
- Required kernel features (landlock V1 ≥ 5.13, user namespaces ≥ 4.18,
  seccomp BPF)
- Tested distros (Ubuntu 22.04+ on CI)
- Known limitations cross-referencing sub-projects E (per-command exec)
  and F (per-host network filter)
- macOS/Windows pending sub-projects J/K

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

---

## Task 9: CI workflow updates

**Files:**
- Modify: `.github/workflows/ci.yml` (add 2 new Linux jobs ~30 LOC delta)

**Summary:** add two new Linux-only CI jobs:

```yaml
test-tau-sandbox-native-e2e:
  name: test (tau-sandbox-native e2e / linux)
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - name: Build controlled-env binary
      run: cargo build --manifest-path crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml --release
    - name: Test tau-sandbox-native e2e
      run: cargo test -p tau-sandbox-native --features integration-tests --tests

test-tau-runtime-e2e:
  name: test (tau-runtime e2e / linux)
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - name: Build controlled-env binary
      run: cargo build --manifest-path crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml --release
    - name: Test tau-runtime e2e
      run: cargo test -p tau-runtime --features integration-tests --tests
```

The existing `test (tau-plugin-compat / linux)` job runs the 7 newly-flipped Layer 4 tests as part of its existing test invocation — no changes needed there.

**Branch protection migration:** the 2 new check names surface on Task 10's PR push; user manually adds them to GitHub branch protection settings (27 → 29 required checks).

**Spec references:** Spec §5.3; Decision 5.

**Verification:** YAML lint (`yamllint .github/workflows/ci.yml` if installed; otherwise visual). The CI run on Task 10's PR push validates execution.

**Commit message:**
```
ci: add tau-sandbox-native + tau-runtime e2e Linux test jobs

Sub-project D Task 9. Two new Linux-only CI jobs:

- test (tau-sandbox-native e2e / linux): builds controlled-env binary,
  runs cargo test -p tau-sandbox-native --features integration-tests --tests
- test (tau-runtime e2e / linux): same shape for tau-runtime

Branch protection rises 27 → 29 required checks. The 2 new check names
need a manual GitHub-settings update after Task 10's PR push.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
```

---

## Task 10: USER GATE — final verification + open PR

**Type:** PAUSE for user approval.

The implementer:
1. Runs the full local verification suite on the latest commit:
   - `cargo fmt --all -- --check`
   - `cargo build --workspace`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace --all-targets`
   - `cargo test --workspace --doc`
   - On Linux additionally:
     - `cargo test -p tau-sandbox-native --features integration-tests --tests`
     - `cargo test -p tau-runtime --features integration-tests --tests`
     - `cargo test -p tau-plugin-compat --features integration-tests --tests`
2. Pushes the branch: `git push -u origin feat/e2e-landlock-spec`.
3. Drafts a PR body in `/tmp/<branch-name>-pr-body.md` (avoid heredoc pitfalls).
4. Opens the PR: `gh pr create --draft --base main --head feat/e2e-landlock-spec --title "feat: end-to-end landlock CI integration + port-aware Layer 4 driver (sub-project D)" --body-file <tempfile>`.
5. Surfaces the new CI check names emitted by `gh pr checks <PR#>`. The user adds these to GitHub branch protection (Settings → Branches → main → Required status checks).
6. Waits for CI green on the new branch protection set.
7. PAUSES; reports status; waits for user "Task 10 ok" before Task 11.

**No commit.** This task is verification + push + draft PR open.

---

## Task 11: USER GATE — ADR-0017 + ROADMAP + followups + squash-merge

**Type:** PAUSE for user approval.

**Files:**
- Create: `docs/decisions/0017-e2e-landlock-and-driver.md` (full ADR body covering 5 D-decisions: single sub-project, driver = wrapper around plugin_host's public API, 7-of-10 flips, original test locations, 3 separate CI jobs)
- Modify: `ROADMAP.md` — add row `12-D` (sub-project D — e2e landlock CI integration); update status paragraph to mention 2026-05-05 ship date, 7-of-10 ignored Layer 4 tests flipped, 3 deferred to sub-project F
- Modify: `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` — mark D done with honest "what diverged" notes; update sub-project F's section to flag F as the unblocker for the 3 remaining ignored tests

The implementer commits these doc updates as a single commit. CI re-runs (~10-15 minutes for the 29 checks). Once green, the user squash-merges via:

```bash
gh pr merge <PR#> --squash --delete-branch \
   --subject "feat: end-to-end landlock CI integration + port-aware Layer 4 driver (sub-project D from priority-12 followups) (#<N>)" \
   --body-file /tmp/<branch-name>-merge-body.md
```

**No implementer-only commit on this task.** This task is ADR + ROADMAP + followups + waiting for CI + user-driven squash-merge.

After merge:
- `git checkout main && git pull origin main`
- Branch deleted by `--delete-branch`.

---

## Self-review

**Spec coverage:**
- Section 1 (scope + architecture): all 4 deliverables covered (5 e2e files → Task 2; driver → Task 3; 7-of-10 flips → Tasks 5+6+7; CI → Task 9). The platform-support doc → Task 8.
- Section 2 (components): controlled-env mode dispatch → Task 1; tau-sandbox-native feature + tests → Task 2; tau-runtime feature + test → Task 4; driver module → Task 3; layer4_*.rs flips → Tasks 5+6+7; ci.yml → Task 9. All present.
- Section 3 (data flow): adapter e2e flow exercised by Task 2's tests; plugin-compat driver flow by Tasks 5+6+7; HTTP cassette-replay flow by Task 7.
- Section 4 (error handling): `DriveError` defined in Task 3; test failure rendering codified in Task 2's test bodies; cassette-replay error paths covered by `wiremock` semantics.
- Section 5 (CI): Task 9 adds the 2 new jobs; existing `test (tau-plugin-compat / linux)` runs the flipped tests automatically.

**Placeholder scan:** no "TBD", "TODO", "fill in details" in plan body. Stub test in Task 2 (`strict_exec_gating.rs`) is intentionally `#[ignore]`'d with a rationale — that's a real ship-state, not a placeholder. Task 4 does NOT include the test code verbatim (hybrid format) — the implementer constructs from the spec references; this is the documented Task 4-9 hybrid pattern, not a placeholder.

**Type consistency:** `LockedPlugin`, `SandboxPlan`, `SandboxAdapter`, `DynTool`/`DynLlmBackend`/`DynStorage`, `PluginHostOptions`, `TraceContext` referenced consistently across Tasks 3, 4, 5, 6, 7. `DriveError` 5 variants consistent across Task 3 (definition) and 5/6/7 (usage). `TAU_FIXTURE_MODE` env var and its 4 modes consistent across Task 1 (definition) and Tasks 2/4 (usage).

**Branch protection delta:** stated 27 → 29 in plan-erratum + Task 9 + Task 10. 2 new jobs (sandbox-native e2e + runtime e2e) add 2 new check names. The existing `test (tau-plugin-compat / linux)` is reused, not duplicated.

**Sub-project F cross-reference:** the 3 deferred container HTTP plugin tests are flagged in Task 7 (rationale strings) and Task 11 (followups doc update) as needing sub-project F. Consistent.

Plan complete.

---

Plan complete and saved to `docs/superpowers/plans/2026-05-05-e2e-landlock.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
