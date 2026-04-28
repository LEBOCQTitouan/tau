//! Snapshot tests for `--help` text on every subcommand and the top-level CLI.
//! Catches accidental help-text regressions.

use assert_cmd::Command;

fn capture_help(args: &[&str]) -> String {
    let output = Command::cargo_bin("tau")
        .unwrap()
        .args(args)
        .output()
        .expect("binary runs");
    // clap's `--help` exits with status 0 and writes to stdout.
    // Normalize line endings: snapshots are recorded with `\n` on
    // macOS/Linux; Windows captures `\r\n`. Convert before snapshotting
    // so the same `.snap` files are valid on every host.
    String::from_utf8(output.stdout).expect("utf8").replace("\r\n", "\n")
}

#[test]
fn snapshot_top_level_help() {
    let s = capture_help(&["--help"]);
    insta::assert_snapshot!("top_level_help", s);
}

#[test]
fn snapshot_init_help() {
    let s = capture_help(&["init", "--help"]);
    insta::assert_snapshot!("init_help", s);
}

#[test]
fn snapshot_install_help() {
    let s = capture_help(&["install", "--help"]);
    insta::assert_snapshot!("install_help", s);
}

#[test]
fn snapshot_list_help() {
    let s = capture_help(&["list", "--help"]);
    insta::assert_snapshot!("list_help", s);
}

#[test]
fn snapshot_run_help() {
    let s = capture_help(&["run", "--help"]);
    insta::assert_snapshot!("run_help", s);
}

#[test]
fn snapshot_chat_help() {
    let s = capture_help(&["chat", "--help"]);
    insta::assert_snapshot!("chat_help", s);
}
