//! `fs-read` plugin configuration.
//!
//! v0.1 has no knobs; the empty config still goes through
//! `Configure::from_config` for round-trip consistency with the SDK
//! handshake.
//!
//! See `docs/superpowers/specs/2026-04-29-tool-plugins-design.md` §6.1.

use serde::Deserialize;

/// Top-level config for the fs-read plugin.
///
/// Reserved for future expansion (e.g. `follow_symlinks: bool`).
/// `#[non_exhaustive]` so additive fields remain non-breaking.
///
/// # Example
///
/// ```ignore
/// // `FsReadConfig` is `#[non_exhaustive]`; external callers
/// // construct via serde or Default.
/// use fs_read_plugin_lib::config::FsReadConfig;
/// let cfg = FsReadConfig::default();
/// let _ = cfg;
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FsReadConfig {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_empty() {
        let _cfg = FsReadConfig::default();
    }

    #[test]
    fn deserializes_empty_object() {
        let cfg: FsReadConfig = serde_json::from_str("{}").unwrap();
        let _ = cfg;
    }

    #[test]
    fn rejects_unknown_fields() {
        let result: Result<FsReadConfig, _> = serde_json::from_str(r#"{"unknown":"x"}"#);
        assert!(result.is_err());
    }
}
