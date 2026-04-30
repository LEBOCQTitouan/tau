//! `shell` plugin configuration.
//!
//! Two knobs tune the wall-clock timeout: `default_timeout_secs`
//! (used when args.timeout_secs is None) and `max_timeout_secs`
//! (caps args.timeout_secs).
//!
//! See `docs/superpowers/specs/2026-04-29-tool-plugins-design.md`
//! §6.2.

use serde::Deserialize;
use tau_plugin_sdk::ConfigError;

/// Top-level config for the shell plugin.
///
/// `#[non_exhaustive]` so additive fields are non-breaking.
///
/// # Example
///
/// ```ignore
/// // `ShellConfig` is `#[non_exhaustive]`; external callers
/// // construct via serde or Default.
/// use shell_plugin_lib::config::ShellConfig;
/// let cfg = ShellConfig::default();
/// assert_eq!(cfg.default_timeout_secs, 30);
/// assert_eq!(cfg.max_timeout_secs, 600);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellConfig {
    /// Default wall-clock timeout in seconds when `args.timeout_secs`
    /// is absent. Default 30.
    #[serde(default = "default_timeout_secs")]
    pub default_timeout_secs: u64,

    /// Maximum wall-clock timeout in seconds (caps `args.timeout_secs`).
    /// Default 600 (10 min).
    #[serde(default = "default_max_timeout_secs")]
    pub max_timeout_secs: u64,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: default_timeout_secs(),
            max_timeout_secs: default_max_timeout_secs(),
        }
    }
}

fn default_timeout_secs() -> u64 {
    30
}
fn default_max_timeout_secs() -> u64 {
    600
}

/// Validate `ShellConfig` invariants.
///
/// Returns `ConfigError::InvalidValue` when:
/// - either timeout is zero, OR
/// - `default_timeout_secs > max_timeout_secs`.
pub(crate) fn validate(cfg: &ShellConfig) -> Result<(), ConfigError> {
    if cfg.default_timeout_secs == 0 {
        return Err(ConfigError::InvalidValue {
            field: "default_timeout_secs",
            detail: "must be >= 1".into(),
        });
    }
    if cfg.max_timeout_secs == 0 {
        return Err(ConfigError::InvalidValue {
            field: "max_timeout_secs",
            detail: "must be >= 1".into(),
        });
    }
    if cfg.default_timeout_secs > cfg.max_timeout_secs {
        return Err(ConfigError::InvalidValue {
            field: "default_timeout_secs",
            detail: format!(
                "default ({}) must be <= max ({})",
                cfg.default_timeout_secs, cfg.max_timeout_secs,
            ),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_30_and_600() {
        let cfg = ShellConfig::default();
        assert_eq!(cfg.default_timeout_secs, 30);
        assert_eq!(cfg.max_timeout_secs, 600);
    }

    #[test]
    fn validate_default_greater_than_max_rejected() {
        let cfg = ShellConfig {
            default_timeout_secs: 100,
            max_timeout_secs: 50,
        };
        let err = validate(&cfg).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidValue { .. }));
    }

    #[test]
    fn validate_zero_default_rejected() {
        let cfg = ShellConfig {
            default_timeout_secs: 0,
            max_timeout_secs: 600,
        };
        let err = validate(&cfg).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidValue { .. }));
    }

    #[test]
    fn validate_zero_max_rejected() {
        let cfg = ShellConfig {
            default_timeout_secs: 30,
            max_timeout_secs: 0,
        };
        let err = validate(&cfg).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidValue { .. }));
    }

    #[test]
    fn validate_happy_path() {
        validate(&ShellConfig::default()).unwrap();
    }
}
