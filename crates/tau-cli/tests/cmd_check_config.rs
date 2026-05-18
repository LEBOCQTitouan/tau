//! Integration tests: `tau check config` on a malformed tau.toml.

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
fn bad_config_fails_with_exit_2() {
    check_common::ensure_tau_home();
    let tmp = TempDir::new().unwrap();
    let src = fixture("bad-config-project");
    let proj = tmp.path().join("proj");
    std::fs::create_dir(&proj).unwrap();
    std::fs::copy(src.join("tau.toml"), proj.join("tau.toml")).unwrap();

    let out = Command::cargo_bin("tau")
        .unwrap()
        .args(["check", "config"])
        .current_dir(&proj)
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2 for bad config\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
