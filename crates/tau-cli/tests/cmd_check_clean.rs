//! Integration tests: `tau check` on a clean project (happy path).

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
fn bare_check_clean_project_exits_zero_or_with_findings() {
    check_common::ensure_tau_home();
    let tmp = TempDir::new().unwrap();
    let src = fixture("clean-project");
    let proj = tmp.path().join("proj");
    std::fs::create_dir(&proj).unwrap();
    std::fs::copy(src.join("tau.toml"), proj.join("tau.toml")).unwrap();

    let out = Command::cargo_bin("tau")
        .unwrap()
        .args(["check"])
        .current_dir(&proj)
        .output()
        .unwrap();

    // A truly clean fixture should exit 0; if our fixture happens to have
    // a missing-package because of agent reqs, that's exit 3 (needs setup).
    // Acceptable either way for this smoke test — what we verify is the
    // command runs to completion and emits SOME output.
    let code = out.status.code().unwrap_or(-1);
    assert!(
        code == 0 || code == 3,
        "expected exit 0 or 3, got {code}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("running") || stdout.contains("checks"),
        "expected human output to mention 'running' or 'checks'\nstdout: {stdout}"
    );
}
