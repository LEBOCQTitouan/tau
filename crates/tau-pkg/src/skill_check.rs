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
    let skill = manifest.skill().expect(
        "cross_check_skill_package called on non-skill package — caller must dispatch on kind",
    );

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
            return Err(InstallError::Internal {
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
            Capability::Filesystem(FsCapability::Read { paths, .. }) => Some(paths.as_slice()),
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
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '*') {
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
///
/// Both `reference` and the glob start with the literal `${SKILL_DIR}/`
/// prefix. We strip that prefix from both sides before handing the
/// relative portion to globset, avoiding any risk of `{` or `}` in the
/// prefix being misinterpreted as glob alternation groups.
fn is_reference_covered(reference: &str, fs_read_paths: &[&str]) -> bool {
    // Strip the literal ${SKILL_DIR}/ prefix from the reference.
    let rel_reference = match reference.strip_prefix(SKILL_DIR_PREFIX) {
        Some(r) => r,
        None => return false,
    };

    for glob_str in fs_read_paths {
        // Only consider globs that themselves start with ${SKILL_DIR}.
        let rel_glob = match glob_str.strip_prefix(SKILL_DIR_PREFIX) {
            Some(r) => r,
            None => continue,
        };
        match GlobBuilder::new(rel_glob).literal_separator(false).build() {
            Ok(g) => {
                if g.compile_matcher().is_match(rel_reference) {
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
                assert_eq!(
                    declared_paths,
                    vec!["${SKILL_DIR}/templates/**".to_string()]
                );
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
