# Capability Override Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement project-tau.toml `[agents.<id>.capabilities]` overrides with intersect-only semantics (allow narrowing + deny carve-outs), realizing ADR-0007 §4 reservation.

**Architecture:** Three layers. (1) tau-runtime gains a `capability_override` module with a glob-subset analyzer and an `EffectiveCapability` builder that side-loads narrowed allow/deny lists onto the package's `Capability`. (2) tau-cli's project parser deserializes the override table, validates with `compute_effective`, and stores the effective set on each `AgentEntry`. (3) Plugin sessions consume a new `SessionContext.deny_entries` field; fs-read and shell honor deny-after-allow on every invoke. Validation runs at parse AND at every runtime load (fail-closed both places). The override never widens; expansion is rejected with typed errors.

**Tech Stack:** Rust 2021, tokio, serde, globset (promoted from per-crate dep to workspace dep), thiserror.

---

## Plan-erratum (carryover constraints from sub-projects 1+2a+2b+2c+priority-3)

Apply preemptively. Do NOT re-derive.

- **`Capability` and inner enums are `#[non_exhaustive]`.** `FsCapability::Read{paths}`, `FsCapability::Write{paths,max_bytes}`, `FsCapability::Exec{paths}`, `NetCapability::Http{hosts,methods}`, `ProcessCapability::Spawn{commands}`, `AgentCapability::Spawn{allowed_kinds}` — none of these can be constructed via struct-literal cross-crate. The override layer **side-loads** narrowed allow/deny on `EffectiveCapability { source, allow_override, deny, max_bytes_override }` rather than re-constructing variants.

- **`tau_ports::SessionContext` is `#[non_exhaustive]`** with current fields `agent_instance_id, session_id, deadline, granted_capabilities`. Task 4 ADDS `deny_entries: Vec<DenyEntry>` plus a `with_deny_entries(deny_entries)` builder. Keep the 3-arg `::new` constructor unchanged. Existing call sites that use `with_granted_capabilities` chain `.with_deny_entries(...)` after.

- **`DenyEntry` is a NEW `#[non_exhaustive]` struct** in `tau-ports::tool` with fields `kind: String, deny: Vec<String>`. Module-level doctests must be `ignore`-marked (cross-crate construction blocked). Provide a `DenyEntry::new(kind, deny)` constructor.

- **`ProjectConfigError` is `#[non_exhaustive]`.** Task 3 REMOVES `CapabilityOverrideUnsupported` (in-tree only — only `tau-cli`'s own tests reference it; rename `validate_rejects_capability_override` to `validate_accepts_capability_override` and add `validate_rejects_expanding_override`). Task 3 ADDS `CapabilityOverrideExpands { id, kind, reason }`.

- **`RuntimeError` is `#[non_exhaustive]`.** Task 6 ADDS `CapabilityOverrideExpands { kind, reason }`. Additive non-breaking.

- **`RunOptions` is `#[non_exhaustive]`.** Task 5 adds `project_override: Vec<CapabilityOverride>` (default empty `Vec`). `Runtime::run_with_history` signature stays the same; the override flows through options.

- **`globset` promotes to workspace dep.** `crates/tau-plugins/fs-read/Cargo.toml` line 29 currently has `globset = "0.4"` directly. Task 1 promotes it to `[workspace.dependencies]` in the root `Cargo.toml` and updates fs-read to `globset = { workspace = true }`, then adds the workspace-form to tau-runtime.

- **DynTool::invoke signature is unchanged.** Priority-3 already added `&'a SessionContext`. Plugins read `ctx.deny_entries` to populate session state at `init`.

- **Doctests on `#[non_exhaustive]` types must be `ignore`-marked.** `cargo test --all-targets` does NOT include doctests; verify with `cargo test --doc` separately.

- **For tests destructuring `#[non_exhaustive]` enums cross-crate:** `let X { fields, .. } = value else { panic!() };`.

- **Plugins emit `ToolError::BadArgs`** for in-scope-but-bad-target errors (path matches deny). NOT `ToolError::CapabilityDenied` — that's the kernel's domain at `run.rs:272`.

- **No new escape-hatch variants** — plugins reuse existing typed `ToolError`/`Capability` variants only.

- **No new CI jobs.** No new workspace member; no new external service. Branch protection stays at 23 required checks.

---

## File structure

| Path | Status | Purpose |
|------|--------|---------|
| `Cargo.toml` (root) | Modify | Add `globset = "0.4"` to `[workspace.dependencies]` |
| `crates/tau-plugins/fs-read/Cargo.toml` | Modify | Switch to `globset = { workspace = true }` |
| `crates/tau-runtime/Cargo.toml` | Modify | Add `globset = { workspace = true }` |
| `crates/tau-runtime/src/capability_override/mod.rs` | Create | `EffectiveCapability`, `CapabilityOverride`, `OverrideExpandError`, `compute_effective` |
| `crates/tau-runtime/src/capability_override/glob_subset.rs` | Create | `is_glob_subset` + `is_glob_subset_set` |
| `crates/tau-runtime/src/lib.rs` | Modify | Export new module |
| `crates/tau-runtime/src/error.rs` | Modify | Add `RuntimeError::CapabilityOverrideExpands` |
| `crates/tau-runtime/src/options.rs` | Modify | Add `RunOptions.project_override: Vec<CapabilityOverride>` |
| `crates/tau-runtime/src/run.rs` | Modify | Use `compute_effective` at line 120; populate `SessionContext.deny_entries` at line 336 |
| `crates/tau-runtime/tests/capability_override_e2e.rs` | Create | Gated `#![cfg(unix)]` end-to-end test |
| `crates/tau-ports/src/tool.rs` | Modify | Add `SessionContext.deny_entries`, new `DenyEntry` type, `with_deny_entries` builder |
| `crates/tau-cli/src/config/project.rs` | Modify | Replace `Option<toml::Value>` with `Vec<UncheckedCapabilityOverride>`; replace `CapabilityOverrideUnsupported` with `CapabilityOverrideExpands`; populate `AgentEntry.effective_capabilities` |
| `crates/tau-cli/src/cli.rs` | Modify | Add `ListArgs.capabilities: bool` |
| `crates/tau-cli/src/cmd/list.rs` | Modify | Render effective capability set when `--capabilities` is set; JSON shape too |
| `crates/tau-cli/src/cmd/run.rs` | Modify | Pass `agent_entry.effective_capabilities` into `RunOptions.project_override` |
| `crates/tau-cli/src/cmd/chat.rs` | Modify | Same (mirror) |
| `crates/tau-cli/tests/list_agents_capabilities.rs` | Create | assert_cmd integration tests for the new flag |
| `crates/tau-plugins/fs-read/src/path_check.rs` | Modify | Add `admit_with_deny(path, allow, deny)` |
| `crates/tau-plugins/fs-read/src/plugin.rs` | Modify | `FsReadSession.denied_globs`; consume from `ctx.deny_entries`; call `admit_with_deny` |
| `crates/tau-plugins/fs-read/tests/invoke.rs` | Modify | Add `integration_deny_overrides_allow` test |
| `crates/tau-plugins/shell/src/command_check.rs` | Modify | `admit` honors `denied: &[String]` (or new `admit_with_deny`) |
| `crates/tau-plugins/shell/src/plugin.rs` | Modify | `ShellSession.denied_commands`; consume from `ctx.deny_entries` |
| `crates/tau-plugins/shell/tests/invoke.rs` | Modify | Add deny-test |
| `docs/decisions/0007-tau-cli.md` | Modify | §4 amendment: drop "reserved", link to spec |
| `ROADMAP.md` | Modify | Mark Tier 2 priority 4 done |

---

## Task 1: glob-subset analyzer module + `globset` workspace promotion

**Files:**
- Modify: `Cargo.toml` (root) — add globset to workspace deps
- Modify: `crates/tau-plugins/fs-read/Cargo.toml:29` — switch to workspace form
- Modify: `crates/tau-runtime/Cargo.toml` — add `globset = { workspace = true }`
- Create: `crates/tau-runtime/src/capability_override/glob_subset.rs`
- Create: `crates/tau-runtime/src/capability_override/mod.rs` (stub for this task — full content added in Task 2)
- Modify: `crates/tau-runtime/src/lib.rs` — declare the new module

### Steps

- [ ] **Step 1.1: Promote `globset` to workspace deps**

Edit `Cargo.toml` (root) — add the line after the existing `walkdir` line in `[workspace.dependencies]`:

```toml
walkdir         = "2"
globset         = "0.4"
```

- [ ] **Step 1.2: Update fs-read to use workspace form**

Edit `crates/tau-plugins/fs-read/Cargo.toml:29` — replace `globset             = "0.4"` with `globset             = { workspace = true }`.

- [ ] **Step 1.3: Add globset to tau-runtime**

Edit `crates/tau-runtime/Cargo.toml` `[dependencies]` — add after the `base64` line:

```toml
# Used by capability_override::glob_subset for glob-subset analysis when
# a project tau.toml narrows fs.* paths under a package manifest's grant.
globset             = { workspace = true }
```

- [ ] **Step 1.4: Verify `globset` compiles in both crates**

Run: `cargo build --workspace`
Expected: PASS (no other code consumes globset yet in tau-runtime; the dep is dormant).

- [ ] **Step 1.5: Declare the new module in `tau-runtime/src/lib.rs`**

Insert after `pub(crate) mod capability;`:

```rust
pub(crate) mod capability_override;
```

- [ ] **Step 1.6: Create the module stub `mod.rs`**

Create `crates/tau-runtime/src/capability_override/mod.rs`:

```rust
//! Capability override — narrows a package manifest's grants under a
//! project tau.toml `[agents.<id>.capabilities]` table.
//!
//! Realizes ADR-0007 §4 reservation. The override never widens; the
//! parse-time and runtime checks both fail closed.
//!
//! See `docs/superpowers/specs/2026-04-30-capability-override-design.md`.

pub(crate) mod glob_subset;
```

- [ ] **Step 1.7: Write the failing test fixture**

Create `crates/tau-runtime/src/capability_override/glob_subset.rs` with the test module first (TDD):

```rust
//! Glob-subset analyzer — `is_glob_subset(child, parent)` returns true
//! iff every concrete path matched by `child` is also matched by `parent`.
//!
//! Algorithm (in order):
//! 1. Literal equality.
//! 2. Prefix expansion — strip a trailing `**` from `parent`; if `child`
//!    starts with the parent prefix (followed by `/` or end-of-string),
//!    return true.
//! 3. Brace expansion — handle `{a,b}` by enumerating arms and recursing.
//! 4. Bounded sample fallback for `?`/character-class adversarial cases —
//!    generate ≤ 64 sample paths from `child`; assert each matches `parent`.
//! 5. Otherwise, fail-closed (return false).
//!
//! See `docs/superpowers/specs/2026-04-30-capability-override-design.md` §5.

use globset::Glob;

/// Maximum number of sample paths generated for the fallback check.
/// Above this bound, `is_glob_subset` returns false (fail-closed).
const MAX_SAMPLES: usize = 64;

/// True iff every concrete path matched by `child` is also matched by `parent`.
#[allow(dead_code)] // wired up by Task 2
pub(crate) fn is_glob_subset(child: &str, parent: &str) -> bool {
    if child == parent {
        return true;
    }
    if prefix_subset(child, parent) {
        return true;
    }
    if let Some(child_arms) = brace_expand(child) {
        return child_arms.iter().all(|arm| is_glob_subset(arm, parent));
    }
    if let Some(parent_arms) = brace_expand(parent) {
        return parent_arms.iter().any(|arm| is_glob_subset(child, arm));
    }
    sample_subset(child, parent)
}

/// Verify each entry of `children` is a subset of at least one entry of `parents`.
/// Returns `Ok(())` on success, or `Err(child_glob)` naming the first glob
/// that is not a subset.
#[allow(dead_code)] // wired up by Task 2
pub(crate) fn is_glob_subset_set(children: &[String], parents: &[String]) -> Result<(), String> {
    for child in children {
        if !parents.iter().any(|p| is_glob_subset(child, p)) {
            return Err(child.clone());
        }
    }
    Ok(())
}

fn prefix_subset(child: &str, parent: &str) -> bool {
    // Parent of the form "<prefix>/**" admits any path starting with
    // "<prefix>/". `**` alone admits anything (empty prefix).
    let prefix = if let Some(stripped) = parent.strip_suffix("/**") {
        stripped
    } else if parent == "**" {
        ""
    } else {
        return false;
    };
    if prefix.is_empty() {
        return true;
    }
    child == prefix
        || child.starts_with(&format!("{prefix}/"))
}

/// Expand a single top-level brace alternation. Returns `None` when no
/// brace is present. Nested braces are not handled at v0.1 — if `child`
/// has nested braces the analyzer falls through to the sample fallback.
fn brace_expand(pattern: &str) -> Option<Vec<String>> {
    let open = pattern.find('{')?;
    let close = pattern[open..].find('}').map(|i| open + i)?;
    let prefix = &pattern[..open];
    let suffix = &pattern[close + 1..];
    let arms_str = &pattern[open + 1..close];
    if arms_str.contains('{') {
        return None; // nested brace: defer to sample fallback
    }
    let arms: Vec<String> = arms_str
        .split(',')
        .map(|arm| format!("{prefix}{arm}{suffix}"))
        .collect();
    Some(arms)
}

/// Generate up to `MAX_SAMPLES` concrete paths matching `child`, then
/// assert each matches `parent`. Returns false if the sample budget is
/// exceeded (fail-closed) or if any sample fails to match `parent`.
fn sample_subset(child: &str, parent: &str) -> bool {
    let parent_glob = match Glob::new(parent) {
        Ok(g) => g.compile_matcher(),
        Err(_) => return false,
    };
    let samples = match generate_samples(child, MAX_SAMPLES) {
        Some(s) => s,
        None => return false, // overflow → fail-closed
    };
    samples.iter().all(|p| parent_glob.is_match(p))
}

/// Generate sample paths by enumerating `?` (one ASCII letter) and simple
/// character classes `[abc]`. `*` and `**` expand to a fixed seed string.
/// Returns `None` if the sample count would exceed `cap`.
fn generate_samples(child: &str, cap: usize) -> Option<Vec<String>> {
    let mut accum: Vec<String> = vec![String::new()];
    let mut chars = child.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '?' => {
                let next: Vec<String> = accum
                    .iter()
                    .flat_map(|s| ['a', 'b'].iter().map(move |ch| format!("{s}{ch}")))
                    .collect();
                if next.len() > cap {
                    return None;
                }
                accum = next;
            }
            '[' => {
                let mut class = String::new();
                for cc in chars.by_ref() {
                    if cc == ']' {
                        break;
                    }
                    class.push(cc);
                }
                let chars_in_class: Vec<char> = class.chars().collect();
                if chars_in_class.is_empty() {
                    return None;
                }
                let next: Vec<String> = accum
                    .iter()
                    .flat_map(|s| chars_in_class.iter().map(move |ch| format!("{s}{ch}")))
                    .collect();
                if next.len() > cap {
                    return None;
                }
                accum = next;
            }
            '*' => {
                // `*` and `**` expand to a fixed seed; this is enough to
                // probe parent admission for the structural cases.
                if matches!(chars.peek(), Some('*')) {
                    chars.next();
                    accum = accum.iter().map(|s| format!("{s}seed/path")).collect();
                } else {
                    accum = accum.iter().map(|s| format!("{s}seed")).collect();
                }
            }
            other => {
                accum = accum.iter().map(|s| format!("{s}{other}")).collect();
            }
        }
        if accum.len() > cap {
            return None;
        }
    }
    Some(accum)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_equality_is_subset() {
        assert!(is_glob_subset("/tmp/foo", "/tmp/foo"));
    }

    #[test]
    fn prefix_expansion_strict_subdir() {
        assert!(is_glob_subset("/proj/src/**", "/proj/**"));
    }

    #[test]
    fn prefix_expansion_concrete_path_under_double_star() {
        assert!(is_glob_subset("/proj/src/main.rs", "/proj/**"));
    }

    #[test]
    fn prefix_expansion_match_at_prefix_only() {
        assert!(is_glob_subset("/proj", "/proj/**"));
    }

    #[test]
    fn prefix_expansion_root_glob_admits_everything() {
        assert!(is_glob_subset("/anything/anywhere", "/**"));
        assert!(is_glob_subset("/anything", "**"));
    }

    #[test]
    fn disjoint_paths_are_not_subset() {
        assert!(!is_glob_subset("/etc/**", "/proj/src/**"));
    }

    #[test]
    fn parent_more_specific_than_child_is_not_subset() {
        // Child admits /proj/etc/passwd; parent does not.
        assert!(!is_glob_subset("/proj/**", "/proj/src/**"));
    }

    #[test]
    fn brace_expansion_child_all_arms_subset() {
        assert!(is_glob_subset("/proj/{src,docs}/**", "/proj/**"));
    }

    #[test]
    fn brace_expansion_child_one_arm_not_subset_rejects() {
        assert!(!is_glob_subset("/proj/{src,etc}/**", "/proj/src/**"));
    }

    #[test]
    fn brace_expansion_parent_arm_admits_child() {
        assert!(is_glob_subset("/proj/src/**", "/proj/{src,docs}/**"));
    }

    #[test]
    fn nested_braces_falls_through_to_sample_fallback() {
        // Nested braces aren't decomposed at v0.1 — the structural rules
        // bail and the sample fallback runs. With `*` expansion seeding
        // a deterministic path, this still admits since /proj/{a,{b,c}}
        // matches under /proj/**.
        assert!(is_glob_subset("/proj/{a,{b,c}}/file", "/proj/**"));
    }

    #[test]
    fn question_mark_sample_fallback_admits() {
        // `/proj/?` matches one-char names — all admitted under /proj/**.
        assert!(is_glob_subset("/proj/?", "/proj/**"));
    }

    #[test]
    fn character_class_sample_fallback_admits() {
        assert!(is_glob_subset("/proj/[abc]", "/proj/**"));
    }

    #[test]
    fn sample_budget_overflow_fails_closed() {
        // 65 char-class arms would generate 65 samples; budget is 64.
        // We expect false (fail-closed).
        let huge_class: String = (0..65).map(|i| (b'a' + (i % 26) as u8) as char).collect();
        let child = format!("/proj/[{huge_class}]");
        assert!(!is_glob_subset(&child, "/proj/**"));
    }

    #[test]
    fn invalid_parent_glob_is_not_subset() {
        // An invalid parent glob defaults to no-match (defensive).
        assert!(!is_glob_subset("/proj/foo", "[unclosed"));
    }

    #[test]
    fn empty_child_is_subset_only_of_empty_parent() {
        assert!(is_glob_subset("", ""));
        assert!(!is_glob_subset("", "/proj/**"));
    }

    #[test]
    fn is_glob_subset_set_returns_first_offender() {
        let children = vec![
            "/proj/src/**".to_string(),
            "/proj/etc/**".to_string(),
            "/proj/docs/**".to_string(),
        ];
        let parents = vec!["/proj/{src,docs}/**".to_string()];
        let err = is_glob_subset_set(&children, &parents).unwrap_err();
        assert_eq!(err, "/proj/etc/**");
    }

    #[test]
    fn is_glob_subset_set_succeeds_when_all_admitted() {
        let children = vec!["/proj/src/**".to_string(), "/proj/docs/**".to_string()];
        let parents = vec!["/proj/**".to_string()];
        is_glob_subset_set(&children, &parents).unwrap();
    }
}
```

- [ ] **Step 1.8: Run `cargo build --workspace`**

Run: `cargo build --workspace`
Expected: PASS — module compiles (the `#[allow(dead_code)]` attributes silence the unused-fn warnings; Task 2 wires them up).

- [ ] **Step 1.9: Run unit tests**

Run: `cargo test -p tau-runtime --all-targets capability_override::glob_subset`
Expected: 16/16 PASS.

- [ ] **Step 1.10: Run formatter + clippy + doctests**

Run (all four):
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-runtime --doc
cargo build --workspace
```
Expected: all PASS.

- [ ] **Step 1.11: Commit**

```bash
git add Cargo.toml \
        crates/tau-plugins/fs-read/Cargo.toml \
        crates/tau-runtime/Cargo.toml \
        crates/tau-runtime/src/lib.rs \
        crates/tau-runtime/src/capability_override/
git commit -m "feat(runtime): add capability_override::glob_subset analyzer

Adds the glob-subset analyzer that powers project tau.toml capability
narrowing. Algorithm per spec §5: literal equality → prefix expansion
(strip trailing /**) → brace expansion → bounded sample fallback (≤ 64
samples). Fail-closed on overflow.

Promotes globset 0.4 from per-crate fs-read dep to workspace dep so
both fs-read and tau-runtime share a pin.

Refs: docs/superpowers/specs/2026-04-30-capability-override-design.md §5"
```

- [ ] **Step 1.12: Push**

```bash
git push
```
Expected: CI runs (no new jobs, 23 checks).

---

## Task 2: `compute_effective` + `EffectiveCapability`/`CapabilityOverride`/`OverrideExpandError`

**Files:**
- Modify: `crates/tau-runtime/src/capability_override/mod.rs` — full content
- (Tests live in the same module under `#[cfg(test)] mod tests`.)

### Steps

- [ ] **Step 2.1: Replace the stub `mod.rs` with the full module**

Rewrite `crates/tau-runtime/src/capability_override/mod.rs`:

```rust
//! Capability override — narrows a package manifest's grants under a
//! project tau.toml `[agents.<id>.capabilities]` table.
//!
//! See `docs/superpowers/specs/2026-04-30-capability-override-design.md` §6, §7.3.
//!
//! `Capability` and inner enums are `#[non_exhaustive]` — variant fields
//! cannot be constructed cross-crate. The override layer side-loads the
//! narrowed allow-list and deny-list onto an `EffectiveCapability` rather
//! than re-constructing variants.

pub(crate) mod glob_subset;

use tau_domain::{Capability, FsCapability, NetCapability, ProcessCapability};

use self::glob_subset::is_glob_subset_set;

/// Override entry parsed from project tau.toml. Constructed by tau-cli at
/// parse time and passed through to the runtime via `RunOptions.project_override`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilityOverride {
    /// Capability kind discriminator (`fs.read`, `fs.write`, `fs.exec`,
    /// `net.http`, `process.spawn`).
    pub kind: String,
    /// Narrowed allow-list. `None` means "use the source's own field".
    pub allow: Option<Vec<String>>,
    /// Strings to subtract from the effective allow-list.
    pub deny: Vec<String>,
    /// Narrowed `max_bytes` for `fs.write`. `None` means "use the source's value".
    pub max_bytes: Option<u64>,
}

impl CapabilityOverride {
    /// Construct a `CapabilityOverride`. `#[non_exhaustive]` blocks struct-literal
    /// construction outside this crate.
    pub fn new(
        kind: String,
        allow: Option<Vec<String>>,
        deny: Vec<String>,
        max_bytes: Option<u64>,
    ) -> Self {
        Self {
            kind,
            allow,
            deny,
            max_bytes,
        }
    }
}

/// Effective capability after applying the project override.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct EffectiveCapability {
    /// The package-side capability as-given. Field values inside this
    /// struct are NOT narrowed — they remain the package's grant.
    pub source: Capability,
    /// Narrowed allow-list. Same shape as the strings inside `source`
    /// (paths/hosts/commands). `None` means use `source`'s own field.
    pub allow_override: Option<Vec<String>>,
    /// Deny-list to subtract. Empty = no carve-outs.
    pub deny: Vec<String>,
    /// Narrowed `max_bytes` for `fs.write`. `None` means use source's value.
    pub max_bytes_override: Option<u64>,
}

/// Error returned when a project override expands the package's grants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideExpandError {
    /// The capability kind that expanded.
    pub kind: String,
    /// Human-readable reason.
    pub reason: String,
}

impl std::fmt::Display for OverrideExpandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "capability override on {:?} expands package grant: {}",
            self.kind, self.reason
        )
    }
}

impl std::error::Error for OverrideExpandError {}

/// Compute the effective capability set by intersecting `package_caps` with
/// `project_override`. Returns the effective list, or `OverrideExpandError`
/// if any override entry expands the corresponding package grant.
pub fn compute_effective(
    package_caps: &[Capability],
    project_override: &[CapabilityOverride],
) -> Result<Vec<EffectiveCapability>, OverrideExpandError> {
    // Reject duplicate kinds in the override itself.
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for ov in project_override {
        if !seen.insert(ov.kind.as_str()) {
            return Err(OverrideExpandError {
                kind: ov.kind.clone(),
                reason: "duplicate kind in project override".into(),
            });
        }
    }

    // Reject override entries that have no matching package cap, or that
    // target a Capability::Custom.
    for ov in project_override {
        match find_package_cap(package_caps, &ov.kind) {
            None => {
                return Err(OverrideExpandError {
                    kind: ov.kind.clone(),
                    reason: "no matching capability in package manifest".into(),
                });
            }
            Some(cap) if matches!(cap, Capability::Custom { .. }) => {
                return Err(OverrideExpandError {
                    kind: ov.kind.clone(),
                    reason: "custom capabilities are not narrowable at v0.1".into(),
                });
            }
            _ => {}
        }
    }

    // Build the effective list: each package cap with its matching override
    // applied (if any).
    let mut effective: Vec<EffectiveCapability> = Vec::with_capacity(package_caps.len());
    for cap in package_caps {
        let kind = cap_kind(cap);
        let ov = project_override.iter().find(|o| o.kind == kind);
        let entry = match ov {
            None => EffectiveCapability {
                source: cap.clone(),
                allow_override: None,
                deny: Vec::new(),
                max_bytes_override: None,
            },
            Some(ov) => {
                if let Some(allow) = &ov.allow {
                    validate_allow_subset(cap, allow).map_err(|reason| OverrideExpandError {
                        kind: kind.to_string(),
                        reason,
                    })?;
                }
                if let Some(mb) = ov.max_bytes {
                    validate_max_bytes(cap, mb).map_err(|reason| OverrideExpandError {
                        kind: kind.to_string(),
                        reason,
                    })?;
                }
                EffectiveCapability {
                    source: cap.clone(),
                    allow_override: ov.allow.clone(),
                    deny: ov.deny.clone(),
                    max_bytes_override: ov.max_bytes,
                }
            }
        };
        effective.push(entry);
    }
    Ok(effective)
}

fn find_package_cap<'a>(caps: &'a [Capability], kind: &str) -> Option<&'a Capability> {
    caps.iter().find(|c| cap_kind(c) == kind)
}

fn cap_kind(cap: &Capability) -> &'static str {
    match cap {
        Capability::Filesystem(FsCapability::Read { .. }) => "fs.read",
        Capability::Filesystem(FsCapability::Write { .. }) => "fs.write",
        Capability::Filesystem(FsCapability::Exec { .. }) => "fs.exec",
        Capability::Network(NetCapability::Http { .. }) => "net.http",
        Capability::Process(ProcessCapability::Spawn { .. }) => "process.spawn",
        Capability::Agent(_) => "agent.spawn",
        Capability::Custom { .. } => "custom",
        // Catch-all for future Capability variants — typed as expand-rejected
        // until support is added explicitly.
        _ => "unknown",
    }
}

fn validate_allow_subset(cap: &Capability, allow: &[String]) -> Result<(), String> {
    let parents = match cap {
        Capability::Filesystem(FsCapability::Read { paths, .. }) => paths,
        Capability::Filesystem(FsCapability::Write { paths, .. }) => paths,
        Capability::Filesystem(FsCapability::Exec { paths, .. }) => paths,
        Capability::Network(NetCapability::Http { hosts, .. }) => hosts,
        Capability::Process(ProcessCapability::Spawn { commands, .. }) => commands,
        _ => {
            return Err("allow narrowing not supported for this capability kind".into());
        }
    };
    // Filesystem fields are globs → glob-subset analysis. Hosts and commands
    // are exact-match strings → set inclusion.
    if matches!(cap, Capability::Filesystem(_)) {
        is_glob_subset_set(allow, parents).map_err(|offender| {
            format!("allow entry {offender:?} is not a subset of any package grant")
        })
    } else {
        for entry in allow {
            if !parents.iter().any(|p| p == entry) {
                return Err(format!(
                    "allow entry {entry:?} is not in package grant"
                ));
            }
        }
        Ok(())
    }
}

fn validate_max_bytes(cap: &Capability, requested: u64) -> Result<(), String> {
    match cap {
        Capability::Filesystem(FsCapability::Write { max_bytes, .. }) => match max_bytes {
            None => Ok(()), // package = unlimited; any value is a tightening
            Some(pkg_max) if requested <= *pkg_max => Ok(()),
            Some(pkg_max) => Err(format!(
                "max_bytes={requested} exceeds package grant {pkg_max}"
            )),
        },
        _ => Err("max_bytes only meaningful for fs.write".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(json: &str) -> Capability {
        serde_json::from_str(json).expect("test capability JSON must be valid")
    }

    fn ov(kind: &str, allow: Option<Vec<String>>, deny: Vec<String>, max_bytes: Option<u64>) -> CapabilityOverride {
        CapabilityOverride::new(kind.to_string(), allow, deny, max_bytes)
    }

    #[test]
    fn no_override_returns_package_caps_unchanged() {
        let pkg = vec![cap(r#"{"kind":"fs.read","paths":["/proj/**"]}"#)];
        let eff = compute_effective(&pkg, &[]).unwrap();
        assert_eq!(eff.len(), 1);
        assert!(eff[0].allow_override.is_none());
        assert!(eff[0].deny.is_empty());
    }

    #[test]
    fn well_formed_fs_read_override_narrows() {
        let pkg = vec![cap(r#"{"kind":"fs.read","paths":["/proj/**"]}"#)];
        let over = vec![ov("fs.read", Some(vec!["/proj/src/**".into()]), vec!["/proj/secrets/**".into()], None)];
        let eff = compute_effective(&pkg, &over).unwrap();
        assert_eq!(eff[0].allow_override.as_deref().unwrap(), &["/proj/src/**".to_string()]);
        assert_eq!(eff[0].deny, vec!["/proj/secrets/**".to_string()]);
    }

    #[test]
    fn allow_outside_package_scope_rejected() {
        let pkg = vec![cap(r#"{"kind":"fs.read","paths":["/proj/src/**"]}"#)];
        let over = vec![ov("fs.read", Some(vec!["/etc/**".into()]), vec![], None)];
        let err = compute_effective(&pkg, &over).unwrap_err();
        assert_eq!(err.kind, "fs.read");
        assert!(err.reason.contains("not a subset"), "got: {}", err.reason);
    }

    #[test]
    fn override_kind_with_no_matching_package_cap_rejected() {
        let pkg = vec![cap(r#"{"kind":"fs.read","paths":["/proj/**"]}"#)];
        let over = vec![ov("fs.write", Some(vec!["/proj/**".into()]), vec![], None)];
        let err = compute_effective(&pkg, &over).unwrap_err();
        assert_eq!(err.kind, "fs.write");
        assert!(err.reason.contains("no matching"), "got: {}", err.reason);
    }

    #[test]
    fn duplicate_kind_in_override_rejected() {
        let pkg = vec![cap(r#"{"kind":"fs.read","paths":["/proj/**"]}"#)];
        let over = vec![
            ov("fs.read", Some(vec!["/proj/src/**".into()]), vec![], None),
            ov("fs.read", Some(vec!["/proj/docs/**".into()]), vec![], None),
        ];
        let err = compute_effective(&pkg, &over).unwrap_err();
        assert!(err.reason.contains("duplicate"), "got: {}", err.reason);
    }

    #[test]
    fn custom_capability_not_narrowable() {
        let pkg = vec![cap(r#"{"kind":"mcp.tool.use","tool":"x"}"#)];
        let over = vec![ov("mcp.tool.use", Some(vec![]), vec![], None)];
        let err = compute_effective(&pkg, &over).unwrap_err();
        assert!(err.reason.contains("custom"), "got: {}", err.reason);
    }

    #[test]
    fn process_spawn_string_subset_check() {
        let pkg = vec![cap(r#"{"kind":"process.spawn","commands":["git","rg","sed"]}"#)];
        let over = vec![ov("process.spawn", Some(vec!["git".into(), "rg".into()]), vec![], None)];
        let eff = compute_effective(&pkg, &over).unwrap();
        assert_eq!(eff[0].allow_override.as_deref().unwrap(), &["git".to_string(), "rg".to_string()]);
    }

    #[test]
    fn process_spawn_command_outside_package_rejected() {
        let pkg = vec![cap(r#"{"kind":"process.spawn","commands":["git"]}"#)];
        let over = vec![ov("process.spawn", Some(vec!["rm".into()]), vec![], None)];
        let err = compute_effective(&pkg, &over).unwrap_err();
        assert!(err.reason.contains("not in package grant"), "got: {}", err.reason);
    }

    #[test]
    fn fs_write_max_bytes_lower_accepted() {
        let pkg = vec![cap(r#"{"kind":"fs.write","paths":["/proj/build/**"],"max_bytes":5000000}"#)];
        let over = vec![ov("fs.write", None, vec![], Some(1_000_000))];
        let eff = compute_effective(&pkg, &over).unwrap();
        assert_eq!(eff[0].max_bytes_override, Some(1_000_000));
    }

    #[test]
    fn fs_write_max_bytes_higher_rejected() {
        let pkg = vec![cap(r#"{"kind":"fs.write","paths":["/proj/build/**"],"max_bytes":1000000}"#)];
        let over = vec![ov("fs.write", None, vec![], Some(5_000_000))];
        let err = compute_effective(&pkg, &over).unwrap_err();
        assert!(err.reason.contains("exceeds package grant"), "got: {}", err.reason);
    }

    #[test]
    fn fs_write_max_bytes_with_unlimited_package_accepted() {
        let pkg = vec![cap(r#"{"kind":"fs.write","paths":["/proj/build/**"]}"#)];
        let over = vec![ov("fs.write", None, vec![], Some(1_000_000))];
        let eff = compute_effective(&pkg, &over).unwrap();
        assert_eq!(eff[0].max_bytes_override, Some(1_000_000));
    }

    #[test]
    fn deny_with_no_matching_package_path_accepted() {
        // Deny is pure subtraction — no subset check.
        let pkg = vec![cap(r#"{"kind":"fs.read","paths":["/proj/**"]}"#)];
        let over = vec![ov("fs.read", None, vec!["/totally/elsewhere".into()], None)];
        let eff = compute_effective(&pkg, &over).unwrap();
        assert_eq!(eff[0].deny, vec!["/totally/elsewhere".to_string()]);
    }

    #[test]
    fn empty_allow_means_zero_scope() {
        let pkg = vec![cap(r#"{"kind":"fs.read","paths":["/proj/**"]}"#)];
        let over = vec![ov("fs.read", Some(vec![]), vec![], None)];
        let eff = compute_effective(&pkg, &over).unwrap();
        assert_eq!(eff[0].allow_override.as_deref().unwrap(), &[] as &[String]);
    }
}
```

- [ ] **Step 2.2: Run unit tests**

Run: `cargo test -p tau-runtime --all-targets capability_override`
Expected: 13/13 in this module + 16/16 from glob_subset = all PASS.

- [ ] **Step 2.3: Run formatter + clippy + doctests**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-runtime --doc
cargo build --workspace
```
Expected: all PASS.

- [ ] **Step 2.4: Commit**

```bash
git add crates/tau-runtime/src/capability_override/mod.rs
git commit -m "feat(runtime): add compute_effective + EffectiveCapability + CapabilityOverride

Iterates package capabilities; for each, looks up a matching override
entry by kind and validates that allow_override is a glob-subset (or
exact-match for hosts/commands) of the source's grant. Rejects:
- override entries with no matching package cap
- override entries on Capability::Custom (not narrowable at v0.1)
- duplicate kinds
- max_bytes raise

Side-loads narrowed allow-list and deny-list onto EffectiveCapability
rather than re-constructing #[non_exhaustive] variants.

Refs: docs/superpowers/specs/2026-04-30-capability-override-design.md §6.3, §7.3"
```

- [ ] **Step 2.5: Push**

```bash
git push
```

---

## Task 3: tau-cli `[agents.<id>.capabilities]` typed schema

**Files:**
- Modify: `crates/tau-cli/src/config/project.rs` — typed `Vec<UncheckedCapabilityOverride>`, replace error variant, populate `AgentEntry.effective_capabilities`
- (Tests are inline in the same file under `mod tests`.)

This is the parse-time half of validation. The runtime re-check lands in Task 5.

### Steps

- [ ] **Step 3.1: Add the typed override structs**

In `crates/tau-cli/src/config/project.rs`, replace the line `pub capabilities: Option<toml::Value>,` (currently around line 44) with:

```rust
    /// Capability override entries; default empty. Each entry must
    /// match a `kind` declared by the agent's package manifest.
    /// Validation runs in `validate_agent`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<UncheckedCapabilityOverride>,
```

Add a new struct definition immediately after `UncheckedPrompt`:

```rust
/// Single `[[agents.<id>.capabilities]]` array-of-tables entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UncheckedCapabilityOverride {
    /// Capability kind discriminator (`fs.read`, `fs.write`, `fs.exec`,
    /// `net.http`, `process.spawn`).
    pub kind: String,
    /// Narrowed allow-list (paths). Optional; absent = "use package's
    /// allow-list verbatim".
    #[serde(default)]
    pub allow_paths: Option<Vec<String>>,
    /// Path globs to subtract from the effective allow-list.
    #[serde(default)]
    pub deny_paths: Vec<String>,
    /// Narrowed allow-list (hosts) for `net.http`.
    #[serde(default)]
    pub allow_hosts: Option<Vec<String>>,
    /// Hosts to subtract from the effective allow-list (`net.http`).
    #[serde(default)]
    pub deny_hosts: Vec<String>,
    /// Narrowed allow-list (commands) for `process.spawn`.
    #[serde(default)]
    pub allow_commands: Option<Vec<String>>,
    /// Commands to subtract (`process.spawn`).
    #[serde(default)]
    pub deny_commands: Vec<String>,
    /// Narrowed `max_bytes` (only meaningful for `fs.write`).
    #[serde(default)]
    pub max_bytes: Option<u64>,
}
```

- [ ] **Step 3.2: Add the `capability_overrides` field on `AgentEntry`**

In `AgentEntry` (around line 92), append after `prompt`:

```rust
    /// Project-supplied capability overrides (raw, validated only for
    /// shape + duplicate-kind at parse time). The intersect-vs-manifest
    /// check runs at `tau run` time (in tau-runtime) and at
    /// `tau list --capabilities` rendering time. Empty = no override
    /// (effective grant = package manifest verbatim).
    pub capability_overrides: Vec<tau_runtime::capability_override::CapabilityOverride>,
```

- [ ] **Step 3.3: Replace the error variant**

In `ProjectConfigError`, replace the `CapabilityOverrideUnsupported { id }` arm with:

```rust
    /// Project override on `kind` expanded the package's grant. Carries
    /// the agent id, the failing kind, and a human-readable reason.
    #[error(
        "agent {id:?}: capability override on {kind:?} expands the package's grant: {reason}"
    )]
    CapabilityOverrideExpands {
        /// Agent id whose override failed validation.
        id: String,
        /// The capability kind that expanded.
        kind: String,
        /// Human-readable reason from `compute_effective`.
        reason: String,
    },
```

- [ ] **Step 3.4: Wire conversion + parse-time duplicate-kind check into `validate_agent`**

In `validate_agent` (around line 216), replace the existing block

```rust
    if raw.capabilities.is_some() {
        return Err(ProjectConfigError::CapabilityOverrideUnsupported { id });
    }
```

with:

```rust
    // Convert the typed unchecked overrides into runtime-shape
    // CapabilityOverride values. The intersect-vs-manifest check runs
    // at `tau run` time (Task 5) and at `tau list --capabilities`
    // rendering time (Task 9); here we only validate parse-local
    // invariants (duplicate kinds).
    let capability_overrides: Vec<tau_runtime::capability_override::CapabilityOverride> = raw
        .capabilities
        .iter()
        .map(unchecked_to_capability_override)
        .collect();

    {
        use std::collections::BTreeSet;
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for ov in &capability_overrides {
            if !seen.insert(ov.kind.clone()) {
                return Err(ProjectConfigError::CapabilityOverrideExpands {
                    id: id.clone(),
                    kind: ov.kind.clone(),
                    reason: "duplicate kind in project override".into(),
                });
            }
        }
    }
```

And update the `Ok(AgentEntry { ... })` constructor at the end of `validate_agent` to include the new field:
```rust
    Ok(AgentEntry {
        id,
        display_name: raw.display_name,
        package: raw.package,
        llm_backend: raw.llm_backend,
        requires,
        config,
        prompt,
        capability_overrides,
    })
```

- [ ] **Step 3.5: Add the `unchecked_to_capability_override` helper**

Append a free function to the module (private):

```rust
fn unchecked_to_capability_override(
    raw: &UncheckedCapabilityOverride,
) -> tau_runtime::capability_override::CapabilityOverride {
    use tau_runtime::capability_override::CapabilityOverride;

    // Fold the kind-specific allow_* / deny_* fields into a single
    // `(allow, deny)` pair. The runtime cap_kind() picks the right
    // strings based on the matching package capability.
    let (allow, deny) = match raw.kind.as_str() {
        "fs.read" | "fs.write" | "fs.exec" => (raw.allow_paths.clone(), raw.deny_paths.clone()),
        "net.http" => (raw.allow_hosts.clone(), raw.deny_hosts.clone()),
        "process.spawn" => (raw.allow_commands.clone(), raw.deny_commands.clone()),
        _ => (None, Vec::new()),
    };
    CapabilityOverride::new(raw.kind.clone(), allow, deny, raw.max_bytes)
}
```

- [ ] **Step 3.6: Add `tau-runtime` to tau-cli's `[dependencies]`**

In `crates/tau-cli/Cargo.toml`, verify `tau-runtime = { workspace = true }` is already in `[dependencies]`. If not, add it. (It is — tau-cli already runs the agent loop.)

- [ ] **Step 3.7: Update existing test `validate_rejects_capability_override` → `validate_accepts_capability_override`**

In the test module at the bottom of `crates/tau-cli/src/config/project.rs`, replace the existing test (currently around line 351-371):

```rust
    #[test]
    fn validate_accepts_capability_override() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = "anthropic"

            [[agents.r.capabilities]]
            kind        = "fs.read"
            allow_paths = ["${PROJECT}/src/**"]
            deny_paths  = ["${PROJECT}/.env"]
        "#;
        let cfg = parse(toml_str).unwrap();
        let agent = cfg.agents.get("r").unwrap();
        assert_eq!(agent.capability_overrides.len(), 1);
        let ov = &agent.capability_overrides[0];
        assert_eq!(ov.kind, "fs.read");
        assert_eq!(ov.allow.as_deref().unwrap(), &["${PROJECT}/src/**".to_string()]);
        assert_eq!(ov.deny, vec!["${PROJECT}/.env".to_string()]);
    }

    #[test]
    fn validate_rejects_duplicate_kind_in_override() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = "anthropic"

            [[agents.r.capabilities]]
            kind        = "fs.read"
            allow_paths = ["${PROJECT}/src/**"]

            [[agents.r.capabilities]]
            kind        = "fs.read"
            allow_paths = ["${PROJECT}/docs/**"]
        "#;
        let result = parse(toml_str);
        let Err(ProjectConfigError::CapabilityOverrideExpands { id, kind, reason }) = result else {
            panic!("expected CapabilityOverrideExpands: {result:?}")
        };
        assert_eq!(id, "r");
        assert_eq!(kind, "fs.read");
        assert!(reason.contains("duplicate"));
    }

    #[test]
    fn validate_no_capability_block_keeps_overrides_empty() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = "anthropic"
        "#;
        let cfg = parse(toml_str).unwrap();
        assert!(cfg.agents.get("r").unwrap().capability_overrides.is_empty());
    }
```

Delete the original `validate_rejects_capability_override` test entirely.

- [ ] **Step 3.8: Update any test using the old `AgentEntry` struct-literal**

Search-and-replace in this file: any test constructing `AgentEntry { ... }` directly (around line 562 per the spec's grep) needs `capability_overrides: Vec::new(),` added to the struct literal. Run a grep first:

```bash
grep -n "AgentEntry {" crates/tau-cli/src/config/project.rs crates/tau-cli/src/cmd/*.rs crates/tau-cli/tests/*.rs
```

For each hit, add `capability_overrides: Vec::new(),` (or remove the literal in favor of going through `validate_agent` if reasonable).

- [ ] **Step 3.9: Run tests**

```bash
cargo test -p tau-cli --all-targets config::project
```
Expected: All tests pass; the renamed + new tests validate behavior.

- [ ] **Step 3.10: Run full verification**

```bash
cargo build --workspace
cargo test --workspace --all-targets
cargo test -p tau-cli --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```
Expected: all PASS.

- [ ] **Step 3.11: Commit**

```bash
git add crates/tau-cli/src/config/project.rs
git commit -m "feat(cli): typed [[agents.<id>.capabilities]] override schema

Replaces the placeholder Option<toml::Value> shape with a typed
Vec<UncheckedCapabilityOverride>. Each entry mirrors the package
manifest's capability shape and adds allow_* / deny_* / max_bytes
narrowing fields per spec §4.

Replaces ProjectConfigError::CapabilityOverrideUnsupported with
CapabilityOverrideExpands { id, kind, reason }.

Stores the raw override list on AgentEntry.capability_overrides; the
intersect check vs. the package manifest runs at runtime (Task 5).

Refs: docs/superpowers/specs/2026-04-30-capability-override-design.md §4, §7.1"
```

- [ ] **Step 3.12: Push**

```bash
git push
```

---

## Task 4: tau-ports `SessionContext.deny_entries` + `DenyEntry`

**Hybrid format.**

**Files:**
- Modify: `crates/tau-ports/src/tool.rs` — add `DenyEntry` struct, add `deny_entries` field to `SessionContext`, add `with_deny_entries` builder.
- Modify: `crates/tau-ports/src/lib.rs` — re-export `DenyEntry`.

**Spec sections:** §7.4.

**Per-task summary:**

1. Define `DenyEntry`:
   ```rust
   #[non_exhaustive]
   #[derive(Debug, Clone)]
   #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
   pub struct DenyEntry {
       pub kind: String,
       pub deny: Vec<String>,
   }

   impl DenyEntry {
       pub fn new(kind: String, deny: Vec<String>) -> Self {
           Self { kind, deny }
       }
   }
   ```
   Module-level + struct-level rustdoc with an `ignore`-marked example (cross-crate construction blocked).

2. Add field to `SessionContext`:
   ```rust
   #[cfg_attr(feature = "serde", serde(default))]
   pub deny_entries: Vec<DenyEntry>,
   ```
   Default empty in `SessionContext::new`.

3. Add builder:
   ```rust
   pub fn with_deny_entries(mut self, deny_entries: Vec<DenyEntry>) -> Self {
       self.deny_entries = deny_entries;
       self
   }
   ```

4. Re-export `DenyEntry` from `tau-ports/src/lib.rs` next to `SessionContext`.

5. Add unit tests:
   - `session_context_default_deny_entries_is_empty` — verify `SessionContext::new(...)` has `deny_entries.is_empty()`.
   - `session_context_with_deny_entries_replaces_field` — chain the builder, verify the field.
   - `deny_entry_new_round_trips_kind_and_deny` — straightforward constructor test.

6. **Verification:**
   ```bash
   cargo test -p tau-ports --all-targets
   cargo test -p tau-ports --doc
   cargo build --workspace
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```

7. **Commit message:**
   ```
   feat(ports): add SessionContext.deny_entries + DenyEntry type

   Additive non-breaking change (#[non_exhaustive] on SessionContext).
   The new field carries per-capability deny carve-outs from a project
   tau.toml override into the plugin's `init`. Plugins consult deny_entries
   after their allow check passes — deny-wins precedence per spec §9.

   Refs: docs/superpowers/specs/2026-04-30-capability-override-design.md §7.4
   ```

8. Push.

---

## Task 5: `RunOptions.project_override` + runtime integration in `run.rs`

**Hybrid format.**

**Files:**
- Modify: `crates/tau-runtime/src/options.rs` — add `RunOptions.project_override: Vec<CapabilityOverride>`.
- Modify: `crates/tau-runtime/src/run.rs:120-180, 336` — call `compute_effective`, populate `SessionContext.deny_entries`.
- Modify: `crates/tau-runtime/src/lib.rs` — re-export `CapabilityOverride`, `EffectiveCapability`, `OverrideExpandError` at the public surface.
- Modify: `crates/tau-cli/src/cmd/run.rs` and `chat.rs` — populate `RunOptions.project_override` from `agent_entry.capability_overrides`.

**Spec sections:** §6.2, §6.3, §7.4.

**Per-task summary:**

1. Add `project_override` field to `RunOptions`:
   ```rust
   /// Project tau.toml capability override; default empty. Validated
   /// at runtime via `compute_effective` (defense-in-depth — tau-cli
   /// also validates at parse time).
   pub project_override: Vec<CapabilityOverride>,
   ```
   `Default::default()` returns empty Vec.

2. Promote `capability_override` module from `pub(crate)` to `pub` in `crates/tau-runtime/src/lib.rs`:
   ```rust
   pub mod capability_override;
   ```
   And re-export common types:
   ```rust
   pub use capability_override::{CapabilityOverride, EffectiveCapability, OverrideExpandError};
   ```

3. In `run.rs:118-124`, replace the existing `let granted: &[Capability] = package_manifest.capabilities();` block with:
   ```rust
   let effective = crate::capability_override::compute_effective(
       package_manifest.capabilities(),
       &options.project_override,
   )
   .map_err(|e| RuntimeError::CapabilityOverrideExpands {
       kind: e.kind,
       reason: e.reason,
   })?;
   debug!(
       name = "runtime.capability_set_loaded",
       count = effective.len(),
       overrides_applied = options.project_override.len(),
   );
   ```
   (`RuntimeError::CapabilityOverrideExpands` is added in Task 6 — Task 5's commit depends on Task 6 for clean compile. **Pull Task 6 forward and merge with Task 5 if needed**, OR add a temporary stub variant. Cleanest: implement Task 6 first and commit them in order; the hybrid format here just describes intent. **Implementation order: 6 then 5**, despite plan numbering.)

4. Compute the granted slice for downstream call sites — the kernel's structural check at `run.rs:272` and the tool filter at `run.rs:160` should still see the package's structural grants (capability KINDS), not the narrowed allow-list. Build a `granted_for_kernel: Vec<Capability>` by cloning `effective[i].source` for each entry.

5. The `granted_capabilities` field on `SessionContext` (line 336-337) carries the **post-narrow** view. Build it from `effective` by replacing `source.paths` with `allow_override` when present. This requires constructing new `Capability` variants — but `Capability::*::*` are `#[non_exhaustive]` cross-crate. Workaround: serialize the source, splice in the narrowed list, deserialize back. Concrete helper:
   ```rust
   fn narrowed_capability_for_session(eff: &EffectiveCapability) -> Capability {
       use serde_json::{json, Value as Jv};
       let source_json = serde_json::to_value(&eff.source).expect("Capability serializes");
       let mut obj = source_json.as_object().expect("Capability serializes to map").clone();
       if let Some(allow) = &eff.allow_override {
           // Replace the kind-appropriate field.
           let field = match obj.get("kind").and_then(Jv::as_str) {
               Some("fs.read") | Some("fs.write") | Some("fs.exec") => "paths",
               Some("net.http") => "hosts",
               Some("process.spawn") => "commands",
               _ => return eff.source.clone(),
           };
           obj.insert(field.to_string(), json!(allow));
       }
       if let Some(mb) = eff.max_bytes_override {
           obj.insert("max_bytes".to_string(), json!(mb));
       }
       serde_json::from_value(Jv::Object(obj)).expect("narrowed capability deserializes")
   }
   ```
   Add this helper next to the SessionContext construction at `run.rs:336`.

6. Build `deny_entries` from `effective`:
   ```rust
   let deny_entries: Vec<tau_ports::DenyEntry> = effective
       .iter()
       .filter(|e| !e.deny.is_empty())
       .map(|e| {
           let kind = match &e.source {
               Capability::Filesystem(FsCapability::Read { .. }) => "fs.read",
               Capability::Filesystem(FsCapability::Write { .. }) => "fs.write",
               Capability::Filesystem(FsCapability::Exec { .. }) => "fs.exec",
               Capability::Network(NetCapability::Http { .. }) => "net.http",
               Capability::Process(ProcessCapability::Spawn { .. }) => "process.spawn",
               _ => "unknown",
           };
           tau_ports::DenyEntry::new(kind.to_string(), e.deny.clone())
       })
       .collect();
   ```

7. Replace the existing SessionContext construction at `run.rs:336`:
   ```rust
   let granted_for_session: Vec<Capability> =
       effective.iter().map(narrowed_capability_for_session).collect();
   let ctx = SessionContext::new(agent_instance_id, uuid::Uuid::new_v4(), None)
       .with_granted_capabilities(granted_for_session)
       .with_deny_entries(deny_entries.clone());
   ```

8. Update `run.rs:160` (tool-filter loop) and `run.rs:278-279` (per-call kernel cap check) to use `granted_for_kernel` (the un-narrowed structural grants), since narrowing the allow-list doesn't change the capability *kind* — narrowing is enforced plugin-side.

9. Update tau-cli's `run.rs` and `chat.rs` to pass the capability overrides through:
   ```rust
   let mut options = tau_runtime::RunOptions::default();
   options.max_turns = ...;
   options.project_override = agent_entry.capability_overrides.clone();
   ```

10. **Verification:**
    ```bash
    cargo build --workspace
    cargo test -p tau-runtime --all-targets
    cargo test -p tau-cli --all-targets
    cargo test --workspace --doc
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    ```

11. **Commit message:**
    ```
    feat(runtime): wire RunOptions.project_override through dispatch

    `Runtime::run_with_history` now calls `compute_effective` against the
    package manifest + project override. Failure raises
    RuntimeError::CapabilityOverrideExpands (defense-in-depth — tau-cli
    also validates at parse time).

    Tool filtering (run.rs:160) and the structural cap check (run.rs:272)
    keep using the package's own grants; narrowing applies plugin-side
    via SessionContext.granted_capabilities (post-narrow allow-list)
    and SessionContext.deny_entries (carve-outs).

    tau-cli's run + chat commands populate RunOptions.project_override
    from the parsed AgentEntry.capability_overrides.

    Refs: docs/superpowers/specs/2026-04-30-capability-override-design.md §6.2, §6.3, §7.4
    ```

12. Push.

---

## Task 6: `RuntimeError::CapabilityOverrideExpands` typed variant

**Hybrid format.**

**Files:**
- Modify: `crates/tau-runtime/src/error.rs` — add `CapabilityOverrideExpands { kind, reason }` variant.

**Spec sections:** §7.2.

**Per-task summary:**

1. Add to `RuntimeError`:
   ```rust
   /// Project capability override expanded the package's grant.
   /// Raised at runtime as a defense-in-depth check (tau-cli also
   /// rejects at parse time).
   #[error("capability override on {kind:?} expands package grant: {reason}")]
   CapabilityOverrideExpands {
       /// The capability kind that expanded.
       kind: String,
       /// Human-readable reason from `compute_effective`.
       reason: String,
   },
   ```

2. Add a tracing event constant if the error site uses one (see how `runtime.run_failed` is emitted at `run.rs:314`). Concretely in `run.rs`, after the override check fails, emit:
   ```rust
   info!(name = "runtime.capability_override_rejected", kind = %k, reason = %r);
   ```

3. Unit test (in error.rs's `#[cfg(test)] mod tests`):
   ```rust
   #[test]
   fn capability_override_expands_displays_with_kind_and_reason() {
       let err = RuntimeError::CapabilityOverrideExpands {
           kind: "fs.read".into(),
           reason: "/etc/** is not a subset".into(),
       };
       let msg = format!("{err}");
       assert!(msg.contains("fs.read"));
       assert!(msg.contains("not a subset"));
   }
   ```

4. **Verification:** standard 5-command suite.

5. **Commit message:**
   ```
   feat(runtime): RuntimeError::CapabilityOverrideExpands typed variant

   Additive #[non_exhaustive] variant. Surfaces via runtime defense-in-
   depth check when a project override expands the package's grant.

   Refs: docs/superpowers/specs/2026-04-30-capability-override-design.md §7.2
   ```

6. Push.

**Implementation note:** Tasks 5 and 6 are mutually-dependent — Task 5 references the variant added in Task 6. Implement Task 6 first to keep each commit compiling; the plan numbering reflects the design order, not the commit order. Either way is fine — flag this to the controller.

---

## Task 7: fs-read deny enforcement

**Hybrid format.**

**Files:**
- Modify: `crates/tau-plugins/fs-read/src/path_check.rs` — add `admit_with_deny(path, allow, deny)`.
- Modify: `crates/tau-plugins/fs-read/src/plugin.rs` — `FsReadSession.denied_globs`; populate from `ctx.deny_entries`; call `admit_with_deny`.
- Modify: `crates/tau-plugins/fs-read/tests/invoke.rs` — add `integration_deny_overrides_allow` test.

**Spec sections:** §7.5, §9.

**Per-task summary:**

1. Add `admit_with_deny` in `path_check.rs`:
   ```rust
   /// Check `path` is admitted by the allow-list AND not denied. Deny
   /// wins per spec §9.
   pub(crate) fn admit_with_deny(path: &str, allow: &[String], deny: &[String]) -> bool {
       if !admit(path, allow) {
           return false;
       }
       !admit(path, deny) // re-use existing admit() against the deny globs
   }
   ```

2. Unit tests in `path_check.rs`:
   ```rust
   #[test]
   fn admit_with_deny_denies_when_deny_matches() {
       let allow = vec!["/proj/**".to_string()];
       let deny = vec!["/proj/secrets/**".to_string()];
       assert!(!admit_with_deny("/proj/secrets/api.key", &allow, &deny));
   }

   #[test]
   fn admit_with_deny_admits_when_no_deny_matches() {
       let allow = vec!["/proj/**".to_string()];
       let deny = vec!["/proj/secrets/**".to_string()];
       assert!(admit_with_deny("/proj/src/main.rs", &allow, &deny));
   }

   #[test]
   fn admit_with_deny_denies_when_allow_misses() {
       let allow = vec!["/proj/**".to_string()];
       let deny: Vec<String> = vec![];
       assert!(!admit_with_deny("/etc/passwd", &allow, &deny));
   }

   #[test]
   fn admit_with_deny_empty_deny_falls_through_to_allow() {
       let allow = vec!["/proj/**".to_string()];
       let deny: Vec<String> = vec![];
       assert!(admit_with_deny("/proj/foo", &allow, &deny));
   }
   ```

3. Modify `FsReadSession`:
   ```rust
   pub struct FsReadSession {
       allowed_globs: Vec<String>,
       denied_globs: Vec<String>,
   }
   ```

4. Modify `init`:
   ```rust
   async fn init(&self, ctx: SessionContext) -> Result<Self::Session, ToolError> {
       let allowed_globs = extract_fs_read_paths(&ctx.granted_capabilities);
       let denied_globs = ctx
           .deny_entries
           .iter()
           .find(|e| e.kind == "fs.read")
           .map(|e| e.deny.clone())
           .unwrap_or_default();
       Ok(FsReadSession { allowed_globs, denied_globs })
   }
   ```

5. Modify `invoke`'s admission line:
   ```rust
   if !admit_with_deny(path, &session.allowed_globs, &session.denied_globs) {
       return Err(ToolError::BadArgs {
           reason: BadArgs::NotInScope.reason(),
       });
   }
   ```
   The `BadArgs::NotInScope` reason is reused — a deny match is conceptually still "not in scope".

6. Add integration test in `crates/tau-plugins/fs-read/tests/invoke.rs` (gated `#[cfg(unix)]` for tempfile path stability — match the existing test convention):
   ```rust
   #[cfg(unix)]
   #[tokio::test]
   async fn integration_deny_overrides_allow() {
       let tmpfile = tempfile::NamedTempFile::new().unwrap();
       let path = tmpfile.path().to_str().unwrap().to_string();
       std::fs::write(tmpfile.path(), b"secret").unwrap();
       let parent = tmpfile.path().parent().unwrap().to_str().unwrap();
       let allow_glob = format!("{parent}/**");

       let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
       let plugin = FsReadPlugin::from_config(Default::default()).unwrap();
       let runner = tokio::spawn(async move {
           run_tool_with_io(&mut sut_reader, &mut sut_writer, plugin, "fs-read", "0.1.0").await
       });

       do_handshake(&mut peer).await;
       // Allow covers the file; deny lists the exact file → expect rejection.
       let ctx = SessionContext::new(
           AgentInstanceId::new(),
           Uuid::now_v7(),
           Some(SystemTime::UNIX_EPOCH),
       )
       .with_granted_capabilities(vec![fs_read_cap(&[&allow_glob])])
       .with_deny_entries(vec![DenyEntry::new("fs.read".into(), vec![path.clone()])]);
       send_tool_call(&mut peer, 2, &ctx, serde_json::json!({ "path": path })).await;
       let err = recv_tool_response(&mut peer)
           .await
           .expect_err("expected scope-rejection RPC error");
       assert!(
           err.contains("not in capability scope"),
           "expected scope-violation error; got: {err}"
       );
       shutdown(&mut peer).await;
       drop(peer);
       let _ = runner.await;
   }
   ```

7. **Verification:**
   ```bash
   cargo build --workspace
   cargo test -p fs-read --all-targets
   cargo test -p tau-runtime --all-targets
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```

8. **Commit message:**
   ```
   feat(fs-read): honor deny_entries (deny wins after allow)

   FsReadSession now carries denied_globs alongside allowed_globs,
   populated from SessionContext.deny_entries at init. Path admission
   first checks allow, then rejects on deny match — deny-wins precedence
   per spec §9.

   Refs: docs/superpowers/specs/2026-04-30-capability-override-design.md §7.5, §9
   ```

9. Push.

---

## Task 8: shell deny enforcement

**Hybrid format.**

**Files:**
- Modify: `crates/tau-plugins/shell/src/command_check.rs` — add deny check.
- Modify: `crates/tau-plugins/shell/src/plugin.rs` — `ShellSession.denied_commands`; populate from `ctx.deny_entries`.
- Modify: `crates/tau-plugins/shell/tests/invoke.rs` — add deny integration test.

**Spec sections:** §7.5, §9.

**Per-task summary:**

1. Add `admit_with_deny(command, allow, deny)` in `command_check.rs`:
   ```rust
   pub(crate) fn admit_with_deny(command: &str, allow: &[String], deny: &[String]) -> bool {
       if !admit(command, allow) {
           return false;
       }
       !deny.iter().any(|d| d == command)
   }
   ```
   Plus 3 unit tests mirroring fs-read's pattern (deny-match denies, no-deny-match falls through, allow-miss still denies).

2. Modify `ShellSession`:
   ```rust
   pub struct ShellSession {
       allowed_commands: Vec<String>,
       denied_commands: Vec<String>,
   }
   ```

3. Modify `init`:
   ```rust
   async fn init(&self, ctx: SessionContext) -> Result<Self::Session, ToolError> {
       let allowed_commands = extract_allowed_commands(&ctx.granted_capabilities);
       let denied_commands = ctx
           .deny_entries
           .iter()
           .find(|e| e.kind == "process.spawn")
           .map(|e| e.deny.clone())
           .unwrap_or_default();
       Ok(ShellSession { allowed_commands, denied_commands })
   }
   ```

4. Replace the admit call in `invoke` (`plugin.rs:119`):
   ```rust
   if !admit_with_deny(&parsed.command, &session.allowed_commands, &session.denied_commands) {
       return Err(ToolError::BadArgs {
           reason: format!("shell: command not in capability scope: {}", parsed.command),
       });
   }
   ```

5. Add integration test in `crates/tau-plugins/shell/tests/invoke.rs` (gated `#[cfg(unix)]`):
   ```rust
   #[cfg(unix)]
   #[tokio::test]
   async fn integration_deny_overrides_allow_for_shell() {
       // Allow includes "echo"; deny lists "echo" → expect rejection.
       let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
       let plugin = ShellPlugin::from_config(Default::default()).unwrap();
       let runner = tokio::spawn(async move {
           run_tool_with_io(&mut sut_reader, &mut sut_writer, plugin, "shell", "0.1.0").await
       });

       do_handshake(&mut peer).await;
       let ctx = SessionContext::new(
           AgentInstanceId::new(),
           Uuid::now_v7(),
           Some(SystemTime::UNIX_EPOCH),
       )
       .with_granted_capabilities(vec![process_spawn_cap(&["echo"])])
       .with_deny_entries(vec![DenyEntry::new("process.spawn".into(), vec!["echo".into()])]);
       send_tool_call(&mut peer, 2, &ctx, serde_json::json!({ "command": "echo", "args": ["hi"] })).await;
       let err = recv_tool_response(&mut peer)
           .await
           .expect_err("expected scope-rejection RPC error");
       assert!(
           err.contains("not in capability scope"),
           "expected scope-violation error; got: {err}"
       );
       shutdown(&mut peer).await;
       drop(peer);
       let _ = runner.await;
   }
   ```
   The `process_spawn_cap` helper mirrors the existing `fs_read_cap` helper convention (build via JSON deserialization).

6. **Verification:**
   ```bash
   cargo build --workspace
   cargo test -p shell --all-targets
   cargo test --workspace --all-targets
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```

7. **Commit message:**
   ```
   feat(shell): honor deny_entries (deny wins after allow)

   ShellSession.denied_commands populated from
   SessionContext.deny_entries at init. Command admission now does
   allow check, then deny check — deny-wins precedence per spec §9.

   Refs: docs/superpowers/specs/2026-04-30-capability-override-design.md §7.5, §9
   ```

8. Push.

---

## Task 9: `tau list agents --capabilities` flag

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` — add `ListArgs.capabilities: bool`.
- Modify: `crates/tau-cli/src/cmd/list.rs` — render the effective capability set per agent.
- Modify: `crates/tau-cli/src/output.rs` only if needed for new column rendering.
- Create: `crates/tau-cli/tests/list_agents_capabilities.rs` — assert_cmd integration test.

**Spec sections:** §8.

**Per-task summary:**

1. Add to `ListArgs`:
   ```rust
   /// When listing agents, also print the effective capability set.
   /// (Ignored when `resource` is `packages`.)
   #[arg(long)]
   pub capabilities: bool,
   ```

2. Update `clap` integration tests in `cli.rs` `#[cfg(test)] mod tests` to include parsing `tau list agents --capabilities`.

3. Compute the effective set in `list_agents`. Use the existing helper `crate::config::build_agent_definition(entry, &cwd, &scope)` (in `crates/tau-cli/src/config/agent.rs:156`) which returns `(AgentDefinition, PackageManifest)`. Flow:
   ```rust
   let cwd = std::env::current_dir()?;
   let scope = tau_pkg::Scope::resolve(&cwd)?;
   for (_id, agent_entry) in &cfg.agents {
       // Best-effort manifest lookup. If the package is not installed, the
       // row renders the override list as-given and notes "package not
       // installed" — exit 0 (read-only command).
       let effective = match crate::config::build_agent_definition(agent_entry, &cwd, &scope) {
           Ok((_def, manifest)) => Some(
               tau_runtime::capability_override::compute_effective(
                   manifest.capabilities(),
                   &agent_entry.capability_overrides,
               )
               .map_err(|e| anyhow::anyhow!("agent {:?}: {}", agent_entry.id, e))?,
           ),
           Err(_) => None, // package not installed; render shell row only
       };
       // build EffectiveCapabilityRow set + AgentRow
   }
   ```
   `compute_effective` failures exit 2 (per `tau-cli`'s exit code convention for config errors).

4. Render format (human):
   ```
   ID         DISPLAY_NAME    PACKAGE              LLM_BACKEND  EFFECTIVE_CAPABILITIES
   reviewer   Code Reviewer   code-reviewer@^0.1   anthropic    fs.read[allow=src/**;deny=secrets/**], process.spawn[allow=git,rg]
   ```
   Implementation: extend `AgentRow` with `effective_capabilities: Option<Vec<EffectiveCapabilityRow>>` (None when `--capabilities` is not set) and a small `format_effective_caps_human(&[EffectiveCapabilityRow]) -> String` helper.

5. Render format (JSON, when `--json`):
   ```json
   {
     "id": "reviewer",
     "display_name": "Code Reviewer",
     "package": "code-reviewer@^0.1",
     "llm_backend": "anthropic",
     "effective_capabilities": [
       { "kind": "fs.read", "allow_paths": ["${PROJECT}/src/**"], "deny_paths": ["${PROJECT}/secrets/**"] }
     ]
   }
   ```
   Build the JSON shape from `EffectiveCapability` via a small adapter that emits per-kind allow/deny field names matching the TOML schema.

6. **`EffectiveCapabilityRow` adapter** — defined locally in `list.rs`:
   ```rust
   #[derive(Debug, Serialize)]
   struct EffectiveCapabilityRow {
       kind: String,
       #[serde(skip_serializing_if = "Option::is_none")]
       allow_paths: Option<Vec<String>>,
       #[serde(skip_serializing_if = "Vec::is_empty", default)]
       deny_paths: Vec<String>,
       #[serde(skip_serializing_if = "Option::is_none")]
       allow_hosts: Option<Vec<String>>,
       #[serde(skip_serializing_if = "Vec::is_empty", default)]
       deny_hosts: Vec<String>,
       #[serde(skip_serializing_if = "Option::is_none")]
       allow_commands: Option<Vec<String>>,
       #[serde(skip_serializing_if = "Vec::is_empty", default)]
       deny_commands: Vec<String>,
       #[serde(skip_serializing_if = "Option::is_none")]
       max_bytes: Option<u64>,
   }
   ```
   Build from `EffectiveCapability` per kind.

7. Add tests in `crates/tau-cli/tests/list_agents_capabilities.rs` (use `assert_cmd` like other tau-cli integration tests):
   - Test 1: `tau list agents --capabilities` prints a row with the narrowed `fs.read[allow=...;deny=...]` field.
   - Test 2: `tau list agents --capabilities --json` produces a JSON array with the structured shape.
   - Test 3: `tau list agents` without `--capabilities` does NOT print the new column (verifies opt-in).
   - Test 4: When `compute_effective` rejects the override, exit code 2 and the error message names the offending kind.

8. **Verification:** standard 5-command suite.

9. **Commit message:**
   ```
   feat(cli): tau list agents --capabilities prints effective grants

   New opt-in flag computes each agent's effective capability set
   (compute_effective(package_manifest, project_override)) and renders
   it in human + JSON modes per spec §8. Empty when the agent's package
   is not installed (non-fatal); exit 2 if compute_effective rejects.

   Refs: docs/superpowers/specs/2026-04-30-capability-override-design.md §8
   ```

10. Push.

---

## Task 10: e2e integration test + final verification + open PR

**Hybrid format. User-driven gate — PAUSE before this task.**

**Files:**
- Create: `crates/tau-runtime/tests/capability_override_e2e.rs` — gated `#![cfg(unix)]`.

**Spec sections:** §10 (testing tier).

**Per-task summary:**

1. Write the e2e test that wires together the full path: project tau.toml → AgentEntry → RunOptions → Runtime → fs-read plugin → narrowed admission. Mirror the existing `tool_plugin_e2e.rs` structure.

2. Test bodies (≥ 2 tests):
   - **`narrowed_allow_denies_path_in_package_but_outside_override`** — package grants `${PROJECT}/**`; project override narrows to `${PROJECT}/src/**`; agent attempts to read `${PROJECT}/etc/foo`; expect `RunOutcome::Failed { kind: PolicyDenied | ToolBadArgs, ... }` (exact shape depends on whether the kernel or plugin emits the rejection — adjust at impl time).
   - **`deny_overrides_allow_in_e2e_run`** — package grants `${PROJECT}/**`; project override allows `${PROJECT}/**` and denies `${PROJECT}/.env`; agent attempts to read `${PROJECT}/.env`; expect rejection.
   - **`expanding_override_rejects_at_runtime`** — package grants `${PROJECT}/src/**`; project override tries to allow `${PROJECT}/etc/**`; expect `RunOutcome::Failed` with `RuntimeError::CapabilityOverrideExpands` surfaced as the failure cause.

3. Test fixture pattern: build a `RunOptions` with `project_override: vec![CapabilityOverride::new(...)]` and feed a package manifest with a known `fs.read` grant. Use the same in-process adapter (`InProcessFsRead`) introduced by Phase 1 priority 3 for the existing `tool_plugin_e2e.rs`.

4. **Run the full local verification suite:**
   ```bash
   cargo build --workspace
   cargo test --workspace --all-targets
   cargo test --workspace --doc
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```

5. **Open the PR (or mark it ready for review):**
   ```bash
   gh pr create --title "feat: capability override implementation (Tier 2 priority 4)" \
     --body "$(cat <<'EOF'
   ## Summary
   - Implements `[agents.<id>.capabilities]` overrides with intersect-only semantics (allow narrowing + deny carve-outs), realizing ADR-0007 §4.
   - Adds glob-subset analyzer + `compute_effective` in tau-runtime; `RunOptions.project_override` flows from tau-cli to the runtime.
   - Plugins (fs-read, shell) honor `SessionContext.deny_entries` after their allow check (deny wins).
   - New `tau list agents --capabilities` audit surface.

   ## Test plan
   - [ ] `cargo test --workspace --all-targets` green
   - [ ] `cargo test --workspace --doc` green
   - [ ] `cargo clippy --workspace --all-targets -- -D warnings` green
   - [ ] `cargo fmt --all -- --check` green
   - [ ] CI matrix (23 required checks) green

   🤖 Generated with [Claude Code](https://claude.com/claude-code)
   EOF
   )"
   ```
   If the PR already exists (draft), mark ready: `gh pr ready`.

6. **Commit message** (only if there are changes — the e2e test file is the new file):
   ```
   test(runtime): capability override end-to-end coverage

   Three scenarios via the in-process FsRead adapter:
   - narrow allow denies a path in package scope but outside override
   - deny carve-out denies a path admitted by allow
   - expanding override fails the run with CapabilityOverrideExpands

   Refs: docs/superpowers/specs/2026-04-30-capability-override-design.md §10
   ```

7. Push and verify CI on the PR (23 required checks).

**PAUSE — wait for the user to confirm CI is green before moving to Task 11.**

---

## Task 11: ADR amendment + ROADMAP + squash merge

**User-driven gate — PAUSE before this task.**

**Files:**
- Modify: `docs/decisions/0007-tau-cli.md` — §4 amendment.
- Modify: `ROADMAP.md` — mark Tier 2 priority 4 done with PR link.

### Steps

- [ ] **Step 11.1: Amend ADR-0007 §4**

Replace the §4 body (lines 95-110 of `docs/decisions/0007-tau-cli.md`) with:

```markdown
### 4. Capability override (intersect-only) lands in Phase 1

The `[[agents.<id>.capabilities]]` array-of-tables in project tau.toml
narrows — but never expands — the capabilities granted by an agent's
package manifest. Each entry's `kind` discriminator must match a
package-side capability; `allow_*` fields must be a glob-subset (or
exact-match for hosts/commands) of the package's grant; `deny_*`
fields are pure subtractions with deny-wins precedence. Validation
runs at parse time AND at every runtime load, both fail-closed.

Realized by the capability-override sub-project (Tier 2 priority 4):
see `docs/superpowers/specs/2026-04-30-capability-override-design.md`
for the full design.

Trigger to revisit: per-tool overrides (narrower than per-agent),
hostname-glob narrowing, or `Capability::Custom` parameter narrowing
— each deferred to a future sub-project.
```

- [ ] **Step 11.2: Update ROADMAP**

In `ROADMAP.md`, find the Tier 2 priority 4 entry (around line 98):

```markdown
4. **Capability override implementation** (project tau.toml
   `[agents.<id>.capabilities]` with intersect-only semantics, per
   ADR-0007 §4 reservation).
```

Replace with:

```markdown
4. **Capability override implementation** ✅ Shipped 2026-04-30 — see
   spec
   `docs/superpowers/specs/2026-04-30-capability-override-design.md`.
   Realizes ADR-0007 §4 reservation. Project tau.toml
   `[[agents.<id>.capabilities]]` narrows package grants via
   semantic glob-subset on allow + deny carve-outs (deny wins).
   Audit surface: `tau list agents --capabilities`.
```

Also update the Tier 1/Tier 2 summary table at the top of ROADMAP if it tracks per-priority status.

- [ ] **Step 11.3: Commit the docs**

```bash
git add docs/decisions/0007-tau-cli.md ROADMAP.md
git commit -m "docs: ADR-0007 §4 amendment + ROADMAP Tier 2 priority 4 done

Drops the 'reserved' qualifier on ADR-0007 §4 now that the
capability override implementation has shipped. Links to the
realizing spec.

Refs: docs/superpowers/specs/2026-04-30-capability-override-design.md"
```

- [ ] **Step 11.4: Push final commit**

```bash
git push
```

- [ ] **Step 11.5: Wait for CI green on the PR (23 required checks).**

- [ ] **Step 11.6: Squash merge**

```bash
gh pr merge --squash --delete-branch
```

- [ ] **Step 11.7: Verify branch protection unchanged**

Branch protection on `main` requires 23 checks. No new CI jobs were added in this sub-project, so the count stays at 23. Verify via:
```bash
gh api repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks/contexts | jq 'length'
```
Expected: `23`.

- [ ] **Step 11.8: Report back to the user with the squash-commit SHA on `main`.**

---

## Verification standard (applied per task)

Each task ends with:

```bash
cargo build --workspace
cargo test -p <crate> --all-targets
cargo test -p <crate> --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

For tasks touching multiple crates (5, 9, 10), run `cargo test --workspace --all-targets` instead.

CI continues on push; no new jobs added; branch protection stays at 23.
