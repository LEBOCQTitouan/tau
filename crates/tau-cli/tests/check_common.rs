//! Shared helpers for `tau check` Layer 2 integration tests.

#![allow(dead_code)]

/// Ensure tau's global scope can be resolved and parallel tests don't
/// race on the default `~/.tau/config.toml`.
///
/// Points `$TAU_HOME` at a process-local tempdir initialized once;
/// pre-creates config.toml so concurrent tests don't race on its write.
pub fn ensure_tau_home() {
    use std::sync::OnceLock;
    static TAU_HOME_INIT: OnceLock<()> = OnceLock::new();
    TAU_HOME_INIT.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("tau-check-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create tau_home tempdir");
        let cfg = dir.join("config.toml");
        if !cfg.exists() {
            let _ = std::fs::write(&cfg, "");
        }
        std::env::set_var("TAU_HOME", dir);
    });
}
