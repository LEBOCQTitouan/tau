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
