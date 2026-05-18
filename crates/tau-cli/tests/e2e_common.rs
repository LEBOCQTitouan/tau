//! Shared helpers for `tau serve` e2e tests.
//!
//! Each e2e test file includes this via `#[path = "e2e_common.rs"] mod e2e_common;`.

#![allow(dead_code)]

/// Ensure tau's global scope can be resolved.
///
/// Sets `$TAU_HOME` to a sensible existing directory if it's unset.
/// Matches the pattern used by `crates/tau-cli/tests/cmd_resolve.rs`.
/// The spawned `tau` subprocess inherits the env.
pub fn ensure_home_env() {
    if std::env::var_os("TAU_HOME").is_some() {
        return;
    }
    let fallback = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .or_else(|| std::env::var_os("TEMP"))
        .unwrap_or_else(|| std::ffi::OsString::from("/tmp"));
    std::env::set_var("TAU_HOME", fallback);
}
