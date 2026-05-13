# Skills-1 Manifest Extension Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the typed `[skill]` manifest block, `SkillManifest` / `SkillFrontmatter` / `SkillContent` types, `parse_skill_md` parser, and `${SKILL_DIR}` public constant in `tau-domain`. Skills-1 is the foundation everything else in ROADMAP §16 builds on; ships entirely in `tau-domain` with no `tau-pkg`, `tau-runtime`, or `tau-cli` changes.

**Architecture:** New `tau-domain::package::skill` module owns the skill-specific types and the `parse_skill_md` frontmatter-splitter. `UncheckedManifest` gains an optional `skill: Option<SkillManifest>` field (parallel to existing `plugin: Option<PluginManifest>` pattern). `PackageManifest::skill()` accessor exposes the parsed block. `${SKILL_DIR}` is a public string constant — actual substitution lives in Skills-4 (runtime invocation).

**Tech Stack:** Rust 2021 edition. `serde` (already a tau-domain optional dep) for de/ser. `serde_yaml` (new dep, MIT/Apache-2.0) for SKILL.md frontmatter parsing.

**Branch:** `feat/skills-1-manifest-extension` (already cut from main `0b4f981`).
**Spec:** `docs/superpowers/specs/2026-05-12-skills-1-manifest-design.md` (commit `fc4b1fe`).

**CLAUDE.md rules in effect:**
- Every cargo invocation: `timeout <secs> env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/<role> cargo <cmd> -p tau-domain`. `<role>` = `main` for foreground, `agent-<purpose>` for subagents.
- Push via `scripts/agent-push.sh` OR `git push --no-verify` fallback.
- `cargo-deny` is active. `serde_yaml` is MIT/Apache-2.0 — cargo-deny-allowed.

---

## File structure

| Path | Status | Responsibility |
|---|---|---|
| `crates/tau-domain/Cargo.toml` | Modify | Add `serde_yaml = "0.9"` to optional deps gated behind the `serde` feature. |
| `crates/tau-domain/src/package/skill.rs` | Create | All Skills-1 types: `SkillManifest`, `SkillFrontmatter`, `SkillContent`. `parse_skill_md` function. `default_skill_content` serde hook. `SKILL_DIR` constant. Unit tests for the parser + serde round-trips. |
| `crates/tau-domain/src/package/mod.rs` | Modify | Add `pub mod skill;` declaration. |
| `crates/tau-domain/src/package/manifest.rs` | Modify | Add `skill: Option<SkillManifest>` field to `UncheckedManifest` (serde-default `None`). Add `PackageManifest::skill()` accessor. New `validation_tests` mod test for the round-trip with `[skill]` block. |
| `crates/tau-domain/src/lib.rs` | Modify | Re-export the new public types: `SkillManifest`, `SkillFrontmatter`, `SkillContent`, `SkillContentError`, `SKILL_DIR_VAR`, `parse_skill_md`. |
| `docs/decisions/0025-skills-foundation.md` | Create | ADR documenting the two-layer design (Anthropic `SKILL.md` + tau `[skill]` packaging) and the rejected alternatives. |

---

## Task 1: Scaffold the `skill` module + add `serde_yaml` dependency

**Files:**
- Modify: `crates/tau-domain/Cargo.toml`
- Create: `crates/tau-domain/src/package/skill.rs`
- Modify: `crates/tau-domain/src/package/mod.rs`

- [ ] **Step 1: Add `serde_yaml` to tau-domain optional deps**

In `crates/tau-domain/Cargo.toml`, find the `[dependencies]` section and add `serde_yaml` as a new optional dep gated behind the `serde` feature:

```bash
grep -n "^serde\s*=\|^\[features\]\|^serde\s*=\s*\[" /Users/titouanlebocq/code/tau/crates/tau-domain/Cargo.toml | head -10
```

Then edit `[dependencies]` to add `serde_yaml = { version = "0.9", optional = true }`. Edit `[features]` so the `serde` feature pulls it in: change `serde = ["dep:serde", "dep:base64", ...]` to also include `"dep:serde_yaml"`.

After editing, verify deps compile:
```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo check -p tau-domain --features serde 2>&1 | tail -5
```

Expected: `Finished dev profile ...` with no new warnings.

- [ ] **Step 2: Scaffold `crates/tau-domain/src/package/skill.rs`**

Create the file with the module-level doc + only the public constant and type stubs (full bodies come in Tasks 2-3):

```rust
//! Skill-specific manifest block + SKILL.md parsing.
//!
//! ROADMAP §16 (Skills as first-class packages, Constitution G10).
//! v1 design ratified in
//! `docs/superpowers/specs/2026-05-12-skills-1-manifest-design.md`.
//!
//! A tau skill package is a directory containing both an
//! Anthropic-format `SKILL.md` (content) and a tau-format `tau.toml`
//! (packaging). This module owns:
//!
//! - [`SkillManifest`] — the typed `[skill]` block parsed from
//!   `tau.toml`.
//! - [`SkillFrontmatter`] + [`SkillContent`] — the parsed YAML
//!   frontmatter + Markdown body of `SKILL.md`.
//! - [`parse_skill_md`] — the frontmatter splitter + YAML parser.
//! - [`SKILL_DIR_VAR`] — the public string constant for the
//!   `${SKILL_DIR}` interpolation variable. Substitution itself
//!   is Skills-4's responsibility (runtime invocation); Skills-1
//!   just establishes the variable as a recognized symbolic form.
//!
//! See `docs/decisions/0025-skills-foundation.md` for the ADR.

use crate::package::manifest::PackageDep;

/// Public string constant for the `${SKILL_DIR}` interpolation
/// variable that resolves at runtime to the absolute path of the
/// installed skill's directory.
///
/// Parallel to the conventional `${SCOPE}` and `${PROJECT}` variables
/// (also symbolic in v1; substitution lives outside `tau-domain`).
///
/// Used in capability `paths` entries and `SKILL.md` body references.
/// Validated for syntactic recognition at install time (Skills-2);
/// substituted at spawn time (Skills-4).
pub const SKILL_DIR_VAR: &str = "${SKILL_DIR}";

/// Skill-specific manifest block (parsed from `[skill]` in `tau.toml`).
///
/// `content` defaults to `"SKILL.md"` (the canonical Anthropic skill
/// content filename); `requires_tools` and `requires_skills` default
/// to empty lists.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SkillManifest {
    /// Path to the SKILL.md content file, relative to the package
    /// root. Defaults to `"SKILL.md"`.
    #[cfg_attr(feature = "serde", serde(default = "default_skill_content"))]
    pub content: String,

    /// Tool dependencies (same shape as the top-level
    /// `[[requires.tools]]`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub requires_tools: Vec<PackageDep>,

    /// Sub-skill dependencies. Resolved at install time
    /// (Skills-2); the runtime side wires through `agent.<kind>.spawn`
    /// in Skills-4.
    #[cfg_attr(feature = "serde", serde(default))]
    pub requires_skills: Vec<PackageDep>,
}

#[cfg(feature = "serde")]
fn default_skill_content() -> String {
    "SKILL.md".to_string()
}

/// Parsed YAML frontmatter from a `SKILL.md` file.
///
/// Both `name` and `description` are required by the Anthropic skill
/// format. Other frontmatter fields are tolerated and discarded
/// in v1 — Skills-5 may surface them if Agent Skills spec compliance
/// requires it.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SkillFrontmatter {
    /// The skill's canonical name. Must equal the `name` field of
    /// the containing package's `tau.toml`; mismatch is rejected at
    /// install time (Skills-2).
    pub name: String,

    /// Short human-readable description.
    pub description: String,
}

/// Parsed `SKILL.md` content: frontmatter + body.
///
/// `body` is the verbatim text between the closing `---` frontmatter
/// delimiter and end of file. Becomes the spawned child agent's
/// `system_prompt` at runtime (Skills-4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SkillContent {
    /// The frontmatter (parsed from the YAML block).
    pub frontmatter: SkillFrontmatter,
    /// The Markdown body (becomes the spawned child's
    /// `system_prompt`).
    pub body: String,
}

/// Errors raised by [`parse_skill_md`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkillContentError {
    /// File did not begin with a `---` frontmatter opener (on its
    /// own line, optionally with surrounding whitespace).
    #[error("missing leading `---` frontmatter delimiter")]
    MissingFrontmatterOpener,

    /// File began with `---` but no closing `---` was found.
    #[error("missing closing `---` frontmatter delimiter")]
    MissingFrontmatterCloser,

    /// YAML parse failure inside the frontmatter block.
    #[error("frontmatter YAML parse error: {0}")]
    YamlParse(String),

    /// Frontmatter parsed but is missing the required `name` field.
    #[error("frontmatter missing required field `name`")]
    MissingName,

    /// Frontmatter parsed but is missing the required `description`
    /// field.
    #[error("frontmatter missing required field `description`")]
    MissingDescription,
}

/// Parse a `SKILL.md` file's text into [`SkillContent`].
///
/// Format:
/// ```text
/// ---
/// name: foo
/// description: ...
/// ---
///
/// Markdown body...
/// ```
///
/// The body is everything after the closing `---` (with one leading
/// newline trimmed if present).
pub fn parse_skill_md(_input: &str) -> Result<SkillContent, SkillContentError> {
    // Implementation lands in Task 2.
    Err(SkillContentError::MissingFrontmatterOpener)
}
```

- [ ] **Step 3: Wire the module into `mod.rs`**

Edit `crates/tau-domain/src/package/mod.rs` to add the new module. Find the existing `pub mod` lines and add (alphabetically):

```rust
pub mod skill;
```

- [ ] **Step 4: Verify compile**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo check -p tau-domain --features serde 2>&1 | tail -5
```

Expected: `Finished dev profile ...`. There may be a warning about `parse_skill_md`'s unused `_input` parameter — acceptable (filled in Task 2).

- [ ] **Step 5: Commit**

```bash
git add crates/tau-domain/Cargo.toml crates/tau-domain/src/package/skill.rs crates/tau-domain/src/package/mod.rs
git commit --no-verify -m "$(cat <<'EOF'
feat(domain/skill): scaffold module + SkillManifest types + SKILL_DIR_VAR

Skills-1 sub-project from ROADMAP §16. Creates the
tau-domain::package::skill module with the public types
(SkillManifest, SkillFrontmatter, SkillContent, SkillContentError),
the SKILL_DIR_VAR constant for the ${SKILL_DIR} interpolation
variable, and a stub parse_skill_md function. Adds serde_yaml = "0.9"
as an optional dep gated behind the `serde` feature for SKILL.md
frontmatter parsing.

Module compiles cleanly; bodies for parse_skill_md and the
UncheckedManifest integration land in subsequent tasks.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Implement `parse_skill_md`

**Files:**
- Modify: `crates/tau-domain/src/package/skill.rs`

- [ ] **Step 1: Write the failing tests first**

In `crates/tau-domain/src/package/skill.rs`, append a test module at the bottom of the file:

```rust
#[cfg(all(test, feature = "serde"))]
mod parse_tests {
    use super::*;

    #[test]
    fn parses_valid_skill_md() {
        let input = "---\nname: critic\ndescription: Reviews drafts.\n---\n\nYou are a strict editor.\n";
        let parsed = parse_skill_md(input).unwrap();
        assert_eq!(parsed.frontmatter.name, "critic");
        assert_eq!(parsed.frontmatter.description, "Reviews drafts.");
        assert_eq!(parsed.body, "\nYou are a strict editor.\n");
    }

    #[test]
    fn rejects_missing_opener() {
        let input = "no frontmatter here\nname: critic\n";
        assert_eq!(
            parse_skill_md(input).unwrap_err(),
            SkillContentError::MissingFrontmatterOpener
        );
    }

    #[test]
    fn rejects_missing_closer() {
        let input = "---\nname: critic\ndescription: stuck\n";
        assert_eq!(
            parse_skill_md(input).unwrap_err(),
            SkillContentError::MissingFrontmatterCloser
        );
    }

    #[test]
    fn rejects_malformed_yaml() {
        let input = "---\nname: critic\ndescription: : :\n  - bad indent\n---\nbody\n";
        let err = parse_skill_md(input).unwrap_err();
        assert!(
            matches!(err, SkillContentError::YamlParse(_)),
            "expected YamlParse, got {err:?}"
        );
    }

    #[test]
    fn rejects_missing_name() {
        let input = "---\ndescription: Reviews drafts.\n---\nbody\n";
        assert_eq!(
            parse_skill_md(input).unwrap_err(),
            SkillContentError::MissingName
        );
    }

    #[test]
    fn rejects_missing_description() {
        let input = "---\nname: critic\n---\nbody\n";
        assert_eq!(
            parse_skill_md(input).unwrap_err(),
            SkillContentError::MissingDescription
        );
    }

    #[test]
    fn tolerates_extra_frontmatter_fields() {
        // Future-compat: extra YAML keys are accepted and ignored.
        let input = "---\nname: critic\ndescription: x\ntags: [editing]\nversion: 0.1\n---\nbody\n";
        let parsed = parse_skill_md(input).unwrap();
        assert_eq!(parsed.frontmatter.name, "critic");
    }

    #[test]
    fn tolerates_crlf_line_endings() {
        let input = "---\r\nname: critic\r\ndescription: x\r\n---\r\nbody\r\n";
        let parsed = parse_skill_md(input).unwrap();
        assert_eq!(parsed.frontmatter.name, "critic");
    }
}
```

- [ ] **Step 2: Run tests, see them fail**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-domain --lib --features serde -E 'test(parse_tests::)' 2>&1 | tail -10
```

Expected: 8 tests fail (parse_skill_md stub always returns `MissingFrontmatterOpener`).

- [ ] **Step 3: Implement the parser**

Replace the stub `parse_skill_md` body with the real implementation. Add `#[cfg(feature = "serde")]` to the function (since it depends on serde_yaml). For builds without the `serde` feature, the type stubs are still public but `parse_skill_md` is unavailable.

```rust
/// Parse a `SKILL.md` file's text into [`SkillContent`].
///
/// Format:
/// ```text
/// ---
/// name: foo
/// description: ...
/// ---
///
/// Markdown body...
/// ```
///
/// The body is everything after the closing `---` (with one leading
/// newline trimmed if present, including CRLF).
#[cfg(feature = "serde")]
pub fn parse_skill_md(input: &str) -> Result<SkillContent, SkillContentError> {
    // Helper: strip an optional trailing `\r` from a line slice.
    fn trim_cr(s: &str) -> &str {
        s.strip_suffix('\r').unwrap_or(s)
    }

    let mut lines = input.split_inclusive('\n');

    // First line must be `---` (with optional whitespace + CR).
    let first = lines.next().ok_or(SkillContentError::MissingFrontmatterOpener)?;
    let first_stripped = first.strip_suffix('\n').unwrap_or(first);
    let first_stripped = trim_cr(first_stripped);
    if first_stripped.trim() != "---" {
        return Err(SkillContentError::MissingFrontmatterOpener);
    }

    // Collect lines until the closing `---`.
    let mut yaml_buf = String::new();
    let mut closer_found = false;
    let mut after_closer_idx: usize = 0;
    let mut consumed = first.len();
    for line in lines.by_ref() {
        consumed += line.len();
        let line_stripped = line.strip_suffix('\n').unwrap_or(line);
        let line_stripped = trim_cr(line_stripped);
        if line_stripped.trim() == "---" {
            closer_found = true;
            after_closer_idx = consumed;
            break;
        }
        yaml_buf.push_str(line);
    }
    if !closer_found {
        return Err(SkillContentError::MissingFrontmatterCloser);
    }

    // Parse YAML body into a generic map first so we can produce
    // field-specific errors before serde_yaml's generic missing-field
    // message.
    let map: serde_yaml::Mapping = serde_yaml::from_str(&yaml_buf)
        .map_err(|e| SkillContentError::YamlParse(e.to_string()))?;

    let name = map
        .get(serde_yaml::Value::String("name".into()))
        .and_then(|v| v.as_str().map(String::from))
        .ok_or(SkillContentError::MissingName)?;
    let description = map
        .get(serde_yaml::Value::String("description".into()))
        .and_then(|v| v.as_str().map(String::from))
        .ok_or(SkillContentError::MissingDescription)?;

    let frontmatter = SkillFrontmatter { name, description };

    // Body is everything after the closing `---` line.
    let body = input[after_closer_idx..].to_string();

    Ok(SkillContent { frontmatter, body })
}
```

Also delete or update the original stub so there's only one `parse_skill_md` definition.

- [ ] **Step 4: Run tests, see them pass**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-domain --lib --features serde -E 'test(parse_tests::)' 2>&1 | tail -12
```

Expected: 8 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-domain/src/package/skill.rs
git commit --no-verify -m "$(cat <<'EOF'
feat(domain/skill): parse_skill_md frontmatter splitter + 8 unit tests

Implements the SKILL.md parser: splits on `---` delimiters, parses
the YAML frontmatter via serde_yaml, validates that the `name` and
`description` fields are both present, returns the verbatim body
text for use as the spawned agent's system_prompt at runtime
(Skills-4).

Tolerates CRLF line endings + extra frontmatter fields (future-compat
with the broader Agent Skills spec).

8 unit tests cover: happy path, missing opener, missing closer,
malformed YAML, missing name, missing description, extra fields,
CRLF.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Wire `SkillManifest` into `UncheckedManifest`

**Files:**
- Modify: `crates/tau-domain/src/package/manifest.rs`

- [ ] **Step 1: Add the `skill` field to `UncheckedManifest`**

In `crates/tau-domain/src/package/manifest.rs`, find the `UncheckedManifest` struct (around line 200-235). After the existing `sandbox` field, add:

```rust
    /// Skill manifest declared via the `[skill]` table.
    ///
    /// `None` for non-skill packages (no skill table). `Some` for skill
    /// packages — `tau-pkg::skill_check` (Skills-2) uses this to gate
    /// SKILL.md validation during install. See ROADMAP §16 and
    /// `docs/decisions/0025-skills-foundation.md`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub skill: Option<crate::package::skill::SkillManifest>,
```

- [ ] **Step 2: Add `PackageManifest::skill()` accessor**

Find the `impl PackageManifest` block (after `pub fn sandbox(&self)`). Add:

```rust
    /// Skill manifest from the `[skill]` table, if any.
    ///
    /// `None` for non-skill packages; `Some` for `kind = "skill"`
    /// packages. Surfaced verbatim from the `[skill]` TOML table.
    pub fn skill(&self) -> Option<&crate::package::skill::SkillManifest> {
        self.0.skill.as_ref()
    }
```

- [ ] **Step 3: Update the `good()` helper in `validation_tests`**

In the `#[cfg(test)] mod validation_tests` block (around line 523+), the `good()` helper builds an `UncheckedManifest` with explicit fields. Add `skill: None,` to the literal so it continues to compile:

```rust
            sandbox: crate::package::sandbox::PluginSandboxRequirements::default(),
            skill: None,
        }
```

- [ ] **Step 4: Verify compile + existing tests still pass**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-domain --lib --features serde 2>&1 | tail -5
```

Expected: all existing tau-domain tests pass; the previously-added 8 parse_tests still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-domain/src/package/manifest.rs
git commit --no-verify -m "$(cat <<'EOF'
feat(domain/manifest): wire SkillManifest into UncheckedManifest

Adds `skill: Option<SkillManifest>` as a serde-default-None field on
UncheckedManifest, paralleling the existing `plugin: Option<PluginManifest>`
pattern. Adds PackageManifest::skill() accessor. Updates the
validation_tests::good() helper for the new field.

Foundation for tau-pkg's Skills-2 install pipeline integration.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Round-trip tests for the `[skill]` block

**Files:**
- Modify: `crates/tau-domain/src/package/manifest.rs`

This task adds the serde round-trip tests required by the spec. Lives in `manifest.rs` because that's where the manifest tests live; tests deserialize from TOML to mirror real `tau.toml` parsing.

- [ ] **Step 1: Write the failing tests**

In `crates/tau-domain/src/package/manifest.rs`, find the existing `#[cfg(test)] mod validation_tests`. After the existing tests, add:

```rust
    #[cfg(feature = "serde")]
    #[test]
    fn skill_block_minimal_round_trips_through_toml() {
        let toml_src = r#"
name = "critic"
version = "0.1.0"
description = "Reviews drafts."
source = "git+https://example.com/critic.git"
kind = "skill"

[skill]
"#;
        let u: UncheckedManifest = toml::from_str(toml_src).expect("parse");
        let skill = u.skill.as_ref().expect("skill present");
        // Defaults applied.
        assert_eq!(skill.content, "SKILL.md");
        assert!(skill.requires_tools.is_empty());
        assert!(skill.requires_skills.is_empty());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn skill_block_full_round_trips_through_toml() {
        let toml_src = r#"
name = "critic"
version = "0.1.0"
description = "Reviews drafts."
source = "git+https://example.com/critic.git"
kind = "skill"

[skill]
content = "skills/critic.md"

[[skill.requires_tools]]
name = "fs-read"
source = "git+https://example.com/fs-read.git"
version = "^0.1"

[[skill.requires_skills]]
name = "fact-checker"
source = "git+https://example.com/fact-checker.git"
version = "^0.1"
"#;
        let u: UncheckedManifest = toml::from_str(toml_src).expect("parse");
        let skill = u.skill.as_ref().expect("skill present");
        assert_eq!(skill.content, "skills/critic.md");
        assert_eq!(skill.requires_tools.len(), 1);
        assert_eq!(skill.requires_tools[0].name.as_str(), "fs-read");
        assert_eq!(skill.requires_skills.len(), 1);
        assert_eq!(skill.requires_skills[0].name.as_str(), "fact-checker");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn manifest_without_skill_block_parses_with_skill_none() {
        let toml_src = r#"
name = "regular-tool"
version = "0.1.0"
description = "A tool, not a skill."
source = "git+https://example.com/tool.git"
kind = "tool"
"#;
        let u: UncheckedManifest = toml::from_str(toml_src).expect("parse");
        assert!(u.skill.is_none());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn manifest_with_skill_round_trips_through_validate() {
        // Validate() succeeds for skill packages with the [skill] block;
        // skill-vs-plugin cross-field validation is Skills-2's job, so
        // for now the validator accepts the block as-is.
        let toml_src = r#"
name = "critic"
version = "0.1.0"
description = "Reviews drafts."
source = "git+https://example.com/critic.git"
kind = "skill"

[skill]
"#;
        let u: UncheckedManifest = toml::from_str(toml_src).expect("parse");
        let manifest = u.validate().expect("validate");
        assert!(manifest.skill().is_some());
        assert_eq!(manifest.skill().unwrap().content, "SKILL.md");
    }
```

- [ ] **Step 2: Run tests, see them pass**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-domain --lib --features serde -E 'test(skill_block) | test(skill_none) | test(skill_round_trips)' 2>&1 | tail -10
```

Expected: 4 new tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-domain/src/package/manifest.rs
git commit --no-verify -m "$(cat <<'EOF'
test(domain/manifest): 4 round-trip tests for the [skill] block

Validates that:
- Minimal [skill] block parses with serde defaults (content="SKILL.md")
- Full [skill] block with requires_tools + requires_skills round-trips
- Manifests without a [skill] block parse with skill = None
- A validated PackageManifest exposes Some(skill) when present

Skills-2 will add cross-field validation (skill+plugin mutual
exclusion). Skills-1 stops at "the field exists and parses cleanly."

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Re-export public types + `${SKILL_DIR}` round-trip

**Files:**
- Modify: `crates/tau-domain/src/lib.rs`
- Modify: `crates/tau-domain/src/package/skill.rs`

- [ ] **Step 1: Re-export public types from lib.rs**

Find the existing re-export block in `crates/tau-domain/src/lib.rs`. Add the new public surface:

```rust
pub use crate::package::skill::{
    parse_skill_md, SkillContent, SkillContentError, SkillFrontmatter, SkillManifest,
    SKILL_DIR_VAR,
};
```

(Place it alphabetically — after the existing `pub use crate::package::sandbox::*` line if present, before `pub use crate::value::*` etc.)

If you're unsure where the existing re-export block lives, grep for it first:

```bash
grep -nE "^pub use crate::package" /Users/titouanlebocq/code/tau/crates/tau-domain/src/lib.rs
```

- [ ] **Step 2: Write the failing test for `${SKILL_DIR}` round-trip**

In `crates/tau-domain/src/package/skill.rs`, add a new test module (after the existing `parse_tests` module):

```rust
#[cfg(all(test, feature = "serde"))]
mod skill_dir_var_tests {
    use super::*;
    use crate::package::manifest::UncheckedManifest;

    #[test]
    fn skill_dir_var_constant_is_the_canonical_string() {
        assert_eq!(SKILL_DIR_VAR, "${SKILL_DIR}");
    }

    #[test]
    fn skill_dir_var_in_capability_path_round_trips_verbatim() {
        // Skills-1 is purely symbolic — the ${SKILL_DIR} token survives
        // serde round-trip without expansion. Substitution is Skills-4.
        let toml_src = r#"
name = "critic"
version = "0.1.0"
description = "Reviews drafts."
source = "git+https://example.com/critic.git"
kind = "skill"

[[capabilities]]
kind = "fs.read"
paths = ["${SKILL_DIR}/references/**", "${SKILL_DIR}/templates/**"]

[skill]
"#;
        let u: UncheckedManifest = toml::from_str(toml_src).expect("parse");
        let cap = &u.capabilities[0];
        match cap {
            crate::package::capability::Capability::Filesystem(
                crate::package::capability::FsCapability::Read { paths },
            ) => {
                assert_eq!(paths.len(), 2);
                assert!(paths[0].contains(SKILL_DIR_VAR));
                assert!(paths[1].contains(SKILL_DIR_VAR));
                // Verbatim — no expansion happened.
                assert_eq!(paths[0], "${SKILL_DIR}/references/**");
            }
            other => panic!("expected fs.read, got {other:?}"),
        }
    }
}
```

- [ ] **Step 3: Run tests, see them pass**

```bash
timeout 60 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-domain --lib --features serde -E 'test(skill_dir_var_tests::)' 2>&1 | tail -8
```

Expected: 2 tests pass.

- [ ] **Step 4: Run the full tau-domain test sweep to confirm nothing broke**

```bash
timeout 120 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-domain --lib --features serde 2>&1 | tail -5
```

Expected: all previous tests + 14 new Skills-1 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/package/skill.rs
git commit --no-verify -m "$(cat <<'EOF'
feat(domain): re-export Skills-1 types + ${SKILL_DIR} round-trip tests

Lifts SkillManifest, SkillFrontmatter, SkillContent, SkillContentError,
SKILL_DIR_VAR, and parse_skill_md to the tau_domain::* re-export surface
so tau-pkg (Skills-2) and tau-runtime (Skills-4) can import them
without nested module paths.

2 new tests confirm:
- SKILL_DIR_VAR equals the canonical "${SKILL_DIR}" string
- ${SKILL_DIR} survives serde round-trip in capability paths
  verbatim — symbolic at this stage; substitution lives in Skills-4

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: ADR-0025

**Files:**
- Create: `docs/decisions/0025-skills-foundation.md`

- [ ] **Step 1: Verify ADR-0025 is free**

```bash
ls /Users/titouanlebocq/code/tau/docs/decisions/ | grep "^002[5]"
```

If 0025 is taken, increment to the next available number (and update the doc-comments in `skill.rs` to match).

- [ ] **Step 2: Write the ADR**

Write `docs/decisions/0025-skills-foundation.md`:

```markdown
# ADR-0025 — Skills foundation (manifest extension)

**Status:** Accepted 2026-05-13.
**Branch / PR:** `feat/skills-1-manifest-extension` (PR pending).
**Spec:** `docs/superpowers/specs/2026-05-12-skills-1-manifest-design.md`.
**Plan:** `docs/superpowers/plans/2026-05-13-skills-1-manifest-extension.md`.

## Context

ROADMAP §16 — "Skills as first-class packages." Constitution G10
commits to Skills being first-class in core ("Skills and MCP are
first-class concepts in core. Tau understands the Agent Skills spec
natively"). `kinds::SKILL = "skill"` has been a recognized
`PackageKind` since the v0.1 manifest design, but no runtime concept,
manifest block, parser, or install pipeline has existed.

Skills-1 is the first of six sub-projects that close this gap. It
ships the manifest types + parser + interpolation-variable constant
in `tau-domain` only — no `tau-pkg`, `tau-runtime`, or `tau-cli`
changes. Skills-2 (install pipeline), Skills-3 (discovery), Skills-4
(runtime invocation), Skills-5 (Agent Skills spec compliance), and
Skills-6 (reference packages + docs) follow as separate PRs.

## Decision

A tau skill package is a **directory with two manifest files**:

- **`SKILL.md`** — content. Pure Anthropic skill format: YAML
  frontmatter + Markdown body. Bit-identical to what claude-code /
  claude.ai / any compliant runtime expects. Zero tau-specific
  extensions in this file.
- **`tau.toml`** — packaging. Capability declaration, tool / skill
  dependencies, version, source. The same shape as a plugin or any
  other tau package, with a new `[skill]` block.

A tau skill IS an Anthropic skill (strip `tau.toml`, ship to
claude-code). An Anthropic skill becomes a tau skill by adding
`tau.toml`. No translation; no two-spec divergence.

Skills-1 lands:
- `tau-domain::package::skill` module with `SkillManifest`,
  `SkillFrontmatter`, `SkillContent`, `SkillContentError`.
- `parse_skill_md` function: frontmatter splitter (handles `---`
  delimiters + CRLF) + `serde_yaml`-driven YAML parse + required-
  field validation (`name`, `description`).
- `UncheckedManifest.skill: Option<SkillManifest>` field, defaulting
  to `None` (mirrors the existing `plugin: Option<PluginManifest>`
  pattern).
- `PackageManifest::skill()` accessor.
- `SKILL_DIR_VAR = "${SKILL_DIR}"` public constant for the
  interpolation variable. Symbolic in v1; substitution lives in
  Skills-4.

## Alternatives considered

During the brainstorm, three alternatives were considered before
landing on the two-layer design:

1. **Typed `[skill]` block embedding `system_prompt` inline.** Rejected:
   TOML triple-quoted strings are awkward for long prompts; the format
   diverges from the Anthropic ecosystem; no cross-runtime portability.
2. **Metadata-only `[skill]` + separate prompt file in a tau-specific
   layout.** Rejected: invents a layout convention that diverges from
   Anthropic skills for no benefit. The chosen design is this option
   except the layout convention IS the Anthropic format.
3. **Adopt the Agent Skills spec format verbatim as the only
   manifest.** Rejected as the *only* manifest: would require either
   replacing `tau.toml` or splitting capability declarations across
   two manifest files. The chosen design adopts the Anthropic format
   for content (`SKILL.md`) while keeping tau's packaging machinery
   (capabilities, tool deps, lockfile) in `tau.toml` — best of both.

## Consequences

- `serde_yaml` (MIT/Apache-2.0) is now a tau-domain optional dep
  behind the `serde` feature. Allow-listed under cargo-deny.
- The public re-export surface of `tau_domain` grows by ~6 items
  (the new types + parse function + constant).
- Skills-2 can now wire `tau install <skill-pkg>` through the
  install pipeline using the parser + manifest field added here.
- Skills-3 / Skills-4 can read `SkillManifest` via the existing
  manifest serde flow.

## Out of scope (deferred to Skills-2+)

- The `tau install` install-time validation pipeline (Skills-2).
- `${SKILL_DIR}` runtime substitution (Skills-4).
- Cross-field validation that `kind = "skill"` rejects `[plugin]`
  block (Skills-2 — closer to where the install-time error message
  is rendered).
- Lockfile schema migration for cached frontmatter + content_sha256
  (Skills-2).

## References

- Constitution G10 (Skills + MCP as first-class).
- Spec: `docs/superpowers/specs/2026-05-12-skills-1-manifest-design.md`.
- Priority queue: `docs/superpowers/specs/2026-05-12-post-multi-agent-priority-queue.md`.
- ROADMAP §16.
```

- [ ] **Step 3: Commit**

```bash
git add docs/decisions/0025-skills-foundation.md
git commit --no-verify -m "$(cat <<'EOF'
docs(adr): ADR-0025 — Skills foundation (manifest extension)

Accepted. Records Skills-1's two-layer design (Anthropic SKILL.md +
tau-format tau.toml [skill] block) and the three rejected
alternatives from the brainstorming session. Links to the spec,
plan, and ROADMAP §16; cross-references the 5 deferred sub-projects
(Skills-2 through Skills-6).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: USER GATE — push + open PR

**Files:** none modified.

- [ ] **Step 1: Run pre-push verification**

```bash
timeout 30 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo fmt --all -- --check
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo clippy -p tau-domain --all-targets --features serde -- -D warnings
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-domain --lib --features serde
```

Each command must exit 0. If fmt fails, run `cargo fmt --all` and recommit.

- [ ] **Step 2: Push via `--no-verify` fallback**

Per established workflow (PRs #53-#62), the lefthook deep gate fails on a pre-existing `cmd_chat` echo-llm fixture issue unrelated to this PR. Skip via:

```bash
git push --no-verify -u origin feat/skills-1-manifest-extension 2>&1 | tail -5
```

- [ ] **Step 3: Open the PR**

```bash
gh pr create --base main \
  --title "feat(domain): Skills-1 — manifest extension foundation (ROADMAP §16)" \
  --body "$(cat <<'EOF'
## Summary

First of 6 sub-projects from ROADMAP §16 (Skills as first-class packages, Constitution G10). Lands the typed manifest block + parser + interpolation-variable constant in tau-domain — no tau-pkg, tau-runtime, or tau-cli changes. Skills-2 through Skills-6 follow as separate PRs.

## What's in the PR

- **\`tau-domain::package::skill\`** (new module): \`SkillManifest\`, \`SkillFrontmatter\`, \`SkillContent\`, \`SkillContentError\`, \`parse_skill_md\` function, \`SKILL_DIR_VAR\` public constant.
- **\`UncheckedManifest.skill: Option<SkillManifest>\`** — serde-default \`None\`. Mirrors the existing \`plugin: Option<PluginManifest>\` pattern.
- **\`PackageManifest::skill()\`** accessor.
- **\`serde_yaml = "0.9"\`** added as a tau-domain optional dep behind the \`serde\` feature. cargo-deny-allowed (MIT/Apache-2.0).
- **ADR-0025** documenting the two-layer design (Anthropic \`SKILL.md\` + tau \`[skill]\` packaging) and the rejected alternatives.

## v1 design (locked in spec + ADR)

A tau skill package is a directory containing:
- \`SKILL.md\` — Anthropic-format content (YAML frontmatter + Markdown body). Drop-in compatible with claude-code / claude.ai.
- \`tau.toml\` — tau packaging layer (capabilities, tool/skill deps, version, source) with the new \`[skill]\` block.

A tau skill IS an Anthropic skill; an Anthropic skill becomes a tau skill by adding tau.toml.

## Test coverage

14 new tests:
- 8 in \`skill::parse_tests\`: happy path + 5 error variants + CRLF + extra-field tolerance
- 4 in \`manifest::validation_tests\`: minimal [skill] round-trip, full [skill] with requires_tools/requires_skills, manifest-without-skill-block, validated PackageManifest exposes Some(skill)
- 2 in \`skill::skill_dir_var_tests\`: constant value + \${SKILL_DIR} verbatim round-trip in capability paths

\`cargo fmt\` + \`cargo clippy --all-targets -- -D warnings\` clean.

## Out of scope (deferred per Skills-1 spec)

- Install pipeline integration → Skills-2
- \`tau skill list / show\` → Skills-3
- Runtime invocation + \${SKILL_DIR} substitution → Skills-4
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
  - `SkillManifest` (typed `[skill]` block) → Task 1 (type) + Task 4 (round-trip tests)
  - `SkillFrontmatter` / `SkillContent` → Task 1 (types) + Task 2 (parser)
  - `parse_skill_md` → Task 2
  - `default_skill_content` serde hook → Task 1
  - `${SKILL_DIR}` constant + symbolic round-trip → Task 1 + Task 5
  - `PackageManifest::skill()` accessor + `skill: Option<SkillManifest>` field → Task 3
  - 10+ tests (spec called for ~8) → 14 total across Tasks 2, 4, 5
  - ADR → Task 6
- **Placeholder scan:** none — every step has complete code, exact commands, expected output.
- **Type consistency:** `SkillManifest`, `SkillFrontmatter`, `SkillContent`, `SkillContentError`, `SKILL_DIR_VAR`, `parse_skill_md` — names match across Tasks 1-7.
- **CLAUDE.md cargo rules:** every cargo invocation includes `timeout` + `CARGO_INCREMENTAL=0` + `CARGO_TARGET_DIR=target/main` (or `target/agent-<role>` for subagents) + `-p tau-domain`.
- **CLAUDE.md push rules:** Task 7 uses `git push --no-verify` per established workflow.
- **No code in tau-pkg / tau-runtime / tau-cli:** confirmed. Skills-1 is tau-domain only.
