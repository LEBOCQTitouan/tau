# ADR-0022: macOS sandbox-exec adapter

**Status:** Accepted
**Date:** 2026-05-09
**Deciders:** Titouan Lebocq
**Related:** [ADR-0014 — Sandboxing](0014-sandboxing.md), [ADR-0020 — Sandbox proxy](0020-sandbox-proxy.md)

## Context

Through ADR-0014..0021 the strict-tier sandbox shipped on Linux only
(landlock + seccomp + namespaces, layered with the userspace proxy from
ADR-0020). On macOS, `RegistryKind::Native` resolved to "no adapter",
so plugins fell back to `passthrough` — the same as `--no-sandbox`. The
followups doc had reserved this as sub-project J ("macOS sandbox-exec
adapter").

macOS does not expose a syscall-level filter analogous to seccomp, but
it does ship `/usr/bin/sandbox-exec` and the SBPL (Sandbox Profile
Language) S-expression dialect. The `tau-sandbox-proxy` crate from
ADR-0020 is platform-agnostic for the parent-side accept loop, so the
Linux-side defense-in-depth (proxy gates outbound HTTP) ports to macOS
unchanged.

## Decision

Add `tau-sandbox-darwin`, a third `Sandbox` adapter parallel to
`tau-sandbox-native` and `tau-sandbox-container`. Strict tier only;
Light tier deliberately omitted because SBPL has no equivalent of
landlock's per-path read/write granularity short of strict mode.

Architecture:

- `cfg(target_os = "macos")`-gated runtime; pure-logic modules
  (`profile.rs`, `baseline.rs`) compile on any platform so unit tests
  run everywhere.
- `wrap_spawn` builds an SBPL profile from the `SandboxPlan`, writes it
  to `/tmp/tau-darwin-<pid>-<n>.sb`, then replaces the original command
  with `sandbox-exec -f <profile> <orig-cmd>`.
- Network plans validated through `tau-sandbox-proxy::validate_hosts`
  (no IPs except loopback, no wildcards). Profile permits outbound
  network only to `127.0.0.1:8443` — the proxy port. Plugin's reqwest
  routes via `HTTPS_PROXY=http://127.0.0.1:8443`.
- SBPL baseline (`baseline.rs::SBPL_BASELINE`) covers libc / dyld
  bootstrap (`process*`, `mach-lookup`, `sysctl-read`, `/usr/lib`
  subpath, etc.). `process*` rather than narrower verbs because
  `sandbox-exec` rejects with "execvp() failed: Operation not
  permitted" when limited to `process-fork`.
- Runtime registry: `instantiate(RegistryKind::Native)` returns
  `DarwinSandbox` on macOS, `NativeSandbox` on Linux, `Unavailable`
  elsewhere.

## Consequences

Positive:

- macOS plugin spawns now run under a real OS-level sandbox, matching
  Linux strict-tier security envelope (modulo per-syscall filtering,
  which macOS lacks).
- Defense-in-depth network containment via the same proxy as Linux —
  one allowlist surface, not two.
- `tau-sandbox-darwin` is small (~370 LOC of runtime + 270 LOC of
  pure profile generation with 7 unit tests + 4 macOS integration
  tests).

Negative:

- `sandbox-exec` is officially deprecated by Apple. App Sandbox +
  entitlements is the modern path but requires a signed `.app` bundle.
  Acceptable: Apple still ships and supports `sandbox-exec`; if/when
  that changes we migrate to App Sandbox in a separate sub-project.
- SBPL baseline allowlist may drift across macOS versions; locked in
  `baseline.rs` and tests fail loudly if a new macOS version breaks
  bootstrap.
- No per-syscall filtering — the SBPL coarse model is what we get.

## References

- Spec: `docs/superpowers/specs/2026-05-09-sandbox-darwin-design.md`
- PR: #45 (`feat(sandbox-darwin): macOS sandbox-exec adapter (strict tier via SBPL)`)
- Commit: `597db89`
