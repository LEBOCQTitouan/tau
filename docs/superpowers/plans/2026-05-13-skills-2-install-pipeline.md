# Skills-2 Install Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire `tau install <skill-pkg>` end-to-end through `tau-pkg` so a skill package fetches, validates SKILL.md content + frontmatter, resolves transitive deps, computes a content SHA-256 + caches frontmatter, and writes to the lockfile. Ships the install-time validation that makes "the skill actually got installed correctly" provable.

**Architecture:** New `tau-pkg::skill_check` module mirrors the existing `tau-pkg::sandbox_check`. `install_with_options` dispatches on `kind = "skill"`: skips Layer 2 sandbox cross-check, calls `skill_check::cross_check_skill_package`, computes content_sha256, caches frontmatter into a new `LockedSkill` lockfile entry. Lockfile schema bumps v4 → v5 with the standard `was_pre_vN` + `tracing::warn` once-per-process auto-upgrade pattern. `tau verify` gains a `SkillContentDrift` report variant parallel to `BinaryDrift`. tau-domain rejects packages combining `kind = "skill"` with a `[plugin]` block at parse time.

**Tech Stack:** Rust 2021. `sha2` (already a workspace dep — used for `tree_hash`). `serde` (existing). `tracing` (existing). `insta` (existing).

**Branch:** `feat/skills-2-install-pipeline` (already cut from main `1d71032`).
**Spec:** `docs/superpowers/specs/2026-05-12-skills-2-install-pipeline-design.md` (merged at `0b4f981`).
**Depends on:** Skills-1 (`1d71032`) — `SkillManifest`, `SkillFrontmatter`, `SkillContent`, `parse_skill_md`, `SKILL_DIR_VAR`, `PackageManifest::skill()`.

**CLAUDE.md rules in effect:**
- Every cargo invocation: `timeout <secs> env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/<role> cargo <cmd> -p <crate>`.
- Push via `git push --no-verify` (lefthook deep gate hits pre-existing `cmd_chat` flake; CI is authoritative).
- `cargo-deny` is active — no new external deps in this plan.

---

## File structure

| Path | Status | Responsibility |
|---|---|---|
| `crates/tau-domain/src/package/manifest.rs` | Modify | Add `PackageManifestError::SkillCannotHavePluginBlock`. `UncheckedManifest::validate` rejects `kind = "skill"` packages that also carry a `[plugin]` block. |
| `crates/tau-pkg/src/error.rs` | Modify | Add 4 new variants to `InstallError`: `SkillContentMissing`, `SkillNameMismatch`, `SkillFrontmatterInvalid`, `SkillReferenceWithoutCapability`. |
| `crates/tau-pkg/src/skill_check.rs` | Create | New module. Single entry: `cross_check_skill_package(install_dir, manifest)`. 4-step flow (read SKILL.md → parse → name match → reference lint). 6 unit tests. |
| `crates/tau-pkg/src/lib.rs` | Modify | `pub mod skill_check;` + re-export `cross_check_skill_package`. |
| `crates/tau-pkg/src/lockfile.rs` | Modify | Bump `MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION = 5`. Add `LockedSkill { content_sha256: String, frontmatter: SkillFrontmatterSnapshot }`. Add `LockedPackage.skill: Option<LockedSkill>` field. Auto-upgrade logic for v4 → v5 (warn once per process when `was_pre_v5`). Add `SkillFrontmatterSnapshot { name, description }` type. |
| `crates/tau-pkg/src/install.rs` | Modify | Dispatch on `kind`: when `"skill"`, skip Layer 2 sandbox cross-check, call `skill_check::cross_check_skill_package`, compute SKILL.md SHA-256, build `LockedSkill` entry, write to lockfile. |
| `crates/tau-pkg/src/verify.rs` | Modify | Add `VerifyReport::SkillContentDrift { name, expected, got }`. Verify logic re-hashes SKILL.md against the cached `content_sha256`. 2 new tests. |
| `crates/tau-pkg/tests/fixtures/skills/critic/` | Create | Minimal critic skill fixture: `tau.toml` + `SKILL.md` + `references/style-guide.md`. Reused by integration tests in T7. |
| `crates/tau-pkg/tests/install_skill_cross_check.rs` | Create | 4 integration tests covering install happy path + 3 InstallError variants. |
| `crates/tau-pkg/tests/verify_skill_drift.rs` | Create | 2 verify-time tests for `SkillContentDrift`. |
| `crates/tau-cli/src/cmd/error_render.rs` | Modify | 4 new render branches mirroring `render_cross_check_error`. |
| `crates/tau-cli/tests/cmd_install_skill_render.rs` | Create | 4 insta snapshot tests for the new error variants. |
| `docs/decisions/0026-skills-install-pipeline.md` | Create | ADR documenting Skills-2 (hard-fail reference lint, lockfile v5 migration, install-time validation pipeline). |

---

## Task 1: `tau-domain` — reject `kind = "skill"` with `[plugin]` block

**Files:**
- Modify: `crates/tau-domain/src/package/manifest.rs`

- [ ] **Step 1: Write the failing test**

In `crates/tau-domain/src/package/manifest.rs`, find the `#[cfg(test)] mod validation_tests` block. After the existing skill round-trip tests (from Skills-1), add:

```rust
    #[cfg(feature = "serde")]
    #[test]
    fn skill_kind_with_plugin_block_is_rejected() {
        // Skills-2: cross-field validation — a package declaring kind = "skill"
        // must NOT also carry a [plugin] table.
        let toml_src = r#"
name = "critic"
version = "0.1.0"
description = "Reviews drafts."
authors = []
source = "https://example.com/critic.git"
kind = "skill"
dependencies = []
capabilities = []

[plugin]
bin = "critic"
protocol = "1"

[skill]
"#;
        let u: UncheckedManifest = toml::from_str(toml_src).expect("parse");
        let err = u.validate().unwrap_err();
        assert!(
            matches!(err, PackageManifestError::SkillCannotHavePluginBlock),
            "expected SkillCannotHavePluginBlock, got {err:?}"
        );
    }
```

- [ ] **Step 2: Run test, see it fail**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t1 cargo nextest run -p tau-domain --lib --features serde -E 'test(skill_kind_with_plugin_block)' 2>&1 | tail -10
```

Expected: compile error — `SkillCannotHavePluginBlock` variant doesn't exist.

- [ ] **Step 3: Add the error variant**

In `crates/tau-domain/src/package/manifest.rs`, find the `pub enum PackageManifestError`. Add the new variant alphabetically (or at the end if the order is by additive time):

```rust
    /// A package declaring `kind = "skill"` must not also carry a
    /// `[plugin]` table. Plugins and skills are mutually exclusive
    /// package kinds — a "tool that ships its own usage doc as a skill"
    /// is a future Skills-5 concern (composable manifests); v1 keeps
    /// the two kinds separate.
    #[error("kind = \"skill\" packages cannot have a [plugin] block")]
    SkillCannotHavePluginBlock,
```

- [ ] **Step 4: Add the validate() check**

Find the `pub fn validate(self) -> Result<PackageManifest, PackageManifestError>` impl. After the existing capability-name-emptiness check (the `for (i, cap) in self.capabilities.iter().enumerate()` loop), and before the final `Ok(PackageManifest::from_checked(self))`:

```rust
        // Skills-2: kind = "skill" rejects [plugin] block.
        if matches!(&self.kind, PackageKind::Custom { kind } if kind == kinds::SKILL)
            && self.plugin.is_some()
        {
            return Err(PackageManifestError::SkillCannotHavePluginBlock);
        }
```

If `kinds::SKILL` isn't already imported in scope, add `use crate::package::manifest::kinds;` at the top of the file (it's likely already imported since `kinds` is in the same module — verify by reading the file).

- [ ] **Step 5: Run test, see it pass**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t1 cargo nextest run -p tau-domain --lib --features serde -E 'test(skill_kind_with_plugin_block)' 2>&1 | tail -10
```

Expected: 1 test passes.

- [ ] **Step 6: Verify no other tests broke**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t1 cargo nextest run -p tau-domain --lib --features serde 2>&1 | tail -5
```

Expected: 91/91 tests pass (was 90 before; +1 new).

- [ ] **Step 7: Commit**

```bash
git add crates/tau-domain/src/package/manifest.rs
git commit --no-verify -m "$(cat <<'EOF'
feat(domain): reject kind="skill" packages with [plugin] block

Skills-2 cross-field validation. Per ADR-0026 (pending), a package
declaring kind = "skill" must NOT also carry a [plugin] table.
Plugins and skills are mutually exclusive package kinds.

New PackageManifestError::SkillCannotHavePluginBlock variant.
UncheckedManifest::validate enforces the constraint after the
existing capability-name-emptiness check.

1 unit test.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `tau-pkg` — 4 new `InstallError` variants

**Files:**
- Modify: `crates/tau-pkg/src/error.rs`

- [ ] **Step 1: Locate the `InstallError` enum**

```bash
grep -nE "pub enum InstallError" /Users/titouanlebocq/code/tau/crates/tau-pkg/src/error.rs
```

- [ ] **Step 2: Add the 4 new variants**

After the last existing variant in `pub enum InstallError`, add:

```rust
    /// Skills-2: `kind = "skill"` package's `SKILL.md` is absent at install time.
    ///
    /// Path comes from `manifest.skill().unwrap().content` (default
    /// `"SKILL.md"`) joined with the install directory.
    #[error("skill {name:?}: SKILL.md not found at {expected_path:?}")]
    SkillContentMissing {
        /// Skill package name (for human-readable error).
        name: String,
        /// Absolute path the install pipeline tried to read.
        expected_path: std::path::PathBuf,
    },

    /// Skills-2: SKILL.md frontmatter's `name` field does not match the
    /// package's tau.toml `name`. Both must equal.
    #[error("skill name mismatch: tau.toml says {tau_toml:?}, SKILL.md frontmatter says {skill_md:?}")]
    SkillNameMismatch {
        /// `name` field from tau.toml.
        tau_toml: String,
        /// `name` field from SKILL.md frontmatter.
        skill_md: String,
    },

    /// Skills-2: SKILL.md frontmatter failed to parse, or is missing
    /// the required `name` / `description` fields.
    #[error("skill frontmatter invalid: {detail}")]
    SkillFrontmatterInvalid {
        /// Human-readable reason (e.g. "missing required field `name`",
        /// "YAML parse error: ...").
        detail: String,
    },

    /// Skills-2: SKILL.md body contains `${SKILL_DIR}/<rel-path>`
    /// references but no `[[capabilities]] kind = "fs.read"` glob
    /// covers the path. Hard-fail because runtime would fail with a
    /// confusing capability-denied error mid-task; install time is
    /// the right gate.
    #[error("skill references {reference:?} but no fs.read capability covers it (declared paths: {declared_paths:?})")]
    SkillReferenceWithoutCapability {
        /// The offending `${SKILL_DIR}/<path>` reference found in the body.
        reference: String,
        /// `fs.read` glob entries declared in the manifest at install time
        /// (so the user can see what to extend).
        declared_paths: Vec<String>,
    },
```

- [ ] **Step 3: Verify compile**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t2 cargo check -p tau-pkg 2>&1 | tail -5
```

Expected: `Finished dev profile ...`. No new warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-pkg/src/error.rs
git commit --no-verify -m "$(cat <<'EOF'
feat(pkg/error): 4 new InstallError variants for Skills-2

Additive extension to InstallError covering the four failure modes
of the Skills-2 install-time validation pipeline:

- SkillContentMissing: SKILL.md absent at the install path
- SkillNameMismatch: tau.toml name diverges from frontmatter name
- SkillFrontmatterInvalid: YAML parse or missing required fields
- SkillReferenceWithoutCapability: body references ${SKILL_DIR}/<path>
  without a covering fs.read glob (hard-fail; caught at install rather
  than mid-task at runtime)

All 4 are #[non_exhaustive]-friendly additions to the existing
#[non_exhaustive] InstallError enum.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `tau-pkg::skill_check` module

**Files:**
- Create: `crates/tau-pkg/src/skill_check.rs`
- Modify: `crates/tau-pkg/src/lib.rs`

- [ ] **Step 1: Wire the module into `lib.rs`**

In `crates/tau-pkg/src/lib.rs`, add (alphabetically with existing `pub mod` declarations):

```rust
pub mod skill_check;
```

And add a re-export with the other `pub use` lines:

```rust
pub use skill_check::cross_check_skill_package;
```

- [ ] **Step 2: Create `skill_check.rs` with the module skeleton + 6 failing tests**

Create `crates/tau-pkg/src/skill_check.rs`:

```rust
//! Install-time cross-check for `kind = "skill"` packages.
//!
//! Skills-2 sub-project from ROADMAP §16. Mirrors the existing
//! `tau-pkg::sandbox_check` pattern: a single entry point invoked
//! from `install_with_options` between manifest validation and
//! lockfile write. Parses the package's `SKILL.md`, validates that
//! its frontmatter name matches `tau.toml`, and hard-fails if the
//! body references `${SKILL_DIR}/<path>` files without a covering
//! `fs.read` capability.
//!
//! See ADR-0026 and
//! `docs/superpowers/specs/2026-05-12-skills-2-install-pipeline-design.md`.

use std::path::Path;

use globset::GlobBuilder;
use tau_domain::{parse_skill_md, Capability, FsCapability, PackageManifest, SkillContentError};

use crate::error::InstallError;

/// Substring scan target: every reference like `${SKILL_DIR}/<rel-path>`
/// in the SKILL.md body must be covered by a `[[capabilities]] kind = "fs.read"`
/// entry whose `paths` glob matches the resolved path. We match the
/// substring conservatively: `${SKILL_DIR}/` followed by `[A-Za-z0-9_\-./*]+`.
///
/// Markdown link syntax, inline code, and prose mention all match
/// equivalently.
const SKILL_DIR_PREFIX: &str = "${SKILL_DIR}/";

/// Cross-check a skill package's installed directory against its
/// `tau.toml` manifest. Called from `install_with_options` when
/// `manifest.kind() == PackageKind::Custom { kind: "skill" }`.
///
/// 4-step flow:
///
/// 1. Read `SKILL.md` from `install_dir/<content_path>`.
/// 2. Parse via `tau_domain::parse_skill_md`.
/// 3. Validate `frontmatter.name == manifest.name()`.
/// 4. Reference lint (hard-fail): every `${SKILL_DIR}/<rel-path>` in
///    the body must have a covering `fs.read` glob.
pub fn cross_check_skill_package(
    install_dir: &Path,
    manifest: &PackageManifest,
) -> Result<(), InstallError> {
    let skill = manifest
        .skill()
        .expect("cross_check_skill_package called on non-skill package — caller must dispatch on kind");

    // Step 1: read SKILL.md
    let content_path = install_dir.join(&skill.content);
    let body_text = match std::fs::read_to_string(&content_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(InstallError::SkillContentMissing {
                name: manifest.name().as_str().to_string(),
                expected_path: content_path,
            });
        }
        Err(e) => {
            return Err(InstallError::Io {
                message: format!("reading SKILL.md at {content_path:?}: {e}"),
            });
        }
    };

    // Step 2: parse
    let parsed = parse_skill_md(&body_text).map_err(|e| {
        let detail = match &e {
            SkillContentError::MissingFrontmatterOpener => {
                "missing leading `---` frontmatter delimiter".to_string()
            }
            SkillContentError::MissingFrontmatterCloser => {
                "missing closing `---` frontmatter delimiter".to_string()
            }
            SkillContentError::YamlParse(msg) => format!("YAML parse error: {msg}"),
            SkillContentError::MissingName => "missing required field `name`".to_string(),
            SkillContentError::MissingDescription => {
                "missing required field `description`".to_string()
            }
            _ => format!("{e}"),
        };
        InstallError::SkillFrontmatterInvalid { detail }
    })?;

    // Step 3: name match
    if parsed.frontmatter.name != manifest.name().as_str() {
        return Err(InstallError::SkillNameMismatch {
            tau_toml: manifest.name().as_str().to_string(),
            skill_md: parsed.frontmatter.name,
        });
    }

    // Step 4: reference lint (hard-fail)
    let fs_read_paths: Vec<&str> = manifest
        .capabilities()
        .iter()
        .filter_map(|c| match c {
            Capability::Filesystem(FsCapability::Read { paths }) => Some(paths.as_slice()),
            _ => None,
        })
        .flatten()
        .map(String::as_str)
        .collect();

    for reference in scan_skill_dir_references(&parsed.body) {
        if !is_reference_covered(&reference, &fs_read_paths) {
            return Err(InstallError::SkillReferenceWithoutCapability {
                reference,
                declared_paths: fs_read_paths.iter().map(|s| s.to_string()).collect(),
            });
        }
    }

    Ok(())
}

/// Extract every distinct `${SKILL_DIR}/<rel-path>` substring from the
/// body text. Conservative scan — matches `${SKILL_DIR}/` followed by
/// any sequence of path-friendly chars (letters, digits, `_`, `-`, `.`,
/// `/`, `*`). Stops at whitespace, quote, backtick, or angle bracket.
fn scan_skill_dir_references(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut search_from = 0usize;
    while let Some(idx) = body[search_from..].find(SKILL_DIR_PREFIX) {
        let start = search_from + idx;
        let mut end = start + SKILL_DIR_PREFIX.len();
        for (i, ch) in body[end..].char_indices() {
            if ch.is_ascii_alphanumeric()
                || matches!(ch, '_' | '-' | '.' | '/' | '*')
            {
                end = start + SKILL_DIR_PREFIX.len() + i + ch.len_utf8();
            } else {
                break;
            }
        }
        // end now points just past the last accepted char.
        let reference = body[start..end].to_string();
        if !out.contains(&reference) {
            out.push(reference);
        }
        search_from = end;
    }
    out
}

/// Does `reference` (e.g. `"${SKILL_DIR}/references/foo.md"`) match
/// any of the `${SKILL_DIR}/...` globs declared in `fs_read_paths`?
fn is_reference_covered(reference: &str, fs_read_paths: &[&str]) -> bool {
    for glob_str in fs_read_paths {
        // Only consider globs that themselves start with ${SKILL_DIR}.
        if !glob_str.starts_with(SKILL_DIR_PREFIX) {
            continue;
        }
        match GlobBuilder::new(glob_str).literal_separator(false).build() {
            Ok(g) => {
                if g.compile_matcher().is_match(reference) {
                    return true;
                }
            }
            Err(_) => continue,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_skill_md(dir: &Path, body: &str) {
        fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    /// Build a `PackageManifest` from a TOML string. The TOML must
    /// include the new `[skill]` block to exercise the skill code path.
    fn manifest_from_toml(toml_src: &str) -> PackageManifest {
        let u: tau_domain::UncheckedManifest = toml::from_str(toml_src).expect("parse");
        u.validate().expect("validate")
    }

    fn good_manifest_toml() -> &'static str {
        r#"
name = "critic"
version = "0.1.0"
description = "Reviews drafts."
authors = []
source = "https://example.com/critic.git"
kind = "skill"
dependencies = []
capabilities = []

[skill]
"#
    }

    #[test]
    fn happy_path_returns_ok() {
        let dir = tempdir().unwrap();
        write_skill_md(
            dir.path(),
            "---\nname: critic\ndescription: Reviews drafts.\n---\nbody\n",
        );
        let manifest = manifest_from_toml(good_manifest_toml());
        cross_check_skill_package(dir.path(), &manifest).unwrap();
    }

    #[test]
    fn returns_content_missing_when_skill_md_absent() {
        let dir = tempdir().unwrap();
        // Deliberately do NOT write SKILL.md.
        let manifest = manifest_from_toml(good_manifest_toml());
        let err = cross_check_skill_package(dir.path(), &manifest).unwrap_err();
        match err {
            InstallError::SkillContentMissing { name, .. } => {
                assert_eq!(name, "critic");
            }
            other => panic!("expected SkillContentMissing, got {other:?}"),
        }
    }

    #[test]
    fn returns_frontmatter_invalid_on_malformed_yaml() {
        let dir = tempdir().unwrap();
        write_skill_md(
            dir.path(),
            "---\nname: critic\ndescription: : :\n  - bad indent\n---\nbody\n",
        );
        let manifest = manifest_from_toml(good_manifest_toml());
        let err = cross_check_skill_package(dir.path(), &manifest).unwrap_err();
        assert!(
            matches!(err, InstallError::SkillFrontmatterInvalid { .. }),
            "expected SkillFrontmatterInvalid, got {err:?}"
        );
    }

    #[test]
    fn returns_name_mismatch_when_diverged() {
        let dir = tempdir().unwrap();
        write_skill_md(
            dir.path(),
            "---\nname: kritic\ndescription: typo.\n---\nbody\n",
        );
        let manifest = manifest_from_toml(good_manifest_toml());
        let err = cross_check_skill_package(dir.path(), &manifest).unwrap_err();
        match err {
            InstallError::SkillNameMismatch { tau_toml, skill_md } => {
                assert_eq!(tau_toml, "critic");
                assert_eq!(skill_md, "kritic");
            }
            other => panic!("expected SkillNameMismatch, got {other:?}"),
        }
    }

    #[test]
    fn returns_reference_without_capability_when_body_refs_uncovered_path() {
        // SKILL.md references ${SKILL_DIR}/refs/foo.md but manifest grants
        // fs.read only on ${SKILL_DIR}/templates/**.
        let dir = tempdir().unwrap();
        write_skill_md(
            dir.path(),
            "---\nname: critic\ndescription: x\n---\nSee ${SKILL_DIR}/refs/foo.md for context.\n",
        );
        let manifest = manifest_from_toml(
            r#"
name = "critic"
version = "0.1.0"
description = "Reviews drafts."
authors = []
source = "https://example.com/critic.git"
kind = "skill"
dependencies = []

[[capabilities]]
kind = "fs.read"
paths = ["${SKILL_DIR}/templates/**"]

[skill]
"#,
        );
        let err = cross_check_skill_package(dir.path(), &manifest).unwrap_err();
        match err {
            InstallError::SkillReferenceWithoutCapability {
                reference,
                declared_paths,
            } => {
                assert!(reference.contains("refs/foo.md"));
                assert_eq!(declared_paths, vec!["${SKILL_DIR}/templates/**".to_string()]);
            }
            other => panic!("expected SkillReferenceWithoutCapability, got {other:?}"),
        }
    }

    #[test]
    fn accepts_reference_covered_by_fs_read_glob() {
        let dir = tempdir().unwrap();
        write_skill_md(
            dir.path(),
            "---\nname: critic\ndescription: x\n---\nSee ${SKILL_DIR}/refs/foo.md for context.\n",
        );
        let manifest = manifest_from_toml(
            r#"
name = "critic"
version = "0.1.0"
description = "Reviews drafts."
authors = []
source = "https://example.com/critic.git"
kind = "skill"
dependencies = []

[[capabilities]]
kind = "fs.read"
paths = ["${SKILL_DIR}/refs/**"]

[skill]
"#,
        );
        cross_check_skill_package(dir.path(), &manifest).unwrap();
    }
}
```

- [ ] **Step 3: Run the 6 tests**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t3 cargo nextest run -p tau-pkg --lib skill_check 2>&1 | tail -15
```

Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-pkg/src/skill_check.rs crates/tau-pkg/src/lib.rs
git commit --no-verify -m "$(cat <<'EOF'
feat(pkg/skill_check): new module + 6 unit tests

Mirrors the existing tau-pkg::sandbox_check pattern. Single entry
point cross_check_skill_package(install_dir, manifest) performs the
4-step Skills-2 install-time validation:

  1. Read SKILL.md from install_dir/<content_path>
  2. Parse via tau_domain::parse_skill_md (Skills-1)
  3. Validate frontmatter.name == manifest.name
  4. Reference lint (HARD-FAIL): every ${SKILL_DIR}/<rel-path>
     substring in body must be covered by an fs.read glob

Reference scanner uses a conservative substring match; glob coverage
check uses globset (already a tau-pkg dep). Hard-fail rationale: the
false-positive cost is one [[capabilities]] line; the true-positive
cost is severe (agent capability-denied mid-task with no clear
remediation from the runtime error alone).

6 unit tests cover happy path + 4 error variants + covered-reference
glob match.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Lockfile schema v4 → v5 + `LockedSkill` type

**Files:**
- Modify: `crates/tau-pkg/src/lockfile.rs`

- [ ] **Step 1: Bump the schema version constant**

In `crates/tau-pkg/src/lockfile.rs`, find:

```rust
pub const MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION: u32 = 4;
```

Change to:

```rust
pub const MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION: u32 = 5;
```

Also update the corresponding doctest assertion (around line 66) from `assert_eq!(lf.schema_version, 4);` to `assert_eq!(lf.schema_version, 5);`.

- [ ] **Step 2: Add `SkillFrontmatterSnapshot` and `LockedSkill` types**

Find the `LockedPlugin` type (around line 148-200). After the `impl LockedPlugin` block, add the new skill types:

```rust
/// Snapshot of `SKILL.md` frontmatter at install time. Lets
/// `tau skill list` and `tau skill show` (Skills-3) enumerate installed
/// skills without per-skill disk seeks. The body is NOT cached —
/// arbitrarily large; loaded lazily at spawn time by Skills-4.
///
/// Added in lockfile schema v5.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillFrontmatterSnapshot {
    /// Name field from SKILL.md frontmatter (matches tau.toml name —
    /// equality enforced at install time by skill_check).
    pub name: String,
    /// Short human-readable description.
    pub description: String,
}

/// Recorded install-time metadata for a `kind = "skill"` package.
///
/// Written by [`crate::install_with_options`] when the installed
/// package's manifest has `kind = "skill"` and the SKILL.md
/// validation in [`crate::skill_check`] passes. Consumed by:
/// - `tau verify` (this crate) — compares `content_sha256` against
///   the re-hashed SKILL.md to detect drift.
/// - Skills-3 (`tau skill list / show`) — reads `frontmatter` for
///   the summary view.
/// - Skills-4 (runtime invocation) — reads `frontmatter.name` for
///   resolution; reads the SKILL.md body on demand (NOT cached).
///
/// Added in lockfile schema v5.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedSkill {
    /// SHA-256 of the SKILL.md file bytes at install time. Hex
    /// encoded (lowercase). Empty for v4-leftover entries (informational
    /// `unverified` status from `tau verify`, not drift).
    pub content_sha256: String,

    /// Snapshot of SKILL.md frontmatter (name + description).
    pub frontmatter: SkillFrontmatterSnapshot,
}

impl LockedSkill {
    /// Construct a `LockedSkill`. `#[non_exhaustive]`; external callers
    /// (notably test synthesis) use this constructor.
    pub fn new(content_sha256: String, frontmatter: SkillFrontmatterSnapshot) -> Self {
        Self {
            content_sha256,
            frontmatter,
        }
    }
}
```

- [ ] **Step 3: Add the `skill` field to `LockedPackage`**

Find the `pub struct LockedPackage` definition (around line 109). After the existing `plugin: Option<LockedPlugin>` field, add:

```rust
    /// Skill metadata recorded at install time. `None` for non-skill
    /// packages and for legacy v4 lockfile entries (which had no
    /// `skill` field; `#[serde(default)]` populates it as `None` on
    /// auto-upgrade).
    ///
    /// Added in lockfile schema v5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<LockedSkill>,
```

- [ ] **Step 4: Extend the auto-upgrade logic**

Find the existing auto-upgrade block (around line 343):

```rust
        let was_pre_v4 = parsed.schema_version < 4;
```

Replace the entire migration logic block (lines ~343-358) with:

```rust
        // Schema migrations — additive. Each `was_pre_vN` flag captures
        // a lockfile that needs an additive field populated to a
        // sensible default. `serde(default)` handles the in-memory
        // population; this block emits the once-per-process warnings
        // and bumps the recorded schema_version so the next save()
        // writes the current version.
        let was_pre_v4 = parsed.schema_version < 4;
        let was_pre_v5 = parsed.schema_version < 5;
        if parsed.schema_version < MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION {
            parsed.schema_version = MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION;
        }

        // v3 → v4 migration: `required_shapes` empty on plugin entries.
        if was_pre_v4 {
            for pkg in &parsed.packages {
                if let Some(plugin) = &pkg.plugin {
                    if plugin.required_shapes.is_empty() {
                        warn_missing_required_shapes(&plugin.manifest.bin);
                    }
                }
            }
        }

        // v4 → v5 migration: skill entries are absent on legacy
        // lockfiles. Emit a once-per-process warn for any skill
        // package detected by `kind = "skill"` but missing the
        // `skill` field — Skills-3 will see `frontmatter` as None
        // and re-read SKILL.md on demand.
        if was_pre_v5 {
            for pkg in &parsed.packages {
                if pkg.skill.is_none()
                    && matches!(&pkg.source, _) // placeholder — see note below
                {
                    // Schema v4 entries have no kind discriminator on
                    // LockedPackage itself. The only way to know whether
                    // a v4 entry was a skill is to re-parse its tau.toml,
                    // which we don't do at lockfile load. Skills-3 will
                    // surface "unverified" status for v4 entries that
                    // ARE skills but missing the cached frontmatter.
                    let _ = pkg.name.as_str();
                }
            }
            warn_lockfile_pre_v5_once();
        }

        Ok(parsed)
    }
```

Then below the `warn_missing_required_shapes` helper function in the file, add:

```rust
/// Emit a once-per-process warning that the lockfile was auto-upgraded
/// from v4 to v5 (added `LockedSkill` field on `LockedPackage`). Any
/// skill packages installed before the upgrade will surface as
/// "unverified" via `tau verify` until reinstalled.
fn warn_lockfile_pre_v5_once() {
    use std::sync::Once;
    static WARN_ONCE: Once = Once::new();
    WARN_ONCE.call_once(|| {
        tracing::warn!(
            name = "tau_pkg.lockfile.v4_to_v5_auto_upgrade",
            "lockfile auto-upgraded from v4 to v5; skill packages installed before \
             the upgrade have no cached SKILL.md hash + frontmatter — \
             re-run `tau install <skill>` to refresh"
        );
    });
}
```

- [ ] **Step 5: Add a migration test**

In the existing `#[cfg(test)] mod tests` block in `lockfile.rs`, add:

```rust
    #[test]
    fn loads_v4_lockfile_with_skill_none_on_auto_upgrade() {
        // v4 lockfile (no `skill` field). On load, schema_version
        // bumps to v5 and pkg.skill is None.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tau.lock");
        let v4_text = r#"
schema_version = 4
generated_at = "2025-01-01T00:00:00Z"

[[packages]]
name = "regular-tool"
active_version = "0.1.0"
source = "https://example.com/tool.git"
versions = []
"#;
        std::fs::write(&path, v4_text).unwrap();
        let lf = LockFile::load(&path).unwrap();
        assert_eq!(lf.schema_version, MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION);
        assert_eq!(lf.packages.len(), 1);
        assert!(lf.packages[0].skill.is_none());
    }
```

- [ ] **Step 6: Run lockfile tests**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t4 cargo nextest run -p tau-pkg --lib lockfile 2>&1 | tail -10
```

Expected: existing lockfile tests + 1 new migration test all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/tau-pkg/src/lockfile.rs
git commit --no-verify -m "$(cat <<'EOF'
feat(pkg/lockfile): schema v4 → v5 + LockedSkill type

Skills-2 lockfile migration. Bumps MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION
to 5. Adds two new types:

- SkillFrontmatterSnapshot { name, description }: cached SKILL.md
  frontmatter for fast `tau skill list / show` enumeration without
  per-skill disk seeks.
- LockedSkill { content_sha256, frontmatter }: install-time skill
  metadata. content_sha256 is hex-encoded SHA-256 of SKILL.md bytes;
  drift detection via `tau verify` parallels binary_sha256 for plugin
  binaries.

LockedPackage gains `skill: Option<LockedSkill>` with serde-default
None — backwards-compatible with v4 lockfiles. Auto-upgrade emits a
once-per-process `tracing::warn!` so users know to re-run `tau install`
on legacy skill entries.

1 new unit test covers the v4 → v5 auto-upgrade path.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: `install_with_options` integration

**Files:**
- Modify: `crates/tau-pkg/src/install.rs`

- [ ] **Step 1: Locate the install dispatch + sandbox_check call**

```bash
grep -nE "sandbox_check|kind\(\)\s*==|PackageKind|LockedPackage|skill_check" /Users/titouanlebocq/code/tau/crates/tau-pkg/src/install.rs | head -20
```

This locates the current Layer 2 sandbox cross-check dispatch and the lockfile write site. The skill_check insertion goes in the same dispatch.

- [ ] **Step 2: Add kind-dispatch around the sandbox cross-check**

Find where `sandbox_check::cross_check_plugin_capabilities` is called. Wrap the existing call in a kind check, and add the skill branch:

```rust
// Determine whether this package is a plugin, a skill, or neither.
let kind_is_plugin = matches!(
    manifest.kind(),
    PackageKind::Custom { kind } if kind == kinds::PLUGIN
);
let kind_is_skill = matches!(
    manifest.kind(),
    PackageKind::Custom { kind } if kind == kinds::SKILL
);

// Layer 2 sandbox cross-check applies only to plugin packages
// (spawns the binary + diffs declared vs runtime capabilities).
// Skills have no plugin process; nothing to spawn.
if kind_is_plugin {
    sandbox_check::cross_check_plugin_capabilities(
        // ... existing args ...
    )?;
}

// Skills-2 cross-check: validates SKILL.md content + frontmatter +
// reference lint. Hard-fails on missing capabilities.
if kind_is_skill {
    skill_check::cross_check_skill_package(&install_dir, &manifest)?;
}
```

Adapt the existing args to whatever they currently are; the structure may need minor wrapping in an `if`.

**Implementer:** the exact pre-existing code may differ. Read the file first; preserve all existing behavior; insert the new `if kind_is_skill { ... }` branch immediately after the plugin branch (or wherever sandbox_check currently lives, restructured to dispatch on kind).

- [ ] **Step 3: After install, compute content_sha256 + build LockedSkill**

Find where the lockfile entry is constructed (the `LockedPackage { ... }` literal or builder call). For skill packages, populate the new `skill` field. Insert this code just before the `LockedPackage` build:

```rust
let locked_skill: Option<LockedSkill> = if kind_is_skill {
    // Compute SHA-256 of the SKILL.md file bytes.
    let content_path = install_dir.join(&manifest.skill().unwrap().content);
    let bytes = std::fs::read(&content_path).map_err(|e| InstallError::Io {
        message: format!("reading SKILL.md for sha256 at {content_path:?}: {e}"),
    })?;
    let content_sha256 = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };

    // Re-parse for the frontmatter snapshot. cross_check_skill_package
    // already validated; re-parsing here is a small cost to keep the
    // two concerns (validation vs snapshot) cleanly separated.
    let body_text = std::str::from_utf8(&bytes).map_err(|e| InstallError::Io {
        message: format!("SKILL.md is not UTF-8 at {content_path:?}: {e}"),
    })?;
    let parsed = tau_domain::parse_skill_md(body_text).map_err(|e| {
        // Should be unreachable — cross_check just validated — but
        // surface as Internal if it fires.
        InstallError::Internal {
            message: format!("post-validation SKILL.md re-parse failed: {e}"),
        }
    })?;

    Some(LockedSkill::new(
        content_sha256,
        SkillFrontmatterSnapshot {
            name: parsed.frontmatter.name,
            description: parsed.frontmatter.description,
        },
    ))
} else {
    None
};
```

Then thread `skill: locked_skill,` into the `LockedPackage { ... }` construction site.

- [ ] **Step 4: Verify `sha2` is a tau-pkg dep**

```bash
grep -n "sha2" /Users/titouanlebocq/code/tau/crates/tau-pkg/Cargo.toml
```

Expected: present (already used by `tree_hash`). If absent, add to `[dependencies]`:

```toml
sha2 = { workspace = true }
```

- [ ] **Step 5: Verify compile**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t5 cargo check -p tau-pkg 2>&1 | tail -10
```

Expected: clean compile. If any warnings about unused imports (e.g. `kinds::PLUGIN` not previously referenced), add the import.

- [ ] **Step 6: Verify existing tests still pass**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t5 cargo nextest run -p tau-pkg --lib 2>&1 | tail -5
```

Expected: all existing tau-pkg lib tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/tau-pkg/src/install.rs
git commit --no-verify -m "$(cat <<'EOF'
feat(pkg/install): wire skill_check into install_with_options

Adds kind-dispatch around the existing Layer 2 sandbox cross-check:
- kind = "plugin" → existing sandbox_check (unchanged)
- kind = "skill" → new skill_check::cross_check_skill_package
- other kinds → neither (existing behavior)

After cross-check passes for skill packages, compute SHA-256 of the
SKILL.md bytes + snapshot frontmatter (name, description) into a
LockedSkill, then thread through the lockfile entry's new `skill`
field (added in T4).

Layer 2 sandbox cross-check stays skipped for skills (they have no
plugin process to spawn).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: `tau verify` extension — `SkillContentDrift`

**Files:**
- Modify: `crates/tau-pkg/src/verify.rs`

- [ ] **Step 1: Locate the `VerifyReport` enum and the existing drift logic**

```bash
grep -nE "pub enum VerifyReport|BinaryDrift|binary_sha256" /Users/titouanlebocq/code/tau/crates/tau-pkg/src/verify.rs | head -10
```

- [ ] **Step 2: Add the `SkillContentDrift` variant**

In `crates/tau-pkg/src/verify.rs`, add the new variant to `pub enum VerifyReport`, parallel to `BinaryDrift`:

```rust
    /// A skill package's `SKILL.md` content hash differs from the
    /// install-time snapshot recorded in the lockfile. Parallel to
    /// `BinaryDrift` for plugin binaries.
    ///
    /// Remediation: re-run `tau install <skill>` to refresh.
    SkillContentDrift {
        /// Package name.
        name: String,
        /// Expected SHA-256 (hex; from the lockfile).
        expected: String,
        /// Actual SHA-256 (hex; from re-hashing the on-disk file).
        got: String,
    },
```

- [ ] **Step 3: Write the failing tests**

In `crates/tau-pkg/src/verify.rs`, locate the existing `#[cfg(test)] mod tests` (or the test module nearest to verify logic). Add the two tests. If the file has no test module yet, create one. If the file uses an external `tests/` dir for integration tests, see T7 — but unit-level coverage of the hash compare goes here:

```rust
#[cfg(test)]
mod skill_drift_tests {
    use super::*;
    use crate::lockfile::{LockedSkill, SkillFrontmatterSnapshot};
    use std::fs;
    use tempfile::tempdir;

    fn write_skill_md(dir: &std::path::Path, body: &str) -> String {
        // Returns the SHA-256 of the body.
        let path = dir.join("SKILL.md");
        fs::write(&path, body).unwrap();
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(body.as_bytes());
        format!("{:x}", h.finalize())
    }

    #[test]
    fn ok_when_skill_md_matches_locked_hash() {
        let dir = tempdir().unwrap();
        let body = "---\nname: critic\ndescription: x\n---\nbody\n";
        let sha = write_skill_md(dir.path(), body);
        let locked = LockedSkill::new(
            sha,
            SkillFrontmatterSnapshot {
                name: "critic".into(),
                description: "x".into(),
            },
        );
        let result = verify_skill_content(dir.path(), "critic", &locked);
        assert!(matches!(result, Ok(())), "expected Ok, got {result:?}");
    }

    #[test]
    fn drift_when_skill_md_modified_after_install() {
        let dir = tempdir().unwrap();
        let body = "---\nname: critic\ndescription: x\n---\nbody\n";
        let original_sha = write_skill_md(dir.path(), body);
        // Mutate SKILL.md after recording the snapshot.
        fs::write(
            dir.path().join("SKILL.md"),
            "---\nname: critic\ndescription: x\n---\nMUTATED\n",
        )
        .unwrap();
        let locked = LockedSkill::new(
            original_sha.clone(),
            SkillFrontmatterSnapshot {
                name: "critic".into(),
                description: "x".into(),
            },
        );
        let result = verify_skill_content(dir.path(), "critic", &locked);
        match result {
            Err(VerifyReport::SkillContentDrift { name, expected, got }) => {
                assert_eq!(name, "critic");
                assert_eq!(expected, original_sha);
                assert_ne!(expected, got);
            }
            other => panic!("expected SkillContentDrift, got {other:?}"),
        }
    }
}
```

- [ ] **Step 4: Add the `verify_skill_content` function**

In `crates/tau-pkg/src/verify.rs`, add a new public function:

```rust
/// Re-hash a skill package's `SKILL.md` and compare against the
/// `content_sha256` recorded in the lockfile. Returns `Ok(())` on
/// match; `Err(VerifyReport::SkillContentDrift { ... })` on mismatch.
///
/// Used by `tau verify` to detect post-install drift on skill
/// packages, parallel to how `binary_sha256` is checked for plugin
/// binaries.
///
/// `install_dir` is the absolute path of the installed skill
/// directory (i.e. where `SKILL.md` lives). `name` is the package
/// name (carried into the error for human-readable display).
pub fn verify_skill_content(
    install_dir: &std::path::Path,
    name: &str,
    locked: &crate::lockfile::LockedSkill,
) -> Result<(), VerifyReport> {
    let path = install_dir.join("SKILL.md");
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            // SKILL.md missing on disk despite being recorded → drift.
            // We could surface this as a separate variant
            // (`SkillContentMissing`), but the user remediation is the
            // same: re-install. Keep one variant.
            return Err(VerifyReport::SkillContentDrift {
                name: name.to_string(),
                expected: locked.content_sha256.clone(),
                got: "<missing>".to_string(),
            });
        }
    };
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(&bytes);
    let got = format!("{:x}", h.finalize());
    if got == locked.content_sha256 {
        Ok(())
    } else {
        Err(VerifyReport::SkillContentDrift {
            name: name.to_string(),
            expected: locked.content_sha256.clone(),
            got,
        })
    }
}
```

- [ ] **Step 5: Run tests**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t6 cargo nextest run -p tau-pkg --lib skill_drift 2>&1 | tail -8
```

Expected: 2 tests pass.

- [ ] **Step 6: Wire `verify_skill_content` into the existing `verify_package` entry**

Find the existing `pub fn verify_package` (or `verify_packages`, or however the top-level entry is named). After the existing `binary_sha256` check for plugins, add a parallel branch for skills:

```rust
if let Some(locked_skill) = &locked_package.skill {
    if let Err(drift_report) = verify_skill_content(&install_dir, locked_package.name.as_str(), locked_skill) {
        return drift_report; // or push into accumulator if the API returns a Vec<VerifyReport>
    }
}
```

The exact integration shape depends on the existing verify API. Read it first; match the pattern. If `verify_package` returns a single `VerifyReport`, return the drift report directly; if it returns a `Vec<VerifyReport>`, push.

- [ ] **Step 7: Commit**

```bash
git add crates/tau-pkg/src/verify.rs
git commit --no-verify -m "$(cat <<'EOF'
feat(pkg/verify): SkillContentDrift report variant + re-hash check

Skills-2 verify extension. Parallel to `BinaryDrift` for plugin
binaries: re-hashes the installed SKILL.md and compares against the
content_sha256 cached at install time (in LockedSkill, added in T4).

New verify_skill_content function reads SKILL.md, computes SHA-256,
compares to the lockfile snapshot. Missing SKILL.md surfaces as
SkillContentDrift with got = "<missing>" (same remediation as drift:
re-run `tau install`).

verify_package now dispatches on locked_package.skill to call
verify_skill_content for skill entries after the existing binary
hash check for plugin entries.

2 unit tests cover happy path + drift detection.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Integration tests with critic fixture

**Files:**
- Create: `crates/tau-pkg/tests/fixtures/skills/critic/tau.toml`
- Create: `crates/tau-pkg/tests/fixtures/skills/critic/SKILL.md`
- Create: `crates/tau-pkg/tests/fixtures/skills/critic/references/style-guide.md`
- Create: `crates/tau-pkg/tests/install_skill_cross_check.rs`

- [ ] **Step 1: Create the critic fixture directory tree**

```bash
mkdir -p /Users/titouanlebocq/code/tau/crates/tau-pkg/tests/fixtures/skills/critic/references
```

- [ ] **Step 2: Write `tau.toml`**

Create `crates/tau-pkg/tests/fixtures/skills/critic/tau.toml`:

```toml
name = "critic"
version = "0.1.0"
description = "Reviews drafts for unsourced claims."
authors = []
source = "https://example.com/critic.git"
kind = "skill"
dependencies = []

[[capabilities]]
kind = "fs.read"
paths = ["${SKILL_DIR}/references/**"]

[skill]
```

- [ ] **Step 3: Write `SKILL.md`**

Create `crates/tau-pkg/tests/fixtures/skills/critic/SKILL.md`:

```markdown
---
name: critic
description: Reviews drafts for unsourced claims.
---

# Critic

You are a strict editor. Flag every claim that lacks a source.

For style examples, see ${SKILL_DIR}/references/style-guide.md.
```

- [ ] **Step 4: Write `references/style-guide.md`**

Create `crates/tau-pkg/tests/fixtures/skills/critic/references/style-guide.md`:

```markdown
# Style guide

Bullet format: imperative mood. No preamble.
```

- [ ] **Step 5: Write 4 integration tests**

Create `crates/tau-pkg/tests/install_skill_cross_check.rs`:

```rust
//! Skills-2 integration tests: drive cross_check_skill_package
//! against a real fixture package on disk. Confirms the install
//! pipeline's skill-validation logic end-to-end.
//!
//! Fixtures live at tests/fixtures/skills/critic/. Each test that
//! needs to mutate the fixture (e.g. delete SKILL.md) copies to a
//! tempdir first.

use std::path::PathBuf;

use tau_pkg::skill_check::cross_check_skill_package;
use tempfile::tempdir;

fn critic_fixture() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
        .join("skills")
        .join("critic")
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

fn load_manifest(install_dir: &std::path::Path) -> tau_domain::PackageManifest {
    let toml_path = install_dir.join("tau.toml");
    let text = std::fs::read_to_string(&toml_path).unwrap();
    let u: tau_domain::UncheckedManifest = toml::from_str(&text).unwrap();
    u.validate().unwrap()
}

#[test]
fn happy_path_critic_fixture_passes_cross_check() {
    let fixture = critic_fixture();
    let manifest = load_manifest(&fixture);
    cross_check_skill_package(&fixture, &manifest).unwrap();
}

#[test]
fn missing_skill_md_returns_content_missing() {
    let tmp = tempdir().unwrap();
    copy_dir(&critic_fixture(), tmp.path());
    std::fs::remove_file(tmp.path().join("SKILL.md")).unwrap();
    let manifest = load_manifest(tmp.path());
    let err = cross_check_skill_package(tmp.path(), &manifest).unwrap_err();
    assert!(
        matches!(err, tau_pkg::InstallError::SkillContentMissing { .. }),
        "expected SkillContentMissing, got {err:?}"
    );
}

#[test]
fn mutated_name_returns_name_mismatch() {
    let tmp = tempdir().unwrap();
    copy_dir(&critic_fixture(), tmp.path());
    let skill_md_path = tmp.path().join("SKILL.md");
    let body = std::fs::read_to_string(&skill_md_path).unwrap();
    let mutated = body.replace("name: critic", "name: kritic");
    std::fs::write(&skill_md_path, mutated).unwrap();
    let manifest = load_manifest(tmp.path());
    let err = cross_check_skill_package(tmp.path(), &manifest).unwrap_err();
    match err {
        tau_pkg::InstallError::SkillNameMismatch { tau_toml, skill_md } => {
            assert_eq!(tau_toml, "critic");
            assert_eq!(skill_md, "kritic");
        }
        other => panic!("expected SkillNameMismatch, got {other:?}"),
    }
}

#[test]
fn uncovered_reference_returns_reference_without_capability() {
    // Mutate the manifest to remove the fs.read capability that covers
    // ${SKILL_DIR}/references/**.
    let tmp = tempdir().unwrap();
    copy_dir(&critic_fixture(), tmp.path());
    let toml_path = tmp.path().join("tau.toml");
    let text = std::fs::read_to_string(&toml_path).unwrap();
    // Strip the [[capabilities]] block — easiest by replacing the
    // multi-line section with empty string.
    let stripped = text
        .lines()
        .filter(|line| {
            !line.starts_with("[[capabilities]]")
                && !line.starts_with("kind = \"fs.read\"")
                && !line.starts_with("paths = [\"${SKILL_DIR}/references/**\"]")
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&toml_path, stripped).unwrap();
    let manifest = load_manifest(tmp.path());
    let err = cross_check_skill_package(tmp.path(), &manifest).unwrap_err();
    match err {
        tau_pkg::InstallError::SkillReferenceWithoutCapability { reference, .. } => {
            assert!(
                reference.contains("references/style-guide.md"),
                "got reference: {reference}"
            );
        }
        other => panic!("expected SkillReferenceWithoutCapability, got {other:?}"),
    }
}
```

- [ ] **Step 6: Run the integration tests**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t7 cargo nextest run -p tau-pkg --test install_skill_cross_check 2>&1 | tail -10
```

Expected: 4 tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/tau-pkg/tests/fixtures crates/tau-pkg/tests/install_skill_cross_check.rs
git commit --no-verify -m "$(cat <<'EOF'
test(pkg/skill): 4 integration tests + critic fixture

Critic fixture package at tests/fixtures/skills/critic/ provides a
minimal-but-realistic skill: tau.toml with fs.read capability on
${SKILL_DIR}/references/**, SKILL.md that references the references
folder, and a stub references/style-guide.md.

Tests drive cross_check_skill_package directly with the fixture (and
tempdir copies for tests that mutate). Covers:
  • Happy path
  • SkillContentMissing (delete SKILL.md)
  • SkillNameMismatch (mutate frontmatter)
  • SkillReferenceWithoutCapability (strip the fs.read capability)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: tau-cli error_render extension + insta snapshots

**Files:**
- Modify: `crates/tau-cli/src/cmd/error_render.rs`
- Create: `crates/tau-cli/tests/cmd_install_skill_render.rs`

- [ ] **Step 1: Locate the existing render dispatch**

```bash
grep -nE "render_cross_check_error|fn render|InstallError::" /Users/titouanlebocq/code/tau/crates/tau-cli/src/cmd/error_render.rs | head -15
```

This finds the existing render function + its match arms.

- [ ] **Step 2: Add 4 new render branches**

In `crates/tau-cli/src/cmd/error_render.rs`, find the function that matches on `InstallError` variants (typically `render_install_error` or similar). Add 4 new match arms:

```rust
        tau_pkg::InstallError::SkillContentMissing { name, expected_path } => {
            format!(
                "error: skill {name:?} failed install validation\n\n  \
                 SKILL.md not found at:\n    {}\n\n  \
                 The package declares kind = \"skill\" but no SKILL.md \
                 was found at the path specified by [skill] content.\n  \
                 Verify the package source contains SKILL.md, or update \
                 the [skill] content field in tau.toml.\n",
                expected_path.display()
            )
        }
        tau_pkg::InstallError::SkillNameMismatch { tau_toml, skill_md } => {
            format!(
                "error: skill name mismatch\n\n  \
                 tau.toml declares name = {tau_toml:?}\n  \
                 SKILL.md frontmatter declares name = {skill_md:?}\n\n  \
                 Both must match. Fix the name field in one of:\n    \
                 tau.toml (top-level `name`)\n    \
                 SKILL.md (YAML frontmatter `name`)\n"
            )
        }
        tau_pkg::InstallError::SkillFrontmatterInvalid { detail } => {
            format!(
                "error: skill SKILL.md frontmatter is invalid\n\n  \
                 {detail}\n\n  \
                 SKILL.md must begin with a YAML frontmatter block:\n    \
                 ---\n    name: <skill-name>\n    description: <short description>\n    \
                 ---\n    <markdown body>\n"
            )
        }
        tau_pkg::InstallError::SkillReferenceWithoutCapability { reference, declared_paths } => {
            let declared_block = if declared_paths.is_empty() {
                "    (no fs.read capabilities declared)".to_string()
            } else {
                declared_paths
                    .iter()
                    .map(|p| format!("    {p}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            format!(
                "error: SKILL.md references a file the skill cannot read\n\n  \
                 Reference: {reference}\n\n  \
                 Declared fs.read paths:\n{declared_block}\n\n  \
                 Add an `fs.read` capability whose paths glob covers the \
                 reference, or remove the reference from SKILL.md.\n"
            )
        }
```

The exact match arm structure depends on the existing function. If the function returns `String`, return these strings directly. If it returns `()` and prints to stderr, adapt to use `println!` / `eprintln!` instead.

- [ ] **Step 3: Write snapshot tests**

Create `crates/tau-cli/tests/cmd_install_skill_render.rs`:

```rust
//! Insta snapshot tests for the Skills-2 install error renders.
//! Mirrors crates/tau-cli/tests/cmd_install_cross_check_render.rs
//! (sub-project B precedent).

use std::path::PathBuf;

use tau_cli::cmd::error_render::render_install_error;
use tau_pkg::InstallError;

#[test]
fn render_skill_content_missing() {
    let err = InstallError::SkillContentMissing {
        name: "critic".to_string(),
        expected_path: PathBuf::from("/scope/.tau/skills/critic/SKILL.md"),
    };
    insta::assert_snapshot!(render_install_error(&err));
}

#[test]
fn render_skill_name_mismatch() {
    let err = InstallError::SkillNameMismatch {
        tau_toml: "critic".to_string(),
        skill_md: "kritic".to_string(),
    };
    insta::assert_snapshot!(render_install_error(&err));
}

#[test]
fn render_skill_frontmatter_invalid() {
    let err = InstallError::SkillFrontmatterInvalid {
        detail: "missing required field `name`".to_string(),
    };
    insta::assert_snapshot!(render_install_error(&err));
}

#[test]
fn render_skill_reference_without_capability() {
    let err = InstallError::SkillReferenceWithoutCapability {
        reference: "${SKILL_DIR}/references/foo.md".to_string(),
        declared_paths: vec!["${SKILL_DIR}/templates/**".to_string()],
    };
    insta::assert_snapshot!(render_install_error(&err));
}
```

- [ ] **Step 4: Run the snapshot tests (accept them on first run)**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t8 cargo nextest run -p tau-cli --test cmd_install_skill_render 2>&1 | tail -10
```

First run: snapshots fail / pending. Accept them:

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t8 cargo insta accept -p tau-cli 2>&1 | tail -3
```

(If `cargo-insta` isn't installed: `cargo install cargo-insta` — see if the workspace already has it. Alternative: run the tests with `INSTA_UPDATE=auto` env var.)

Re-run:

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t8 cargo nextest run -p tau-cli --test cmd_install_skill_render 2>&1 | tail -5
```

Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-cli/src/cmd/error_render.rs crates/tau-cli/tests/cmd_install_skill_render.rs crates/tau-cli/tests/snapshots
git commit --no-verify -m "$(cat <<'EOF'
feat(cli/error_render): render the 4 Skills-2 InstallError variants

Adds render branches for SkillContentMissing, SkillNameMismatch,
SkillFrontmatterInvalid, SkillReferenceWithoutCapability. Mirrors
the render_cross_check_error precedent from sub-project B
(plugin-compat).

Output shape: human-readable error message + actionable remediation
hint (which file to edit, which capability to add, etc.).

4 insta snapshot tests guarantee the rendered output stays stable.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: ADR-0026

**Files:**
- Create: `docs/decisions/0026-skills-install-pipeline.md`

- [ ] **Step 1: Verify ADR-0026 is free**

```bash
ls /Users/titouanlebocq/code/tau/docs/decisions/ | grep "^002[6]"
```

If 0026 is taken, increment.

- [ ] **Step 2: Write the ADR**

Write `docs/decisions/0026-skills-install-pipeline.md`:

```markdown
# ADR-0026 — Skills install pipeline (Skills-2)

**Status:** Accepted 2026-05-13.
**Branch / PR:** `feat/skills-2-install-pipeline` (PR pending).
**Spec:** `docs/superpowers/specs/2026-05-12-skills-2-install-pipeline-design.md`.
**Plan:** `docs/superpowers/plans/2026-05-13-skills-2-install-pipeline.md`.
**Depends on:** ADR-0025 (Skills-1 foundation).

## Context

Second of 6 sub-projects from ROADMAP §16 (Skills as first-class
packages, Constitution G10). Skills-1 (ADR-0025, PR #63) shipped the
manifest types + parser + interpolation-variable constant in
tau-domain only — no install pipeline integration. Skills-2 wires
`tau install <skill-pkg>` end-to-end through tau-pkg so a skill
package fetches, validates SKILL.md content + frontmatter, resolves
transitive deps, computes a content SHA-256 + caches frontmatter,
and writes to the lockfile.

## Decision

A new module `tau-pkg::skill_check` mirrors `tau-pkg::sandbox_check`.
Single entry point `cross_check_skill_package(install_dir, manifest)`
runs a 4-step flow:

1. Read `SKILL.md` from `install_dir/<content_path>` (default
   `"SKILL.md"`, from Skills-1's `SkillManifest.content`).
2. Parse via `tau_domain::parse_skill_md` (Skills-1).
3. Validate `frontmatter.name == manifest.name`.
4. Reference lint (**hard-fail**): scan body for `${SKILL_DIR}/<rel-path>`
   substrings; if no `[[capabilities]] kind = "fs.read"` glob covers,
   reject.

`install_with_options` dispatches on `manifest.kind()`:
- `kind = "plugin"` → existing Layer 2 `sandbox_check` (unchanged)
- `kind = "skill"` → new `skill_check`, then compute SHA-256 of
  SKILL.md bytes + snapshot frontmatter into a new `LockedSkill`
  lockfile entry
- other kinds → neither (existing behavior unchanged)

The Layer 2 sandbox cross-check is skipped for skill packages because
skills have no plugin process to spawn.

Lockfile schema bumps v4 → v5. New `LockedPackage.skill: Option<LockedSkill>`
field. `LockedSkill { content_sha256: String, frontmatter:
SkillFrontmatterSnapshot }`. Auto-upgrade follows the standard
`was_pre_vN` + `tracing::warn` once-per-process pattern (v2→v3
precedent).

`tau verify` gains a `VerifyReport::SkillContentDrift { name,
expected, got }` variant parallel to `BinaryDrift`. Re-hashes SKILL.md
on verify, compares to the cached `content_sha256`.

tau-domain rejects packages combining `kind = "skill"` with a
`[plugin]` block at parse time via the new
`PackageManifestError::SkillCannotHavePluginBlock`. Cross-field
validation runs in `UncheckedManifest::validate()`.

## Alternatives considered

### Inline `skill_check` logic in `install_with_options`

Rejected: `sandbox_check` is its own module for the same reasons
(size of `install.rs`, future shareability, testability in isolation).
The shared `parse_skill_md` parser + the discrete 4-step flow benefit
from being callable from Skills-3 (`tau skill list / show` — reads
frontmatter on demand) and Skills-4 (runtime invocation — reads
body) without dragging install.rs along.

### Warn-only reference lint

The initial Skills-2 spec draft (commit `78f4352` in PR #62) had a
warn-only lint. Rejected on review (commit `6176e18` in PR #62):
- True-positive cost (caught at runtime when the agent gets a
  capability-denied error mid-task) is severe — confusing failure
  mode for the agent's LLM, no clear remediation path from the
  runtime error alone.
- False-positive cost (skill author adds one `[[capabilities]]`
  line or removes a stale reference) is trivial.
- Hard-fail at install time matches how the rest of tau handles
  capability declarations: explicit > implicit. Warn-only would be
  the outlier.

### Keep lockfile schema unchanged

The initial Skills-2 spec draft had no lockfile migration —
`tau skill list` would re-read every `SKILL.md` from disk on demand.
Rejected on the same PR #62 review:
- A scope with 30-50 installed skills incurs 30-50 file opens per
  list. Noticeable latency, compounds across CLI surfaces.
- Drift is solved by the same SHA-256 mechanism that protects plugin
  binaries today (`tau verify`).
- Cached frontmatter is ~200 bytes per skill — negligible lockfile
  growth.

## Consequences

- `tau-pkg`'s public surface grows by `cross_check_skill_package` +
  `verify_skill_content` + `LockedSkill` + `SkillFrontmatterSnapshot`.
- 4 new `InstallError` variants.
- 1 new `PackageManifestError` variant.
- 1 new `VerifyReport` variant.
- Lockfile schema v4 → v5 auto-upgrade. v4 lockfiles load cleanly;
  skill packages installed pre-v5 will surface as "unverified" via
  `tau verify` until reinstalled.
- `tau install <skill-pkg>` is now fully functional end-to-end.
- Skills-3 / Skills-4 unblocked.

## Out of scope (deferred to Skills-3+)

- `tau skill list` / `tau skill show` CLI subcommands → Skills-3
  (consumes the cached frontmatter added here).
- Runtime invocation (`agent.<skill>.spawn` resolves to installed
  manifest; SKILL.md body becomes spawned child's system_prompt) →
  Skills-4.
- Agent Skills spec export / import → Skills-5.
- Plugin-package symmetry (`kind = "plugin"` rejecting `[skill]`
  block) — Skills-5 if needed.

## References

- Spec: `docs/superpowers/specs/2026-05-12-skills-2-install-pipeline-design.md`
- Plan: `docs/superpowers/plans/2026-05-13-skills-2-install-pipeline.md`
- Skills-1 ADR: `docs/decisions/0025-skills-foundation.md`
- ROADMAP §16
```

- [ ] **Step 3: Commit**

```bash
git add docs/decisions/0026-skills-install-pipeline.md
git commit --no-verify -m "$(cat <<'EOF'
docs(adr): ADR-0026 — Skills install pipeline (Skills-2)

Accepted. Records the install-time validation pipeline + lockfile
v4→v5 migration + tau verify drift detection. Documents the
hard-fail reference lint (vs the warn-only initial draft) and
the cached-frontmatter lockfile choice (vs unchanged schema).

Cross-references ADR-0025 (Skills-1) and the spec + plan.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: USER GATE — push + open PR

**Files:** none modified.

- [ ] **Step 1: Run pre-push verification**

```bash
timeout 30 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo fmt --all -- --check
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo clippy -p tau-domain -p tau-pkg -p tau-cli --all-targets --features serde -- -D warnings
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-domain --lib --features serde
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-pkg
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-cli --test cmd_install_skill_render
```

Each command must exit 0. If fmt fails, run `cargo fmt --all` and recommit.

- [ ] **Step 2: Push via `--no-verify`**

```bash
git push --no-verify -u origin feat/skills-2-install-pipeline 2>&1 | tail -5
```

- [ ] **Step 3: Open the PR**

```bash
gh pr create --base main \
  --title "feat(pkg): Skills-2 — install pipeline (ROADMAP §16)" \
  --body "$(cat <<'EOF'
## Summary

Second of 6 sub-projects from ROADMAP §16 (Skills as first-class packages, Constitution G10). Wires `tau install <skill-pkg>` end-to-end through tau-pkg. Builds on Skills-1 (merged in #63).

Spec: `docs/superpowers/specs/2026-05-12-skills-2-install-pipeline-design.md` (merged in #62)
Plan: `docs/superpowers/plans/2026-05-13-skills-2-install-pipeline.md` (in this PR)
ADR: `docs/decisions/0026-skills-install-pipeline.md`

## What's in the PR

- **New \`tau-pkg::skill_check\` module**: \`cross_check_skill_package(install_dir, manifest)\` — 4-step validation flow (read SKILL.md → parse → name match → reference lint). 6 unit tests.
- **4 new \`InstallError\` variants**: \`SkillContentMissing\`, \`SkillNameMismatch\`, \`SkillFrontmatterInvalid\`, \`SkillReferenceWithoutCapability\`.
- **\`install_with_options\` integration**: kind-dispatched (plugin → sandbox_check; skill → skill_check + sha256 + frontmatter cache).
- **Lockfile schema v4 → v5**: new \`LockedSkill { content_sha256, frontmatter }\` + \`LockedPackage.skill: Option<LockedSkill>\` field. Auto-upgrade with once-per-process \`tracing::warn!\`.
- **\`tau verify\` extension**: \`VerifyReport::SkillContentDrift\` parallel to \`BinaryDrift\`. Re-hashes SKILL.md on verify.
- **tau-domain validation**: rejects \`kind = "skill"\` + \`[plugin]\` block at parse time. \`PackageManifestError::SkillCannotHavePluginBlock\`.
- **tau-cli error render**: 4 new render branches + 4 insta snapshots.
- **ADR-0026**.

## Test coverage

- 1 new tau-domain test (skill+plugin rejection)
- 6 new tau-pkg unit tests in skill_check
- 1 new tau-pkg unit test (lockfile v4 → v5 migration)
- 2 new tau-pkg unit tests (verify_skill_content drift detection)
- 4 new tau-pkg integration tests (install pipeline + critic fixture)
- 4 new tau-cli insta snapshots (error rendering)

**~18 new tests total.**

## v1 deferrals (per spec)

- \`tau skill list / show\` → Skills-3
- Runtime invocation + \`\${SKILL_DIR}\` substitution → Skills-4
- Agent Skills spec compliance → Skills-5
- Reference skill packages → Skills-6

## Test plan
- [ ] CI green on all 19 required checks

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

PAUSE for the user to confirm CI is green and approve the squash-merge.

- [ ] **Step 4: On user approval, squash-merge**

```bash
gh pr merge $(gh pr view --json number -q .number) --squash --delete-branch
git checkout main && git pull
```

---

## Self-review checklist

- **Spec coverage:**
  - New `tau-pkg::skill_check` module → T3
  - 4 InstallError variants → T2
  - install_with_options integration → T5 (with content_sha256 + frontmatter caching)
  - Layer 2 sandbox cross-check skipped for skills → T5 (kind-dispatch)
  - Reject `kind = "skill"` + `[plugin]` block → T1
  - Lockfile v4 → v5 migration with `LockedSkill` → T4
  - `tau verify` extension with `SkillContentDrift` → T6
  - tau-cli error render extension → T8
  - 4 insta snapshots → T8
  - ADR-0026 → T9
  - All test counts match the spec (6 unit + 4 integration + 2 verify + 1 tau-domain + 4 snapshots = 17, slightly under the spec's projected ~18 because the 1 lockfile-migration test was counted separately in the plan).
- **Placeholder scan:** none — every step has complete code, exact commands, expected output. Two implementer-discretion points (the exact integration shape in T5 + T6 step 6) are flagged with read-the-file-first guidance, which is appropriate given the existing API shape may differ slightly from the plan's sketch.
- **Type consistency:**
  - `SkillManifest`, `SkillFrontmatter`, `SkillContent`, `parse_skill_md`, `SKILL_DIR_VAR` (Skills-1 types) — used in T3
  - `SkillContentMissing`, `SkillNameMismatch`, `SkillFrontmatterInvalid`, `SkillReferenceWithoutCapability` (new InstallError variants) — declared T2, used T3, T5, T7, T8
  - `LockedSkill`, `SkillFrontmatterSnapshot` (new lockfile types) — declared T4, used T5, T6
  - `SkillContentDrift` (new VerifyReport variant) — declared T6, used T6
  - `SkillCannotHavePluginBlock` (new PackageManifestError) — declared T1
  - `verify_skill_content` (new verify function) — declared T6, used T6
  - All names match across tasks. ✅
- **CLAUDE.md cargo rules:** every cargo invocation includes `timeout` + `CARGO_INCREMENTAL=0` + `CARGO_TARGET_DIR=target/<role>` + `-p <crate>`. ✅
- **CLAUDE.md push rules:** T10 uses `git push --no-verify`. ✅
- **No new external deps:** confirmed. `sha2` is already used by `tree_hash`; `globset` is already used by `compute_effective`; `tempfile`, `insta`, `tracing`, `serde`, `serde_yaml` all already in workspace.
