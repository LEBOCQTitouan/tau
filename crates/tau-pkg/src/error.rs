//! Per-operation typed errors for `tau-pkg`.
//!
//! All errors are `#[non_exhaustive]` so additive variants are non-breaking.
//! All errors derive `Debug + Error + Clone + PartialEq + Eq`. Tests with
//! free-form `String` fields use `matches!()` to avoid brittle wording
//! comparisons.
//!
//! The error taxonomy is per-operation, not per-error-source: callers
//! match on the operation they performed (`InstallError`, `UninstallError`)
//! and propagate composing errors via `?` and `#[from]`. There is no
//! umbrella `PkgError` — see ADR-0004.
//!
//! `ScopeError`, `GitError`, `ManifestReadError`, and `RegistryError` are
//! the leaf errors (this file). `InstallError` and `UninstallError`
//! compose them via `#[from]` (added in Task 3).

use std::process::ExitStatus;

use thiserror::Error;

/// Errors from scope detection and management.
///
/// Returned by `Scope::resolve`, `Scope::global`, and `Scope::new_project`
/// (added in Task 5) and by `ScopeConfig` deserialization (Task 4).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScopeError {
    /// `$HOME` not set and `$XDG_DATA_HOME` not set; can't locate global scope.
    #[error("HOME directory not found; set $HOME or $XDG_DATA_HOME")]
    HomeNotFound,
    /// Path expected to be a directory but is something else (regular file, symlink loop, etc.).
    #[error("scope path is not a directory: {path}")]
    NotADirectory {
        /// The offending path (lossy UTF-8).
        path: String,
    },
    /// `config.toml` schema_version is newer than this tau version supports.
    #[error("config.toml schema version {found} not supported (max supported: {supported})")]
    ConfigSchemaTooNew {
        /// The schema_version found in the config file.
        found: u32,
        /// The maximum schema_version this tau version recognizes.
        supported: u32,
    },
    /// `config.toml` failed TOML parsing.
    #[error("config.toml parse error: {reason}")]
    ConfigParse {
        /// Human-readable parser message.
        reason: String,
    },
    /// File-system I/O error while interacting with the scope.
    #[error("io: {message}")]
    Io {
        /// Human-readable I/O message.
        message: String,
    },
    /// Catch-all for scope-resolution failures not yet covered by typed variants.
    /// See: [escape-hatches.md#scopeerror-internal](../docs/explanation/escape-hatches.md#scopeerror-internal).
    #[error("internal: {message}")]
    Internal {
        /// Human-readable message describing the internal failure.
        message: String,
    },
}

/// Errors from the `git` subprocess wrapper (added in Task 8).
///
/// Note: `GitError` has no `Internal` variant on purpose — every git
/// failure has a typed cause (binary missing, clone failure with stderr,
/// command failure with stderr, or std::io). Adding `Internal` would
/// invite catch-all use; new git failure modes get new typed variants
/// instead.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GitError {
    /// The `git` binary was not found on PATH.
    #[error("git not found on PATH; install git or add it to PATH")]
    GitMissing,
    /// `git clone` exited non-zero.
    #[error("git clone failed: exit {exit_code}: {stderr}")]
    CloneFailed {
        /// Exit code from `git clone`.
        exit_code: i32,
        /// Captured stderr (lossy UTF-8).
        stderr: String,
    },
    /// A non-clone git command failed.
    #[error("git command failed: {what}: {stderr}")]
    CommandFailed {
        /// Short identifier of the command (e.g. `"git rev-parse HEAD"`).
        what: String,
        /// Captured stderr (lossy UTF-8).
        stderr: String,
    },
    /// File-system I/O error while interacting with a clone or staging dir.
    #[error("io: {message}")]
    Io {
        /// Human-readable I/O message.
        message: String,
    },
}

/// Errors from `read_manifest` (added in Task 9).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ManifestReadError {
    /// The manifest file was not found at the expected path.
    #[error("manifest not found at {path}")]
    NotFound {
        /// Lossy-UTF-8 path that was tried.
        path: String,
    },
    /// File-system I/O error reading the manifest.
    #[error("manifest io: {message}")]
    Io {
        /// Human-readable I/O message.
        message: String,
    },
    /// TOML parser rejected the manifest text.
    #[error("manifest TOML parse: {reason}")]
    Parse {
        /// Human-readable parser message.
        reason: String,
    },
    /// Structural / cross-field validation failure from `tau_domain`.
    #[error("manifest validation: {0}")]
    Validation(#[from] tau_domain::PackageManifestError),
}

/// Errors from lockfile load/save and registry read accessors (added in Tasks 6, 7, 12).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    /// File-system I/O error.
    #[error("io: {message}")]
    Io {
        /// Human-readable I/O message.
        message: String,
    },
    /// TOML parser rejected the lockfile text.
    #[error("lockfile TOML parse: {reason}")]
    Parse {
        /// Human-readable parser message.
        reason: String,
    },
    /// Lockfile schema_version is newer than this tau version supports.
    #[error("lockfile schema version {found} not supported (max supported: {supported})")]
    SchemaTooNew {
        /// The schema_version found in the lockfile.
        found: u32,
        /// The maximum schema_version this tau version recognizes.
        supported: u32,
    },
    /// Phase-1 use; `sha256` slot is empty at v0.1, so this never fires today.
    #[error("lockfile checksum mismatch for {name}@{version}")]
    ChecksumMismatch {
        /// Package name.
        name: String,
        /// Package version (string for display purposes).
        version: String,
    },
    /// Catch-all for lockfile / registry-read failures not yet covered by typed variants.
    /// See: [escape-hatches.md#registryerror-internal](../docs/explanation/escape-hatches.md#registryerror-internal).
    #[error("internal: {message}")]
    Internal {
        /// Human-readable message describing the internal failure.
        message: String,
    },
}

/// Errors from `install` and `install_with_options` (added in Task 10).
///
/// Composes `GitError`, `ManifestReadError`, `RegistryError`, and
/// `ScopeError` via `#[from]` so the install lifecycle can use `?`
/// propagation throughout.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InstallError {
    /// The `git` subprocess failed.
    #[error("git: {0}")]
    Git(#[from] GitError),
    /// Manifest parsing or structural validation failed.
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestReadError),
    /// Lockfile read / write failed.
    #[error("registry: {0}")]
    Registry(#[from] RegistryError),
    /// Scope resolution / management failed.
    #[error("scope: {0}")]
    Scope(#[from] ScopeError),
    /// User-supplied `source` doesn't match the manifest's declared `source`.
    #[error("source / manifest mismatch: expected {expected}, found {found}")]
    SourceManifestMismatch {
        /// The source the user passed to `install`, formatted via `Display`.
        expected: String,
        /// The source declared in the cloned package's `tau.toml`, formatted via `Display`.
        found: String,
    },
    /// Another `install` operation already holds the per-scope advisory lock.
    #[error("install operation already in progress for scope {scope}")]
    Locked {
        /// Lossy-UTF-8 path of the scope state directory.
        scope: String,
    },
    /// `cargo build` exited non-zero while building a `kind = "rust-cargo"`
    /// plugin. Carries the cargo exit status and the last ~4 KiB of cargo's
    /// stderr so users can diagnose the compile failure without re-running.
    /// The cloned source is left on disk; users retry via `tau install --force`
    /// or inspect the staging area.
    #[error("plugin build failed: cargo exited {exit_status}")]
    BuildFailed {
        /// Exit status returned by the `cargo build` subprocess.
        exit_status: ExitStatus,
        /// Last ~4 KiB of cargo's stderr (lossy UTF-8) for diagnostic display.
        stderr_tail: String,
    },
    /// The `cargo` binary was not found at the configured location.
    /// Either `BuildOptions::cargo_path` was set to a non-existent path,
    /// or the default discovery (`cargo` on PATH) found nothing.
    #[error("`cargo` not found on PATH; set BuildOptions::cargo_path or install Rust")]
    CargoNotFound,
    /// Catch-all for install lifecycle failures not yet covered by typed variants.
    /// Use this only when the failure cannot be reported as `Git`, `Manifest`,
    /// `Registry`, `Scope`, `SourceManifestMismatch`, or `Locked`.
    /// See: [escape-hatches.md#installerror-internal](../docs/explanation/escape-hatches.md#installerror-internal).
    #[error("internal: {message}")]
    Internal {
        /// Human-readable message describing the internal failure.
        message: String,
    },
}

/// Errors from `uninstall` (added in Task 11).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UninstallError {
    /// Lockfile read / write failed.
    #[error("registry: {0}")]
    Registry(#[from] RegistryError),
    /// Scope resolution / management failed.
    #[error("scope: {0}")]
    Scope(#[from] ScopeError),
    /// The package isn't recorded in the scope's lockfile at all.
    #[error("package not installed: {name}")]
    NotInstalled {
        /// Package name.
        name: String,
    },
    /// The package is installed but not at the requested version.
    #[error("version not installed: {name}@{version}")]
    VersionNotInstalled {
        /// Package name.
        name: String,
        /// Requested version (string for display purposes).
        version: String,
    },
    /// File-system I/O error while removing the package directory.
    #[error("io: {message}")]
    Io {
        /// Human-readable I/O message.
        message: String,
    },
    /// Another `uninstall` operation already holds the per-scope advisory lock.
    #[error("uninstall operation already in progress for scope {scope}")]
    Locked {
        /// Lossy-UTF-8 path of the scope state directory.
        scope: String,
    },
    /// Catch-all for uninstall failures not yet covered by typed variants.
    /// See: [escape-hatches.md#uninstallerror-internal](../docs/explanation/escape-hatches.md#uninstallerror-internal).
    #[error("internal: {message}")]
    Internal {
        /// Human-readable message describing the internal failure.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_error_composes_git_error_via_from() {
        let git_err = GitError::GitMissing;
        let install_err: InstallError = git_err.into();
        assert!(matches!(
            install_err,
            InstallError::Git(GitError::GitMissing)
        ));
    }

    #[test]
    fn install_error_composes_manifest_error_via_from() {
        let manifest_err = ManifestReadError::NotFound {
            path: "/tmp/missing".into(),
        };
        let install_err: InstallError = manifest_err.into();
        assert!(matches!(
            install_err,
            InstallError::Manifest(ManifestReadError::NotFound { .. })
        ));
    }

    #[test]
    fn install_error_composes_registry_error_via_from() {
        let reg_err = RegistryError::Parse {
            reason: "bad toml".into(),
        };
        let install_err: InstallError = reg_err.into();
        assert!(matches!(
            install_err,
            InstallError::Registry(RegistryError::Parse { .. })
        ));
    }

    #[test]
    fn install_error_composes_scope_error_via_from() {
        let scope_err = ScopeError::HomeNotFound;
        let install_err: InstallError = scope_err.into();
        assert!(matches!(
            install_err,
            InstallError::Scope(ScopeError::HomeNotFound)
        ));
    }

    #[test]
    fn uninstall_error_composes_registry_via_from() {
        let reg_err = RegistryError::Io {
            message: "x".into(),
        };
        let un_err: UninstallError = reg_err.into();
        assert!(matches!(
            un_err,
            UninstallError::Registry(RegistryError::Io { .. })
        ));
    }

    #[test]
    fn uninstall_error_composes_scope_via_from() {
        let scope_err = ScopeError::NotADirectory { path: "/x".into() };
        let un_err: UninstallError = scope_err.into();
        assert!(matches!(
            un_err,
            UninstallError::Scope(ScopeError::NotADirectory { .. })
        ));
    }

    #[test]
    fn display_renders_human_readable() {
        let err = InstallError::Locked {
            scope: "/tmp/.tau".into(),
        };
        let s = format!("{err}");
        assert!(
            s.contains("install operation already in progress"),
            "got: {s}"
        );
        assert!(s.contains("/tmp/.tau"), "got: {s}");
    }
}
