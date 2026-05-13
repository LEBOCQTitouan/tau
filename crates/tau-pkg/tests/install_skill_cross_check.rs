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
    // Replace the [[capabilities]] section with an empty capabilities
    // array so the manifest still parses but has no fs.read entries.
    let stripped = text
        .lines()
        .filter(|line| {
            !line.starts_with("[[capabilities]]")
                && !line.starts_with("kind = \"fs.read\"")
                && !line.starts_with("paths = [\"${SKILL_DIR}/references/**\"]")
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Insert an explicit empty capabilities array before the [skill]
    // section so the manifest field is present (it has no serde(default))
    // and is not interpreted as part of the [skill] table.
    let with_empty_caps = stripped.replace("[skill]", "capabilities = []\n\n[skill]");
    std::fs::write(&toml_path, with_empty_caps).unwrap();
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
