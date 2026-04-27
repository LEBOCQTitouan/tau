# Tau Pkg (sub-project 3) — Design Spec

**Date:** 2026-04-27
**Sub-project:** `tau-pkg` package manager (sub-project 3; third of the Phase-0 sub-projects, after `tau-domain` and `tau-ports`)
**Author:** Titouan Lebocq
**Status:** Approved for implementation planning

---

## 1. Scope & success criteria

### Scope

Implement the package manager that performs `tau install` from git URLs, validates manifests, and tracks installed packages per scope. tau-pkg is the **first sub-project that does real I/O** (git clone, file system, TOML parsing from disk). It depends on tau-domain (manifest types, source URL grammar, capability types) and consumes tau-ports' types only insofar as it stores plugin-source trees that downstream tau-runtime will load.

### Done when

- `crates/tau-pkg/` exposes the public API enumerated in §3.7.
- `cargo build -p tau-pkg --no-default-features` succeeds locally and in CI.
- `cargo build -p tau-pkg --all-features` succeeds locally and in CI.
- `cargo clippy -p tau-pkg --all-targets --all-features -- -D warnings` succeeds.
- `cargo fmt --all -- --check` succeeds.
- `cargo test -p tau-pkg --all-targets --all-features` succeeds (unit, proptest, integration).
- `cargo test -p tau-pkg --doc --all-features` succeeds — every public item has an example.
- ADR-0004 (tau-pkg public API + storage layout + lockfile schema) is filed in `docs/decisions/` and accepted.
- The git log on `main` contains a clean per-sub-task series of Conventional Commits.
- CI green on Linux + macOS (Windows non-blocking per G15) for both `--no-default-features` and `--all-features` build modes.
- New escape-hatch entries (if any) registered in `docs/explanation/escape-hatches.md`.

### Out of scope (explicit, deferred to later sub-projects or ADRs)

| Item | Owner / Trigger |
|---|---|
| Async public API | If real concurrent-install use case appears, wrap sync API in `spawn_blocking` at call site or add async wrappers in tau-pkg via additive minor. |
| Pure-Rust git client (`gitoxide` / `git2-rs`) | Phase 1+ if shipping pre-built tau binaries to non-developer users; v0.1 shells out to `git`. |
| Transitive dependency resolution | Phase 1+ when ecosystem grows enough that auto-resolve is desired. v0.1 reads `dependencies` informationally. |
| Lockfile sha256 content hashing | v0.1 leaves the schema slot empty; populated when `tau verify` (Phase 1+) lands. |
| Cargo-style version conflict resolver | Phase 1+. |
| `tau update` (refetch latest matching constraint) | v0.1: user does `tau uninstall foo && tau install <git-url>`. |
| `tau verify` (re-check on-disk state vs lockfile) | Phase 1+ when sandboxing/signing matters. |
| `tau install --frozen` (install from existing lockfile only) | Phase 1+ alongside transitive resolution. |
| `tau init` command | Phase 1 (sub-project 5: tau-cli). The Scope API exposes `Scope::new_project(path)` for tau-cli to call; the actual `tau init` CLI verb lives in tau-cli. |
| `tau switch <name>@<version>` (change active version without reinstall) | Phase 1+. The disk layout supports it; the CLI verb is deferred. |
| Package signing / attestation | Phase 1+ supply-chain (G12). |
| Centralized package registry | NG4 forever. Git URLs only. |
| Credential management | NG9 forever. Inherits user's git credential setup. |
| Package marketplace / curation / ranking | NG4 forever. |

---

## 2. Module layout & dependencies

```
crates/tau-pkg/
├── Cargo.toml
└── src/
    ├── lib.rs           # crate-level docs, lints, re-exports
    ├── error.rs         # ScopeError, InstallError, UninstallError, RegistryError, GitError
    ├── scope.rs         # Scope + ScopeKind + resolve() + ScopeConfig (config.toml schema)
    ├── lockfile.rs      # LockFile + LockedPackage + LockedVersion + lockfile read/write
    ├── git.rs           # Git binary wrapper: clone, resolve_rev, version_check
    ├── install.rs       # install() + uninstall() + lifecycle (clone → manifest → lockfile)
    ├── registry.rs      # list() + get() — read accessors for the lockfile
    └── manifest.rs      # read_manifest(path) + structural validation hooks
```

### Cargo.toml additions

`[workspace.dependencies]` gains:
- `fs4 = "0.8"` (advisory file locks, cross-platform).
- `toml = "0.8"` is already a dev-dep in tau-domain; promote to a workspace-level runtime dep so tau-pkg can use it.

`crates/tau-pkg/Cargo.toml`:

```toml
[dependencies]
tau-domain = { workspace = true, features = ["serde"] }
thiserror  = { workspace = true }
toml       = { workspace = true }
serde      = { workspace = true, features = ["derive"] }
fs4        = { workspace = true }
semver     = { workspace = true }

[features]
default = []

[dev-dependencies]
proptest    = { workspace = true }
tempfile    = "3"
serde_json  = "1"
```

**Key calls:**

- **No async runtime dep** — tau-pkg is sync per Q1.
- **`toml` is a runtime dep** (was dev-dep in tau-domain) since tau-pkg parses manifests + lockfiles from disk.
- **`serde` is a runtime dep with `derive`** — tau-pkg's lockfile and config schemas use `#[derive(Serialize, Deserialize)]`.
- **No git crate** (gitoxide / git2-rs) — shells out to `git` binary per Q2.
- **`fs4` for advisory file locks** — cross-platform `flock`/`LockFileEx` wrapper.
- **`tempfile` is dev-only** — tests need clean scratch dirs.
- **`tau-domain`'s `serde` feature is enabled** — tau-pkg deserializes `UncheckedManifest` from disk via tau-domain's serde derives.

---

## 3. Type-by-type design

### 3.1 Scope (`scope.rs`)

```rust
use std::path::{Path, PathBuf};

use crate::error::ScopeError;

/// Active scope for a tau operation. Encodes G8: global vs project
/// scope, where project scope is detected by walking up from the cwd
/// looking for a `.tau/` directory.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// Scope root.
    /// - Global: `~/.tau` (or `$XDG_DATA_HOME/tau` if set).
    /// - Project: the project root directory (the parent of the `.tau/` dir).
    path: PathBuf,
    /// Local state directory.
    /// - Global: same as `path` (`~/.tau`).
    /// - Project: `<path>/.tau` (gitignored local state).
    state_path: PathBuf,
    kind: ScopeKind,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    Global,
    Project,
}

impl Scope {
    /// Detect the active scope from the given cwd. Walks up the directory
    /// tree looking for a `.tau/` directory; falls back to global scope
    /// (`~/.tau/`) if none found.
    pub fn resolve(cwd: &Path) -> Result<Self, ScopeError>;

    /// Construct a Scope rooted at the given project path. Used by
    /// `tau init` (lives in tau-cli) to materialize a new project scope.
    /// Creates `<path>/.tau/` and a default `config.toml`.
    pub fn new_project(project_root: &Path) -> Result<Self, ScopeError>;

    /// Construct the global Scope. Reads `$TAU_HOME` env var if set,
    /// otherwise `$XDG_DATA_HOME/tau` if set, otherwise `~/.tau/`.
    /// Creates the directory if absent.
    pub fn global() -> Result<Self, ScopeError>;

    pub fn path(&self) -> &Path;
    pub fn state_path(&self) -> &Path;
    pub fn kind(&self) -> ScopeKind;

    pub fn lockfile_path(&self) -> PathBuf;          // <path>/tau-lock.toml
    pub fn config_path(&self) -> PathBuf;            // <state_path>/config.toml
    pub fn packages_dir(&self) -> PathBuf;           // <state_path>/packages/
    pub fn install_lock_path(&self) -> PathBuf;      // <state_path>/locks/install.lock

    /// Path where a specific package's source tree should live.
    pub fn package_dir(&self, name: &PackageName, version: &Version) -> PathBuf;
        // <state_path>/packages/<name>/<version>/
}

/// Schema for `<scope>/config.toml`. Future-grown additively.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeConfig {
    pub schema_version: u32,             // 1 at v0.1
    pub kind: ScopeKind,                 // matches Scope::kind
    #[serde(with = "humantime_serde")]
    pub created_at: SystemTime,
    pub created_by_tau_version: String,
    /// Reserved for future scope-level defaults (default LLM backend,
    /// timeouts, etc.). Empty at v0.1.
    #[serde(default)]
    pub defaults: BTreeMap<String, tau_domain::Value>,
}
```

### 3.2 Lockfile (`lockfile.rs`)

```rust
use std::path::Path;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tau_domain::{PackageName, PackageSource, Version};

use crate::error::RegistryError;

/// Schema for `tau-lock.toml`. Project scope: lives at `<project>/tau-lock.toml`
/// (committed). Global scope: lives at `~/.tau/tau-lock.toml` (local).
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockFile {
    pub schema_version: u32,                   // 1 at v0.1
    pub generated_by_tau_version: String,
    #[serde(with = "humantime_serde")]
    pub generated_at: SystemTime,
    #[serde(default, rename = "package")]
    pub packages: Vec<LockedPackage>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockedPackage {
    pub name: PackageName,
    pub active_version: Version,
    pub source: PackageSource,
    #[serde(default, rename = "versions")]
    pub installed_versions: Vec<LockedVersion>,
}

#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockedVersion {
    pub version: Version,
    /// Branch name, tag, or 40-char SHA — opaque, as user supplied.
    pub rev: Option<String>,
    /// Full commit SHA after git resolves `rev`. Captured at install time.
    pub resolved_commit: String,
    /// Hash of the cloned working tree contents. Empty at v0.1; populated
    /// when `tau verify` ships (Phase 1+).
    #[serde(default)]
    pub sha256: String,
    #[serde(with = "humantime_serde")]
    pub installed_at: SystemTime,
}

impl LockFile {
    /// Read the lockfile from `<scope.lockfile_path()>`. Returns an
    /// empty LockFile if the file doesn't exist (lazy creation).
    pub fn load(path: &Path) -> Result<Self, RegistryError>;

    /// Atomically write the lockfile to `<scope.lockfile_path()>`.
    /// Implementation: write-to-temp-then-rename.
    pub fn save(&self, path: &Path) -> Result<(), RegistryError>;

    /// Find a package by name. Returns `None` if not in the lockfile.
    pub fn find(&self, name: &PackageName) -> Option<&LockedPackage>;

    /// Insert or update a package entry. Used by install().
    pub fn upsert(&mut self, package: LockedPackage);

    /// Remove a package entry by name. Used by uninstall().
    /// Returns the removed entry if present.
    pub fn remove(&mut self, name: &PackageName) -> Option<LockedPackage>;
}

impl Default for LockFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            generated_by_tau_version: env!("CARGO_PKG_VERSION").into(),
            generated_at: SystemTime::now(),
            packages: Vec::new(),
        }
    }
}
```

### 3.3 Git wrapper (`git.rs`)

```rust
use std::path::Path;

use crate::error::GitError;

/// Shell out to the system `git` binary.
///
/// The wrapper is intentionally thin — it captures stdout/stderr,
/// inspects exit codes, and produces typed errors. Authentication
/// (SSH keys, credential helpers) is inherited from the user's git
/// configuration (NG9 — tau doesn't manage credentials).
pub(crate) struct Git;

impl Git {
    /// Verify `git` is on PATH and return its version. Called once at
    /// install start; produces `GitError::GitMissing` with a clear
    /// remediation message if `git --version` fails.
    pub(crate) fn version_check() -> Result<String, GitError>;

    /// Clone `source.url()` into `dest`. If `source.rev` is `Some`,
    /// pass `--branch <rev>` and `--single-branch`; tag, branch, or SHA
    /// disambiguation happens at clone time per git's normal logic.
    pub(crate) fn clone(source: &PackageSource, dest: &Path) -> Result<(), GitError>;

    /// Resolve the cloned repo's HEAD to a 40-char commit SHA. Used to
    /// populate `LockedVersion.resolved_commit` regardless of whether
    /// `rev` was a branch / tag / SHA originally.
    pub(crate) fn resolve_head(repo: &Path) -> Result<String, GitError>;
}
```

### 3.4 Install lifecycle (`install.rs`)

```rust
use std::path::Path;
use std::time::SystemTime;

use tau_domain::{PackageManifest, PackageName, PackageSource, UncheckedManifest, Version};

use crate::error::{InstallError, UninstallError};
use crate::git::Git;
use crate::lockfile::{LockFile, LockedPackage, LockedVersion};
use crate::scope::Scope;

/// Install a package from `source` into `scope`. Shells out to git for
/// the clone, parses and validates the manifest, materializes the
/// package source tree at `<scope.packages_dir()>/<name>/<version>/`,
/// and updates `tau-lock.toml`.
///
/// The active version after install is set to the manifest's declared
/// version (replacing any previously active version of the same package).
/// Other previously installed versions on disk are preserved.
///
/// Acquires an advisory file lock at `<scope.install_lock_path()>` for
/// the duration of the operation; concurrent installs in the same scope
/// either wait or error (depending on caller's lock-mode preference, see
/// `InstallOptions`).
pub fn install(
    source: &PackageSource,
    scope: &Scope,
) -> Result<InstalledPackage, InstallError>;

pub fn install_with_options(
    source: &PackageSource,
    scope: &Scope,
    options: InstallOptions,
) -> Result<InstalledPackage, InstallError>;

#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// If `true`, wait indefinitely for a concurrent install to release
    /// the file lock. If `false`, error immediately with `InstallError::Locked`.
    /// Default: `true`.
    pub block_on_lock: bool,
    /// If `true`, force re-clone even if the target version directory
    /// already exists. Default: `false` (idempotent — skip clone if dir
    /// exists and lockfile already records this version).
    pub force: bool,
}

/// Uninstall a package. If `version` is `None`, removes ALL installed
/// versions and the lockfile entry. If `Some`, removes just that version
/// directory and updates the lockfile; if removing the active_version,
/// promotes the highest remaining version (semver-sorted), or removes
/// the package entirely if no versions remain.
pub fn uninstall(
    name: &PackageName,
    version: Option<&Version>,
    scope: &Scope,
) -> Result<(), UninstallError>;

#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub name: PackageName,
    pub version: Version,                      // the version just installed (becomes active)
    pub source: PackageSource,
    pub installed_path: PathBuf,
    pub installed_at: SystemTime,
}
```

#### Install lifecycle (step-by-step)

1. **Pre-flight**: `Git::version_check()` once. Lock the install dir via `fs4`.
2. **Clone**: `Git::clone(source, temp_dir)` to a temp staging directory under `<scope.state_path()>/packages/.staging/<random>/`. (Avoids torn state if clone fails partway.)
3. **Parse manifest**: read `temp_dir/tau.toml`. `toml::from_str::<UncheckedManifest>()`. Surface parse errors as `InstallError::Manifest(ManifestParseError)`.
4. **Validate manifest**: `unchecked.validate()` — surfaces structural errors as `InstallError::Manifest(PackageManifestError)` via `#[from]`.
5. **Verify source matches manifest**: the cloned repo's manifest's `source` should match the source the user passed (e.g., user passed `https://github.com/foo/bar.git`, manifest claims `name = "bar"` and `source` matches). If mismatch, surface `InstallError::SourceManifestMismatch { expected, found }`.
6. **Capability validation (G14)**: structural validation already done by `UncheckedManifest::validate`. Plus warn (don't error) if `kind` isn't a known canonical from `tau_domain::kinds`. Plus warn if `Capability::Custom { name }` doesn't follow dot-namespaced convention.
7. **Resolve commit**: `Git::resolve_head(temp_dir)` → SHA-40.
8. **Materialize**: `fs::rename(temp_dir, scope.package_dir(name, version))`. Atomic on same filesystem.
9. **Update lockfile**: read existing `tau-lock.toml` (or default empty), `upsert` the package, set `active_version`, append/upsert the `LockedVersion`, write atomically (temp+rename).
10. **Release lock**, return `InstalledPackage`.

Failure at any step before (8) leaves no state. Failure at (8) or (9) is unlikely (mv + atomic write) but if it happens, the user can `tau uninstall <name>@<version>` to clean up.

### 3.5 Registry (`registry.rs`)

```rust
use tau_domain::PackageName;

use crate::error::RegistryError;
use crate::lockfile::{LockedPackage, LockFile};
use crate::scope::Scope;

/// List all packages installed in `scope`, in lockfile order.
pub fn list(scope: &Scope) -> Result<Vec<LockedPackage>, RegistryError>;

/// Look up a single package by name.
pub fn get(
    scope: &Scope,
    name: &PackageName,
) -> Result<Option<LockedPackage>, RegistryError>;
```

Both are thin wrappers around `LockFile::load` + `find`. They exist as the public API surface so consumers (tau-cli, future tau-runtime) don't need to reach into `lockfile.rs`'s internals.

### 3.6 Manifest reading (`manifest.rs`)

```rust
use std::path::Path;

use tau_domain::{PackageManifest, UncheckedManifest};

use crate::error::ManifestReadError;

/// Read and validate a manifest from disk. Convenience wrapper around
/// `toml::from_str` + `UncheckedManifest::validate`.
///
/// Path is the manifest file (typically `<package_root>/tau.toml`),
/// not the package root.
pub fn read_manifest(path: &Path) -> Result<PackageManifest, ManifestReadError>;
```

`ManifestReadError` composes `std::io::Error`, `toml::de::Error`, and `tau_domain::PackageManifestError` via `#[from]`.

### 3.7 Public API surface (re-exports in `lib.rs`)

```rust
// Errors
pub use error::{
    GitError, InstallError, ManifestReadError, RegistryError, ScopeError, UninstallError,
};

// Scope
pub use scope::{Scope, ScopeConfig, ScopeKind};

// Lockfile
pub use lockfile::{LockFile, LockedPackage, LockedVersion};

// Operations
pub use install::{install, install_with_options, uninstall, InstallOptions, InstalledPackage};
pub use registry::{get, list};
pub use manifest::read_manifest;
```

### 3.8 Errors (`error.rs`)

Per-operation typed errors, all `#[non_exhaustive]`, all derive `Debug + Error + Clone + PartialEq + Eq`. Composition via `#[from]` mirrors the tau-domain / tau-ports pattern.

```rust
use thiserror::Error;

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScopeError {
    #[error("HOME directory not found")]
    HomeNotFound,
    #[error("scope path is not a directory: {path}")]
    NotADirectory { path: String },
    #[error("config.toml schema version {found} not supported (max supported: {supported})")]
    ConfigSchemaTooNew { found: u32, supported: u32 },
    #[error("config.toml parse error: {reason}")]
    ConfigParse { reason: String },
    #[error("io: {message}")]
    Io { message: String },
    /// Plugin internal error.
    /// See: [escape-hatches.md#scopeerror-internal](../docs/explanation/escape-hatches.md#scopeerror-internal).
    #[error("internal: {message}")]
    Internal { message: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GitError {
    #[error("git not found on PATH; install git or add it to PATH")]
    GitMissing,
    #[error("git clone failed: exit {exit_code}: {stderr}")]
    CloneFailed { exit_code: i32, stderr: String },
    #[error("git command failed: {what}: {stderr}")]
    CommandFailed { what: String, stderr: String },
    #[error("io: {message}")]
    Io { message: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ManifestReadError {
    #[error("manifest not found at {path}")]
    NotFound { path: String },
    #[error("manifest io: {message}")]
    Io { message: String },
    #[error("manifest TOML parse: {reason}")]
    Parse { reason: String },
    #[error("manifest validation: {0}")]
    Validation(#[from] tau_domain::PackageManifestError),
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    #[error("io: {message}")]
    Io { message: String },
    #[error("lockfile TOML parse: {reason}")]
    Parse { reason: String },
    #[error("lockfile schema version {found} not supported (max supported: {supported})")]
    SchemaTooNew { found: u32, supported: u32 },
    /// Phase-1 use; structural at v0.1.
    #[error("lockfile checksum mismatch for {name}@{version}")]
    ChecksumMismatch { name: String, version: String },
    /// Plugin internal error.
    /// See: [escape-hatches.md#registryerror-internal](../docs/explanation/escape-hatches.md#registryerror-internal).
    #[error("internal: {message}")]
    Internal { message: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InstallError {
    #[error("git: {0}")]
    Git(#[from] GitError),
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestReadError),
    #[error("registry: {0}")]
    Registry(#[from] RegistryError),
    #[error("scope: {0}")]
    Scope(#[from] ScopeError),
    #[error("source / manifest mismatch: expected {expected:?}, found {found:?}")]
    SourceManifestMismatch { expected: String, found: String },
    #[error("install operation already in progress for scope {scope}")]
    Locked { scope: String },
    /// Plugin internal error.
    /// See: [escape-hatches.md#installerror-internal](../docs/explanation/escape-hatches.md#installerror-internal).
    #[error("internal: {message}")]
    Internal { message: String },
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UninstallError {
    #[error("registry: {0}")]
    Registry(#[from] RegistryError),
    #[error("scope: {0}")]
    Scope(#[from] ScopeError),
    #[error("package not installed: {name}")]
    NotInstalled { name: String },
    #[error("version not installed: {name}@{version}")]
    VersionNotInstalled { name: String, version: String },
    #[error("io: {message}")]
    Io { message: String },
    #[error("uninstall operation already in progress for scope {scope}")]
    Locked { scope: String },
    /// Plugin internal error.
    /// See: [escape-hatches.md#uninstallerror-internal](../docs/explanation/escape-hatches.md#uninstallerror-internal).
    #[error("internal: {message}")]
    Internal { message: String },
}
```

Five new escape-hatch entries land in the registry: `scopeerror-internal`, `registryerror-internal`, `installerror-internal`, `uninstallerror-internal`, plus one for the not-yet-named `giterror` if we add one (currently `GitError` doesn't have an `Internal` variant — keep it that way: every git failure has a typed cause).

---

## 4. Parsers

Per QG5, parsers of external input (manifests, IPC messages, user config) earn proptest coverage. tau-pkg's parsers:

| Parser | What | Proptest coverage |
|---|---|---|
| `read_manifest()` | TOML `tau.toml` → `PackageManifest` | round-trip + malformed-input rejection table |
| `LockFile::load()` | TOML `tau-lock.toml` → `LockFile` | round-trip with arbitrary `LockFile` instances; schema-version too-new rejection |
| `ScopeConfig` | TOML `config.toml` → `ScopeConfig` | round-trip with arbitrary configs; schema-version too-new rejection |

Plus the git-URL parsing already done by tau-domain's `PackageSource::FromStr` — covered there.

---

## 5. Testing strategy

Per QG5 + the Phase-0 testing layers established in tau-domain / tau-ports.

### Layers

**Unit tests, inline in `src/*.rs`:**
- `Scope::resolve` walk-up algorithm with mocked `cwd` (use `tempfile::TempDir`).
- `Scope::lockfile_path` / `config_path` / `packages_dir` etc. produce expected paths per scope kind.
- `LockFile::default`/`upsert`/`remove`/`find` operate correctly.
- `Git::version_check` returns success when `git` is on PATH, `GitError::GitMissing` when not (mock by setting PATH to empty in subprocess).
- Per error enum: `Display` impls render readable text.

**Integration tests, in `tests/`:**
- `tests/scope_resolve.rs` — table of cwd paths + expected resolved scopes.
- `tests/install_lifecycle.rs` — full end-to-end install of a tiny test fixture package (uses a local file:// git URL via `git init --bare` + push from a test fixture).
- `tests/uninstall.rs` — install then uninstall, verify state.
- `tests/concurrent_install.rs` — spawn two threads, both call `install_with_options(block_on_lock = false)`, expect one succeeds and the other returns `InstallError::Locked`.
- `tests/manifest_validation.rs` — table of malformed manifest TOML inputs and expected error variants.

**Proptest:**
- `proptest_lockfile_roundtrip` — arbitrary `LockFile` instances round-trip through TOML.
- `proptest_scope_config_roundtrip` — arbitrary `ScopeConfig` round-trips.
- `proptest_manifest_validation_rejection` — malformed manifests rejected with specific error variants.

**Doc tests:**
- Every public item has at least one example.
- Examples on `#[non_exhaustive]` types are `ignore`-marked per the established pattern.
- Runnable examples on `Scope::resolve`, `read_manifest`, `list`, `get`, `LockFile::load/save`.

**Test fixtures:**
- A `tests/fixtures/` directory containing tiny synthetic git repos (created on demand via `git init --bare` + scripted commits). The integration tests use these as install sources via `file://` URLs to avoid hitting the network.

### CI implications

Two new CI jobs in `.github/workflows/ci.yml`:
- `no-default-features-pkg`: `cargo build -p tau-pkg --no-default-features` + `cargo test -p tau-pkg --no-default-features --lib`.
- The existing matrix jobs (test on Linux+macOS+Windows × stable+MSRV) automatically cover tau-pkg with default features.

`fs4` is cross-platform; `git` should be installed on all CI runners (GitHub Actions runners come with git pre-installed). Windows tests should pass without special handling.

---

## 6. ADR-0004 — tau-pkg public API

ADR-0004 is filed at `docs/decisions/0004-tau-pkg.md` as part of this sub-project. Records:

1. **Sync public API** — no async runtime in tau-pkg. Trigger to revisit: real concurrent-install use case appears.
2. **Shell out to `git` binary** — assumes git on PATH (tau is a developer tool per NG11). Revisit when shipping pre-built binaries to non-developers.
3. **Storage layout** — `<scope>/.tau/packages/<name>/<version>/`. Multi-version cohabitation per scope.
4. **Scope resolution** — walk up for `.tau/`, fall back to global. `~/.tau` (or `$XDG_DATA_HOME/tau` if set).
5. **Lockfile location** — project: `<project>/tau-lock.toml` at root (committed); global: `~/.tau/tau-lock.toml` (local).
6. **`.tau/` is gitignored local state** — `tau init` prints a hint, does NOT modify `.gitignore`. Pure information.
7. **Lockfile schema** — versioned, includes `sha256` slot empty at v0.1.
8. **Public API verbs** — `install`, `uninstall`, `list`, `get`, `resolve_scope`, `read_manifest`. Update + verify deferred to Phase 1+.
9. **Manifest location convention** — `tau.toml` at package repo root.
10. **No transitive resolution at v0.1** — `dependencies` is informational.
11. **Concurrent install protection** — advisory file lock via `fs4`.
12. **Error taxonomy** — per-operation typed errors with `#[from]` composition (no top-level `PkgError` umbrella).

---

## 7. Commit / sub-task strategy

The plan derived from this spec follows the per-task commit pattern from Plans 1–3. Anticipated task ordering:

1. Workspace + crate `Cargo.toml` updates (add `fs4` and promote `toml` to workspace deps).
2. `error.rs`: leaf errors (ScopeError, GitError, ManifestReadError, RegistryError).
3. `error.rs`: composing errors (InstallError, UninstallError) with `#[from]`.
4. `scope.rs`: `ScopeConfig` schema + serde derives.
5. `scope.rs`: `Scope` + `ScopeKind` + `resolve()` + `global()` + `new_project()`.
6. `lockfile.rs`: `LockFile` + `LockedPackage` + `LockedVersion` schema + serde.
7. `lockfile.rs`: `LockFile::load/save/find/upsert/remove` + atomic write.
8. `git.rs`: `Git::version_check` + `clone` + `resolve_head`.
9. `manifest.rs`: `read_manifest` + tests.
10. `install.rs`: `install` lifecycle (10-step pipeline above).
11. `install.rs`: `uninstall` + `InstallOptions`.
12. `registry.rs`: `list` + `get`.
13. Proptest suite (lockfile / config / manifest).
14. Integration test suite (scope_resolve, install_lifecycle, uninstall, concurrent_install, manifest_validation) + test fixtures.
15. CI: `no-default-features-pkg` job.
16. Update `docs/explanation/escape-hatches.md` with new entries.
17. ADR-0004.
18. Final local verification.
19. ADR-0004 sign-off (24h wait per QG22).
20. QG22 overnight checkpoint + Plan 4 sign-off.

---

## 8. Risks & rollbacks

| Risk | Mitigation |
|---|---|
| `git` binary not on PATH; tau-pkg fails opaquely | `Git::version_check` runs at install start; produces clear `GitError::GitMissing` with remediation message. |
| Concurrent installs corrupt lockfile / packages dir | `fs4` advisory lock + atomic write-then-rename. |
| Lockfile schema evolves; old tau version reads new lockfile | `RegistryError::SchemaTooNew { found, supported }` rejects gracefully. |
| Manifest claims a different name than the user's source URL | `InstallError::SourceManifestMismatch` errors out before disk state is touched. |
| Clone partially succeeds then fails | All clones go through a `.staging/` temp dir; final `fs::rename` is atomic. Failure leaves no partial state. |
| User's git config has unusual auth (GPG-signed-clone, etc.) | tau-pkg inherits via subprocess; failures surface git's own error message (which the user understands). |
| `.tau/` accidentally committed to git | `tau init` (Phase 1) prints a hint; spec calls out the gitignore guidance prominently in tau-cli's UX. |
| Multi-version cohabitation produces disk bloat | Disk space is cheap at v0.1 scale. `tau uninstall <name>@<version>` is the GC tool. |
| Active version pointer drift if user manually deletes a `packages/<name>/<version>/` directory | Phase-1 `tau verify` detects. v0.1 trusts the user. |

Rollback strategy: any single sub-task commit is independently revertable. The plan ordering (deps → leaf errors → composing errors → leaf modules → integrating modules → tests → CI → docs) is dependency-bottom-up.

---

## 9. Handoff to writing-plans

Inputs to the next stage:

- **This spec.**
- **`CONSTITUTION.md`** — guidelines G7, G8, G14, G16, NG4, NG6, NG9, NG11, QG2, QG3, QG5, QG18.
- **`ROADMAP.md` row 3** — sub-project 3 scope summary.
- **Plan 3** (`docs/superpowers/plans/2026-04-26-tau-ports.md`) for the established commit/test pattern.
- **ADR-0001 + ADR-0002 + ADR-0003** for the typed-error / forbid-unsafe / strict-clippy / escape-hatch posture.

The plan should:
1. Decompose §7's 20-step task list into discrete commit-sized steps.
2. Specify the test invocations after each step that prove the step lands cleanly.
3. Include the workspace-deps step as Task 1 (gates everything below).
4. End with: a "final verification" task (mirrors prior plans' patterns), an ADR-0004 task, and a QG22 overnight checkpoint.

After plan acceptance, hand off to `superpowers:subagent-driven-development` for execution under the same branch-protection workflow established in sub-projects 1 + 2 (feat branch + PR + CI gate, branch protection on main).
