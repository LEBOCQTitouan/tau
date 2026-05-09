//! Per-plugin startup-IO path helpers for Layer 4 tests.
//!
//! The `tau-sandbox-native::light::BASELINE_SYSTEM_READ_PATHS` runtime
//! constant covers paths that every Rust binary needs (libc/dyld bootstrap,
//! `/proc/self`, `/sys/fs/cgroup`, etc.). This module supplies the
//! long-tail of plugin-specific paths beyond that baseline — for example,
//! an HTTP plugin that reads a distribution-specific TLS root cert bundle
//! that isn't covered by the `/etc` baseline entry.
//!
//! Tests in `tau-plugin-compat/tests/layer4_native.rs` call
//! `startup_io_paths_for(plugin_bin)` and add the returned paths as an
//! additional `cap_fs_read` entry in the test's `SandboxPlan`.

/// Return plugin-specific filesystem paths needed at startup that aren't
/// already covered by `tau-sandbox-native`'s
/// `BASELINE_SYSTEM_READ_PATHS`.
///
/// `plugin_bin` is the plugin binary's basename (e.g. `"shell-plugin"`,
/// `"anthropic-plugin"`). Unknown plugins return an empty slice — the
/// caller is responsible for either providing a binding here or relying
/// solely on the runtime baseline.
///
/// Empty arms for shell + fs-read in PR 1 reflect that those plugins
/// don't touch any plugin-specific paths beyond the runtime baseline
/// (T1 findings 2026-05-09). HTTP plugin arms (anthropic, ollama, openai)
/// are populated in PR 2 (`feat/layer4-startup-io-http`).
pub fn startup_io_paths_for(plugin_bin: &str) -> &'static [&'static str] {
    match plugin_bin {
        // PR 1 — simple plugins. No plugin-specific paths needed beyond
        // the runtime baseline (per T1 findings).
        "shell-plugin" => &[],
        "fs-read-plugin" => &[],
        // PR 2 — HTTP plugins. Populated in feat/layer4-startup-io-http.
        "anthropic-plugin" => &[],
        "ollama-plugin" => &[],
        "openai-plugin" => &[],
        // Unknown plugin: caller bears responsibility.
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::startup_io_paths_for;

    #[test]
    fn shell_plugin_has_no_extras_in_pr1() {
        assert!(
            startup_io_paths_for("shell-plugin").is_empty(),
            "shell does not need plugin-specific startup paths in PR 1"
        );
    }

    #[test]
    fn fs_read_plugin_has_no_extras_in_pr1() {
        assert!(
            startup_io_paths_for("fs-read-plugin").is_empty(),
            "fs-read does not need plugin-specific startup paths in PR 1"
        );
    }

    #[test]
    fn unknown_plugin_returns_empty() {
        assert!(startup_io_paths_for("nonexistent-plugin").is_empty());
    }
}
