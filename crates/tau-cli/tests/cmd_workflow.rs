//! CLI integration tests for `tau workflow ...`.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn workflow_list_prints_each_toml_basename() {
    let dir = TempDir::new().unwrap();
    let wf_dir = dir.path().join("workflows");
    fs::create_dir_all(&wf_dir).unwrap();
    fs::write(wf_dir.join("alpha.toml"), b"[workflow]\n").unwrap();
    fs::write(wf_dir.join("beta.toml"), b"[workflow]\n").unwrap();

    let assert = Command::cargo_bin("tau")
        .unwrap()
        .arg("workflow")
        .arg("list")
        .current_dir(dir.path())
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(out.contains("alpha"), "missing alpha; got {out}");
    assert!(out.contains("beta"), "missing beta; got {out}");
}

#[test]
fn workflow_list_handles_no_workflows_dir() {
    let dir = TempDir::new().unwrap();
    Command::cargo_bin("tau")
        .unwrap()
        .arg("workflow")
        .arg("list")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("No workflows/ directory"));
}

#[test]
#[ignore = "requires echo-llm plugin fixture + project scaffold; lift from cmd_chat.rs/cmd_run.rs when those helpers stabilize"]
fn workflow_run_writes_jsonl_and_succeeds() {
    // Implementer: lift the fixture-setup helper from the existing
    // cmd_chat.rs or cmd_run.rs tests in this directory. If no helper
    // exists, build inline using the patterns from
    // crates/tau-plugin-compat/tests/layer4_native.rs.
    //
    // The test should:
    // 1. Create a temp project dir with a tau.toml declaring one agent
    //    (echo-llm as llm_backend, the standard echo-tool as requires.tools).
    // 2. Write workflows/echo-pipeline.toml with one agent.run step.
    // 3. Run `tau workflow run echo-pipeline --input "hello"`.
    // 4. Assert exit code 0, stdout contains the echo-llm's reply,
    //    and stderr contains "run_id: " followed by a ULID.
    // 5. Optionally verify the JSONL log was created under
    //    .tau/workflow-runs/echo-pipeline/<ulid>.jsonl.
    let _dir = TempDir::new().unwrap();
    todo!("lift fixture from cmd_chat.rs / cmd_run.rs or layer4_native.rs");
}
