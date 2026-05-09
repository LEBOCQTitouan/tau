# Windows AppContainer adapter — design

> **Status:** spec, executing inline. Cuts from main at `597db89` (PR #45 sandbox-darwin).

## Goal

Add a fourth `Sandbox` adapter for Windows hosts that enforces tau's strict-tier capability model via Microsoft's AppContainer + the existing `tau-sandbox-proxy`. Windows users (CI runners + future Windows dev users) get the same security envelope as Linux + macOS strict.

## Development constraint (locked)

**This adapter cannot be developed locally on macOS.** Iteration is push-to-CI only (~5-7 min per `windows-latest` GHA cycle). Reasons:

- AppContainer is a Windows-kernel primitive (Windows 8+); no equivalent on macOS or Linux.
- Wine is incomplete for AppContainer — does not faithfully reproduce the kernel sandboxing.
- The lefthook pre-push gate's `cargo check --target x86_64-pc-windows-gnu` step catches syntax / cfg-gating mistakes; runtime behavior is verified only on CI.

**Future work to unlock local dev:** UTM + Windows 11 ARM VM (deferred from the dev-environment ADR).

## Locked decisions

| # | Decision |
|---|---|
| 1 | New crate `crates/tau-sandbox-windows`. `cfg(target_os = "windows")`-gated runtime; pure-logic modules compile everywhere. |
| 2 | Use AppContainer (Windows 8+, modern). Implemented via the `windows` crate's Win32 bindings (MIT/Apache-2; FOSS-compliant). |
| 3 | Strict tier via existing `tau-sandbox-proxy`. AppContainer profile **omits** `internetClient`; sets `HTTPS_PROXY=http://127.0.0.1:8443`; allows loopback via `privateNetworkClientServer` capability. Same defense-in-depth as macOS. |
| 4 | Filesystem access via ACL grants on plan-specified paths (AppContainer SID + GENERIC_READ / GENERIC_WRITE). Revoked by `SandboxHandle` drop. |
| 5 | Process spawn via `CreateProcessAsUserW` with `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` and `STARTUPINFOEXW`. |
| 6 | AppContainer profile created per-spawn (unique name from process ID + counter), removed by `SandboxHandle` drop. |
| 7 | Runtime registry: `RegistryKind::Native` resolves to `WindowsSandbox` on Windows; existing `NativeSandbox` (Linux) and `DarwinSandbox` (macOS) unchanged. |

## Components

**NEW**
- `crates/tau-sandbox-windows/Cargo.toml` — depends on `windows` crate, `tau-ports`, `tau-domain`, `tau-sandbox-proxy`.
- `crates/tau-sandbox-windows/src/lib.rs` — `WindowsSandbox` impl `Sandbox`. `wrap_spawn` builds AppContainer profile, grants ACLs, replaces `Command`.
- `crates/tau-sandbox-windows/src/profile.rs` — pure `build_appcontainer_caps(plan: &SandboxPlan) -> AppContainerCaps`. Compiles + unit-tested on any platform.
- `crates/tau-sandbox-windows/src/acl.rs` — Win32 ACL grant/revoke helpers; runtime cfg-gated to Windows.
- `crates/tau-sandbox-windows/src/spawn.rs` — `CreateProcessAsUserW` wrapper; cfg-gated to Windows.
- `crates/tau-sandbox-windows/tests/strict_integration.rs` — Windows-only Layer 4 integration tests.

**MODIFIED**
- Workspace `Cargo.toml` — new member.
- `crates/tau-runtime/src/sandbox/resolver.rs` — new `SandboxAdapter::Windows` variant cfg-gated to Windows.
- `crates/tau-runtime/Cargo.toml` — target-specific dep for `cfg(target_os = "windows")`.
- `crates/tau-cli/tests/cmd_no_sandbox_flag.rs` — `sandbox_native_on_windows_errors_clearly` deleted (Windows now succeeds).
- `docs/decisions/0022-sandbox-windows.md` (new ADR).

## Architecture

```
HOST (Windows)
─ Plugin host
─ tau-sandbox-windows::wrap_spawn
   ├─ generates AppContainer SID + capabilities from SandboxPlan
   ├─ grants ACL on plan-specified FS paths to the AppContainer SID
   ├─ spawns proxy task on 127.0.0.1:8443 (HTTP plans)
   ├─ CreateProcessAsUserW with PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES
   └─ SandboxHandle drop: revoke ACLs + delete AppContainer profile
─ Proxy task (host's network namespace, 127.0.0.1:8443)
   ├─ accepts connections from sandboxed plugin
   ├─ validates Host against allowlist
   └─ opens TCP to real remote
```

## Risks & mitigations

| Risk | Mitigation |
|---|---|
| AppContainer programming complexity | Mirror macOS adapter's structure (lib.rs orchestrates; profile.rs pure; spawn.rs / acl.rs Win32 wrappers). One crate; small surface. |
| ACL grants leak after crash | `SandboxHandle::drop` revokes; per-spawn unique SIDs scope leaks. |
| `windows-latest` runner lacks an API | Documented in ADR; integration tests can `#[ignore]` per missing primitive. |
| CI-only iteration is slow | Accepted; UTM Windows VM is the future unlock. |

## Phasing

1. Cut `feat/sandbox-windows` from main
2. Scaffold crate + workspace member
3. `build_appcontainer_caps` pure function + unit tests
4. ACL grant/revoke helpers (cfg-gated)
5. `CreateProcessAsUserW` spawn wrapper (cfg-gated)
6. `WindowsSandbox` `Sandbox` trait impl
7. Layer 4 integration tests (Windows-only)
8. Runtime registry wiring
9. ADR-0022 + remove obsolete test
10. USER GATE — open PR, watch CI
11. USER GATE — squash-merge

## Verification

- `cargo nextest run -p tau-sandbox-windows --features integration-tests --tests` passes on `windows-latest` GHA runner.
- `cargo check --target x86_64-pc-windows-gnu --workspace` passes locally on macOS.
- No regressions in Linux / macOS adapters.
- Plugin spawned under `WindowsSandbox` reaches a cassette server via the proxy.

## Out of scope

- Windows local dev environment (UTM + Windows 11 ARM VM) — separate sub-project.
- Per-syscall filtering on Windows — no equivalent of seccomp.
- Old Job Objects approach — superseded by AppContainer.
- WSL2 + Linux native — defeats the "native Windows sandboxing" goal.
