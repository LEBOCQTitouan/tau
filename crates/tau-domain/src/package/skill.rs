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
