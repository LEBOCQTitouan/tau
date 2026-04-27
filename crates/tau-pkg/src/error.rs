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
