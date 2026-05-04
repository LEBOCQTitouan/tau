//! Layer 4 native live spawn tests — sub-project B Task 8.
//!
//! Each test installs a real plugin binary into a tempdir scope, then
//! drives a golden-path agent invocation under the Native adapter
//! (`--sandbox native`) which engages landlock + seccomp + namespaces.
//! These tests exercise Task 3's symlink-resolution fix
//! (`resolve_symlinks_for_landlock`) — landlock V1 path lookup against
//! Ubuntu's `/bin → /usr/bin` symlinks.
//!
//! # v0.1 scope (Task 8)
//!
//! All 5 tests are scaffolded but `#[ignore]`'d pending sub-project D's
//! e2e infrastructure. The full `tau run → kernel → plugin_host →
//! NativeAdapter → landlock+seccomp` pipeline is non-trivial to drive
//! from a test harness without infrastructure that sub-project D will
//! provide (controlled-environment binary spawning, real-kernel
//! verification on `ubuntu-latest`, etc.).
//!
//! Until then, these are placeholders that compile cleanly and document
//! intent. When sub-project D lands, the `#[ignore]` attributes get
//! flipped and the test bodies are filled in.
//!
//! # Linux-only
//!
//! The `tau-sandbox-native` adapter is Linux-only. This file is gated
//! with `cfg(target_os = "linux")` so non-Linux platforms compile
//! cleanly without the test bodies needing platform-specific code paths.

#![cfg(feature = "integration-tests")]
#![cfg(target_os = "linux")]

#[test]
#[ignore = "Pending sub-project D's e2e landlock+seccomp infrastructure"]
fn shell_layer4_native_runs_echo_hello() {
    // Will install crates/tau-plugins/shell from workspace path,
    // run a tool invocation under --sandbox native, assert "hello"
    // appears in stdout. Exercises Task 3's symlink-resolution fix.
}

#[test]
#[ignore = "Pending sub-project D's e2e landlock+seccomp infrastructure"]
fn fs_read_layer4_native_reads_data_file() {
    // Will install crates/tau-plugins/fs-read, run a tool invocation
    // reading a tempdir data.txt under --sandbox native. Exercises
    // landlock fs.read enforcement.
}

#[test]
#[ignore = "Pending sub-project D's e2e infrastructure + cassette-replay through sandboxed plugin"]
fn anthropic_layer4_native_completes_via_cassette() {
    // HTTP plugin under --sandbox native. Cassette-replay; no real network.
}

#[test]
#[ignore = "Pending sub-project D's e2e infrastructure + cassette-replay through sandboxed plugin"]
fn ollama_layer4_native_completes_via_cassette() {
    // HTTP plugin under --sandbox native. Cassette-replay; no real network.
}

#[test]
#[ignore = "Pending sub-project D's e2e infrastructure + cassette-replay through sandboxed plugin"]
fn openai_layer4_native_completes_via_cassette() {
    // HTTP plugin under --sandbox native. Cassette-replay; no real network.
}
