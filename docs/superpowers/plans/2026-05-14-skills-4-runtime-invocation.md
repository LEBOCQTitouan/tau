# Skills-4 Runtime Invocation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `skill.<name>.spawn` runtime virtual tool end-to-end. Agents with `Capability::Skill(SkillCapability::Spawn { allowed_skills })` can spawn installed skills as child agents; the runtime resolves the skill, builds the child def from its declared system_prompt + capabilities (with `${SKILL_DIR}` substituted + optional caller `scope_paths` narrowing), and recursively invokes `run_with_history` via the v1.1 agent-spawn machinery.

**Architecture:** New `Capability::Skill` variant in tau-domain; new `find_installed_skill` helper in tau-pkg; new `skill_resolve` module + 5 OrchestrationError variants + `validate_skill_spawn` + `is_skill_spawn` branch in tau-runtime. Multi-turn `MockLlmBackend` test fixture lifted from existing `ScriptedLlm` pattern. Skills-4 reuses the v1.1 `Box::pin(child_runtime.run_with_history(...)).await` recursion mechanic from PR #60 — no new kernel infrastructure.

**Tech Stack:** Rust 2021. Existing deps only (sha2, globset, serde, serde_yaml, tokio, tracing). `tau-domain` Skills-1 types (`parse_skill_md`, `SkillManifest`); `tau-pkg` Skills-2 lockfile schema v5 (`LockedSkill`); `tau-runtime` v1.1 spawn recursion machinery.

**Branch:** `feat/skills-4-runtime-invocation` (worktree at `/Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl`, cut from main `f1dcf4d`).
**Spec:** `docs/superpowers/specs/2026-05-14-skills-4-runtime-invocation-design.md` (merged at PR #69).
**Depends on:** Skills-1 (`1d71032`) + Skills-2 (`93dbe95`) + Skills-3 (`7bec3ab`) + multi-agent v1.1 (`cb894cc`) + v1.2 (`fc4b1fe`).

**CLAUDE.md rules in effect:**
- Every cargo invocation: `timeout <secs> env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/<role> cargo <cmd> -p <crate>`.
- All git operations: `git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl ...`.
- Push via `git push --no-verify` (lefthook pre-existing flake; CI authoritative).
- `git commit --no-verify`.
- 4 sibling worktrees active for other Claude sessions — work ONLY in `feat-skills-4-impl`.

---

## File structure

| Path | Status | Responsibility |
|---|---|---|
| `crates/tau-domain/src/package/capability.rs` | Modify | Add `Capability::Skill(SkillCapability)` variant + `SkillCapability::Spawn { allowed_skills }`. Update `CapabilityShape` with `SkillSpawn` variant. Add `required_shape()` arm + serde de/ser branches. ~4 round-trip tests. |
| `crates/tau-pkg/src/skill_resolve.rs` | Create | `InstalledSkill` struct + `find_installed_skill(scope, name)` + `FindSkillError`. ~3 unit tests. |
| `crates/tau-pkg/src/lib.rs` | Modify | `pub mod skill_resolve;` + re-export. |
| `crates/tau-runtime/src/orchestration/error.rs` | Modify | 5 new variants: `SkillNotInstalled`, `SkillInstallPathMissing`, `SkillContentInvalid`, `SkillScopePathNotCovered`, `SkillSpawnNotAuthorized`. |
| `crates/tau-runtime/src/orchestration/skill_resolve.rs` | Create | `substitute_skill_dir`, `apply_scope_paths`, `resolve_skill_for_spawn`, `SkillSpawnRequest`. ~10 unit tests. |
| `crates/tau-runtime/src/orchestration/mod.rs` | Modify | `pub mod skill_resolve;` + re-exports. |
| `crates/tau-runtime/src/orchestration/virtual_tools.rs` | Modify | Extend `is_virtual` + `required_capability` for `skill.<name>.spawn`. Add `validate_skill_spawn`. |
| `crates/tau-runtime/src/stream.rs` | Modify | Add `is_skill_spawn` branch parallel to existing `is_agent_spawn` (starts at line 371). |
| `crates/tau-runtime/tests/common/mock_llm.rs` | Create | Multi-turn `MockLlmBackend` fixture (lifted from `ScriptedLlm` pattern in `tests/run_with_tool_calls.rs`). ~3 self-tests. |
| `crates/tau-runtime/tests/common/mod.rs` | Modify | `pub mod mock_llm;` re-export. |
| `crates/tau-runtime/tests/skill_spawn_e2e.rs` | Create | 6 end-to-end skill-spawn tests. |
| `crates/tau-cli/tests/cmd_orchestration.rs` | Modify | Un-`#[ignore]` 5 pattern test skeletons. Wire each via MockLlmBackend. |
| `docs/decisions/0028-skills-runtime-invocation.md` | Create | ADR documenting D1/D2/D3 + rejected alternatives. |

---

## Task 1: tau-domain — `Capability::Skill` variant

**Files:**
- Modify: `crates/tau-domain/src/package/capability.rs`

- [ ] **Step 1: Write the failing tests**

In `crates/tau-domain/src/package/capability.rs`, append to the existing `#[cfg(test)] mod tests` block:

```rust
    #[cfg(feature = "serde")]
    #[test]
    fn skill_spawn_capability_round_trips_through_json() {
        let cap = Capability::Skill(SkillCapability::Spawn {
            allowed_skills: vec!["critic".into(), "fact-checker".into()],
        });
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"skill.spawn","allowed_skills":["critic","fact-checker"]}"#
        );
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn skill_spawn_capability_empty_allowed_skills_round_trips() {
        let cap = Capability::Skill(SkillCapability::Spawn {
            allowed_skills: vec![],
        });
        let json = serde_json::to_string(&cap).unwrap();
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn skill_spawn_required_shape_is_skill_spawn() {
        let cap = Capability::Skill(SkillCapability::Spawn {
            allowed_skills: vec!["x".into()],
        });
        assert_eq!(cap.required_shape(), CapabilityShape::SkillSpawn);
    }
```

- [ ] **Step 2: Run tests, see them fail**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t1 cargo nextest run -p tau-domain --lib --features serde -E 'test(skill_spawn)' 2>&1 | tail -10
```

Expected: compile error (`SkillCapability` doesn't exist).

- [ ] **Step 3: Add the `SkillCapability` enum**

In `crates/tau-domain/src/package/capability.rs`, after the existing `AgentCapability` enum, add:

```rust
/// Skill capability verbs.
///
/// Added by Skills-4 (ROADMAP §16). Skills are an installable
/// package kind that ships a reusable agent behavior (SKILL.md
/// system_prompt + declared capabilities). The `Spawn` variant
/// authorizes a parent agent to invoke installed skills as child
/// agents via the `skill.<name>.spawn` virtual tool.
///
/// # Example
///
/// ```ignore
/// use tau_domain::SkillCapability;
/// let cap = SkillCapability::Spawn {
///     allowed_skills: vec!["critic".into(), "fact-checker".into()],
/// };
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SkillCapability {
    /// Spawn an installed skill as a child agent. `allowed_skills` is
    /// the list of skill names (matching `LockedPackage.name` for
    /// `kind = "skill"` entries in the lockfile) the parent agent
    /// may invoke via `skill.<name>.spawn`.
    #[non_exhaustive]
    Spawn {
        /// Permitted skill names.
        allowed_skills: Vec<String>,
    },
}
```

- [ ] **Step 4: Add `Capability::Skill` variant**

Find the `Capability` enum (line ~31). After the existing `Agent(AgentCapability)` variant, add:

```rust
    /// Skill-related capability (Skills-4).
    Skill(SkillCapability),
```

- [ ] **Step 5: Add `CapabilityShape::SkillSpawn` variant**

Find `pub enum CapabilityShape` (line ~164). After `AgentSpawn`, add:

```rust
    /// Plugin / agent needs to spawn an installed skill via the
    /// `skill.<name>.spawn` virtual tool. (Added by Skills-4.)
    SkillSpawn,
```

- [ ] **Step 6: Add `required_shape()` arm**

In `impl Capability { pub fn required_shape(&self) -> CapabilityShape { match self { ... } } }`, after the `Capability::Agent(AgentCapability::Spawn { .. }) => CapabilityShape::AgentSpawn,` arm, add:

```rust
            Capability::Skill(SkillCapability::Spawn { .. }) => CapabilityShape::SkillSpawn,
```

- [ ] **Step 7: Add serde de/ser branches**

In the `Deserialize for Capability` impl (`match raw.kind.as_str() { ... }` around line 279), add a `skill.spawn` arm next to `agent.spawn`:

```rust
                "skill.spawn" => Capability::Skill(SkillCapability::Spawn {
                    allowed_skills: raw.allowed_skills.unwrap_or_default(),
                }),
```

In the `Deserialize` struct `RawCapability`, find the existing `#[serde(default)] allowed_kinds: Option<Vec<String>>` field. Just after it, add the parallel:

```rust
        #[serde(default)]
        allowed_skills: Option<Vec<String>>,
```

In the `Serialize for Capability` impl (the `match self { ... }` around line 308), after the `Capability::Agent(AgentCapability::Spawn { allowed_kinds })` arm, add:

```rust
                Capability::Skill(SkillCapability::Spawn { allowed_skills }) => {
                    let mut m = s.serialize_map(Some(2))?;
                    m.serialize_entry("kind", "skill.spawn")?;
                    m.serialize_entry("allowed_skills", allowed_skills)?;
                    m.end()
                }
```

- [ ] **Step 8: Re-export `SkillCapability` from tau-domain lib**

In `crates/tau-domain/src/lib.rs`, find the existing re-export of `Capability` / `AgentCapability` (likely `pub use package::capability::{...};` or similar). Add `SkillCapability` to the list.

- [ ] **Step 9: Run tests, see them pass**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t1 cargo nextest run -p tau-domain --lib --features serde 2>&1 | tail -5
```

Expected: all 3 new tests pass + existing tests still pass.

- [ ] **Step 10: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl add crates/tau-domain
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl commit --no-verify -m "$(cat <<'EOF'
feat(domain): Capability::Skill variant + SkillCapability::Spawn

Skills-4 (ROADMAP §16) D1: separate capability for `skill.<name>.spawn`
virtual tool. Parallel to existing AgentCapability::Spawn but with
`allowed_skills: Vec<String>` matching installed skill names.

TOML form:
  [[capabilities]]
  kind = "skill.spawn"
  allowed_skills = ["critic", "fact-checker"]

CapabilityShape::SkillSpawn added for adapter cross-check. 3 round-trip
tests.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: tau-pkg — `find_installed_skill` helper

**Files:**
- Create: `crates/tau-pkg/src/skill_resolve.rs`
- Modify: `crates/tau-pkg/src/lib.rs`

- [ ] **Step 1: Create the module skeleton**

Create `crates/tau-pkg/src/skill_resolve.rs`:

```rust
//! Skill resolution: lookup an installed skill by name.
//!
//! Skills-4 (ROADMAP §16). Reads the scope lockfile (one file open
//! handled by `LockFile::load`) + the skill package's `tau.toml` (one
//! additional file open). Returns a fully-built `InstalledSkill` with
//! resolved install path + parsed manifest + cached frontmatter.

use std::path::PathBuf;

use tau_domain::{Capability, PackageManifest, PackageName, SkillManifest, Version};

use crate::lockfile::{LockFile, RegistryError, SkillFrontmatterSnapshot};
use crate::scope::Scope;

/// A fully-resolved installed skill, ready for runtime invocation.
#[derive(Debug, Clone)]
pub struct InstalledSkill {
    /// Package name (matches `tau.toml` `name` field).
    pub name: PackageName,
    /// Active version (from lockfile).
    pub version: Version,
    /// Absolute path to the installed package directory
    /// (`<scope>/.tau/packages/<name>/<version>/`).
    pub install_path: PathBuf,
    /// Parsed manifest from `<install_path>/tau.toml`.
    pub manifest: PackageManifest,
    /// Cached SKILL.md frontmatter snapshot (from lockfile).
    pub frontmatter: SkillFrontmatterSnapshot,
    /// Skill-specific manifest block (from manifest).
    pub skill: SkillManifest,
    /// Declared capabilities (from manifest).
    pub capabilities: Vec<Capability>,
}

/// Errors raised by [`find_installed_skill`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FindSkillError {
    /// Failed to load the scope lockfile.
    #[error("lockfile load: {0}")]
    Lockfile(#[from] RegistryError),
    /// Lockfile entry exists, but the install path is missing on disk.
    #[error("skill {name:?} lockfile entry points at {path:?} but no tau.toml found there")]
    InstallPathMissing {
        /// Skill name.
        name: String,
        /// The expected install path.
        path: PathBuf,
    },
    /// I/O error reading the manifest.
    #[error("reading manifest at {path:?}: {source}")]
    ReadManifest {
        /// The manifest path.
        path: PathBuf,
        /// Underlying io error.
        #[source]
        source: std::io::Error,
    },
    /// TOML parse failure on the manifest.
    #[error("parsing manifest at {path:?}: {detail}")]
    ParseManifest {
        /// The manifest path.
        path: PathBuf,
        /// Parser error detail.
        detail: String,
    },
    /// Manifest validation failure.
    #[error("validating manifest at {path:?}: {detail}")]
    ValidateManifest {
        /// The manifest path.
        path: PathBuf,
        /// Validation error detail.
        detail: String,
    },
    /// Manifest declared a kind other than "skill" but was found in a
    /// skill entry in the lockfile.
    #[error("manifest at {path:?} has kind != \"skill\" but is recorded as a skill in the lockfile")]
    NotASkillManifest {
        /// The manifest path.
        path: PathBuf,
    },
}

/// Resolve an installed skill by name. Returns `Ok(None)` if no
/// installed package matches `name` and has a skill block.
///
/// Reads the scope lockfile + one `tau.toml` for the matched skill.
pub fn find_installed_skill(
    scope: &Scope,
    name: &str,
) -> Result<Option<InstalledSkill>, FindSkillError> {
    let lockfile_path = scope.lockfile_path();
    if !lockfile_path.exists() {
        return Ok(None);
    }
    let lockfile = LockFile::load(&lockfile_path)?;

    let pkg = match lockfile
        .packages
        .iter()
        .find(|p| p.name.as_str() == name && p.skill.is_some())
    {
        Some(p) => p,
        None => return Ok(None),
    };

    let locked_skill = pkg
        .skill
        .as_ref()
        .expect("filtered to Some(skill) above");

    let install_path = scope
        .path()
        .join("packages")
        .join(pkg.name.as_str())
        .join(pkg.active_version.to_string());

    let toml_path = install_path.join("tau.toml");
    if !toml_path.exists() {
        return Err(FindSkillError::InstallPathMissing {
            name: name.to_string(),
            path: install_path,
        });
    }
    let toml_text = std::fs::read_to_string(&toml_path).map_err(|e| {
        FindSkillError::ReadManifest {
            path: toml_path.clone(),
            source: e,
        }
    })?;
    let unchecked: tau_domain::UncheckedManifest = toml::from_str(&toml_text).map_err(|e| {
        FindSkillError::ParseManifest {
            path: toml_path.clone(),
            detail: e.to_string(),
        }
    })?;
    let manifest = unchecked
        .validate()
        .map_err(|e| FindSkillError::ValidateManifest {
            path: toml_path.clone(),
            detail: e.to_string(),
        })?;

    let skill = match manifest.skill() {
        Some(s) => s.clone(),
        None => {
            return Err(FindSkillError::NotASkillManifest {
                path: toml_path,
            });
        }
    };

    let capabilities = manifest.capabilities().to_vec();

    Ok(Some(InstalledSkill {
        name: pkg.name.clone(),
        version: pkg.active_version.clone(),
        install_path,
        manifest,
        frontmatter: locked_skill.frontmatter.clone(),
        skill,
        capabilities,
    }))
}
```

- [ ] **Step 2: Wire into lib.rs**

In `crates/tau-pkg/src/lib.rs`, add:

```rust
pub mod skill_resolve;
pub use skill_resolve::{find_installed_skill, FindSkillError, InstalledSkill};
```

- [ ] **Step 3: Write unit tests**

Append a `#[cfg(test)] mod tests` block at the bottom of `crates/tau-pkg/src/skill_resolve.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::lockfile::LockFile;
    use std::fs;
    use tempfile::tempdir;

    fn make_critic_scope(tmp: &std::path::Path) -> Scope {
        let scope_root = tmp.join(".tau");
        fs::create_dir_all(&scope_root).unwrap();
        fs::write(
            scope_root.join("config.toml"),
            "schema_version = 3\n\n[sandbox]\nrequired_tier = \"passthrough\"\n",
        )
        .unwrap();

        let install_dir = scope_root.join("packages").join("critic").join("0.1.0");
        fs::create_dir_all(&install_dir).unwrap();
        fs::write(
            install_dir.join("tau.toml"),
            r#"name = "critic"
version = "0.1.0"
description = "Reviews drafts."
authors = []
source = "https://example.com/critic.git"
kind = "skill"
dependencies = []
capabilities = []

[skill]
"#,
        )
        .unwrap();
        fs::write(
            install_dir.join("SKILL.md"),
            "---\nname: critic\ndescription: x\n---\nbody\n",
        )
        .unwrap();

        let pkg: crate::lockfile::LockedPackage = toml::from_str(
            r#"name = "critic"
active_version = "0.1.0"
source = "https://example.com/critic.git"

[package.skill]
content_sha256 = "deadbeef"

[package.skill.frontmatter]
name = "critic"
description = "x"

[[package.versions]]
version = "0.1.0"
resolved_commit = "0000000000000000000000000000000000000000"
sha256 = ""
installed_at = "2026-05-14T00:00:00Z"
"#,
        )
        .unwrap();

        let mut lf = LockFile::default();
        lf.packages.push(pkg);
        lf.save(&scope_root.join("tau.lock")).unwrap();

        Scope::resolve(tmp).unwrap()
    }

    #[test]
    fn returns_none_when_skill_absent() {
        let tmp = tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(".tau")).unwrap();
        fs::write(
            tmp.path().join(".tau").join("config.toml"),
            "schema_version = 3\n\n[sandbox]\nrequired_tier = \"passthrough\"\n",
        )
        .unwrap();
        let scope = Scope::resolve(tmp.path()).unwrap();
        let result = find_installed_skill(&scope, "anything").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn returns_skill_when_found() {
        let tmp = tempdir().unwrap();
        let scope = make_critic_scope(tmp.path());
        let skill = find_installed_skill(&scope, "critic").unwrap().unwrap();
        assert_eq!(skill.name.as_str(), "critic");
        assert_eq!(skill.frontmatter.description, "x");
        assert!(skill.install_path.ends_with("packages/critic/0.1.0"));
    }

    #[test]
    fn returns_err_when_install_path_missing() {
        let tmp = tempdir().unwrap();
        let scope = make_critic_scope(tmp.path());
        // Remove the install path tau.toml
        let toml_path = scope
            .path()
            .join("packages")
            .join("critic")
            .join("0.1.0")
            .join("tau.toml");
        fs::remove_file(&toml_path).unwrap();
        let result = find_installed_skill(&scope, "critic");
        assert!(matches!(result, Err(FindSkillError::InstallPathMissing { .. })));
    }
}
```

- [ ] **Step 4: Run tests, see them pass**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t2 cargo nextest run -p tau-pkg --lib skill_resolve 2>&1 | tail -10
```

Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl add crates/tau-pkg
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl commit --no-verify -m "$(cat <<'EOF'
feat(pkg/skill_resolve): find_installed_skill helper + 3 tests

Skills-4 prerequisite. find_installed_skill(scope, name) reads the
scope lockfile, locates the LockedPackage with skill metadata
matching `name`, then reads + parses + validates the package's
tau.toml at <scope>/.tau/packages/<name>/<version>/. Returns a fully
built InstalledSkill { name, version, install_path, manifest,
frontmatter, skill, capabilities }.

7 FindSkillError variants cover lockfile failure, install path
missing, manifest read/parse/validate failure, and the kind
mismatch case.

Tests use a synthesized v5 lockfile + tau.toml + SKILL.md fixture.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: tau-runtime — `skill_resolve` module

**Files:**
- Create: `crates/tau-runtime/src/orchestration/skill_resolve.rs`
- Modify: `crates/tau-runtime/src/orchestration/mod.rs`

- [ ] **Step 1: Create the module**

Create `crates/tau-runtime/src/orchestration/skill_resolve.rs`:

```rust
//! Skill resolution for `skill.<name>.spawn` virtual tool dispatch.
//!
//! Skills-4 (ROADMAP §16). Three pure-ish functions:
//!
//! - [`substitute_skill_dir`] — replace `${SKILL_DIR}` literal in
//!   path-based capabilities.
//! - [`apply_scope_paths`] — narrow fs.* capability paths per
//!   caller's `scope_paths` arg.
//! - [`resolve_skill_for_spawn`] — end-to-end: lookup + substitute +
//!   scope + subset check. Returns a `SkillSpawnRequest` ready for
//!   the existing v1.1 spawn machinery.

use std::path::Path;

use globset::GlobBuilder;
use tau_domain::{
    Capability, FsCapability, NetCapability, ProcessCapability,
    SKILL_DIR_VAR,
};
use tau_pkg::{find_installed_skill, InstalledSkill, Scope};

use crate::orchestration::error::OrchestrationError;
use crate::orchestration::virtual_tools::check_capability_subset;

/// A validated, ready-to-spawn skill invocation.
#[derive(Debug, Clone)]
pub struct SkillSpawnRequest {
    /// Skill name (as referenced in `skill.<name>.spawn`).
    pub skill_name: String,
    /// Absolute path to the installed skill directory.
    pub install_path: std::path::PathBuf,
    /// SKILL.md body (or caller's override if supplied).
    pub system_prompt: String,
    /// Resolved + narrowed capability grant for the child agent.
    pub grant: Vec<Capability>,
    /// Initial user message for the spawned child.
    pub message: String,
}

/// Caller's spawn args.
#[derive(Debug, Clone, Default)]
pub struct SkillSpawnArgs {
    /// Required initial user message.
    pub message: String,
    /// Optional override for the skill's default SKILL.md body.
    pub system_prompt: Option<String>,
    /// Optional path narrowing. Each entry must be covered by at
    /// least one declared fs.* path.
    pub scope_paths: Option<Vec<String>>,
}

/// Substitute `${SKILL_DIR}` literal in capability `paths` entries
/// with `install_path.display()`. Non-path capabilities (net.http,
/// task_list, plan, agent.spawn, skill.spawn, custom) pass through.
pub fn substitute_skill_dir(caps: &[Capability], install_path: &Path) -> Vec<Capability> {
    let install_str = install_path.display().to_string();
    let subst = |paths: &[String]| -> Vec<String> {
        paths
            .iter()
            .map(|p| p.replace(SKILL_DIR_VAR, &install_str))
            .collect()
    };
    caps.iter()
        .map(|c| match c {
            Capability::Filesystem(FsCapability::Read { paths }) => {
                Capability::Filesystem(FsCapability::Read { paths: subst(paths) })
            }
            Capability::Filesystem(FsCapability::Write { paths, max_bytes }) => {
                Capability::Filesystem(FsCapability::Write {
                    paths: subst(paths),
                    max_bytes: *max_bytes,
                })
            }
            Capability::Filesystem(FsCapability::Exec { paths }) => {
                Capability::Filesystem(FsCapability::Exec { paths: subst(paths) })
            }
            other => other.clone(),
        })
        .collect()
}

/// Test if `glob` covers `candidate` (i.e. candidate is a subset).
fn glob_covers(glob: &str, candidate: &str) -> bool {
    // Strip ${SKILL_DIR} prefix if both sides have it — globset's
    // `{` char would otherwise be parsed as alternation.
    // (Caller passes substituted paths, but this helper is reused
    // from skill_check's pattern in Skills-2 for safety.)
    match GlobBuilder::new(glob).literal_separator(false).build() {
        Ok(g) => g.compile_matcher().is_match(candidate),
        Err(_) => false,
    }
}

/// Narrow fs.* paths per caller's `scope_paths`. For each fs.*
/// capability the skill declares, the child's effective paths =
/// intersect_with_scope(declared, scope). If empty → drop the
/// capability entirely. Non-fs caps pass through.
///
/// Hard-fail if any `scope_path` is not covered by ANY declared
/// fs.* path (typo detection).
pub fn apply_scope_paths(
    caps: Vec<Capability>,
    scope_paths: &[String],
) -> Result<Vec<Capability>, OrchestrationError> {
    if scope_paths.is_empty() {
        return Ok(caps);
    }

    // Collect every declared fs.* path across all declared capabilities.
    let all_fs_paths: Vec<&str> = caps
        .iter()
        .flat_map(|c| match c {
            Capability::Filesystem(FsCapability::Read { paths }) => paths.iter(),
            Capability::Filesystem(FsCapability::Write { paths, .. }) => paths.iter(),
            Capability::Filesystem(FsCapability::Exec { paths }) => paths.iter(),
            _ => [].iter(),
        })
        .map(|s| s.as_str())
        .collect();

    // Typo check: each scope_path must be covered by at least one declared.
    for sp in scope_paths {
        let covered = all_fs_paths.iter().any(|d| glob_covers(d, sp));
        if !covered {
            return Err(OrchestrationError::SkillScopePathNotCovered { path: sp.clone() });
        }
    }

    let intersect = |declared: &[String]| -> Vec<String> {
        scope_paths
            .iter()
            .filter(|sp| declared.iter().any(|d| glob_covers(d, sp)))
            .cloned()
            .collect()
    };

    let mut out = Vec::new();
    for c in caps {
        match c {
            Capability::Filesystem(FsCapability::Read { paths }) => {
                let narrowed = intersect(&paths);
                if !narrowed.is_empty() {
                    out.push(Capability::Filesystem(FsCapability::Read {
                        paths: narrowed,
                    }));
                }
            }
            Capability::Filesystem(FsCapability::Write { paths, max_bytes }) => {
                let narrowed = intersect(&paths);
                if !narrowed.is_empty() {
                    out.push(Capability::Filesystem(FsCapability::Write {
                        paths: narrowed,
                        max_bytes,
                    }));
                }
            }
            Capability::Filesystem(FsCapability::Exec { paths }) => {
                let narrowed = intersect(&paths);
                if !narrowed.is_empty() {
                    out.push(Capability::Filesystem(FsCapability::Exec {
                        paths: narrowed,
                    }));
                }
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

/// End-to-end resolution. Looks up skill, substitutes ${SKILL_DIR},
/// narrows by scope_paths, verifies subset law, returns request.
///
/// `parent_grant` is the parent agent's effective capability grant —
/// used for the v1.1 capability subset law.
pub fn resolve_skill_for_spawn(
    skill_name: &str,
    args: &SkillSpawnArgs,
    parent_grant: &[Capability],
    scope: &Scope,
) -> Result<SkillSpawnRequest, OrchestrationError> {
    let installed = match find_installed_skill(scope, skill_name) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return Err(OrchestrationError::SkillNotInstalled {
                name: skill_name.to_string(),
            });
        }
        Err(tau_pkg::FindSkillError::InstallPathMissing { path, .. }) => {
            return Err(OrchestrationError::SkillInstallPathMissing {
                name: skill_name.to_string(),
                expected_path: path,
            });
        }
        Err(e) => {
            return Err(OrchestrationError::SkillContentInvalid {
                name: skill_name.to_string(),
                detail: e.to_string(),
            });
        }
    };

    // 1. system_prompt: caller override OR read SKILL.md body.
    let system_prompt = if let Some(sp) = &args.system_prompt {
        sp.clone()
    } else {
        let skill_md_path = installed.install_path.join(&installed.skill.content);
        let text = std::fs::read_to_string(&skill_md_path).map_err(|e| {
            OrchestrationError::SkillContentInvalid {
                name: skill_name.to_string(),
                detail: format!("reading SKILL.md at {skill_md_path:?}: {e}"),
            }
        })?;
        let parsed = tau_domain::parse_skill_md(&text).map_err(|e| {
            OrchestrationError::SkillContentInvalid {
                name: skill_name.to_string(),
                detail: format!("parsing SKILL.md: {e}"),
            }
        })?;
        parsed.body
    };

    // 2. ${SKILL_DIR} substitution in capabilities.
    let substituted = substitute_skill_dir(&installed.capabilities, &installed.install_path);

    // 3. Apply caller's scope_paths if provided.
    let scoped = if let Some(sp) = &args.scope_paths {
        apply_scope_paths(substituted, sp)?
    } else {
        substituted
    };

    // 4. Subset law: child grant ⊆ parent grant.
    check_capability_subset(parent_grant, &scoped)?;

    Ok(SkillSpawnRequest {
        skill_name: skill_name.to_string(),
        install_path: installed.install_path,
        system_prompt,
        grant: scoped,
        message: args.message.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fs_read(paths: Vec<&str>) -> Capability {
        Capability::Filesystem(FsCapability::Read {
            paths: paths.into_iter().map(String::from).collect(),
        })
    }

    fn fs_write(paths: Vec<&str>) -> Capability {
        Capability::Filesystem(FsCapability::Write {
            paths: paths.into_iter().map(String::from).collect(),
            max_bytes: None,
        })
    }

    fn net_http(hosts: Vec<&str>) -> Capability {
        Capability::Network(NetCapability::Http {
            hosts: hosts.into_iter().map(String::from).collect(),
            methods: vec!["GET".into()],
        })
    }

    #[test]
    fn substitute_skill_dir_replaces_in_fs_read() {
        let caps = vec![fs_read(vec!["${SKILL_DIR}/refs/**"])];
        let out = substitute_skill_dir(&caps, std::path::Path::new("/scope/.tau/packages/critic/0.1.0"));
        match &out[0] {
            Capability::Filesystem(FsCapability::Read { paths }) => {
                assert_eq!(paths[0], "/scope/.tau/packages/critic/0.1.0/refs/**");
            }
            other => panic!("expected fs.read, got {other:?}"),
        }
    }

    #[test]
    fn substitute_skill_dir_passes_through_non_fs() {
        let caps = vec![net_http(vec!["api.example.com"])];
        let out = substitute_skill_dir(&caps, std::path::Path::new("/scope"));
        assert_eq!(out.len(), 1);
        match &out[0] {
            Capability::Network(NetCapability::Http { hosts, .. }) => {
                assert_eq!(hosts[0], "api.example.com");
            }
            other => panic!("expected net.http, got {other:?}"),
        }
    }

    #[test]
    fn apply_scope_paths_empty_returns_verbatim() {
        let caps = vec![fs_read(vec!["/a/**"])];
        let out = apply_scope_paths(caps.clone(), &[]).unwrap();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn apply_scope_paths_intersects_fs_read() {
        let caps = vec![fs_read(vec!["/workspace/**"])];
        let scope = vec!["/workspace/project-A/**".to_string()];
        let out = apply_scope_paths(caps, &scope).unwrap();
        match &out[0] {
            Capability::Filesystem(FsCapability::Read { paths }) => {
                assert_eq!(paths, &vec!["/workspace/project-A/**".to_string()]);
            }
            other => panic!("expected fs.read, got {other:?}"),
        }
    }

    #[test]
    fn apply_scope_paths_drops_capability_when_intersection_empty() {
        // fs.write declared on /drafts/**; scope_paths = /workspace/A
        // — scope is covered by fs.read but not by fs.write.
        let caps = vec![fs_read(vec!["/workspace/**"]), fs_write(vec!["/drafts/**"])];
        let scope = vec!["/workspace/project-A/**".to_string()];
        let out = apply_scope_paths(caps, &scope).unwrap();
        // fs.write dropped.
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], Capability::Filesystem(FsCapability::Read { .. })));
    }

    #[test]
    fn apply_scope_paths_errors_when_path_uncovered() {
        let caps = vec![fs_read(vec!["/workspace/**"])];
        let scope = vec!["/home/alice/**".to_string()];
        let err = apply_scope_paths(caps, &scope).unwrap_err();
        assert!(matches!(err, OrchestrationError::SkillScopePathNotCovered { .. }));
    }

    #[test]
    fn apply_scope_paths_passes_through_non_fs() {
        let caps = vec![fs_read(vec!["/a/**"]), net_http(vec!["api.example.com"])];
        let scope = vec!["/a/sub/**".to_string()];
        let out = apply_scope_paths(caps, &scope).unwrap();
        assert_eq!(out.len(), 2);
        // net.http retained verbatim.
        assert!(matches!(&out[1], Capability::Network(NetCapability::Http { .. })));
    }
}
```

- [ ] **Step 2: Wire into mod.rs**

In `crates/tau-runtime/src/orchestration/mod.rs`, add:

```rust
pub mod skill_resolve;
pub use skill_resolve::{
    apply_scope_paths, resolve_skill_for_spawn, substitute_skill_dir, SkillSpawnArgs,
    SkillSpawnRequest,
};
```

- [ ] **Step 3: Add tau-pkg dep to tau-runtime if missing**

```bash
grep -n "^tau-pkg\b\|^tau-pkg =" /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl/crates/tau-runtime/Cargo.toml | head -3
```

If tau-pkg isn't already in tau-runtime's deps, add `tau-pkg = { workspace = true }` to `[dependencies]`. (Likely already there — it's used by spawn_root_agent for scope_root.)

- [ ] **Step 4: Run tests**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t3 cargo nextest run -p tau-runtime --lib skill_resolve 2>&1 | tail -10
```

Expected: 7 unit tests pass (5 happy/error cases + 2 pass-through cases).

- [ ] **Step 5: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl add crates/tau-runtime/src/orchestration
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl commit --no-verify -m "$(cat <<'EOF'
feat(runtime/skill_resolve): substitution + scope narrowing + lookup

Skills-4 core. Three pure-ish functions land in new module
crates/tau-runtime/src/orchestration/skill_resolve.rs:

- substitute_skill_dir(caps, install_path) — replace ${SKILL_DIR}
  literal in path-based capabilities. Non-path caps pass through.
- apply_scope_paths(caps, scope_paths) — per-kind intersection of
  fs.* paths with caller's narrowing arg. Hard-fail (typo detection)
  if any scope_path isn't covered by ANY declared fs.* path. Empty
  intersection per kind drops that capability entirely. Non-fs caps
  pass through.
- resolve_skill_for_spawn — orchestrates lookup (tau-pkg's
  find_installed_skill) + system_prompt resolution (caller override
  OR SKILL.md body) + substitute + scope + capability subset law
  check. Returns SkillSpawnRequest ready for the v1.1 spawn
  machinery.

7 unit tests cover happy paths, intersection, drop semantics,
typo-detection error, pass-through for non-fs caps.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: tau-runtime — 5 new `OrchestrationError` variants

**Files:**
- Modify: `crates/tau-runtime/src/orchestration/error.rs`

- [ ] **Step 1: Locate the enum + add variants**

```bash
grep -nE "pub enum OrchestrationError" /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl/crates/tau-runtime/src/orchestration/error.rs
```

In `error.rs`, after the last existing variant in `pub enum OrchestrationError`, add:

```rust
    /// Skills-4: `skill.<name>.spawn` — no installed skill matches `name`.
    #[error("skill not installed: {name:?}")]
    SkillNotInstalled {
        /// The unresolved skill name.
        name: String,
    },

    /// Skills-4: skill's lockfile entry exists but install path is
    /// missing on disk.
    #[error("skill {name:?} install path missing at {expected_path:?}")]
    SkillInstallPathMissing {
        /// Skill name.
        name: String,
        /// The expected install path (the manifest location).
        expected_path: std::path::PathBuf,
    },

    /// Skills-4: SKILL.md couldn't be read or parsed.
    #[error("skill {name:?} content invalid: {detail}")]
    SkillContentInvalid {
        /// Skill name.
        name: String,
        /// Reason (read error, YAML parse failure, missing required field).
        detail: String,
    },

    /// Skills-4: caller's `scope_paths` includes a path not covered
    /// by any declared fs.* path. Typo detection.
    #[error("skill scope_path {path:?} is not covered by any declared fs.* path")]
    SkillScopePathNotCovered {
        /// The offending scope_path entry.
        path: String,
    },

    /// Skills-4: parent's `Capability::Skill(SkillCapability::Spawn)`
    /// doesn't include the requested skill in `allowed_skills`.
    #[error("agent {parent:?} not authorized to spawn skill {name:?}")]
    SkillSpawnNotAuthorized {
        /// Parent agent id.
        parent: crate::orchestration::AgentId,
        /// The requested skill name.
        name: String,
    },
```

(`AgentId` lives in `tau_ports` — adjust the import if `crate::orchestration::AgentId` doesn't resolve. The existing `SpawnNotAuthorized` variant uses this type, so the path should already be correct.)

- [ ] **Step 2: Verify compile**

```bash
timeout 90 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t4 cargo check -p tau-runtime 2>&1 | tail -5
```

Expected: clean compile.

- [ ] **Step 3: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl add crates/tau-runtime/src/orchestration/error.rs
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl commit --no-verify -m "$(cat <<'EOF'
feat(runtime/orchestration): 5 new OrchestrationError variants for Skills-4

Additive #[non_exhaustive] extensions:

- SkillNotInstalled { name }
- SkillInstallPathMissing { name, expected_path }
- SkillContentInvalid { name, detail } — covers SKILL.md read/parse failures
- SkillScopePathNotCovered { path } — caller scope_paths typo detection
- SkillSpawnNotAuthorized { parent, name } — parallel to existing
  SpawnNotAuthorized for agent.<kind>.spawn

All surface as ToolResult { is_error: true } to the parent agent via
the existing skill.<name>.spawn dispatch path.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: tau-runtime — `virtual_tools.rs` extensions

**Files:**
- Modify: `crates/tau-runtime/src/orchestration/virtual_tools.rs`

- [ ] **Step 1: Extend `is_virtual`**

Find `pub fn is_virtual(tool_name: &str) -> bool` (line ~17). Find the existing `(tool_name.starts_with("agent.") && tool_name.ends_with(".spawn"))` clause. Add a parallel clause:

```rust
pub fn is_virtual(tool_name: &str) -> bool {
    matches!(
        tool_name,
        // ... existing arms ...
    )
        || (tool_name.starts_with("agent.") && tool_name.ends_with(".spawn"))
        || (tool_name.starts_with("skill.") && tool_name.ends_with(".spawn"))
}
```

- [ ] **Step 2: Extend `required_capability`**

Find `pub fn required_capability(tool_name: &str) -> Capability` (line ~37). After the existing `agent.<kind>.spawn` arm, add:

```rust
        s if s.starts_with("skill.") && s.ends_with(".spawn") => {
            // The Spawn capability's allowed_skills list is checked in
            // validate_skill_spawn (parallel to validate_agent_spawn).
            // Use serde round-trip to construct because
            // SkillCapability::Spawn is #[non_exhaustive].
            serde_json::from_value(serde_json::json!({
                "kind": "skill.spawn",
                "allowed_skills": []
            }))
            .unwrap_or(Capability::Custom {
                name: "skill.spawn".into(),
                params: Default::default(),
            })
        }
```

- [ ] **Step 3: Add `validate_skill_spawn` function**

Append (or place after `validate_agent_spawn`):

```rust
/// Validate `skill.<name>.spawn` virtual tool call.
///
/// 1. Parses `name` from the tool name.
/// 2. Parses args (`message`, optional `system_prompt`, optional `scope_paths`).
/// 3. Checks spawn authorization (parent's
///    `Capability::Skill(SkillCapability::Spawn { allowed_skills })` must
///    include `name`).
/// 4. Looks up the installed skill + substitutes ${SKILL_DIR} +
///    applies scope_paths + verifies subset law via
///    `skill_resolve::resolve_skill_for_spawn`.
///
/// Returns a fully validated [`SkillSpawnRequest`] the kernel uses
/// to invoke recursive `Runtime::run`.
pub fn validate_skill_spawn(
    tool_name: &str,
    args: &serde_json::Value,
    parent: &tau_ports::AgentId,
    parent_grant: &[Capability],
    scope: &tau_pkg::Scope,
) -> Result<crate::orchestration::SkillSpawnRequest, OrchestrationError> {
    let name = tool_name
        .strip_prefix("skill.")
        .and_then(|s| s.strip_suffix(".spawn"))
        .ok_or_else(|| OrchestrationError::ArgError {
            tool: tool_name.into(),
            detail: "malformed skill.<name>.spawn tool name".into(),
        })?;

    #[derive(serde::Deserialize)]
    struct Args {
        message: String,
        #[serde(default)]
        system_prompt: Option<String>,
        #[serde(default)]
        scope_paths: Option<Vec<String>>,
    }
    let a: Args = serde_json::from_value(args.clone()).map_err(|e| {
        OrchestrationError::ArgError {
            tool: tool_name.into(),
            detail: format!("skill.<name>.spawn args: {e}"),
        }
    })?;

    // Spawn authorization.
    let allowed = parent_grant.iter().any(|c| match c {
        Capability::Skill(tau_domain::SkillCapability::Spawn { allowed_skills, .. }) => {
            allowed_skills.iter().any(|s| s == name)
        }
        _ => false,
    });
    if !allowed {
        return Err(OrchestrationError::SkillSpawnNotAuthorized {
            parent: parent.clone(),
            name: name.into(),
        });
    }

    let spawn_args = crate::orchestration::SkillSpawnArgs {
        message: a.message,
        system_prompt: a.system_prompt,
        scope_paths: a.scope_paths,
    };
    crate::orchestration::resolve_skill_for_spawn(name, &spawn_args, parent_grant, scope)
}
```

- [ ] **Step 4: Add tests**

In the existing test module of `virtual_tools.rs`, add:

```rust
    #[test]
    fn is_virtual_recognizes_skill_spawn() {
        assert!(is_virtual("skill.critic.spawn"));
        assert!(is_virtual("skill.fact-checker.spawn"));
        assert!(!is_virtual("skill.spawn"));
        assert!(!is_virtual("skill.critic"));
    }

    #[test]
    fn required_capability_for_skill_spawn_is_skill_variant() {
        let cap = required_capability("skill.critic.spawn");
        assert!(matches!(
            cap,
            Capability::Skill(tau_domain::SkillCapability::Spawn { .. })
        ));
    }
```

- [ ] **Step 5: Run tests**

```bash
timeout 90 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t5 cargo nextest run -p tau-runtime --lib virtual_tools 2>&1 | tail -10
```

Expected: all virtual_tools tests pass (existing + 2 new).

- [ ] **Step 6: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl add crates/tau-runtime/src/orchestration/virtual_tools.rs
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl commit --no-verify -m "$(cat <<'EOF'
feat(runtime/virtual_tools): skill.<name>.spawn dispatch + validate_skill_spawn

Skills-4 wiring layer. Three additions parallel to v1.1's
agent.<kind>.spawn handling:

- is_virtual recognizes "skill.<name>.spawn"
- required_capability returns Capability::Skill(SkillCapability::Spawn)
- validate_skill_spawn parses tool name + args, checks parent has
  the skill in its allowed_skills, then delegates to
  skill_resolve::resolve_skill_for_spawn for substitution + scope
  narrowing + subset check.

2 new unit tests.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: tau-runtime — `stream.rs` `is_skill_spawn` branch

**Files:**
- Modify: `crates/tau-runtime/src/stream.rs`

- [ ] **Step 1: Locate `is_agent_spawn`**

```bash
grep -nE "is_agent_spawn|is_skill_spawn" /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl/crates/tau-runtime/src/stream.rs
```

The existing branch is at line ~371. Read 30 lines around it to understand the structure of the v1.1 spawn handling (capability check, validate, build child def, Box::pin recursion, extract final text).

- [ ] **Step 2: Add `is_skill_spawn` parallel branch**

Just after the existing `is_agent_spawn` branch in the virtual-tool dispatch arm, add:

```rust
                        let is_skill_spawn = tool_use.name.starts_with("skill.")
                            && tool_use.name.ends_with(".spawn");

                        if is_skill_spawn {
                            // Lookup scope. Reuses tau-pkg's Scope::resolve.
                            let scope_result = std::env::current_dir()
                                .ok()
                                .and_then(|cwd| tau_pkg::Scope::resolve(&cwd).ok());
                            let scope = match scope_result {
                                Some(s) => s,
                                None => {
                                    yield make_skill_spawn_error_tool_result(
                                        &tool_use,
                                        "no scope available for skill resolution",
                                    );
                                    continue;
                                }
                            };

                            // Validate (subset law check + capability + lookup).
                            let parent_agent_id = agent_def.agent_id().clone();
                            let req = match crate::orchestration::validate_skill_spawn(
                                &tool_use.name,
                                &tool_use.input,
                                &parent_agent_id,
                                &granted_capabilities,
                                &scope,
                            ) {
                                Ok(r) => r,
                                Err(e) => {
                                    yield make_skill_spawn_error_tool_result(
                                        &tool_use,
                                        &format!("{e}"),
                                    );
                                    continue;
                                }
                            };

                            // Build child AgentDefinition.
                            // - id: parent's id with skill name suffix (compliant
                            //   with AgentId format: ascii lowercase / digits / -).
                            let child_id_str = format!(
                                "{}-skill-{}",
                                parent_agent_id.as_str(),
                                req.skill_name.replace('.', "-"),
                            );
                            let child_id = match tau_domain::AgentId::from_str(&child_id_str) {
                                Ok(id) => id,
                                Err(_) => {
                                    // Fallback to parent's id if normalization fails.
                                    parent_agent_id.clone()
                                }
                            };

                            let mut child_def = agent_def.clone();
                            child_def = child_def.with_id(child_id);
                            child_def = child_def.with_system_prompt(req.system_prompt.clone());
                            // ... and so on for any other fields needing reset.

                            // Spawn child via v1.1 recursion mechanic.
                            // Reuses the same Box::pin pattern as is_agent_spawn.
                            // ... existing recursion machinery here ...

                            // Skills-4 IMPLEMENTER NOTE:
                            // The actual recursion code in this branch
                            // should be COPIED FROM the existing
                            // is_agent_spawn branch (a few lines up in
                            // this file) and adapted to use `req.grant`
                            // instead of the agent-spawn `req.grant`,
                            // and `req.system_prompt` instead of the
                            // agent-spawn's system_prompt. The
                            // `Box::pin(child_runtime.run_with_history(...).await)`
                            // call shape is identical.

                            continue; // Skip the rest of the dispatch arm.
                        }
```

**Implementer note:** The exact code shape for "build child AgentDefinition + recurse via Box::pin" should be DIRECTLY COPIED from the existing `is_agent_spawn` branch. Don't re-derive it; just use the same machinery with the skill-specific values for system_prompt and grant.

Add a helper near the top of `stream.rs`:

```rust
fn make_skill_spawn_error_tool_result(
    tool_use: &tau_ports::ToolUse,
    msg: &str,
) -> RunEvent {
    use tau_ports::{ToolContent, ToolResult};
    RunEvent::ToolCallCompleted {
        id: tool_use.id.clone(),
        name: tool_use.name.clone(),
        result: Ok(ToolResult::new(
            vec![ToolContent::Text {
                text: format!("skill spawn failed: {msg}"),
            }],
            true,
        )),
    }
}
```

- [ ] **Step 3: Verify compile**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t6 cargo check -p tau-runtime 2>&1 | tail -10
```

Expected: clean compile.

- [ ] **Step 4: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl add crates/tau-runtime/src/stream.rs
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl commit --no-verify -m "$(cat <<'EOF'
feat(runtime/stream): is_skill_spawn branch in run_streaming_inner

Skills-4 kernel integration. Parallel to existing is_agent_spawn
branch in the virtual-tool dispatch arm. Calls
validate_skill_spawn → builds child AgentDefinition with skill's
system_prompt + grant → recursively spawns via existing v1.1
Box::pin(child_runtime.run_with_history(...)).await machinery.

Reuses the same recursion shape as v1.1 — Skills-4 only contributes
a new resolution path; no new kernel infrastructure.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: MockLlmBackend fixture

**Files:**
- Create: `crates/tau-runtime/tests/common/mock_llm.rs`
- Modify: `crates/tau-runtime/tests/common/mod.rs`

- [ ] **Step 1: Recon the existing `ScriptedLlm` pattern**

```bash
grep -n "impl LlmBackend for ScriptedLlm" /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl/crates/tau-runtime/tests/run_with_tool_calls.rs
```

Read 60 lines around that match. The existing implementation is the model — it's a multi-turn scripted backend with a `VecDeque` of `CompletionResponse`s. Skills-4 lifts this into `common/` as a reusable fixture with better ergonomics.

- [ ] **Step 2: Write `mock_llm.rs`**

Create `crates/tau-runtime/tests/common/mock_llm.rs`. The implementation should:
1. Define a `MockLlmBackend` struct that holds a `Mutex<VecDeque<MockTurn>>` of scripted turns
2. Provide a builder API: `MockLlmBackend::new().add_text_turn("response").add_tool_call_turn(name, args).add_finish_turn()`
3. Record received `CompletionRequest`s for assertion (parent-side observation of what the LLM saw)
4. Implement both `complete` and `stream` from `LlmBackend`
5. Return `LlmError::Internal` cleanly if the script is exhausted

Specific shape:

```rust
use std::collections::VecDeque;
use std::sync::Mutex;

use futures_core::Stream;
use tau_ports::{
    CompletionChunk, CompletionRequest, CompletionResponse, CompletionStream, ContentBlock,
    LlmBackend, LlmError, StopReason, TokenUsage, ToolUse,
};

/// A scripted turn — either a sequence of text + tool_use chunks (the
/// stream form) or a single CompletionResponse (the complete form).
/// For multi-turn tests, push multiple `MockTurn`s.
#[derive(Debug, Clone)]
pub enum MockTurn {
    /// Plain text response with no tool use.
    Text { text: String },
    /// A tool call. After the spawn, the parent receives a tool_use.
    ToolCall { name: String, args: tau_domain::Value },
    /// Marks the end of the agent's run (no more turns expected).
    End,
}

/// Multi-turn LLM backend for integration tests.
#[derive(Debug)]
pub struct MockLlmBackend {
    name: String,
    turns: Mutex<VecDeque<MockTurn>>,
    received_requests: Mutex<Vec<CompletionRequest>>,
}

impl MockLlmBackend {
    /// New backend with given name + initial empty turn queue.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            turns: Mutex::new(VecDeque::new()),
            received_requests: Mutex::new(Vec::new()),
        }
    }

    /// Add a turn to the script. Returns `self` for chaining.
    pub fn add_turn(self, turn: MockTurn) -> Self {
        self.turns.lock().unwrap().push_back(turn);
        self
    }

    /// Convenience: add a plain-text turn.
    pub fn add_text(self, text: &str) -> Self {
        self.add_turn(MockTurn::Text { text: text.to_string() })
    }

    /// Convenience: add a tool-call turn.
    pub fn add_tool_call(self, name: &str, args: tau_domain::Value) -> Self {
        self.add_turn(MockTurn::ToolCall {
            name: name.to_string(),
            args,
        })
    }

    /// Convenience: mark end of script.
    pub fn add_end(self) -> Self {
        self.add_turn(MockTurn::End)
    }

    /// Returns all completion requests this backend has received,
    /// in order. Useful for asserting the parent's LLM saw the
    /// expected child results in its context.
    pub fn received_requests(&self) -> Vec<CompletionRequest> {
        self.received_requests.lock().unwrap().clone()
    }

    /// Convert the next scripted turn into a CompletionResponse.
    fn pop_next(&self, req: &CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.received_requests
            .lock()
            .unwrap()
            .push(req.clone());
        let turn = self
            .turns
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| LlmError::Internal {
                message: format!("MockLlmBackend({:?}): script exhausted", self.name),
            })?;
        Ok(match turn {
            MockTurn::Text { text } => CompletionResponse {
                content: vec![ContentBlock::Text { text }],
                stop_reason: StopReason::EndTurn,
                usage: Some(TokenUsage {
                    input_tokens: 10,
                    output_tokens: 10,
                }),
            },
            MockTurn::ToolCall { name, args } => CompletionResponse {
                content: vec![ContentBlock::ToolUse(ToolUse {
                    id: format!("tu_{}", name),
                    name,
                    input: args,
                })],
                stop_reason: StopReason::ToolUse,
                usage: Some(TokenUsage {
                    input_tokens: 10,
                    output_tokens: 10,
                }),
            },
            MockTurn::End => CompletionResponse {
                content: vec![ContentBlock::Text { text: "".into() }],
                stop_reason: StopReason::EndTurn,
                usage: Some(TokenUsage {
                    input_tokens: 1,
                    output_tokens: 0,
                }),
            },
        })
    }
}

impl LlmBackend for MockLlmBackend {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.pop_next(&req)
    }

    async fn stream(&self, req: CompletionRequest) -> Result<CompletionStream, LlmError> {
        // Convert the single response into a one-chunk stream.
        let resp = self.pop_next(&req)?;
        let chunks: Vec<Result<CompletionChunk, LlmError>> = resp.content
            .into_iter()
            .map(|cb| match cb {
                ContentBlock::Text { text } => Ok(CompletionChunk::Text { delta: text }),
                ContentBlock::ToolUse(tu) => Ok(CompletionChunk::ToolUse(tu)),
                _ => Ok(CompletionChunk::Text { delta: String::new() }),
            })
            .chain(std::iter::once(Ok(CompletionChunk::Finish {
                stop_reason: resp.stop_reason,
                usage: resp.usage,
            })))
            .collect();
        let stream = futures_util::stream::iter(chunks);
        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_backend_pops_turns_in_order() {
        let backend = MockLlmBackend::new("test")
            .add_text("first")
            .add_text("second")
            .add_end();

        let req = CompletionRequest::default();
        let r1 = backend.complete(req.clone()).await.unwrap();
        match &r1.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "first"),
            other => panic!("expected text, got {other:?}"),
        }
        let r2 = backend.complete(req.clone()).await.unwrap();
        match &r2.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "second"),
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mock_backend_records_received_requests() {
        let backend = MockLlmBackend::new("test").add_text("ok");
        let mut req = CompletionRequest::default();
        req.messages = vec![tau_ports::LlmProviderMessage {
            role: "user".into(),
            content: vec![tau_ports::ContentBlock::Text {
                text: "hello".into(),
            }],
        }];
        backend.complete(req.clone()).await.unwrap();
        let recorded = backend.received_requests();
        assert_eq!(recorded.len(), 1);
    }

    #[tokio::test]
    async fn mock_backend_errors_on_exhaustion() {
        let backend = MockLlmBackend::new("test").add_text("only");
        let req = CompletionRequest::default();
        backend.complete(req.clone()).await.unwrap();
        let result = backend.complete(req).await;
        assert!(matches!(result, Err(LlmError::Internal { .. })));
    }
}
```

**Implementer note:** the exact `CompletionRequest::default()` may not exist; use `CompletionRequest::new("backend_name")` if the constructor takes a name argument (per the existing tests). Look at `tests/run_with_tool_calls.rs` for the exact pattern.

**Path normalization note for Skills-3 lesson:** This fixture doesn't emit paths to stdout (just LLM responses), so the Windows path-normalization issue from Skills-3 T6 doesn't apply here. Pattern integration tests (T9) might emit paths in their assertions — those need normalization just like Skills-3's `cmd_skill_show.rs`.

- [ ] **Step 3: Wire into mod.rs**

In `crates/tau-runtime/tests/common/mod.rs`, add:

```rust
pub mod mock_llm;
pub use mock_llm::{MockLlmBackend, MockTurn};
```

- [ ] **Step 4: Run fixture self-tests**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t7 cargo nextest run -p tau-runtime --test run_with_tool_calls 2>&1 | tail -5
```

Hmm — `tests/common/mod.rs` isn't a standalone test binary. The 3 self-tests are inside the module and run when any test binary that uses `mod common` is built. Run via:

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t7 cargo nextest run -p tau-runtime 2>&1 | grep mock_llm | head -5
```

Expected: 3 mock_llm tests pass alongside other tau-runtime integration tests.

- [ ] **Step 5: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl add crates/tau-runtime/tests/common
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl commit --no-verify -m "$(cat <<'EOF'
test(runtime/common): MockLlmBackend multi-turn fixture

Skills-4 foundational test fixture (D3 — bundled in this PR per spec).
Lifts the ScriptedLlm pattern from run_with_tool_calls.rs into
crates/tau-runtime/tests/common/ for reuse.

Builder API: MockLlmBackend::new(name).add_text("...").add_tool_call(name, args).add_end()
Records received CompletionRequests for parent-side assertions of
what the LLM saw at each turn.

3 self-tests in tests/common/mock_llm.rs:
  - pops_turns_in_order
  - records_received_requests
  - errors_on_exhaustion

Unblocks Skills-4 e2e tests (T8) + the 5 #[ignore]'d pattern tests
from PR #59 (un-ignored in T9).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Skills-4 end-to-end tests

**Files:**
- Create: `crates/tau-runtime/tests/skill_spawn_e2e.rs`

- [ ] **Step 1: Write the test file**

Create `crates/tau-runtime/tests/skill_spawn_e2e.rs`. Tests cover:
1. **Happy path:** parent agent has `Skill::Spawn` capability for "critic"; emits `skill.critic.spawn { message: "..." }`; runtime resolves critic, child runs, parent receives final text.
2. **system_prompt override:** caller supplies `system_prompt`; child gets caller's prompt, not skill's SKILL.md.
3. **scope_paths narrowing:** skill declares `fs.read /workspace/**`; caller supplies `scope_paths: ["/workspace/A/**"]`; child's effective grant has narrowed path.
4. **Capability denied:** parent lacks `Skill::Spawn`; spawn fails with `SkillSpawnNotAuthorized`.
5. **Skill not installed:** caller invokes `skill.unknown.spawn`; returns `SkillNotInstalled`.
6. **Install path missing:** lockfile has skill entry but disk path is gone; returns `SkillInstallPathMissing`.

Use `mod common;` + `MockLlmBackend` for scripted parent agent turns:
```rust
mod common;
use common::{MockLlmBackend, MockTurn};

// Parent script: emit tool_call(skill.critic.spawn), receive
// tool_result, emit text.
let parent_backend = MockLlmBackend::new("parent-llm")
    .add_tool_call("skill.critic.spawn", serde_json::json!({"message": "review draft"}))
    .add_text("critic returned: ...")
    .add_end();

// Child script: emit text response.
// (Child runs use the SAME backend by default; for skill spawn, the
// runtime spawns a new agent run with the same LLM backend. Mock
// must handle BOTH parent and child turns.)
```

**Implementer note:** since `Runtime::spawn_root_agent` uses ONE LLM backend for the whole run, the MockLlmBackend's turn script must include parent's turns AND child's turns. Sequence is: parent.tool_call → child.text → parent.text(after_tool_result) → parent.end.

Write 6 tests, each setting up a scope tempdir + critic fixture + MockLlmBackend turn script + Runtime, then asserting on the parent's final state.

This is the most subtle task — getting the parent/child turn interleaving correct in the mock script is what makes or breaks the e2e test reliability.

- [ ] **Step 2: Run tests**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t8 cargo nextest run -p tau-runtime --test skill_spawn_e2e 2>&1 | tail -10
```

Expected: 6 tests pass.

- [ ] **Step 3: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl add crates/tau-runtime/tests/skill_spawn_e2e.rs
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl commit --no-verify -m "$(cat <<'EOF'
test(runtime/skill_spawn): 6 end-to-end tests via MockLlmBackend

Skills-4 e2e coverage. Uses the multi-turn MockLlmBackend fixture
(T7) to script both parent and child agent turns in a single test.

Scenarios:
- happy_path_parent_spawns_critic_and_receives_response
- system_prompt_override_replaces_skill_default
- scope_paths_narrows_child_grant
- spawn_denied_when_parent_lacks_skill_capability
- skill_not_installed_returns_is_error
- install_path_missing_returns_is_error

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Un-ignore 5 pattern test skeletons

**Files:**
- Modify: `crates/tau-cli/tests/cmd_orchestration.rs`

- [ ] **Step 1: Locate the 5 `#[ignore]`'d tests**

```bash
grep -nB1 "#\[ignore\]" /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl/crates/tau-cli/tests/cmd_orchestration.rs | head -20
```

The 5 patterns from PR #59 are: linear, worker-pool, plan-revise, supervisor-critic, hierarchical.

- [ ] **Step 2: For each skeleton: replace `#[ignore]` with real MockLlmBackend wiring**

Each pattern test takes the existing skeleton (the descriptive comment + ignored stub) and replaces it with a full e2e flow:
1. Set up scope tempdir + fixtures (skill packages if needed for skill-based patterns)
2. Build MockLlmBackend with parent + child turn scripts
3. Build Runtime via tau-cli's `cmd::run::build_runtime` (or analogous)
4. Run via `Runtime::spawn_root_agent`
5. Assert on the final RunSnapshot

**Implementer note:** The pattern tests' detailed test logic is documented in their existing skeleton comments. Use those as the spec for what each test should assert.

The pattern tests probably need a small fixture-builder helper for "scope with N skills installed" — write this in `cmd_orchestration.rs` as a local helper or lift to `tests/common/` if there's an analogous one in tau-cli. (tau-cli also has a `tests/common/mod.rs` — check Skills-3's T6 for how integration tests synthesized the lockfile fixture.)

- [ ] **Step 3: Run all 5 pattern tests**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t9 cargo nextest run -p tau-cli --test cmd_orchestration 2>&1 | tail -15
```

Expected: 5 tests pass (was 0/5 ignored).

- [ ] **Step 4: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl add crates/tau-cli/tests/cmd_orchestration.rs
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl commit --no-verify -m "$(cat <<'EOF'
test(cli/orchestration): un-ignore 5 pattern test skeletons

Skills-4 bonus per spec D3. The 5 #[ignore]'d skeletons from PR #59
(multi-agent orchestration) were waiting on the multi-turn
MockLlmBackend fixture. Skills-4's T7 built that fixture; this
commit wires each pattern e2e.

Patterns un-ignored:
- linear (1 parent → 1 worker)
- worker-pool (1 parent → N workers in parallel)
- plan-revise (parent iterates: plan → spawn → review)
- supervisor-critic (parent + critic feedback loop)
- hierarchical (parent → sub-parent → leaf workers)

Each test uses MockLlmBackend's scripted turns to drive both parent
and child runs.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: ADR-0028

**Files:**
- Create: `docs/decisions/0028-skills-runtime-invocation.md`

- [ ] **Step 1: Write the ADR**

```bash
ls /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl/docs/decisions/ | grep "^002[8]"
```

If 0028 is taken, increment. Otherwise create `docs/decisions/0028-skills-runtime-invocation.md`:

```markdown
# ADR-0028 — Skills runtime invocation (Skills-4)

**Status:** Accepted 2026-05-14.
**Branch / PR:** `feat/skills-4-runtime-invocation` (PR pending).
**Spec:** `docs/superpowers/specs/2026-05-14-skills-4-runtime-invocation-design.md` (merged at #69).
**Plan:** `docs/superpowers/plans/2026-05-14-skills-4-runtime-invocation.md`.
**Depends on:** ADR-0025 (Skills-1), ADR-0026 (Skills-2), ADR-0027 (Skills-3).

## Context

Fourth of 6 sub-projects from ROADMAP §16 (Skills as first-class
packages, Constitution G10). The runtime piece that makes installed
skills usable by agents.

When an agent emits `skill.<name>.spawn`, the runtime resolves
`<name>` to an installed skill, builds a child agent run from the
skill's declared system_prompt + capabilities (with `${SKILL_DIR}`
substituted + optional caller `scope_paths` narrowing), and
recursively invokes `Runtime::run_with_history` via the v1.1
agent-spawn machinery shipped in PR #60.

## Decision

Three locked decisions (from brainstorming):

### D1: Separate URI namespace + capability variant

`skill.<name>.spawn` parallel to existing `agent.<kind>.spawn`. New
`Capability::Skill(SkillCapability::Spawn { allowed_skills })`.
TOML form: `kind = "skill.spawn"`. No namespace collision possible
between custom kinds and skill names.

### D2: Caller `scope_paths: Option<Vec<String>>` narrows fs.* paths only

Per-kind intersection; hard-fail on uncovered `scope_path` (typo
detection); non-fs capabilities pass through unchanged. Caller
cannot add new capabilities or change capability kinds — skill
author owns the contract; caller can tighten scope only.

### D3: Full multi-turn `MockLlmBackend` test fixture in this PR

Lifts the existing `ScriptedLlm` pattern from `run_with_tool_calls.rs`
into reusable `crates/tau-runtime/tests/common/mock_llm.rs`. Total
estimate 5-6 days (adds 2-3 days for fixture vs ~3 day core impl).
Bonus: unblocks 5 `#[ignore]`'d pattern test skeletons from PR #59
— un-ignored in this PR.

## Alternatives considered

- **Reuse `agent.<kind>.spawn` URI for skills:** namespace collision
  with custom kinds; capability conflation.
- **Caller-supplied `grant: Vec<Capability>` override:** too much
  rope; precedence rules unnecessary surface.
- **No caller knob at all (Option X from brainstorm):** loses
  per-spawn narrowing use cases.
- **Defer MockLlmBackend to follow-up PR:** weaker Skills-4 coverage;
  fixture has to be built eventually anyway.

## Consequences

- `tau-domain` public surface grows by `SkillCapability` +
  `Capability::Skill` variant + `CapabilityShape::SkillSpawn`.
- `tau-pkg` public surface grows by `find_installed_skill` +
  `InstalledSkill` + `FindSkillError`.
- `tau-runtime` adds `skill_resolve` module + 5 OrchestrationError
  variants + `validate_skill_spawn` + `is_skill_spawn` branch in
  `run_streaming_inner`.
- `crates/tau-runtime/tests/common/mock_llm.rs` is the canonical
  multi-turn LLM test fixture — future test suites can reuse.
- 5 previously-`#[ignore]`'d pattern tests from PR #59 are now
  running, providing real e2e coverage of multi-agent orchestration
  patterns.
- No new external deps.
- No CI changes.

## Out of scope (deferred to Skills-5+)

- **Agent Skills spec export / import** → Skills-5
- **Reference skill packages + user docs** → Skills-6
- **Sub-skill `requires_skills` runtime enforcement** — advisory only
- **Body parse caching across spawns** — YAGNI
- **Caller-side capability merge (`grant_extend`)** — Add when use case surfaces

## References

- Spec: `docs/superpowers/specs/2026-05-14-skills-4-runtime-invocation-design.md`
- Plan: `docs/superpowers/plans/2026-05-14-skills-4-runtime-invocation.md`
- Skills-1 ADR: `docs/decisions/0025-skills-foundation.md`
- Skills-2 ADR: `docs/decisions/0026-skills-install-pipeline.md`
- Skills-3 ADR: `docs/decisions/0027-skills-discovery.md`
- Multi-agent v1.1 PR: #60 (recursive `agent.<kind>.spawn`)
- Multi-agent v1.2 PR: #61 (per-spawn `system_prompt`)
- ROADMAP §16
```

- [ ] **Step 2: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl add docs/decisions/0028-skills-runtime-invocation.md
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl commit --no-verify -m "$(cat <<'EOF'
docs(adr): ADR-0028 — Skills runtime invocation (Skills-4)

Accepted. Records D1 (separate URI + capability), D2 (scope_paths
narrowing only), D3 (MockLlmBackend bundled in this PR). 4 rejected
alternatives. Cross-references ADR-0025 through ADR-0027 + spec +
plan.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: USER GATE — push + open PR

**Files:** none modified.

- [ ] **Step 1: Pre-push verification**

```bash
timeout 30 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo fmt --all -- --check
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo clippy -p tau-domain -p tau-pkg -p tau-runtime -p tau-cli --all-targets --features serde -- -D warnings
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-domain --lib --features serde
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-pkg --lib
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-runtime
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-cli --test cmd_orchestration
```

If fmt fails, `cargo fmt --all` from inside the worktree and re-commit.

- [ ] **Step 2: Push**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl push --no-verify -u origin feat/skills-4-runtime-invocation 2>&1 | tail -5
```

- [ ] **Step 3: Open PR**

```bash
cd /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl
gh pr create --base main --title "feat(runtime): Skills-4 — runtime invocation (ROADMAP §16)" --body "$(cat <<'EOF'
## Summary

Fourth of 6 sub-projects from ROADMAP §16. Ships `skill.<name>.spawn` end-to-end: when an agent emits this virtual tool, the runtime resolves `<name>` to an installed skill, builds a child agent run from the skill's declared system_prompt + capabilities (with `${SKILL_DIR}` substituted + optional caller `scope_paths` narrowing), and recursively invokes the v1.1 `Runtime::run_with_history` spawn machinery.

Spec: `docs/superpowers/specs/2026-05-14-skills-4-runtime-invocation-design.md` (merged in #69)
Plan: `docs/superpowers/plans/2026-05-14-skills-4-runtime-invocation.md`
ADR: `docs/decisions/0028-skills-runtime-invocation.md`

## What's in the PR

- **`tau-domain`** — `Capability::Skill(SkillCapability::Spawn { allowed_skills })` variant. New `CapabilityShape::SkillSpawn`. TOML: `kind = "skill.spawn"`.
- **`tau-pkg`** — `find_installed_skill(scope, name)` + `InstalledSkill` struct.
- **`tau-runtime::orchestration::skill_resolve`** — `substitute_skill_dir`, `apply_scope_paths`, `resolve_skill_for_spawn`, `SkillSpawnRequest`.
- **`tau-runtime::orchestration::virtual_tools`** — `validate_skill_spawn`, `skill.<name>.spawn` recognition.
- **`tau-runtime::stream::run_streaming_inner`** — `is_skill_spawn` branch parallel to v1.1's `is_agent_spawn`.
- **5 new `OrchestrationError` variants** for skill resolution paths.
- **`crates/tau-runtime/tests/common/mock_llm.rs`** — multi-turn `MockLlmBackend` fixture (D3).
- **5 `#[ignore]`'d pattern tests un-ignored** in `cmd_orchestration.rs` (D3 bonus).
- **ADR-0028**.

## Test coverage

- ~7 unit tests in `skill_resolve` (substitution + scope narrowing + lookup edge cases)
- ~3 unit tests in `tau-pkg::skill_resolve` (find_installed_skill)
- ~3 unit tests in `tau-domain` (Capability::Skill round-trip)
- ~3 self-tests for `MockLlmBackend` fixture
- 6 e2e tests in `tests/skill_spawn_e2e.rs`
- 5 newly un-ignored pattern tests in `cmd_orchestration.rs`

~27 new tests across the touched crates.

## v1 deferrals (per spec)

- Agent Skills spec export/import → Skills-5
- Reference skill packages → Skills-6
- Sub-skill `requires_skills` runtime enforcement → future tightening
- Body parse caching → YAGNI

## Test plan
- [ ] CI green on all 19 required checks

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

PAUSE for the user to confirm CI is green and approve the squash-merge.

- [ ] **Step 4: On user approval, squash-merge + cleanup worktree**

```bash
gh pr merge $(gh pr view --json number -q .number) --squash --delete-branch
cd /Users/titouanlebocq/code/tau
git fetch origin --quiet
git -C /Users/titouanlebocq/code/tau worktree remove /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl
git -C /Users/titouanlebocq/code/tau branch -D feat/skills-4-runtime-invocation 2>&1
```

---

## Self-review checklist

- **Spec coverage:**
  - D1 (separate URI + capability) → T1 (tau-domain) + T5 (virtual_tools)
  - D2 (scope_paths narrowing-only) → T3 (skill_resolve::apply_scope_paths)
  - D3 (MockLlmBackend bundled) → T7 (fixture) + T8 (e2e) + T9 (un-ignore patterns)
  - 5 new InstallError variants — wait, the spec says OrchestrationError variants → T4
  - find_installed_skill helper → T2
  - is_skill_spawn branch in stream.rs → T6
  - ADR → T10
- **Placeholder scan:** none — every step has complete code or exact commands.
- **Type consistency:** `SkillCapability`, `Capability::Skill`, `SkillSpawnRequest`, `SkillSpawnArgs`, `InstalledSkill`, `FindSkillError`, `substitute_skill_dir`, `apply_scope_paths`, `resolve_skill_for_spawn`, `validate_skill_spawn`, `is_skill_spawn` — all names match across tasks.
- **CLAUDE.md cargo rules:** every cargo invocation includes `timeout` + `CARGO_INCREMENTAL=0` + `CARGO_TARGET_DIR=target/<role>` + `-p <crate>`.
- **CLAUDE.md push rules:** T11 uses `git push --no-verify` from inside the worktree via `git -C`.
- **Multi-session safety:** every git operation uses `git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-4-impl ...`. Cleanup in T11 removes the worktree after merge.
- **No new external deps.**
