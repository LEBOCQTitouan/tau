# macOS sandbox-exec adapter — design

> **Status:** spec, executing inline. Cuts from main at `f341366` (PR #44).

## Goal

Add a third `Sandbox` adapter for macOS hosts that enforces tau's strict-tier capability model via Apple's `sandbox-exec` + the existing `tau-sandbox-proxy` crate. Plugin developers on macOS (and macOS CI runners) get the same security envelope as Linux strict.

## Locked decisions

| # | Decision |
|---|---|
| 1 | New crate `crates/tau-sandbox-darwin` (parallel to `tau-sandbox-native` + `tau-sandbox-container`). Pure macOS — `cfg(target_os = "macos")`-gated; on other platforms, probe returns `Unavailable`. |
| 2 | Strict tier via existing `tau-sandbox-proxy` (host-side proxy task; plugin's reqwest routes via `HTTPS_PROXY=http://127.0.0.1:8443`). |
| 3 | SBPL profile restricts outbound network to `127.0.0.1:8443` only — defense-in-depth. |
| 4 | Profile generated dynamically per `SandboxPlan`. Written to a temp file; `sandbox-exec -f <profile> <plugin>`. |
| 5 | Network plans validated via `validate_hosts` from `tau-sandbox-proxy` (no IPs except loopback, no wildcards). |
| 6 | Probe checks `/usr/bin/sandbox-exec` exists; returns `Available { tier: Strict }` on macOS, `Unavailable` elsewhere. |

## Architecture

```
HOST (macOS)
─ Plugin host
─ tau-sandbox-darwin::wrap_spawn
   ├─ generates SBPL profile from SandboxPlan
   │  • file-read* / file-write* per FS capability
   │  • process-exec per ProcessExec
   │  • network-outbound only to localhost:8443 (HTTP plans)
   │  • deny default
   ├─ spawns proxy task on 127.0.0.1:8443
   ├─ writes profile to /tmp/<random>.sb
   └─ replaces cmd with: sandbox-exec -f <profile> <orig-cmd> <orig-args>
─ Proxy task on host's network namespace
   ├─ accepts connections from sandboxed plugin via 127.0.0.1:8443
   ├─ validates Host against allowlist
   └─ opens TCP to real remote (DNS resolved from host)
```

## Components

**NEW**
- `crates/tau-sandbox-darwin/Cargo.toml` — depends on `tau-ports`, `tau-domain`, `tau-sandbox-proxy`, `tokio`.
- `crates/tau-sandbox-darwin/src/lib.rs` — `DarwinSandbox` struct + `Sandbox` impl.
- `crates/tau-sandbox-darwin/src/profile.rs` — `build_sbpl_profile(plan: &SandboxPlan) -> String`. Pure; unit-testable on any platform.
- `crates/tau-sandbox-darwin/src/baseline.rs` — SBPL baseline string (system paths libc / dyld bootstrap need).

**MODIFIED**
- `Cargo.toml` (workspace) — add to `members`.
- `crates/tau-runtime/src/sandbox/registry.rs` — register `DarwinSandbox` for `RegistryKind::Native` on macOS.
- `.github/workflows/ci.yml` — new `test (tau-sandbox-darwin / macos)` job.

**TEST INFRASTRUCTURE**
- `crates/tau-sandbox-darwin/tests/strict_proxy.rs` — macOS-only Layer 4 integration test mirroring `tau-sandbox-native/tests/strict_proxy.rs`.

## Risks

| Risk | Mitigation |
|---|---|
| sandbox-exec deprecated | Apple still ships and supports it. Worst case: switch to App Sandbox + entitlements (separate sub-project). |
| Baseline allowlist drift across macOS versions | Empirical discovery; lock paths in `baseline.rs`. Tests fail loudly if a new macOS version breaks bootstrap. |
| SBPL profile parse errors | `sandbox-exec` rejects invalid profiles → `SandboxError::WrapFailed` with parser message. |

## Out of scope

- Windows AppContainer adapter (separate sub-project).
- App Sandbox + entitlements (covers production-grade signed-app sandboxing; deferred until forced by signing requirements).
- Per-syscall filtering on macOS (no equivalent of seccomp readily available; sandbox-exec's coarse model is what we get).

## Verification

- `cargo nextest run -p tau-sandbox-darwin --features integration-tests --tests` passes on macOS local + CI macOS runner.
- No regressions in Linux native / container adapters.
- Plugin spawned under `DarwinSandbox` reaches a cassette server via the proxy (mirrors layer4 integration tests).
