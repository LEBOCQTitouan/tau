# Transitive Dependency Resolution — Design Spec

**Date:** 2026-04-30
**Status:** Approved (pending user review of this written spec)
**Sub-project:** Tier 2 priority 5 (per ROADMAP `Tier 2 — completes Phase 0 deferrals`).
**Closes deferral:** ADR-0007 §5 — `requires.tools` advisory-only commitment.

---

## 1. Summary

Implement the auto-install path for `[agents.<id>.requires.tools]`. When
`tau run` / `tau chat` (or the new `tau resolve` subcommand) sees an agent
declaring required tools, missing tool packages are fetched + installed
automatically instead of erroring out. Bare-string entries (today's
`tools = ["fs-read"]`) are no longer supported; each entry must declare
its source, mirroring Cargo's `[dependencies]` shape.

Auto-install is **one level deep at v0.1**: the agent's `requires.tools`
gets resolved, but the resolved tool packages' own `dependencies` field
stays advisory (ADR-0004 §10's deferral remains). Recursive package-level
resolution is a future sub-project.

This sub-project is wholly in-tree: changes to `tau-pkg`, `tau-cli`, and
an ADR-0007 §5 amendment. No new workspace member.

---

## 2. Background and motivation

ADR-0007 §5 reserved the auto-install path at v0.1:

> `tau run` and `tau chat` check each entry against the local registry;
> any missing entry yields a clear error and exit code 2. At v0.1 this
> is advisory only: tau-cli does NOT auto-install missing dependencies.
> Phase 1+ activates auto-install via tau-pkg's transitive-resolution
> work. Trigger to revisit: Phase 1's transitive-resolution work, at
> which point this hook becomes the auto-install entry point.

Today's user experience: declare required tools in tau.toml, then *manually*
`tau install <git-url>` each one before running an agent. Onboarding friction
for any project with more than one tool dependency.

The existing schema's gap: `requires.tools = Vec<String>` carries names
only, no source. Without source info, auto-install has no fetch target.
This sub-project closes both gaps in a single pass — extends the schema
to carry source + version constraints, then implements resolution +
fetch.

## 3. Decisions table

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| Q1 | Scope of auto-install | **A+B**: extended schema (source + optional version) is foundational; auto-install is the default; `--no-install` falls back to copy-pasteable error hints | One level deep matches ADR-0007 §5's exact deferred semantic; recursive stays deferred under ADR-0004 §10 |
| Q2 | When auto-install fires | **C**: lazy at `tau run`/`chat` AND a new explicit `tau resolve` subcommand | Lazy fits the existing flow; explicit `tau resolve` serves CI cache warm-up + pre-flight validation |
| Q3 | Schema change | Reject bare strings; require struct form on every entry | Schema churn is contained (3 test fixtures, no production tau.toml configs ship with bare strings); cleaner long-term schema |
| Q4 | Source field type | **A**: typed nested `source = { kind = "git", url = "...", rev = "..." }` reusing existing `tau_domain::PackageSource` | Reuses existing typed enum; explicit; easy to evolve |
| Q5 | UX confirmation | npm-style progress output (one line per phase) over the existing `Output` channel; respects `--quiet` and `--json` | Familiar idiom; non-silent; reuses existing CLI output plumbing |
| Q6 | Lockfile | Existing per-scope lockfile records auto-installed tool deps (same shape as `tau install`) | No new file; reuse existing `tau_pkg::registry` |
| Q7 | Error type | New typed `tau_pkg::ResolveError`; new `ProjectConfigError::RequiresToolsBareStringRejected` for the parse-time rejection | Per ADR-0009 typed-error policy |
| Q8 | Distribution | In-tree changes to tau-pkg + tau-cli; ADR-0007 §5 amendment | YAGNI — only one consumer (tau-cli) at v0.1 |
| Q9 | Version conflict resolution | **C**: Cargo-style semver intersection per `(name, source)` group | Schema gains optional `version: VersionReq` field per entry (defaults to `"*"`); intersection picks highest-compatible version |

---

## 4. Schema

### 4.1 Project tau.toml

`[agents.<id>.requires.tools]` becomes an array-of-tables (or inline array
of struct tables — both deserialize identically):

```toml
[[agents.reviewer.requires.tools]]
name    = "fs-read"
source  = { kind = "git", url = "https://example.com/fs-read.git" }
version = "^0.1"   # optional; defaults to "*" (any version)

[[agents.reviewer.requires.tools]]
name    = "shell"
source  = { kind = "git", url = "https://example.com/shell.git", rev = "main" }
# version absent → "*"
```

Inline-array form (equivalent):

```toml
[agents.reviewer.requires]
tools = [
  { name = "fs-read", source = { kind = "git", url = "https://example.com/fs-read.git" }, version = "^0.1" },
  { name = "shell",   source = { kind = "git", url = "https://example.com/shell.git", rev = "main" } },
]
```

### 4.2 Field semantics

- **`name`** — required; package name (validated against `PackageName` rules).
- **`source`** — required; typed `PackageSource` (variants `Git { url, rev: Option<String> }`, `Path { path }`).
  Reused verbatim from `tau-domain`.
- **`version`** — optional string; parsed as `semver::VersionReq`. Defaults to
  `"*"` (any version) when absent.
- **Bare strings rejected at parse.** `tools = ["fs-read"]` produces
  `ProjectConfigError::RequiresToolsBareStringRejected { agent_id, index, value }`
  with a clear migration hint pointing at this spec.

### 4.3 Backward-compatibility

No production tau.toml files ship with `requires.tools` populated. The
break is contained to:
- `crates/tau-cli/src/config/project.rs:416` — one inline test fixture.
- `crates/tau-cli/src/config/agent.rs:633` — one inline test fixture.
- `crates/tau-cli/tests/common/mod.rs:408` — test scaffold helper.

The `tau init` template at `crates/tau-cli/src/cmd/init.rs:8-20` does
NOT scaffold a `requires` block, so users running `tau init` post-this-
sub-project never see the legacy syntax.

---

## 5. Resolution algorithm (Q9 — semver intersection)

Resolution is **one level deep**: `requires.tools` only. Recursive
package-level `dependencies` resolution stays deferred (ADR-0004 §10).

### 5.1 Inputs

A `Vec<(AgentId, RequiredTool)>` flattened from the project tau.toml,
plus a `tau_pkg::Scope` (project or global) for lockfile lookup.

### 5.2 Phase 1 — group by `name`

Build `BTreeMap<PackageName, Vec<&RequiredTool>>`. Same-name entries
across multiple agents get unified into a single resolution group.

### 5.3 Phase 2 — same-name conflict checks

For each group:

1. **Source equality.** All entries must declare equal `PackageSource`
   (typed `PartialEq`). Any difference — different `Git.url`, different
   `Git.rev`, mixing `Git` and `Path`, or different `Path.path` — is a
   conflict that rejects with
   `ResolveError::ConflictingSources { name, sources, agents }`.
   Matches Cargo's "no two distinct sources for the same crate" principle.
   If two agents legitimately need the same tool from the same git URL,
   they must declare the *exact* same `source` value.

2. **Version-constraint intersection.** Collect all `version_req`
   values from the group. Validate that the intersection is non-empty
   by attempting Phase-3 resolution; if Phase 3 returns
   `NoCompatibleVersion`, that's the empty-intersection case. (semver
   crate doesn't intersect natively; the empty-intersection check
   falls out of the per-version satisfaction test in Phase 3.)

### 5.4 Phase 3 — pick a concrete version per name

For each `(name, source, [version_reqs...])` triple:

1. **Lockfile reuse.** Query the per-scope lockfile via
   `tau_pkg::registry::list(scope)`. If an installed version of `name`
   from the same `source` satisfies *all* `version_reqs`, **reuse**
   (no fetch).
2. **List available versions.** Otherwise, call
   `list_versions_at_source(source) -> Vec<Version>`:
   - For `PackageSource::Git { url, rev: None }`: shell out to
     `git ls-remote --tags <url>`, parse the tag list, filter to those
     parsing as `semver::Version` (after stripping a leading `v` if
     present), drop non-semver tags. The available set is the resulting
     `Vec<Version>`.
   - For `PackageSource::Git { url, rev: Some(_) }`: only one version
     is available — the one declared in the manifest at that exact
     rev. We clone the rev shallowly, read its `tau.toml`, and return
     `vec![manifest.version()]`. `version_req` must accept that single
     version, otherwise → `NoCompatibleVersion` with `available` = that
     one entry.
   - For `PackageSource::Path { path }`: read the manifest at `path`,
     return `vec![manifest.version()]`. Same single-point semantics.
3. **Pick highest-compatible.** Among `available`, find the highest
   `Version` satisfying all `version_reqs`. If none → `ResolveError::NoCompatibleVersion`.
4. **Fall through to install.** Add `(name, picked_version, source,
   requested_by)` to `ResolutionPlan.installs`.

### 5.5 Output

```rust
pub struct ResolutionPlan {
    pub installs: Vec<PlannedInstall>,
    pub reuses:   Vec<ReusedInstall>,
}

pub struct PlannedInstall {
    pub name: PackageName,
    pub version: Version,
    pub source: PackageSource,
    pub requested_by: Vec<AgentId>,  // agents that listed this tool
}

pub struct ReusedInstall {
    pub name: PackageName,
    pub version: Version,
}
```

`PlannedInstall.requested_by` is plumbed through to error messages so
users can trace "why is this tool being installed?" back to a specific
agent declaration.

---

## 6. Install execution

After Phase 3 produces a plan, the installer iterates `plan.installs`
sequentially. The resolver does **not** acquire the per-scope advisory
file lock itself; each `tau_pkg::install_with_options(...)` call
acquires + releases the lock individually per ADR-0004 §11. This avoids
double-acquisition (the existing `install_with_options` flow is the
sole owner of the lock) and stays compatible with concurrent
unrelated `tau install` invocations from other shells (one will block
on the lock per call, not for the whole resolve).

Errors from any per-package install propagate as
`ResolveError::Install { name, source, install_err }` and abort the
remaining plan items. Already-installed packages from earlier in the
plan are NOT rolled back — they're idempotent and harmless to leave
on disk; the next `tau resolve` / `tau run` will re-pick up where this
one failed.

Install order is **declaration order in the project tau.toml** (stable
across runs). Sequential — concurrent fetch parallelism is deferred to
a future perf sub-project.

---

## 7. CLI surface

### 7.1 Lazy resolve at `tau run` / `tau chat`

Replaces the existing Step 5 "verify each requires.tools entry is
installed" check at `crates/tau-cli/src/config/agent.rs:240-254`. Now
the flow is:

1. Parse project tau.toml → `ProjectConfig`.
2. Build `Vec<(AgentId, RequiredTool)>` for the targeted agent.
3. Call `tau_pkg::resolve_requires_tools(&[...], &scope)` → `ResolutionPlan`.
4. If `--no-install` is set:
   - Plan empty → proceed.
   - Plan non-empty → exit 2 with the helpful-error hint (§7.4).
5. Otherwise emit npm-style progress lines while the installer fetches.
6. Continue to existing `build_agent_definition` flow. Step 5 is gone
   (resolution + lockfile lookup replaces it).

The existing `RequiredToolMissing` error variant at
`crates/tau-cli/src/config/agent.rs:71-75` is **deleted** — it's
unreachable post-resolve.

### 7.2 New `tau resolve` subcommand

```
tau resolve [--no-install] [--dry-run] [--json]
```

- Reads project tau.toml.
- Resolves `requires.tools` for **all** agents combined (not just one).
- Prints the same npm-style progress as the lazy path.
- `--dry-run`: print plan, exit 0, no fetch.
- `--no-install`: print missing-deps hints, exit 2 if anything missing.
- `--json`: structured event stream (§7.3).
- Default: install missing, reuse compatible installed, exit 0.
- Idempotent: re-running on a fully-resolved project is a no-op.

Lives at `crates/tau-cli/src/cmd/resolve.rs`. Dispatcher in `cli.rs`
gains `Resolve(ResolveArgs)` variant on `CliCommand`.

### 7.3 Output channels

**Human mode** (default), via `output.status(...)` (stderr, suppressed
by `--quiet`):

```
[resolve] 3 required tools — 1 already installed, 2 to fetch
[install] fs-read ^0.1 from git+https://example.com/fs-read.git
[install] fs-read 0.1.4 (1.2s)
[install] shell ^0.1 from git+https://example.com/shell.git@main
[install] shell 0.1.0 (0.8s)
[resolve] done in 2.0s
```

**JSON mode** (`--json`), one event per line via `output.json_event(...)`:

```json
{"event":"resolve_start","required":3,"installed":1,"to_fetch":2}
{"event":"install_start","name":"fs-read","version_req":"^0.1","source":{"kind":"git","url":"https://example.com/fs-read.git"}}
{"event":"install_complete","name":"fs-read","version":"0.1.4","duration_ms":1200}
{"event":"install_start","name":"shell","version_req":"^0.1","source":{"kind":"git","url":"https://example.com/shell.git","rev":"main"}}
{"event":"install_complete","name":"shell","version":"0.1.0","duration_ms":800}
{"event":"resolve_complete","duration_ms":2000}
```

Existing JSON consumers parse line-by-line. Backward-compatible (new
event kinds, no removed/changed ones).

### 7.4 `--no-install` helpful-error hint

```
tau: 2 tools missing; --no-install set. To install:
  tau install git+https://example.com/fs-read.git
  tau install git+https://example.com/shell.git
```

Exact `tau install <url>` commands are derived from each
`PlannedInstall.source`. For `PackageSource::Git { url, rev: None }`,
the hint is `tau install git+<url>`. For `Git { url, rev: Some(r) }`,
it's `tau install git+<url>@<r>`. For `Path { path }`, it's
`tau install path:<path>`.

---

## 8. Type changes

### 8.1 New types in `tau-pkg`

```rust
// tau-pkg::resolve

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ResolutionPlan {
    pub installs: Vec<PlannedInstall>,
    pub reuses:   Vec<ReusedInstall>,
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct PlannedInstall {
    pub name: PackageName,
    pub version: Version,
    pub source: PackageSource,
    pub requested_by: Vec<AgentId>,
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ReusedInstall {
    pub name: PackageName,
    pub version: Version,
}

pub fn resolve_requires_tools(
    requires: &[(AgentId, RequiredTool)],
    scope: &Scope,
) -> Result<ResolutionPlan, ResolveError>;

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("tool {name:?}: agents {agents:?} declared conflicting sources")]
    ConflictingSources {
        name: PackageName,
        sources: Vec<PackageSource>,
        agents: Vec<AgentId>,
    },
    #[error("tool {name:?}: agents {agents:?} declared incompatible version constraints {constraints:?}")]
    IncompatibleVersions {
        name: PackageName,
        constraints: Vec<VersionReq>,
        agents: Vec<AgentId>,
    },
    #[error("tool {name:?} from {source:?}: no version satisfies all of {constraints:?}; available: {available:?}")]
    NoCompatibleVersion {
        name: PackageName,
        source: PackageSource,
        constraints: Vec<VersionReq>,
        available: Vec<Version>,
    },
    #[error("listing versions at {source:?}: {source_err}")]
    SourceListing {
        source: PackageSource,
        #[source]
        source_err: SourceListError,
    },
    #[error("installing {name} from {source:?}: {install_err}")]
    Install {
        name: PackageName,
        source: PackageSource,
        #[source]
        install_err: InstallError,
    },
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SourceListError {
    #[error("git ls-remote failed: {0}")]
    GitLsRemote(String),
    #[error("path manifest read failed: {0}")]
    PathManifest(#[from] ManifestReadError),
    #[error("source kind not yet supported for listing")]
    Unsupported,
}
```

### 8.2 Modified types in `tau-cli::config::project`

```rust
// UncheckedRequires.tools changes shape
pub struct UncheckedRequires {
    #[serde(default)]
    pub tools: Vec<UncheckedRequiredTool>,
    #[serde(default)]
    pub packages: Vec<String>, // unchanged; advisory at v0.1
}

#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UncheckedRequiredTool {
    pub name: String,
    pub source: tau_domain::PackageSource,
    #[serde(default)]
    pub version: Option<String>,
}

// RequiresEntry.tools likewise
pub struct RequiresEntry {
    pub tools: Vec<RequiredTool>,
}

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct RequiredTool {
    pub name: PackageName,
    pub source: PackageSource,
    pub version_req: VersionReq, // parsed; default VersionReq::STAR
}
```

`ProjectConfigError` gains:
```rust
#[error("agent {agent_id:?}: requires.tools[{index}]: bare-string {value:?} no longer supported; use struct form with `source` per spec docs/superpowers/specs/2026-04-30-transitive-deps-design.md §4")]
RequiresToolsBareStringRejected {
    agent_id: String,
    index: usize,
    value: String,
},
```

`AgentResolutionError::RequiredToolMissing` is **removed** (unreachable
post-resolve). This is a breaking removal from a `#[non_exhaustive]`
enum — in-tree only; matches the precedent set by Tier 2 priority 4's
removal of `CapabilityOverrideUnsupported`.

### 8.3 New CLI args

```rust
// cli.rs
pub struct RunArgs {
    // existing fields...
    /// Skip auto-install of missing requires.tools dependencies.
    /// If anything would need fetching, exit 2 with copy-pasteable hints.
    #[arg(long)]
    pub no_install: bool,
}
// ChatArgs gains the same field

pub struct ResolveArgs {
    /// Skip install; print missing-deps hints and exit 2 if anything missing.
    #[arg(long)]
    pub no_install: bool,
    /// Print the resolution plan without fetching anything.
    #[arg(long)]
    pub dry_run: bool,
}

pub enum CliCommand {
    // existing variants...
    Resolve(ResolveArgs),
}
```

---

## 9. Module layout

| Path | Status | Purpose |
|------|--------|---------|
| `Cargo.toml` (root) | (no change) | `semver`, `git2` not added; we use `git ls-remote` via shell-out for parity with existing `tau-pkg` git clone path |
| `crates/tau-pkg/src/resolve.rs` | Create | `resolve_requires_tools`, `ResolutionPlan`, `PlannedInstall`, `ReusedInstall`, `ResolveError`. Phases 1–3. ~200 LOC. |
| `crates/tau-pkg/src/source_list.rs` | Create | `list_versions_at_source(source) -> Vec<Version>`; `git ls-remote --tags` shell-out + tag-to-semver filter; `Path` manifest read. ~80 LOC. |
| `crates/tau-pkg/src/lib.rs` | Modify | Export `ResolutionPlan`, `PlannedInstall`, `ReusedInstall`, `ResolveError`, `SourceListError`, `resolve_requires_tools`. |
| `crates/tau-cli/src/config/project.rs` | Modify | `UncheckedRequiredTool`/`RequiredTool` typed structs; `RequiresEntry.tools` shape change; `RequiresToolsBareStringRejected` error. |
| `crates/tau-cli/src/config/agent.rs` | Modify | Step 5 (`requires.tools` verify) → resolve + lockfile-reuse-or-install flow. Removes `RequiredToolMissing` variant. |
| `crates/tau-cli/src/cmd/run.rs` | Modify | Lazy resolve before agent definition build; `--no-install` flag handling. |
| `crates/tau-cli/src/cmd/chat.rs` | Modify | Same. |
| `crates/tau-cli/src/cmd/resolve.rs` | Create | New `tau resolve` subcommand. ~100 LOC. |
| `crates/tau-cli/src/cli.rs` | Modify | `CliCommand::Resolve(ResolveArgs)`; `--no-install` on `RunArgs`/`ChatArgs`. |
| `crates/tau-cli/src/cmd/plugin_loader.rs` | Modify | Iterates `entry.requires.tools`; needs `.iter().map(|t| &t.name)` adjustment to handle the new struct shape. |
| `crates/tau-cli/tests/common/mod.rs` | Modify | Helper that builds the `requires` block from `Vec<String>` → builds from `Vec<RequiredTool>`. Test fixtures across the crate adapt. |
| `crates/tau-cli/tests/cmd_resolve.rs` | Create | assert_cmd integration tests for `tau resolve` (~5 tests). |
| `docs/decisions/0007-tau-cli.md` | Modify | §5 amendment: drop "advisory only" language; link to this spec. (Same pattern as §4 amendment shipped in Tier 2 priority 4.) |
| `ROADMAP.md` | Modify | Mark Tier 2 priority 5 ✅ Shipped. |

---

## 10. Testing tier

| Tier | Coverage | Where |
|------|----------|-------|
| Unit | semver intersection across 0/1/2/N constraints; conflicting sources; no-compatible-version; lockfile reuse path | `crates/tau-pkg/src/resolve.rs::tests` (~12 tests) |
| Unit | `list_versions_at_source` parses `git ls-remote --tags` output; handles non-semver tags gracefully; `Path` manifest read | `crates/tau-pkg/src/source_list.rs::tests` (~5 tests) |
| Unit | `UncheckedRequiredTool` deserialize: struct form OK; bare string rejected; missing `source` rejected; unknown fields rejected; default `version` = `"*"` | `crates/tau-cli/src/config/project.rs::tests` (~5 tests) |
| Integration | `tau resolve` end-to-end via `assert_cmd`: dry-run, no-install, full install path with `Path` source (avoids real network) | `crates/tau-cli/tests/cmd_resolve.rs` (~5 tests) |
| Integration | `tau run --no-install` errors with copy-pasteable install hints when deps missing | `crates/tau-cli/tests/cmd_run.rs` (~2 tests) |
| Integration | `tau run` lazy path: missing deps trigger install (via `Path` source fixture); run proceeds | `crates/tau-cli/tests/cmd_run.rs` (~2 tests) |

`Path` source is the test workhorse — it lets integration tests exercise
the install path without `git ls-remote`. A separate `Git` smoke test
that hits a real public repo (e.g., the project's own GitHub URL) is
NOT added at this sub-project — flaky network in CI is worse than
deferring the smoke. Manual smoke documented in the PR description.

---

## 11. Best-possible-security model

Auto-install fetches code and runs `cargo build` against it (per the
existing `tau install` flow at `tau-pkg/src/install.rs`). This is real
trust the user is extending. The design preserves all existing
mitigations:

1. **No silent install.** Progress lines print every package + source
   before any cargo build runs. Users can interrupt mid-resolve.
2. **No new code paths in the install pipeline.** The resolver delegates
   to `tau_pkg::install_with_options` verbatim — same lock acquisition,
   same git clone path, same cargo build sandboxing (which is "none" at
   v0.1 per ADR-0004; sandboxing is Tier 3 priority 12).
3. **Source field is mandatory and typed.** Users explicitly declare
   the URL they're trusting; no implicit registry resolution.
4. **`--no-install` is a hard opt-out.** Restricted CI environments can
   gate `tau run` to fail loudly rather than fetch unexpectedly.
5. **Lockfile reuse is the default.** Once installed at a specific
   version, future runs reuse — no repeat fetches, no surprise updates.

What this design does NOT add:
- Source-pin verification (sha256 of the cloned tree). ADR-0004 §10 notes
  the lockfile's `sha256` is empty at v0.1; `tau verify` (Tier 2 priority
  7) is the right place to populate + verify it.
- Sandboxing of `cargo build`. Tier 3 priority 12.
- Network disable / offline mode beyond `--no-install`. Future work.

---

## 12. Implementation plan outline (~10–12 tasks)

The plan derived from this spec will have these tasks. Final wording
lives in the implementation plan.

1. `tau-pkg::source_list` module — `list_versions_at_source` for `Git`
   (ls-remote + semver-tag-filter) and `Path` (manifest read). Unit tests.
2. `tau-pkg::resolve` module — types + `resolve_requires_tools`. Unit
   tests covering Phases 1–3 + all `ResolveError` variants.
3. `tau-cli::config::project` schema upgrade — typed
   `UncheckedRequiredTool`; reject bare strings; new error variant.
   Existing test fixtures in this file adapt.
4. `tau-cli::config::agent` resolve integration — Step 5 becomes
   resolve + lockfile-reuse-or-install. Removes `RequiredToolMissing`.
   `plugin_loader.rs` adapts to the new struct shape.
5. `tau-cli::tests::common::mod` test scaffold helper update — every
   test fixture in tau-cli that builds a `[agents.<id>.requires]` block
   adapts via this helper.
6. `tau-cli::cmd::run` lazy path — calls resolve before agent build;
   `--no-install` flag handling; npm-style progress output.
7. `tau-cli::cmd::chat` lazy path — same.
8. `tau-cli::cmd::resolve` new subcommand — full + dry-run + no-install
   + JSON modes. `cli.rs` dispatcher update.
9. e2e integration tests at `tau-cli` level (cmd_resolve.rs and
   cmd_run.rs additions) using `Path` source fixtures.
10. ADR-0007 §5 amendment + ROADMAP Tier 2 priority 5 done. Squash merge.

Each task is a single Conventional Commits commit, following the
established sub-project pattern. CI: no new jobs (no new workspace
member; no new external service in CI). Branch protection stays at 23.

---

## 13. Out of scope

- **Recursive package-level `dependencies` resolution** — ADR-0004 §10
  stays deferred. `PackageDep` (the package-manifest field) doesn't
  even have a source field at v0.1; that schema extension is the
  blocker for a future Cargo-style transitive resolver.
- **Registry-source kind.** No registry exists. Only `Git` and `Path`
  via `PackageSource`'s existing variants.
- **Concurrent fetch parallelism.** Sequential install is fine for the
  typical 2–5 deps per project. Real perf work for Tier 4.
- **Source-pin verification (sha256 lockfile).** Owned by Tier 2
  priority 7 (`tau verify`).
- **Live network smoke tests.** Flaky CI worse than no test; manual
  smoke documented in PR description.
- **`tau update` / `tau verify` / `tau uninstall`.** Tier 2 priority 7
  — separate sub-project.
- **Hostname-glob, registry resolution, mirror sources.** Phase 2+.

---

## 14. Cross-references

- ADR-0004 §10 — package-level `dependencies` deferral; this spec
  closes the project-level half.
- ADR-0004 §11 — install lock; reused verbatim by the resolver.
- ADR-0007 §5 — original advisory-only reservation; this spec realizes
  it.
- ADR-0009 — typed-error policy; new `ResolveError` follows it.
- ROADMAP Tier 2 priority 5 — this is the priority being closed.
- `crates/tau-cli/src/config/agent.rs:71-75` — current
  `RequiredToolMissing` error, removed by this work.
- `crates/tau-cli/src/config/agent.rs:240-254` — current Step 5 verify
  block, replaced by the resolve flow.
- `crates/tau-pkg/src/install.rs` — existing install pipeline, unchanged
  (resolver delegates to it).
- `crates/tau-domain/src/package/manifest.rs:34` — `PackageDep` struct
  whose schema extension blocks the recursive Tier 3+ work.
