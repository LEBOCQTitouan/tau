//! Snapshot tests for `--json` output schemas. Catches accidental schema drift
//! across subcommand outputs that consumers (CI scripts, dashboards) depend on.

mod common;

use assert_cmd::Command;
use insta::assert_json_snapshot;
use serde_json::Value;

fn run_and_parse_json(
    args: &[&str],
    envs: &[(&str, &std::path::Path)],
    cwd: Option<&std::path::Path>,
) -> Value {
    let mut cmd = Command::cargo_bin("tau").unwrap();
    cmd.args(args);
    for (key, val) in envs {
        cmd.env(key, val);
    }
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "command failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    serde_json::from_str(stdout.trim()).expect("valid JSON")
}

#[test]
fn json_schema_init() {
    let dir = common::temp_project();
    let val = run_and_parse_json(&["init", "--json"], &[], Some(dir.path()));
    // Mask the dynamic path; insta supports redactions:
    assert_json_snapshot!("init_json", val, {
        ".path" => "[PATH]"
    });
}

#[test]
fn json_schema_install() {
    let (fixture, url, _bare) = common::setup_local_package_fixture("hello-tool", "0.1.0");
    let global_dir = fixture.path().join("scope-global");
    std::fs::create_dir_all(&global_dir).unwrap();

    let val = run_and_parse_json(
        &["install", "--global", "--json", &url],
        &[("TAU_HOME", global_dir.as_path())],
        None,
    );
    assert_json_snapshot!("install_json", val, {
        ".path" => "[PATH]"
    });
}

#[test]
fn json_schema_list_packages_empty() {
    let global_dir = tempfile::tempdir().unwrap();
    let val = run_and_parse_json(
        &["list", "--global", "--json"],
        &[("TAU_HOME", global_dir.path())],
        None,
    );
    assert_json_snapshot!("list_packages_empty_json", val);
}

#[test]
fn json_schema_list_agents() {
    let dir = tempfile::tempdir().unwrap();
    let toml = r#"
[project]
name = "demo"

[agents.reviewer]
display_name = "Code Reviewer"
package      = "code-reviewer@^0.1"
llm_backend  = "anthropic"
"#;
    std::fs::write(dir.path().join("tau.toml"), toml).unwrap();

    let val = run_and_parse_json(
        &["list", "agents", "--json"],
        &[("TAU_HOME", dir.path())],
        Some(dir.path()),
    );
    assert_json_snapshot!("list_agents_json", val);
}

#[cfg(feature = "test-mock")]
#[test]
fn json_schema_run_completed() {
    let dir = common::setup_project_with_installed_agent(
        "reviewer",
        "code-reviewer",
        "0.1.0",
        "mock-llm",
    );
    let global_dir = dir.path().join("global");

    let mut cmd = Command::cargo_bin("tau").unwrap();
    let output = cmd
        .args(["run", "reviewer", "hi", "--json"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .env("TAU_MOCK_LLM_TEXT", "ok")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected success; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let val: Value = serde_json::from_str(std::str::from_utf8(&output.stdout).unwrap().trim())
        .expect("valid JSON");
    assert_json_snapshot!("run_completed_json", val);
}

#[cfg(feature = "test-mock")]
#[test]
fn json_schema_run_failed() {
    let dir = common::setup_project_with_installed_agent(
        "reviewer",
        "code-reviewer",
        "0.1.0",
        "mock-llm",
    );
    let global_dir = dir.path().join("global");

    let mut cmd = Command::cargo_bin("tau").unwrap();
    let output = cmd
        .args(["run", "reviewer", "hi", "--json", "--max-turns", "1"])
        .current_dir(dir.path())
        .env("TAU_HOME", &global_dir)
        .env("TAU_MOCK_LLM_TOOL_USES", "echo")
        .output()
        .unwrap();
    // Failed exit code 1
    assert_eq!(output.status.code(), Some(1));
    let val: Value = serde_json::from_str(std::str::from_utf8(&output.stdout).unwrap().trim())
        .expect("valid JSON");
    assert_json_snapshot!("run_failed_json", val);
}
