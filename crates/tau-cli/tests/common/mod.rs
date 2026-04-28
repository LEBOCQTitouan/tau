//! Shared test helpers for tau-cli integration tests.

#![allow(dead_code)]

use std::path::Path;

use tempfile::TempDir;

/// Create a tempdir to use as the project cwd.
pub fn temp_project() -> TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// Create a tempdir with a tau.toml of the given contents.
pub fn temp_project_with_tau_toml(contents: &str) -> TempDir {
    let dir = temp_project();
    std::fs::write(dir.path().join("tau.toml"), contents).expect("write tau.toml");
    dir
}

/// Read the tau.toml from a tempdir; panic if missing.
pub fn read_tau_toml(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("tau.toml")).expect("read tau.toml")
}
