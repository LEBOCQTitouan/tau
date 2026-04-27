//! Lockfile types — `tau-lock.toml` schema.
//!
//! The lockfile records every installed package per scope:
//!
//! - **Project scope:** `<project>/tau-lock.toml` (lives at the project
//!   root and is **committed** to the project's git repository).
//! - **Global scope:** `<scope.path()>/tau-lock.toml` (typically
//!   `~/.tau/tau-lock.toml`; **local state**, not committed).
//!
//! TOML round-trip uses `humantime-serde` so timestamps are RFC-3339
//! strings (human-readable in diffs). `schema_version` is bumped only
//! on breaking changes; lockfiles with a newer version than this tau
//! version supports are rejected via [`crate::RegistryError::SchemaTooNew`].
//!
//! The `sha256` slot on [`LockedVersion`] is reserved for content
//! hashing in Phase 1+ (`tau verify`); it is left empty at v0.1.
//!
//! [`LockFile::load`]/[`save`]/[`find`]/[`upsert`]/[`remove`] land in
//! Task 7. This file (Task 6) defines only the data shapes + `Default`.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tau_domain::{PackageName, PackageSource, Version};

/// Maximum `LockFile::schema_version` this tau version recognizes.
/// A `tau-lock.toml` with a higher value is rejected by
/// `LockFile::load` (Task 7) via `RegistryError::SchemaTooNew`.
pub const MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION: u32 = 1;

/// Schema for `tau-lock.toml`.
///
/// Project scope: lives at `<project>/tau-lock.toml` (committed).
/// Global scope: lives at `~/.tau/tau-lock.toml` (local state).
///
/// # Example
///
/// ```ignore
/// // `LockFile` is `#[non_exhaustive]`; constructed via [`LockFile::default`].
/// use tau_pkg::lockfile::LockFile;
///
/// let lf = LockFile::default();
/// assert_eq!(lf.schema_version, 1);
/// assert!(lf.packages.is_empty());
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockFile {
    /// Schema version. v0.1 ships `1`. Bumped on breaking changes only.
    pub schema_version: u32,
    /// `CARGO_PKG_VERSION` of the tau-pkg crate that last wrote this file.
    pub generated_by_tau_version: String,
    /// Timestamp of the last [`Self::default`] or `save()` call. Set to
    /// `SystemTime::now()` on construction so a freshly-defaulted but
    /// not-yet-saved `LockFile` already carries a value.
    #[serde(with = "humantime_serde")]
    pub generated_at: SystemTime,
    /// Installed packages. Renamed to `[[package]]` in TOML output for
    /// natural diff output.
    #[serde(default, rename = "package")]
    pub packages: Vec<LockedPackage>,
}

/// One installed package's lockfile entry.
///
/// `active_version` is the version the runtime loads when no version
/// pin is supplied. `installed_versions` records every version
/// currently materialized on disk for this package (multi-version
/// cohabitation per scope).
///
/// # Example
///
/// ```ignore
/// // `LockedPackage` is `#[non_exhaustive]`; in tests, construct via
/// // struct literal from within the crate. External callers receive
/// // values from `LockFile::find` / `list` / `get`.
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockedPackage {
    /// Validated package name, from `tau_domain::PackageName`.
    pub name: PackageName,
    /// The version the runtime loads by default for this package.
    pub active_version: Version,
    /// Where the package was fetched from.
    pub source: PackageSource,
    /// Every version currently installed on disk. Renamed to
    /// `[[package.versions]]` in TOML output.
    #[serde(default, rename = "versions")]
    pub installed_versions: Vec<LockedVersion>,
}

/// One installed version's lockfile entry.
///
/// `rev` is opaque user input (branch name, tag, or 40-char SHA);
/// `resolved_commit` is the 40-char SHA that `git rev-parse HEAD`
/// produced after the clone. Together they support reproducible
/// installs even when the user pinned a moving branch.
///
/// `sha256` is reserved for Phase-1 content hashing (`tau verify`)
/// and is left empty at v0.1.
///
/// # Example
///
/// ```ignore
/// // `LockedVersion` is `#[non_exhaustive]`; constructed by the install
/// // lifecycle (Task 10) and consumed by `LockFile` accessors (Task 7).
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockedVersion {
    /// The version this entry refers to.
    pub version: Version,
    /// Branch name, tag, or SHA as supplied by the user (opaque).
    pub rev: Option<String>,
    /// Full 40-char commit SHA after `git rev-parse HEAD` at install time.
    pub resolved_commit: String,
    /// Reserved for Phase-1 `tau verify` content hashing. Empty at v0.1.
    #[serde(default)]
    pub sha256: String,
    /// When this version was installed.
    #[serde(with = "humantime_serde")]
    pub installed_at: SystemTime,
}

impl Default for LockFile {
    fn default() -> Self {
        Self {
            schema_version: MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION,
            generated_by_tau_version: env!("CARGO_PKG_VERSION").to_owned(),
            generated_at: SystemTime::now(),
            packages: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::{Duration, UNIX_EPOCH};

    fn fixture_locked_version() -> LockedVersion {
        LockedVersion {
            version: "1.2.3".parse().unwrap(),
            rev: Some("main".into()),
            resolved_commit: "0123456789abcdef0123456789abcdef01234567".into(),
            sha256: String::new(),
            installed_at: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
        }
    }

    fn fixture_locked_package() -> LockedPackage {
        LockedPackage {
            name: "acme-tool".parse().unwrap(),
            active_version: "1.2.3".parse().unwrap(),
            source: "https://example.com/acme/tool.git#main".parse().unwrap(),
            installed_versions: vec![fixture_locked_version()],
        }
    }

    #[test]
    fn default_lockfile_has_schema_version_one() {
        let lf = LockFile::default();
        assert_eq!(lf.schema_version, MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION);
        assert_eq!(lf.schema_version, 1);
    }

    #[test]
    fn default_lockfile_has_empty_packages() {
        let lf = LockFile::default();
        assert!(lf.packages.is_empty());
    }

    #[test]
    fn default_lockfile_records_tau_version() {
        let lf = LockFile::default();
        assert_eq!(lf.generated_by_tau_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn locked_version_constructs_with_required_fields() {
        let lv = fixture_locked_version();
        assert_eq!(lv.version.to_string(), "1.2.3");
        assert_eq!(lv.rev.as_deref(), Some("main"));
        assert_eq!(lv.resolved_commit.len(), 40);
        assert!(lv.sha256.is_empty());
    }

    #[test]
    fn locked_package_constructs_with_required_fields() {
        let lp = fixture_locked_package();
        assert_eq!(lp.name.as_str(), "acme-tool");
        assert_eq!(lp.active_version.to_string(), "1.2.3");
        assert_eq!(lp.installed_versions.len(), 1);
    }

    #[test]
    fn lockfile_round_trips_through_toml_with_packages() {
        let mut lf = LockFile::default();
        lf.packages.push(fixture_locked_package());

        let toml_str = toml::to_string_pretty(&lf).unwrap();
        let parsed: LockFile = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.schema_version, lf.schema_version);
        assert_eq!(parsed.packages.len(), 1);
        assert_eq!(parsed.packages[0].name.as_str(), "acme-tool");
        assert_eq!(
            parsed.packages[0].installed_versions[0].resolved_commit,
            lf.packages[0].installed_versions[0].resolved_commit
        );

        // SystemTime round-trip via humantime_serde may lose sub-second
        // precision; compare at second granularity.
        let original_secs = lf
            .generated_at
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let parsed_secs = parsed
            .generated_at
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(parsed_secs, original_secs);
    }

    #[test]
    fn lockfile_round_trips_when_empty() {
        let lf = LockFile::default();
        let toml_str = toml::to_string_pretty(&lf).unwrap();
        let parsed: LockFile = toml::from_str(&toml_str).unwrap();
        assert!(parsed.packages.is_empty());
    }

    #[test]
    fn lockfile_uses_package_array_table_in_toml() {
        let mut lf = LockFile::default();
        lf.packages.push(fixture_locked_package());

        let toml_str = toml::to_string_pretty(&lf).unwrap();

        // The #[serde(rename = "package")] turns Vec<LockedPackage>
        // into [[package]] in TOML output. Confirm the rename took effect.
        assert!(
            toml_str.contains("[[package]]"),
            "expected `[[package]]` in TOML output; got:\n{toml_str}"
        );
    }

    #[test]
    fn locked_package_uses_versions_array_table_in_toml() {
        let mut lf = LockFile::default();
        lf.packages.push(fixture_locked_package());

        let toml_str = toml::to_string_pretty(&lf).unwrap();

        // The #[serde(rename = "versions")] gives [[package.versions]].
        assert!(
            toml_str.contains("[[package.versions]]"),
            "expected `[[package.versions]]` in TOML output; got:\n{toml_str}"
        );
    }

    #[test]
    fn lockfile_parses_when_packages_field_omitted() {
        // #[serde(default)] should let a TOML doc with no [[package]] parse cleanly.
        let toml_str = r#"
            schema_version = 1
            generated_by_tau_version = "0.0.0"
            generated_at = "2026-04-27T10:00:00Z"
        "#;
        let parsed: LockFile = toml::from_str(toml_str).unwrap();
        assert!(parsed.packages.is_empty());
        assert_eq!(parsed.schema_version, 1);
    }

    #[test]
    fn locked_version_sha256_defaults_to_empty_when_missing() {
        let toml_str = r#"
            version = "1.0.0"
            resolved_commit = "0123456789abcdef0123456789abcdef01234567"
            installed_at = "2026-04-27T10:00:00Z"
        "#;
        // rev is Option<String> — None is fine when missing.
        let parsed: LockedVersion = toml::from_str(toml_str).unwrap();
        assert!(parsed.sha256.is_empty());
        assert!(parsed.rev.is_none());
    }

    #[test]
    fn locked_version_round_trips_with_sha256_present() {
        let mut lv = fixture_locked_version();
        lv.sha256 = "deadbeef".to_string().repeat(8); // 64-char hex
        let toml_str = toml::to_string_pretty(&lv).unwrap();
        let parsed: LockedVersion = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.sha256, lv.sha256);
    }
}
