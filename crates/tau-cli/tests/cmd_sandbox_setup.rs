//! Integration tests for `tau sandbox setup`.
//!
//! Non-interactive mode only; interactive prompt is not exercised by
//! these tests because assert_cmd does not provide a clean stdin
//! stream for prompts.

use assert_cmd::Command as AssertCmd;
use predicates::prelude::*;
use std::path::PathBuf;

mod common;

fn config_path(scope_root: &std::path::Path) -> PathBuf {
    scope_root.join(".tau").join("config.toml")
}

#[test]
fn setup_writes_strict_tier_non_interactive() {
    let dir = common::setup_echo_project("echo", "canned_text = \"r\"\n", &[]);
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["sandbox", "setup", "--tier", "strict", "--non-interactive"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .success()
        .stdout(predicate::str::contains("required_tier"));

    let cfg = std::fs::read_to_string(config_path(dir.path())).unwrap();
    assert!(cfg.contains("required_tier = \"strict\""));
}

#[test]
fn setup_writes_none_tier_non_interactive() {
    let dir = common::setup_echo_project("echo", "canned_text = \"r\"\n", &[]);
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["sandbox", "setup", "--tier", "none", "--non-interactive"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .success();

    let cfg = std::fs::read_to_string(config_path(dir.path())).unwrap();
    assert!(cfg.contains("required_tier = \"none\""));
}

#[test]
fn setup_idempotent_on_repeated_invocations() {
    let dir = common::setup_echo_project("echo", "canned_text = \"r\"\n", &[]);
    for _ in 0..2 {
        AssertCmd::cargo_bin("tau")
            .unwrap()
            .args(["sandbox", "setup", "--tier", "light", "--non-interactive"])
            .current_dir(dir.path())
            .env("TAU_HOME", dir.path().join("global"))
            .assert()
            .success();
    }
    let cfg = std::fs::read_to_string(config_path(dir.path())).unwrap();
    assert!(cfg.contains("required_tier = \"light\""));
}

#[test]
fn setup_non_interactive_without_tier_errors() {
    let dir = common::setup_echo_project("echo", "canned_text = \"r\"\n", &[]);
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["sandbox", "setup", "--non-interactive"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("--tier"));
}

#[test]
fn setup_overwrites_existing_block() {
    let dir = common::setup_echo_project("echo", "canned_text = \"r\"\n", &[]);
    // First write: strict
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["sandbox", "setup", "--tier", "strict", "--non-interactive"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .success();
    // Second write: light
    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["sandbox", "setup", "--tier", "light", "--non-interactive"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .success();
    let cfg = std::fs::read_to_string(config_path(dir.path())).unwrap();
    // Only the latest tier should be present.
    assert!(cfg.contains("required_tier = \"light\""));
    assert!(!cfg.contains("required_tier = \"strict\""));
}
