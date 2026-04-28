//! Capability satisfies-relation. Determines whether an agent's
//! granted capabilities cover the capabilities required by a tool
//! invocation. Pure logic, no I/O.
//!
//! All helpers are `pub(crate)` — capability satisfaction is a kernel-
//! internal concern (used by the dispatcher to gate tool invocations);
//! it is intentionally not part of the public `tau-runtime` API.
//!
//! # Conservatism note
//!
//! v0.1 chooses safe-conservative defaults at every ambiguous edge:
//! a bounded grant cannot satisfy an unbounded request; an unknown
//! `Custom` namespace requires exact key+value parity. Tightening
//! later is non-breaking; loosening later is a security regression.
//!
//! # Dead-code allow
//!
//! Most helpers in this module are exercised both by the in-module
//! `tests` submodule and by the runtime agent run loop ([`crate::run`],
//! Task 10). A few internals (e.g. the per-namespace `*_satisfies`
//! helpers) are reached only transitively through
//! [`capability_satisfies`] and would warn under the
//! `dead_code` lint when their direct callers are limited to tests; we
//! keep the module-level `allow` to suppress those rather than
//! one-by-one annotations.

#![allow(dead_code)]

use std::collections::BTreeMap;

use tau_domain::{
    AgentCapability, Capability, FsCapability, NetCapability, ProcessCapability, Value,
};

/// Returns `true` iff `granted` covers `required` for one capability pair.
///
/// Different namespaces never satisfy one another — a `Filesystem(Read)`
/// grant does not cover a `Filesystem(Write)` request, nor does any
/// `Filesystem` grant cover a `Network` request.
pub(crate) fn capability_satisfies(granted: &Capability, required: &Capability) -> bool {
    match (granted, required) {
        (Capability::Filesystem(g), Capability::Filesystem(r)) => fs_satisfies(g, r),
        (Capability::Network(g), Capability::Network(r)) => net_satisfies(g, r),
        (Capability::Process(g), Capability::Process(r)) => process_satisfies(g, r),
        (Capability::Agent(g), Capability::Agent(r)) => agent_satisfies(g, r),
        (
            Capability::Custom {
                name: gn,
                params: gp,
            },
            Capability::Custom {
                name: rn,
                params: rp,
            },
        ) => gn == rn && custom_params_satisfy(gp, rp),
        _ => false,
    }
}

/// Top-level check: every required capability must be satisfied by at
/// least one grant. Returns `Some(&first_missing)` on the first
/// required capability with no covering grant, or `None` if every
/// required is covered.
pub(crate) fn check_capabilities<'a>(
    granted: &[Capability],
    required: &'a [Capability],
) -> Option<&'a Capability> {
    required
        .iter()
        .find(|req| !granted.iter().any(|g| capability_satisfies(g, req)))
}

pub(crate) fn fs_satisfies(granted: &FsCapability, required: &FsCapability) -> bool {
    match (granted, required) {
        (FsCapability::Read { paths: gp, .. }, FsCapability::Read { paths: rp, .. }) => {
            paths_subset(gp, rp)
        }
        (
            FsCapability::Write {
                paths: gp,
                max_bytes: gmb,
                ..
            },
            FsCapability::Write {
                paths: rp,
                max_bytes: rmb,
                ..
            },
        ) => paths_subset(gp, rp) && max_bytes_satisfies(*gmb, *rmb),
        (FsCapability::Exec { paths: gp, .. }, FsCapability::Exec { paths: rp, .. }) => {
            paths_subset(gp, rp)
        }
        _ => false,
    }
}

pub(crate) fn net_satisfies(granted: &NetCapability, required: &NetCapability) -> bool {
    match (granted, required) {
        (
            NetCapability::Http {
                hosts: gh,
                methods: gm,
                ..
            },
            NetCapability::Http {
                hosts: rh,
                methods: rm,
                ..
            },
        ) => paths_subset(gh, rh) && string_subset(gm, rm),
        // Future `NetCapability` variants added in tau-domain default
        // to deny — additive evolution must not silently widen grants.
        _ => false,
    }
}

pub(crate) fn process_satisfies(granted: &ProcessCapability, required: &ProcessCapability) -> bool {
    match (granted, required) {
        (
            ProcessCapability::Spawn { commands: gc, .. },
            ProcessCapability::Spawn { commands: rc, .. },
        ) => paths_subset(gc, rc),
        _ => false,
    }
}

pub(crate) fn agent_satisfies(granted: &AgentCapability, required: &AgentCapability) -> bool {
    match (granted, required) {
        (
            AgentCapability::Spawn {
                allowed_kinds: gk, ..
            },
            AgentCapability::Spawn {
                allowed_kinds: rk, ..
            },
        ) => string_subset(gk, rk),
        _ => false,
    }
}

/// Conservative v0.1 rule: every required key must exist in granted
/// with an equal `Value`. Extra granted keys are allowed; extra
/// required keys (missing from granted) deny.
pub(crate) fn custom_params_satisfy(
    granted: &BTreeMap<String, Value>,
    required: &BTreeMap<String, Value>,
) -> bool {
    required
        .iter()
        .all(|(k, rv)| granted.get(k).is_some_and(|gv| gv == rv))
}

/// `max_bytes` satisfies if the grant is unbounded (`None`) OR the
/// grant's bound is `>=` the request's bound. A bounded grant cannot
/// cover an unbounded request — that would silently widen the cap.
fn max_bytes_satisfies(granted: Option<u64>, required: Option<u64>) -> bool {
    match (granted, required) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(g), Some(r)) => g >= r,
    }
}

/// Every required string must match at least one grant pattern under
/// the path/host glob rules (`**`, `*`, exact).
fn paths_subset(granted: &[String], required: &[String]) -> bool {
    required
        .iter()
        .all(|r| granted.iter().any(|g| glob_matches(g, r)))
}

/// Every required string must equal at least one grant string. Used
/// for HTTP methods and agent-spawn kinds — non-glob string sets.
fn string_subset(granted: &[String], required: &[String]) -> bool {
    required.iter().all(|r| granted.contains(r))
}

/// Glob matcher. Splits on `/`. `**` matches zero or more segments,
/// `*` matches exactly one segment (no `/`), other segments match
/// literally. v0.1 does NOT support partial-segment globs like
/// `pre*.txt` — full segment only or full wildcard. Used for both
/// filesystem paths AND HTTP hosts (`*.example.com`); host segments
/// are split on `/` which is fine because hostnames don't contain `/`.
fn glob_matches(pattern: &str, candidate: &str) -> bool {
    let p_segs: Vec<&str> = pattern.split('/').collect();
    let c_segs: Vec<&str> = candidate.split('/').collect();
    glob_segs(&p_segs, &c_segs)
}

fn glob_segs(pattern: &[&str], candidate: &[&str]) -> bool {
    match (pattern.first(), candidate.first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(&"**"), _) => {
            // `**` matches zero or more segments. Try every suffix.
            (0..=candidate.len()).any(|i| glob_segs(&pattern[1..], &candidate[i..]))
        }
        (Some(_), None) => false,
        (Some(&p), Some(&c)) => segment_matches(p, c) && glob_segs(&pattern[1..], &candidate[1..]),
    }
}

/// Single-segment match. Three accepted forms:
///
/// 1. `*` — covers any single segment.
/// 2. `*.suffix` — host-style leading wildcard; matches if the
///    candidate ends with `.suffix` (e.g. `*.example.com` covers
///    `api.example.com`). The candidate must be strictly longer than
///    the suffix so that bare `example.com` is NOT covered by
///    `*.example.com` — that would silently widen the grant.
/// 3. exact string equality.
///
/// Other partial-segment forms (`pre*.txt`, `*foo*`) are deliberately
/// unsupported at v0.1 — full segment, full wildcard, or host suffix.
fn segment_matches(pattern: &str, candidate: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix("*.") {
        // Bare `*.` is meaningless; treat as no-match.
        if suffix.is_empty() {
            return false;
        }
        // Candidate must end with `.suffix` AND be strictly longer
        // than `.suffix` (so the wildcard prefix is non-empty).
        let dotted = format!(".{suffix}");
        return candidate.len() > dotted.len() && candidate.ends_with(&dotted);
    }
    pattern == candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct CapWrapper {
        cap: Capability,
    }

    /// Construct a `Capability` from a TOML fragment that defines
    /// `[cap]` with the flat dot-namespaced shape (per ADR-0002).
    /// Variant-level `#[non_exhaustive]` blocks struct-literal
    /// construction from outside `tau-domain`, so tests round-trip
    /// through the canonical TOML form instead.
    fn cap(toml_str: &str) -> Capability {
        toml::from_str::<CapWrapper>(toml_str)
            .expect("test capability TOML must parse")
            .cap
    }

    // -------------------- Filesystem --------------------

    #[test]
    fn fs_read_grant_satisfies_fs_read_required() {
        let granted = cap(r#"[cap]
kind = "fs.read"
paths = ["/tmp/**"]
"#);
        let required = cap(r#"[cap]
kind = "fs.read"
paths = ["/tmp/foo.txt"]
"#);
        assert!(capability_satisfies(&granted, &required));
    }

    #[test]
    fn fs_read_grant_does_not_satisfy_fs_write_required() {
        let granted = cap(r#"[cap]
kind = "fs.read"
paths = ["/tmp/**"]
"#);
        let required = cap(r#"[cap]
kind = "fs.write"
paths = ["/tmp/foo.txt"]
"#);
        assert!(!capability_satisfies(&granted, &required));
    }

    #[test]
    fn fs_glob_grant_satisfies_specific_required() {
        let granted = cap(r#"[cap]
kind = "fs.read"
paths = ["/tmp/**"]
"#);
        let required = cap(r#"[cap]
kind = "fs.read"
paths = ["/tmp/sub/file.txt"]
"#);
        assert!(capability_satisfies(&granted, &required));
    }

    #[test]
    fn fs_specific_grant_does_not_satisfy_glob_required() {
        // A grant of one literal path does NOT cover an open-ended
        // `/tmp/**` request — that would silently widen the cap.
        let granted = cap(r#"[cap]
kind = "fs.read"
paths = ["/tmp/foo.txt"]
"#);
        let required = cap(r#"[cap]
kind = "fs.read"
paths = ["/tmp/**"]
"#);
        assert!(!capability_satisfies(&granted, &required));
    }

    #[test]
    fn fs_write_max_bytes_unbounded_grant_satisfies_bounded_required() {
        let granted = cap(r#"[cap]
kind = "fs.write"
paths = ["/tmp/**"]
"#);
        let required = cap(r#"[cap]
kind = "fs.write"
paths = ["/tmp/foo.txt"]
max_bytes = 1024
"#);
        assert!(capability_satisfies(&granted, &required));
    }

    #[test]
    fn fs_write_bounded_grant_satisfies_smaller_required() {
        let granted = cap(r#"[cap]
kind = "fs.write"
paths = ["/tmp/**"]
max_bytes = 2048
"#);
        let required = cap(r#"[cap]
kind = "fs.write"
paths = ["/tmp/foo.txt"]
max_bytes = 1024
"#);
        assert!(capability_satisfies(&granted, &required));
    }

    #[test]
    fn fs_write_bounded_grant_does_not_satisfy_unbounded_required() {
        let granted = cap(r#"[cap]
kind = "fs.write"
paths = ["/tmp/**"]
max_bytes = 2048
"#);
        let required = cap(r#"[cap]
kind = "fs.write"
paths = ["/tmp/foo.txt"]
"#);
        assert!(!capability_satisfies(&granted, &required));
    }

    // -------------------- Network --------------------

    #[test]
    fn net_http_grant_satisfies_subset_methods() {
        let granted = cap(r#"[cap]
kind = "net.http"
hosts = ["api.example.com"]
methods = ["GET", "POST"]
"#);
        let required = cap(r#"[cap]
kind = "net.http"
hosts = ["api.example.com"]
methods = ["GET"]
"#);
        assert!(capability_satisfies(&granted, &required));
    }

    #[test]
    fn net_http_grant_does_not_satisfy_method_outside_grant() {
        let granted = cap(r#"[cap]
kind = "net.http"
hosts = ["api.example.com"]
methods = ["GET"]
"#);
        let required = cap(r#"[cap]
kind = "net.http"
hosts = ["api.example.com"]
methods = ["DELETE"]
"#);
        assert!(!capability_satisfies(&granted, &required));
    }

    #[test]
    fn net_http_host_glob_satisfies() {
        let granted = cap(r#"[cap]
kind = "net.http"
hosts = ["*.example.com"]
methods = ["GET"]
"#);
        let required = cap(r#"[cap]
kind = "net.http"
hosts = ["api.example.com"]
methods = ["GET"]
"#);
        assert!(capability_satisfies(&granted, &required));
    }

    // -------------------- Process --------------------

    #[test]
    fn process_spawn_subset_satisfies() {
        let granted = cap(r#"[cap]
kind = "process.spawn"
commands = ["git", "cargo"]
"#);
        let required = cap(r#"[cap]
kind = "process.spawn"
commands = ["git"]
"#);
        assert!(capability_satisfies(&granted, &required));
    }

    // -------------------- Agent --------------------

    #[test]
    fn agent_spawn_subset_satisfies() {
        let granted = cap(r#"[cap]
kind = "agent.spawn"
allowed_kinds = ["worker", "planner"]
"#);
        let required = cap(r#"[cap]
kind = "agent.spawn"
allowed_kinds = ["worker"]
"#);
        assert!(capability_satisfies(&granted, &required));
    }

    // -------------------- Custom --------------------

    #[test]
    fn custom_params_exact_match_satisfies() {
        let granted = cap(r#"[cap]
kind = "mcp.tool.use"
servers = ["fs-mcp"]
"#);
        let required = cap(r#"[cap]
kind = "mcp.tool.use"
servers = ["fs-mcp"]
"#);
        assert!(capability_satisfies(&granted, &required));
    }

    #[test]
    fn custom_params_extra_required_key_fails() {
        // Required has an extra `mode` key not present in the grant —
        // conservative deny.
        let granted = cap(r#"[cap]
kind = "mcp.tool.use"
servers = ["fs-mcp"]
"#);
        let required = cap(r#"[cap]
kind = "mcp.tool.use"
servers = ["fs-mcp"]
mode = "strict"
"#);
        assert!(!capability_satisfies(&granted, &required));
    }

    #[test]
    fn custom_different_names_fail() {
        let granted = cap(r#"[cap]
kind = "mcp.tool.use"
servers = ["fs-mcp"]
"#);
        let required = cap(r#"[cap]
kind = "mcp.resource.read"
servers = ["fs-mcp"]
"#);
        assert!(!capability_satisfies(&granted, &required));
    }

    // -------------------- check_capabilities --------------------

    #[test]
    fn check_capabilities_with_empty_required_returns_none() {
        let granted: Vec<Capability> = vec![];
        let required: Vec<Capability> = vec![];
        assert!(check_capabilities(&granted, &required).is_none());
    }

    #[test]
    fn check_capabilities_returns_first_missing_when_some_unsatisfied() {
        let granted = vec![cap(r#"[cap]
kind = "fs.read"
paths = ["/tmp/**"]
"#)];
        let required = vec![
            cap(r#"[cap]
kind = "fs.read"
paths = ["/tmp/foo.txt"]
"#),
            cap(r#"[cap]
kind = "fs.write"
paths = ["/tmp/foo.txt"]
"#),
            cap(r#"[cap]
kind = "net.http"
hosts = ["api.example.com"]
methods = ["GET"]
"#),
        ];
        let missing =
            check_capabilities(&granted, &required).expect("expected a missing capability");
        // First missing = the fs.write request (index 1), since the
        // fs.read at index 0 is satisfied by the /tmp/** grant.
        match missing {
            Capability::Filesystem(FsCapability::Write { .. }) => {}
            other => panic!("expected first missing = fs.write, got {:?}", other),
        }
    }

    #[test]
    fn check_capabilities_returns_none_when_all_satisfied() {
        let granted = vec![
            cap(r#"[cap]
kind = "fs.read"
paths = ["/tmp/**"]
"#),
            cap(r#"[cap]
kind = "net.http"
hosts = ["*.example.com"]
methods = ["GET", "POST"]
"#),
        ];
        let required = vec![
            cap(r#"[cap]
kind = "fs.read"
paths = ["/tmp/foo.txt"]
"#),
            cap(r#"[cap]
kind = "net.http"
hosts = ["api.example.com"]
methods = ["GET"]
"#),
        ];
        assert!(check_capabilities(&granted, &required).is_none());
    }
}
