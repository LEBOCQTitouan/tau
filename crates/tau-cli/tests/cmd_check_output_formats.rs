//! Integration tests: `tau check` output format flags (--json, --sarif,
//! mutual exclusion).

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

fn setup() -> (TempDir, PathBuf) {
    check_common::ensure_tau_home();
    let tmp = TempDir::new().unwrap();
    let src = fixture("clean-project");
    let proj = tmp.path().join("proj");
    std::fs::create_dir(&proj).unwrap();
    std::fs::copy(src.join("tau.toml"), proj.join("tau.toml")).unwrap();
    (tmp, proj)
}

#[test]
fn json_output_is_jsonl() {
    let (_tmp, proj) = setup();
    let out = Command::cargo_bin("tau")
        .unwrap()
        .args(["check", "--json"])
        .current_dir(&proj)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    // First line must parse as JSON and have type=run_started.
    let first = stdout.lines().next().unwrap_or("");
    let v: serde_json::Value =
        serde_json::from_str(first).expect("first line must be valid JSON");
    assert_eq!(
        v["type"], "run_started",
        "first JSONL line type must be run_started, got: {first}"
    );
}

#[test]
fn sarif_output_is_sarif_document() {
    let (_tmp, proj) = setup();
    let out = Command::cargo_bin("tau")
        .unwrap()
        // --sarif without a path writes to stdout (default_missing_value = "-")
        .args(["check", "--sarif"])
        .current_dir(&proj)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"version\": \"2.1.0\""),
        "SARIF output must contain version 2.1.0\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("\"runs\""),
        "SARIF output must contain 'runs' array\nstdout: {stdout}"
    );
}

#[test]
fn json_and_sarif_are_mutually_exclusive() {
    let (_tmp, proj) = setup();
    let out = Command::cargo_bin("tau")
        .unwrap()
        .args(["check", "--json", "--sarif"])
        .current_dir(&proj)
        .output()
        .unwrap();
    // clap rejects conflicting flags with a non-zero exit (typically 2).
    assert!(
        !out.status.success(),
        "expected failure for conflicting --json and --sarif flags\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
