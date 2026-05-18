//! Integration tests: `tau check packages` with a missing required tool.

#[path = "check_common.rs"]
mod check_common;

use assert_cmd::Command;
use std::path::PathBuf;
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/check")
        .join(name)
}

#[test]
fn missing_package_yields_exit_3() {
    check_common::ensure_tau_home();
    let tmp = TempDir::new().unwrap();
    let src = fixture("missing-package-project");
    let proj = tmp.path().join("proj");
    std::fs::create_dir(&proj).unwrap();
    std::fs::copy(src.join("tau.toml"), proj.join("tau.toml")).unwrap();

    let out = Command::cargo_bin("tau")
        .unwrap()
        .args(["check", "packages"])
        .current_dir(&proj)
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(3),
        "expected exit 3 for missing-package\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
