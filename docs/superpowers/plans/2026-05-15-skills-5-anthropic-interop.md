# Skills-5 Anthropic Interop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `tau skill import` + `tau skill export` + `tau verify --anthropic-strict` + `tau install` auto-detection of Anthropic-format skills, with lockfile v5→v6 provenance tracking.

**Architecture:** Detection + synthesis live in tau-domain (pure). Lockfile schema bump + install-pipeline integration + `--anthropic-strict` verify mode live in tau-pkg. Two new CLI subcommands (`import`/`export`) + one new flag (`--anthropic-strict`) live in tau-cli. Skills-5 reuses Skills-1's `parse_skill_md`, Skills-2's lockfile schema machinery, Skills-3's CLI subcommand layout, Skills-4's `find_installed_skill`.

**Tech Stack:** Rust 2021. Existing deps only (serde, toml, sha2, thiserror, tokio, assert_cmd, insta, tempfile). No new external dependencies.

**Branch:** `feat/skills-5-anthropic-interop-design` (worktree at `/Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design`, cut from origin/main at `69c6b8d`).
**Spec:** `docs/superpowers/specs/2026-05-15-skills-5-anthropic-interop-design.md` (PR #102, open).
**Depends on:** Skills-1 (`1d71032`), Skills-2 (`93dbe95`), Skills-3 (`7bec3ab`), Skills-4 (`1f6f331`).

**CLAUDE.md rules in effect:**
- Every cargo invocation: `timeout <secs> env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/<role> cargo <cmd> -p <crate>`.
- All git operations: `git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design ...`.
- Push via `git push --no-verify` (Homebrew rust pre-commit hook fails; CI is authoritative).
- `git commit --no-verify`.
- 4-5 sibling worktrees active for other Claude sessions — work ONLY in `feat-skills-5-design`.

---

## File structure

| Path | Status | Responsibility |
|---|---|---|
| `crates/tau-domain/src/package/skill_format.rs` | Create | `SkillFormat::{Tau, Anthropic, Invalid}` + `detect_format(dir)` + `synthesize_manifest_from_skill_md(parsed, source_url) -> PackageManifest`. Pure logic. |
| `crates/tau-domain/src/package/mod.rs` | Modify | Re-export `SkillFormat`, `synthesize_manifest_from_skill_md`. |
| `crates/tau-domain/src/lib.rs` | Modify | Re-export from crate root. |
| `crates/tau-pkg/src/lockfile.rs` | Modify | Bump `MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION` 5→6. Add `synthesized_from: Option<SynthesizedSource>` field to `LockedPackage`. Add `SynthesizedSource` enum. Backward-compatible v5 reader. |
| `crates/tau-pkg/src/synthesize.rs` | Create | Bridge: `synthesize_anthropic_skill(workspace, source_url) -> Result<PackageManifest, SynthesizeError>`. Orchestrates `detect_format` + SKILL.md read + domain's synthesizer. |
| `crates/tau-pkg/src/install.rs` | Modify | After `source::clone_to_workspace`, call `detect_format`; if Anthropic, synthesize manifest in-memory; pipe `synthesized_from = Some(Anthropic)` through to lockfile write. |
| `crates/tau-pkg/src/error.rs` | Modify | Add `InstallError::NotASkillPackage` + `InstallError::SynthesizeFailed` variants. |
| `crates/tau-pkg/src/verify.rs` | Modify | `verify_with_options(..., anthropic_strict: bool)`. Add `VerifyStatus::AnthropicConformance { skill_name, issue }`. Add `AnthropicConformanceIssue` enum. |
| `crates/tau-pkg/src/lib.rs` | Modify | Re-export `synthesize` module + `SynthesizedSource`. |
| `crates/tau-pkg/tests/install_anthropic_format.rs` | Create | 4 integration tests: auto-detect + install; lockfile records synthesized_from; existing path unaffected; v5→v6 migration. |
| `crates/tau-cli/src/cli.rs` | Modify | Add `SkillSubcommand::{Import, Export}` + `SkillImportArgs` + `SkillExportArgs` + `VerifyArgs::anthropic_strict`. |
| `crates/tau-cli/src/cmd/skill/mod.rs` | Modify | Wire `Import` + `Export` subcommand dispatch. |
| `crates/tau-cli/src/cmd/skill/import.rs` | Create | `tau skill import` impl. Clone + write synthesized tau.toml + print hint. |
| `crates/tau-cli/src/cmd/skill/export.rs` | Create | `tau skill export` impl. Walk install_path, copy except tau.toml. Drop-warning + `--strict`. |
| `crates/tau-cli/src/cmd/skill/show.rs` | Modify | Display `Source: synthesized (Anthropic format)` when `synthesized_from.is_some()`. |
| `crates/tau-cli/src/cmd/verify.rs` | Modify | Wire `--anthropic-strict` through. |
| `crates/tau-cli/src/cmd/error_render.rs` | Modify | Render new ImportError / ExportError / InstallError variants. |
| `crates/tau-cli/tests/cmd_skill_import.rs` | Create | 4 integration tests. |
| `crates/tau-cli/tests/cmd_skill_export.rs` | Create | 5 integration tests (multi-file + strict + warning). |
| `crates/tau-cli/tests/cmd_verify.rs` | Modify | Add 3 tests for `--anthropic-strict` flag. |
| `crates/tau-cli/tests/skill_format_roundtrip.rs` | Create | 2 e2e roundtrip tests. |
| `docs/decisions/0029-skills-anthropic-interop.md` | Create | ADR documenting D1-D5 + 6 rejected alternatives. |

---

## Task 1: tau-domain — `SkillFormat` + `synthesize_manifest_from_skill_md`

**Files:**
- Create: `crates/tau-domain/src/package/skill_format.rs`
- Modify: `crates/tau-domain/src/package/mod.rs`
- Modify: `crates/tau-domain/src/lib.rs`

**Subagent:** sonnet.

- [ ] **Step 1: Create the module skeleton**

Create `crates/tau-domain/src/package/skill_format.rs`:

```rust
//! Skill-package format detection + manifest synthesis (Skills-5).
//!
//! Two responsibilities:
//!
//! 1. [`detect_format`] examines a directory and classifies it as
//!    [`SkillFormat::Tau`] (has `tau.toml`), [`SkillFormat::Anthropic`]
//!    (has `SKILL.md` but no `tau.toml`), or [`SkillFormat::Invalid`]
//!    (neither).
//!
//! 2. [`synthesize_manifest_from_skill_md`] takes a parsed
//!    [`SkillContent`] (from [`parse_skill_md`]) plus a source URL
//!    and produces a default [`PackageManifest`] equivalent to what
//!    a hand-written `tau.toml` would emit for an Anthropic skill.
//!
//! Pure logic — no I/O except the small directory peek in
//! [`detect_format`]. Used by `tau-pkg::synthesize` (the bridge
//! into the install pipeline) and by `tau-cli::cmd::skill::import`.

use std::path::Path;

use crate::package::{
    Capability, PackageKind, PackageManifest, PackageName, PackageSource, SkillContent,
    SkillManifest, Version,
};

/// Classification of a directory containing a skill package.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillFormat {
    /// Directory contains `tau.toml` — a tau-native package.
    Tau,
    /// Directory contains `SKILL.md` but no `tau.toml` — a vanilla
    /// Anthropic-format skill source.
    Anthropic,
    /// Directory contains neither file — not a recognized skill package.
    Invalid,
}

/// Classify `dir` by which manifest files it contains.
///
/// Checks two file names at the directory root:
/// - `tau.toml` → [`SkillFormat::Tau`]
/// - `SKILL.md` (only checked if no `tau.toml`) → [`SkillFormat::Anthropic`]
/// - neither → [`SkillFormat::Invalid`]
///
/// This is a peek, not a full validation. Both Skills-2's
/// `tau-pkg::install` and `tau skill import` re-read + validate
/// the file contents after this dispatch.
pub fn detect_format(dir: &Path) -> SkillFormat {
    if dir.join("tau.toml").is_file() {
        SkillFormat::Tau
    } else if dir.join("SKILL.md").is_file() {
        SkillFormat::Anthropic
    } else {
        SkillFormat::Invalid
    }
}

/// Synthesize a [`PackageManifest`] from a parsed SKILL.md
/// ([`SkillContent`]) plus a source URL.
///
/// Used when `tau install` auto-detects an Anthropic-format source
/// or when `tau skill import` produces a tau.toml on disk.
///
/// Defaults:
/// - `version`: `"0.1.0"` (Anthropic skills don't carry semver)
/// - `kind`: `"skill"`
/// - `capabilities`: empty (Anthropic skills declare none)
/// - `authors`: empty (Anthropic skills don't carry an authors field)
/// - `dependencies`: empty
///
/// Errors are propagated via the `PackageName` / `Version` parsers
/// (e.g. `Skills-1` rejects names containing `/` or whitespace).
pub fn synthesize_manifest_from_skill_md(
    parsed: &SkillContent,
    source: PackageSource,
) -> Result<PackageManifest, SynthesizeError> {
    let name = PackageName::from_str(&parsed.frontmatter.name)
        .map_err(|e| SynthesizeError::InvalidName {
            name: parsed.frontmatter.name.clone(),
            detail: e.to_string(),
        })?;
    let version = Version::parse("0.1.0").expect("0.1.0 is a valid semver");

    let skill = SkillManifest {
        content: "SKILL.md".to_string(),
        requires_tools: vec![],
        requires_skills: vec![],
    };

    PackageManifest::new(
        name,
        version,
        parsed.frontmatter.description.clone(),
        vec![], // authors
        source,
        PackageKind::Skill,
        vec![], // dependencies
        vec![] as Vec<Capability>,
    )
    .map_err(|e| SynthesizeError::ManifestBuild {
        detail: e.to_string(),
    })
    .map(|mut m| {
        m.skill = Some(skill);
        m
    })
}

/// Errors raised by [`synthesize_manifest_from_skill_md`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum SynthesizeError {
    /// SKILL.md `frontmatter.name` is not a valid tau package name
    /// (e.g. contains `/`, whitespace, or invalid chars).
    #[error("invalid skill name {name:?}: {detail}")]
    InvalidName { name: String, detail: String },

    /// `PackageManifest::new` rejected the constructed manifest
    /// (would surprise: should not happen given valid inputs from
    /// `parse_skill_md`).
    #[error("manifest build failed: {detail}")]
    ManifestBuild { detail: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::parse_skill_md;
    use std::str::FromStr;
    use tempfile::tempdir;

    #[test]
    fn detect_format_returns_tau_when_tau_toml_present() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("tau.toml"), "name = \"x\"").unwrap();
        assert_eq!(detect_format(tmp.path()), SkillFormat::Tau);
    }

    #[test]
    fn detect_format_returns_anthropic_when_only_skill_md_present() {
        let tmp = tempdir().unwrap();
        std::fs::write(
            tmp.path().join("SKILL.md"),
            "---\nname: critic\ndescription: x\n---\nbody\n",
        )
        .unwrap();
        assert_eq!(detect_format(tmp.path()), SkillFormat::Anthropic);
    }

    #[test]
    fn detect_format_returns_invalid_when_neither_present() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("other.md"), "x").unwrap();
        assert_eq!(detect_format(tmp.path()), SkillFormat::Invalid);
    }

    #[test]
    fn detect_format_tau_wins_when_both_present() {
        let tmp = tempdir().unwrap();
        std::fs::write(tmp.path().join("tau.toml"), "name = \"x\"").unwrap();
        std::fs::write(
            tmp.path().join("SKILL.md"),
            "---\nname: x\ndescription: y\n---\n",
        )
        .unwrap();
        assert_eq!(detect_format(tmp.path()), SkillFormat::Tau);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn synthesize_produces_skill_kind_manifest_with_defaults() {
        let parsed = parse_skill_md(
            "---\nname: critic\ndescription: Reviews drafts.\n---\nBody.\n",
        )
        .unwrap();
        let source = PackageSource::from_str("https://example.com/critic.git").unwrap();
        let manifest = synthesize_manifest_from_skill_md(&parsed, source).unwrap();

        assert_eq!(manifest.name.as_str(), "critic");
        assert_eq!(manifest.version.to_string(), "0.1.0");
        assert!(matches!(manifest.kind, PackageKind::Skill));
        assert!(manifest.capabilities.is_empty());
        assert!(manifest.authors.is_empty());
        assert!(manifest.dependencies.is_empty());
        assert_eq!(manifest.description, "Reviews drafts.");

        let skill = manifest.skill.as_ref().expect("skill block synthesized");
        assert_eq!(skill.content, "SKILL.md");
        assert!(skill.requires_tools.is_empty());
        assert!(skill.requires_skills.is_empty());
    }
}
```

**Implementer notes:**
- `PackageManifest::new` may have a different signature than shown — adapt to the actual one. Check `crates/tau-domain/src/package/manifest.rs` for the real constructor.
- `PackageName::from_str` requires `use std::str::FromStr;`.
- The synthesize function's "set skill block" pattern assumes `PackageManifest.skill: Option<SkillManifest>` — verify and adapt the setter call accordingly. If it's a builder-style API or a private field, use the appropriate accessor.
- Test #5 requires the `serde` feature for `parse_skill_md`. Check existing tau-domain test patterns for how to gate.
- `tempfile::tempdir()` is already a dev-dep of tau-domain (Skills-1 used it). If not, add `tempfile = { workspace = true }` to `[dev-dependencies]`.

- [ ] **Step 2: Wire into package/mod.rs**

```rust
pub mod skill_format;
pub use skill_format::{detect_format, synthesize_manifest_from_skill_md, SkillFormat, SynthesizeError};
```

- [ ] **Step 3: Re-export from lib.rs**

In `crates/tau-domain/src/lib.rs`, find the line re-exporting other skill-related types (likely `pub use package::{...};`). Add `SkillFormat` + `synthesize_manifest_from_skill_md` + `SynthesizeError` to the re-export list.

- [ ] **Step 4: Run tests**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t1 cargo nextest run -p tau-domain --lib --features serde -E 'test(skill_format)' 2>&1 | tail -10
```

Expected: 5 tests pass.

- [ ] **Step 5: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design add crates/tau-domain
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design commit --no-verify -m "$(cat <<'EOF'
feat(domain): SkillFormat + detect_format + synthesize_manifest_from_skill_md

Skills-5 prerequisite. New pure module crates/tau-domain/src/package/
skill_format.rs:

- SkillFormat::{Tau, Anthropic, Invalid} enum
- detect_format(dir) inspects directory for tau.toml / SKILL.md
- synthesize_manifest_from_skill_md(parsed, source) builds a default
  PackageManifest from a parsed SKILL.md (used when auto-detecting
  Anthropic-format sources in tau install + when tau skill import
  writes a tau.toml to disk)
- SynthesizeError for invalid skill-name / manifest-build failures

Defaults for synthesized manifests: version 0.1.0, kind = skill,
capabilities = [], authors = [], dependencies = []. Authors who want
capabilities edit the synthesized tau.toml via the tau skill import
flow.

5 unit tests.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: tau-pkg lockfile v5 → v6 + `synthesized_from` provenance field

**Files:**
- Modify: `crates/tau-pkg/src/lockfile.rs`

**Subagent:** sonnet.

- [ ] **Step 1: Recon the v4→v5 bump pattern**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design show 93dbe95 -- crates/tau-pkg/src/lockfile.rs | head -80
```

Skills-2 PR #64 (commit `93dbe95`) did the v4→v5 bump. Mirror its shape exactly: bump the constant, add the new field with `#[serde(default)]`, ensure backward-compatible deserialization (v5 entries get `None`).

- [ ] **Step 2: Add `SynthesizedSource` enum**

Just before `pub struct LockedPackage` (around line 116 of `lockfile.rs`), add:

```rust
/// Provenance marker for synthesized manifests (Skills-5).
///
/// Recorded on [`LockedPackage::synthesized_from`] when the install
/// pipeline auto-detected a non-tau format (currently: Anthropic
/// Agent Skills) and synthesized the `tau.toml` in-memory rather
/// than reading one from the source. `tau skill show` surfaces this
/// to the user.
///
/// Added in lockfile schema v6.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SynthesizedSource {
    /// The package was installed from a vanilla Anthropic Agent Skills
    /// source (SKILL.md only; no tau.toml in the source tree).
    Anthropic,
}
```

- [ ] **Step 3: Add `synthesized_from` field to `LockedPackage`**

Inside the existing `pub struct LockedPackage` (around line 116), add at the end:

```rust
    /// Provenance: `Some(_)` if this package's manifest was synthesized
    /// at install time from a non-tau source format (e.g. Anthropic
    /// Agent Skills). `None` for packages installed from sources
    /// that already had a `tau.toml`.
    ///
    /// Added in lockfile schema v6 (Skills-5). v5 entries deserialize
    /// as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synthesized_from: Option<SynthesizedSource>,
```

The `skip_serializing_if = "Option::is_none"` keeps existing v5 lockfiles minimal-diff after a write.

- [ ] **Step 4: Bump `MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION`**

Find line 57 (`pub const MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION: u32 = 5;`) and change to:

```rust
pub const MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION: u32 = 6;
```

The existing reader at line 419-420 (`if parsed.schema_version < MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION { parsed.schema_version = MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION; }`) auto-upgrades v5 lockfiles to v6 on next write.

- [ ] **Step 5: Update existing test fixtures**

Search for `schema_version = 5` or `schema_version: 5` in `crates/tau-pkg/src/lockfile.rs` and update each to `= 6` / `: 6`. Also check `crates/tau-pkg/src/install.rs`, `crates/tau-pkg/src/verify.rs`, `crates/tau-pkg/src/skill_check.rs`, `crates/tau-cli/tests/common/mod.rs`, `crates/tau-cli/tests/cmd_skill_*.rs`, and `crates/tau-runtime/tests/skill_spawn_e2e.rs` for hardcoded `schema_version = 5` literals. The implementer for Skills-2's T4 missed two of these (per memory) — be thorough.

```bash
grep -rn "schema_version\s*=\s*5\|schema_version\":\s*5\|schema_version: 5" /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design/crates 2>&1 | head -20
```

Update each match to 6.

- [ ] **Step 6: Add migration test**

In `crates/tau-pkg/src/lockfile.rs` `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn v5_lockfile_reads_as_v6_with_none_synthesized_from() {
        let v5_toml = r#"schema_version = 5
generated_by_tau_version = "0.0.0"
generated_at = "2026-05-12T10:00:00Z"

[[package]]
name = "critic"
active_version = "0.1.0"
source = "https://example.com/critic.git"

[[package.versions]]
version = "0.1.0"
resolved_commit = "0000000000000000000000000000000000000000"
sha256 = ""
installed_at = "2026-05-12T10:00:00Z"
"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tau-lock.toml");
        std::fs::write(&path, v5_toml).unwrap();
        let lf = LockFile::load(&path).unwrap();

        assert_eq!(lf.schema_version, MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION);
        assert_eq!(lf.packages.len(), 1);
        assert!(lf.packages[0].synthesized_from.is_none());
    }

    #[test]
    fn synthesized_from_anthropic_serializes_and_roundtrips() {
        let v6_toml = r#"schema_version = 6
generated_by_tau_version = "0.0.0"
generated_at = "2026-05-15T10:00:00Z"

[[package]]
name = "critic"
active_version = "0.1.0"
source = "https://example.com/critic.git"
synthesized_from = "anthropic"

[[package.versions]]
version = "0.1.0"
resolved_commit = "0000000000000000000000000000000000000000"
sha256 = ""
installed_at = "2026-05-15T10:00:00Z"
"#;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tau-lock.toml");
        std::fs::write(&path, v6_toml).unwrap();
        let lf = LockFile::load(&path).unwrap();
        assert_eq!(
            lf.packages[0].synthesized_from,
            Some(SynthesizedSource::Anthropic)
        );

        // Round-trip: save + reload.
        let out = tmp.path().join("out.toml");
        lf.save(&out).unwrap();
        let lf2 = LockFile::load(&out).unwrap();
        assert_eq!(lf2.packages[0].synthesized_from, lf.packages[0].synthesized_from);
    }
```

- [ ] **Step 7: Run tests**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t2 cargo nextest run -p tau-pkg --lib 2>&1 | tail -10
```

Expected: 138+ tests pass (existing) + 2 new. All previously-passing tests still pass (the hardcoded-5 updates ensure this).

- [ ] **Step 8: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design add crates/
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design commit --no-verify -m "$(cat <<'EOF'
feat(pkg/lockfile): v5→v6 + LockedPackage.synthesized_from provenance

Skills-5 (ROADMAP §16) prerequisite. Lockfile schema bumps v5→v6
adding `synthesized_from: Option<SynthesizedSource>` to LockedPackage.

SynthesizedSource currently has one variant: Anthropic. Recorded
by tau install when it auto-detects a SKILL.md-only source and
synthesizes the manifest in-memory.

Backward-compatible: v5 lockfiles read with synthesized_from = None
for all entries (via #[serde(default)]); v5→v6 upgrade is automatic
on next write (existing pattern from v4→v5 bump in PR #64). All
hardcoded `schema_version = 5` literals updated to 6.

2 new lockfile tests cover the v5-reads-cleanly + v6-round-trip paths.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: tau-pkg install auto-detect Anthropic format + `synthesize.rs` bridge

**Files:**
- Create: `crates/tau-pkg/src/synthesize.rs`
- Modify: `crates/tau-pkg/src/install.rs`
- Modify: `crates/tau-pkg/src/error.rs`
- Modify: `crates/tau-pkg/src/lib.rs`
- Create: `crates/tau-pkg/tests/install_anthropic_format.rs`

**Subagent:** sonnet.

- [ ] **Step 1: Create the synthesize bridge module**

Create `crates/tau-pkg/src/synthesize.rs`:

```rust
//! Bridge between tau-domain's pure synthesis logic and tau-pkg's
//! install pipeline.
//!
//! Skills-5. Reads SKILL.md from a cloned workspace, parses it, hands
//! off to tau-domain's `synthesize_manifest_from_skill_md`, and
//! returns a [`PackageManifest`] ready for the rest of the install
//! pipeline to consume.

use std::path::Path;

use tau_domain::{
    parse_skill_md, synthesize_manifest_from_skill_md, PackageManifest, PackageSource,
};

/// Read SKILL.md from `workspace`, parse, synthesize a manifest.
///
/// Called by [`crate::install::install_with_options`] when
/// [`tau_domain::detect_format`] classifies `workspace` as
/// [`tau_domain::SkillFormat::Anthropic`].
///
/// `source` is the original install URL (or `local://<path>` for
/// path installs) — propagated into the synthesized manifest's
/// `source` field for the lockfile.
pub fn synthesize_anthropic_skill(
    workspace: &Path,
    source: PackageSource,
) -> Result<PackageManifest, SynthesizeError> {
    let skill_md_path = workspace.join("SKILL.md");
    let text = std::fs::read_to_string(&skill_md_path).map_err(|e| SynthesizeError::ReadSkillMd {
        path: skill_md_path.clone(),
        source: e,
    })?;
    let parsed = parse_skill_md(&text).map_err(|e| SynthesizeError::ParseSkillMd {
        path: skill_md_path.clone(),
        detail: e.to_string(),
    })?;
    synthesize_manifest_from_skill_md(&parsed, source).map_err(|e| {
        SynthesizeError::DomainSynthesize {
            detail: e.to_string(),
        }
    })
}

/// Errors raised by [`synthesize_anthropic_skill`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SynthesizeError {
    /// Failed to read SKILL.md from disk.
    #[error("reading SKILL.md at {path:?}: {source}")]
    ReadSkillMd {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// SKILL.md parse failed (missing required frontmatter field, etc.).
    #[error("parsing SKILL.md at {path:?}: {detail}")]
    ParseSkillMd {
        path: std::path::PathBuf,
        detail: String,
    },
    /// tau-domain's `synthesize_manifest_from_skill_md` returned an
    /// error (e.g. invalid package name in frontmatter).
    #[error("synthesizing manifest: {detail}")]
    DomainSynthesize { detail: String },
}
```

- [ ] **Step 2: Wire into lib.rs**

In `crates/tau-pkg/src/lib.rs`, add:

```rust
pub mod synthesize;
pub use synthesize::{synthesize_anthropic_skill, SynthesizeError};
```

Also re-export `SynthesizedSource` from the lockfile module if it isn't already:

```rust
pub use lockfile::SynthesizedSource;
```

- [ ] **Step 3: Add new `InstallError` variants**

In `crates/tau-pkg/src/error.rs`, find `pub enum InstallError` and append:

```rust
    /// Cloned source has neither tau.toml nor SKILL.md (Skills-5).
    /// Not a recognized skill package format.
    #[error("not a skill package: {path:?} has neither tau.toml nor SKILL.md ({detail})")]
    NotASkillPackage {
        /// The workspace directory that was inspected.
        path: std::path::PathBuf,
        /// Human-readable detail.
        detail: String,
    },

    /// Detected Anthropic format but SKILL.md synthesis failed (Skills-5).
    #[error("synthesizing manifest for Anthropic-format source: {0}")]
    SynthesizeFailed(#[from] crate::synthesize::SynthesizeError),
```

Note: if `InstallError` already uses `#[from]` for other types, follow the same pattern. If it doesn't, switch to a `{ detail: String }` shape — the implementer should check which pattern matches the existing `InstallError` style.

- [ ] **Step 4: Integrate auto-detect into install pipeline**

In `crates/tau-pkg/src/install.rs`, find `pub fn install_with_options(...)`. The pipeline today reads `tau.toml` after cloning. Slot in the format check before the read.

Locate the existing code that reads `workspace/tau.toml`. Just before it, add:

```rust
    // Skills-5: auto-detect format.
    let workspace_format = tau_domain::detect_format(&workspace);
    let (manifest, synthesized_from) = match workspace_format {
        tau_domain::SkillFormat::Tau => {
            // Existing path: read tau.toml.
            let toml_path = workspace.join("tau.toml");
            let toml_text = std::fs::read_to_string(&toml_path)
                .map_err(|e| InstallError::ReadManifest {
                    path: toml_path.clone(),
                    source: e,
                })?;
            let unchecked: tau_domain::UncheckedManifest = toml::from_str(&toml_text)
                .map_err(|e| InstallError::ParseManifest {
                    path: toml_path.clone(),
                    detail: e.to_string(),
                })?;
            let m = unchecked.validate().map_err(|e| InstallError::ValidateManifest {
                path: toml_path,
                detail: e.to_string(),
            })?;
            (m, None)
        }
        tau_domain::SkillFormat::Anthropic => {
            // Skills-5: synthesize from SKILL.md.
            let manifest = crate::synthesize::synthesize_anthropic_skill(
                &workspace,
                package_source.clone(),
            )?;
            (manifest, Some(crate::lockfile::SynthesizedSource::Anthropic))
        }
        tau_domain::SkillFormat::Invalid => {
            return Err(InstallError::NotASkillPackage {
                path: workspace.clone(),
                detail: "directory has neither tau.toml nor SKILL.md".into(),
            });
        }
    };
```

Then below, where the existing code writes the `LockedPackage`, set `synthesized_from`:

```rust
    let locked = LockedPackage {
        // ... existing fields ...
        synthesized_from,
    };
```

**Implementer notes:**
- The implementer must adapt this template to the actual `install_with_options` code shape — variable names (`workspace`, `package_source`), the existing manifest-read code, and the `LockedPackage` construction site will differ. Read the function end-to-end first.
- `tau_domain::detect_format`, `tau_domain::SkillFormat`, `tau_domain::UncheckedManifest`: verify these are public exports of `tau-domain`. If not, the implementer needs to expose them.
- `package_source` is the relevant variable for the synthesized manifest's source URL. If the install API doesn't already have this in scope, derive it from the install request input.
- The existing `tau install` code path likely fans out by `manifest.kind` (skill / tool / llm-backend); the synthesized-manifest path joins at the same spot, so subsequent fanout still works.

- [ ] **Step 5: Write integration tests**

Create `crates/tau-pkg/tests/install_anthropic_format.rs`:

```rust
//! Integration tests for Skills-5 Anthropic-format auto-detection
//! in the tau-pkg install pipeline.

use std::path::Path;
use tempfile::TempDir;

/// Set up a project scope tempdir with `.tau/config.toml` so that
/// `Scope::resolve` finds a project root at the temp directory.
fn setup_project_scope(tmp: &Path) {
    std::fs::create_dir_all(tmp.join(".tau")).unwrap();
    std::fs::write(
        tmp.join(".tau").join("config.toml"),
        "schema_version = 3\n\n[sandbox]\nrequired_tier = \"none\"\n",
    )
    .unwrap();
}

/// Construct a tempdir simulating a cloned Anthropic-skill workspace
/// (SKILL.md only).
fn make_anthropic_workspace() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: critic\ndescription: Reviews drafts.\n---\nReview the draft.\n",
    )
    .unwrap();
    dir
}

/// Construct a tempdir simulating a cloned tau-format workspace
/// (tau.toml + SKILL.md present).
fn make_tau_workspace() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("tau.toml"),
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
    std::fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: critic\ndescription: Reviews drafts.\n---\nReview the draft.\n",
    )
    .unwrap();
    dir
}

// Test 1: auto-detect Anthropic + synthesize manifest succeeds.
// The exact API call for "install from a local workspace" depends on
// tau-pkg's install entry point. The implementer should call the same
// path that `tau install <local-path>` would use, asserting:
// - install succeeds
// - resulting lockfile has 1 entry with name=critic, version=0.1.0
// - synthesized_from = Some(SynthesizedSource::Anthropic)
#[test]
fn install_anthropic_format_synthesizes_manifest_and_marks_provenance() {
    let scope_dir = tempfile::tempdir().unwrap();
    setup_project_scope(scope_dir.path());
    let workspace = make_anthropic_workspace();

    // ... invoke install_with_options with the workspace as the source ...
    // ... read the resulting tau-lock.toml ...
    // ... assert: 1 package, name=critic, version=0.1.0,
    //              synthesized_from = Some(Anthropic) ...
}

// Test 2: existing tau-format install path unaffected — no
// synthesized_from marker emitted.
#[test]
fn install_tau_format_does_not_set_synthesized_from() {
    // ... same scope setup ...
    // ... use make_tau_workspace ...
    // ... assert: synthesized_from = None on the LockedPackage ...
}

// Test 3: directory with neither tau.toml nor SKILL.md errors cleanly.
#[test]
fn install_invalid_workspace_errors_with_not_a_skill_package() {
    // ... make empty tempdir ...
    // ... assert: install returns InstallError::NotASkillPackage ...
}

// Test 4: v5→v6 migration — install over an existing v5 lockfile
// produces a v6 lockfile.
#[test]
fn install_upgrades_v5_lockfile_to_v6() {
    // ... seed a v5 lockfile with one tau-format package ...
    // ... install an Anthropic-format package on top ...
    // ... assert: lockfile is v6 on next read, has 2 entries,
    //             the original entry has synthesized_from = None,
    //             the new entry has synthesized_from = Some(Anthropic) ...
}
```

**Implementer notes:**
- The placeholder test bodies above show INTENT. The actual `install_with_options` call site needs to match the real API. The implementer should consult `crates/tau-pkg/src/install.rs` for the signature and call shape.
- If the install requires a git-clone step (instead of a local workspace), substitute the workspace fixture for a local git bare repo + working repo (the pattern from Skills-3's `cmd_skill_show.rs` `setup_local_package_fixture` helper). This is more setup but mirrors the real install path.
- Acceptable alternative: if going through the full clone pipeline is too painful, refactor `install_with_options` so the synthesize hook is testable in isolation (e.g. extract a `decide_manifest(workspace, source)` helper that the install pipeline calls + tests call directly).

- [ ] **Step 6: Run tests**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t3 cargo nextest run -p tau-pkg 2>&1 | tail -10
```

Expected: existing tests still pass + 4 new tests pass.

- [ ] **Step 7: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design add crates/tau-pkg
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design commit --no-verify -m "$(cat <<'EOF'
feat(pkg/install): auto-detect Anthropic format + synthesize bridge

Skills-5 (ROADMAP §16). After source clone in install_with_options,
tau_domain::detect_format classifies the workspace. Tau-format
workspaces follow the existing path (read tau.toml). Anthropic-format
workspaces are routed through new tau-pkg::synthesize bridge module,
which delegates to tau-domain::synthesize_manifest_from_skill_md.
Invalid workspaces fail with NotASkillPackage.

Synthesized installs record provenance via the new
LockedPackage.synthesized_from field (Some(Anthropic)) so tau skill
show + future tooling can surface the source format.

2 new InstallError variants (NotASkillPackage + SynthesizeFailed via
#[from] on SynthesizeError). 4 integration tests cover synthesize
happy path, tau-path unaffected, invalid-workspace error, v5→v6
migration during install.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: tau-pkg verify `--anthropic-strict` mode

**Files:**
- Modify: `crates/tau-pkg/src/verify.rs`

**Subagent:** sonnet.

- [ ] **Step 1: Add `AnthropicConformanceIssue` enum + new `VerifyStatus` variant**

In `crates/tau-pkg/src/verify.rs`, after the existing `pub enum VerifyStatus` variants, add a new variant + a supporting enum:

```rust
    /// Skills-5: skill failed Anthropic-format conformance check
    /// (triggered by the `--anthropic-strict` flag on `tau verify`).
    #[error("skill {skill_name:?} fails Anthropic conformance: {issue:?}")]
    AnthropicConformance {
        /// Name of the affected skill package.
        skill_name: String,
        /// Specific conformance issue detected.
        issue: AnthropicConformanceIssue,
    },
```

Below the `VerifyStatus` enum, add:

```rust
/// Specific Anthropic-conformance issues raised by Skills-5's
/// `tau verify --anthropic-strict` mode.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnthropicConformanceIssue {
    /// SKILL.md frontmatter is missing the required `description` field
    /// (or it parsed as an empty string).
    MissingDescription,
    /// SKILL.md body (after frontmatter strip) is empty or
    /// whitespace-only.
    EmptyBody,
    /// SKILL.md frontmatter is malformed (YAML parse failed, missing
    /// closing `---`, etc.).
    MalformedFrontmatter {
        /// Underlying parser error detail.
        detail: String,
    },
}
```

- [ ] **Step 2: Add `anthropic_strict` parameter to verify entry point**

Locate the verify entry function (e.g. `pub fn verify_with_options(...)` or whatever Skills-2 exposed). Add a `bool` parameter:

```rust
pub fn verify_with_options(
    scope: &Scope,
    anthropic_strict: bool,  // NEW
    // ... other existing args ...
) -> Vec<VerifyStatus> {
    let mut report = Vec::new();

    // ... existing skill-content-drift checks ...

    if anthropic_strict {
        for pkg in &lockfile.packages {
            if !pkg.has_skill_block() {
                continue;
            }
            let install_path = scope.package_dir(&pkg.name, &pkg.active_version);
            let skill_md = install_path.join("SKILL.md");
            let text = match std::fs::read_to_string(&skill_md) {
                Ok(t) => t,
                Err(_) => continue, // existing drift check would have caught this
            };
            match tau_domain::parse_skill_md(&text) {
                Err(e) => {
                    report.push(VerifyStatus::AnthropicConformance {
                        skill_name: pkg.name.to_string(),
                        issue: AnthropicConformanceIssue::MalformedFrontmatter {
                            detail: e.to_string(),
                        },
                    });
                }
                Ok(content) => {
                    if content.frontmatter.description.trim().is_empty() {
                        report.push(VerifyStatus::AnthropicConformance {
                            skill_name: pkg.name.to_string(),
                            issue: AnthropicConformanceIssue::MissingDescription,
                        });
                    }
                    if content.body.trim().is_empty() {
                        report.push(VerifyStatus::AnthropicConformance {
                            skill_name: pkg.name.to_string(),
                            issue: AnthropicConformanceIssue::EmptyBody,
                        });
                    }
                }
            }
        }
    }

    report
}
```

**Implementer notes:**
- The exact verify entry function signature may differ — adapt. The new `anthropic_strict` parameter should slot in alongside whatever Skills-2 already exposed.
- `pkg.has_skill_block()` is conceptual — the real check is `pkg.skill.is_some()` per the v5 schema.
- All existing call sites of the verify entry function need to be updated to pass `false` for backward compatibility.

- [ ] **Step 3: Re-export new types from lib.rs**

In `crates/tau-pkg/src/lib.rs`, ensure `AnthropicConformanceIssue` is re-exported alongside `VerifyStatus`:

```rust
pub use verify::{verify_with_options, AnthropicConformanceIssue, VerifyStatus};
```

- [ ] **Step 4: Add unit tests**

In `crates/tau-pkg/src/verify.rs` test module, add:

```rust
    #[test]
    fn anthropic_strict_passes_for_conformant_skill() {
        // ... synthesize a scope with a critic skill whose SKILL.md
        //     has non-empty name/description/body ...
        // ... call verify_with_options(&scope, anthropic_strict=true) ...
        // ... assert: no AnthropicConformance variants in the report ...
    }

    #[test]
    fn anthropic_strict_flags_missing_description() {
        // ... SKILL.md frontmatter: { name: "critic", description: "" } ...
        // ... assert: AnthropicConformance { issue: MissingDescription } ...
    }

    #[test]
    fn anthropic_strict_off_does_not_run_conformance() {
        // ... SKILL.md with empty description ...
        // ... call with anthropic_strict=false ...
        // ... assert: no AnthropicConformance variants emitted ...
    }
```

**Implementer notes:**
- The "synthesize a scope" pattern follows Skills-4's `make_critic_scope` (search `crates/tau-pkg/src/skill_resolve.rs` test module for the canonical fixture builder).
- `parse_skill_md` from Skills-1 will catch malformed YAML at parse time and return `MissingDescription` as a parser error, NOT a passed-through empty string. So the "missing description" test should write SKILL.md frontmatter that LACKS the description field rather than supplying an empty one. (Adjust the fixture accordingly; the variant naming still matches.)

- [ ] **Step 5: Run tests**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t4 cargo nextest run -p tau-pkg --lib verify 2>&1 | tail -10
```

Expected: existing verify tests pass + 3 new tests pass.

- [ ] **Step 6: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design add crates/tau-pkg
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design commit --no-verify -m "$(cat <<'EOF'
feat(pkg/verify): --anthropic-strict mode + AnthropicConformance variant

Skills-5 D5. New verify mode for Anthropic-format conformance:
- frontmatter.description must be non-empty
- SKILL.md body must be non-empty
- frontmatter must be well-formed YAML

3 new failure cases via AnthropicConformanceIssue enum
(MissingDescription / EmptyBody / MalformedFrontmatter { detail }).

Existing `tau verify` behavior unchanged when --anthropic-strict
is not set. CLI wiring lands in Task 7.

3 unit tests.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: `tau skill import` subcommand

**Files:**
- Create: `crates/tau-cli/src/cmd/skill/import.rs`
- Modify: `crates/tau-cli/src/cmd/skill/mod.rs`
- Modify: `crates/tau-cli/src/cli.rs`
- Modify: `crates/tau-cli/src/cmd/error_render.rs`
- Create: `crates/tau-cli/tests/cmd_skill_import.rs`

**Subagent:** sonnet.

- [ ] **Step 1: Add the subcommand to cli.rs**

In `crates/tau-cli/src/cli.rs`, find `SkillSubcommand`. Add an `Import` variant:

```rust
    /// Import an Anthropic-format skill source as a tau-skill directory
    /// (Skills-5). Synthesizes a tau.toml; does NOT install. Run
    /// `tau install <output-dir>` afterwards.
    Import(SkillImportArgs),
```

Add an `Export` variant too (used in Task 6 — wire now to avoid touching cli.rs twice):

```rust
    /// Export an installed skill to a vanilla Anthropic-format directory
    /// (Skills-5). Drops tau.toml + capabilities.
    Export(SkillExportArgs),
```

Add the args structs:

```rust
#[derive(Debug, clap::Args)]
pub struct SkillImportArgs {
    /// Source URL or path to import from (an Anthropic-format skill
    /// directory or git URL).
    pub source: String,

    /// Output directory to write the synthesized tau-skill into.
    #[arg(long, short)]
    pub output: PathBuf,

    /// Overwrite an existing output directory.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, clap::Args)]
pub struct SkillExportArgs {
    /// Name of the installed skill to export.
    pub name: String,

    /// Output directory.
    #[arg(long, short)]
    pub output: PathBuf,

    /// Fail (instead of silently dropping) if anything tau-specific
    /// would be lost in the Anthropic-format export (e.g.
    /// capabilities or requires_skills).
    #[arg(long)]
    pub strict: bool,

    /// Overwrite an existing output directory.
    #[arg(long)]
    pub force: bool,
}
```

- [ ] **Step 2: Create the import implementation**

Create `crates/tau-cli/src/cmd/skill/import.rs`:

```rust
//! `tau skill import` — convert an Anthropic-format source into a
//! tau-skill directory.
//!
//! Skills-5 D2 (explicit import flow). Clones the source, detects
//! format, synthesizes a `tau.toml` alongside SKILL.md, leaves the
//! result for the user to inspect or edit before `tau install`.

use std::path::PathBuf;

use crate::cli::SkillImportArgs;

/// Errors raised by `tau skill import`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ImportError {
    /// Source directory has a `tau.toml` already — not an Anthropic
    /// skill; user should `tau install` directly.
    #[error("source already has tau.toml at {path:?}; use `tau install {path}` instead")]
    SourceAlreadyTauSkill { path: PathBuf },

    /// Source has neither tau.toml nor SKILL.md.
    #[error("not a skill package: {path:?} has neither tau.toml nor SKILL.md")]
    NotASkillPackage { path: PathBuf },

    /// Output directory exists and `--force` was not set.
    #[error("output directory {path:?} already exists; pass --force to overwrite")]
    OutputDirectoryExists { path: PathBuf },

    /// I/O error during clone, read, or write.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// SKILL.md parse or synthesis failed.
    #[error("synthesizing manifest: {0}")]
    Synthesize(#[from] tau_pkg::SynthesizeError),

    /// TOML serialization of the synthesized manifest failed.
    #[error("serializing tau.toml: {detail}")]
    SerializeManifest { detail: String },

    /// Source clone failed (git operation, network, etc.).
    #[error("cloning source: {detail}")]
    CloneFailed { detail: String },
}

pub fn run(args: SkillImportArgs) -> Result<(), ImportError> {
    // 1. Handle --force / existing dir.
    if args.output.exists() {
        if !args.force {
            return Err(ImportError::OutputDirectoryExists {
                path: args.output.clone(),
            });
        }
        std::fs::remove_dir_all(&args.output)?;
    }

    // 2. Clone source (or copy if local path).
    let source = parse_source(&args.source)?;
    let cloned_workspace = clone_source_to(&source, &args.output)?;

    // 3. Detect format.
    use tau_domain::SkillFormat;
    match tau_domain::detect_format(&cloned_workspace) {
        SkillFormat::Tau => {
            return Err(ImportError::SourceAlreadyTauSkill {
                path: cloned_workspace,
            });
        }
        SkillFormat::Invalid => {
            return Err(ImportError::NotASkillPackage {
                path: cloned_workspace,
            });
        }
        SkillFormat::Anthropic => {} // proceed
    }

    // 4. Synthesize manifest.
    let manifest =
        tau_pkg::synthesize_anthropic_skill(&cloned_workspace, source.clone())?;

    // 5. Serialize to tau.toml and write alongside SKILL.md.
    let toml_text = toml::to_string_pretty(&manifest)
        .map_err(|e| ImportError::SerializeManifest {
            detail: e.to_string(),
        })?;
    std::fs::write(cloned_workspace.join("tau.toml"), toml_text)?;

    // 6. Print hint.
    println!(
        "Wrote {}/tau.toml.\nRun `tau install {}` to install.",
        args.output.display(),
        args.output.display()
    );

    Ok(())
}

/// Parse `--source` arg into a `tau_domain::PackageSource`. Supports
/// `https://`, `git@`, `file://`, and bare local paths (which are
/// converted to `file://`).
fn parse_source(s: &str) -> Result<tau_domain::PackageSource, ImportError> {
    // ... use tau_domain::PackageSource::from_str + fall back to file://
    //     for bare paths. Adapt to whatever Skills-2's install pipeline
    //     already does (look at crates/tau-pkg/src/source.rs).
    todo!("see implementer notes")
}

/// Clone or copy the source into `output`. Calls into tau-pkg::source
/// or equivalent. The clone leaves the workspace at the output path
/// directly (no nested tau-pkg `clone_to_workspace` temp dir).
fn clone_source_to(
    source: &tau_domain::PackageSource,
    output: &std::path::Path,
) -> Result<PathBuf, ImportError> {
    // ... call tau-pkg's clone helper or invoke git directly ...
    // ... return the path the content was cloned into ...
    todo!("see implementer notes")
}
```

**Implementer notes:**
- The `todo!()` placeholders are intentional — the implementer should resolve them by reading `crates/tau-pkg/src/source.rs` (or wherever Skills-2 put the clone helper) and either reusing or adapting. The bare-path → `file://` conversion + git clone is the pattern.
- `tau_pkg::synthesize_anthropic_skill` was created in Task 3.
- `tau_domain::detect_format` was created in Task 1.
- The serialized tau.toml should be deterministic + human-readable; `toml::to_string_pretty` works. Alternative: `toml_edit` for nicer field ordering, but only add if the basic `to_string_pretty` output is ugly.

- [ ] **Step 3: Wire import into mod.rs dispatch**

In `crates/tau-cli/src/cmd/skill/mod.rs`, find the `match` on `SkillSubcommand`. Add the arm:

```rust
        SkillSubcommand::Import(args) => import::run(args).map_err(Into::into),
        SkillSubcommand::Export(args) => export::run(args).map_err(Into::into), // T6
```

Add the module declarations at the top:

```rust
pub mod import;
pub mod export; // wired in Task 6
```

- [ ] **Step 4: Wire error rendering**

In `crates/tau-cli/src/cmd/error_render.rs`, find the existing error-rendering dispatch. Add cases for the new `ImportError` and `ExportError` variants. Use Skills-3's `tau skill show` not-found render as a template (it surfaces a remediation hint).

- [ ] **Step 5: Write integration tests**

Create `crates/tau-cli/tests/cmd_skill_import.rs`:

```rust
//! Integration tests for `tau skill import` (Skills-5).

use assert_cmd::Command;
use tempfile::TempDir;

fn make_anthropic_source() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: critic\ndescription: Reviews drafts.\n---\nReview the draft.\n",
    )
    .unwrap();
    dir
}

fn make_tau_source() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("tau.toml"),
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
    std::fs::write(
        dir.path().join("SKILL.md"),
        "---\nname: critic\ndescription: Reviews drafts.\n---\nReview.\n",
    )
    .unwrap();
    dir
}

#[test]
fn import_anthropic_source_writes_tau_toml() {
    let source = make_anthropic_source();
    let out = tempfile::tempdir().unwrap();
    let out_path = out.path().join("my-critic");
    let _ = std::fs::remove_dir_all(&out_path); // ensure clean

    let result = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "skill",
            "import",
            source.path().to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(result.status.success(), "stderr: {}", String::from_utf8_lossy(&result.stderr));
    assert!(out_path.join("tau.toml").exists());
    assert!(out_path.join("SKILL.md").exists());

    let toml_text = std::fs::read_to_string(out_path.join("tau.toml")).unwrap();
    assert!(toml_text.contains("name = \"critic\""));
    assert!(toml_text.contains("version = \"0.1.0\""));
    assert!(toml_text.contains("kind = \"skill\""));
}

#[test]
fn import_refuses_existing_output_without_force() {
    let source = make_anthropic_source();
    let out = tempfile::tempdir().unwrap();
    let out_path = out.path().join("existing");
    std::fs::create_dir(&out_path).unwrap();
    std::fs::write(out_path.join("placeholder"), "x").unwrap();

    let result = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "skill",
            "import",
            source.path().to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("already exists"));
}

#[test]
fn import_refuses_tau_format_source() {
    let source = make_tau_source();
    let out = tempfile::tempdir().unwrap();
    let out_path = out.path().join("would-fail");

    let result = Command::cargo_bin("tau")
        .unwrap()
        .args([
            "skill",
            "import",
            source.path().to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(!result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("tau.toml") || stderr.contains("tau install"));
}

#[test]
fn import_synthesized_tau_toml_matches_expected_content() {
    let source = make_anthropic_source();
    let out = tempfile::tempdir().unwrap();
    let out_path = out.path().join("assert-content");

    Command::cargo_bin("tau")
        .unwrap()
        .args([
            "skill",
            "import",
            source.path().to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    let toml_text = std::fs::read_to_string(out_path.join("tau.toml")).unwrap();
    let parsed: toml::Value = toml::from_str(&toml_text).unwrap();
    assert_eq!(parsed["name"].as_str().unwrap(), "critic");
    assert_eq!(parsed["version"].as_str().unwrap(), "0.1.0");
    assert_eq!(parsed["kind"].as_str().unwrap(), "skill");
    assert!(parsed["capabilities"].as_array().unwrap().is_empty());
    assert!(parsed["dependencies"].as_array().unwrap().is_empty());
    assert!(parsed["skill"].as_table().is_some());
}
```

- [ ] **Step 6: Run tests**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t5 cargo nextest run -p tau-cli --test cmd_skill_import 2>&1 | tail -10
```

Expected: 4 tests pass.

- [ ] **Step 7: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design add crates/tau-cli
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design commit --no-verify -m "$(cat <<'EOF'
feat(cli/skill): tau skill import subcommand

Skills-5 D2 (explicit import flow). Clone an Anthropic-format
skill source, synthesize tau.toml alongside SKILL.md, leave for
user inspection / edit before `tau install`.

Errors: SourceAlreadyTauSkill (input has tau.toml — suggest
`tau install` instead), NotASkillPackage (no SKILL.md either),
OutputDirectoryExists (refuse without --force), Synthesize
(propagates from tau-pkg::synthesize), CloneFailed.

cli.rs also wires SkillSubcommand::Export args (subcommand
implementation lands in Task 6).

4 integration tests.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: `tau skill export` subcommand

**Files:**
- Create: `crates/tau-cli/src/cmd/skill/export.rs`
- Modify: `crates/tau-cli/src/cmd/error_render.rs`
- Create: `crates/tau-cli/tests/cmd_skill_export.rs`

**Subagent:** sonnet.

- [ ] **Step 1: Create the export implementation**

Create `crates/tau-cli/src/cmd/skill/export.rs`:

```rust
//! `tau skill export` — strip tau.toml from an installed skill,
//! produce a vanilla Anthropic-format directory.
//!
//! Skills-5 D3. Walks the installed package directory at
//! `<scope>/.tau/packages/<name>/<version>/`; copies every file
//! except `tau.toml` to the output dir. Emits an stderr warning
//! if dropping capabilities; `--strict` makes that warning a
//! hard error.

use std::path::{Path, PathBuf};

use crate::cli::SkillExportArgs;

/// Errors raised by `tau skill export`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExportError {
    /// No skill matches `name`. `suggestion` may be `Some(closest)`
    /// from levenshtein (Skills-3) if a near-match exists.
    #[error("skill not found: {name:?}")]
    SkillNotInstalled {
        name: String,
        suggestion: Option<String>,
    },

    /// `--strict` was set and the export would drop tau-specific
    /// metadata.
    #[error("would drop metadata: {dropped:?} (skill {name:?}); remove --strict to proceed with a warning")]
    WouldDropMetadata {
        name: String,
        dropped: Vec<String>,
    },

    /// Output dir exists and `--force` was not set.
    #[error("output directory {path:?} already exists; pass --force to overwrite")]
    OutputDirectoryExists { path: PathBuf },

    /// I/O failure during copy.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to look up the installed skill.
    #[error("locating skill: {0}")]
    FindSkill(#[from] tau_pkg::FindSkillError),

    /// Scope resolution failed (no `.tau/` in cwd).
    #[error("no scope: {detail}")]
    NoScope { detail: String },
}

pub fn run(args: SkillExportArgs) -> Result<(), ExportError> {
    // 1. Resolve scope from cwd.
    let cwd = std::env::current_dir().map_err(|e| ExportError::NoScope {
        detail: e.to_string(),
    })?;
    let scope = tau_pkg::Scope::resolve(&cwd).map_err(|e| ExportError::NoScope {
        detail: e.to_string(),
    })?;

    // 2. Look up installed skill.
    let installed = tau_pkg::find_installed_skill(&scope, &args.name)?
        .ok_or_else(|| ExportError::SkillNotInstalled {
            name: args.name.clone(),
            suggestion: suggest_skill_name(&scope, &args.name),
        })?;

    // 3. Check --strict against any tau-only metadata.
    let mut dropped: Vec<String> = Vec::new();
    for cap in &installed.capabilities {
        dropped.push(tau_runtime_capability_kind_str(cap));
    }
    if !installed.skill.requires_skills.is_empty() {
        dropped.push(format!("requires_skills ({})", installed.skill.requires_skills.len()));
    }

    if args.strict && !dropped.is_empty() {
        return Err(ExportError::WouldDropMetadata {
            name: args.name.clone(),
            dropped,
        });
    }

    // 4. Handle --force / existing output.
    if args.output.exists() {
        if !args.force {
            return Err(ExportError::OutputDirectoryExists {
                path: args.output.clone(),
            });
        }
        std::fs::remove_dir_all(&args.output)?;
    }
    std::fs::create_dir_all(&args.output)?;

    // 5. Walk install_path, copy everything except tau.toml.
    copy_dir_except_tau_toml(&installed.install_path, &args.output)?;

    // 6. Warn (stderr) about dropped metadata if any.
    if !dropped.is_empty() {
        eprintln!(
            "note: {} dropped on Anthropic export ({}); Anthropic format does not preserve them",
            dropped.len(),
            dropped.join(", "),
        );
    }

    println!(
        "Exported {} to {}",
        args.name,
        args.output.display()
    );
    Ok(())
}

/// Recursive copy of `src` into `dst`, omitting any file named exactly
/// `tau.toml` at any depth (be defensive — the canonical case is
/// tau.toml at the root, but installs may produce nested files in
/// future and we want the contract to be airtight).
fn copy_dir_except_tau_toml(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let name = entry.file_name();
        if ft.is_dir() {
            let new_dst = dst.join(&name);
            std::fs::create_dir_all(&new_dst)?;
            copy_dir_except_tau_toml(&entry.path(), &new_dst)?;
        } else if ft.is_file() {
            if name == "tau.toml" {
                continue;
            }
            std::fs::copy(entry.path(), dst.join(&name))?;
        }
    }
    Ok(())
}

/// Map a Capability to a short stringified kind for the drop-warning
/// message. Reuses Skills-4's pattern (`capability_kind_str` in
/// tau-runtime).
fn tau_runtime_capability_kind_str(cap: &tau_domain::Capability) -> String {
    use tau_domain::{AgentCapability, FsCapability, NetCapability, ProcessCapability, SkillCapability};
    match cap {
        tau_domain::Capability::Filesystem(FsCapability::Read { .. }) => "fs.read".into(),
        tau_domain::Capability::Filesystem(FsCapability::Write { .. }) => "fs.write".into(),
        tau_domain::Capability::Filesystem(FsCapability::Exec { .. }) => "fs.exec".into(),
        tau_domain::Capability::Network(NetCapability::Http { .. }) => "net.http".into(),
        tau_domain::Capability::Process(ProcessCapability::Spawn { .. }) => "process.spawn".into(),
        tau_domain::Capability::Agent(AgentCapability::Spawn { .. }) => "agent.spawn".into(),
        tau_domain::Capability::TaskList { .. } => "task_list".into(),
        tau_domain::Capability::Plan { .. } => "plan".into(),
        tau_domain::Capability::Skill(SkillCapability::Spawn { .. }) => "skill.spawn".into(),
        tau_domain::Capability::Custom { name, .. } => name.clone(),
        _ => "unknown".into(),
    }
}

/// Reuse Skills-3's levenshtein helper for "did you mean ...?".
fn suggest_skill_name(scope: &tau_pkg::Scope, query: &str) -> Option<String> {
    // ... read scope's lockfile, find all skill packages, return
    //     closest match within distance 2 via Skills-3's wagner-fischer
    //     helper at crates/tau-cli/src/cmd/skill/levenshtein.rs ...
    todo!("see implementer notes")
}
```

**Implementer notes:**
- The `Capability` arms in `tau_runtime_capability_kind_str` should match the actual variant set in tau-domain. Skills-4 added `SkillCapability::Spawn`; the rest are pre-existing. Read `crates/tau-domain/src/package/capability.rs` for the canonical list.
- `suggest_skill_name` should call into `crates/tau-cli/src/cmd/skill/levenshtein.rs` (Skills-3's helper). Read that module for the exact API.
- `installed.skill` is a `SkillManifest` from Skills-1; field name + accessor pattern needs to match Skills-4's `InstalledSkill` shape.
- The `_ => "unknown".into()` arm on Capability matches Skills-4's pattern for `#[non_exhaustive]`.

- [ ] **Step 2: Write integration tests**

Create `crates/tau-cli/tests/cmd_skill_export.rs`:

```rust
//! Integration tests for `tau skill export` (Skills-5).

mod common;

use assert_cmd::Command;
use std::path::Path;
use tempfile::TempDir;

// Helper: install a critic skill (capability-less by default).
// Use the Skills-3 / Skills-4 lockfile-synthesis pattern from
// crates/tau-cli/tests/common/mod.rs — call install_fixture or
// reuse the Skills-3 setup helpers if available.
fn install_capability_less_critic_in(scope_root: &Path) {
    common::install_fixture(
        scope_root,
        "critic",
        "0.1.0",
        "skill",
        "https://example.com/critic.git",
    );

    // The install_fixture from common/ writes tau.toml + SKILL.md
    // for tool packages but may not produce the kind=skill + SKILL.md
    // structure Skills-4 needs. The implementer should adapt or
    // extend the helper, OR hand-write the package layout (matching
    // crates/tau-cli/tests/cmd_skill_show.rs's setup_critic_scope
    // approach).

    let pkg_dir = scope_root.join(".tau").join("packages").join("critic").join("0.1.0");
    std::fs::write(
        pkg_dir.join("SKILL.md"),
        "---\nname: critic\ndescription: Reviews drafts.\n---\nReview the draft.\n",
    )
    .unwrap();
}

#[test]
fn export_capability_less_skill_roundtrips() {
    let scope = tempfile::tempdir().unwrap();
    install_capability_less_critic_in(scope.path());

    let out = tempfile::tempdir().unwrap();
    let out_path = out.path().join("exported");

    let result = Command::cargo_bin("tau")
        .unwrap()
        .current_dir(scope.path())
        .args([
            "skill",
            "export",
            "critic",
            "--output",
            out_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        result.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(out_path.join("SKILL.md").exists());
    assert!(!out_path.join("tau.toml").exists());

    // Should print no warning since no capabilities to drop.
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!stderr.contains("dropped"));
}

#[test]
fn export_capability_bearing_skill_warns_and_drops() {
    // ... install a critic with capabilities=[fs.read /workspace/**] ...
    // ... export without --strict ...
    // ... assert success, SKILL.md present, no tau.toml, stderr has "dropped"
    //     mentioning fs.read ...
}

#[test]
fn export_strict_fails_when_capabilities_present() {
    // ... install a critic with capabilities ...
    // ... export with --strict ...
    // ... assert non-zero exit, stderr contains "would drop metadata" ...
}

#[test]
fn export_refuses_existing_output_without_force() {
    // ... install critic, pre-create out dir ...
    // ... export to existing dir without --force ...
    // ... assert failure + "already exists" message ...
}

#[test]
fn export_multi_file_skill_copies_all_referenced_files() {
    // ... install a skill with SKILL.md + refs/style-guide.md + refs/examples.md ...
    // ... export ...
    // ... assert all 3 files present in output, no tau.toml ...
}
```

- [ ] **Step 3: Run tests**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t6 cargo nextest run -p tau-cli --test cmd_skill_export 2>&1 | tail -15
```

Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design add crates/tau-cli
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design commit --no-verify -m "$(cat <<'EOF'
feat(cli/skill): tau skill export subcommand

Skills-5 D3 (export flow). Walk an installed skill's directory at
<scope>/.tau/packages/<name>/<version>/; copy everything except
tau.toml to --output.

Capabilities + requires_skills are dropped silently with stderr
warning. --strict makes drops a hard error. --force overwrites
existing output. levenshtein-suggested skill-name correction on
typos (reuses Skills-3's helper).

5 integration tests (capability-less roundtrip + capability drop
warning + strict denial + force collision + multi-file copy).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Roundtrip e2e tests + `tau skill show` synthesized_from display

**Files:**
- Modify: `crates/tau-cli/src/cmd/skill/show.rs`
- Create: `crates/tau-cli/tests/skill_format_roundtrip.rs`
- Modify: `crates/tau-cli/tests/cmd_verify.rs` (add 3 anthropic_strict tests)

**Subagent:** sonnet.

- [ ] **Step 1: Update `tau skill show` to display synthesized_from**

In `crates/tau-cli/src/cmd/skill/show.rs`, find the human-output rendering (the section that prints `Name: ...`, `Source: ...` etc.). After the existing `Source:` line, add:

```rust
    if let Some(syn) = &installed.lockfile_entry.synthesized_from {
        writeln!(out, "Source format: synthesized ({})", match syn {
            tau_pkg::SynthesizedSource::Anthropic => "Anthropic Agent Skills",
        })?;
    }
```

For the `--json` output, add `synthesized_from` to the JSON struct (serialized as the lowercased variant name, e.g. `"anthropic"` or `null`).

**Implementer notes:**
- `installed.lockfile_entry` is the conceptual reference — the actual field name in Skills-3's `tau skill show` may differ (likely `installed.locked_package` or similar). Read the existing show.rs.
- If the existing snapshot test for show.rs already covers the human render, that snapshot needs a new line. Use `insta review` after running tests, or update the .snap file by hand.

- [ ] **Step 2: Add `--anthropic-strict` flag to `tau verify`**

In `crates/tau-cli/src/cli.rs`, find `VerifyArgs`. Add:

```rust
    /// Skills-5: in addition to the standard drift check, validate
    /// each installed skill against the Anthropic Agent Skills
    /// spec (non-empty description, non-empty body, well-formed
    /// frontmatter).
    #[arg(long)]
    pub anthropic_strict: bool,
```

In `crates/tau-cli/src/cmd/verify.rs`, find the call to `tau_pkg::verify_with_options` and pass through:

```rust
let report = tau_pkg::verify_with_options(&scope, args.anthropic_strict)?;
```

Render new `AnthropicConformance` cases in the human output (and JSON).

- [ ] **Step 3: Write the roundtrip e2e tests**

Create `crates/tau-cli/tests/skill_format_roundtrip.rs`:

```rust
//! End-to-end tests for Skills-5 format roundtripping (Anthropic ↔ tau).

mod common;

use assert_cmd::Command;
use tempfile::TempDir;

/// Test 1: install an Anthropic-format source via auto-detect, then
/// export it; the exported directory should be byte-equivalent to
/// the original Anthropic source (for the SKILL.md + any extra files;
/// tau.toml is the only diff).
#[test]
fn roundtrip_anthropic_source_through_tau() {
    let source = tempfile::tempdir().unwrap();
    std::fs::write(
        source.path().join("SKILL.md"),
        "---\nname: critic\ndescription: Reviews drafts.\n---\nReview.\n",
    )
    .unwrap();
    std::fs::write(source.path().join("README.md"), "Anthropic skill: critic\n").unwrap();

    let scope = tempfile::tempdir().unwrap();
    // ... initialize .tau scope ...
    // ... install from source path ...
    // ... verify install succeeded ...

    let out = tempfile::tempdir().unwrap();
    let out_path = out.path().join("exported");

    // ... call tau skill export critic --output <out_path> ...

    // Assert byte-identity (ignoring tau.toml which only exists in tau-format).
    let original_skill_md = std::fs::read(source.path().join("SKILL.md")).unwrap();
    let exported_skill_md = std::fs::read(out_path.join("SKILL.md")).unwrap();
    assert_eq!(original_skill_md, exported_skill_md);
    let original_readme = std::fs::read(source.path().join("README.md")).unwrap();
    let exported_readme = std::fs::read(out_path.join("README.md")).unwrap();
    assert_eq!(original_readme, exported_readme);
    assert!(!out_path.join("tau.toml").exists());
}

/// Test 2: tau skill import + tau install reproduces a direct tau
/// install of the same Anthropic source.
#[test]
fn import_then_install_matches_direct_install() {
    let source = tempfile::tempdir().unwrap();
    std::fs::write(
        source.path().join("SKILL.md"),
        "---\nname: critic\ndescription: Reviews drafts.\n---\nReview.\n",
    )
    .unwrap();

    let scope_a = tempfile::tempdir().unwrap();
    // ... initialize .tau scope_a ...
    // ... `tau install <source-path>` ... (auto-detect path)
    // ... read scope_a's lockfile ...

    let scope_b = tempfile::tempdir().unwrap();
    // ... initialize .tau scope_b ...
    // ... `tau skill import <source-path> --output ./my-critic` ...
    // ... `tau install ./my-critic` ... (now tau-format)
    // ... read scope_b's lockfile ...

    // Assert: both lockfiles record the same name + version. The
    // synthesized_from field will be Some(Anthropic) in scope_a
    // (auto-detect path) but None in scope_b (the imported tau.toml
    // is now an explicit user-authored manifest from tau's POV).
    // assert eq on name + version; differ on synthesized_from.
}
```

- [ ] **Step 4: Add `--anthropic-strict` tests in cmd_verify.rs**

In `crates/tau-cli/tests/cmd_verify.rs`, append 3 tests:

```rust
#[test]
fn verify_anthropic_strict_passes_for_conformant_skill() {
    // ... synthesize scope + conformant critic ...
    // ... `tau verify --anthropic-strict` ...
    // ... assert exit 0, no AnthropicConformance entries in output ...
}

#[test]
fn verify_anthropic_strict_fails_for_missing_description() {
    // ... synthesize scope + SKILL.md with frontmatter LACKING description ...
    //     (this fails parse_skill_md, surfacing as MalformedFrontmatter,
    //      OR write description: "" and skip the field, depending on
    //      parser strictness — adapt the assertion accordingly)
    // ... assert non-zero exit, stderr contains "AnthropicConformance" ...
}

#[test]
fn verify_without_flag_does_not_check_anthropic_conformance() {
    // ... synthesize scope + SKILL.md that would fail strict check ...
    // ... `tau verify` (no flag) ...
    // ... assert exit 0 or only existing-drift errors, never
    //     AnthropicConformance ...
}
```

- [ ] **Step 5: Run tests**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t7 cargo nextest run -p tau-cli --test skill_format_roundtrip --test cmd_verify --test cmd_skill_show 2>&1 | tail -15
```

Expected: 2 roundtrip tests + 3 new verify tests pass; existing show.rs snapshot tests pass (after snapshot update if applicable).

- [ ] **Step 6: Update help snapshots if necessary**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t7 cargo nextest run -p tau-cli --test help_snapshots 2>&1 | tail -10
```

If the top-level or skill subcommand snapshots changed (because `import` / `export` are new), inspect via:

```bash
ls /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design/crates/tau-cli/tests/snapshots/*.snap.new 2>&1
```

Review + accept with `cargo insta accept` (or move .new to overwrite manually). Commit the updated snapshots in the same commit as Step 7.

**Cross-platform gotcha (Skills-3 lesson):** Windows path normalization. Any snapshot that includes a path (e.g. the `tau skill show` output that has the install_path) needs `\` → `/` normalization at test-time. See `crates/tau-cli/tests/cmd_skill_show.rs::normalize_paths` for the canonical implementation. The new roundtrip tests should adopt the same helper if they assert on path strings.

- [ ] **Step 7: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design add crates/tau-cli
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design commit --no-verify -m "$(cat <<'EOF'
test(cli): Skills-5 e2e roundtrip + verify --anthropic-strict + show synthesized

Three additions to close the Skills-5 user-facing surface:

1. tau skill show now displays a "Source format: synthesized
   (Anthropic Agent Skills)" line when synthesized_from is Some.
   JSON output gains a synthesized_from field.
2. tau verify --anthropic-strict CLI flag wires through to
   tau-pkg's verify_with_options(anthropic_strict).
3. Two end-to-end roundtrip tests:
   - Install Anthropic source → export → byte-identical SKILL.md
     and extra files in output (tau.toml is the only diff).
   - tau skill import + tau install matches a direct tau install
     of the same source (modulo synthesized_from provenance).
4. Three new cmd_verify tests for --anthropic-strict.

Help snapshots updated for the new subcommands + flag.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: ADR-0029

**Files:**
- Create: `docs/decisions/0029-skills-anthropic-interop.md`

**Subagent:** haiku.

- [ ] **Step 1: Verify ADR number is free**

```bash
ls /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design/docs/decisions/ | grep -E "^002[89]"
```

If 0029 is taken (because a parallel Claude session shipped one), use 0030 or the next free number.

- [ ] **Step 2: Write the ADR**

Template structure (sized for ~80-100 lines):

```markdown
# ADR-0029 — Skills Anthropic interop (Skills-5)

**Status:** Accepted 2026-05-15.
**Branch / PR:** `feat/skills-5-anthropic-interop-design` (PR pending).
**Spec:** `docs/superpowers/specs/2026-05-15-skills-5-anthropic-interop-design.md` (PR #102 merged).
**Plan:** `docs/superpowers/plans/2026-05-15-skills-5-anthropic-interop.md`.
**Depends on:** ADR-0025 (Skills-1), ADR-0026 (Skills-2), ADR-0027 (Skills-3), ADR-0028 (Skills-4).

## Context

Fifth of 6 sub-projects from ROADMAP §16. Makes tau skill packages bidirectionally exchangeable with Anthropic Agent Skills format.

## Decision

Five locked decisions:

### D1: Bidirectional scope
Export + import + conformance, not export-only / conformance-only / extension-key variants.

### D2: Both auto-detect and explicit import
`tau install` auto-detects Anthropic format (synthesizes tau.toml in-memory). `tau skill import` produces an editable tau.toml on disk for the customize-before-install workflow.

### D3: Capabilities dropped on export
Synthesized manifests start capability-less. Export drops capabilities with stderr warning; `--strict` makes drops fatal. Synthesized [skill] block follows SkillManifest's actual schema (no `files` glob); export uses simple copy-everything-except-tau.toml.

### D4: Lockfile v5 → v6
`LockedPackage.synthesized_from: Option<SynthesizedSource>` for provenance. v5 reads as None.

### D5: tau verify --anthropic-strict
New flag adds Anthropic-conformance check on top of existing drift detection.

## Alternatives considered
- Conformance-only Skills-5 (rejected: ergonomics)
- Export-only Skills-5 (rejected: import is highest-value)
- Auto-detect only (rejected: power users want to edit before install)
- Explicit import only (rejected: forces 2-step for common case)
- x-tau-capabilities YAML extension (rejected: YAGNI)
- Multi-format `--format <foo>` plumbing (rejected: premature)

## Consequences
- tau-domain public surface: SkillFormat + detect_format + synthesize_manifest_from_skill_md + SynthesizeError
- tau-pkg public surface: synthesize_anthropic_skill + SynthesizedSource + AnthropicConformanceIssue + 2 new InstallError variants
- tau-cli public surface: 2 new subcommands (import/export) + 1 new flag (--anthropic-strict)
- Lockfile schema v6 (additive); v5 reads cleanly with synthesized_from = None
- No new external deps
- No CI changes

## References
- Spec, plan, predecessor ADRs (0025-0028), ROADMAP §16
```

- [ ] **Step 3: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design add docs/decisions/0029-skills-anthropic-interop.md
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design commit --no-verify -m "docs(adr): ADR-0029 — Skills Anthropic interop (Skills-5)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: USER GATE — push + open PR

**Files:** none modified.

**Subagent:** main agent only.

- [ ] **Step 1: Full pre-push verification**

```bash
cd /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design
timeout 30 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo fmt --all -- --check
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo clippy -p tau-domain -p tau-pkg -p tau-cli --all-targets --features serde -- -D warnings
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-domain --lib --features serde
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-pkg
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-cli
```

Fix any fmt / clippy / test failures before push. Format issues often slip through per-crate runs (Skills-4 pattern); the workspace-wide check catches them.

- [ ] **Step 2: Push**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design push --no-verify 2>&1 | tail -5
```

The spec PR (#102) is on the same branch. The plan + impl commits land on top of the spec commits. The single combined PR will land both the spec + the implementation.

- [ ] **Step 3: PR title update + body**

The existing PR title is `docs(specs): Skills-5 — Anthropic interop design (ROADMAP §16)`. Update it now that the PR includes implementation:

```bash
gh pr edit 102 --title "feat(skills): Skills-5 — Anthropic interop (ROADMAP §16)" --body "$(cat <<'EOF'
## Summary

Fifth of 6 sub-projects from ROADMAP §16. Ships Anthropic Agent Skills format interop end-to-end.

- **Export:** `tau skill export <name> --output <dir>` — strip tau.toml, produce a vanilla Anthropic-format directory.
- **Import:** `tau install <git-url>` auto-detects Anthropic format and synthesizes tau.toml in-memory. `tau skill import <src> --output <dir>` writes the synthesized tau.toml to disk for customization before install.
- **Conformance:** `tau verify --anthropic-strict` flags installed skills that fail SKILL.md frontmatter / body requirements.

Spec: `docs/superpowers/specs/2026-05-15-skills-5-anthropic-interop-design.md`
Plan: `docs/superpowers/plans/2026-05-15-skills-5-anthropic-interop.md`
ADR: `docs/decisions/0029-skills-anthropic-interop.md`

## What's in the PR

- **`tau-domain`** — `SkillFormat` enum + `detect_format` + `synthesize_manifest_from_skill_md` + `SynthesizeError`.
- **`tau-pkg`** — new `synthesize.rs` bridge module; install pipeline auto-detects format; lockfile schema v5→v6 with `LockedPackage.synthesized_from`; new `InstallError::NotASkillPackage` + `InstallError::SynthesizeFailed`; `verify_with_options(anthropic_strict)` + `AnthropicConformanceIssue` enum + `VerifyStatus::AnthropicConformance` variant.
- **`tau-cli`** — new `tau skill import` + `tau skill export` subcommands; new `--anthropic-strict` flag on `tau verify`; `tau skill show` displays synthesized_from; error rendering for new variants.

## Test coverage

~25 new tests:
- 5 unit (tau-domain skill_format)
- 2 lockfile migration unit
- 4 tau-pkg install integration
- 3 tau-pkg verify unit
- 4 tau-cli import integration
- 5 tau-cli export integration
- 3 tau-cli verify --anthropic-strict integration
- 2 e2e roundtrip

## v1 deferrals (per spec)
- Reference skill packages → Skills-6
- Sub-skill `requires_skills` cross-format mapping (drop silently)
- Conformance against future spec revisions

## Test plan
- [ ] CI green on all required checks

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)" 2>&1 | tail -3
```

- [ ] **Step 4: Monitor CI**

Use the Monitor tool with a polling loop on `gh pr checks 102`. If macOS jobs fail with the dtolnay rust-toolchain transient (`error: unexpected argument 'nextest' found` or `'check'`), rerun via `gh run rerun <run-id> --failed` after the workflow completes.

If CI doesn't fire at all for >30 minutes while other branches' CI is running (Skills-4's wedge issue, 4th-PR risk), force-rebase onto current main:

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design fetch origin --quiet
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design rebase origin/main
# resolve any conflicts; tests should still pass after
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design push --force-with-lease --no-verify
```

- [ ] **Step 5: PAUSE for user approval**

Once CI is fully green, surface PR URL + summary. Wait for user to approve squash-merge.

- [ ] **Step 6: On user approval — squash-merge + worktree cleanup**

```bash
gh pr merge 102 --squash --delete-branch
cd /Users/titouanlebocq/code/tau
git -C /Users/titouanlebocq/code/tau fetch origin --quiet
git -C /Users/titouanlebocq/code/tau worktree remove /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design
git -C /Users/titouanlebocq/code/tau branch -D feat/skills-5-anthropic-interop-design
```

---

## Self-review checklist

- **Spec coverage:**
  - D1 (bidirectional scope) → T5 (import) + T6 (export) + T4 (conformance)
  - D2 (both auto-detect + explicit import) → T3 (auto-detect in install) + T5 (explicit import)
  - D3 (capabilities dropped on export) → T6 (export + --strict + warning)
  - D4 (lockfile v5→v6) → T2
  - D5 (--anthropic-strict) → T4 (tau-pkg) + T7 (tau-cli wiring)
  - `tau skill show synthesized_from` display → T7
  - 2 new InstallError variants → T3
  - ImportError + ExportError → T5 + T6
  - ADR → T8
  - Roundtrip e2e tests → T7
- **Placeholder scan:** Several `// ... pseudocode ...` blocks in test bodies are intentional — they need real install/scope wiring that depends on Skills-2/3/4 fixture helpers. Each is annotated "see implementer notes" with the canonical references. Two `todo!()` placeholders in T5 (`parse_source`, `clone_source_to`) are similarly intentional — the implementer reads tau-pkg::source for the canonical pattern.
- **Type consistency:** `SkillFormat`, `SynthesizedSource`, `SynthesizeError`, `AnthropicConformanceIssue`, `ImportError`, `ExportError`, `synthesize_manifest_from_skill_md`, `synthesize_anthropic_skill`, `detect_format` — all names match across tasks.
- **CLAUDE.md cargo rules:** every cargo invocation includes timeout + `CARGO_INCREMENTAL=0` + `CARGO_TARGET_DIR=target/<role>` + `-p <crate>`.
- **CLAUDE.md push rules:** T9 uses `git push --no-verify` from inside the worktree via `git -C`.
- **Multi-session safety:** every git operation uses `git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-5-design ...`. T9 cleanup removes the worktree after merge.
- **Cross-platform gotchas annotated:** Windows path normalization (T7); dtolnay rust-toolchain transient (T9); CI-wedge mitigation (T9 step 4 force-rebase fallback).
- **No new external dependencies.**
- **No CI changes.**
