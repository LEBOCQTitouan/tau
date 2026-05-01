//! Integration tests for `tau verify`.
//!
//! Uses local `file://`-based git fixtures (bare repo + working repo
//! pattern) so the suite has no network requirement. Each test sets
//! `TAU_HOME` to an isolated tempdir so global-scope operations do not
//! leak across tests or pollute the developer's real `~/.tau`.
//!
//! Exit code contract (per ADR-0007 §7):
//! - 0: all packages Ok or Unverified.
//! - 2: any TreeDrift, BinaryDrift, or Missing.

mod common;

use assert_cmd::Command;
use predicates::prelude::*;

/// Test 1: after a clean install, `tau verify` reports all packages ok and exits 0.
#[test]
fn cmd_verify_clean_install_exits_0() {
    let (fixture, url, _bare) = common::setup_local_package_fixture("verify-clean-pkg", "1.0.0");
    let global_dir = fixture.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    // Install the package (real install computes sha256).
    Command::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", &url])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success();

    // Verify all: should be exit 0 with "ok" in output.
    Command::cargo_bin("tau")
        .unwrap()
        .args(["verify", "--global"])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success()
        .stdout(predicate::str::contains("ok"));
}

/// Test 2: after tampering with a file in the install tree, `tau verify` exits 2
/// and reports tree drift.
#[test]
fn cmd_verify_tampered_file_exits_2() {
    let (fixture, url, _bare) = common::setup_local_package_fixture("verify-tamper-pkg", "1.0.0");
    let global_dir = fixture.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    // Install the package.
    Command::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", &url])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success();

    // Tamper: write an extra file into the installed package tree.
    let pkg_dir = global_dir.join("packages/verify-tamper-pkg/1.0.0");
    assert!(
        pkg_dir.exists(),
        "install dir should exist: {}",
        pkg_dir.display()
    );
    std::fs::write(
        pkg_dir.join("tampered.txt"),
        b"this file was not there at install time",
    )
    .unwrap();

    // Verify: should exit 2 with drift info.
    Command::cargo_bin("tau")
        .unwrap()
        .args(["verify", "--global"])
        .env("TAU_HOME", &global_dir)
        .assert()
        .failure()
        .stdout(predicate::str::contains("drift (tree)"));
}

/// Test 3: if the install directory is removed after install, `tau verify`
/// exits 2 and reports the package as missing.
#[test]
fn cmd_verify_missing_install_dir_exits_2() {
    let (fixture, url, _bare) = common::setup_local_package_fixture("verify-missing-pkg", "1.0.0");
    let global_dir = fixture.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    // Install the package.
    Command::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", &url])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success();

    // Remove the install directory to simulate a corrupted/missing install.
    let pkg_dir = global_dir.join("packages/verify-missing-pkg/1.0.0");
    assert!(
        pkg_dir.exists(),
        "install dir should exist: {}",
        pkg_dir.display()
    );
    std::fs::remove_dir_all(&pkg_dir).unwrap();

    // Verify: should exit 2 with missing drift info.
    Command::cargo_bin("tau")
        .unwrap()
        .args(["verify", "--global"])
        .env("TAU_HOME", &global_dir)
        .assert()
        .failure()
        .stdout(predicate::str::contains("drift (missing)"));
}

/// Test 4: a v2-leftover lockfile entry (empty sha256) with an existing install
/// dir results in `Unverified` status and exit 0 — this is NOT drift.
#[test]
fn cmd_verify_v2_leftover_unverified_exits_0() {
    let global_dir = tempfile::tempdir().unwrap();
    let global_path = global_dir.path();

    // Hand-author a lockfile with empty sha256 (v2-leftover format).
    let now_rfc3339 = "2026-04-28T00:00:00Z";
    let resolved_commit = "0".repeat(40);
    let lockfile_contents = format!(
        r#"schema_version = 1
generated_by_tau_version = "0.0.0"
generated_at = "{now_rfc3339}"

[[package]]
name = "leftover-pkg"
active_version = "2.0.0"
source = "https://example.com/leftover-pkg.git"

[[package.versions]]
version = "2.0.0"
resolved_commit = "{resolved_commit}"
sha256 = ""
installed_at = "{now_rfc3339}"
"#
    );
    std::fs::write(global_path.join("tau-lock.toml"), lockfile_contents).unwrap();

    // Create the install dir so it's "present" (sha256 empty → Unverified, not Missing).
    let pkg_dir = global_path.join("packages/leftover-pkg/2.0.0");
    std::fs::create_dir_all(&pkg_dir).unwrap();
    std::fs::write(
        pkg_dir.join("tau.toml"),
        b"name = \"leftover-pkg\"\nversion = \"2.0.0\"\n",
    )
    .unwrap();

    // Verify: should exit 0 with "unverified" in output.
    Command::cargo_bin("tau")
        .unwrap()
        .args(["verify", "--global"])
        .env("TAU_HOME", global_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("unverified"));
}

/// Test 5: `tau verify --json` emits one JSON object per line on stdout,
/// each with an "event" field.
#[test]
fn cmd_verify_json_mode_emits_one_event_per_line() {
    let (fixture, url, _bare) = common::setup_local_package_fixture("verify-json-pkg", "1.0.0");
    let global_dir = fixture.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    // Install the package so there is something to verify.
    Command::cargo_bin("tau")
        .unwrap()
        .args(["install", "--global", &url])
        .env("TAU_HOME", &global_dir)
        .assert()
        .success();

    // Run verify in JSON mode.
    let output = Command::cargo_bin("tau")
        .unwrap()
        .args(["--json", "verify", "--global"])
        .env("TAU_HOME", &global_dir)
        .output()
        .expect("tau --json verify ran");

    // Exit 0 (clean install → all ok).
    assert!(
        output.status.success(),
        "expected exit 0 for clean install; got: {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // Every non-empty stdout line must be valid JSON with an "event" field.
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let mut line_count = 0usize;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(trimmed)
            .unwrap_or_else(|e| panic!("line is not valid JSON: {trimmed:?}\nerror: {e}"));
        assert!(
            v.get("event").is_some(),
            "JSON line missing \"event\" field: {trimmed}"
        );
        line_count += 1;
    }

    assert!(
        line_count >= 3,
        "expected at least 3 JSON lines (started + package + completed), got {line_count}\nstdout: {stdout}",
    );
}
