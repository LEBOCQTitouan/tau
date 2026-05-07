# Sandbox Proxy — Design (replaces F's veth+nft per-host filtering)

**Status:** Proposed
**Date:** 2026-05-07
**Authors:** Titouan Lebocq
**Supersedes:** [ADR-0019 — Per-host network filter](../decisions/0019-per-host-network-filter.md) (when adopted)
**Closes:** F task 6.5 follow-up #1 (Container-adapter network filtering), F task 6.5 follow-up #2 (strict_net_filter integration test hang)

## Goal

Replace tau's current per-host network filter (Linux veth + nftables, requiring `CAP_NET_ADMIN` in the parent process) with a userspace HTTP-CONNECT proxy. The proxy runs unprivileged in tau's parent process; the plugin runs inside an empty network namespace and reaches the proxy via a small bridge process.

This eliminates four pain points simultaneously:

1. **Privileged-Docker requirement in CI** — strict-tier sandbox tests no longer need any kernel-level capabilities
2. **strict_net_filter integration test hang** — no veth+nft setup means no kernel interaction with seccomp `KillProcess`; tests stop hanging
3. **Container-adapter HTTP plugin tests `#[ignore]`'d** — proxy works through Docker bind-mounts; the 3 stuck tests become runnable
4. **Production privilege requirement** — tau drops from "must run as root or with `CAP_NET_ADMIN`" to "any unprivileged user"

The pattern is well-established: Anthropic's own `sandbox-runtime` (Oct 2025, open source) uses bubblewrap + empty netns + Unix-socket bridge + HTTP-CONNECT proxy. Tau's design is the same shape with a Rust implementation.

## Scope

**In scope (this iteration):**

- Native sandbox adapter (Linux strict tier) — replace veth+nft with proxy + bridge
- Container sandbox adapter (Docker-based) — same proxy pattern, bind-mounted into the container
- Delete F's `tau-sandbox-native::net_filter` module entirely (~640 LOC + 26 unit tests)
- Delete F task 6.5's sync-pipe machinery (`SandboxHandle::sync_write_fd`, `signal_post_spawn_complete`, etc.) — no longer needed
- Replace 4 `#[ignore]`'d `strict_net_filter.rs` tests with new `strict_proxy.rs` tests that actually run on every PR
- Un-`#[ignore]` 3 `layer4_container.rs` HTTP plugin tests (anthropic / ollama / openai cassette-replay)
- Update CI: remove the `test-net-filter / linux` privileged-Docker job; tests run on stock `ubuntu-latest`
- Update docs: ADR-0020 supersedes ADR-0019; ROADMAP 12-F entry rewritten

**Out of scope (deferred):**

- Method-level or path-level enforcement on HTTP requests (pass-through CONNECT proxy can't see plaintext; future TLS-terminating proxy if richer capability schema lands)
- Non-HTTP egress (raw TCP, UDP, QUIC). No current plugin uses them. Future plugins would need a different design or extend the proxy
- Light tier proxy support. Light tier doesn't isolate networking today; this iteration doesn't change that
- Cross-platform sandbox (Windows, macOS native). The empty-netns part is intrinsically Linux-only. macOS would need Seatbelt + similar proxy plumbing; Windows AppContainer + similar. Separate sub-projects

## Constraints

- **All FOSS.** No Tart, no proprietary tooling
- **Production-like in tests.** Tests run real spawn, real netns, real seccomp — same shape as production
- **Existing capability schema preserved.** `Network(Http) { hosts: Vec<String>, methods: Vec<String> }` stays as-is. Proxy enforces hosts; methods declared but unenforced (same as today)
- **Apple Silicon dev host compatibility.** No design assumption rules out arm64 dev workflows; the proxy is portable; integration tests run in Linux containers (existing infrastructure)

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│ Parent process (tau, unprivileged user)                          │
│                                                                  │
│  • create temp Unix socket file: /tmp/tau-proxy-XXXXXX.sock      │
│  • spawn proxy tokio task listening on that path                 │
│  • build child Command:                                          │
│      tau-net-bridge --proxy-sock=/tmp/X.sock --listen=127:8443   │
│         -- <plugin-binary> <plugin-args>                         │
│  • set HTTPS_PROXY=http://127.0.0.1:8443 on cmd env              │
│  • landlock allow-list += proxy socket path (read+write)         │
│  • cmd.spawn() — pre_exec does landlock + unshare + seccomp      │
│                                                                  │
│  ┌────────────────────────────────────────────────────────┐      │
│  │ proxy tokio task (in tau's address space)              │      │
│  │  • accept Unix-socket connections from the bridge      │      │
│  │  • parse CONNECT host:port                             │      │
│  │  • validate host in plan.allowed_hosts                 │      │
│  │  • peek TLS ClientHello, extract SNI                   │      │
│  │  • verify SNI == CONNECT host                          │      │
│  │  • open TCP to host:443, splice both ways              │      │
│  └────────────────────────────────────────────────────────┘      │
│                                                                  │
│  ┌─────────────────────────────────────────────────────┐         │
│  │ Plugin process tree                                 │         │
│  │ (in empty netns + landlock + seccomp)               │         │
│  │                                                     │         │
│  │   tau-net-bridge (forked parent of plugin)          │         │
│  │     • bring lo up via rtnetlink                     │         │
│  │     • bind TCP listener on 127.0.0.1:8443           │         │
│  │     • for each accept: connect to proxy socket,     │         │
│  │       splice TCP ↔ Unix-socket                      │         │
│  │                                                     │         │
│  │     fork()                                          │         │
│  │       └─ plugin (reqwest, etc.)                     │         │
│  │            HTTPS_PROXY=http://127.0.0.1:8443        │         │
│  │            → bridge → proxy → real internet         │         │
│  └─────────────────────────────────────────────────────┘         │
└──────────────────────────────────────────────────────────────────┘
```

**Components:**

- **`tau-sandbox-native::proxy`** — module with the HTTP-CONNECT proxy logic. Tokio task in the parent's address space. ~150 LOC.
- **`tau-net-bridge`** — small standalone binary (`[[bin]]` target). Brings `lo` up in the netns, listens on TCP loopback, splices to the inherited Unix socket file. ~100 LOC.
- **Plugin** — existing plugin binaries unchanged. Standard HTTP clients (reqwest, hyper, ureq) honor `HTTPS_PROXY` env var.
- **Empty netns** — `unshare(CLONE_NEWUSER | CLONE_NEWNET)` (already done by tau today; no change). Child has no network interfaces except `lo` (which the bridge brings up).
- **Landlock allow-list** — the proxy socket path is added to the strict tier's read+write paths so the bridge can dial it.

**Why three components and not two:**

reqwest / hyper / ureq don't natively support `unix:/path` proxy URLs. They expect `http://host:port`. The bridge translates "TCP-on-loopback inside the netns" (which they DO support) to "Unix-socket connection in the parent's filesystem" (which the parent's proxy task reads from). Without the bridge, every plugin's HTTP client would need a custom unix-socket connector — invasive and breaks the "any HTTP client just works" property.

## Locked decisions

### Decision 1 — Proxy pattern, not eBPF or microVM

**Decision:** Use a userspace HTTP-CONNECT proxy + empty netns + bridge. Reject eBPF (`cgroup_skb`, BPF LSM `socket_connect`) and microVM (Firecracker, libkrun) alternatives.

**Rationale:** eBPF approaches still require `CAP_NET_ADMIN` (cgroup attach) or `CAP_BPF + CAP_PERFMON` (BPF LSM) — they don't eliminate the privilege requirement. MicroVM approaches need `/dev/kvm` access (group `kvm`) which is as constraining for CI as the current setup. The proxy pattern is the only one that achieves true unprivileged execution. Production precedent: Anthropic `sandbox-runtime`, Matchlock, gVisor, bubblewrap.

### Decision 2 — Replace F entirely, not augment

**Decision:** Delete `tau-sandbox-native::net_filter` module wholesale. Don't keep F as a fallback for non-HTTP capabilities.

**Rationale:** No current plugin uses non-HTTP egress. Keeping F as a fallback adds maintenance overhead for no immediate value. If a future plugin needs raw TCP / UDP egress, that's a separate sub-project — likely with a different proxy variant (TCP transparent proxy) or a microVM approach. Don't pre-build for hypothetical future requirements.

### Decision 3 — Pass-through CONNECT proxy, not TLS-terminating

**Decision:** The proxy parses the `CONNECT host:port` line, peeks the TLS ClientHello to verify SNI, but does NOT terminate TLS. Plugin's TLS handshake goes end-to-end with the real remote server.

**Rationale:** Pass-through CONNECT preserves the current enforcement level (host-only — same as F's nft rules today). TLS-terminating would let us enforce method/path/headers, but adds CA-cert distribution complexity, breaks cert pinning, and creates test/prod skew. Tau's `Network(Http)` capability spec lists methods but the current implementation never enforces them at the kernel level either; pass-through doesn't regress this. Future option to upgrade to TLS-terminating if richer capabilities land.

### Decision 4 — Native + Container in same iteration

**Decision:** This iteration covers BOTH the Native adapter (replacing F's machinery) AND the Container adapter (Docker bind-mount of the proxy socket). Single PR closes both F task 6.5 follow-up gap rows.

**Rationale:** The Container side is small (a bind-mount + env var added to ContainerSandbox's spawn). Splitting into two PRs leaves the Container HTTP plugin tests `#[ignore]`'d for an extra cycle with no benefit.

### Decision 5 — Sync-pipe machinery deleted entirely

**Decision:** Remove `SandboxHandle::sync_write_fd`, `with_sync_write_fd`, `sync_write_fd_value`, `signal_post_spawn_complete`. Remove the `pipe(2)` + blocking-read step in the strict-tier pre_exec closure. Remove the runtime caller in `tau-runtime::plugin_host::process` that signals post-spawn.

**Rationale:** F's sync pipe existed because the parent had to do privileged setup AFTER the child started but BEFORE the child could run normally. The proxy pattern moves all setup to BEFORE the spawn (proxy task starts; HTTPS_PROXY env set). There's nothing for the child to wait on; nothing for the parent to do after spawn. Net code removal: ~80 LOC plus one trait field plus runtime caller.

`Sandbox::apply_post_spawn` trait method itself stays as a default no-op (reserved for future adapters that might need post-spawn work; not used by Native/Container/Mock/Light/Passthrough in this iteration). `SandboxHandle::nest_handle` stays — the proxy task guard nests via this same mechanism.

### Decision 6 — Bridge as `[[bin]]` target, host-binary bind-mount in Container adapter

**Decision:** `tau-net-bridge` is a `[[bin]]` target inside `tau-sandbox-native` (or its own crate `tau-net-bridge` — implementation choice; not load-bearing). Container adapter bind-mounts the host's bridge binary into the container at a known path (`/usr/local/bin/tau-net-bridge`); plugin Dockerfiles need nothing tau-specific.

**Rationale:** Plugin authors should not need to bake tau-specific binaries into their images. Tau's installer produces a `tau-net-bridge` binary; the Container adapter bind-mounts it. Cross-arch concern: if a plugin's container is x86_64 Linux but the host is arm64, tau must build the bridge for the target arch; defer to packaging story.

### Decision 7 — Strict allow-list (no wildcards), 443 only, SNI ↔ CONNECT enforcement

**Decision:** Allow-list lookup is exact-match only. Reject CONNECT to any port other than 443. Verify SNI in the TLS ClientHello matches the CONNECT host; mismatch → close connection.

**Rationale:** Carries forward F's existing `validate::validate_hosts` policy (no wildcards, no IP literals except 127.0.0.1). Port 443 only matches the spirit of `Network(Http)` in 2026 (effectively HTTPS). SNI/CONNECT match closes the domain-fronting hole. If a future plugin legitimately needs non-443 ports or non-HTTPS, extend the capability schema; don't widen proxy defaults.

## Files

### Created

- `crates/tau-sandbox-native/src/proxy/mod.rs` — proxy task entry point, allow-list types, lifecycle (~50 LOC)
- `crates/tau-sandbox-native/src/proxy/connect.rs` — HTTP CONNECT request parser, SNI peek, splice loop (~100 LOC)
- `crates/tau-sandbox-native/src/proxy/validate.rs` — allow-list validation (carried forward from F's `net_filter::validate`, lightly adapted)
- `crates/tau-sandbox-native/src/bin/tau-net-bridge.rs` — bridge binary (~100 LOC)
- `crates/tau-sandbox-native/tests/strict_proxy.rs` — Layer 4 integration tests (5 tests; replaces strict_net_filter.rs)
- `docs/decisions/0020-sandbox-proxy.md` — supersedes ADR-0019

### Deleted

- `crates/tau-sandbox-native/src/net_filter/` (entire directory):
  - `mod.rs` — apply_per_host_filter orchestrator
  - `error.rs` — NetFilterError
  - `exec.rs` — CommandExecutor abstraction
  - `handle.rs` — NetFilterHandle
  - `netns.rs` — VethSubnet, allocate_subnet, setup_veth_pair_with_subnet
  - `probe.rs` — probe_prerequisites
  - `resolve.rs` — DNS resolution
  - `rules.rs` — nft DSL generator
  - `validate.rs` — moved to `proxy/validate.rs`
- `crates/tau-sandbox-native/tests/strict_net_filter.rs` — 4 `#[ignore]`'d tests
- F's `apply_post_spawn` integration tests in tau-runtime (any specific to veth setup; verify during implementation)

### Modified

- **`crates/tau-ports/src/sandbox.rs`** — drop `SandboxHandle::sync_write_fd: Option<OwnedFd>`, `with_sync_write_fd`, `sync_write_fd_value`, `signal_post_spawn_complete`. Keep `apply_post_spawn` trait method (default no-op). Keep `nest_handle`. Rename `SandboxError::NetFilter` → `SandboxError::Proxy` (or keep generic; small naming choice during implementation).
- **`crates/tau-sandbox-native/src/lib.rs`** — `NativeSandbox.apply_post_spawn` override: drop `veth_subnets` HashMap field, drop `cached_net_filter_probe`, replace with proxy spawn + bridge wiring (or move all of this to `wrap_spawn` since post-spawn has nothing to do; default override applies).
- **`crates/tau-sandbox-native/src/strict.rs`** — `apply_strict` pre-spawn:
  - Drop `VethSubnet` pre-allocation
  - Drop `TAU_NET_PARENT_VETH_IP` env var
  - Drop `pipe(2)` sync pipe creation
  - Drop the blocking-read step from the pre_exec closure
  - For `Network(Http)` plans: create temp Unix socket file, spawn proxy task, wrap `Command` with `tau-net-bridge`, set `HTTPS_PROXY` env, add proxy socket path to landlock allow-list
- **`crates/tau-sandbox-native/src/net.rs`** — `unshare_flags_for_plan` UNCHANGED (still returns `CLONE_NEWUSER | CLONE_NEWNET`)
- **`crates/tau-runtime/src/plugin_host/process.rs`** — drop the `signal_post_spawn_complete()` call after spawn
- **`crates/tau-sandbox-container/src/lib.rs`** (or wherever the Container adapter lives) — `wrap_spawn` for `Network(Http)` plans: create proxy socket, spawn proxy task, build docker run with `-v <host-sock>:/run/tau-proxy.sock`, `-v <bridge-binary>:/usr/local/bin/tau-net-bridge`, `-e HTTPS_PROXY=http://127.0.0.1:8443`, wrap container's entrypoint with `tau-net-bridge`
- **`crates/tau-plugin-compat/tests/layer4_container.rs`** — un-`#[ignore]` the 3 HTTP plugin tests; rewire them to use the proxy bind-mount instead of the veth IP shenanigans
- **`.github/workflows/ci.yml`** — delete `test-net-filter / linux` job. Add `strict_proxy.rs` runs to existing `test-stable / linux` and `tau-sandbox-native e2e / linux` matrix entries
- **`ROADMAP.md`** — rewrite 12-F entry: proxy approach replaces veth+nft; drop "PARTIAL"
- **`docs/superpowers/specs/2026-05-03-sandboxing-followups.md`** — close both F task 6.5 follow-up gap rows; add a single closed row for "proxy adoption complete"
- **`docs/decisions/0019-per-host-network-filter.md`** — addendum: superseded by ADR-0020

### Cargo.lock

Adds `rtnetlink` (or raw netlink via `nix`) for `lo` up. May add `tokio` features (`net`, `io-util`) if not already enabled. Removes any F-specific dependencies if present (none likely; F shelled out to `ip`/`nft`).

## Execution model

| Phase | What happens | Where |
|---|---|---|
| `validate_plan` | host syntax check (no wildcards); accept Network(Http) | parent, before fork |
| `wrap_spawn` | proxy task spawned, `cmd` wrapped with bridge, `HTTPS_PROXY` env set, landlock paths extended | parent, before fork |
| `cmd.spawn()` → `pre_exec` | landlock + unshare + seccomp | child, before exec |
| child binary started | `tau-net-bridge` is the actual binary that runs | child |
| bridge brings lo up | rtnetlink call inside the netns | bridge |
| bridge forks plugin | plugin process becomes a child of the bridge | child of bridge |
| plugin makes HTTPS request | reqwest reads `HTTPS_PROXY`, dials `127.0.0.1:8443` (the bridge) | plugin |
| bridge accepts connection, dials proxy socket | bridge → proxy | bridge ↔ parent proxy task |
| proxy parses CONNECT, validates allow-list, peeks SNI | proxy task | parent proxy task |
| proxy opens TCP to remote, splices | parent proxy task | parent |
| plugin exits | bridge's `waitpid` returns; bridge exits with same code | bridge |
| `Child::wait` returns | tau sees the bridge's exit status | parent |
| `SandboxHandle::Drop` | proxy task aborted, temp `.sock` file unlinked | parent |

## Testing strategy

**Layer 1 — unit tests** (in `src/`, fast, no spawn):

`tau-sandbox-native::proxy::connect`:
- `parse_connect_request_well_formed`
- `reject_non_connect_methods`
- `reject_missing_port`
- `tls_client_hello_sni_extraction`
- `tls_no_sni_rejected`
- `allow_list_exact_match` — `api.anthropic.com` matches; `api.anthropic.com.evil.com` does not
- `allow_list_port_mismatch` — port 80 rejected even if host allowed

`tau-sandbox-native::proxy::mod`:
- `proxy_task_drop_unlinks_socket_file`
- `403_returned_for_disallowed_host`
- `sni_mismatch_closes_connection`

`tau-net-bridge`:
- `parses_args_correctly`
- `lo_brought_up_via_rtnetlink` — verified inside a test netns or mocked
- `tcp_listener_binds_loopback`
- `unix_socket_connect_to_proxy_path`
- `splice_bidirectional`
- `plugin_exit_status_propagated`

**Layer 4 — integration tests** (real spawn, real netns, real plugins):

`tau-sandbox-native/tests/strict_proxy.rs`:
- `localhost_socket_allowed_with_http_cap` — child reaches a test cassette server via proxy; SOCKET_OK in stdout
- `external_host_blocked_when_not_in_allowlist` — child's CONNECT to non-allowed host returns 403
- `no_network_cap_socket_denied_by_seccomp` — seccomp blocks `socket(2)` (SIGSYS = 31)
- `proxy_handle_drop_cleans_up_temp_socket` — `.sock` file unlinked after handle drop
- `sni_mismatch_rejected` — SNI ≠ CONNECT host → connection terminated

`tau-plugin-compat/tests/layer4_container.rs`:
- Un-`#[ignore]` the 3 HTTP plugin tests (anthropic / ollama / openai cassette-replay)
- Use Container adapter's proxy bind-mount; bind cassette server on `0.0.0.0:0`; plan's hosts list `127.0.0.1`; HTTPS_PROXY in container env routes through to the host

## CI changes

**Before:**
- `test-net-filter / linux` job (privileged Docker, custom apt-install, `--no-run` on integration tests, ~2 min)
- 15 required CI checks
- 7 sandbox tests `#[ignore]`'d

**After (optimistic case):**
- `test-net-filter / linux` deleted entirely
- `strict_proxy.rs` tests run in `test-stable / linux` (regular `ubuntu-latest`; no Docker, no caps)
- `layer4_container.rs` HTTP tests run in `test-tau-plugin-compat / linux` (regular Docker, no `--privileged`, no caps)
- 14 required CI checks (one fewer)
- 0 sandbox tests `#[ignore]`'d

**Fallback case** (if stock `ubuntu-latest` blocks unprivileged user namespaces in 2026):
- Replace `test-net-filter / linux` with `test-sandbox / linux`: Docker container with `--cap-add SYS_ADMIN` only (uid_map writes), no `NET_ADMIN`
- Document the fallback in this spec as an addendum once verified

**Verification during implementation:** run `unshare -Urn whoami` on a stock GHA `ubuntu-latest` runner. If it prints `root`, optimistic case applies. If it errors, fallback case applies.

## What this enables

- F task 6.5 follow-up #1 (Container-adapter network filtering) — closed; the 3 HTTP plugin tests un-`#[ignore]`'d and runnable on stock CI
- F task 6.5 follow-up #2 (`strict_net_filter.rs` integration test hang) — closed; the 4 tests rewritten as `strict_proxy.rs`, no veth-seccomp interaction
- The `cfg(unix)` class of bug — caught at commit time IF a separate dev-environment iteration ships (out of scope here)
- Privilege drift in CI — moot; no caps used at all
- Production deploys — drop from "must run as root or with `CAP_NET_ADMIN`" to "any unprivileged user"
- Cross-platform e2e dev story — the empty-netns part is still Linux-only, but the proxy itself is portable. macOS could grow a Seatbelt + same proxy implementation in a future sub-project; design unblocks that direction.

## References

- [ADR-0019 — Per-host network filter](../decisions/0019-per-host-network-filter.md): the design this supersedes
- ADR-0020 — Sandbox proxy: the new ADR (created in T1)
- Anthropic sandbox-runtime: production precedent (bubblewrap + empty netns + Unix-socket bridge + HTTP-CONNECT proxy)
- Matchlock: per-agent microVM with transparent proxy (alternative C considered and rejected)
- Landlock kernel docs: network access control roadmap (port-only in v4; no IP filtering in sight)
- BPF cgroup_skb: alternative B considered (still requires CAP_NET_ADMIN)
- Tau's existing capability schema: `Network(Http) { hosts: Vec<String>, methods: Vec<String> }` — see `crates/tau-domain/src/capability.rs`

## Out of scope (explicit)

- TLS-terminating proxy (would let us enforce method/path; future iteration if richer capabilities land)
- Non-HTTP egress (raw TCP, UDP, QUIC) — no current plugin uses; future iteration if needed
- Light tier proxy support — no networking isolation today; out of scope here
- Cross-platform sandbox (Windows AppContainer, macOS Seatbelt) — separate sub-projects
- Local dev environment / pre-push gate — separate sub-project (the paused `feat/dev-environment` branch); orthogonal to this work
