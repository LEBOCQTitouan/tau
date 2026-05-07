# Sandbox Proxy Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace tau's veth+nft+CAP_NET_ADMIN per-host network filtering with a userspace HTTP-CONNECT proxy + small bridge binary, eliminating the privileged-Docker requirement and unblocking the 7 `#[ignore]`'d sandbox tests.

**Architecture:** A tokio task in tau's parent address space accepts CONNECT requests on a temp Unix socket file. The strict-tier child runs in `unshare(CLONE_NEWUSER | CLONE_NEWNET)` — empty netns, no internet. A small `tau-net-bridge` binary forks: parent of fork brings `lo` up, listens on `127.0.0.1:8443`, splices to the inherited proxy socket; child of fork runs the actual plugin. Plugin's standard HTTP client honors `HTTPS_PROXY=http://127.0.0.1:8443`. Pass-through CONNECT (proxy doesn't terminate TLS); SNI must match CONNECT host.

**Tech Stack:** tokio (async I/O + task spawning), `rtnetlink` (bring `lo` up inside the netns), seccompiler/landlock (existing strict-tier machinery, unchanged), Docker (Container adapter — existing).

**Branch:** `feat/sandbox-proxy` (already cut from main; spec committed at `16bc746`)

**Spec reference:** `docs/superpowers/specs/2026-05-07-sandbox-proxy-design.md` — 7 locked decisions (proxy not eBPF/microVM, replace F entirely, pass-through CONNECT, Native+Container same iteration, sync-pipe deleted, bridge as `[[bin]]`, strict allow-list 443-only SNI-checked).

**Plan-erratum carryovers (apply preemptively):**
- VERIFY against BASE_SHA = `16bc746` before claiming "pre-existing failure"
- Cargo.lock will be touched (rtnetlink dep added in T4); stage in same commit
- Per-task focused gate (single-crate / single-test); not full workspace test for non-Rust changes
- `unshare -Urn whoami` verification at T1; if it fails, switch to fallback CI shape
- `#[non_exhaustive]` on new public types (`ProxyHandle`)
- Container adapter location confirmed: `crates/tau-sandbox-container/{lib.rs,probe.rs,runner.rs}`

**Implementer prerequisites:**

The implementer needs `cargo-nextest` installed (`cargo install cargo-nextest --locked`). All other deps come from the workspace toolchain.

For T7-T8 verification on a Linux host (not strictly required during implementation; CI will catch issues), the implementer can use the existing privileged-Docker job until T9 deletes it, OR run on a real Linux machine.

---

## File structure

| File | Action | Responsibility |
|---|---|---|
| `crates/tau-sandbox-native/src/net_filter/` (entire dir) | DELETE | F's veth+nft machinery; replaced by proxy |
| `crates/tau-sandbox-native/tests/strict_net_filter.rs` | DELETE | F's 4 #[ignore]'d integration tests; replaced by strict_proxy.rs |
| `crates/tau-ports/src/sandbox.rs` | MODIFY | Drop `SandboxHandle::sync_write_fd`/`with_sync_write_fd`/`sync_write_fd_value`/`signal_post_spawn_complete` |
| `crates/tau-ports/src/error.rs` | MODIFY | Rename `SandboxError::NetFilter` → `SandboxError::Proxy` |
| `crates/tau-sandbox-native/src/proxy/mod.rs` | CREATE | Proxy task entry point, `ProxyHandle` lifecycle |
| `crates/tau-sandbox-native/src/proxy/connect.rs` | CREATE | HTTP CONNECT parser, SNI peek, splice loop |
| `crates/tau-sandbox-native/src/proxy/validate.rs` | CREATE | Allow-list validation (carried forward from `net_filter::validate`) |
| `crates/tau-sandbox-native/src/bin/tau-net-bridge.rs` | CREATE | Bridge binary (fork; listen on `127.0.0.1:8443`; splice to inherited Unix socket) |
| `crates/tau-sandbox-native/Cargo.toml` | MODIFY | Add `rtnetlink` dep + `[[bin]]` target |
| `crates/tau-sandbox-native/src/lib.rs` | MODIFY | Drop F-specific NativeSandbox fields; drop `apply_post_spawn` override (default no-op) |
| `crates/tau-sandbox-native/src/strict.rs` | MODIFY | `apply_strict`: drop sync-pipe + veth alloc; add proxy spawn + bridge wrap |
| `crates/tau-sandbox-native/src/net.rs` | UNCHANGED | `unshare_flags_for_plan` already returns `CLONE_NEWUSER \| CLONE_NEWNET` |
| `crates/tau-runtime/src/plugin_host/process.rs` | MODIFY | Drop `signal_post_spawn_complete()` call after spawn |
| `crates/tau-sandbox-container/src/runner.rs` | MODIFY | Add proxy bind-mount + HTTPS_PROXY env + bridge wrap on Network(Http) plans |
| `crates/tau-sandbox-native/tests/strict_proxy.rs` | CREATE | 5 Layer 4 integration tests (replaces strict_net_filter.rs) |
| `crates/tau-plugin-compat/tests/layer4_container.rs` | MODIFY | Un-#[ignore] the 3 HTTP plugin tests |
| `.github/workflows/ci.yml` | MODIFY | Delete `test-net-filter / linux` job; add `strict_proxy` to existing matrix |
| `docs/decisions/0020-sandbox-proxy.md` | CREATE | New ADR superseding 0019 |
| `docs/decisions/0019-per-host-network-filter.md` | MODIFY | Add "superseded by ADR-0020" addendum |
| `ROADMAP.md` | MODIFY | Rewrite 12-F entry; drop "PARTIAL" |
| `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` | MODIFY | Close both F task 6.5 follow-up gap rows |

---

## Task 1: Verify GHA unprivileged-userns capability + delete F's net_filter module

**Files:**
- Verify: stock GHA `ubuntu-latest` allows `unshare -Urn`
- Delete: `crates/tau-sandbox-native/src/net_filter/` (entire directory)
- Delete: `crates/tau-sandbox-native/tests/strict_net_filter.rs`
- Modify: `crates/tau-sandbox-native/src/lib.rs` (drop `pub mod net_filter;` declaration)

**What this delivers:** Confirms the optimistic CI case (no caps needed); removes ~640 LOC of F's machinery in one focused deletion. The build will fail at this point because other files still reference deleted symbols — that's intentional; subsequent tasks fix them.

- [ ] **Step 1: Verify unprivileged user namespaces work on stock GHA**

The implementer SHOULD verify this by writing a 1-time CI smoke test or checking GHA documentation. For local verification (Apple Silicon Mac doesn't help here; user namespaces are Linux-only):

If you have access to a Linux box, run:
```bash
unshare -Urn whoami
```
Expected output: `root` (means unprivileged user namespaces work).

If you don't have a Linux box: trust the spec's optimistic case for now. CI will reveal at T11 (PR open) whether the assumption holds. If T11's CI shows the strict_proxy tests failing on stock `ubuntu-latest`, fall back to the spec's "fallback case" (Docker container with `--cap-add SYS_ADMIN` only).

- [ ] **Step 2: Delete the entire net_filter directory**

```bash
rm -rf crates/tau-sandbox-native/src/net_filter/
rm crates/tau-sandbox-native/tests/strict_net_filter.rs
```

- [ ] **Step 3: Drop the module declaration in lib.rs**

Use the Edit tool. Find the line declaring the module and remove it. The exact line in `crates/tau-sandbox-native/src/lib.rs` should look like:

```rust
mod net_filter;
```

or possibly:

```rust
pub(crate) mod net_filter;
```

Remove it. Also remove any `use crate::net_filter::...` imports in lib.rs.

- [ ] **Step 4: Verify the build fails — surfacing the call sites**

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo build -p tau-sandbox-native --all-targets 2>&1 | tail -30
```

Expected: build FAILS with errors pointing at:
- `crates/tau-sandbox-native/src/strict.rs` references to `net_filter::netns::*`, `net_filter::apply_per_host_filter`
- `crates/tau-sandbox-native/src/lib.rs` references to `net_filter::NetFilterError` / `cached_net_filter_probe` / `veth_subnets`
- `crates/tau-ports/src/error.rs` `SandboxError::NetFilter` variant still exists (still compiles in tau-ports; will be addressed in T2)

The list of errors at this step is the input to T2-T5.

- [ ] **Step 5: Commit (do NOT run other gates yet — they will fail)**

```bash
git add -A crates/tau-sandbox-native/src/ crates/tau-sandbox-native/tests/
git commit --no-verify -m "feat(sandbox-native): delete net_filter module (F replaced by proxy)

Per ADR-0020, F's veth+nft+CAP_NET_ADMIN per-host filter is replaced by
a userspace HTTP-CONNECT proxy. This commit deletes the entire
tau-sandbox-native::net_filter module (~640 LOC + 26 unit tests + 4
integration tests). Build is INTENTIONALLY broken at this commit;
subsequent commits in this PR add the proxy + bridge replacement and
fix all call sites.

The four #[ignore]'d strict_net_filter integration tests are deleted
wholesale; they will be replaced by strict_proxy.rs in T7.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

**Note:** Use `--no-verify` because this commit intentionally breaks the build. Lefthook is not yet wired in this branch; if it ever is, this commit must bypass.

---

## Task 2: Drop sync-pipe machinery from `tau-ports`; rename SandboxError variant

**Files:**
- Modify: `crates/tau-ports/src/sandbox.rs` (`SandboxHandle` struct + impls)
- Modify: `crates/tau-ports/src/error.rs` (`SandboxError::NetFilter` → `SandboxError::Proxy`)

**What this delivers:** Trait-level cleanup. After this task, `SandboxHandle` no longer carries the sync-pipe FD or signal method; `SandboxError::Proxy` replaces `NetFilter`. Callers in tau-runtime + tau-sandbox-native still need updating (T3-T6).

- [ ] **Step 1: Read current SandboxHandle struct + impl**

```bash
sed -n '/^pub struct SandboxHandle/,/^}/p' crates/tau-ports/src/sandbox.rs
sed -n '/^impl SandboxHandle/,/^impl Drop/p' crates/tau-ports/src/sandbox.rs
```

Expected current state (verified): SandboxHandle has `cleanup`, `sync_write_fd` (cfg(unix)), `nested` fields. Impl has `new`, `noop`, `with_sync_write_fd`, `sync_write_fd_value`, `nest_handle`, `signal_post_spawn_complete` methods.

- [ ] **Step 2: Edit SandboxHandle struct — drop sync_write_fd field**

Use Edit tool with this exact transformation:

old_string:
```rust
pub struct SandboxHandle {
    cleanup: Option<Box<dyn FnOnce() + Send + 'static>>,
    #[cfg(unix)]
    sync_write_fd: Option<std::os::fd::OwnedFd>,
    nested: Vec<Box<dyn Send>>,
}
```

new_string:
```rust
pub struct SandboxHandle {
    cleanup: Option<Box<dyn FnOnce() + Send + 'static>>,
    nested: Vec<Box<dyn Send>>,
}
```

- [ ] **Step 3: Drop `with_sync_write_fd`, `sync_write_fd_value`, `signal_post_spawn_complete` methods**

Find each method body in the `impl SandboxHandle { ... }` block (use grep to locate exact lines):

```bash
grep -n "with_sync_write_fd\|sync_write_fd_value\|signal_post_spawn_complete" crates/tau-ports/src/sandbox.rs
```

Delete all 3 methods. Keep `new`, `noop`, `nest_handle`.

- [ ] **Step 4: Update `SandboxHandle::new` and `noop` to drop the sync_write_fd init**

old_string (in `new`):
```rust
        Self {
            cleanup: Some(Box::new(cleanup)),
            #[cfg(unix)]
            sync_write_fd: None,
            nested: Vec::new(),
        }
```

new_string:
```rust
        Self {
            cleanup: Some(Box::new(cleanup)),
            nested: Vec::new(),
        }
```

Same for `noop()` — drop the `sync_write_fd: None` line.

- [ ] **Step 5: Update Drop impl — drop the explicit close of sync_write_fd**

Find the `impl Drop for SandboxHandle` block. Remove any code that touches `self.sync_write_fd`. The OwnedFd field's Drop already closed the fd; we don't need explicit cleanup. After this step, Drop should run cleanup() then drop nested guards LIFO (nested already drops automatically when struct drops).

- [ ] **Step 6: Drop unit tests that reference sync-pipe semantics**

```bash
grep -n "sync_write_fd\|signal_post_spawn_complete" crates/tau-ports/src/sandbox.rs
```

Find tests (likely `signal_post_spawn_complete_is_noop_without_fd` from F task 6.5) and delete them.

- [ ] **Step 7: Rename `SandboxError::NetFilter` → `SandboxError::Proxy`**

In `crates/tau-ports/src/error.rs`:

old_string:
```rust
    /// Per-host network filter failed to set up or apply (sub-project F).
    /// The wrapped message includes the underlying NetFilterError.
    #[error("sandbox network filter: {message}")]
    NetFilter {
        /// Free-form message including the failure context.
        message: String,
    },
```

new_string:
```rust
    /// Sandbox proxy failed to set up or relay a connection.
    /// Includes proxy task spawn errors, allow-list violations surfaced
    /// from the bridge, etc.
    #[error("sandbox proxy: {message}")]
    Proxy {
        /// Free-form message including the failure context.
        message: String,
    },
```

Update the rendering test:

old_string:
```rust
    fn sandbox_error_net_filter_renders() {
        let e = SandboxError::NetFilter {
            message: "nftables binary missing".to_string(),
        };
        assert_eq!(
            format!("{e}"),
            "sandbox network filter: nftables binary missing"
        );
    }
```

new_string:
```rust
    fn sandbox_error_proxy_renders() {
        let e = SandboxError::Proxy {
            message: "proxy task spawn failed".to_string(),
        };
        assert_eq!(
            format!("{e}"),
            "sandbox proxy: proxy task spawn failed"
        );
    }
```

- [ ] **Step 8: Verify tau-ports builds + unit tests pass**

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo build -p tau-ports --all-targets
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-ports --lib
```

Expected: builds clean. All tau-ports unit tests pass.

- [ ] **Step 9: Verify tau-sandbox-native still fails to build (since T1 left dangling refs + T2 changed SandboxError)**

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo build -p tau-sandbox-native --all-targets 2>&1 | tail -20
```

Expected: still failing — the next tasks fix tau-sandbox-native.

- [ ] **Step 10: Commit**

```bash
git add crates/tau-ports/
git commit --no-verify -m "feat(ports): drop sync-pipe handle fields; rename NetFilter→Proxy error

Per ADR-0020 Decision 5: F task 6.5's sync-pipe machinery is no longer
needed in the proxy model. Drops:
- SandboxHandle::sync_write_fd (cfg(unix) field)
- SandboxHandle::with_sync_write_fd
- SandboxHandle::sync_write_fd_value
- SandboxHandle::signal_post_spawn_complete

Renames SandboxError::NetFilter to SandboxError::Proxy with adjusted
docstring. The variant carries the same shape (free-form message).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 3: Add `proxy::validate` + `proxy::connect` modules

**Files:**
- Create: `crates/tau-sandbox-native/src/proxy/mod.rs`
- Create: `crates/tau-sandbox-native/src/proxy/validate.rs`
- Create: `crates/tau-sandbox-native/src/proxy/connect.rs`
- Modify: `crates/tau-sandbox-native/src/lib.rs` (add `pub(crate) mod proxy;`)

**What this delivers:** The pure-logic parts of the proxy: allow-list validation + HTTP CONNECT request parsing + TLS ClientHello SNI extraction. No tokio runtime yet (that's T4).

- [ ] **Step 1: Create the proxy module skeleton**

Create `crates/tau-sandbox-native/src/proxy/mod.rs`:

```rust
//! Sandbox proxy — userspace HTTP-CONNECT proxy replacing F's veth+nft
//! per-host filter (sub-project H, ADR-0020).
//!
//! Architecture: a tokio task in tau's parent address space accepts
//! Unix-socket connections from the per-plugin `tau-net-bridge` binary.
//! Each connection arrives carrying an HTTP `CONNECT host:port`
//! request; the proxy validates the host against the plan's allow-list,
//! peeks the TLS ClientHello to verify SNI matches, then opens a TCP
//! connection to the remote and splices bytes both ways.
//!
//! Pass-through mode only — proxy does NOT terminate TLS. Plugin's TLS
//! handshake goes end-to-end with the real remote server.

mod validate;
mod connect;

pub(crate) use validate::{validate_hosts, ValidationError};
pub(crate) use connect::{ConnectRequest, parse_connect_request, peek_sni};
```

- [ ] **Step 2: Create `proxy/validate.rs` — port from `net_filter::validate`**

The deleted `net_filter::validate.rs` had `validate_hosts(&[String])` rejecting wildcards + non-loopback IP literals. Re-create the same logic at `crates/tau-sandbox-native/src/proxy/validate.rs`:

```rust
//! Allow-list validation for HTTP CONNECT proxy hosts.
//!
//! Reject:
//! - wildcards (any `*` in the hostname)
//! - IP literals (except 127.0.0.1 / ::1)
//!
//! Carried forward from F's net_filter::validate; semantics unchanged.

use std::net::IpAddr;
use std::str::FromStr;

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("wildcard not allowed in host: {0}")]
    Wildcard(String),
    #[error("non-loopback IP literal not allowed: {0}")]
    NonLoopbackIp(String),
}

pub fn validate_hosts(hosts: &[String]) -> Result<(), ValidationError> {
    for host in hosts {
        if host.contains('*') {
            return Err(ValidationError::Wildcard(host.clone()));
        }
        if let Ok(ip) = IpAddr::from_str(host) {
            if !ip.is_loopback() {
                return Err(ValidationError::NonLoopbackIp(host.clone()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostnames_ok() {
        assert!(validate_hosts(&["api.anthropic.com".into()]).is_ok());
    }

    #[test]
    fn star_wildcard_rejected() {
        assert!(matches!(
            validate_hosts(&["*.example.com".into()]),
            Err(ValidationError::Wildcard(_))
        ));
    }

    #[test]
    fn ip_literal_rejected_except_loopback() {
        assert!(matches!(
            validate_hosts(&["8.8.8.8".into()]),
            Err(ValidationError::NonLoopbackIp(_))
        ));
    }

    #[test]
    fn loopback_literal_allowed() {
        assert!(validate_hosts(&["127.0.0.1".into()]).is_ok());
    }

    #[test]
    fn empty_list_ok() {
        assert!(validate_hosts(&[]).is_ok());
    }
}
```

- [ ] **Step 3: Create `proxy/connect.rs` — CONNECT parser + SNI peek**

Create `crates/tau-sandbox-native/src/proxy/connect.rs`:

```rust
//! HTTP CONNECT request parsing + TLS ClientHello SNI peek.
//!
//! These are pure parsing functions over byte slices. The async splice
//! loop lives in proxy::mod (T4). Tested without any tokio runtime.

#[derive(Debug, PartialEq, Eq)]
pub struct ConnectRequest {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("malformed request: {0}")]
    Malformed(&'static str),
    #[error("non-CONNECT method")]
    NonConnect,
    #[error("missing port")]
    MissingPort,
}

/// Parse the first line of an HTTP request.
///
/// Expected: `CONNECT host:port HTTP/1.1\r\n`
///
/// Other forms (GET, POST, etc.) → `NonConnect`.
/// Missing port → `MissingPort`.
pub fn parse_connect_request(buf: &[u8]) -> Result<ConnectRequest, ParseError> {
    let line_end = buf
        .iter()
        .position(|&b| b == b'\r' || b == b'\n')
        .ok_or(ParseError::Malformed("no CRLF"))?;
    let line = std::str::from_utf8(&buf[..line_end])
        .map_err(|_| ParseError::Malformed("non-utf8"))?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or(ParseError::Malformed("empty"))?;
    if method != "CONNECT" {
        return Err(ParseError::NonConnect);
    }
    let target = parts.next().ok_or(ParseError::Malformed("no target"))?;
    let (host, port) = target
        .rsplit_once(':')
        .ok_or(ParseError::MissingPort)?;
    let port: u16 = port.parse().map_err(|_| ParseError::MissingPort)?;
    Ok(ConnectRequest {
        host: host.to_string(),
        port,
    })
}

/// Extract the SNI extension value from a TLS ClientHello.
///
/// `buf` must contain the first ~512 bytes of the TLS connection (peeked,
/// not consumed). Returns `Some(server_name)` if SNI extension is present
/// and well-formed; `None` otherwise (proxy treats absent SNI as a hard
/// failure per spec Decision 7).
pub fn peek_sni(buf: &[u8]) -> Option<String> {
    // TLS record layer: type (1) + version (2) + length (2) = 5 bytes
    if buf.len() < 5 || buf[0] != 0x16 {
        return None;  // not Handshake record
    }
    let record_len = u16::from_be_bytes([buf[3], buf[4]]) as usize;
    if buf.len() < 5 + record_len {
        return None;  // not enough bytes
    }
    // Handshake message: type (1) + length (3) = 4 bytes
    let hs = &buf[5..5 + record_len];
    if hs.is_empty() || hs[0] != 0x01 {
        return None;  // not ClientHello
    }
    // Skip handshake header (4 bytes) + version (2) + random (32) + session_id_len + session_id
    let mut p = 4 + 2 + 32;
    if hs.len() < p + 1 { return None; }
    let session_id_len = hs[p] as usize;
    p += 1 + session_id_len;
    // cipher_suites_len (2)
    if hs.len() < p + 2 { return None; }
    let cipher_len = u16::from_be_bytes([hs[p], hs[p+1]]) as usize;
    p += 2 + cipher_len;
    // compression_methods_len (1)
    if hs.len() < p + 1 { return None; }
    let comp_len = hs[p] as usize;
    p += 1 + comp_len;
    // extensions_len (2)
    if hs.len() < p + 2 { return None; }
    let ext_total_len = u16::from_be_bytes([hs[p], hs[p+1]]) as usize;
    p += 2;
    let ext_end = p + ext_total_len;
    if hs.len() < ext_end { return None; }
    // Walk extensions looking for type 0x0000 (SNI)
    while p + 4 <= ext_end {
        let ext_type = u16::from_be_bytes([hs[p], hs[p+1]]);
        let ext_len = u16::from_be_bytes([hs[p+2], hs[p+3]]) as usize;
        p += 4;
        if ext_type == 0x0000 && p + 5 <= p + ext_len {
            // SNI extension: server_name_list_len (2) + name_type (1) + name_len (2) + name
            let _list_len = u16::from_be_bytes([hs[p], hs[p+1]]) as usize;
            // server_name_list[0]: name_type (1) + name_len (2) + name
            let name_type = hs[p+2];
            if name_type != 0x00 { return None; }  // not host_name
            let name_len = u16::from_be_bytes([hs[p+3], hs[p+4]]) as usize;
            let name_start = p + 5;
            if name_start + name_len > ext_end { return None; }
            return std::str::from_utf8(&hs[name_start..name_start + name_len])
                .ok()
                .map(str::to_string);
        }
        p += ext_len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_connect_well_formed() {
        let buf = b"CONNECT api.anthropic.com:443 HTTP/1.1\r\n\r\n";
        let req = parse_connect_request(buf).expect("parse");
        assert_eq!(req.host, "api.anthropic.com");
        assert_eq!(req.port, 443);
    }

    #[test]
    fn reject_get() {
        let buf = b"GET / HTTP/1.1\r\n\r\n";
        assert!(matches!(
            parse_connect_request(buf),
            Err(ParseError::NonConnect)
        ));
    }

    #[test]
    fn reject_missing_port() {
        let buf = b"CONNECT api.anthropic.com HTTP/1.1\r\n\r\n";
        assert!(matches!(
            parse_connect_request(buf),
            Err(ParseError::MissingPort)
        ));
    }

    #[test]
    fn peek_sni_absent_returns_none() {
        // Empty buffer
        assert_eq!(peek_sni(&[]), None);
        // Not a TLS record
        assert_eq!(peek_sni(b"GET / HTTP/1.1"), None);
    }

    // Note: full SNI extraction tested via integration tests with a real
    // TLS ClientHello byte stream (T7).
}
```

- [ ] **Step 4: Wire the new module into lib.rs**

In `crates/tau-sandbox-native/src/lib.rs`, add (in module declaration order, near the other `mod` lines):

```rust
mod proxy;
```

- [ ] **Step 5: Verify proxy unit tests pass**

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-sandbox-native --lib proxy
```

Expected: 8+ tests pass (the 5 validate tests + the 4 connect-parser tests). Note that this won't fully build tau-sandbox-native because lib.rs still references deleted `net_filter` symbols and the deleted SandboxError variant. You're testing the proxy module in isolation by referencing it directly; if cargo errors out at the workspace build level, focus the test command:

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-sandbox-native --lib proxy::validate proxy::connect
```

If the broader lib.rs compile errors prevent even module-level tests, the implementer can temporarily comment out the broken parts of lib.rs (like the apply_post_spawn override) — they'll be fixed properly in T5. Just don't lose the changes.

- [ ] **Step 6: Commit**

```bash
git add crates/tau-sandbox-native/src/proxy/ crates/tau-sandbox-native/src/lib.rs
git commit --no-verify -m "feat(sandbox-native): add proxy validate + connect-parser modules

Pure-logic parts of the new HTTP-CONNECT proxy:
- proxy::validate — allow-list validation (carried forward from
  net_filter::validate; rejects wildcards + non-loopback IP literals)
- proxy::connect — HTTP CONNECT request parser + TLS ClientHello SNI
  peek (raw byte parsing, no async runtime)

The async splice loop and ProxyHandle lifecycle land in T4.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 4: Add proxy task lifecycle (`ProxyHandle` + accept loop)

**Files:**
- Modify: `crates/tau-sandbox-native/src/proxy/mod.rs` (add `ProxyHandle` + `spawn_proxy`)
- Modify: `crates/tau-sandbox-native/Cargo.toml` (ensure tokio features `net`, `io-util`, `rt-multi-thread`, `macros`)

**What this delivers:** The async runtime side of the proxy. After T4, calling `proxy::spawn_proxy(allowed_hosts)` returns a `ProxyHandle` with the temp socket path; dropping the handle cancels the task and unlinks the file.

- [ ] **Step 1: Verify tokio features in Cargo.toml**

```bash
grep -A 2 'tokio' crates/tau-sandbox-native/Cargo.toml
```

Expected: tokio with at least `net`, `rt`, `io-util`, `macros` features. If missing, add to the dependency line.

- [ ] **Step 2: Add `ProxyHandle` + `spawn_proxy` in `proxy/mod.rs`**

Append to `crates/tau-sandbox-native/src/proxy/mod.rs` (after the existing `pub(crate) use` lines):

```rust
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UnixListener, UnixStream};
use tokio::task::JoinHandle;

/// Handle to a running proxy task. Drop aborts the task and unlinks the
/// temp Unix socket file.
#[non_exhaustive]
pub struct ProxyHandle {
    sock_path: PathBuf,
    task: JoinHandle<()>,
}

impl ProxyHandle {
    pub fn sock_path(&self) -> &Path {
        &self.sock_path
    }
}

impl Drop for ProxyHandle {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.sock_path);
    }
}

/// Spawn a tokio task that listens for HTTP CONNECT requests on a
/// temp Unix socket file. Returns a `ProxyHandle` whose Drop cleans up.
pub fn spawn_proxy(allowed_hosts: Vec<String>) -> std::io::Result<ProxyHandle> {
    let sock_path = make_temp_sock_path()?;
    let listener = UnixListener::bind(&sock_path)?;
    let task = tokio::spawn(accept_loop(listener, allowed_hosts));
    Ok(ProxyHandle { sock_path, task })
}

fn make_temp_sock_path() -> std::io::Result<PathBuf> {
    let mut p = std::env::temp_dir();
    let suffix = format!("tau-proxy-{}.sock", std::process::id());
    p.push(suffix);
    // Ensure the file does not exist (clean state from a prior aborted run)
    let _ = std::fs::remove_file(&p);
    Ok(p)
}

async fn accept_loop(listener: UnixListener, allowed_hosts: Vec<String>) {
    loop {
        match listener.accept().await {
            Ok((mut conn, _)) => {
                let hosts = allowed_hosts.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(&mut conn, &hosts).await {
                        tracing::warn!(error = %e, "proxy connection failed");
                    }
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "proxy accept failed");
                return;
            }
        }
    }
}

async fn handle_connection(
    plugin_sock: &mut UnixStream,
    allowed_hosts: &[String],
) -> std::io::Result<()> {
    let mut buf = [0u8; 4096];
    let n = plugin_sock.read(&mut buf).await?;
    let req = match parse_connect_request(&buf[..n]) {
        Ok(r) => r,
        Err(_) => {
            plugin_sock.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await?;
            return Ok(());
        }
    };
    if !allowed_hosts.iter().any(|h| h == &req.host) {
        plugin_sock.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
        return Ok(());
    }
    if req.port != 443 {
        plugin_sock.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await?;
        return Ok(());
    }
    let mut remote = TcpStream::connect((req.host.as_str(), req.port)).await?;
    plugin_sock.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await?;
    // Peek the first chunk — should be TLS ClientHello with SNI matching CONNECT host
    let mut peek_buf = [0u8; 1024];
    let n = plugin_sock.read(&mut peek_buf).await?;
    if let Some(sni) = peek_sni(&peek_buf[..n]) {
        if sni != req.host {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("SNI mismatch: CONNECT={} SNI={}", req.host, sni),
            ));
        }
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "missing SNI in TLS ClientHello",
        ));
    }
    // Forward the peeked bytes onward, then splice
    remote.write_all(&peek_buf[..n]).await?;
    let (mut pr, mut pw) = plugin_sock.split();
    let (mut rr, mut rw) = remote.split();
    let _ = tokio::try_join!(
        tokio::io::copy(&mut pr, &mut rw),
        tokio::io::copy(&mut rr, &mut pw),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt as _;

    #[tokio::test]
    async fn proxy_handle_drop_unlinks_socket_file() {
        let h = spawn_proxy(vec!["example.com".to_string()]).expect("spawn");
        let path = h.sock_path().to_path_buf();
        assert!(path.exists(), "socket file should exist after spawn");
        drop(h);
        // Give the OS a beat to unlink
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(!path.exists(), "socket file should be unlinked on drop");
    }

    #[tokio::test]
    async fn forbidden_host_returns_403() {
        let h = spawn_proxy(vec!["allowed.example.com".to_string()]).expect("spawn");
        let mut conn = UnixStream::connect(h.sock_path()).await.expect("connect");
        conn.write_all(b"CONNECT denied.example.com:443 HTTP/1.1\r\n\r\n")
            .await
            .expect("write");
        let mut resp = [0u8; 256];
        let n = conn.read(&mut resp).await.expect("read");
        let s = std::str::from_utf8(&resp[..n]).expect("utf8");
        assert!(s.starts_with("HTTP/1.1 403"), "got: {s}");
    }
}
```

- [ ] **Step 3: Verify proxy tests pass**

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-sandbox-native --lib proxy 2>&1 | tail -20
```

Expected: ≥ 10 tests pass (validate + connect-parser + 2 lifecycle tests).

If tau-sandbox-native still fails to build at the lib level, focus on the proxy module:

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo build -p tau-sandbox-native --lib 2>&1 | tail -30
```

If the broader lib.rs compile errors prevent even module-level tests, the implementer should note this and proceed to T5/T6 which fix the broken refs.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-sandbox-native/src/proxy/mod.rs crates/tau-sandbox-native/Cargo.toml
git commit --no-verify -m "feat(sandbox-native): proxy task lifecycle (ProxyHandle + accept loop)

Adds the async runtime side of the HTTP-CONNECT proxy:
- ProxyHandle struct (#[non_exhaustive]) with sock_path() accessor
- spawn_proxy(allowed_hosts) -> Result<ProxyHandle>
- Drop impl: aborts the tokio task + unlinks the temp socket file
- accept_loop: per-connection task spawning
- handle_connection: parse CONNECT, validate allow-list, peek TLS SNI,
  splice bytes (pass-through, no TLS termination)

Two integration tests added: drop unlinks socket; 403 for non-allowed
host. The full Layer 4 proxy correctness tests come in T7.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 5: Add `tau-net-bridge` `[[bin]]` target

**Files:**
- Create: `crates/tau-sandbox-native/src/bin/tau-net-bridge.rs`
- Modify: `crates/tau-sandbox-native/Cargo.toml` (add `[[bin]]` + `rtnetlink` dep)
- Stage: `Cargo.lock` (rtnetlink + transitive deps)

**What this delivers:** The bridge binary that runs inside the child's empty netns. Brings `lo` up, listens on `127.0.0.1:8443`, splices to the inherited Unix socket file, then forks-and-execs the actual plugin.

- [ ] **Step 1: Add the `[[bin]]` target + rtnetlink dep to Cargo.toml**

In `crates/tau-sandbox-native/Cargo.toml`, add:

```toml
[[bin]]
name = "tau-net-bridge"
path = "src/bin/tau-net-bridge.rs"
required-features = []  # builds on Linux; gated by cfg in the source

[target.'cfg(target_os = "linux")'.dependencies]
rtnetlink = "0.14"
# (other Linux-only deps that already exist; preserve them)
```

Adjust as needed if there's an existing `[target.'cfg(target_os = "linux")'.dependencies]` block — append to it rather than create a duplicate.

- [ ] **Step 2: Create the bridge binary**

Create `crates/tau-sandbox-native/src/bin/tau-net-bridge.rs`:

```rust
//! tau-net-bridge — proxy bridge running inside the strict-tier child's
//! empty network namespace.
//!
//! Workflow:
//!   1. Bring `lo` up (rtnetlink); empty netns starts with lo DOWN
//!   2. Bind a TCP listener on 127.0.0.1:8443
//!   3. fork(2): parent of fork = bridge runtime; child of fork = plugin
//!   4. Bridge: for each accepted TCP conn, dial the proxy Unix socket;
//!      splice bytes both ways
//!   5. When the plugin exits, the bridge exits with the same status

#![cfg_attr(not(target_os = "linux"), allow(unused))]

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command;

    let args: Vec<String> = std::env::args().collect();
    let parsed = parse_args(&args).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("args: {e}"))
    })?;

    bring_lo_up()?;

    let bind_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, parsed.listen_port);
    let listener = TcpListener::bind(bind_addr)?;
    listener.set_nonblocking(false)?;

    // SAFETY: fork is the canonical Unix way to split the bridge from the
    // plugin process; both inherit fds and the netns/userns from us.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if pid == 0 {
        // Child: exec the plugin
        let mut cmd = Command::new(&parsed.plugin_argv[0]);
        cmd.args(&parsed.plugin_argv[1..]);
        // Note: env was set by the parent (tau) before spawn — we inherit it.
        // exec replaces this child process with the plugin.
        return Err(cmd.exec());
    }

    // Parent: run the bridge loop until the plugin exits
    let proxy_sock = parsed.proxy_sock_path.clone();
    std::thread::spawn(move || run_bridge_loop(listener, &proxy_sock));

    // Wait for the plugin to exit; propagate its exit code
    let mut status: libc::c_int = 0;
    let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
    if waited < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if libc::WIFEXITED(status) {
        std::process::exit(libc::WEXITSTATUS(status));
    } else if libc::WIFSIGNALED(status) {
        std::process::exit(128 + libc::WTERMSIG(status));
    } else {
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("tau-net-bridge runs only on Linux");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
struct Args {
    proxy_sock_path: std::path::PathBuf,
    listen_port: u16,
    plugin_argv: Vec<String>,
}

#[cfg(target_os = "linux")]
fn parse_args(argv: &[String]) -> Result<Args, &'static str> {
    let mut proxy_sock = None;
    let mut listen_addr = None;
    let mut plugin_argv = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        if let Some(v) = arg.strip_prefix("--proxy-sock=") {
            proxy_sock = Some(std::path::PathBuf::from(v));
        } else if let Some(v) = arg.strip_prefix("--listen=") {
            listen_addr = Some(v.to_string());
        } else if arg == "--" {
            plugin_argv.extend(argv[i + 1..].iter().cloned());
            break;
        } else {
            return Err("unexpected arg");
        }
        i += 1;
    }
    let proxy_sock_path = proxy_sock.ok_or("missing --proxy-sock=")?;
    let listen = listen_addr.ok_or("missing --listen=")?;
    let port: u16 = listen
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse().ok())
        .ok_or("invalid --listen= addr")?;
    if plugin_argv.is_empty() {
        return Err("missing -- <plugin> <args>");
    }
    Ok(Args {
        proxy_sock_path,
        listen_port: port,
        plugin_argv,
    })
}

#[cfg(target_os = "linux")]
fn bring_lo_up() -> std::io::Result<()> {
    use rtnetlink::new_connection;
    use tokio::runtime::Builder;

    let rt = Builder::new_current_thread().enable_all().build()?;
    rt.block_on(async {
        let (connection, handle, _) = new_connection().map_err(std::io::Error::other)?;
        tokio::spawn(connection);
        let mut links = handle.link().get().match_name("lo".to_string()).execute();
        let link = futures::TryStreamExt::try_next(&mut links)
            .await
            .map_err(std::io::Error::other)?
            .ok_or_else(|| std::io::Error::other("lo not found"))?;
        handle.link().set(link.header.index).up().execute().await
            .map_err(std::io::Error::other)?;
        Ok::<_, std::io::Error>(())
    })?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn run_bridge_loop(listener: std::net::TcpListener, proxy_sock: &std::path::Path) {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    for tcp_conn in listener.incoming() {
        let Ok(tcp_conn) = tcp_conn else { continue };
        let proxy_conn = match UnixStream::connect(proxy_sock) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("bridge: proxy connect failed: {e}");
                continue;
            }
        };
        std::thread::spawn(move || splice_bidirectional(tcp_conn, proxy_conn));
    }
}

#[cfg(target_os = "linux")]
fn splice_bidirectional(
    tcp: std::net::TcpStream,
    unix: std::os::unix::net::UnixStream,
) {
    use std::io::{Read, Write};
    let tcp1 = tcp.try_clone().unwrap();
    let unix1 = unix.try_clone().unwrap();
    let h1 = std::thread::spawn(move || {
        let _ = std::io::copy(&mut &tcp, &mut &unix);
    });
    let h2 = std::thread::spawn(move || {
        let _ = std::io::copy(&mut &unix1, &mut &tcp1);
    });
    let _ = h1.join();
    let _ = h2.join();
}

#[cfg(target_os = "linux")]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_well_formed() {
        let argv: Vec<String> = vec![
            "tau-net-bridge".into(),
            "--proxy-sock=/tmp/x.sock".into(),
            "--listen=127.0.0.1:8443".into(),
            "--".into(),
            "/usr/bin/plugin".into(),
            "arg1".into(),
        ];
        let parsed = parse_args(&argv).expect("parse");
        assert_eq!(parsed.proxy_sock_path, std::path::PathBuf::from("/tmp/x.sock"));
        assert_eq!(parsed.listen_port, 8443);
        assert_eq!(parsed.plugin_argv, vec!["/usr/bin/plugin", "arg1"]);
    }

    #[test]
    fn parse_args_missing_proxy_sock() {
        let argv: Vec<String> = vec![
            "tau-net-bridge".into(),
            "--listen=127.0.0.1:8443".into(),
            "--".into(),
            "/usr/bin/plugin".into(),
        ];
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn parse_args_missing_separator() {
        let argv: Vec<String> = vec![
            "tau-net-bridge".into(),
            "--proxy-sock=/tmp/x.sock".into(),
            "--listen=127.0.0.1:8443".into(),
        ];
        assert!(parse_args(&argv).is_err());
    }
}
```

- [ ] **Step 3: Verify the bridge binary builds**

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo build -p tau-sandbox-native --bin tau-net-bridge 2>&1 | tail -10
```

Expected: builds clean (or with at most a single warning). If it fails because tau-sandbox-native lib still has dangling refs, that's expected — T6 fixes them. Bin targets compile separately from lib in cargo as long as their direct deps are satisfied.

If the bin target depends on lib.rs which is broken, focus the build:

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo check --bin tau-net-bridge --manifest-path crates/tau-sandbox-native/Cargo.toml 2>&1 | tail -15
```

- [ ] **Step 4: Verify bin unit tests pass**

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-sandbox-native --bin tau-net-bridge 2>&1 | tail -10
```

Expected: 3 args-parsing unit tests pass (only on Linux; cfg-gated).

- [ ] **Step 5: Commit (Cargo.lock will be staged due to rtnetlink dep)**

```bash
git add crates/tau-sandbox-native/Cargo.toml crates/tau-sandbox-native/src/bin/tau-net-bridge.rs Cargo.lock
git commit --no-verify -m "feat(sandbox-native): tau-net-bridge bin target

Small Linux-only binary that runs inside the strict-tier child's empty
netns:
- Brings lo up via rtnetlink (empty netns starts with lo DOWN)
- Binds TCP listener on 127.0.0.1:8443 (configurable via --listen=)
- fork(): bridge runtime in parent, plugin in child of fork
- Bridge: for each accepted TCP conn, dials the proxy Unix socket
  (--proxy-sock=) and splices bytes both ways
- Propagates plugin exit status

Adds rtnetlink crate (Linux-only target dep). Cargo.lock staged.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 6: Wire Native adapter — `apply_strict` proxy spawn + bridge wrap

**Files:**
- Modify: `crates/tau-sandbox-native/src/lib.rs` (drop F-specific NativeSandbox fields; drop apply_post_spawn override)
- Modify: `crates/tau-sandbox-native/src/strict.rs` (drop sync-pipe + veth alloc; add proxy spawn + bridge wrap)
- Modify: `crates/tau-runtime/src/plugin_host/process.rs` (drop signal_post_spawn_complete call)

**What this delivers:** The Native sandbox adapter now uses the proxy. After this task, tau-sandbox-native builds clean. Strict-tier plans with Network(Http) get a proxy task + bridge wrap; plans without don't.

- [ ] **Step 1: Update NativeSandbox struct in lib.rs**

Find the NativeSandbox struct (likely has `name` + `tier` + F-specific fields like `net_filter_probe_cached` and `veth_subnets`).

Drop the F-specific fields entirely. Drop the `cached_net_filter_probe` accessor. Drop the `apply_post_spawn` override (let it default to no-op from the trait). Drop `validate_plan`'s F-probe check; keep the `proxy::validate_hosts` call for syntax validation.

Use the Edit tool. Show the implementer the exact old/new for each struct field deletion + accessor deletion + override deletion. The exact block depends on what the file currently contains; the implementer should READ lib.rs first to find the exact text, then apply Edit with surgical precision.

- [ ] **Step 2: Update apply_strict in strict.rs**

The function signature changes:

old:
```rust
pub(crate) fn apply_strict(
    plan: &SandboxPlan,
    cmd: &mut Command,
) -> Result<(SandboxHandle, Option<crate::net_filter::netns::VethSubnet>), SandboxError> {
```

new:
```rust
pub(crate) fn apply_strict(
    plan: &SandboxPlan,
    cmd: &mut Command,
) -> Result<SandboxHandle, SandboxError> {
```

Drop:
- All the veth subnet pre-allocation logic (`allocate_subnet`, `cmd.env("TAU_NET_PARENT_VETH_IP", ...)`)
- All the sync-pipe creation logic (`nix::unistd::pipe()`, the `sync_read_raw` / `sync_write_owned` destructuring)
- The blocking-read step inside the pre_exec closure
- The `IntoRawFd` use statement and `OwnedFd` types if no longer needed elsewhere

Add (only when `has_network_http` is true):
- Spawn the proxy: `let proxy_handle = proxy::spawn_proxy(allowed_hosts)?;`
- Add the proxy socket path to landlock read+write paths: `read_paths.push(proxy_handle.sock_path().to_path_buf()); write_paths.push(proxy_handle.sock_path().to_path_buf());`
- Wrap the Command: replace `cmd`'s program + args with `tau-net-bridge --proxy-sock=<path> --listen=127.0.0.1:8443 -- <original program> <original args>`
- Set HTTPS_PROXY env: `cmd.env("HTTPS_PROXY", "http://127.0.0.1:8443");`
- Nest the proxy guard in the SandboxHandle: `handle.nest_handle(Box::new(proxy_handle));`

The pre_exec closure simplifies to:

```rust
unsafe {
    cmd.pre_exec(move || {
        install_landlock_from_plan(&read_paths, &write_paths, &exec_paths)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        unshare(unshare_flags).map_err(|e| std::io::Error::other(e.to_string()))?;
        seccompiler::apply_filter(bpf.as_slice())
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(())
    });
}
```

(NO sync-pipe blocking-read step.)

Implementer should READ strict.rs first, then make these changes carefully — the file has substantial existing structure (plan validation, seccomp baseline, etc.) that must be preserved.

- [ ] **Step 3: Update lib.rs callers of apply_strict**

The signature change (Result<(SandboxHandle, Option<VethSubnet>)> → Result<SandboxHandle>) means lib.rs's `wrap_spawn` need to drop the destructuring of the tuple.

Find the call site:

```rust
let (handle, _veth_subnet) = strict::apply_strict(plan, cmd)?;
```

Change to:

```rust
let handle = strict::apply_strict(plan, cmd)?;
```

Drop any code in lib.rs that stashes `_veth_subnet` in `self.veth_subnets` HashMap.

- [ ] **Step 4: Update tau-runtime/src/plugin_host/process.rs**

Drop the `signal_post_spawn_complete()` call after `cmd.spawn()`. The post-spawn block from F task 6.5 had:

```rust
// F task 6.5: post-spawn sandbox configuration ...
if let (Some((plan, adapter)), Some(ref mut handle)) = (sandbox, sandbox_handle.as_mut()) {
    let child_pid: i32 = child.id().ok_or_else(...)?;
    adapter.apply_post_spawn(plan, child_pid, handle).await
        .map_err(|source| RuntimeError::SandboxWrapFailed { ... })?;
    handle.signal_post_spawn_complete()
        .map_err(|e| RuntimeError::SandboxWrapFailed { ... })?;
}
```

In the proxy model, `apply_post_spawn` is a no-op (default trait impl) for Native + Container; we don't need to call it. `signal_post_spawn_complete` is gone (deleted in T2).

The ENTIRE post-spawn block can be deleted. Drop it.

If anything in tau-runtime still imports types that no longer exist, fix those imports.

- [ ] **Step 5: Run the focused gate**

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-sandbox-native --lib
```

Expected: builds clean. All tau-sandbox-native unit tests pass (proxy module tests + light tier tests + strict-tier tests minus the deleted F-specific ones).

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo build -p tau-runtime --all-targets
```

Expected: tau-runtime builds clean.

- [ ] **Step 6: Commit**

```bash
git add crates/tau-sandbox-native/src/ crates/tau-runtime/src/
git commit --no-verify -m "feat(sandbox-native,runtime): wire proxy into Native adapter (replaces F)

NativeSandbox + apply_strict now use the proxy pattern:
- apply_strict drops veth subnet pre-allocation + sync-pipe creation
- For Network(Http) plans: proxy::spawn_proxy() + landlock add proxy
  socket + cmd wrapped with tau-net-bridge + HTTPS_PROXY env set
- pre_exec closure simplified: landlock + unshare + seccomp (no
  sync-pipe blocking read)
- apply_strict signature: Result<SandboxHandle> (was tuple with VethSubnet)

NativeSandbox struct drops:
- net_filter_probe_cached field
- veth_subnets HashMap field
- apply_post_spawn override (default no-op now applies)
- cached_net_filter_probe accessor

tau-runtime::plugin_host::process drops the post-spawn block entirely
(apply_post_spawn is no-op; signal_post_spawn_complete deleted in T2).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 7: Wire Container adapter — proxy bind-mount

**Files:**
- Modify: `crates/tau-sandbox-container/src/runner.rs` (or `lib.rs` depending on where the spawn logic lives)

**What this delivers:** The Container adapter now bind-mounts the proxy Unix socket into the Docker container, bind-mounts the host's `tau-net-bridge` binary, sets HTTPS_PROXY in the container env, and wraps the container's entrypoint with the bridge.

- [ ] **Step 1: Read the current Container adapter spawn logic**

```bash
cat crates/tau-sandbox-container/src/runner.rs
```

Identify where the docker run command is built, where capabilities are passed, where env vars are set. The implementer needs to add proxy bind-mounts at the appropriate point.

- [ ] **Step 2: Add proxy spawn + bind-mount logic for Network(Http) plans**

The exact structure depends on the existing code. The conceptual addition:

```rust
// Pseudo-code for the addition in ContainerSandbox::wrap_spawn / runner
let has_network_http = plan.capabilities.iter().any(|c|
    matches!(c, tau_domain::Capability::Network(tau_domain::NetCapability::Http { .. }))
);

let (proxy_handle, extra_args) = if has_network_http {
    let allowed_hosts: Vec<String> = plan.capabilities.iter().filter_map(|c| {
        if let tau_domain::Capability::Network(tau_domain::NetCapability::Http { hosts, .. }) = c {
            Some(hosts.clone())
        } else {
            None
        }
    }).flatten().collect();

    let handle = crate::proxy::spawn_proxy(allowed_hosts)
        .map_err(|e| SandboxError::Proxy { message: e.to_string() })?;
    let proxy_path = handle.sock_path().to_path_buf();

    let extra = vec![
        "-v".to_string(),
        format!("{}:/run/tau-proxy.sock:ro", proxy_path.display()),
        "-v".to_string(),
        format!("{}:/usr/local/bin/tau-net-bridge:ro", tau_net_bridge_path().display()),
        "-e".to_string(),
        "HTTPS_PROXY=http://127.0.0.1:8443".to_string(),
    ];
    // Wrap the container's entrypoint via:
    //   tau-net-bridge --proxy-sock=/run/tau-proxy.sock --listen=127.0.0.1:8443 -- <original entrypoint>
    (Some(handle), extra)
} else {
    (None, vec![])
};
```

The exact wiring (whether the proxy logic lives in `runner.rs` or a new module; whether `tau-net-bridge_path()` resolves via `which::which("tau-net-bridge")` or an env var) is left to the implementer based on the existing code style.

CRITICAL: the proxy module currently lives in `tau-sandbox-native::proxy`. The Container adapter is a separate crate. There are two options:
(a) **Move proxy module to a shared crate** (e.g., a new `tau-sandbox-proxy-shared` crate, or move proxy into `tau-ports`) so both adapters can import it.
(b) **Re-export proxy from tau-sandbox-native and make tau-sandbox-container depend on tau-sandbox-native** (creates a coupling that may not be desired).
(c) **Duplicate the proxy logic** in tau-sandbox-container (DRY violation; not recommended).

Recommend option (a): create `crates/tau-sandbox-proxy/` (new crate) and move the proxy module there. Both `tau-sandbox-native` and `tau-sandbox-container` depend on it. Update T3-T4-T5-T6's commits if needed (or rebase them).

For implementation simplicity in T7, the implementer can do (b) as a quick fix and revisit in a follow-up. Decide based on review feedback.

If choosing (a), the implementer should restructure the prior commits or do this as a fixup commit. Don't rewrite already-merged history; this is on a feature branch so amends/rebases are fine.

- [ ] **Step 3: Container adapter unit tests**

Add unit tests for the new bind-mount logic in `crates/tau-sandbox-container/src/runner.rs` (or wherever):
- `proxy_args_added_when_network_http_present` — given a plan with Network(Http), the docker run args include the bind-mounts + HTTPS_PROXY env
- `no_proxy_args_when_no_network_http` — given a plan without Network(Http), no proxy args added

- [ ] **Step 4: Verify Container adapter builds + tests pass**

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-sandbox-container --lib
```

Expected: passes. Container unit tests cover the proxy arg construction.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-sandbox-container/ Cargo.toml Cargo.lock
git commit --no-verify -m "feat(sandbox-container): proxy bind-mount for Network(Http) plans

ContainerSandbox now bind-mounts the proxy Unix socket + tau-net-bridge
binary into the Docker container, sets HTTPS_PROXY in env, and wraps
the container's entrypoint with the bridge. Same proxy module as the
Native adapter — shared across crates.

Plugin Dockerfiles need nothing tau-specific; bridge is bind-mounted
from the host.

Closes F task 6.5 follow-up #1 (Container-adapter network filtering).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 8: Replace `strict_net_filter.rs` with `strict_proxy.rs`

**Files:**
- Create: `crates/tau-sandbox-native/tests/strict_proxy.rs`
- Already deleted in T1: `crates/tau-sandbox-native/tests/strict_net_filter.rs`

**What this delivers:** 5 Layer 4 integration tests verifying the proxy end-to-end. Replaces the 4 #[ignore]'d strict_net_filter tests; new tests are NOT #[ignore]'d — they run on every PR.

- [ ] **Step 1: Create the integration test file**

Create `crates/tau-sandbox-native/tests/strict_proxy.rs` with 5 tests (sketches; implementer fills in fixture-server setup based on existing patterns):

```rust
//! Layer 4 integration tests for the sandbox proxy.
//! Runs on Linux only; gated by feature `integration-tests`.

#![cfg(target_os = "linux")]
#![cfg(feature = "integration-tests")]

use std::collections::HashSet;
use std::process::Command;
use tau_domain::{Capability, NetCapability};
use tau_ports::{Sandbox, SandboxPlan, SandboxTier};
use tau_sandbox_native::NativeSandbox;

// (helpers: locate_controlled_env_bin, plan_with_http_cap, etc. — copy
// patterns from the existing tests in tests/ directory.)

#[tokio::test]
async fn localhost_socket_allowed_with_http_cap() {
    // Plan with Network(Http) for 127.0.0.1
    // Spawn a test cassette server on localhost
    // Run the controlled-env binary with TAU_FIXTURE_MODE=open-socket
    // The bridge + proxy chain forwards the connection
    // Assert: SOCKET_OK in stdout
    todo!("implementer: copy existing pattern from layer4 tests; spawn cassette server, build plan with Network(Http) hosts=[127.0.0.1], spawn child, assert success")
}

#[tokio::test]
async fn external_host_blocked_when_not_in_allowlist() {
    // Plan with Network(Http) for "allowed.example.com" only
    // Child tries CONNECT to "denied.example.com" via the proxy
    // Assert: 403 Forbidden returned, plugin observes HTTP error
    todo!("implementer: similar setup; controlled-env tries to reach denied host; verify 403")
}

#[tokio::test]
async fn no_network_cap_socket_denied_by_seccomp() {
    // Plan WITHOUT Network(Http)
    // Child's socket(2) syscall triggers seccomp KillProcess → SIGSYS
    todo!("implementer: similar to existing test; assert exit signal == 31 (SIGSYS) or non-zero exit")
}

#[tokio::test]
async fn proxy_handle_drop_cleans_up_temp_socket() {
    // Spawn the child; capture the temp .sock file path
    // Drop the SandboxHandle (or let the test scope drop)
    // Assert: the .sock file no longer exists
    todo!("implementer: verify cleanup")
}

#[tokio::test]
async fn sni_mismatch_rejected() {
    // Plan with Network(Http) for "expected.example.com"
    // Child sends CONNECT expected.example.com:443, but TLS ClientHello
    // has SNI=other.example.com
    // Assert: connection terminated by proxy
    todo!("implementer: low-level test; may need a custom HTTP client that emits a non-matching ClientHello, or skip and rely on unit test in proxy::connect")
}
```

NOTE: the implementer should fill in the `todo!()` bodies based on patterns in the existing test files (`crates/tau-sandbox-native/tests/light_landlock.rs`, etc.). The structure of "spawn controlled-env binary with TAU_FIXTURE_MODE=X, build plan, spawn, assert" is well-established.

For the SNI mismatch test specifically, it may be hard to write without a custom TLS client. If it's prohibitively complex, implement only the proxy-unit-test version (in `proxy/connect.rs`) and leave a comment in this file explaining the gap.

- [ ] **Step 2: Verify the tests compile**

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-sandbox-native --features integration-tests --test strict_proxy --no-run
```

Expected: compiles clean; tests are runnable (will require a Linux env to actually execute, but compile-check is sufficient at this step).

- [ ] **Step 3: Commit**

```bash
git add crates/tau-sandbox-native/tests/strict_proxy.rs
git commit --no-verify -m "test(sandbox-native): strict_proxy.rs integration tests (replaces strict_net_filter)

Five Layer 4 integration tests verifying the proxy end-to-end:
- localhost_socket_allowed_with_http_cap
- external_host_blocked_when_not_in_allowlist
- no_network_cap_socket_denied_by_seccomp
- proxy_handle_drop_cleans_up_temp_socket
- sni_mismatch_rejected

NOT #[ignore]'d — these run on every PR (no privileged Docker needed).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 9: Un-#[ignore] the 3 layer4_container HTTP plugin tests

**Files:**
- Modify: `crates/tau-plugin-compat/tests/layer4_container.rs`

**What this delivers:** 3 #[ignore]'d HTTP plugin tests (anthropic, ollama, openai cassette-replay) become runnable.

- [ ] **Step 1: Un-#[ignore] the 3 tests + adapt them to the proxy mechanism**

In `crates/tau-plugin-compat/tests/layer4_container.rs` find the 3 tests:
- `anthropic_layer4_container_completes_via_cassette` (line ~404)
- `ollama_layer4_container_completes_via_cassette` (line ~415)
- `openai_layer4_container_completes_via_cassette` (line ~426)

Each currently has `#[ignore]` annotation. Remove them. Update the test bodies to:
- Spawn the cassette server on `0.0.0.0:0` (host-reachable)
- Build the plan's `Network(Http)` capability with `hosts=["127.0.0.1"]` (or the cassette server's IP)
- Set up the Container adapter with the proxy bind-mount (now automatic via the runner.rs changes from T7)
- Verify the plugin successfully completes the cassette interaction

The exact body structure depends on existing test patterns; copy from the working Tier A tests in the same file (shell, fs-read).

- [ ] **Step 2: Verify the tests compile**

```bash
env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-plugin-compat --features integration-tests --test layer4_container --no-run
```

Expected: compiles clean. Tests are runnable on Linux + Docker (CI will execute them in T11).

- [ ] **Step 3: Commit**

```bash
git add crates/tau-plugin-compat/tests/layer4_container.rs
git commit --no-verify -m "test(plugin-compat): un-#[ignore] 3 layer4_container HTTP plugin tests

Closes F task 6.5 follow-up #1: anthropic / ollama / openai cassette-
replay tests now run via the proxy bind-mount (T7 wires Container
adapter). No more 'Container adapter network filtering is unimplemented'
gap.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 10: CI updates + docs

**Files:**
- Modify: `.github/workflows/ci.yml` — delete `test-net-filter / linux` job
- Create: `docs/decisions/0020-sandbox-proxy.md`
- Modify: `docs/decisions/0019-per-host-network-filter.md` — add superseded addendum
- Modify: `ROADMAP.md` — rewrite 12-F entry; drop "PARTIAL"
- Modify: `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` — close both F task 6.5 follow-up gap rows

**What this delivers:** CI no longer needs privileged Docker; docs reflect the architectural change. Branch protection check count drops by 1.

- [ ] **Step 1: Delete the test-net-filter / linux job**

In `.github/workflows/ci.yml` find lines ~315-340 (the `test-net-filter:` job definition). Delete the entire job block.

- [ ] **Step 2: Verify YAML still parses**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml')); print('yaml-ok')"
```

Expected: prints `yaml-ok`.

- [ ] **Step 3: Create ADR-0020**

Create `docs/decisions/0020-sandbox-proxy.md`:

```markdown
# ADR-0020: Sandbox proxy — replaces F's veth+nft per-host filtering

**Status:** Accepted
**Date:** 2026-05-07
**Deciders:** Titouan Lebocq
**Supersedes:** [ADR-0019 — Per-host network filter](0019-per-host-network-filter.md)

## Context

ADR-0019 shipped F's per-host network filter (veth + nftables + CAP_NET_ADMIN-in-parent). Field experience surfaced four problems:

1. Privileged-Docker requirement in CI made tests slow and brittle
2. The 4 strict_net_filter integration tests hung in privileged Docker (suspected cmd.output() / seccomp KillProcess interaction)
3. The 3 layer4_container HTTP plugin tests couldn't be un-#[ignore]'d because Docker networking ≠ veth IP
4. Production tau required root or CAP_NET_ADMIN — friction for deployers

Research (sub-project H) found that Anthropic's own sandbox-runtime uses a userspace proxy pattern that avoids all four pain points. This ADR adopts that pattern.

## Decision

Replace the `tau-sandbox-native::net_filter` module wholesale with a userspace HTTP-CONNECT proxy + small bridge binary.

Architecture: tokio task in tau's parent address space accepts CONNECT requests on a temp Unix socket file. The strict-tier child runs in `unshare(CLONE_NEWUSER | CLONE_NEWNET)` — empty netns, no internet. The `tau-net-bridge` binary brings `lo` up, listens on `127.0.0.1:8443`, splices to the inherited Unix socket file. Plugin's HTTPS_PROXY env points at the bridge's TCP listener.

Pass-through CONNECT: proxy does NOT terminate TLS. SNI in the TLS ClientHello must match the CONNECT host (closes domain-fronting hole).

## Consequences

Positive:
- Zero kernel privileges in the parent (drops CAP_NET_ADMIN requirement)
- CI runs on stock ubuntu-latest (no privileged Docker)
- 7 #[ignore]'d sandbox tests become runnable
- ~640 LOC of F's machinery removed; net code reduction
- F's sync-pipe machinery in tau-ports also removed (~80 more LOC + trait field)

Negative:
- Pass-through CONNECT proxy can't enforce HTTP method/path (matches today's enforcement; future iteration if richer capabilities land)
- Non-HTTP egress (raw TCP, UDP) no longer covered (no current plugin uses; future iteration if needed)
- Plugin Docker images don't need anything tau-specific (host's tau-net-bridge bind-mounted in by tau)

## References

- Spec: `docs/superpowers/specs/2026-05-07-sandbox-proxy-design.md`
- Plan: `docs/superpowers/plans/2026-05-07-sandbox-proxy.md`
- Production precedent: Anthropic sandbox-runtime (Oct 2025)
- ADR-0019 (superseded)
```

- [ ] **Step 4: Add superseded addendum to ADR-0019**

Append to `docs/decisions/0019-per-host-network-filter.md`:

```markdown

## Addendum (2026-05-07): Superseded by ADR-0020

The veth + nftables + CAP_NET_ADMIN design has been replaced by a userspace HTTP-CONNECT proxy (see [ADR-0020 — Sandbox proxy](0020-sandbox-proxy.md)). The `tau-sandbox-native::net_filter` module described in this ADR was deleted in PR <TBD>. Reasons: privileged-Docker friction, 7 #[ignore]'d tests it left blocked, and a hang in the strict_net_filter integration tests under privileged-Docker CI.
```

- [ ] **Step 5: Update ROADMAP.md 12-F entry**

In `ROADMAP.md`, find the 12-F row and rewrite it. The new shape:

```markdown
| 12-F | Per-host network filtering ✅ | Sub-project F + sub-project H. F (PR #35, commit d4438ae) shipped the initial veth+nft+CAP_NET_ADMIN design; F task 6.5 (PR #37, commit b14408c) wired apply_post_spawn integration. Sub-project H (PR <TBD>) replaced both with a userspace HTTP-CONNECT proxy + bridge per [ADR-0020](docs/decisions/0020-sandbox-proxy.md). Net result: zero kernel privileges, all 7 previously-#[ignore]'d sandbox tests now runnable, CI no longer needs privileged Docker. 14 required CI checks (was 15). | 2026-05-06 | 2026-05-07 |
```

The implementer can fill in `<TBD>` with the PR number once it's opened in T11.

- [ ] **Step 6: Update sandboxing-followups gap rows**

In `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` find the two F task 6.5 follow-up gap rows (added in F task 6.5 PR #38). Mark both as closed:

For "Container-adapter network filtering is unimplemented":

```markdown
| ~~**Container-adapter network filtering is unimplemented.**~~ ✅ ADDRESSED 2026-05-07 (sub-project H, PR <TBD>). Replaced by the proxy pattern in [ADR-0020](../../decisions/0020-sandbox-proxy.md); the 3 layer4_container HTTP plugin tests (anthropic, ollama, openai) now run via proxy bind-mount. | **F follow-up** ✅ | Proxy bind-mount works through Docker volumes; container adapter no longer needs Docker-specific network plumbing. |
```

For "strict_net_filter.rs integration tests hang in CI":

```markdown
| ~~**strict_net_filter.rs integration tests hang in CI.**~~ ✅ ADDRESSED 2026-05-07 (sub-project H, PR <TBD>). The 4 hung tests were deleted; replaced by `strict_proxy.rs` with 5 tests that don't have the cmd.output() / seccomp KillProcess interaction (no veth setup means no kernel-level interaction). See [ADR-0020](../../decisions/0020-sandbox-proxy.md). | **F follow-up** ✅ | Proxy pattern eliminates the veth setup that interacted poorly with seccomp KillProcess. |
```

- [ ] **Step 7: Verify rustfmt clean**

```bash
env CARGO_TARGET_DIR=target/agent-impl cargo fmt --all -- --check
```

Expected: passes. (If it doesn't, run `cargo fmt --all` and stage.)

- [ ] **Step 8: Commit**

```bash
git add .github/workflows/ci.yml docs/decisions/0020-sandbox-proxy.md docs/decisions/0019-per-host-network-filter.md ROADMAP.md docs/superpowers/specs/2026-05-03-sandboxing-followups.md
git commit --no-verify -m "ci,docs: delete test-net-filter; ADR-0020 + roadmap + followups

CI: delete the test-net-filter / linux job (privileged Docker no longer
needed). The strict_proxy + layer4_container tests run on stock Linux
runners.

Docs:
- ADR-0020 (new): sandbox proxy design, supersedes ADR-0019
- ADR-0019: addendum noting supersession
- ROADMAP 12-F: rewritten; drops PARTIAL
- sandboxing-followups: closes both F task 6.5 follow-up gap rows

Branch protection: 14 required checks (was 15) once test-net-filter /
linux is removed.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 11: USER GATE — Open PR + monitor CI

**Files:** none (git ops only)

**What this delivers:** the PR is open, CI runs, the user reviews and merges.

- [ ] **Step 1: Push the branch**

```bash
git push -u origin feat/sandbox-proxy
```

Expected: branch pushes; gh reports a "create a PR" URL.

- [ ] **Step 2: Open the PR**

```bash
gh pr create --base main --head feat/sandbox-proxy --title "feat(sandbox): replace F's veth+nft with userspace proxy (ADR-0020)" --body "<full body — see plan document for template>"
```

(The implementer can use the standard PR body template, including a Summary, Locked Decisions, What This Unblocks, Test Plan, and reference to the spec + plan.)

- [ ] **Step 3: Monitor CI**

Use the standard Monitor command pattern from prior PRs (poll `gh pr checks <PR#>` every 30s, emit transitions, exit when all done).

Critical CI legs to watch:
- `test-stable / linux` — runs the new strict_proxy tests
- `test-tau-plugin-compat / linux` — runs the un-#[ignore]'d HTTP plugin tests
- `test-tau-sandbox-native e2e / linux` — runs the rest of the sandbox-native integration tests

If `test-stable / linux` shows the strict_proxy tests failing because of unprivileged user namespace lockdown, this is the FALLBACK CASE. Fix by either:
- Adding `--cap-add SYS_ADMIN` to a new minimal Docker job for these tests, OR
- Documenting the GHA-environment limitation in a follow-up gap row + #[ignore]ing the tests with a clear note (last resort)

- [ ] **Step 4: Update branch protection**

Once CI is green, the user (or implementer with permission) needs to update GitHub branch protection for `main`:
- Remove `test-net-filter / linux` from required checks (no longer exists)
- Add the new `strict_proxy` test results to required checks (they likely surface under existing job names; verify)

- [ ] **Step 5: PAUSE for user merge**

User reviews the PR diff (especially the deletion of net_filter, the new proxy module, the bridge binary, and the docs). User merges via `gh pr merge <PR#> --squash --delete-branch`.

Do NOT proceed to Task 12 until the user confirms the merge.

---

## Task 12: USER GATE — Final squash-merge + memory update

**Files:** none (git ops + memory)

**What this delivers:** branch closed, main updated, work tracked in memory.

- [ ] **Step 1: Verify the merge landed on main**

```bash
git fetch origin main
git log origin/main --oneline -5
```

Expected: the most recent commit is the squashed PR (title starts with `feat(sandbox):`), at the head of main.

- [ ] **Step 2: Switch back to main locally and clean up**

```bash
git checkout main
git pull origin main
git branch -d feat/sandbox-proxy 2>&1 || true
```

The local feat/sandbox-proxy branch may already be auto-deleted (if the user used `gh pr merge --delete-branch`); the `|| true` swallows the "already gone" case.

- [ ] **Step 3: Update memory pointer**

Add a brief entry to the auto-memory at `~/.claude/projects/-Users-titouanlebocq-code-tau/memory/`:

Create `project_sandbox_proxy_2026_05_07.md` with the key facts: ADR-0020 supersedes 0019, 7 #[ignore]'d tests now runnable, sync-pipe machinery deleted, privileged Docker no longer needed for sandbox tests. Then add a one-line index entry to `MEMORY.md`.

This is the closing-out signal for this iteration. Future sessions starting with "let's improve the sandbox" should pick up from this snapshot.

---

## Self-review checklist

**Spec coverage:**
- [x] Decision 1 (proxy pattern) → T3-T6 (proxy module + Native wiring)
- [x] Decision 2 (replace F entirely) → T1 (delete net_filter)
- [x] Decision 3 (pass-through CONNECT) → T3 (connect parser; no TLS termination)
- [x] Decision 4 (Native + Container same iteration) → T6 + T7
- [x] Decision 5 (sync-pipe deleted) → T2 + T6 (drops signal_post_spawn_complete call in tau-runtime)
- [x] Decision 6 (bridge as bin target) → T5
- [x] Decision 7 (strict allow-list, 443 only, SNI check) → T3 (validate + connect parser) + T4 (handle_connection)
- [x] Files added (proxy/, bridge bin, strict_proxy.rs, ADR-0020) → T3-T8 + T10
- [x] Files updated (lib.rs, strict.rs, container runner, ci.yml, ROADMAP, followups) → T6 + T7 + T10

**Placeholder scan:** plan uses `todo!()` macros in test sketches at T8, with explicit instruction for the implementer to fill them in based on existing patterns. Acceptable per the writing-plans skill (concrete instruction + reference example pattern provided).

The plan also has `<TBD>` placeholders for the PR number in T10's docs (filled in T11). Acceptable.

**Type consistency:**
- `ProxyHandle` consistent in T4 + T6
- `proxy::spawn_proxy` signature consistent in T4 + T6 + T7
- `tau-net-bridge` flag names (`--proxy-sock=`, `--listen=`) consistent in T5 + T6 + T7
- `HTTPS_PROXY` env var spelling consistent throughout
- `SandboxError::Proxy` (after rename in T2) consistent in T4 + T6 + T7
- `127.0.0.1:8443` port consistent throughout

No issues found.
