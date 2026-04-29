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

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tau_domain::{PackageName, PackageSource, PluginManifest, Version};

use crate::error::RegistryError;

/// Maximum `LockFile::schema_version` this tau version recognizes.
/// A `tau-lock.toml` with a higher value is rejected by
/// `LockFile::load` (Task 7) via `RegistryError::SchemaTooNew`.
///
/// History:
/// - `1` — v0.1: `LockedPackage` had no `plugin` field.
/// - `2` — Plugin loading: `LockedPackage::plugin: Option<LockedPlugin>`
///   added. v1 lockfiles auto-upgrade to v2 on load (the `plugin` field
///   defaults to `None` for legacy entries via `#[serde(default)]`).
pub const MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION: u32 = 2;

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
/// assert_eq!(lf.schema_version, 2);
/// assert!(lf.packages.is_empty());
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockFile {
    /// Schema version. Currently `2`. Bumped on breaking changes only.
    /// v1 lockfiles are accepted on load and auto-upgraded to v2 on the
    /// next save (legacy entries get `plugin = None`).
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
    /// Plugin metadata recorded at install time. `None` for data-only
    /// packages and for legacy v1 lockfile entries (which had no
    /// `plugin` field; `#[serde(default)]` populates it as `None` on
    /// auto-upgrade).
    ///
    /// Added in lockfile schema v2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<LockedPlugin>,
}

/// Recorded build artifact for a plugin package.
///
/// Written by [`crate::install_with_options`] when the installed
/// package's manifest carries a `[plugin]` table and the cargo build
/// step succeeded. Consumed by `tau-runtime` to spawn the plugin
/// subprocess at run time.
///
/// `#[non_exhaustive]`: future fields (e.g. `sha256` of the binary,
/// build features, toolchain version) are non-breaking.
///
/// # Example
///
/// ```ignore
/// // `LockedPlugin` is `#[non_exhaustive]`; constructed by the install
/// // lifecycle (Task 12). External callers (notably tau-runtime
/// // integration tests that synthesize a lockfile against pre-built
/// // binaries) build it via [`LockedPlugin::new`].
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockedPlugin {
    /// Copy of the `[plugin]` table from the package's `tau.toml` at
    /// install time. Frozen here so `tau-runtime` doesn't need to
    /// re-read the manifest from the package source tree.
    pub manifest: PluginManifest,
    /// Canonical absolute path to the built binary
    /// (e.g. `<pkg_dir>/target/release/<bin>`). Set via
    /// `Path::canonicalize` so symlinks and relative components are
    /// resolved at install time.
    pub binary_path: PathBuf,
    /// When the binary was built (the timestamp of the `cargo build`
    /// step that produced it).
    #[serde(with = "humantime_serde")]
    pub built_at: SystemTime,
}

impl LockedPlugin {
    /// Construct a `LockedPlugin`. `#[non_exhaustive]`; external callers
    /// (notably tau-runtime integration tests that synthesize a
    /// lockfile against pre-built binaries) use this constructor.
    pub fn new(manifest: PluginManifest, binary_path: PathBuf, built_at: SystemTime) -> Self {
        Self {
            manifest,
            binary_path,
            built_at,
        }
    }
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

impl LockFile {
    /// Read the lockfile from `path`.
    ///
    /// Returns `LockFile::default()` if the file doesn't exist (lazy creation —
    /// the first install in a scope creates the lockfile via [`Self::save`]).
    ///
    /// # Errors
    ///
    /// - [`RegistryError::Io`] — the file exists but could not be read.
    /// - [`RegistryError::Parse`] — the file is not valid TOML or doesn't
    ///   match the `LockFile` schema.
    /// - [`RegistryError::SchemaTooNew`] — the file's `schema_version` exceeds
    ///   [`MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION`].
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::path::Path;
    /// use tau_pkg::lockfile::LockFile;
    ///
    /// let lf = LockFile::load(Path::new("/nonexistent/tau-lock.toml")).unwrap();
    /// assert!(lf.packages.is_empty()); // lazy creation
    /// ```
    pub fn load(path: &Path) -> Result<Self, RegistryError> {
        match fs::metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LockFile::default());
            }
            Err(e) => {
                return Err(RegistryError::Io {
                    message: format!("reading lockfile metadata {}: {e}", path.display()),
                });
            }
            Ok(_) => {}
        }

        let text = fs::read_to_string(path).map_err(|e| RegistryError::Io {
            message: format!("reading lockfile {}: {e}", path.display()),
        })?;

        let mut parsed: LockFile = toml::from_str(&text).map_err(|e| RegistryError::Parse {
            reason: e.to_string(),
        })?;

        if parsed.schema_version > MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION {
            return Err(RegistryError::SchemaTooNew {
                found: parsed.schema_version,
                supported: MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION,
            });
        }

        // Auto-upgrade v1 lockfiles in memory. v1 had no `plugin` field
        // on `LockedPackage`; `#[serde(default)]` already populates it
        // as `None`. We only need to bump the recorded schema version
        // so the next `save()` writes `schema_version = 2`. Future
        // schema bumps would add additional in-memory transformations
        // here.
        if parsed.schema_version < MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION {
            parsed.schema_version = MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION;
        }

        Ok(parsed)
    }

    /// Atomically write the lockfile to `path`.
    ///
    /// Implementation: write-to-temp-then-rename + `sync_all`. Creates the
    /// parent directory if it doesn't exist. A crash between the write and
    /// the rename leaves the target either non-existent or fully written —
    /// never zero bytes.
    ///
    /// Note: `generated_at` is set at construction time ([`Self::default`]),
    /// not at save time. Callers that want a fresh timestamp must set
    /// `self.generated_at = SystemTime::now()` before calling `save`.
    ///
    /// Note: we do not fsync the parent directory after `persist`. On ext4
    /// (`data=ordered`) and APFS/HFS+ the rename is journaled; a parent
    /// fsync would be belt-and-suspenders. Revisit if tau-pkg targets
    /// FAT32 or other non-journaled mounts.
    ///
    /// # Errors
    ///
    /// - [`RegistryError::Io`] — parent directory creation, temp-file
    ///   creation, write, fsync, or rename failed.
    /// - [`RegistryError::Internal`] — TOML serialization failed (should
    ///   never happen in practice).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use std::path::Path;
    /// use tau_pkg::lockfile::LockFile;
    ///
    /// let lf = LockFile::default();
    /// lf.save(Path::new("/tmp/tau-lock.toml")).unwrap();
    /// ```
    pub fn save(&self, path: &Path) -> Result<(), RegistryError> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));

        fs::create_dir_all(parent).map_err(|e| RegistryError::Io {
            message: format!("creating lockfile directory {}: {e}", parent.display()),
        })?;

        let text = toml::to_string_pretty(self).map_err(|e| RegistryError::Internal {
            message: format!("lockfile serialization: {e}"),
        })?;

        let tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| RegistryError::Io {
            message: format!("creating temp file in {}: {e}", parent.display()),
        })?;

        fs::write(tmp.path(), text.as_bytes()).map_err(|e| RegistryError::Io {
            message: format!("writing temp lockfile {}: {e}", tmp.path().display()),
        })?;

        tmp.as_file().sync_all().map_err(|e| RegistryError::Io {
            message: format!("fsync lockfile {}: {e}", tmp.path().display()),
        })?;

        tmp.persist(path).map_err(|e| RegistryError::Io {
            message: format!(
                "persisting lockfile {} -> {}: {}",
                e.file.path().display(),
                path.display(),
                e.error,
            ),
        })?;

        Ok(())
    }

    /// Find a package by name.
    ///
    /// Linear scan; O(n) with tiny n (packages per scope).
    /// Returns `None` if the package is not in the lockfile.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tau_pkg::lockfile::LockFile;
    ///
    /// let lf = LockFile::default();
    /// let name: tau_domain::PackageName = "acme-tool".parse().unwrap();
    /// assert!(lf.find(&name).is_none());
    /// ```
    pub fn find(&self, name: &PackageName) -> Option<&LockedPackage> {
        self.packages.iter().find(|p| p.name == *name)
    }

    /// Insert or update a package entry.
    ///
    /// If a package with the same name already exists, it is replaced
    /// **in place** (preserving insertion order for other packages).
    /// Otherwise the package is appended.
    ///
    /// Used by `install()` (Task 10).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tau_pkg::lockfile::LockFile;
    ///
    /// let mut lf = LockFile::default();
    /// // lf.upsert(pkg);  // pkg: LockedPackage
    /// ```
    pub fn upsert(&mut self, package: LockedPackage) {
        if let Some(existing) = self.packages.iter_mut().find(|p| p.name == package.name) {
            *existing = package;
        } else {
            self.packages.push(package);
        }
    }

    /// Remove a package entry by name.
    ///
    /// Returns the removed entry if present, `None` otherwise.
    /// Preserves insertion order of the remaining entries
    /// (`Vec::remove`, not `swap_remove`).
    ///
    /// Used by `uninstall()` (Task 11).
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tau_pkg::lockfile::LockFile;
    ///
    /// let mut lf = LockFile::default();
    /// let name: tau_domain::PackageName = "acme-tool".parse().unwrap();
    /// assert!(lf.remove(&name).is_none());
    /// ```
    pub fn remove(&mut self, name: &PackageName) -> Option<LockedPackage> {
        let pos = self.packages.iter().position(|p| p.name == *name)?;
        Some(self.packages.remove(pos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::{Duration, UNIX_EPOCH};

    use tempfile::TempDir;

    use crate::error::RegistryError;

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
            plugin: None,
        }
    }

    #[test]
    fn default_lockfile_has_current_schema_version() {
        let lf = LockFile::default();
        assert_eq!(lf.schema_version, MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION);
        assert_eq!(lf.schema_version, 2);
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
            schema_version = 2
            generated_by_tau_version = "0.0.0"
            generated_at = "2026-04-27T10:00:00Z"
        "#;
        let parsed: LockFile = toml::from_str(toml_str).unwrap();
        assert!(parsed.packages.is_empty());
        assert_eq!(parsed.schema_version, 2);
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

    // ---- load() ----

    #[test]
    fn load_returns_default_when_file_missing() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("does-not-exist.toml");
        let lf = LockFile::load(&path).unwrap();
        assert!(lf.packages.is_empty());
        assert_eq!(lf.schema_version, MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION);
    }

    #[test]
    fn load_round_trips_a_saved_lockfile() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tau-lock.toml");

        let mut original = LockFile::default();
        original.packages.push(fixture_locked_package());

        original.save(&path).unwrap();

        let loaded = LockFile::load(&path).unwrap();
        assert_eq!(loaded.packages.len(), 1);
        assert_eq!(loaded.packages[0].name.as_str(), "acme-tool");
        assert_eq!(loaded.schema_version, original.schema_version);

        // Verify the nested [[package.versions]] array-of-tables round-trips
        // through the full save -> read_to_string -> from_str path (Task 6 only
        // covered the in-memory toml round-trip).
        assert_eq!(loaded.packages[0].installed_versions.len(), 1);
        assert_eq!(
            loaded.packages[0].installed_versions[0].resolved_commit,
            original.packages[0].installed_versions[0].resolved_commit,
        );
    }

    #[test]
    fn load_rejects_too_new_schema_version() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tau-lock.toml");

        let toml_str = r#"
            schema_version = 999
            generated_by_tau_version = "0.0.0"
            generated_at = "2026-04-27T10:00:00Z"
        "#;
        std::fs::write(&path, toml_str).unwrap();

        let err = LockFile::load(&path).unwrap_err();
        assert!(matches!(
            err,
            RegistryError::SchemaTooNew {
                found: 999,
                supported: 2,
            }
        ));
    }

    #[test]
    fn load_rejects_malformed_toml() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tau-lock.toml");
        std::fs::write(&path, "this is not toml = = =").unwrap();

        let err = LockFile::load(&path).unwrap_err();
        assert!(matches!(err, RegistryError::Parse { .. }));
    }

    // ---- save() ----

    #[test]
    fn save_creates_parent_directory() {
        let tmp = TempDir::new().unwrap();
        let path = tmp
            .path()
            .join("nested")
            .join("subdir")
            .join("tau-lock.toml");

        let lf = LockFile::default();
        lf.save(&path).unwrap();

        assert!(path.is_file(), "save should have created the file");
        assert!(
            path.parent().unwrap().is_dir(),
            "save should have created the parent directory"
        );
    }

    #[test]
    fn save_is_atomic_no_temp_files_remain() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tau-lock.toml");

        let lf = LockFile::default();
        lf.save(&path).unwrap();

        let entries: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        // Only the final lockfile should exist; no .tmp residue.
        assert_eq!(entries, vec!["tau-lock.toml".to_string()]);
    }

    #[test]
    fn save_overwrites_existing_file_atomically() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tau-lock.toml");

        // Write an initial lockfile.
        let lf1 = LockFile::default();
        lf1.save(&path).unwrap();

        // Write a different one.
        let mut lf2 = LockFile::default();
        lf2.packages.push(fixture_locked_package());
        lf2.save(&path).unwrap();

        // Reload and verify the second write took effect.
        let loaded = LockFile::load(&path).unwrap();
        assert_eq!(loaded.packages.len(), 1);
    }

    // ---- find() / upsert() / remove() ----

    #[test]
    fn find_returns_none_for_unknown_package() {
        let lf = LockFile::default();
        let name: tau_domain::PackageName = "nonexistent".parse().unwrap();
        assert!(lf.find(&name).is_none());
    }

    #[test]
    fn find_returns_some_for_known_package() {
        let mut lf = LockFile::default();
        lf.packages.push(fixture_locked_package());
        let name: tau_domain::PackageName = "acme-tool".parse().unwrap();
        let found = lf.find(&name);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name.as_str(), "acme-tool");
    }

    #[test]
    fn upsert_inserts_when_missing() {
        let mut lf = LockFile::default();
        lf.upsert(fixture_locked_package());
        assert_eq!(lf.packages.len(), 1);
        assert_eq!(lf.packages[0].name.as_str(), "acme-tool");
    }

    #[test]
    fn upsert_replaces_when_present() {
        let mut lf = LockFile::default();
        lf.upsert(fixture_locked_package());

        // Build a "newer" version of the same package.
        let mut updated = fixture_locked_package();
        updated.active_version = "2.0.0".parse().unwrap();

        lf.upsert(updated);

        assert_eq!(lf.packages.len(), 1, "upsert should not duplicate");
        assert_eq!(lf.packages[0].active_version.to_string(), "2.0.0");
    }

    #[test]
    fn upsert_preserves_insertion_order_for_other_packages() {
        let mut lf = LockFile::default();

        let mut pkg_a = fixture_locked_package();
        pkg_a.name = "aaa-pkg".parse().unwrap();
        let mut pkg_b = fixture_locked_package();
        pkg_b.name = "bbb-pkg".parse().unwrap();

        lf.upsert(pkg_a.clone());
        lf.upsert(pkg_b.clone());

        // Update aaa-pkg — should stay at index 0.
        let mut pkg_a_updated = pkg_a.clone();
        pkg_a_updated.active_version = "9.9.9".parse().unwrap();
        lf.upsert(pkg_a_updated);

        assert_eq!(lf.packages[0].name.as_str(), "aaa-pkg");
        assert_eq!(lf.packages[0].active_version.to_string(), "9.9.9");
        assert_eq!(lf.packages[1].name.as_str(), "bbb-pkg");
    }

    #[test]
    fn remove_returns_none_for_unknown_package() {
        let mut lf = LockFile::default();
        lf.packages.push(fixture_locked_package());

        let name: tau_domain::PackageName = "nonexistent".parse().unwrap();
        assert!(lf.remove(&name).is_none());
        assert_eq!(lf.packages.len(), 1, "should not remove anything");
    }

    #[test]
    fn remove_returns_some_for_known_package() {
        let mut lf = LockFile::default();
        lf.packages.push(fixture_locked_package());

        let name: tau_domain::PackageName = "acme-tool".parse().unwrap();
        let removed = lf.remove(&name);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name.as_str(), "acme-tool");
        assert!(lf.packages.is_empty());
    }

    #[test]
    fn remove_preserves_order_of_other_packages() {
        let mut lf = LockFile::default();

        let mut pkg_a = fixture_locked_package();
        pkg_a.name = "aaa".parse().unwrap();
        let mut pkg_b = fixture_locked_package();
        pkg_b.name = "bbb".parse().unwrap();
        let mut pkg_c = fixture_locked_package();
        pkg_c.name = "ccc".parse().unwrap();

        lf.packages.push(pkg_a);
        lf.packages.push(pkg_b);
        lf.packages.push(pkg_c);

        // Remove bbb — aaa should still be at 0, ccc at 1 (not 2).
        let name: tau_domain::PackageName = "bbb".parse().unwrap();
        lf.remove(&name);

        assert_eq!(lf.packages.len(), 2);
        assert_eq!(lf.packages[0].name.as_str(), "aaa");
        assert_eq!(lf.packages[1].name.as_str(), "ccc");
    }
}
