# Transitive Dependency Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement auto-install for `[agents.<id>.requires.tools]`, realizing ADR-0007 §5 reservation.

**Architecture:** Two new tau-pkg modules — `source_list` (versions available at a source via `git ls-remote --tags`) and `resolve` (group-by-name + intersect constraints + pick highest-compatible). tau-cli's `tau run`/`chat` lazily resolve before agent build; new `tau resolve` subcommand serves CI cache warm-up. The schema gains a typed `requires.tools` array — bare strings rejected at parse with a migration hint. `tau_pkg::install_with_options` is reused verbatim for the actual fetch.

**Tech Stack:** Rust 2021, semver (already workspace dep), tokio (process), thiserror, tempfile (test fixtures), assert_cmd (integration tests).

---

## Plan-erratum (carryover constraints from prior sub-projects)

Apply preemptively. Do NOT re-derive.

- **`tau_domain::PackageSource` is `#[non_exhaustive]`** with one variant at v0.1 (`Git { location: GitLocation, rev: Option<String> }`). Wire format is a string per `PackageSource::FromStr` — `<location>` or `<location>#<rev>`. NO `Path` variant. Local on-disk fixtures use `file://` URLs (git clones from `file://` natively).
- **`tau_domain::PackageDep`, `PackageManifest`, `PackageId`** all `#[non_exhaustive]`. Construct via `::new` constructors or deserialization.
- **`tau_domain::PackageName`** validated newtype, use `from_str`. **`Version`** is the re-exported `semver::Version`, use `parse`.
- **New types in tau-pkg** — `ResolutionPlan`, `PlannedInstall`, `ReusedInstall`, `ResolveError`, `SourceListError` — all `#[non_exhaustive]`. Provide constructor functions. `ResolveError`/`SourceListError` use `thiserror::Error` per ADR-0009.
- **`ProjectConfigError` is `#[non_exhaustive]`.** Add `RequiresToolsBareStringRejected { agent_id, index, value }` (additive). REMOVE `AgentResolutionError::RequiredToolMissing { agent_id, tool }` from the `agent.rs` error enum (in-tree only — only tau-cli's own tests reference it; same precedent as Task 3 of priority 4 removed `CapabilityOverrideUnsupported`).
- **`RequiresEntry.tools` shape** changes from `Vec<String>` to `Vec<RequiredTool>` (validated form). The unchecked deserialization shape is `Vec<UncheckedRequiredTool>` with `#[serde(deny_unknown_fields)]`.
- **`#[serde(deny_unknown_fields)]`** on `UncheckedRequiredTool` (per established convention from priorities 4 + 5).
- **`RunArgs` and `ChatArgs`** get a new `--no-install` flag (clap-derived bool).
- **New `Command::Resolve(ResolveArgs)`** variant in `crates/tau-cli/src/cli.rs` (the enum is named `Command`, NOT `CliCommand`). `ResolveArgs` carries `--no-install` and `--dry-run`. `--json` is global on `Cli`.
- **Test scaffold** at `crates/tau-cli/tests/common/mod.rs` (line ~408) builds the `[agents.<id>.requires]` block from `Vec<String>` today. Must change to take `Vec<RequiredTool>` (or build from a struct fixture). Existing tests in tau-cli that pass strings to this helper need updating.
- **Doctests on `#[non_exhaustive]` types** must be `ignore`-marked.
- **For tests destructuring `#[non_exhaustive]` enums cross-crate:** `let X { fields, .. } = value else { panic!() };`.
- **The resolver does NOT acquire the install lock itself.** Each per-package `tau_pkg::install_with_options(...)` call acquires + releases per ADR-0004 §11.
- **`Git { rev: Some(_) }` resolution** shallow-clones into a tempdir to read the manifest at that rev (single-point version space).
- **`Git { rev: None }` resolution** uses `git ls-remote --tags <location>`, parses + filters to semver-parsable tags (strip leading `v` if present), drops non-semver tags.
- **The install pipeline (`tau_pkg::install_with_options`) is unchanged** — resolver delegates to it verbatim.
- **NO new CI jobs.** No new workspace member; no new external service in CI. Branch protection stays at 23 required checks.

---

## File structure

| Path | Status | Purpose |
|------|--------|---------|
| `crates/tau-pkg/src/source_list.rs` | Create | `list_versions_at_source(source) -> Vec<Version>` — `git ls-remote --tags` for `Git { rev: None }`, shallow clone + manifest read for `Git { rev: Some }`. ~150 LOC + tests. |
| `crates/tau-pkg/src/resolve.rs` | Create | `resolve_requires_tools(requires, scope) -> ResolutionPlan` — three phases (group / conflict / pick). Types `ResolutionPlan`, `PlannedInstall`, `ReusedInstall`, `ResolveError`. ~250 LOC + tests. |
| `crates/tau-pkg/src/lib.rs` | Modify | Declare new modules; re-export new public types. |
| `crates/tau-cli/src/config/project.rs` | Modify | Replace `Vec<String>` with `Vec<UncheckedRequiredTool>`; add `RequiredTool` validated struct; add `RequiresToolsBareStringRejected` error. |
| `crates/tau-cli/src/config/agent.rs` | Modify | Step 5 (verify) → resolve+install flow. Remove `RequiredToolMissing`. |
| `crates/tau-cli/src/cmd/plugin_loader.rs` | Modify | Iterate `entry.requires.tools` via `&t.name` instead of `t`. |
| `crates/tau-cli/tests/common/mod.rs` | Modify | Helper builds the requires block from struct fixtures. |
| `crates/tau-cli/src/cmd/run.rs` | Modify | Lazy resolve before agent build; respect `--no-install`/`--dry-run`. |
| `crates/tau-cli/src/cmd/chat.rs` | Modify | Mirror of run.rs. |
| `crates/tau-cli/src/cmd/resolve.rs` | Create | New `tau resolve` subcommand. ~120 LOC. |
| `crates/tau-cli/src/cmd/mod.rs` | Modify | Register `resolve` submodule. |
| `crates/tau-cli/src/cli.rs` | Modify | Add `Command::Resolve(ResolveArgs)`; add `--no-install` to `RunArgs`/`ChatArgs`. |
| `crates/tau-cli/src/main.rs` | Modify | Dispatch `Command::Resolve` to `cmd::resolve::run`. |
| `crates/tau-cli/tests/cmd_resolve.rs` | Create | assert_cmd integration tests via `file://` fixtures. |
| `crates/tau-cli/tests/cmd_run.rs` | Modify | Add `--no-install` and lazy-install tests. |
| `crates/tau-cli/tests/snapshots/help_snapshots__resolve_help.snap` | Create | Insta snapshot for `tau resolve --help`. |
| `docs/decisions/0007-tau-cli.md` | Modify (Task 10) | §5 amendment. |
| `ROADMAP.md` | Modify (Task 10) | Mark Tier 2 priority 5 ✅. |

---

## Task 1: `tau-pkg::source_list` module

**Files:**
- Create: `crates/tau-pkg/src/source_list.rs`
- Modify: `crates/tau-pkg/src/lib.rs` — declare module + re-export
- Verify: `crates/tau-pkg/Cargo.toml` — `semver`, `tempfile` already present

### Steps

- [ ] **Step 1.1: Verify Cargo.toml**

Run from repo root:
```bash
grep -E "^(semver|tempfile|thiserror)\s*=" crates/tau-pkg/Cargo.toml
```
Expected: all three appear in `[dependencies]` (`semver`, `thiserror`) or `[dev-dependencies]` (`tempfile`). If `semver` is not in `[dependencies]`, add `semver = { workspace = true }` after the existing `thiserror` line. If `tempfile` is not in `[dev-dependencies]`, add `tempfile = { workspace = true }`.

- [ ] **Step 1.2: Declare module in `lib.rs`**

Edit `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/lib.rs`. Find the existing module declarations (around the top of the file, look for `pub mod registry;` or similar). Add:

```rust
pub mod source_list;
```

- [ ] **Step 1.3: Create `source_list.rs` with full content**

Create `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/source_list.rs` with this exact content:

```rust
//! Enumerate available versions at a [`PackageSource`].
//!
//! Used by the resolver (`crate::resolve`) to pick a concrete
//! `Version` for a `(name, source, version_req)` triple. For
//! `Git { rev: None }` we shell out to `git ls-remote --tags` and
//! filter the tag list to those parsing as `semver::Version` (after
//! stripping a leading `v` if present). For `Git { rev: Some(_) }`
//! the source is a single point in version space — we shallow-clone
//! the rev into a tempdir, read the manifest, and return that one
//! version.
//!
//! See `docs/superpowers/specs/2026-04-30-transitive-deps-design.md` §5.4.

use std::path::Path;
use std::process::Command;

use semver::Version;
use tau_domain::{GitLocation, PackageSource};
use tempfile::TempDir;

use crate::manifest::read_manifest;

/// List the versions available at `source`.
pub fn list_versions_at_source(source: &PackageSource) -> Result<Vec<Version>, SourceListError> {
    match source {
        PackageSource::Git {
            location,
            rev: None,
        } => list_git_tags(location),
        PackageSource::Git {
            location,
            rev: Some(rev),
        } => single_version_at_rev(location, rev),
        // Unreachable today (Git is the only variant), but the enum is
        // `#[non_exhaustive]`, so the catch-all is required for forward
        // compatibility. Future variants land here as `Unsupported`
        // until the resolver explicitly handles them.
        _ => Err(SourceListError::Unsupported),
    }
}

fn list_git_tags(location: &GitLocation) -> Result<Vec<Version>, SourceListError> {
    let url = location.to_string();
    let output = Command::new("git")
        .arg("ls-remote")
        .arg("--tags")
        .arg(&url)
        .output()
        .map_err(|e| SourceListError::GitInvoke {
            message: format!("spawning `git ls-remote`: {e}"),
        })?;
    if !output.status.success() {
        return Err(SourceListError::GitLsRemote {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_ls_remote_tags(&stdout))
}

/// Parse `git ls-remote --tags` stdout into a sorted `Vec<Version>`.
///
/// Format: each line is `<sha>\trefs/tags/<tag>` (or `refs/tags/<tag>^{}`
/// for annotated-tag peels). We extract the tag, strip a leading `v`
/// if present, parse as `semver::Version`, drop non-semver tags. The
/// returned vec is sorted ascending; resolver picks the last entry
/// satisfying constraints (= the highest).
fn parse_ls_remote_tags(stdout: &str) -> Vec<Version> {
    let mut versions: Vec<Version> = stdout
        .lines()
        .filter_map(|line| {
            let tag = line.split('\t').nth(1)?;
            // Strip refs/tags/ prefix.
            let tag = tag.strip_prefix("refs/tags/")?;
            // Drop the ^{} peel suffix (annotated tag commit pointer).
            let tag = tag.strip_suffix("^{}").unwrap_or(tag);
            // Strip leading `v`.
            let tag = tag.strip_prefix('v').unwrap_or(tag);
            Version::parse(tag).ok()
        })
        .collect();
    versions.sort();
    versions.dedup();
    versions
}

fn single_version_at_rev(
    location: &GitLocation,
    rev: &str,
) -> Result<Vec<Version>, SourceListError> {
    let tempdir = TempDir::new().map_err(|e| SourceListError::TempDir {
        message: format!("creating tempdir for shallow clone: {e}"),
    })?;
    let dest = tempdir.path().join("repo");
    let url = location.to_string();
    let output = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--branch")
        .arg(rev)
        .arg("--single-branch")
        .arg(&url)
        .arg(&dest)
        .output()
        .map_err(|e| SourceListError::GitInvoke {
            message: format!("spawning `git clone`: {e}"),
        })?;
    if !output.status.success() {
        return Err(SourceListError::GitClone {
            rev: rev.to_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let manifest_path = dest.join("tau.toml");
    let manifest = read_manifest(&manifest_path).map_err(SourceListError::Manifest)?;
    Ok(vec![manifest.version().clone()])
}

#[allow(dead_code)] // wired up by Task 2's resolver
pub(crate) fn _shallow_clone_for_test(
    location: &GitLocation,
    rev: &str,
    dest: &Path,
) -> Result<(), SourceListError> {
    let url = location.to_string();
    let output = Command::new("git")
        .arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--branch")
        .arg(rev)
        .arg("--single-branch")
        .arg(&url)
        .arg(dest)
        .output()
        .map_err(|e| SourceListError::GitInvoke {
            message: format!("spawning `git clone`: {e}"),
        })?;
    if !output.status.success() {
        return Err(SourceListError::GitClone {
            rev: rev.to_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// Errors produced by [`list_versions_at_source`].
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SourceListError {
    /// `git` binary not invocable.
    #[error("git invocation failed: {message}")]
    GitInvoke {
        /// Human-readable failure context.
        message: String,
    },
    /// `git ls-remote` exited non-zero.
    #[error("git ls-remote failed: {stderr}")]
    GitLsRemote {
        /// Captured stderr.
        stderr: String,
    },
    /// `git clone` for a `rev`-pinned source failed.
    #[error("git clone of {rev:?} failed: {stderr}")]
    GitClone {
        /// The rev that was passed to `--branch`.
        rev: String,
        /// Captured stderr.
        stderr: String,
    },
    /// Could not create a tempdir for the shallow clone.
    #[error("tempdir creation failed: {message}")]
    TempDir {
        /// Human-readable failure context.
        message: String,
    },
    /// Reading the manifest at the cloned rev failed.
    #[error("manifest read failed: {0}")]
    Manifest(#[from] crate::error::ManifestReadError),
    /// `PackageSource` variant not supported by the resolver. Reserved
    /// for future variants — `Git` is the only variant at v0.1.
    #[error("source kind not supported by resolver")]
    Unsupported,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a stdout fixture mimicking `git ls-remote --tags` output.
    /// Each line: `<40-char-sha>\trefs/tags/<tag>`.
    fn fake_ls_remote(tags: &[&str]) -> String {
        tags.iter()
            .map(|t| format!("0123456789abcdef0123456789abcdef01234567\trefs/tags/{t}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn parse_ls_remote_tags_returns_only_semver_parsable_tags() {
        let stdout = fake_ls_remote(&["v0.1.0", "v0.2.0", "release-2024", "main"]);
        let versions = parse_ls_remote_tags(&stdout);
        assert_eq!(versions, vec![
            Version::parse("0.1.0").unwrap(),
            Version::parse("0.2.0").unwrap(),
        ]);
    }

    #[test]
    fn parse_ls_remote_tags_strips_v_prefix() {
        let stdout = fake_ls_remote(&["v1.2.3", "0.4.5"]);
        let versions = parse_ls_remote_tags(&stdout);
        assert_eq!(versions, vec![
            Version::parse("0.4.5").unwrap(),
            Version::parse("1.2.3").unwrap(),
        ]);
    }

    #[test]
    fn parse_ls_remote_tags_drops_annotated_tag_peels() {
        // Annotated tags appear twice: once as `<tag>` and once as `<tag>^{}`.
        // Both should resolve to the same Version, deduped.
        let stdout = fake_ls_remote(&["v1.0.0", "v1.0.0^{}"]);
        let versions = parse_ls_remote_tags(&stdout);
        assert_eq!(versions, vec![Version::parse("1.0.0").unwrap()]);
    }

    #[test]
    fn parse_ls_remote_tags_returns_empty_for_no_semver_tags() {
        let stdout = fake_ls_remote(&["release-1", "rc-foo", "untagged"]);
        let versions = parse_ls_remote_tags(&stdout);
        assert!(versions.is_empty());
    }

    #[test]
    fn parse_ls_remote_tags_returns_sorted_ascending() {
        let stdout = fake_ls_remote(&["v2.0.0", "v0.5.0", "v1.0.0"]);
        let versions = parse_ls_remote_tags(&stdout);
        assert_eq!(
            versions,
            vec![
                Version::parse("0.5.0").unwrap(),
                Version::parse("1.0.0").unwrap(),
                Version::parse("2.0.0").unwrap(),
            ]
        );
    }

    /// Set up a local git repository in a tempdir, with a tau.toml
    /// declaring `name = "test-tool"` and `version = "0.1.0"`, then a
    /// commit + tag `v0.1.0`. Returns the tempdir guard + the file://
    /// URL pointing at the bare repo.
    fn make_local_git_fixture(version: &str) -> (TempDir, GitLocation) {
        let tempdir = TempDir::new().unwrap();
        let repo = tempdir.path().join("test-tool");
        std::fs::create_dir(&repo).unwrap();
        let manifest_body = format!(
            r#"
name = "test-tool"
version = "{version}"
description = "fixture"
authors = []
source = "https://example.com/test.git"
kind = "tool"
dependencies = []
capabilities = []
"#
        );
        std::fs::write(repo.join("tau.toml"), manifest_body).unwrap();

        let run = |args: &[&str]| {
            let out = Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "fixture"]);
        run(&["tag", &format!("v{version}")]);

        let url = format!("file://{}", repo.display());
        let location: GitLocation = url.parse().unwrap();
        (tempdir, location)
    }

    #[test]
    fn list_git_tags_against_local_file_url_finds_tag() {
        let (_tempdir, location) = make_local_git_fixture("0.1.0");
        let versions = list_git_tags(&location).unwrap();
        assert_eq!(versions, vec![Version::parse("0.1.0").unwrap()]);
    }

    #[test]
    fn single_version_at_rev_clones_and_reads_manifest() {
        let (_tempdir, location) = make_local_git_fixture("0.3.5");
        let versions = single_version_at_rev(&location, "v0.3.5").unwrap();
        assert_eq!(versions, vec![Version::parse("0.3.5").unwrap()]);
    }
}
```

- [ ] **Step 1.4: Re-export public surface from `lib.rs`**

In `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/lib.rs`, add to the `pub use` block (or near other re-exports):

```rust
pub use source_list::{list_versions_at_source, SourceListError};
```

- [ ] **Step 1.5: Verify**

Run all five commands; report any failures:

```bash
cargo build --workspace
cargo test -p tau-pkg --all-targets source_list
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-pkg --doc
```

Expected: build PASS; 7 unit tests PASS; fmt/clippy/doctest clean.

If clippy flags `#[allow(dead_code)]` on `_shallow_clone_for_test`, drop that helper — Task 2 doesn't actually need it (the shallow clone is encapsulated inside `single_version_at_rev`).

- [ ] **Step 1.6: Commit**

```bash
git add crates/tau-pkg/src/source_list.rs crates/tau-pkg/src/lib.rs
# If Cargo.toml was modified to add semver/tempfile:
# git add crates/tau-pkg/Cargo.toml
git commit -m "$(cat <<'EOF'
feat(pkg): add source_list module — enumerate versions at a PackageSource

Adds list_versions_at_source(source) -> Vec<Version>:
- Git { rev: None }: `git ls-remote --tags <url>`, parse tag list,
  strip leading 'v', filter to semver-parsable, drop non-semver tags,
  return sorted ascending.
- Git { rev: Some(_) }: shallow-clone the rev into a tempdir, read
  tau.toml, return that single Version.

Tests use file:// URLs to local git repos created via tempfile +
git init — no real network. 7 unit tests covering tag parsing
(v-prefix, annotated-tag peels, non-semver drop, sort order) plus
end-to-end against a local fixture.

Refs: docs/superpowers/specs/2026-04-30-transitive-deps-design.md §5.4

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 1.7: Push**

```bash
git push
```

---

## Task 2: `tau-pkg::resolve` module + types

**Files:**
- Create: `crates/tau-pkg/src/resolve.rs`
- Modify: `crates/tau-pkg/src/lib.rs` — declare module + re-export

### Steps

- [ ] **Step 2.1: Declare module in lib.rs**

Add `pub mod resolve;` next to `pub mod source_list;`.

- [ ] **Step 2.2: Create `resolve.rs` with full content**

Create `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/resolve.rs`:

```rust
//! Resolve transitive `requires.tools` dependencies for a project's
//! agents into a concrete install plan.
//!
//! Three phases (see spec §5):
//!   1. Group input entries by `name`.
//!   2. Conflict checks per name: source equality + version-constraint
//!      compatibility.
//!   3. Pick a concrete `Version` per name: lockfile reuse if installed
//!      and compatible, else list available versions at source and pick
//!      the highest satisfying all constraints.
//!
//! One level deep at v0.1: we resolve agents' `requires.tools` only;
//! recursive package-level `dependencies` resolution stays deferred
//! per ADR-0004 §10.

use std::collections::BTreeMap;

use semver::{Version, VersionReq};
use tau_domain::{AgentId, PackageName, PackageSource};

use crate::registry;
use crate::scope::Scope;
use crate::source_list::{list_versions_at_source, SourceListError};

/// One required-tool declaration drawn from a project tau.toml.
///
/// Constructed by tau-cli during project-config validation; passed
/// into [`resolve_requires_tools`] as `(AgentId, RequiredTool)` pairs.
///
/// `#[non_exhaustive]`: external callers must use [`RequiredTool::new`].
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct RequiredTool {
    /// Package name to resolve.
    pub name: PackageName,
    /// Source to fetch from.
    pub source: PackageSource,
    /// Semver requirement; defaults to `*` if absent.
    pub version_req: VersionReq,
}

impl RequiredTool {
    /// Construct a `RequiredTool`. `#[non_exhaustive]` blocks struct-literal
    /// construction outside this crate.
    pub fn new(name: PackageName, source: PackageSource, version_req: VersionReq) -> Self {
        Self {
            name,
            source,
            version_req,
        }
    }
}

/// Output of [`resolve_requires_tools`].
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct ResolutionPlan {
    /// Packages that need to be fetched + installed.
    pub installs: Vec<PlannedInstall>,
    /// Packages already installed and reused (no fetch).
    pub reuses: Vec<ReusedInstall>,
}

/// One package the resolver decided needs fetching.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct PlannedInstall {
    /// Package name.
    pub name: PackageName,
    /// Concrete version selected from the available set.
    pub version: Version,
    /// Source to fetch from.
    pub source: PackageSource,
    /// Agents that requested this package (for diagnostics).
    pub requested_by: Vec<AgentId>,
}

/// One package the resolver decided to reuse from the lockfile.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ReusedInstall {
    /// Package name.
    pub name: PackageName,
    /// Already-installed version.
    pub version: Version,
}

/// Errors returned by [`resolve_requires_tools`].
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// Two or more agents declared the same tool name with different
    /// `PackageSource` values.
    #[error(
        "tool {name:?}: agents {agents:?} declared conflicting sources {sources:?}"
    )]
    ConflictingSources {
        /// The conflicting tool name.
        name: PackageName,
        /// All distinct sources observed.
        sources: Vec<PackageSource>,
        /// Agents that contributed to the conflict.
        agents: Vec<AgentId>,
    },
    /// No `Version` at the source satisfies the intersected constraints.
    #[error(
        "tool {name:?} from {source}: no version satisfies all of {constraints:?}; available: {available:?}"
    )]
    NoCompatibleVersion {
        /// The tool name.
        name: PackageName,
        /// The source consulted.
        source: PackageSource,
        /// All `version_req` values across the group.
        constraints: Vec<VersionReq>,
        /// Versions returned by `list_versions_at_source`.
        available: Vec<Version>,
    },
    /// `list_versions_at_source` itself failed.
    #[error("listing versions at {source}: {source_err}")]
    SourceListing {
        /// The source we tried to list.
        source: PackageSource,
        /// Underlying source-listing error.
        #[source]
        source_err: SourceListError,
    },
    /// Reading the lockfile failed.
    #[error("reading lockfile: {0}")]
    Registry(#[from] crate::error::RegistryError),
}

/// Resolve a flat list of required tools into a [`ResolutionPlan`].
pub fn resolve_requires_tools(
    requires: &[(AgentId, RequiredTool)],
    scope: &Scope,
) -> Result<ResolutionPlan, ResolveError> {
    // Phase 1: group by name.
    let groups = group_by_name(requires);

    // Phase 2: same-name conflict checks (source equality).
    for (name, entries) in &groups {
        check_source_equality(name, entries)?;
    }

    // Phase 3: pick concrete versions.
    let installed = registry::list(scope)?;
    let mut plan = ResolutionPlan::default();
    for (name, entries) in groups {
        let source = entries[0].1.source.clone();
        let constraints: Vec<VersionReq> =
            entries.iter().map(|(_, t)| t.version_req.clone()).collect();
        let requested_by: Vec<AgentId> =
            entries.iter().map(|(a, _)| a.clone()).collect();

        // Lockfile reuse: any installed version satisfying all constraints?
        if let Some(installed_pkg) = installed.iter().find(|p| p.name == name) {
            if installed_pkg.source == source
                && constraints.iter().all(|c| c.matches(&installed_pkg.active_version))
            {
                plan.reuses.push(ReusedInstall {
                    name: name.clone(),
                    version: installed_pkg.active_version.clone(),
                });
                continue;
            }
        }

        // Otherwise: list available versions, pick highest satisfying all constraints.
        let available = list_versions_at_source(&source).map_err(|source_err| {
            ResolveError::SourceListing {
                source: source.clone(),
                source_err,
            }
        })?;
        let picked = available
            .iter()
            .rev() // highest first
            .find(|v| constraints.iter().all(|c| c.matches(v)))
            .cloned();
        match picked {
            Some(version) => plan.installs.push(PlannedInstall {
                name,
                version,
                source,
                requested_by,
            }),
            None => {
                return Err(ResolveError::NoCompatibleVersion {
                    name,
                    source,
                    constraints,
                    available,
                })
            }
        }
    }
    Ok(plan)
}

fn group_by_name<'a>(
    requires: &'a [(AgentId, RequiredTool)],
) -> BTreeMap<PackageName, Vec<&'a (AgentId, RequiredTool)>> {
    let mut groups: BTreeMap<PackageName, Vec<&(AgentId, RequiredTool)>> = BTreeMap::new();
    for entry in requires {
        groups.entry(entry.1.name.clone()).or_default().push(entry);
    }
    groups
}

fn check_source_equality(
    name: &PackageName,
    entries: &[&(AgentId, RequiredTool)],
) -> Result<(), ResolveError> {
    let first = &entries[0].1.source;
    if entries.iter().all(|(_, t)| &t.source == first) {
        return Ok(());
    }
    let sources: Vec<PackageSource> = {
        let mut seen: Vec<PackageSource> = Vec::new();
        for (_, t) in entries {
            if !seen.contains(&t.source) {
                seen.push(t.source.clone());
            }
        }
        seen
    };
    let agents: Vec<AgentId> = entries.iter().map(|(a, _)| a.clone()).collect();
    Err(ResolveError::ConflictingSources {
        name: name.clone(),
        sources,
        agents,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use tempfile::TempDir;

    fn agent_id(s: &str) -> AgentId {
        AgentId::from_str(s).unwrap()
    }

    fn pkg_name(s: &str) -> PackageName {
        PackageName::from_str(s).unwrap()
    }

    fn ver_req(s: &str) -> VersionReq {
        VersionReq::parse(s).unwrap()
    }

    fn make_source(url: &str) -> PackageSource {
        PackageSource::from_str(url).unwrap()
    }

    fn make_scope() -> (TempDir, Scope) {
        let tmp = TempDir::new().unwrap();
        let scope = Scope::global_at(tmp.path().join("tau-home")).unwrap();
        (tmp, scope)
    }

    #[test]
    fn empty_input_yields_empty_plan() {
        let (_tmp, scope) = make_scope();
        let plan = resolve_requires_tools(&[], &scope).unwrap();
        assert!(plan.installs.is_empty());
        assert!(plan.reuses.is_empty());
    }

    #[test]
    fn group_by_name_unifies_same_name_across_agents() {
        let req = RequiredTool::new(
            pkg_name("fs-read"),
            make_source("https://example.com/fs-read.git"),
            ver_req("^0.1"),
        );
        let groups = group_by_name(&[
            (agent_id("a"), req.clone()),
            (agent_id("b"), req.clone()),
        ]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups.get(&pkg_name("fs-read")).unwrap().len(), 2);
    }

    #[test]
    fn conflicting_sources_rejected() {
        let entries = vec![
            (
                agent_id("a"),
                RequiredTool::new(
                    pkg_name("fs-read"),
                    make_source("https://example.com/fs-read.git"),
                    ver_req("*"),
                ),
            ),
            (
                agent_id("b"),
                RequiredTool::new(
                    pkg_name("fs-read"),
                    make_source("https://other.com/fs-read.git"),
                    ver_req("*"),
                ),
            ),
        ];
        let groups = group_by_name(&entries);
        let group = groups.get(&pkg_name("fs-read")).unwrap();
        let err = check_source_equality(&pkg_name("fs-read"), group).unwrap_err();
        let ResolveError::ConflictingSources { sources, agents, .. } = err else {
            panic!("expected ConflictingSources");
        };
        assert_eq!(sources.len(), 2);
        assert_eq!(agents, vec![agent_id("a"), agent_id("b")]);
    }

    #[test]
    fn matching_sources_accepted() {
        let src = make_source("https://example.com/fs-read.git");
        let entries = vec![
            (agent_id("a"), RequiredTool::new(pkg_name("fs-read"), src.clone(), ver_req("^0.1"))),
            (agent_id("b"), RequiredTool::new(pkg_name("fs-read"), src.clone(), ver_req("^0.1"))),
        ];
        let groups = group_by_name(&entries);
        let group = groups.get(&pkg_name("fs-read")).unwrap();
        check_source_equality(&pkg_name("fs-read"), group).unwrap();
    }

    // The remaining tests exercise the full resolve_requires_tools flow.
    // They use a local file:// git fixture and a temp scope — same pattern
    // as source_list's tests.

    fn make_local_git_fixture(name: &str, version: &str) -> (TempDir, PackageSource) {
        use std::process::Command;
        let tempdir = TempDir::new().unwrap();
        let repo = tempdir.path().join(name);
        std::fs::create_dir(&repo).unwrap();
        let manifest_body = format!(
            r#"
name = "{name}"
version = "{version}"
description = "fixture"
authors = []
source = "https://example.com/{name}.git"
kind = "tool"
dependencies = []
capabilities = []
"#
        );
        std::fs::write(repo.join("tau.toml"), manifest_body).unwrap();
        let run = |args: &[&str]| {
            Command::new("git").current_dir(&repo).args(args).output().unwrap();
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "fixture"]);
        run(&["tag", &format!("v{version}")]);

        let url = format!("file://{}", repo.display());
        let source = PackageSource::from_str(&url).unwrap();
        (tempdir, source)
    }

    #[test]
    fn fresh_install_picks_highest_compatible_version() {
        let (_tmp, source) = make_local_git_fixture("fs-read", "0.1.4");
        let req = RequiredTool::new(pkg_name("fs-read"), source.clone(), ver_req("^0.1"));
        let (_scope_tmp, scope) = make_scope();
        let plan = resolve_requires_tools(&[(agent_id("a"), req)], &scope).unwrap();
        assert_eq!(plan.installs.len(), 1);
        assert_eq!(plan.reuses.len(), 0);
        assert_eq!(plan.installs[0].version, Version::parse("0.1.4").unwrap());
    }

    #[test]
    fn no_compatible_version_rejected() {
        let (_tmp, source) = make_local_git_fixture("fs-read", "0.1.0");
        let req = RequiredTool::new(pkg_name("fs-read"), source.clone(), ver_req("^2"));
        let (_scope_tmp, scope) = make_scope();
        let err = resolve_requires_tools(&[(agent_id("a"), req)], &scope).unwrap_err();
        let ResolveError::NoCompatibleVersion { name, available, .. } = err else {
            panic!("expected NoCompatibleVersion, got: {err:?}");
        };
        assert_eq!(name, pkg_name("fs-read"));
        assert_eq!(available, vec![Version::parse("0.1.0").unwrap()]);
    }

    #[test]
    fn intersection_picks_highest_satisfying_all_constraints() {
        // Fixture has v0.1.0; agent A wants ^0.1 (matches), agent B wants
        // >=0.1.0 (matches). Picked = 0.1.0.
        let (_tmp, source) = make_local_git_fixture("fs-read", "0.1.0");
        let req_a = RequiredTool::new(pkg_name("fs-read"), source.clone(), ver_req("^0.1"));
        let req_b = RequiredTool::new(pkg_name("fs-read"), source.clone(), ver_req(">=0.1.0"));
        let (_scope_tmp, scope) = make_scope();
        let plan = resolve_requires_tools(
            &[(agent_id("a"), req_a), (agent_id("b"), req_b)],
            &scope,
        )
        .unwrap();
        assert_eq!(plan.installs.len(), 1);
        assert_eq!(plan.installs[0].version, Version::parse("0.1.0").unwrap());
        assert_eq!(plan.installs[0].requested_by.len(), 2);
    }

    #[test]
    fn conflicting_sources_propagate_to_resolve() {
        // Two different file:// URLs for the same package name → ConflictingSources.
        let (_tmp_a, source_a) = make_local_git_fixture("fs-read", "0.1.0");
        let (_tmp_b, source_b) = make_local_git_fixture("fs-read", "0.1.0");
        let req_a = RequiredTool::new(pkg_name("fs-read"), source_a, ver_req("*"));
        let req_b = RequiredTool::new(pkg_name("fs-read"), source_b, ver_req("*"));
        let (_scope_tmp, scope) = make_scope();
        let err = resolve_requires_tools(
            &[(agent_id("a"), req_a), (agent_id("b"), req_b)],
            &scope,
        )
        .unwrap_err();
        assert!(matches!(err, ResolveError::ConflictingSources { .. }));
    }

    #[test]
    fn empty_intersection_rejected_via_no_compatible_version() {
        // Fixture v0.1.0 — agent A wants ^0.1 (matches), agent B wants ^0.2
        // (doesn't match 0.1.0). Intersection is empty for the available set.
        let (_tmp, source) = make_local_git_fixture("fs-read", "0.1.0");
        let req_a = RequiredTool::new(pkg_name("fs-read"), source.clone(), ver_req("^0.1"));
        let req_b = RequiredTool::new(pkg_name("fs-read"), source.clone(), ver_req("^0.2"));
        let (_scope_tmp, scope) = make_scope();
        let err = resolve_requires_tools(
            &[(agent_id("a"), req_a), (agent_id("b"), req_b)],
            &scope,
        )
        .unwrap_err();
        assert!(matches!(err, ResolveError::NoCompatibleVersion { .. }));
    }
}
```

- [ ] **Step 2.3: Re-export public surface**

In `crates/tau-pkg/src/lib.rs`, add to the `pub use` block:

```rust
pub use resolve::{
    resolve_requires_tools, PlannedInstall, RequiredTool, ResolutionPlan, ResolveError,
    ReusedInstall,
};
```

- [ ] **Step 2.4: Verify**

```bash
cargo build --workspace
cargo test -p tau-pkg --all-targets resolve
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-pkg --doc
```

Expected: build PASS; ~9 unit tests PASS; fmt/clippy/doctest clean.

The local-git-fixture pattern is duplicated between `source_list.rs` and `resolve.rs` tests — that's intentional for test isolation. If clippy flags this as duplication, ignore (it's test-only code).

- [ ] **Step 2.5: Commit**

```bash
git add crates/tau-pkg/src/resolve.rs crates/tau-pkg/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(pkg): add resolve module — three-phase requires.tools resolver

Phase 1 groups input by name; Phase 2 enforces source equality per
group (different sources for the same name → ConflictingSources);
Phase 3 picks a concrete version per group: lockfile reuse first if
already installed and compatible, else list available versions at the
source and pick the highest satisfying all version_req constraints
(empty intersection → NoCompatibleVersion).

One level deep at v0.1: only agents' requires.tools is resolved;
recursive package-level dependencies stays deferred per ADR-0004 §10.

Tests cover all error variants + happy path + intersection picks
highest + reuse path; use file:// URL fixtures from tempdir + git init.

Refs: docs/superpowers/specs/2026-04-30-transitive-deps-design.md §5

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 2.6: Push**

```bash
git push
```

---

## Task 3: tau-cli `[agents.<id>.requires.tools]` typed schema

**Files:**
- Modify: `crates/tau-cli/src/config/project.rs`

### Steps

- [ ] **Step 3.1: Locate the existing UncheckedRequires definition**

```bash
grep -n "UncheckedRequires\|RequiresEntry\|requires.tools" crates/tau-cli/src/config/project.rs | head
```

Find:
- The `UncheckedRequires` struct (likely around line 56-62 — has `tools: Vec<String>`).
- The `RequiresEntry` struct (likely around line 110-116).
- The validator that converts unchecked → validated (around line 320 — `let requires = raw.requires.map_or...`).

- [ ] **Step 3.2: Replace `UncheckedRequires`**

In `/Users/titouanlebocq/code/tau/crates/tau-cli/src/config/project.rs`, find:

```rust
/// `[agents.<id>.requires]` sub-table.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UncheckedRequires {
    /// Tool package names this agent advises requiring (advisory at v0.1).
    #[serde(default)]
    pub tools: Vec<String>,
    /// Phase 1+; ignored at v0.1.
    #[serde(default)]
    pub packages: Vec<String>,
}
```

Replace with:

```rust
/// `[agents.<id>.requires]` sub-table.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UncheckedRequires {
    /// Required tool packages with explicit source declarations.
    /// Replaces the v0.1 advisory-only `Vec<String>` schema.
    #[serde(default)]
    pub tools: Vec<UncheckedRequiredTool>,
    /// Phase 1+; ignored at v0.1.
    #[serde(default)]
    pub packages: Vec<String>,
}

/// One `[[agents.<id>.requires.tools]]` array entry.
///
/// Replaces the v0.1 bare-string form. Each entry must declare a
/// `source` (typed `PackageSource` — string serde format like
/// `"https://example.com/x.git"` or `"<location>#<rev>"`); `version`
/// is optional and defaults to `"*"`.
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UncheckedRequiredTool {
    /// Package name.
    pub name: String,
    /// Source to fetch from. Reuses `PackageSource::FromStr` serde:
    /// `"<location>"` or `"<location>#<rev>"`.
    pub source: tau_domain::PackageSource,
    /// Optional semver requirement; defaults to `"*"` when absent.
    #[serde(default)]
    pub version: Option<String>,
}
```

- [ ] **Step 3.3: Update `RequiresEntry` (validated form)**

Find:

```rust
/// Validated `requires` sub-table.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct RequiresEntry {
    /// Tool package names (advisory at v0.1).
    pub tools: Vec<String>,
}
```

Replace with:

```rust
/// Validated `requires` sub-table.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct RequiresEntry {
    /// Required tool packages with explicit source + optional version
    /// constraint.
    pub tools: Vec<tau_pkg::RequiredTool>,
}
```

(`tau_pkg::RequiredTool` was added in Task 2 and re-exported from `tau-pkg/src/lib.rs`.)

- [ ] **Step 3.4: Add the new error variant to `ProjectConfigError`**

Find the `ProjectConfigError` enum (around line 138-200). Add this variant before the closing brace:

```rust
    /// Bare-string entry in `[agents.<id>.requires.tools]` is no longer
    /// supported. Each entry must use the struct form with a `source`
    /// declaration. Tier 2 priority 5 closed the v0.1 advisory-only
    /// behavior.
    #[error(
        "agent {agent_id:?}: requires.tools[{index}]: bare-string {value:?} no longer supported; use struct form with `source` per spec docs/superpowers/specs/2026-04-30-transitive-deps-design.md §4"
    )]
    RequiresToolsBareStringRejected {
        /// Agent id whose entry was rejected.
        agent_id: String,
        /// Index in the tools array of the offending entry.
        index: usize,
        /// The bare string value as it appeared in the TOML.
        value: String,
    },
```

The bare-string error fires at deserialization time via serde — TOML will fail with a custom error mentioning the value. We surface it as `ProjectConfigError::RequiresToolsBareStringRejected` by mapping the underlying `toml::de::Error` in the `Parse` arm. **However**, given that `UncheckedRequiredTool` has `#[serde(deny_unknown_fields)]` and `name`/`source` are required, a bare string in the array will fail with a clear-enough toml::de error already. The `RequiresToolsBareStringRejected` variant is reserved for any future custom-deserializer path; for v0.1 the toml::de error suffices. Keep the variant defined (additive non-breaking) but don't custom-derive a `Deserialize` for `UncheckedRequiredTool` — the default error is good enough.

- [ ] **Step 3.5: Update the validator**

Find the validator block around line 320:

```rust
    let requires = raw
        .requires
        .map_or(RequiresEntry::default(), |r| RequiresEntry {
            tools: r.tools,
            // r.packages ignored at v0.1
        });
```

Replace with:

```rust
    let requires = match raw.requires {
        None => RequiresEntry::default(),
        Some(r) => {
            let mut tools: Vec<tau_pkg::RequiredTool> = Vec::with_capacity(r.tools.len());
            for raw_tool in r.tools {
                let name = tau_domain::PackageName::from_str(&raw_tool.name).map_err(|e| {
                    ProjectConfigError::AgentValidation {
                        id: id.clone(),
                        message: format!("requires.tools entry {:?}: invalid name: {e}", raw_tool.name),
                    }
                })?;
                let version_req = match raw_tool.version.as_deref() {
                    None | Some("") => semver::VersionReq::STAR,
                    Some(s) => semver::VersionReq::parse(s).map_err(|e| {
                        ProjectConfigError::AgentValidation {
                            id: id.clone(),
                            message: format!(
                                "requires.tools entry {:?}: invalid version {s:?}: {e}",
                                raw_tool.name
                            ),
                        }
                    })?,
                };
                tools.push(tau_pkg::RequiredTool::new(name, raw_tool.source, version_req));
            }
            RequiresEntry { tools }
        }
    };
```

You may need to add `use std::str::FromStr;` and `use semver::VersionReq;` (the latter is re-exported as `semver::VersionReq` since `semver` is now a workspace dep) at the top of the file if not already present. Run `cargo build` after this step to surface missing imports.

- [ ] **Step 3.6: Update existing tests in `project.rs`**

Find existing tests that exercise the bare-string form (search for `tools = [` in the file's test module):

```bash
grep -n 'tools = \[' crates/tau-cli/src/config/project.rs
```

For each hit, update the TOML fixture from:
```toml
[agents.X.requires]
tools = ["fs-read"]
```

to the struct form:
```toml
[[agents.X.requires.tools]]
name = "fs-read"
source = "https://example.com/fs-read.git"
```

Add a new test verifying the bare-string rejection:

```rust
    #[test]
    fn validate_rejects_bare_string_tools_entry() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = "anthropic"

            [agents.r.requires]
            tools = ["fs-read"]
        "#;
        // serde rejects the bare string — toml::de error surfaces as Parse.
        let result: Result<UncheckedProjectConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err(), "bare-string tools entry must fail to deserialize");
    }

    #[test]
    fn validate_accepts_struct_tools_entry() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = "anthropic"

            [[agents.r.requires.tools]]
            name = "fs-read"
            source = "https://example.com/fs-read.git"
            version = "^0.1"
        "#;
        let cfg = parse(toml_str).unwrap();
        let agent = cfg.agents.get("r").unwrap();
        assert_eq!(agent.requires.tools.len(), 1);
        assert_eq!(agent.requires.tools[0].name.as_str(), "fs-read");
        assert_eq!(agent.requires.tools[0].version_req.to_string(), "^0.1");
    }

    #[test]
    fn validate_default_version_is_star() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = "anthropic"

            [[agents.r.requires.tools]]
            name = "fs-read"
            source = "https://example.com/fs-read.git"
        "#;
        let cfg = parse(toml_str).unwrap();
        let agent = cfg.agents.get("r").unwrap();
        assert_eq!(agent.requires.tools[0].version_req.to_string(), "*");
    }
```

- [ ] **Step 3.7: Add `tau-pkg` dep to `tau-cli`'s Cargo.toml if not present**

```bash
grep "^tau-pkg" crates/tau-cli/Cargo.toml
```

Should already match — tau-cli already uses `tau-pkg` for `Scope`/`install`/`registry`. If absent, add `tau-pkg = { workspace = true }` to `[dependencies]`.

- [ ] **Step 3.8: Find and adapt other AgentEntry/RequiresEntry struct-literal sites**

```bash
grep -rn "RequiresEntry {" crates/tau-cli/ --include="*.rs"
```

For each site that constructs `RequiresEntry { tools: vec![...] }` with `Vec<String>`, adapt to `Vec<RequiredTool>`. The most common case will be test helpers — wait until Task 5 to update those (the test scaffold has its own dedicated task). For non-test sites in production code, the only one will be `Default::default()` invocations — already returning empty vec, no change needed.

- [ ] **Step 3.9: Verify**

```bash
cargo build --workspace
cargo test -p tau-cli --all-targets config::project
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-cli --doc
```

Expected:
- `cargo build --workspace` may FAIL with errors at sites that haven't been adapted yet (e.g., `tests/common/mod.rs` test helper, `cmd/run.rs` Step 5 verify). These will be fixed in Tasks 4-5. For Task 3, the goal is `cargo test -p tau-cli --lib config::project` PASS — that's the unit-test scope this task covers.
- If `cargo build --workspace` fails on test code in `agent.rs` Step 5 (which is being replaced in Task 4), that's expected. Task 3 commits are allowed to leave `cargo build --workspace` red, AS LONG AS Task 4 lands the fix in the very next commit. The CI on the branch will be red between Task 3's push and Task 4's push — this is acceptable for in-tree development.
- **Alternatively (preferred)**: combine Task 3 with Task 4 into a single commit if the engineer prefers a single green commit. This is acceptable; just adjust commit messages.

For this plan, treat Tasks 3 + 4 as **two commits, but the branch may be red between them**. Document this in the Task 3 commit message.

- [ ] **Step 3.10: Commit**

```bash
git add crates/tau-cli/src/config/project.rs
git commit -m "$(cat <<'EOF'
feat(cli): typed [[agents.<id>.requires.tools]] schema

Replaces the v0.1 advisory-only Vec<String> schema with
Vec<UncheckedRequiredTool> { name, source, version }. Each entry must
declare a source (typed PackageSource — string serde format reused
verbatim from package manifests). The optional version field parses
to semver::VersionReq, defaulting to "*" when absent.

Bare strings are rejected at deserialization (serde's deny_unknown_fields
+ required name/source surface a clear toml::de error). The matching
ProjectConfigError::RequiresToolsBareStringRejected variant is added
for future custom-deserializer use.

NOTE: This commit may leave the workspace red on `cargo build`. The
agent_def Step 5 verify call site in `config/agent.rs` and the test
scaffold in `tests/common/mod.rs` adapt in Tasks 4-5. The fix lands
in the very next commit; CI on the branch will go green again then.

Refs: docs/superpowers/specs/2026-04-30-transitive-deps-design.md §4, §8

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 3.11: Push**

```bash
git push
```

---

## Task 4: tau-cli `config::agent` Step 5 deletion + plugin_loader adapt

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/config/agent.rs` — DELETE Step 5 verify block; REMOVE `RequiredToolMissing` variant.
- Modify: `crates/tau-cli/src/cmd/plugin_loader.rs` — adapt to `RequiredTool` struct (use `&tool.name` instead of `tool_name`).

**Spec sections:** §7.1.

**Architectural note:** the resolve+install does NOT live in `build_agent_definition`. It lives in the per-CLI-command call sites (Tasks 6/7/8) which run BEFORE `build_agent_definition` is called. By the time `build_agent_definition` runs, the lockfile is already up to date — so the v0.1 Step 5 verify block is redundant and can be deleted entirely. This avoids duplicating the resolve+install logic across `build_agent_definition` AND `cmd/run.rs`/`cmd/chat.rs`/`cmd/resolve.rs`.

**Per-task summary:**

1. **Delete Step 5 block** at `crates/tau-cli/src/config/agent.rs:240-254` (the loop that iterates `entry.requires.tools` as `Vec<String>` and emits `RequiredToolMissing`). Plus the comment lines at `agent.rs:147` ("Step 5: verify each `entry.requires.tools` entry is installed."). After deletion, the function flows from Step 4 (verify llm_backend installed) directly to Step 6 (system prompt). Renumber the surviving steps in the doc comment.

2. **Remove the `RequiredToolMissing` variant** from `AgentResolutionError` (around `agent.rs:71-75`). In-tree breaking-but-non-breaking-cross-crate (`#[non_exhaustive]`). No replacement variant — the error never fires in the new flow because the lazy resolve in cmd/run.rs (Task 6) does the actual work, and any failure is `anyhow::Result`-bubbled there.

3. **Update tests in `agent.rs`** that match on `RequiredToolMissing`. The test at `agent.rs:633-642` (`build_agent_definition_returns_required_tool_missing`) should be **deleted** — the error variant it asserts on no longer exists. The behavior it tested ("required tool not installed → error") is now tested at the `cmd/run.rs` integration-test level in Task 6. Replace with a passing test that the new flow simply IGNORES the requires.tools list at this layer (now a no-op):

   ```rust
   #[test]
   fn build_agent_definition_ignores_requires_tools_list() {
       // Step 5 verify was removed in Tier 2 priority 5 — the resolve+install
       // happens at cmd/run.rs / cmd/chat.rs / cmd/resolve.rs before
       // build_agent_definition is called. By the time we reach this code path,
       // the lockfile is authoritative; we no longer iterate requires.tools here.
       // Verify a non-empty list doesn't trip any code path.
       let entry = entry(|e| {
           e.requires.tools = vec![make_required_tool(
               "fs-read",
               "https://example.com/fs-read.git",
               "^0.1",
           )];
       });
       // The test fixture's package isn't installed in the temp scope, but
       // the new flow doesn't check requires.tools at this layer — it should
       // succeed so long as the agent's package itself is installed.
       // ... (existing helper builds a passing fixture; adjust as needed)
   }
   ```

   The actual test body depends on the existing helpers in `agent.rs::tests`. Look at the existing `build_agent_definition_resolves_package_at_correct_version` test (around line 522) for the helper pattern — it sets up an installed agent package via a tempdir lockfile fixture. Reuse the same harness, populate the test's `requires.tools` with one entry, and assert the function returns `Ok` (i.e., the requires.tools list is now ignored at this layer).

4. **Update `crates/tau-cli/src/cmd/plugin_loader.rs:159`**: change the loop variable. Before:
   ```rust
   for tool_name in &entry.requires.tools {
       // ... uses tool_name as &String
   }
   ```
   After:
   ```rust
   for tool in &entry.requires.tools {
       let tool_name = tool.name.as_str();
       // ... rest unchanged, references tool_name as &str
   }
   ```
   (The body of the loop probably uses `tool_name` to look up an installed package by name; it's already idiomatic-`&str`-friendly. The change is one line at the top of the loop.)

5. **Verification:**
   ```bash
   cargo build --workspace
   cargo test -p tau-cli --all-targets config::agent
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test -p tau-cli --doc
   ```
   Expected: build PASS now (Task 3's red state resolves). Some existing tests using bare-string `e.requires.tools = vec!["..."]` fixtures may still fail — Task 5 finishes the cleanup. The two `agent.rs::tests` updated in this task should pass; the broader workspace test rollup is Task 5's responsibility.

6. **Commit message:**
   ```
   refactor(cli): delete build_agent_definition Step 5 verify; remove RequiredToolMissing

   The resolve+install for requires.tools now lives in the per-CLI-command
   call sites (Tasks 6/7/8: cmd/run.rs, cmd/chat.rs, cmd/resolve.rs).
   By the time build_agent_definition runs, the lockfile is authoritative;
   the v0.1 verify block at this layer is redundant.

   Removes:
     - the Step 5 loop at config/agent.rs:240-254
     - the AgentResolutionError::RequiredToolMissing variant
     - the build_agent_definition_returns_required_tool_missing test

   In-tree breaking removal of a #[non_exhaustive] enum variant; only this
   crate's tests referenced it. Same precedent as Tier 2 priority 4's
   removal of CapabilityOverrideUnsupported.

   plugin_loader.rs adapts to the new struct shape on
   entry.requires.tools (one-line `for tool in &entry.requires.tools`
   loop variable change; tool.name.as_str() bound at top).

   Refs: docs/superpowers/specs/2026-04-30-transitive-deps-design.md §7.1
   ```

7. Push.

---

## Task 5: test scaffold update

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/tests/common/mod.rs` — helper takes `Vec<RequiredTool>`-like fixture.
- Modify: All test fixtures across `crates/tau-cli/` that use the helper or build `e.requires.tools = vec![...]` directly. Likely sites: `crates/tau-cli/src/config/agent.rs` (test module at the bottom), some integration test files in `crates/tau-cli/tests/`.

**Spec sections:** §10.

**Per-task summary:**

1. The helper at `crates/tau-cli/tests/common/mod.rs:408` currently builds the requires block from `Vec<String>`. Find it:

   ```bash
   grep -n "agents.{agent_id}.requires" crates/tau-cli/tests/common/mod.rs
   ```

   Replace its signature to take `Vec<(name, source, version_opt)>` (simple tuple — keeps the call site compact) and emit array-of-tables TOML:

   ```rust
   /// Build a `[agents.<id>.requires.tools]` block from a list of
   /// (name, source-url, optional-version-req) triples.
   ///
   /// Test fixtures use file:// URLs to local git repos; see
   /// crates/tau-pkg/src/source_list.rs::tests for the make_local_git_fixture
   /// pattern.
   pub fn requires_block(agent_id: &str, tools: &[(&str, &str, Option<&str>)]) -> String {
       if tools.is_empty() {
           return String::new();
       }
       let mut out = String::new();
       for (name, source, ver) in tools {
           out.push_str(&format!(
               "\n[[agents.{agent_id}.requires.tools]]\nname = \"{name}\"\nsource = \"{source}\"\n"
           ));
           if let Some(v) = ver {
               out.push_str(&format!("version = \"{v}\"\n"));
           }
       }
       out
   }
   ```

2. Update every call site that previously passed `Vec<String>`. Search:
   ```bash
   grep -rn "requires_block\|requires.tools = vec!" crates/tau-cli/ --include="*.rs"
   ```
   For each hit, adapt to either:
   - `requires_block(agent_id, &[("fs-read", "https://example.com/fs-read.git", Some("^0.1"))])` for TOML emission, OR
   - `e.requires.tools = vec![tau_pkg::RequiredTool::new(name, source, version_req)]` for direct struct fixtures.

3. The test in `agent.rs:633` (`let entry = entry(|e| e.requires.tools = vec!["fs-read".into()]);`) was deleted in Task 4. Add a `make_required_tool` helper for any other test or fixture in the file that needs it:

   ```rust
   fn make_required_tool(name: &str, source_url: &str, version: &str) -> tau_pkg::RequiredTool {
       use std::str::FromStr;
       tau_pkg::RequiredTool::new(
           tau_domain::PackageName::from_str(name).unwrap(),
           tau_domain::PackageSource::from_str(source_url).unwrap(),
           semver::VersionReq::parse(version).unwrap(),
       )
   }
   ```

   Use it in the new `build_agent_definition_ignores_requires_tools_list` test (added in Task 4) and any other site that needs to construct a fixture `RequiredTool`.

4. **Verification:**
   ```bash
   cargo build --workspace
   cargo test --workspace --all-targets
   cargo test --workspace --doc
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```
   Expected: ALL PASS. The branch goes green.

5. **Commit message:**
   ```
   test(cli): adapt test scaffold + fixtures to typed RequiredTool

   - tests/common/mod.rs: requires_block helper now emits array-of-tables
     TOML from (name, source, version) triples instead of bare-string
     Vec<String>.
   - config/agent.rs tests: existing fixtures and the
     RequiredToolMissing-shaped test rebuild via make_required_tool
     helper. The renamed test asserts ResolveFailed when the source URL
     is unreachable (git ls-remote fails).
   - All other call sites that constructed e.requires.tools directly
     adapted to the new RequiredTool struct shape.

   Workspace tests green again; closes Task 3 + Task 4's intentional red
   window.

   Refs: docs/superpowers/specs/2026-04-30-transitive-deps-design.md §10
   ```

6. Push.

---

## Task 6: tau-cli `cmd/run.rs` lazy resolve + `--no-install` + npm-style progress

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` — add `--no-install` to `RunArgs`.
- Modify: `crates/tau-cli/src/cmd/run.rs` — emit npm-style progress before `build_agent_definition`; `--no-install` short-circuits with hint.
- Modify: `crates/tau-cli/tests/cmd_run.rs` — add 2 tests.

**Spec sections:** §7.1, §7.3, §7.4.

**Per-task summary:**

1. Add to `RunArgs` (in `crates/tau-cli/src/cli.rs:188-199`):
   ```rust
       /// Skip auto-install of missing requires.tools dependencies. If
       /// anything would need fetching, exit 2 with copy-pasteable
       /// `tau install <url>` hints instead.
       #[arg(long)]
       pub no_install: bool,
   ```

2. In `crates/tau-cli/src/cmd/run.rs`, before the call to `build_agent_definition` (around line 75), insert the resolve step:

   ```rust
       // Build the (AgentId, RequiredTool) list for THIS agent only.
       let requires: Vec<(tau_domain::AgentId, tau_pkg::RequiredTool)> = entry
           .requires
           .tools
           .iter()
           .map(|t| {
               (
                   tau_domain::AgentId::from_str(&entry.id).expect("AgentId from validated entry"),
                   t.clone(),
               )
           })
           .collect();
       let plan = tau_pkg::resolve_requires_tools(&requires, &scope)
           .with_context(|| format!("resolving requires.tools for agent {:?}", args.agent_id))?;

       // Emit npm-style progress.
       output.status(format!(
           "[resolve] {} required tools — {} already installed, {} to fetch",
           plan.installs.len() + plan.reuses.len(),
           plan.reuses.len(),
           plan.installs.len(),
       ))?;
       output.json(&serde_json::json!({
           "event": "resolve_start",
           "required": plan.installs.len() + plan.reuses.len(),
           "installed": plan.reuses.len(),
           "to_fetch": plan.installs.len(),
       }))?;

       if args.no_install && !plan.installs.is_empty() {
           emit_no_install_hints(&plan, output)?;
           anyhow::bail!("--no-install set; {} tool(s) missing", plan.installs.len());
       }

       let resolve_start = std::time::Instant::now();
       for install in &plan.installs {
           output.status(format!(
               "[install] {} {} from {}",
               install.name, install.version, install.source
           ))?;
           output.json(&serde_json::json!({
               "event": "install_start",
               "name": install.name.as_str(),
               "version": install.version.to_string(),
               "source": install.source.to_string(),
           }))?;
           let started = std::time::Instant::now();
           let _installed = tau_pkg::install_with_options(
               install.source.clone(),
               &scope,
               tau_pkg::InstallOptions::default(),
           )
           .with_context(|| format!("installing {}", install.name))?;
           let elapsed = started.elapsed().as_millis();
           output.status(format!(
               "[install] {} {} ({}ms)",
               install.name, install.version, elapsed
           ))?;
           output.json(&serde_json::json!({
               "event": "install_complete",
               "name": install.name.as_str(),
               "version": install.version.to_string(),
               "duration_ms": elapsed as u64,
           }))?;
       }
       let total_ms = resolve_start.elapsed().as_millis();
       if !plan.installs.is_empty() {
           output.status(format!("[resolve] done in {}ms", total_ms))?;
           output.json(&serde_json::json!({
               "event": "resolve_complete",
               "duration_ms": total_ms as u64,
           }))?;
       }
   ```

3. Add the helper `emit_no_install_hints` to the same file (private fn):
   ```rust
   fn emit_no_install_hints(
       plan: &tau_pkg::ResolutionPlan,
       output: &mut crate::output::Output,
   ) -> anyhow::Result<()> {
       output.warn(format!(
           "tau: {} tool(s) missing; --no-install set. To install:",
           plan.installs.len(),
       ))?;
       for install in &plan.installs {
           output.warn(format!("  tau install {}", install.source))?;
       }
       Ok(())
   }
   ```
   Pre-task: verify `Output::warn` accepts an `impl Display`. From earlier read it does. Good.

4. Note: `build_agent_definition` already runs its own resolve+install in Task 4. The lazy path here in `cmd/run.rs` is a duplicated pre-flight that surfaces progress to the user BEFORE build_agent_definition runs. To avoid double-resolve and double-install, **simplify Task 4's design**: `build_agent_definition` should NOT do the resolve+install — that responsibility moves to `cmd/run.rs` (Task 6) and `cmd/chat.rs` (Task 7) and `cmd/resolve.rs` (Task 8). `build_agent_definition` keeps a simpler responsibility: verify everything is installed (the lockfile lookup) and bail with a clean error if not.

   **Plan-erratum amendment:** Task 4's behavior shifts. Instead of running resolve+install, it does a lockfile-membership check (drop `RequiredToolMissing`, add a simpler variant named `RequiredToolNotInstalled` that's now an internal-state-bug error if the resolve was supposed to have installed it). Or, simpler still: remove the verify entirely from `build_agent_definition`, since by the time we call it the lazy path has already installed everything. The verification at `build_agent_definition` becomes optional.

   **Adjusted plan:** Task 4 deletes Step 5 entirely (no replacement). The resolve+install lives in Task 6 (run.rs), Task 7 (chat.rs), and Task 8 (resolve.rs) — each call site explicitly resolves before building the agent definition. `build_agent_definition` keeps Steps 1-4 and 6+ but loses Step 5. `RequiredToolMissing` is removed; no replacement variant added.

   **Revise Task 4** accordingly. Update its commit message + steps.

5. Add 2 integration tests in `crates/tau-cli/tests/cmd_run.rs`:

   - **`run_with_no_install_emits_install_hints_and_fails`**: Build a project tau.toml with one `requires.tools` entry pointing at `file:///tmp/nonexistent-fixture/missing.git`. Run `tau run reviewer "test prompt" --no-install`. Assert exit code != 0 (probably 1 from anyhow::bail) and stderr contains `"tau install file:///"`.
   - **`run_lazy_resolve_installs_missing_dep`**: Build a project tau.toml + a local git fixture (file:// URL) for the required tool. Run `tau run reviewer "test prompt" --dry-run`. Assert exit code 0 and stderr contains `"[install]"` with the tool name. Use `--dry-run` to skip the actual LLM call (which we don't have configured in the test).

   Note: actually invoking `tau run` end-to-end is brittle without a real LLM. `--dry-run` validates everything (including resolve+install) without the LLM. Use it.

6. **Verification:** standard 5-command suite + workspace test rollup.

7. **Commit message:**
   ```
   feat(cli): tau run lazy resolve + --no-install + npm-style progress

   Before invoking the LLM, tau run resolves the agent's requires.tools
   via tau_pkg::resolve_requires_tools, prints npm-style progress (one
   line per phase: resolve start, install start/complete per package,
   resolve done), and installs missing dependencies via
   tau_pkg::install_with_options.

   --no-install short-circuits: if any tool needs fetching, print
   copy-pasteable `tau install <url>` hints to stderr and exit 2.

   --json mode emits structured per-line events
   (resolve_start, install_start, install_complete, resolve_complete).

   Refs: docs/superpowers/specs/2026-04-30-transitive-deps-design.md §7.1, §7.3, §7.4
   ```

8. Push.

---

## Task 7: tau-cli `cmd/chat.rs` lazy resolve

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` — add `--no-install` to `ChatArgs`.
- Modify: `crates/tau-cli/src/cmd/chat.rs` — mirror of Task 6.
- Modify: `crates/tau-cli/tests/cmd_chat.rs` — 1 test for `--no-install`.

**Spec sections:** §7.1, §7.3, §7.4.

**Per-task summary:**

1. Mirror Task 6's RunArgs change to `ChatArgs`: add `--no-install` field with the same doc.

2. Mirror Task 6's run.rs body changes in chat.rs: insert the same resolve-start/install-loop/resolve-complete block before whatever currently builds the agent definition (around line 130-140 of chat.rs). Same `emit_no_install_hints` helper (or factor into a shared private fn — see Task 8).

3. **Refactor opportunity:** the resolve+install loop is now duplicated between run.rs and chat.rs. Extract to a shared helper at `crates/tau-cli/src/cmd/resolve_helpers.rs` (new private module) with one entry point: `pub(crate) fn resolve_and_install(entry, scope, no_install, output) -> anyhow::Result<()>`. Both run.rs and chat.rs call this. Task 8's resolve subcommand can also call a variant of this (multi-agent).

4. Add 1 integration test in `crates/tau-cli/tests/cmd_chat.rs`:
   - **`chat_with_no_install_fails_when_deps_missing`**: similar shape to Task 6's `--no-install` test but for `tau chat reviewer --dry-run --no-install`.

5. **Verification:** standard 5-command suite.

6. **Commit message:**
   ```
   feat(cli): tau chat lazy resolve + --no-install (mirror of run)

   ChatArgs gains --no-install. cmd/chat.rs runs the same resolve+install
   pre-flight as cmd/run.rs (Task 6) before entering the REPL. The shared
   logic is extracted into cmd::resolve_helpers for reuse by both run +
   chat (and Task 8's `tau resolve` subcommand).

   Refs: docs/superpowers/specs/2026-04-30-transitive-deps-design.md §7.1
   ```

7. Push.

---

## Task 8: tau-cli `cmd/resolve.rs` new subcommand

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` — add `Command::Resolve(ResolveArgs)` variant + `ResolveArgs` struct.
- Modify: `crates/tau-cli/src/main.rs` — dispatch `Command::Resolve` to `cmd::resolve::run`.
- Modify: `crates/tau-cli/src/cmd/mod.rs` — register `pub mod resolve;`.
- Create: `crates/tau-cli/src/cmd/resolve.rs` — full subcommand body. ~120 LOC.
- Create: `crates/tau-cli/tests/cmd_resolve.rs` — 5 assert_cmd integration tests.
- Create: `crates/tau-cli/tests/snapshots/help_snapshots__resolve_help.snap` — insta snapshot for `tau resolve --help`.
- Modify: `crates/tau-cli/tests/help_snapshots.rs` — add a new snapshot test for `tau resolve --help`.

**Spec sections:** §7.2, §7.3, §7.4.

**Per-task summary:**

1. Add to `Command` enum in `cli.rs`:
   ```rust
       /// Install missing requires.tools dependencies for all agents.
       Resolve(ResolveArgs),
   ```

2. Add `ResolveArgs` struct (after `ListArgs`):
   ```rust
   /// Arguments for `tau resolve`.
   #[derive(Args, Debug)]
   pub struct ResolveArgs {
       /// Skip install; print missing-deps hints and exit 2 if anything missing.
       #[arg(long)]
       pub no_install: bool,
       /// Print the resolution plan without fetching anything.
       #[arg(long)]
       pub dry_run: bool,
   }
   ```

3. Create `crates/tau-cli/src/cmd/resolve.rs`:
   ```rust
   //! `tau resolve` — install missing requires.tools dependencies for
   //! all agents in the project tau.toml. CI cache warm-up, pre-flight
   //! validation, and "fix my deps now" workflows.
   //!
   //! Lazy `tau run` / `tau chat` perform the same resolve per-agent at
   //! invocation time; this verb is the "all agents at once" form for
   //! workflows that don't run an agent.
   //!
   //! See `docs/superpowers/specs/2026-04-30-transitive-deps-design.md` §7.2.

   use std::str::FromStr;

   use anyhow::Context as _;

   use crate::cli::ResolveArgs;
   use crate::config::{ProjectConfig, ProjectConfigError};
   use crate::output::Output;
   use crate::cmd::resolve_helpers;

   /// Run `tau resolve`.
   pub async fn run(args: &ResolveArgs, output: &mut Output) -> anyhow::Result<()> {
       let cwd = std::env::current_dir()?;
       let path = cwd.join("tau.toml");
       let config = match ProjectConfig::from_path(&path) {
           Ok(cfg) => cfg,
           Err(ProjectConfigError::NotFound) => {
               anyhow::bail!("no project tau.toml found at {path:?}; run `tau init` to create one");
           }
           Err(e) => return Err(e.into()),
       };

       let scope = tau_pkg::Scope::resolve(&cwd).context("resolving package scope")?;

       // Flatten ALL agents' requires.tools into one (AgentId, RequiredTool) list.
       let mut requires: Vec<(tau_domain::AgentId, tau_pkg::RequiredTool)> = Vec::new();
       for agent in config.agents.values() {
           let agent_id = tau_domain::AgentId::from_str(&agent.id).expect("validated AgentId");
           for tool in &agent.requires.tools {
               requires.push((agent_id.clone(), tool.clone()));
           }
       }

       let plan = tau_pkg::resolve_requires_tools(&requires, &scope)
           .context("resolving requires.tools for project")?;

       // Print plan summary.
       output.status(format!(
           "[resolve] {} required tools — {} already installed, {} to fetch",
           plan.installs.len() + plan.reuses.len(),
           plan.reuses.len(),
           plan.installs.len(),
       ))?;
       output.json(&serde_json::json!({
           "event": "resolve_start",
           "required": plan.installs.len() + plan.reuses.len(),
           "installed": plan.reuses.len(),
           "to_fetch": plan.installs.len(),
       }))?;

       if args.dry_run {
           // Print plan, exit 0 without fetching.
           for install in &plan.installs {
               output.status(format!(
                   "[plan] {} {} from {} (would install)",
                   install.name, install.version, install.source
               ))?;
           }
           return Ok(());
       }

       if args.no_install && !plan.installs.is_empty() {
           resolve_helpers::emit_no_install_hints(&plan, output)?;
           anyhow::bail!("--no-install set; {} tool(s) missing", plan.installs.len());
       }

       resolve_helpers::install_planned(&plan, &scope, output).await?;
       Ok(())
   }
   ```

4. Implement `resolve_helpers` (introduced in Task 7) with the two pub(crate) fns. The full body (lifted from Task 6 run.rs):

   ```rust
   //! Shared helpers for resolve-then-install flows used by
   //! `tau run`, `tau chat`, and `tau resolve`.

   pub(crate) fn emit_no_install_hints(
       plan: &tau_pkg::ResolutionPlan,
       output: &mut crate::output::Output,
   ) -> anyhow::Result<()> {
       output.warn(format!(
           "tau: {} tool(s) missing; --no-install set. To install:",
           plan.installs.len(),
       ))?;
       for install in &plan.installs {
           output.warn(format!("  tau install {}", install.source))?;
       }
       Ok(())
   }

   pub(crate) async fn install_planned(
       plan: &tau_pkg::ResolutionPlan,
       scope: &tau_pkg::Scope,
       output: &mut crate::output::Output,
   ) -> anyhow::Result<()> {
       let resolve_start = std::time::Instant::now();
       for install in &plan.installs {
           output.status(format!(
               "[install] {} {} from {}",
               install.name, install.version, install.source
           ))?;
           output.json(&serde_json::json!({
               "event": "install_start",
               "name": install.name.as_str(),
               "version": install.version.to_string(),
               "source": install.source.to_string(),
           }))?;
           let started = std::time::Instant::now();
           let _installed = tau_pkg::install_with_options(
               install.source.clone(),
               scope,
               tau_pkg::InstallOptions::default(),
           )?;
           let elapsed = started.elapsed().as_millis();
           output.status(format!(
               "[install] {} {} ({}ms)",
               install.name, install.version, elapsed
           ))?;
           output.json(&serde_json::json!({
               "event": "install_complete",
               "name": install.name.as_str(),
               "version": install.version.to_string(),
               "duration_ms": elapsed as u64,
           }))?;
       }
       let total_ms = resolve_start.elapsed().as_millis();
       if !plan.installs.is_empty() {
           output.status(format!("[resolve] done in {}ms", total_ms))?;
           output.json(&serde_json::json!({
               "event": "resolve_complete",
               "duration_ms": total_ms as u64,
           }))?;
       }
       Ok(())
   }
   ```

5. Update `cmd/mod.rs`:
   ```rust
   pub mod resolve;
   pub(crate) mod resolve_helpers;
   ```

6. Update `main.rs` dispatcher: add `Command::Resolve(args) => cmd::resolve::run(&args, &mut output).await?,`.

7. Add 5 assert_cmd integration tests in `crates/tau-cli/tests/cmd_resolve.rs`. Use the file:// fixture pattern from Task 1 + Task 2 to set up a local git repo, then drive `tau resolve` against a project tau.toml that points at it.

   Test names:
   - `resolve_with_no_project_fails_with_init_hint`
   - `resolve_dry_run_prints_plan_without_fetching`
   - `resolve_no_install_hints_when_deps_missing`
   - `resolve_full_install_path_succeeds_against_local_fixture`
   - `resolve_idempotent_on_already_installed_deps`

8. Add an insta snapshot test for `tau resolve --help`. Mirror the existing snapshot tests in `help_snapshots.rs` (one entry per subcommand). Run once with `INSTA_UPDATE=auto` to populate the .snap file, then commit.

9. **Verification:**
   ```bash
   cargo build --workspace
   cargo test --workspace --all-targets
   cargo test --workspace --doc
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```

10. **Commit message:**
    ```
    feat(cli): tau resolve subcommand for project-wide requires.tools install

    Adds `tau resolve` — install missing requires.tools dependencies for
    ALL agents in the project tau.toml. Lazy run/chat already do this
    per-agent at invocation; this verb is the project-wide form for CI
    cache warm-up, pre-flight validation, and "fix my deps now" workflows.

    Flags: --dry-run (print plan without fetching), --no-install (print
    install hints and exit 2 if anything missing), --json (per-line
    structured event stream).

    Idempotent: re-running on a fully-resolved project is a no-op
    (lockfile reuse short-circuits each name).

    Shared resolve+install helpers live at crates/tau-cli/src/cmd/
    resolve_helpers.rs and are reused by run.rs, chat.rs, resolve.rs.

    5 assert_cmd integration tests via file:// git fixtures (no real
    network in CI).

    Refs: docs/superpowers/specs/2026-04-30-transitive-deps-design.md §7.2
    ```

11. Push.

---

## Task 9: Final verification + open PR

**User-driven gate. PAUSE before this task.**

### Steps

- [ ] **Step 9.1: Full local verification**

```bash
cargo build --workspace
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

All must pass. If anything fails, fix it before opening the PR.

- [ ] **Step 9.2: Open the PR (or mark draft → ready)**

```bash
gh pr list --head feat/transitive-deps-spec --json number,state,isDraft
```

If empty, create:

```bash
gh pr create --title "feat: transitive dependency resolution (Tier 2 priority 5)" \
  --body "$(cat <<'EOF'
## Summary

Implements project tau.toml `[agents.<id>.requires.tools]` auto-install, realizing ADR-0007 §5 reservation.

- New `tau-pkg::source_list` — enumerate versions at a `PackageSource` via `git ls-remote --tags` + `Git { rev: Some }` shallow read.
- New `tau-pkg::resolve` — three-phase resolver (group / conflict / pick) producing a `ResolutionPlan`.
- Schema change: `[[agents.<id>.requires.tools]]` becomes typed array-of-tables; bare strings rejected at parse.
- `tau run` / `tau chat` lazily resolve before LLM invocation; new `tau resolve` subcommand for project-wide install.
- npm-style progress output (one line per phase) via existing `Output` channel; `--no-install` opt-out emits copy-pasteable `tau install <url>` hints.
- New typed `tau_pkg::ResolveError`, `SourceListError`, `ProjectConfigError::RequiresToolsBareStringRejected`, `AgentResolutionError::ResolveFailed`/`InstallFailed`. `RequiredToolMissing` removed.
- Tests use `file://` URLs to local git fixtures (no real network in CI).

## Spec / Plan

- Spec: `docs/superpowers/specs/2026-04-30-transitive-deps-design.md`
- Plan: `docs/superpowers/plans/2026-04-30-transitive-deps.md`

## Test plan

- [x] `cargo test --workspace --all-targets` green
- [x] `cargo test --workspace --doc` green
- [x] `cargo clippy --workspace --all-targets -- -D warnings` green
- [x] `cargo fmt --all -- --check` green
- [ ] CI matrix (23 required checks) green — verifying on push

## Out of scope (deferred)

- Recursive package-level `dependencies` resolution (ADR-0004 §10 stays deferred).
- Registry source kind, hostname-glob resolution, mirror sources (Phase 2+).
- Concurrent fetch parallelism (sequential for typical 2-5 deps; perf work is Tier 4).
- Source-pin verification via lockfile sha256 (owned by Tier 2 priority 7 / `tau verify`).
- Live-network smoke test (manual smoke documented in this PR).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

If a draft already exists, mark ready: `gh pr ready <number>`.

- [ ] **Step 9.3: Capture PR URL**

```bash
gh pr view --json number,url --jq '{number, url}'
```

- [ ] **Step 9.4: PAUSE — wait for CI green before Task 10**

Use the same Bash + run_in_background poller pattern from priority 4's Task 10.

---

## Task 10: ADR-0007 §5 amendment + ROADMAP + squash merge

**User-driven gate. PAUSE before this task.**

**Files:**
- Modify: `docs/decisions/0007-tau-cli.md` — §5 amendment.
- Modify: `ROADMAP.md` — mark Tier 2 priority 5 ✅.

### Steps

- [ ] **Step 10.1: Amend ADR-0007 §5**

Find the current §5 body (around line 120-131 of `docs/decisions/0007-tau-cli.md`). Replace with:

```markdown
### 5. Per-agent `requires.tools` auto-install (Phase 1+ active)

Each `[[agents.<id>.requires.tools]]` array entry is a typed package
declaration with `name`, `source` (typed `PackageSource`), and
optional `version` (semver req, defaults to `"*"`). At `tau run` /
`tau chat` time (or on-demand via `tau resolve`), missing entries are
fetched + installed automatically; already-installed compatible
versions are reused via the existing per-scope lockfile.

Realized by the transitive-dependency-resolution sub-project (Tier 2
priority 5): see [`docs/superpowers/specs/2026-04-30-transitive-deps-design.md`](../superpowers/specs/2026-04-30-transitive-deps-design.md)
for the full design.

The original v0.1 reservation (when this section was titled "Per-agent
`requires.tools` advisory check") committed the auto-install path as
the Phase 1+ trigger so the v0.1 advisory error would convert into the
auto-install hook without breaking existing call sites.

Trigger to revisit: recursive package-level `dependencies` resolution
(ADR-0004 §10 deferral) — at which point the resolver gains a second
input axis; the project-level half (this section) stays unchanged.
```

- [ ] **Step 10.2: Update ROADMAP**

Replace the Tier 2 priority 5 entry in `ROADMAP.md` (around line 101-104):

```markdown
5. **Transitive dependency resolution** ✅ Shipped 2026-04-30 — see
   [spec](docs/superpowers/specs/2026-04-30-transitive-deps-design.md).
   Realizes ADR-0007 §5 reservation. Project tau.toml
   `[[agents.<id>.requires.tools]]` declares typed dependencies
   (name + source + optional version constraint); `tau run`/`tau chat`
   auto-install missing entries via lazy resolve; new `tau resolve`
   subcommand serves project-wide install. Cargo-style semver
   intersection across declarations of the same tool. One level deep:
   recursive package-level `dependencies` (ADR-0004 §10) stays
   deferred. No new CI jobs (23 required checks unchanged).
```

Add to the top-of-file shipped table:

```markdown
| 5 | Transitive dependency resolution ✅ | Tier 2 priority 5 — realizes ADR-0007 §5 reservation. New `tau-pkg::source_list` (git ls-remote tag enumeration + rev-pinned shallow read) and `tau-pkg::resolve` (three-phase resolver: group / conflict / pick highest-compatible). New `tau resolve` subcommand. Schema upgrade: `[[agents.<id>.requires.tools]]` typed entries with `name + source + version`; bare strings rejected at parse. Lazy resolve at `tau run`/`tau chat` with `--no-install` opt-out emitting copy-pasteable install hints. npm-style progress output (one line per phase, JSON event stream). New typed `ResolveError`, `SourceListError`, `RequiresToolsBareStringRejected`. Tests use `file://` git fixtures — no real network in CI. No new CI jobs (23 required checks unchanged). | 2026-04-30 |
```

Update the front-matter sentence about remaining Tier 2 work to drop priority 5.

- [ ] **Step 10.3: Commit + push**

```bash
git add docs/decisions/0007-tau-cli.md ROADMAP.md
git commit -m "$(cat <<'EOF'
docs: ADR-0007 §5 amendment + ROADMAP Tier 2 priority 5 done

Drops the "advisory only" qualifier on ADR-0007 §5 now that the
transitive-dependency-resolution sub-project has shipped. Renames
the section heading from "Per-agent requires.tools advisory check"
to "Per-agent requires.tools auto-install (Phase 1+ active)" and
links to the realizing spec. Preserves the original reservation
rationale as historical context.

Updates ROADMAP:
- Top-of-file "shipped" table gains a row for Tier 2 priority 5.
- Tier 2 priority 5 entry marked ✅ Shipped 2026-04-30 with key
  artifacts called out.
- Front-matter narrative updates to reflect that priority 5 is
  closed and remaining Tier 2 work is the next focus.

No new CI jobs in this sub-project; branch protection stays at 23
required checks.

Refs: docs/superpowers/specs/2026-04-30-transitive-deps-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push
```

- [ ] **Step 10.4: Wait for CI green on the PR**

Same poller pattern as priority 4. 23 required checks must all pass.

- [ ] **Step 10.5: Squash merge**

```bash
gh pr merge --squash --delete-branch
```

- [ ] **Step 10.6: Verify branch protection unchanged**

```bash
gh api repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks/contexts | jq 'length'
```
Expected: `23`.

- [ ] **Step 10.7: Sync local main + report squash SHA**

```bash
git checkout main && git pull && git log --oneline -3
```

Report back to the user with the squash SHA.

---

## Verification standard (per task)

Each task ends with:

```bash
cargo build --workspace
cargo test -p <crate> --all-targets
cargo test -p <crate> --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

For tasks touching multiple crates (4, 6, 7, 8, 9), run `cargo test --workspace --all-targets` instead.

CI continues on push; no new jobs added; branch protection stays at 23.
