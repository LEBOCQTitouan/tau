//! Install lifecycle for tau-pkg.
//!
//! [`install`] performs the 10-step pipeline:
//!
//! 1. Pre-flight: `git --version` check + acquire scope-level advisory file lock.
//! 2. Clone source into `<scope>/.tau/packages/.staging/<random>/`.
//! 3. Parse `tau.toml` from the staging directory.
//! 4. Validate the manifest structurally (handled by `read_manifest`).
//! 5. Verify the user-supplied source matches the manifest's declared source.
//! 6. Capability validation — warn (don't error) on unknown kinds and
//!    non-namespaced `Capability::Custom` names.
//! 7. Resolve the cloned repo's HEAD to a 40-char SHA.
//! 8. Materialize: atomically `fs::rename` staging → `<scope>/.tau/packages/<name>/<version>/`.
//! 9. Update the lockfile (atomic write).
//! 10. Release the advisory file lock.
//!
//! Failure at any step before (8) leaves no on-disk state (the staging
//! TempDir is auto-removed). At step (8), the staging directory's
//! auto-cleanup is disabled before the rename; on rename failure the
//! orphaned staging dir is best-effort removed. Failure at (8) post-
//! rename or at (9) is unlikely (both are atomic operations).
//!
//! Note: idempotency cannot be checked before the clone because the
//! package name (lockfile key) is not known until the manifest is
//! parsed. The clone is therefore always performed; the idempotency
//! short-circuit happens at step (8) before the rename, avoiding only
//! the disk-write phase.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use fs4::FileExt;
use tau_domain::{kinds, Capability, PackageName, PackageSource, Version};

use crate::error::InstallError;
use crate::git::Git;
use crate::lockfile::{LockFile, LockedPackage, LockedVersion};
use crate::manifest::read_manifest;
use crate::scope::Scope;

/// Options for [`install_with_options`].
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// If `true` (default), wait indefinitely for a concurrent install
    /// to release the advisory file lock. If `false`, error
    /// immediately with [`InstallError::Locked`].
    pub block_on_lock: bool,
    /// If `true`, force re-clone even if the target version directory
    /// already exists. Default: `false` (idempotent — skip clone if
    /// the dir exists and the lockfile already records this version).
    pub force: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            block_on_lock: true,
            force: false,
        }
    }
}

/// Outcome of a successful [`install`] call.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledPackage {
    /// The package name as declared in `tau.toml`.
    pub name: PackageName,
    /// The package version as declared in `tau.toml`.
    pub version: Version,
    /// The source the package was installed from.
    pub source: PackageSource,
    /// The on-disk path of the installed package directory.
    pub installed_path: PathBuf,
    /// When this version was installed.
    pub installed_at: SystemTime,
}

/// Install a package from `source` into `scope`. Equivalent to
/// `install_with_options(source, scope, InstallOptions::default())`.
///
/// # Example
///
/// ```ignore
/// // `Scope` is `#[non_exhaustive]`; use `Scope::resolve` / `global` /
/// // `new_project` to obtain one.
/// use std::str::FromStr;
/// use tau_domain::PackageSource;
/// use tau_pkg::{install, Scope};
///
/// let scope = Scope::global().unwrap();
/// let source = PackageSource::from_str("https://example.com/pkg.git").unwrap();
/// let installed = install(&source, &scope).unwrap();
/// println!("installed at {}", installed.installed_path.display());
/// ```
pub fn install(source: &PackageSource, scope: &Scope) -> Result<InstalledPackage, InstallError> {
    install_with_options(source, scope, InstallOptions::default())
}

/// Install a package from `source` into `scope` with explicit `options`.
/// See the module-level documentation for the 10-step lifecycle.
pub fn install_with_options(
    source: &PackageSource,
    scope: &Scope,
    options: InstallOptions,
) -> Result<InstalledPackage, InstallError> {
    // Step 1: pre-flight.
    let _git_version = Git::version_check()?;

    let lock_path = scope.install_lock_path();
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|e| InstallError::Internal {
            message: format!("creating lock directory {}: {e}", parent.display()),
        })?;
    }

    let lock_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| InstallError::Internal {
            message: format!("opening install lock {}: {e}", lock_path.display()),
        })?;

    if options.block_on_lock {
        lock_file
            .lock_exclusive()
            .map_err(|e| InstallError::Internal {
                message: format!("acquiring install lock: {e}"),
            })?;
    } else {
        match lock_file.try_lock_exclusive() {
            Ok(()) => {}
            Err(e) if e.kind() == fs4::lock_contended_error().kind() => {
                return Err(InstallError::Locked {
                    scope: scope.state_path().display().to_string(),
                });
            }
            Err(e) => {
                return Err(InstallError::Internal {
                    message: format!("trying install lock: {e}"),
                });
            }
        }
    }

    // From here on, ensure the lock is released even on early returns.
    // `lock_file` stays in scope until the end; we explicitly unlock at step 10.

    let result = (|| -> Result<InstalledPackage, InstallError> {
        // Step 2: clone into staging.
        let staging_root = scope.packages_dir().join(".staging");
        fs::create_dir_all(&staging_root).map_err(|e| InstallError::Internal {
            message: format!("creating staging dir {}: {e}", staging_root.display()),
        })?;

        let staging_dir = tempfile::Builder::new()
            .prefix("staging-")
            .tempdir_in(&staging_root)
            .map_err(|e| InstallError::Internal {
                message: format!("creating staging tempdir: {e}"),
            })?;

        Git::clone(source, staging_dir.path())?;

        // Step 3 + 4: parse + validate manifest (validate is part of read_manifest).
        let manifest = read_manifest(&staging_dir.path().join("tau.toml"))?;

        // Step 5: source / manifest match.
        if *manifest.source() != *source {
            return Err(InstallError::SourceManifestMismatch {
                expected: source.to_string(),
                found: manifest.source().to_string(),
            });
        }

        // Step 6: capability validation (warnings only — NG12).
        warn_unknown_kind(&manifest);
        warn_non_namespaced_custom_capabilities(&manifest);

        // Step 7: resolve commit.
        let resolved_commit = Git::resolve_head(staging_dir.path())?;

        // Step 8: materialize.
        let target = scope.package_dir(manifest.name(), manifest.version());
        let lockfile_path = scope.lockfile_path();
        let mut lf = LockFile::load(&lockfile_path)?;

        let already_recorded = lf
            .find(manifest.name())
            .map(|p| {
                p.installed_versions
                    .iter()
                    .any(|v| v.version == *manifest.version())
            })
            .unwrap_or(false);

        if target.exists() {
            if !options.force && already_recorded {
                // Idempotent: already installed at this version; return the
                // existing entry's metadata.
                let installed_at = lf
                    .find(manifest.name())
                    .and_then(|p| {
                        p.installed_versions
                            .iter()
                            .find(|v| v.version == *manifest.version())
                            .map(|v| v.installed_at)
                    })
                    .ok_or_else(|| InstallError::Internal {
                        message: format!(
                            "lockfile recorded {}@{} but installed_at lookup returned None — \
                             inconsistent state",
                            manifest.name(),
                            manifest.version(),
                        ),
                    })?;
                return Ok(InstalledPackage {
                    name: manifest.name().clone(),
                    version: manifest.version().clone(),
                    source: source.clone(),
                    installed_path: target,
                    installed_at,
                });
            }
            // Otherwise: force OR orphaned directory — overwrite.
            fs::remove_dir_all(&target).map_err(|e| InstallError::Internal {
                message: format!("removing existing target {}: {e}", target.display()),
            })?;
        }

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| InstallError::Internal {
                message: format!("creating target parent {}: {e}", parent.display()),
            })?;
        }

        // Take ownership of the staging path before rename (disables auto-cleanup).
        let staged_path = staging_dir.keep();
        if let Err(e) = fs::rename(&staged_path, &target) {
            // Best-effort cleanup — the rename failed, so the staging dir is now
            // orphaned (tempfile no longer manages it). Ignore secondary failure.
            let _ = fs::remove_dir_all(&staged_path);
            return Err(InstallError::Internal {
                message: format!(
                    "renaming {} -> {}: {e}",
                    staged_path.display(),
                    target.display(),
                ),
            });
        }

        // Step 9: update lockfile.
        let now = SystemTime::now();
        let new_locked_version = LockedVersion {
            version: manifest.version().clone(),
            rev: rev_from_source(source),
            resolved_commit,
            sha256: String::new(),
            installed_at: now,
        };

        let updated_package = match lf.find(manifest.name()).cloned() {
            Some(mut existing) => {
                // Replace the matching version (if present) or append.
                let mut found = false;
                for v in existing.installed_versions.iter_mut() {
                    if v.version == *manifest.version() {
                        *v = new_locked_version.clone();
                        found = true;
                        break;
                    }
                }
                if !found {
                    existing.installed_versions.push(new_locked_version.clone());
                }
                existing.active_version = manifest.version().clone();
                existing.source = source.clone();
                // Sort installed_versions for deterministic lockfile diffs.
                existing
                    .installed_versions
                    .sort_by(|a, b| a.version.cmp(&b.version));
                existing
            }
            None => LockedPackage {
                name: manifest.name().clone(),
                active_version: manifest.version().clone(),
                source: source.clone(),
                installed_versions: vec![new_locked_version.clone()],
            },
        };

        lf.upsert(updated_package);
        lf.packages
            .sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        lf.generated_at = now;
        lf.save(&lockfile_path)?;

        Ok(InstalledPackage {
            name: manifest.name().clone(),
            version: manifest.version().clone(),
            source: source.clone(),
            installed_path: target,
            installed_at: now,
        })
    })();

    // Step 10: release the lock (explicit via FileExt::unlock; also released on drop).
    let _ = lock_file.unlock();

    result
}

/// Extract `rev` from a `PackageSource::Git`. Other variants (none at v0.1)
/// return `None`.
fn rev_from_source(source: &PackageSource) -> Option<String> {
    match source {
        PackageSource::Git { rev, .. } => rev.clone(),
        _ => None,
    }
}

/// Warn (eprintln) if the manifest's kind isn't a known canonical from
/// `tau_domain::kinds`. v0.1 is permissive — unknown kinds are valid;
/// the runtime decides what to do with them. NG12: tau is a runtime,
/// not a framework.
///
/// At v0.1, `PackageKind` has a single variant `Custom { kind: String }`.
/// Canonical kind strings live in `tau_domain::kinds`.
fn warn_unknown_kind(manifest: &tau_domain::PackageManifest) {
    use tau_domain::PackageKind;

    let known_kinds: &[&str] = &[
        kinds::LLM_BACKEND,
        kinds::TOOL,
        kinds::SKILL,
        kinds::PIPELINE,
        kinds::MCP_SERVER,
        kinds::STORAGE,
        kinds::SANDBOX,
    ];

    let kind_str = match manifest.kind() {
        PackageKind::Custom { kind } => kind.as_str(),
        _ => return, // forward-compat: new typed variants are always "known"
    };

    if !known_kinds.contains(&kind_str) {
        eprintln!(
            "warning: package {} has unknown kind {:?}; tau-runtime will treat it as opaque",
            manifest.name(),
            kind_str,
        );
    }
}

/// Warn (eprintln) on `Capability::Custom { name }` without a dot-namespaced name.
/// Encourages `mcp.tool.use` style; permits non-namespaced for forward-compat.
fn warn_non_namespaced_custom_capabilities(manifest: &tau_domain::PackageManifest) {
    for cap in manifest.capabilities() {
        if let Capability::Custom { name, .. } = cap {
            if !name.contains('.') {
                eprintln!(
                    "warning: package {} declares Capability::Custom {{ name = {:?} }} \
                     without a dot-namespaced name; consider e.g. \"vendor.feature.action\"",
                    manifest.name(),
                    name,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_options_default() {
        let opts = InstallOptions::default();
        assert!(opts.block_on_lock);
        assert!(!opts.force);
    }

    #[test]
    fn rev_from_source_extracts_some_rev() {
        let s = "https://x.com/y.git#main".parse::<PackageSource>().unwrap();
        assert_eq!(rev_from_source(&s), Some("main".to_string()));
    }

    #[test]
    fn rev_from_source_returns_none_when_absent() {
        let s = "https://x.com/y.git".parse::<PackageSource>().unwrap();
        assert_eq!(rev_from_source(&s), None);
    }

    // The bulk of install_with_options testing happens in Task 14's
    // integration test suite (tests/install_lifecycle.rs), which uses
    // file://-based git fixtures via `git init --bare`.
}
