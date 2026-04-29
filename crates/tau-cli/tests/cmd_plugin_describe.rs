//! Integration tests for `tau plugin describe <name>` (spec §9 / §10.3).
//!
//! Drives the real CLI against a project synthesized by
//! [`common::setup_echo_project`]: the lockfile holds a
//! `[package.plugin]` table pointing at the pre-built `echo-llm` /
//! `echo-tool` binaries, so `tau plugin describe` spawns the actual
//! plugin process, drives one `meta.handshake`, and shuts the child
//! down — all without any test-mode shims on the host or plugin side.

mod common;

use assert_cmd::Command as AssertCmd;
use predicates::prelude::*;

#[test]
fn plugin_describe_prints_handshake_metadata_for_echo_llm() {
    // echo-tool is included so the lockfile carries it too — keeps the
    // fixture realistic (most real projects have both an LLM backend
    // and at least one tool installed).
    let dir = common::setup_echo_project("echo", "canned_text = \"unused\"\n", &["echo-tool"]);

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["plugin", "describe", "echo-llm"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .success()
        .stdout(predicate::str::contains("echo-llm"))
        // PluginManifest declares `provides = "llm_backend"`.
        .stdout(predicate::str::contains("LlmBackend"))
        // SDK runner advertises llm.complete (and llm.stream).
        .stdout(predicate::str::contains("llm.complete"));
}

#[test]
fn plugin_describe_prints_handshake_metadata_for_echo_tool() {
    let dir = common::setup_echo_project("echo", "canned_text = \"unused\"\n", &["echo-tool"]);

    AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["plugin", "describe", "echo-tool"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .assert()
        .success()
        .stdout(predicate::str::contains("echo-tool"))
        .stdout(predicate::str::contains("Tool"))
        .stdout(predicate::str::contains("tool.call"));
}

#[test]
fn plugin_describe_json_emits_structured_payload() {
    let dir = common::setup_echo_project("echo", "canned_text = \"unused\"\n", &["echo-tool"]);

    let output = AssertCmd::cargo_bin("tau")
        .unwrap()
        .args(["--json", "plugin", "describe", "echo-llm"])
        .current_dir(dir.path())
        .env("TAU_HOME", dir.path().join("global"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected success; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("--json must emit a JSON object");
    assert_eq!(parsed["package"], "echo-llm");
    assert_eq!(parsed["manifest"]["bin"], "echo-llm");
    assert_eq!(parsed["handshake"]["plugin_name"], "echo-llm");
    let methods = parsed["handshake"]["methods"]
        .as_array()
        .expect("handshake.methods must be an array");
    assert!(
        methods.iter().any(|m| m.as_str() == Some("llm.complete")),
        "expected llm.complete in methods; got: {methods:?}"
    );
}
