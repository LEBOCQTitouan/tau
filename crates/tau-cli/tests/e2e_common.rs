//! Shared helpers for `tau serve` e2e tests.
//!
//! Each e2e test file includes this via `#[path = "e2e_common.rs"] mod e2e_common;`.

#![allow(dead_code)]

/// Ensure `$HOME` is set so the spawned `tau` process can resolve its scope.
///
/// `tau_pkg::Scope::resolve` reads `$HOME`. GitHub Actions Windows runners
/// don't set `$HOME` (Windows uses `%USERPROFILE%`), and the env is inherited
/// by the spawned subprocess. Fall back to `USERPROFILE`, `TEMP`, or `/tmp`.
///
/// Idempotent — no-op when `HOME` is already set.
pub fn ensure_home_env() {
    if std::env::var_os("HOME").is_some() {
        return;
    }
    let fallback = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("TEMP"))
        .unwrap_or_else(|| std::ffi::OsString::from("/tmp"));
    std::env::set_var("HOME", fallback);
}
