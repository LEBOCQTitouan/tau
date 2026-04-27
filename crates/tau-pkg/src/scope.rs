//! Scope detection, configuration, and management for `tau-pkg`.
//!
//! Defines:
//!
//! - [`ScopeKind`] — `Global` vs `Project`. Determines path conventions
//!   (project scope keeps its lockfile at the project root and `.tau/`
//!   under it; global scope keeps everything under `~/.tau`).
//! - [`ScopeConfig`] — the on-disk `<scope>/config.toml` schema. Records
//!   `schema_version`, `kind`, `created_at`, `created_by_tau_version`,
//!   and a forward-compatible `defaults` map.
//!
//! The `Scope` struct itself (with `resolve()`, `global()`, `new_project()`,
//! and path accessors) lands in Task 5.

use std::collections::BTreeMap;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::ScopeError;

/// Maximum `ScopeConfig::schema_version` this tau version recognizes.
/// A `config.toml` with a higher value rejects with
/// [`ScopeError::ConfigSchemaTooNew`].
pub const MAX_SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// Distinguishes a global scope (default `~/.tau`) from a project scope
/// (a `.tau/` directory inside a project's source tree).
///
/// Serialized lowercase (`"global"`, `"project"`) so TOML reads naturally.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    /// Global scope (default `~/.tau`, or `$TAU_HOME` / `$XDG_DATA_HOME/tau`).
    Global,
    /// Project scope (a `.tau/` directory in the project's source tree).
    Project,
}

/// Schema for `<scope>/config.toml`. Future-grown additively.
///
/// Stored at `<scope.state_path()>/config.toml`. Created automatically
/// by `Scope::global()` (Task 5) when missing, and by
/// `Scope::new_project()` (Task 5) when a new project scope is materialized.
///
/// Round-trips through TOML via `serde`. `humantime-serde` produces
/// RFC-3339 timestamps so the file is human-readable.
///
/// # Example
///
/// ```ignore
/// // Constructed via [`ScopeConfig::new`] (the type is `#[non_exhaustive]`,
/// // so direct struct-literal construction is forbidden from external crates).
/// use tau_pkg::scope::{ScopeConfig, ScopeKind};
///
/// let cfg = ScopeConfig::new(ScopeKind::Global);
/// let toml = cfg.to_toml_string().unwrap();
/// assert!(toml.contains("schema_version = 1"));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopeConfig {
    /// Schema version of this config. v0.1 ships `1`.
    pub schema_version: u32,
    /// Whether this is a global or project scope.
    pub kind: ScopeKind,
    /// When the scope was first materialized.
    #[serde(with = "humantime_serde")]
    pub created_at: SystemTime,
    /// `CARGO_PKG_VERSION` of the tau-pkg crate that created this scope.
    pub created_by_tau_version: String,
    /// Reserved for future scope-level defaults (default LLM backend,
    /// timeouts, etc.). Empty at v0.1.
    #[serde(default)]
    pub defaults: BTreeMap<String, tau_domain::Value>,
}

impl ScopeConfig {
    /// Construct a new `ScopeConfig` with the current time, the current
    /// crate version, schema version 1, and an empty defaults map.
    pub fn new(kind: ScopeKind) -> Self {
        Self {
            schema_version: MAX_SUPPORTED_SCHEMA_VERSION,
            kind,
            created_at: SystemTime::now(),
            created_by_tau_version: env!("CARGO_PKG_VERSION").to_owned(),
            defaults: BTreeMap::new(),
        }
    }

    /// Parse a `ScopeConfig` from a TOML string. Validates
    /// `schema_version` against [`MAX_SUPPORTED_SCHEMA_VERSION`].
    pub fn read_from_str(text: &str) -> Result<Self, ScopeError> {
        let cfg: Self = toml::from_str(text).map_err(|e| ScopeError::ConfigParse {
            reason: e.to_string(),
        })?;
        if cfg.schema_version > MAX_SUPPORTED_SCHEMA_VERSION {
            return Err(ScopeError::ConfigSchemaTooNew {
                found: cfg.schema_version,
                supported: MAX_SUPPORTED_SCHEMA_VERSION,
            });
        }
        Ok(cfg)
    }

    /// Serialize the config as TOML.
    pub fn to_toml_string(&self) -> Result<String, ScopeError> {
        toml::to_string_pretty(self).map_err(|e| ScopeError::Internal {
            message: format!("config TOML serialization: {e}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_kind_serde_lowercase_global() {
        let json = serde_json::to_string(&ScopeKind::Global).unwrap();
        assert_eq!(json, "\"global\"");
    }

    #[test]
    fn scope_kind_serde_lowercase_project() {
        let json = serde_json::to_string(&ScopeKind::Project).unwrap();
        assert_eq!(json, "\"project\"");
    }

    #[test]
    fn scope_config_new_populates_defaults() {
        let cfg = ScopeConfig::new(ScopeKind::Global);
        assert_eq!(cfg.schema_version, 1);
        assert_eq!(cfg.kind, ScopeKind::Global);
        assert_eq!(cfg.created_by_tau_version, env!("CARGO_PKG_VERSION"));
        assert!(cfg.defaults.is_empty());
    }

    #[test]
    fn scope_config_round_trips_through_toml() {
        let cfg = ScopeConfig::new(ScopeKind::Project);
        let toml_str = cfg.to_toml_string().unwrap();
        let parsed = ScopeConfig::read_from_str(&toml_str).unwrap();

        assert_eq!(parsed.schema_version, cfg.schema_version);
        assert_eq!(parsed.kind, cfg.kind);
        assert_eq!(parsed.created_by_tau_version, cfg.created_by_tau_version);
        assert!(parsed.defaults.is_empty());

        // SystemTime round-trip via humantime_serde may lose sub-second
        // precision; compare at second granularity.
        let cfg_secs = cfg
            .created_at
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let parsed_secs = parsed
            .created_at
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(parsed_secs, cfg_secs);
    }

    #[test]
    fn scope_config_rejects_too_new_schema_version() {
        let cfg = r#"
            schema_version = 999
            kind = "global"
            created_at = "2026-04-27T10:00:00Z"
            created_by_tau_version = "0.0.0"
        "#;
        let err = ScopeConfig::read_from_str(cfg).unwrap_err();
        assert!(matches!(
            err,
            ScopeError::ConfigSchemaTooNew {
                found: 999,
                supported: 1,
            }
        ));
    }

    #[test]
    fn scope_config_rejects_malformed_toml() {
        let bad = "this is not valid TOML = = =";
        let err = ScopeConfig::read_from_str(bad).unwrap_err();
        assert!(matches!(err, ScopeError::ConfigParse { .. }));
    }

    #[test]
    fn scope_config_defaults_field_optional_in_toml() {
        let cfg = r#"
            schema_version = 1
            kind = "global"
            created_at = "2026-04-27T10:00:00Z"
            created_by_tau_version = "0.0.0"
        "#;
        let parsed = ScopeConfig::read_from_str(cfg).unwrap();
        assert!(parsed.defaults.is_empty());
    }
}
