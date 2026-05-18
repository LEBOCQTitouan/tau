//! Shared helpers for `tau serve` e2e tests.
//!
//! Each e2e test file includes this via `#[path = "e2e_common.rs"] mod e2e_common;`.

#![allow(dead_code)]

/// Ensure tau's global scope can be resolved without racing.
///
/// `tau_pkg::Scope::global` reads `$TAU_HOME` and creates a default
/// `config.toml` in it if missing. Parallel tests writing the same
/// path on Windows produce "Access is denied". We point `$TAU_HOME`
/// at a dedicated process-local tempdir initialized once, with the
/// config.toml pre-created so no test races on writing it.
pub fn ensure_home_env() {
    use std::sync::OnceLock;
    static TAU_HOME_INIT: OnceLock<()> = OnceLock::new();
    TAU_HOME_INIT.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("tau-serve-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create tau_home tempdir");
        let cfg = dir.join("config.toml");
        if !cfg.exists() {
            let _ = std::fs::write(&cfg, "");
        }
        std::env::set_var("TAU_HOME", dir);
    });
}
