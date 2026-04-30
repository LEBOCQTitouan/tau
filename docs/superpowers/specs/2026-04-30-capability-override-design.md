# Capability Override Implementation — Design Spec

**Date:** 2026-04-30
**Status:** Approved (pending user review of this written spec)
**Sub-project:** Tier 2 priority 4 (per ROADMAP `Tier 2 — completes Phase 0 deferrals`).
**Closes deferral:** ADR-0007 §4 — `[agents.<id>.capabilities]` reservation.

---

## 1. Summary

Implement the `[agents.<id>.capabilities]` table in project `tau.toml`. The
table lets a project narrow — but never expand — the capability grants its
agents inherit from their package manifest. Two narrowing levers are exposed:

1. **`allow_paths` / `allow_hosts` / `allow_commands`** — replaces the package
   grant's set with a strict subset of it (semantic glob/string subset).
2. **`deny_paths` / `deny_hosts` / `deny_commands`** — explicit subtractions
   carved out of whatever allow-list is effective. Always honored at runtime.

The result is an *effective grant set* per agent that is provably ⊆ the package
manifest's grant set — enforced by failing the project parse closed if any
`allow_*` field is not a subset of the corresponding package field.

This sub-project is wholly in-tree: changes to `tau-cli`, `tau-pkg`,
`tau-runtime`, and a small new utility module for glob-subset analysis. No
new workspace member.

---

## 2. Background and motivation

ADR-0007 §4 reserved the `[agents.<id>.capabilities]` schema slot at v0.1.
Today the field is parsed and *rejected* with
`ProjectConfigError::CapabilityOverrideUnsupported`
(`crates/tau-cli/src/config/project.rs:236`), pointing at the Phase 1+
roadmap. The reservation locked in "intersect-only" semantics:

> A Phase 1 override can narrow but never expand the capabilities granted by
> the package manifest. — ADR-0007 §4

This spec realizes that reservation. The motivation hasn't changed: a project
should be able to harden a packaged agent (e.g., narrow `${PROJECT}/**` to
`${PROJECT}/src/**` for a code-review agent that has no business reading
`.env`) without forking the package.

## 3. Decisions table

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| Q1 | TOML shape of override | Array-of-tables matching package manifest, with `allow_*` and `deny_*` fields | Mirrors package format; `allow`/`deny` split makes intent explicit; precedent: Deno 2.5, Tauri v2, AWS IAM |
| Q2 | Subset semantics for allow-list narrowing | **Semantic glob-subset** (a glob is a subset iff every path it matches is also matched by the parent glob) | Lets projects write `${PROJECT}/src/**` to narrow `${PROJECT}/**` without exact-match gymnastics; standard precedent (Cargo features, Bazel visibility) |
| Q3 | Validation timing | At parse (project `tau.toml` load) AND at runtime tool dispatch | Cheap re-check; package manifest may change between sessions if upgraded; fail-closed both places |
| Q4 | Error type | New typed `ProjectConfigError::CapabilityOverrideExpands` (parse-time) and reuse existing `RuntimeError`/`CapabilityDenial` flow at runtime | Per ADR-0009 typed-error policy; new variant added to existing `#[non_exhaustive]` enum is non-breaking |
| Q5 | Audit surface | New `tau list agents --capabilities` flag prints effective allow + deny per agent | Operators must be able to inspect the effective grant; CLI is the only audit channel today |
| Q6 | Distribution | In-tree changes; no new workspace member | YAGNI — glob-subset module lives next to the validator that uses it |
| Q7 | ADR | Amend ADR-0007 §4 in place: drop "reserved" qualifier, link to this spec | The semantic was already locked in ADR-0007; this is realization, not re-decision |

---

## 4. TOML schema

The override table is an array-of-tables that *parallels* the package
manifest's `capabilities` array. Each entry's `kind` discriminator must
match an entry in the package manifest; the `allow_*` and `deny_*` fields
narrow the matching package entry.

### 4.1 Filesystem capability override

```toml
[[agents.reviewer.capabilities]]
kind        = "fs.read"
allow_paths = ["${PROJECT}/src/**", "${PROJECT}/docs/**"]   # optional; subset of package's `paths`
deny_paths  = ["${PROJECT}/.env", "${PROJECT}/secrets/**"]  # optional; any string

[[agents.reviewer.capabilities]]
kind        = "fs.write"
allow_paths = ["${PROJECT}/build/**"]
# `max_bytes` is a tightening: if the package declares max_bytes = 5_000_000,
# the override may set any value ≤ 5_000_000. If the package leaves it unset
# (= unlimited), any non-negative value is a tightening. Raising or removing
# the cap is rejected at parse.
max_bytes   = 1048576
```

### 4.2 Network capability override

```toml
[[agents.reviewer.capabilities]]
kind         = "net.http"
allow_hosts  = ["api.github.com"]      # subset of package's `hosts`
allow_methods = ["GET"]                # subset of package's `methods`
deny_hosts   = []                      # no carve-outs needed when allow-list is already tight
```

### 4.3 Process capability override

```toml
[[agents.reviewer.capabilities]]
kind            = "process.spawn"
allow_commands  = ["git", "rg"]        # subset of package's `commands`
deny_commands   = []
```

### 4.4 Field semantics

- **`kind`** — required; must exactly match a `kind` in the package manifest.
  An override entry whose `kind` is absent from the package is a parse-time
  error (`CapabilityOverrideExpands`), not a silent no-op.
- **`allow_*` (optional)** — when present, must be a subset of the package's
  corresponding field per the subset rule below. When absent, the package's
  full `paths`/`hosts`/`commands` is used as the effective allow-list.
- **`deny_*` (optional)** — pure subtraction. No subset check. Strings are
  validated for shape (non-empty, no NUL, glob-parsable for path fields) but
  may reference paths/hosts/commands not present in the package — denying
  things that were never granted is a no-op, not an error.
- **`max_bytes` on `fs.write` (optional)** — if declared, must be ≤ the
  package's `max_bytes` (or the package's was unset, in which case any value
  is accepted as a tightening). Raising or removing the cap is rejected.

### 4.5 Custom capabilities

`Capability::Custom { name, params }` is intentionally **not narrowable** at
v0.1. Override entries with a `kind` that resolves to `Capability::Custom` are
rejected with `CapabilityOverrideExpands { reason: "custom capabilities are
not narrowable" }`. This mirrors ADR-0009's "escape-hatch capabilities are
plugin-defined; the kernel cannot reason about their parameters." Phase 2+
can revisit if users need to narrow `params`.

---

## 5. Subset semantics (Q2 — locked: B = semantic glob-subset)

A glob `child` is a subset of glob `parent` iff every concrete path matched
by `child` is also matched by `parent`. Implementation strategy:

1. **Quick literal check** — if `child == parent`, return true.
2. **Prefix expansion** — strip a trailing `**` from `parent`, get
   `parent_prefix`. If `child` starts with `parent_prefix` (and the next
   character is the path separator or end-of-string), return true.
3. **Brace expansion** — `globset::Glob` does not natively decompose
   alternations; handle `{a,b}` by enumerating arms and recursing on each.
4. **Sample-based fallback for adversarial cases** — when the structural rules
   above are inconclusive, generate a small bounded set of test paths that
   match `child` and assert each matches `parent`. The rule-of-thumb implementation:
   - Only triggers for patterns that contain `?` or character classes.
   - Generates ≤ 64 samples per pattern.
   - If sample generation overflows, fail closed (treat as not-subset).

5. **Hosts and commands** — string equality semantics: `child_hosts ⊆
   package_hosts` (set inclusion); same for commands. This sidesteps any
   wildcard-in-host complications. (If we later add hostname-glob support, we
   recurse via the glob-subset analyzer.)

The structural rules (1) and (2) handle the overwhelming majority of expected
real-world overrides:
- `${PROJECT}/src/**` ⊆ `${PROJECT}/**` ✓ (prefix expansion)
- `${PROJECT}/src/**` ⊆ `${PROJECT}/src/**` ✓ (literal)
- `${PROJECT}/src/main.rs` ⊆ `${PROJECT}/**` ✓ (prefix expansion + the literal is matched)
- `${PROJECT}/etc/**` ⊆ `${PROJECT}/src/**` ✗ (rejected at parse)

**Edge cases explicitly handled:**

| Case | Behavior |
|------|----------|
| Empty `allow_paths = []` | Effective allow = ∅; tool always denied for that capability. Useful for "disable this capability" without removing the package's grant. |
| `allow_paths` field absent | Effective allow = package's `paths`. |
| `deny_paths` field absent | No deny carve-outs. |
| `deny_paths = ["${PROJECT}/foo"]` where `foo` is not in package's allow | Accepted; deny is a no-op for paths never granted. Documented as accepted (not rejected). |
| Negated globs (`!pattern`) | **Rejected at parse.** Negation belongs in `deny_*`. |
| Two override entries with same `kind` | **Rejected at parse** (`CapabilityOverrideExpands { reason: "duplicate kind" }`). |

`${PROJECT}` and other variables are expanded *before* subset analysis, so
the analyzer compares post-expansion strings only.

---

## 6. Validation pipeline (Q3 — locked: parse + runtime)

### 6.1 Parse-time validation (`tau-cli`)

`UncheckedAgent.capabilities` becomes `Vec<UncheckedCapability>` (replacing
the `Option<toml::Value>` placeholder). After agent-level validation, call:

```rust
validate_capability_override(
    package_manifest_caps: &[Capability],
    project_override: Vec<UncheckedCapability>,
) -> Result<Vec<EffectiveCapability>, ProjectConfigError>
```

Failures produce `ProjectConfigError::CapabilityOverrideExpands { id, kind,
reason }`. Specifically rejected at parse:

- Override entry whose `kind` has no matching package entry.
- Override entry whose `allow_*` is not a subset of the package's field.
- Override entry whose `max_bytes` is greater than the package's `max_bytes`,
  or whose `max_bytes` is set when the package's was set tighter.
- Override entry on a `Capability::Custom`.
- Two override entries with the same `kind`.
- Negated globs in any allow/deny field.

### 6.2 Runtime re-check (`tau-runtime`)

`run.rs:120` reads `package_manifest.capabilities()` for the granted set.
After this sub-project, the dispatch path takes both the package manifest
*and* the project override, computes the effective grant, and uses *that* as
the `granted` slice for both the kernel's structural check (run.rs:272) and
the SessionContext.granted_capabilities passed to plugins (run.rs:336).

The runtime re-runs the parse-time subset check on the override before
applying it. This is the fail-closed safeguard against:
- The package being upgraded between project parse and `tau run` (rare but
  possible if an operator runs `tau install` and `tau run` in sequence).
- Mistaken hand-edits of cached project state.

If the runtime re-check fails, dispatch returns
`RuntimeError::CapabilityOverrideExpands` (new typed variant) and the run
fails closed. The error wraps the same `{ kind, reason }` payload as the
parse-time variant for telemetry consistency.

### 6.3 Effective set construction

`Capability` and its inner enums are `#[non_exhaustive]` — variant fields
can't be constructed cross-crate. Rather than add constructor APIs to
`tau-domain`, the override layer keeps the package's `Capability` as-is and
side-loads the narrowed allow-list and deny-list:

```rust
// tau-runtime::capability_override
pub struct EffectiveCapability {
    /// The package-side capability as-given. Field values (e.g. paths)
    /// inside this struct are NOT narrowed — they remain the package's grant.
    pub source: Capability,
    /// Narrowed allow-list to apply at the plugin layer. Same shape as the
    /// strings inside `source` (paths/hosts/commands). When `None`, the
    /// plugin should use `source`'s own field.
    pub allow_override: Option<Vec<String>>,
    /// Deny-list to subtract. Empty = no carve-outs.
    pub deny: Vec<String>,
    /// Narrowed `max_bytes` for `fs.write`; `None` = use source's value.
    pub max_bytes_override: Option<u64>,
}
```

Construction algorithm:

```text
For each package capability cap_pkg:
    if no matching override entry:
        EffectiveCapability { source: cap_pkg, allow_override: None,
                              deny: [], max_bytes_override: None }
    else:
        allow_override = override.allow_paths      if present else None
        deny           = override.deny_paths       if present else []
        max_bytes_override = override.max_bytes    if present else None
        EffectiveCapability { source: cap_pkg, allow_override, deny, max_bytes_override }
```

The `granted_capabilities` field on `SessionContext` carries the post-narrow
view that the plugin sees: `EffectiveCapability` is flattened to a
`Capability` whose `paths`/`hosts`/`commands` are replaced by the narrowed
allow-list (when present) — but this flattening happens *via the plugin's
own type* (i.e., the plugin reads `allow_override` directly and constructs
its own session state from it). The kernel's structural check at
`run.rs:272` continues to use the package's grants verbatim, since narrowing
the allow-list doesn't change capability *kinds* (`fs.read` is still
`fs.read`); structural admission is unaffected.

`deny` is enforced at the plugin's `admit()` step *after* the allow check
passes — i.e., a path matching deny is rejected even if allow would have
admitted it. This realizes "deny wins" precedence (AWS IAM convention).

---

## 7. Type changes

### 7.1 `tau-cli::config::project`

Replace:
```rust
pub capabilities: Option<toml::Value>,
```
with a typed shape:
```rust
#[serde(default)]
pub capabilities: Vec<UncheckedCapabilityOverride>,
```

Where `UncheckedCapabilityOverride` deserializes via the same flat-`kind`
discriminator pattern used by `Capability` (`crates/tau-domain/src/package/
capability.rs:155-203`), but with `allow_*` and `deny_*` field names. After
validation, an `AgentEntry` gains a new field
`pub effective_capabilities: Vec<EffectiveCapability>`.

`ProjectConfigError` gains:
```rust
#[error("agent {id:?}: capability override on {kind:?} expands the package's grant: {reason}")]
CapabilityOverrideExpands {
    id: String,
    kind: String,
    reason: String,
},
```

Removed: `CapabilityOverrideUnsupported` (existing parse-time rejection
becomes a no-op since the field is now supported).

### 7.2 `tau-runtime::error`

`RuntimeError` (already `#[non_exhaustive]`) gains:
```rust
#[error("capability override on {kind:?} expands package grant: {reason}")]
CapabilityOverrideExpands {
    kind: String,
    reason: String,
},
```

Telemetry event: `runtime.capability_override_rejected` with fields
`{ agent_id, package_id, kind, reason }`.

### 7.3 New module: `tau-runtime::capability_override`

Public API:
```rust
pub fn compute_effective(
    package_caps: &[Capability],
    project_override: &[CapabilityOverride],
) -> Result<Vec<EffectiveCapability>, OverrideExpandError>;

pub struct CapabilityOverride {
    /// Mirror of `UncheckedCapabilityOverride` post-validation; constructed
    /// by tau-cli at parse time and passed through to the runtime via
    /// `RunOptions.project_override`.
    pub kind: String,
    pub allow: Option<Vec<String>>,
    pub deny: Vec<String>,
    pub max_bytes: Option<u64>,
}

// Shape per §6.3.
pub struct EffectiveCapability {
    pub source: Capability,
    pub allow_override: Option<Vec<String>>,
    pub deny: Vec<String>,
    pub max_bytes_override: Option<u64>,
}

pub struct OverrideExpandError { pub kind: String, pub reason: String }
```

Submodule `glob_subset` exposes:
```rust
pub fn is_glob_subset(child: &str, parent: &str) -> bool;
pub fn is_glob_subset_set(children: &[String], parents: &[String]) -> Result<(), String>;
```

The `*_set` form returns the offending child glob in `Err` for diagnostics.

### 7.4 `tau-runtime::run.rs:120-336`

- Line 113: `run_with_history` already takes `package_manifest: PackageManifest`.
  Add the override via `RunOptions` (preferred — the field is opt-in, defaults
  to empty `Vec`, and avoids growing the function signature):
  ```rust
  pub struct RunOptions {
      pub max_turns: u32,
      pub project_override: Vec<CapabilityOverride>,  // additive, default empty
  }
  ```
  The CLI populates this from the validated `AgentEntry`. The runtime never
  reads project files itself — that boundary stays in tau-cli.
- Line 120: replace `let granted: &[Capability] = package_manifest.capabilities();`
  with `let effective = compute_effective(package_manifest.capabilities(),
  project_override)?;` and use `&effective` for both the filter (line 161)
  and dispatch check (line 279).
- Line 336: `SessionContext` carries the effective set. Plugins still receive
  a `Vec<Capability>` via `granted_capabilities`, but each entry's `paths`
  field is the narrowed allow-list. Deny semantics are enforced **plugin-side**
  via a new field on `SessionContext`:

```rust
// tau-ports/src/tool.rs SessionContext (#[non_exhaustive] — additive non-breaking)
pub deny_entries: Vec<DenyEntry>,

// New type, also #[non_exhaustive], in tau-ports:
#[non_exhaustive]
pub struct DenyEntry {
    /// Capability kind discriminator: `"fs.read"`, `"fs.write"`, `"fs.exec"`,
    /// `"net.http"`, `"process.spawn"`. Matches the wire `kind` field.
    pub kind: String,
    /// Strings to subtract from the matching allow-list. For path-shaped
    /// capabilities these are globs; for `net.http` host names; for
    /// `process.spawn` command names.
    pub deny: Vec<String>,
}
```

Plugins (`fs-read`, `shell`) consult `deny_entries` in their `admit()`/`run()`
paths after the allow check passes — they pick the entry whose `kind` matches
the capability they enforce.

### 7.5 Plugin changes

- **`fs-read`**: `FsReadSession.allowed_globs` already exists. Add
  `denied_globs: Vec<String>` populated from the `ctx.deny_entries` entry
  whose `kind == "fs.read"`. `path_check::admit` becomes
  `admit_with_deny(path, allow, deny)` returning the same bool.
- **`shell`**: `ShellSession.allowed_commands` already exists (or equivalent).
  Add `denied_commands: Vec<String>` populated similarly. `command_check::admit`
  returns false for commands present in `denied_commands`.
- **`echo-tool`**: no change (no capabilities declared).

---

## 8. CLI surface (Q5 — locked: extend `tau list agents`)

Add a `--capabilities` flag to `tau list agents`:

```bash
$ tau list agents --capabilities
ID         DISPLAY_NAME    PACKAGE              LLM_BACKEND  EFFECTIVE_CAPABILITIES
reviewer   Code Reviewer   code-reviewer@^0.1   anthropic    fs.read[allow=src/**;deny=secrets/**], process.spawn[allow=git,rg]
linter     Linter          eslint-bot@^0.2      ollama       fs.read[allow=**], net.http[allow=hosts=api.github.com,methods=GET]
```

The `--json` mode emits:
```json
[
  {
    "id": "reviewer",
    "display_name": "Code Reviewer",
    "package": "code-reviewer@^0.1",
    "llm_backend": "anthropic",
    "effective_capabilities": [
      { "kind": "fs.read", "allow_paths": ["${PROJECT}/src/**"], "deny_paths": ["${PROJECT}/secrets/**"] },
      { "kind": "process.spawn", "allow_commands": ["git", "rg"] }
    ]
  }
]
```

`tau list agents` without `--capabilities` is unchanged. The flag is opt-in
because the column is wide and noisy for the common `tau list agents` case.

---

## 9. Best-possible-security model (locked)

The design realizes the following defensive properties (synthesized from
AWS IAM, AppLocker, and firewall best practices):

1. **Default-deny baseline.** Without an override, the agent inherits the
   package's grants verbatim — no broadening path exists.
2. **Allow-list as primary lever.** `allow_paths` (the narrowing field) is
   strictly checked: any glob outside the package's grant fails parse.
3. **Deny-list for surgical carve-outs.** `deny_paths` doesn't grant; it
   subtracts. It's the right tool for "package allows `${PROJECT}/**`, deny
   `${PROJECT}/.env`" — the package can't anticipate every project's secrets
   layout.
4. **Deny wins precedence.** A path matching both allow and deny is denied.
   Mirrors AWS IAM "explicit deny trumps allow."
5. **Fail-closed validation.** Both at parse and at every runtime load —
   any error rejects the run, not just the override. Rationale: if the
   override doesn't apply cleanly, falling back to the package's broader
   grant would silently widen the agent's authority.
6. **No expansion path possible.** The only way to "raise" an override is to
   delete the agent's `[agents.<id>.capabilities]` block (which then
   inherits the package). There's no syntax for "more than the package."
7. **Auditable.** `tau list agents --capabilities` lets operators inspect
   the effective grant before running. The runtime also logs
   `runtime.capability_set_loaded` (already present, line 121) which we
   amend to include override-applied counts.

---

## 10. Testing

| Tier | Scope | Where |
|------|-------|-------|
| Unit | `glob_subset::is_glob_subset` — literal, prefix-expansion, brace expansion, sample fallback, negative cases | `crates/tau-runtime/src/capability_override/glob_subset.rs` (`#[cfg(test)] mod tests`) |
| Unit | `compute_effective` — well-formed override, expand-rejected, kind-mismatch, custom-capability rejected, duplicate-kind, max_bytes raise rejected, deny-no-op | `crates/tau-runtime/src/capability_override/mod.rs` |
| Unit | `validate_capability_override` (parse-time wrapper) — same matrix, exercised through TOML deserialization | `crates/tau-cli/src/config/project.rs::tests` |
| Integration | tau-cli end-to-end: tau.toml with override loads cleanly; expanding override fails with `CapabilityOverrideExpands` and exit code 2 | `crates/tau-cli/tests/list_agents_capabilities.rs` (new) |
| Integration | tau-runtime end-to-end: agent with narrowed `fs.read` correctly denies a path that's in package scope but outside override allow | `crates/tau-runtime/tests/capability_override_e2e.rs` (new) |
| Integration | Same e2e: a path matching both allow and `deny_paths` is denied | same file |
| Integration | `tau list agents --capabilities` outputs the effective set in both human and JSON modes | `crates/tau-cli/tests/list_agents_capabilities.rs` |

The runtime e2e test mirrors `crates/tau-runtime/tests/tool_plugin_e2e.rs`
(gated `#![cfg(unix)]` for tempfile path stability).

---

## 11. Implementation plan outline (~10–11 tasks)

The plan derived from this spec will have these tasks. Final wording lives
in the implementation plan.

1. **`tau-domain` (or `tau-runtime`) glob-subset module** — `is_glob_subset`
   + tests. Module lives in tau-runtime since only it consumes the function;
   moves to tau-domain only if a second consumer appears.
2. **`tau-runtime::capability_override` module** — `compute_effective`,
   `EffectiveCapability`, `OverrideExpandError`, full unit tests.
3. **`tau-cli::config::project` schema upgrade** — replace
   `Option<toml::Value>` with typed `Vec<UncheckedCapabilityOverride>`,
   wire validation, replace `CapabilityOverrideUnsupported` with
   `CapabilityOverrideExpands`. Update existing tests.
4. **`tau-ports::SessionContext` deny_entries additive field** — additive
   non-breaking via `#[non_exhaustive]`; introduce `DenyEntry` (also
   `#[non_exhaustive]`) and a `with_deny_entries` builder.
5. **`tau-runtime::run.rs` integration** — accept `RunOptions.project_override`,
   call `compute_effective`, populate SessionContext.deny_entries, re-check
   at runtime.
6. **`tau-runtime::error::RuntimeError::CapabilityOverrideExpands`** — new
   typed variant; runtime logs telemetry.
7. **`fs-read` plugin deny enforcement** — `FsReadSession.denied_globs`,
   `path_check::admit_with_deny`, integration test.
8. **`shell` plugin deny enforcement** — `ShellSession.denied_commands`,
   `command_check::admit` deny path, integration test.
9. **`tau list agents --capabilities` flag** — extend `ListArgs`, render
   row, JSON shape, tests.
10. **e2e integration test** at `tau-runtime` and `tau-cli` levels.
11. **ADR-0007 §4 amendment** — drop "reserved", link to this spec; update
    ROADMAP Tier 2 priority 4 entry.

Each task is a single Conventional Commits commit, following the established
sub-project pattern.

CI: this sub-project does not add new CI jobs (no new workspace member, no
new external service). Branch protection count stays at 23.

---

## 12. Out of scope

- **Phase 2+: per-tool overrides.** Today's override is per-agent. A future
  iteration may want `[agents.<id>.tools.<name>.capabilities]` to narrow
  one tool's grants tighter than the agent's overall grant. Deferred.
- **Phase 2+: `Capability::Custom` narrowing.** Custom capabilities have
  plugin-defined `params`; the kernel can't reason about subset. If users
  ask, plugin authors will add per-plugin override schemas at that point.
- **Hostname glob narrowing.** `net.http` allow_hosts is exact-match strings
  at v0.1. Hostname globs (`*.example.com`) are deferred to whichever
  network plugin first needs them.
- **Capability *expansion* via project tau.toml.** Not in scope ever —
  ADR-0007 §4 forbids it. The only way to grant more is to repackage the
  agent or install a different package.
- **Programmatic override APIs.** No `Runtime::with_capability_override(...)`
  builder method — the override is a project-config concern, not a runtime
  API concern.

---

## 13. Cross-references

- ADR-0007 §4 — original reservation; this spec realizes it.
- ADR-0008 §5 — plugin loading; SessionContext is the channel for grants.
- ADR-0009 — typed-error policy; new variants follow this.
- ROADMAP Tier 2 priority 4 — this is the priority being closed.
- `crates/tau-cli/src/config/project.rs:175-184` — current
  `CapabilityOverrideUnsupported` rejection, removed by this work.
- `crates/tau-runtime/src/run.rs:120` — granted-set load site, gains
  `compute_effective` call.
- `crates/tau-runtime/src/run.rs:336` — SessionContext construction site,
  gains deny_globs.
- `crates/tau-domain/src/package/capability.rs` — the package-side schema
  that override entries narrow.
