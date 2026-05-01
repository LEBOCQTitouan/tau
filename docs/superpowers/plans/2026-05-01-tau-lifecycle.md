# tau update / verify / uninstall Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the three lifecycle subcommands (`tau update`, `tau verify`, `tau uninstall`) closing ADR-0007 §1's v0.1 deferral. Whole-tree SHA-256 verify is source-agnostic; update defaults to latest tag with --version pin and optional --prune; uninstall is permissive with a remediation hint.

**Architecture:** New `tau_pkg::tree_hash` module (walkdir + sha2). New `tau_pkg::verify` module returning structured drift reports. New `tau_pkg::update_package` library function composing existing `source_list` + resolver + install + uninstall. Three thin CLI wrappers in `tau-cli/src/cmd/{update,verify,uninstall}.rs`. Lockfile schema v2 → v3 (additive: `LockedVersion.sha256` populated; `LockedPlugin.binary_sha256` new field). v2-leftover entries flagged `unverified` (not drift). The library-level `tau_pkg::uninstall` already exists from Phase 0; the CLI is a thin wrapper.

**Tech Stack:** Rust 2021, `walkdir = "2"` (already a workspace dep), `sha2` (NEW workspace dep), existing `clap`, `serde`, `tau-pkg::source_list`/resolver from priority 5, `tau-cli::Output` JSON helper from priorities 5 + 6 + 8.

---

## Plan-erratum (carryover constraints)

Apply preemptively. Do NOT re-derive.

- **Cargo.lock fixup discipline (priority-6 carryover):** Task 1 adds `sha2` to workspace deps. INCLUDE Cargo.lock in the same commit. Don't repeat priority-6 Task 1's post-PR fixup miss. The same applies to any later task that adds a dep.

- **`walkdir` is already a workspace dep** at root `Cargo.toml:49` (`walkdir = "2"`). Task 1 only needs to declare it in `crates/tau-pkg/Cargo.toml [dependencies]` (no Cargo.toml workspace-deps change needed for walkdir).

- **`sha2` is NOT yet a workspace dep.** Task 1 adds `sha2 = "0.10"` to root `[workspace.dependencies]` AND to `crates/tau-pkg/Cargo.toml [dependencies]`.

- **`#[non_exhaustive]` discipline:** new error enums (`VerifyError`, `UpdateError`) get `#[non_exhaustive]`. Existing `LockedPlugin` is `#[non_exhaustive]` so adding `binary_sha256: String` is non-breaking. Doctests on `#[non_exhaustive]` types get `ignore`-marked.

- **Existing `tau_pkg::uninstall(name, version, scope)` ALREADY exists** at `crates/tau-pkg/src/install.rs:612` and handles whole-package, per-version, active-version-promotion, and lockfile mutation. Task 5 (`cmd/uninstall.rs`) is a thin CLI wrapper — DO NOT reimplement the library logic.

- **`LockedVersion.sha256` field ALREADY exists** in lockfile schema v2 (`crates/tau-pkg/src/lockfile.rs:178`, currently empty at v0.1). Task 2 just populates it; no schema migration needed for THIS field.

- **The `LockedPlugin.binary_sha256` field is NEW** in lockfile schema v3. Additive change — `#[serde(default)]` on the new field; v2 lockfiles auto-upgrade (the new field defaults to empty string).

- **Lockfile schema v2 → v3 is additive only.** v2 lockfiles auto-upgrade on next save. v2-leftover entries (empty `sha256` from old installs) are flagged `unverified` by `tau verify` — exit 0 unless other drifts exist.

- **JSON event-per-line streaming (ADR-0011 carryover):** `tau update --json` and `tau verify --json` use the same per-line event pattern as `tau run --stream --json`. Output via the existing `Output::json` helper.

- **Test fixture pattern (priority 5 + 6 carryover):** all CLI integration tests use `file://` git fixtures (no real network in CI). Mirror existing `cmd_install_*.rs` test pattern. The shared test infrastructure is in `crates/tau-cli/tests/common/mod.rs` and `crates/tau-pkg/tests/fixtures/`.

- **EXISTING test fixtures embed `sha256 = ""` strings** in lockfile TOML expectations (6+ places: `crates/tau-pkg/tests/install_builds_rust_cargo_plugin.rs:301`, `crates/tau-pkg/tests/proptest_lockfile.rs:55`, `crates/tau-pkg/tests/proptest_lockfile.proptest-regressions:7`, `crates/tau-cli/tests/common/mod.rs:128/368/394`). Task 2 MUST audit and update these to either: (a) accept any non-empty sha256 string (using a regex / partial assertion), or (b) embed a deterministic hash that the test inputs reproduce. Pick approach (a) — simpler, more robust to future hash-algorithm migration. Document the count of affected tests upfront in Task 2.

- **`install_with_options` is the canonical install path** at `crates/tau-pkg/src/install.rs` (~700 LOC). Task 2 modifies this function in two places:
  1. After source materialization (post-clone), call `tree_hash` and stuff into `LockedVersion.sha256` BEFORE the lockfile save.
  2. After `cargo build` succeeds for plugin packages, call `sha256_of_file(binary_path)` and stuff into `LockedPlugin.binary_sha256`.
  These mutations must NOT change the existing install path's error handling. New hash failures get folded into `InstallError::Io` with descriptive messages.

- **Three-bucket exit codes (ADR-0007 §7):** verify drift → exit 2; update success → exit 0; update failure → exit 2; uninstall success → exit 0; uninstall not-installed → exit 2. Reuse the existing `CliError` → exit-code dispatch.

- **Insta snapshots:** `tau --help` (top-level subcommand list), `tau update --help`, `tau verify --help`, `tau uninstall --help`. Run `cargo insta accept` after first test run.

- **NO new CI jobs.** No new workspace member; no new external service. Branch protection stays at 23 required checks.

- **Symlinks:** `walkdir::WalkDir::follow_links(false)` in `tree_hash`. Symlinks contribute their path bytes (target name) to the hash, not the resolved file's content. Locked here to prevent symlink-loop pitfalls during walk.

---

## File structure

| Path | Status | Purpose |
|------|--------|---------|
| `Cargo.toml` (root) | Modify | Add `sha2 = "0.10"` to `[workspace.dependencies]` (Task 1). |
| `crates/tau-pkg/Cargo.toml` | Modify | Add `sha2` and `walkdir` to `[dependencies]` (Task 1). |
| `crates/tau-pkg/src/lib.rs` | Modify | Declare `pub mod tree_hash;`, `pub mod verify;`; re-export `tree_hash`, `VerifyReport`, `VerifyError`, `UpdateError`, `update_package`. |
| `crates/tau-pkg/src/tree_hash.rs` | Create | Source-agnostic SHA-256 walker. ~150 LOC + 5 unit tests (Task 1). |
| `crates/tau-pkg/src/lockfile.rs` | Modify | Add `LockedPlugin.binary_sha256: String` field (Task 2). Bump `LockFile::CURRENT_SCHEMA_VERSION` constant from `2` to `3` if it exists, else just bump the version on save when a binary hash is present (Task 2). |
| `crates/tau-pkg/src/install.rs` | Modify | Populate `LockedVersion.sha256` and `LockedPlugin.binary_sha256` in `install_with_options` (Task 2). |
| `crates/tau-pkg/src/verify.rs` | Create | `VerifyReport`, `VerifyStatus`, `VerifyError`, pure `verify(scope, name, version)` and `verify_all(scope)` functions. ~200 LOC + 6 unit tests (Task 3). |
| `crates/tau-pkg/src/update.rs` | Create | `UpdateError`, `UpdateResult`, `update_package(name, version, scope, prune)` library function. Composes `source_list` + resolver + `install_with_options` + optional `uninstall`. ~200 LOC + 4 unit tests (Task 4). |
| `crates/tau-pkg/src/error.rs` | Modify | Re-export new error enums. |
| `crates/tau-pkg/tests/install_*.rs` | Modify | Update lockfile assertions to accept non-empty `sha256` strings (Task 2). |
| `crates/tau-pkg/tests/uninstall.rs` | Modify | Update assertions to accept v3-shape lockfile with binary_sha256 fields (Task 2). |
| `crates/tau-pkg/tests/proptest_lockfile.rs` | Modify | Update fixture string to allow non-empty sha256 (Task 2). |
| `crates/tau-cli/src/cli.rs` | Modify | Add `Command::Update(UpdateArgs)`, `Command::Verify(VerifyArgs)`, `Command::Uninstall(UninstallArgs)` enum variants (Tasks 5, 6, 7). |
| `crates/tau-cli/src/cmd/uninstall.rs` | Create | Thin CLI wrapper over `tau_pkg::uninstall` (Task 5). ~80 LOC. |
| `crates/tau-cli/src/cmd/verify.rs` | Create | CLI wrapper over `tau_pkg::verify`. Human + JSON output. ~150 LOC (Task 6). |
| `crates/tau-cli/src/cmd/update.rs` | Create | CLI wrapper over `tau_pkg::update_package`. Human + JSON output. ~150 LOC (Task 7). |
| `crates/tau-cli/src/cmd/mod.rs` | Modify | Declare new sub-modules. |
| `crates/tau-cli/src/lib.rs` (or main.rs) | Modify | Dispatch new `Command::*` variants to their handlers. |
| `crates/tau-cli/src/error.rs` | Modify | Add `From<tau_pkg::VerifyError>`, `From<tau_pkg::UpdateError>`, `From<tau_pkg::UninstallError>` impls if missing. |
| `crates/tau-cli/tests/common/mod.rs` | Modify | Update fixture `sha256 = ""` strings to accept non-empty (Task 2). |
| `crates/tau-cli/tests/cmd_uninstall.rs` | Create | 3 e2e tests (Task 5). |
| `crates/tau-cli/tests/cmd_verify.rs` | Create | 5 e2e tests (Task 6). |
| `crates/tau-cli/tests/cmd_update.rs` | Create | 4 e2e tests (Task 7). |
| `crates/tau-cli/tests/snapshots/*.snap` | Modify | Insta accepts for new help text (Task 8). |
| `docs/decisions/0012-tau-lifecycle-commands.md` | Create (Task 10) | Full ADR locking the 4 design decisions. |
| `ROADMAP.md` | Modify (Task 10) | Mark Tier 2 priority 7 ✅ Shipped. |

---

## Task 1: `tau_pkg::tree_hash` module

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/Cargo.toml` — add `sha2 = "0.10"` to `[workspace.dependencies]`.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-pkg/Cargo.toml` — add `sha2 = { workspace = true }` and `walkdir = { workspace = true }` to `[dependencies]`.
- Create: `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/tree_hash.rs`
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/lib.rs` — declare `pub mod tree_hash;` and re-export.

### Steps

- [ ] **Step 1.1: Verify dep versions**

```bash
cargo search sha2 2>/dev/null | head -3
cargo search walkdir 2>/dev/null | head -3
```

Expected: `sha2 = "0.10.x"`, `walkdir = "2.x.x"`. Pin to `"0.10"` and `"2"` (major) for forward-compat.

- [ ] **Step 1.2: Add sha2 to root workspace deps**

Edit `/Users/titouanlebocq/code/tau/Cargo.toml`. Find `[workspace.dependencies]` (around line 33). Add alphabetically (after `serde` / before `walkdir`):

```toml
sha2            = "0.10"
```

- [ ] **Step 1.3: Add deps to tau-pkg's Cargo.toml**

Edit `/Users/titouanlebocq/code/tau/crates/tau-pkg/Cargo.toml`. In `[dependencies]`, add (alphabetically):

```toml
# Source-agnostic whole-tree SHA-256 for `tau verify` (ADR-0012 / priority 7).
sha2            = { workspace = true }
walkdir         = { workspace = true }
```

- [ ] **Step 1.4: Verify deps compile**

Run:

```bash
cargo build --workspace
```

Expected: PASS. The deps are added but not yet consumed.

- [ ] **Step 1.5: Declare module in lib.rs**

Edit `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/lib.rs`. Find the existing `pub mod` block. Add alphabetically (between `source_list` and `update` or wherever fits):

```rust
pub mod tree_hash;
```

In the existing `pub use` block, add:

```rust
pub use tree_hash::{tree_hash, FileHash, TreeHashError};
```

- [ ] **Step 1.6: Create tree_hash.rs**

Create `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/tree_hash.rs`:

```rust
//! Source-agnostic whole-tree SHA-256 hashing for `tau verify`
//! (ADR-0012 / Tier 2 priority 7).
//!
//! Walks the install dir, sorts files by relative path, and feeds
//! `path\0content\0` for each into a single SHA-256 stream. Excludes
//! `.git/`, `target/`, and any `*.tau-tmp/` directories.
//!
//! Symlinks are NOT followed (`walkdir::WalkDir::follow_links(false)`).
//! A symlink contributes its target path bytes to the hash, not the
//! resolved file's content. This prevents symlink-loop pitfalls.
//!
//! Used at install time (populate `LockedVersion.sha256`) and at
//! verify time (recompute + compare).

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Error from `tree_hash`.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum TreeHashError {
    /// I/O error reading a file or walking the tree.
    #[error("io error at {path}: {message}")]
    Io {
        /// The path that errored.
        path: PathBuf,
        /// The error message.
        message: String,
    },
}

/// SHA-256 of a single file. Used for plugin binary hashing
/// (`LockedPlugin.binary_sha256`).
///
/// Returns 64-char lowercase hex.
pub fn sha256_of_file(path: &Path) -> Result<String, TreeHashError> {
    let bytes = fs::read(path).map_err(|e| TreeHashError::Io {
        path: path.to_path_buf(),
        message: format!("reading file: {e}"),
    })?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Hash entry for a single file in the tree.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHash {
    /// Relative path from the tree root.
    pub rel_path: String,
    /// SHA-256 hex of the file contents (or symlink target bytes).
    pub sha256: String,
}

/// Compute a whole-tree SHA-256 of `root`.
///
/// Excludes `.git/`, `target/`, and `*.tau-tmp/` directories. Files
/// are sorted lexicographically by relative path. For each file:
///
/// ```text
/// hasher.update(rel_path.as_bytes());
/// hasher.update(&[0]);
/// hasher.update(file_contents);
/// hasher.update(&[0]);
/// ```
///
/// Returns 64-char lowercase hex.
///
/// # Example
///
/// ```ignore
/// use tau_pkg::tree_hash;
/// use std::path::Path;
///
/// let hash = tree_hash(Path::new("/some/install/dir"))?;
/// assert_eq!(hash.len(), 64);
/// # Ok::<(), tau_pkg::TreeHashError>(())
/// ```
pub fn tree_hash(root: &Path) -> Result<String, TreeHashError> {
    let mut entries: Vec<(String, PathBuf)> = Vec::new();

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|e| TreeHashError::Io {
            path: e.path().map(Path::to_path_buf).unwrap_or_default(),
            message: format!("walking tree: {e}"),
        })?;

        if entry.file_type().is_dir() {
            // Skip excluded directories by name.
            let name = entry.file_name().to_string_lossy();
            if name == ".git" || name == "target" || name.ends_with(".tau-tmp") {
                continue;
            }
            // Otherwise descend (walkdir does this automatically).
            continue;
        }

        // Skip files inside excluded directories.
        let path = entry.path();
        if path.components().any(|c| {
            let s = c.as_os_str().to_string_lossy();
            s == ".git" || s == "target" || s.ends_with(".tau-tmp")
        }) {
            continue;
        }

        let rel = path.strip_prefix(root).map_err(|e| TreeHashError::Io {
            path: path.to_path_buf(),
            message: format!("computing relative path: {e}"),
        })?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        entries.push((rel_str, path.to_path_buf()));
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (rel_path, abs_path) in entries {
        let bytes = if abs_path.is_symlink() {
            let target = fs::read_link(&abs_path).map_err(|e| TreeHashError::Io {
                path: abs_path.clone(),
                message: format!("reading symlink: {e}"),
            })?;
            target.to_string_lossy().into_owned().into_bytes()
        } else {
            fs::read(&abs_path).map_err(|e| TreeHashError::Io {
                path: abs_path.clone(),
                message: format!("reading file: {e}"),
            })?
        };

        hasher.update(rel_path.as_bytes());
        hasher.update([0u8]);
        hasher.update(&bytes);
        hasher.update([0u8]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn empty_dir_hashes_to_empty_sha256() {
        let dir = TempDir::new().unwrap();
        let h = tree_hash(dir.path()).unwrap();
        // SHA-256 of empty input is a known value:
        assert_eq!(
            h,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn single_file_is_deterministic() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        let h1 = tree_hash(dir.path()).unwrap();
        let h2 = tree_hash(dir.path()).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn nested_dirs_hash_includes_all_files() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        fs::write(dir.path().join("src/b.rs"), b"fn main() {}").unwrap();
        let h_two_files = tree_hash(dir.path()).unwrap();
        // Drop one file → different hash.
        fs::remove_file(dir.path().join("src/b.rs")).unwrap();
        let h_one_file = tree_hash(dir.path()).unwrap();
        assert_ne!(h_two_files, h_one_file);
    }

    #[test]
    fn dot_git_dir_is_excluded() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        let h_no_git = tree_hash(dir.path()).unwrap();

        // Create a fake .git/ dir with content; hash should NOT change.
        fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
        fs::write(dir.path().join(".git/HEAD"), b"ref: refs/heads/main").unwrap();
        fs::write(dir.path().join(".git/objects/abc"), b"blob").unwrap();
        let h_with_git = tree_hash(dir.path()).unwrap();
        assert_eq!(h_no_git, h_with_git, ".git/ should be excluded");
    }

    #[test]
    fn target_dir_is_excluded() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("Cargo.toml"), b"[package]").unwrap();
        let h_no_target = tree_hash(dir.path()).unwrap();

        // Create a fake target/release/<bin>; hash should NOT change.
        fs::create_dir_all(dir.path().join("target/release")).unwrap();
        fs::write(dir.path().join("target/release/foo"), b"binary").unwrap();
        let h_with_target = tree_hash(dir.path()).unwrap();
        assert_eq!(h_no_target, h_with_target, "target/ should be excluded");
    }
}
```

NOTE: `tempfile` is already a dev-dep in tau-pkg (used by other tests). Verify with `grep -n tempfile crates/tau-pkg/Cargo.toml`. If not, add `tempfile = { workspace = true }` to `[dev-dependencies]`.

- [ ] **Step 1.7: Verify**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
cargo test -p tau-pkg --all-targets tree_hash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-pkg --doc
```

Expected: build PASS; 5 tests PASS; fmt/clippy/doctest clean.

- [ ] **Step 1.8: Check Cargo.lock state**

```bash
git -C /Users/titouanlebocq/code/tau status --short
```

Cargo.lock should show as modified (sha2 + transitive deps added).

- [ ] **Step 1.9: Commit + push**

```bash
git -C /Users/titouanlebocq/code/tau add Cargo.toml Cargo.lock crates/tau-pkg/Cargo.toml crates/tau-pkg/src/tree_hash.rs crates/tau-pkg/src/lib.rs
git -C /Users/titouanlebocq/code/tau commit -m "$(cat <<'EOF'
feat(pkg): add tree_hash module — source-agnostic SHA-256 walker

Foundation for `tau verify` (Tier 2 priority 7). Walks the install
dir with walkdir, sorts files by relative path, feeds path\0content\0
for each into a single SHA-256 stream. Excludes .git/, target/, and
*.tau-tmp/ directories. Symlinks contribute target path bytes (not
resolved content); follow_links(false) prevents symlink loops.

Plus sha256_of_file() helper for plugin binary hashing.

5 unit tests: empty dir; deterministic single file; nested dirs;
.git/ exclusion; target/ exclusion.

Adds sha2 = "0.10" to workspace deps; walkdir was already there.

Refs: docs/superpowers/specs/2026-05-01-tau-lifecycle-design.md §1

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git -C /Users/titouanlebocq/code/tau push -u origin feat/lifecycle-spec
```

---

## Task 2: Populate sha256 in install_with_options + add binary_sha256 (lockfile v3)

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/lockfile.rs` — add `LockedPlugin.binary_sha256: String` field with `#[serde(default)]`. Bump `LockFile::CURRENT_SCHEMA_VERSION` if it exists; else update the save path to write `schema_version = 3`.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/install.rs` — populate `LockedVersion.sha256` (post-clone) and `LockedPlugin.binary_sha256` (post-build) inside `install_with_options`.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-pkg/tests/install_builds_rust_cargo_plugin.rs` — fixture string at line 301 currently hardcodes `sha256 = ""`. Update to assert non-empty (regex or partial match).
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-pkg/tests/install_lifecycle.rs` — same audit.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-pkg/tests/uninstall.rs` — same audit.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-pkg/tests/proptest_lockfile.rs` — fixture string at line 55 (`sha256 = \"\"`). Update.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-pkg/tests/proptest_lockfile.proptest-regressions` — regression embeds the v0.1 lockfile shape; either delete the file (regenerated on next proptest run) or update.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/tests/common/mod.rs` — three `sha256 = ""` strings at lines 128, 368, 394. Update.

### Steps

- [ ] **Step 2.1: Audit and count affected test fixtures**

```bash
cd /Users/titouanlebocq/code/tau
grep -rn 'sha256 = ""' crates/ | tee /tmp/sha256-fixtures.txt
wc -l /tmp/sha256-fixtures.txt
```

Expected: 6+ matches across the files listed above. Document the count in the commit message.

- [ ] **Step 2.2: Add `binary_sha256` field to `LockedPlugin`**

Edit `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/lockfile.rs`. Find `pub struct LockedPlugin` (around line 133). Add the new field:

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockedPlugin {
    pub manifest: PluginManifest,
    pub binary_path: PathBuf,
    #[serde(with = "humantime_serde")]
    pub built_at: SystemTime,
    /// SHA-256 of the built binary at `binary_path`. Populated by
    /// `install_with_options` after `cargo build` succeeds. Empty
    /// for v2-leftover entries (informational `unverified` status
    /// from `tau verify`, not drift).
    ///
    /// Added in lockfile schema v3.
    #[serde(default)]
    pub binary_sha256: String,
}
```

Update `LockedPlugin::new` to take a `binary_sha256` parameter:

```rust
impl LockedPlugin {
    pub fn new(
        manifest: PluginManifest,
        binary_path: PathBuf,
        built_at: SystemTime,
        binary_sha256: String,
    ) -> Self {
        Self { manifest, binary_path, built_at, binary_sha256 }
    }
}
```

NOTE: This breaks any caller of `LockedPlugin::new` — search and update:

```bash
grep -rn "LockedPlugin::new" crates/
```

Update each call-site to pass `String::new()` (or compute the hash if appropriate).

- [ ] **Step 2.3: Bump schema version**

Edit `lockfile.rs`. If there's a `CURRENT_SCHEMA_VERSION` constant, bump from `2` to `3`. Otherwise update the save path: where `LockFile.schema_version` is set on save, set it to `3` if any package's plugin has a non-empty `binary_sha256`.

For simplicity: just bump unconditionally. v2 lockfiles are still loaded successfully (the new field defaults to empty); the new file will be saved as v3.

```rust
// In LockFile::save (or equivalent):
self.schema_version = 3;
```

- [ ] **Step 2.4: Populate sha256 in install_with_options**

Edit `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/install.rs`. Find `install_with_options` (around line 157). Locate the section AFTER `Git::clone` succeeds (the source tree is materialized) and BEFORE the lockfile is built/saved. Add:

```rust
// Compute source-tree SHA-256 for verify (ADR-0012).
let source_sha256 = crate::tree_hash::tree_hash(&install_dir).map_err(|e| {
    InstallError::Io {
        message: format!("computing source tree hash: {e}"),
    }
})?;
```

Then where `LockedVersion` is constructed, set `sha256: source_sha256` (was previously `sha256: String::new()`).

Find the section where `LockedPlugin` is constructed (after `cargo build` succeeds for plugin packages). Add:

```rust
let binary_sha256 = crate::tree_hash::sha256_of_file(&binary_path).map_err(|e| {
    InstallError::Io {
        message: format!("computing plugin binary hash: {e}"),
    }
})?;
```

Pass `binary_sha256` to `LockedPlugin::new(manifest, binary_path, built_at, binary_sha256)`.

- [ ] **Step 2.5: Update test fixture assertions (CLI common)**

Edit `/Users/titouanlebocq/code/tau/crates/tau-cli/tests/common/mod.rs`. The three `sha256 = ""` strings at lines 128, 368, 394 are TOML fixture EXPECTATIONS. Replace strict equality assertions with loose matches.

Where the test reads the lockfile and asserts on its contents, change from:

```rust
let expected = r#"
schema_version = 2
...
sha256 = ""
...
"#;
assert_eq!(lockfile_toml, expected);
```

To either:

```rust
// Allow any non-empty sha256:
let actual: toml::Value = toml::from_str(&lockfile_toml).unwrap();
let sha = actual["package"][0]["versions"][0]["sha256"].as_str().unwrap();
assert!(!sha.is_empty(), "sha256 should be populated");
assert_eq!(sha.len(), 64, "sha256 should be 64-char hex");
```

OR simpler — if the test just wants to assert "the lockfile loaded" and not on field values, just remove the sha256 assertion.

- [ ] **Step 2.6: Update test fixture assertions (tau-pkg tests)**

Same approach as Step 2.5 for:
- `crates/tau-pkg/tests/install_builds_rust_cargo_plugin.rs:301`
- `crates/tau-pkg/tests/install_lifecycle.rs` (any sha256 assertions)
- `crates/tau-pkg/tests/uninstall.rs` (any sha256 assertions)
- `crates/tau-pkg/tests/proptest_lockfile.rs:55`

For `proptest_lockfile.rs`, the proptest input strings can keep `sha256 = ""` as INPUT (testing that v2-leftover lockfiles round-trip); the ASSERTIONS should be loosened.

For `proptest_lockfile.proptest-regressions` — DELETE the file (proptest regenerates it on next run if a regression is found).

- [ ] **Step 2.7: Add 2 new unit tests for the populated path**

In `crates/tau-pkg/src/install.rs`'s `#[cfg(test)] mod tests`, add:

```rust
#[test]
fn install_populates_locked_version_sha256() {
    // Use existing file:// git fixture pattern. After install, load
    // the lockfile and assert the LockedVersion.sha256 is non-empty
    // 64-char hex.
    let scope_dir = TempDir::new().unwrap();
    let scope = Scope::custom(scope_dir.path()).unwrap();
    let source = make_test_git_fixture(); // reuse existing helper
    let installed = install(&source, &scope).expect("install OK");

    let lf = LockFile::load(&scope.lockfile_path()).unwrap();
    let pkg = lf.find(&installed.name).expect("package in lockfile");
    let v = pkg.installed_versions.first().expect("at least one version");
    assert_eq!(v.sha256.len(), 64, "sha256 should be 64-char hex");
    assert!(!v.sha256.is_empty(), "sha256 should be populated");
}

#[test]
fn install_populates_locked_plugin_binary_sha256_for_plugin_pkg() {
    // Use the existing plugin-fixture pattern from
    // install_builds_rust_cargo_plugin.rs. After install + cargo build,
    // assert LockedPlugin.binary_sha256 is non-empty 64-char hex.
    // Skip on platforms / CI where cargo build is too slow if needed.
    let scope_dir = TempDir::new().unwrap();
    let scope = Scope::custom(scope_dir.path()).unwrap();
    let source = make_plugin_git_fixture(); // reuse existing helper
    let installed = install_with_options(
        &source,
        &scope,
        &InstallOptions { build_plugin: true, ..Default::default() },
    ).expect("install + build OK");

    let lf = LockFile::load(&scope.lockfile_path()).unwrap();
    let pkg = lf.find(&installed.name).expect("package in lockfile");
    let plugin = pkg.plugin.as_ref().expect("plugin field populated");
    assert_eq!(plugin.binary_sha256.len(), 64, "binary_sha256 should be 64-char hex");
}
```

NOTE: the test helper names (`make_test_git_fixture`, `make_plugin_git_fixture`) may need adjustment to match the actual existing fixture pattern — search for analogous helpers in `crates/tau-pkg/tests/install_*.rs`. If no exact match, write the inline.

- [ ] **Step 2.8: Verify**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: ALL PASS. The audit count is the number of tests previously asserting `sha256 = ""`; all should now PASS with loose assertions.

- [ ] **Step 2.9: Commit + push**

```bash
git -C /Users/titouanlebocq/code/tau add -A
git -C /Users/titouanlebocq/code/tau commit -m "$(cat <<'EOF'
feat(pkg): populate LockedVersion.sha256 + add LockedPlugin.binary_sha256

`install_with_options` now computes the install tree's source SHA-256
(via tau_pkg::tree_hash) and the plugin binary SHA-256 (via
tau_pkg::sha256_of_file), populating the previously-empty fields:

- LockedVersion.sha256 — schema v2 reservation, now populated.
- LockedPlugin.binary_sha256 — new field (lockfile schema v3).

Lockfile schema bumps v2 → v3 on next save. v2 lockfiles auto-upgrade;
v2-leftover entries (empty sha256) will be flagged `unverified` by
`tau verify` (Task 3 / 6).

Updated N existing test fixtures to accept non-empty sha256 strings
instead of strict-equal `sha256 = ""` assertions.

Refs: docs/superpowers/specs/2026-05-01-tau-lifecycle-design.md §1, §5

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git -C /Users/titouanlebocq/code/tau push
```

---

## Task 3: `tau_pkg::verify` module + drift report types

**Files:**
- Create: `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/verify.rs`
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/lib.rs` — declare module + re-export.

### Steps

- [ ] **Step 3.1: Declare module + re-export**

Edit `crates/tau-pkg/src/lib.rs`. Add:

```rust
pub mod verify;
pub use verify::{verify, verify_all, VerifyError, VerifyReport, VerifyStatus};
```

- [ ] **Step 3.2: Create verify.rs**

Create `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/verify.rs`:

```rust
//! `tau verify` — recompute install-tree SHA-256, compare to lockfile.
//! Source-agnostic; works for any future PackageSource variant.
//!
//! See ADR-0012 + spec §1 / §3.

use std::path::PathBuf;

use tau_domain::{PackageName, Version};

use crate::lockfile::{LockFile, LockfileError};
use crate::scope::Scope;
use crate::tree_hash::{sha256_of_file, tree_hash, TreeHashError};

/// Per-package verification status.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyStatus {
    /// Hash matches the lockfile entry. No drift.
    Ok,
    /// Source tree SHA-256 doesn't match `LockedVersion.sha256`.
    TreeDrift {
        /// Hex hash from the lockfile.
        expected: String,
        /// Hex hash recomputed now.
        actual: String,
    },
    /// Plugin binary SHA-256 doesn't match `LockedPlugin.binary_sha256`.
    BinaryDrift {
        /// Path to the binary.
        path: PathBuf,
        /// Hex hash from the lockfile.
        expected: String,
        /// Hex hash recomputed now.
        actual: String,
    },
    /// Install dir doesn't exist on disk.
    Missing {
        /// The install dir the lockfile pointed to.
        path: PathBuf,
    },
    /// Lockfile entry has empty sha256 (v2-leftover). Informational;
    /// not drift.
    Unverified,
}

impl VerifyStatus {
    /// Whether this status represents drift (non-zero exit).
    pub fn is_drift(&self) -> bool {
        matches!(
            self,
            VerifyStatus::TreeDrift { .. }
                | VerifyStatus::BinaryDrift { .. }
                | VerifyStatus::Missing { .. }
        )
    }
}

/// Verification result for one package version.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    /// Package name.
    pub name: PackageName,
    /// Version.
    pub version: Version,
    /// Per-package status.
    pub status: VerifyStatus,
}

/// Error from `verify` / `verify_all`.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// Lockfile load failed.
    #[error("loading lockfile: {source}")]
    LockfileLoad {
        /// Source error.
        #[from]
        source: LockfileError,
    },

    /// Package not found in lockfile.
    #[error("package {name} not installed")]
    PackageNotInstalled {
        /// Package name.
        name: String,
    },

    /// Version not found in lockfile entry.
    #[error("package {name}: version {version} not installed")]
    VersionNotInstalled {
        /// Package name.
        name: String,
        /// Version.
        version: String,
    },

    /// I/O error during hash computation.
    #[error("io error: {source}")]
    Io {
        /// Source error.
        #[from]
        source: TreeHashError,
    },
}

/// Verify a single (package, version) pair.
pub fn verify(
    scope: &Scope,
    name: &PackageName,
    version: &Version,
) -> Result<VerifyReport, VerifyError> {
    let lockfile = LockFile::load(&scope.lockfile_path())?;

    let pkg = lockfile.find(name).ok_or_else(|| VerifyError::PackageNotInstalled {
        name: name.as_str().to_owned(),
    })?;

    let lv = pkg.installed_versions.iter().find(|lv| &lv.version == version).ok_or_else(|| {
        VerifyError::VersionNotInstalled {
            name: name.as_str().to_owned(),
            version: version.to_string(),
        }
    })?;

    let install_dir = scope.package_dir(name, version);

    if !install_dir.exists() {
        return Ok(VerifyReport {
            name: name.clone(),
            version: version.clone(),
            status: VerifyStatus::Missing { path: install_dir },
        });
    }

    if lv.sha256.is_empty() {
        return Ok(VerifyReport {
            name: name.clone(),
            version: version.clone(),
            status: VerifyStatus::Unverified,
        });
    }

    let actual = tree_hash(&install_dir)?;
    if actual != lv.sha256 {
        return Ok(VerifyReport {
            name: name.clone(),
            version: version.clone(),
            status: VerifyStatus::TreeDrift {
                expected: lv.sha256.clone(),
                actual,
            },
        });
    }

    // Binary check (if applicable).
    if let Some(plugin) = pkg.plugin.as_ref() {
        if !plugin.binary_sha256.is_empty() && plugin.binary_path.exists() {
            let bin_actual = sha256_of_file(&plugin.binary_path)?;
            if bin_actual != plugin.binary_sha256 {
                return Ok(VerifyReport {
                    name: name.clone(),
                    version: version.clone(),
                    status: VerifyStatus::BinaryDrift {
                        path: plugin.binary_path.clone(),
                        expected: plugin.binary_sha256.clone(),
                        actual: bin_actual,
                    },
                });
            }
        }
    }

    Ok(VerifyReport {
        name: name.clone(),
        version: version.clone(),
        status: VerifyStatus::Ok,
    })
}

/// Verify every (package, version) pair in the lockfile.
pub fn verify_all(scope: &Scope) -> Result<Vec<VerifyReport>, VerifyError> {
    let lockfile = LockFile::load(&scope.lockfile_path())?;
    let mut reports = Vec::new();

    for pkg in &lockfile.packages {
        for lv in &pkg.installed_versions {
            reports.push(verify(scope, &pkg.name, &lv.version)?);
        }
    }

    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_scope_with_one_install() -> (TempDir, Scope, PackageName, Version) {
        let dir = TempDir::new().unwrap();
        let scope = Scope::custom(dir.path()).unwrap();
        // Construct a minimal lockfile + install dir manually for unit
        // testing. (E2E tests use the real install path.)
        // ... [implementation: create install dir with one file, compute
        // its tree_hash, write a LockFile::default with one
        // LockedPackage + LockedVersion populated with that hash]
        unimplemented!("test helper — see tests/install_*.rs for fixture pattern")
    }

    #[test]
    fn verify_ok_when_install_matches_lockfile() {
        // ... use setup helper, call verify(), assert VerifyStatus::Ok.
    }

    #[test]
    fn verify_detects_tree_drift() {
        // ... setup, mutate a file in the install dir, call verify(),
        // assert VerifyStatus::TreeDrift { expected, actual: != expected }.
    }

    #[test]
    fn verify_detects_missing_install_dir() {
        // ... setup, fs::remove_dir_all the install dir, call verify(),
        // assert VerifyStatus::Missing { path }.
    }

    #[test]
    fn verify_returns_unverified_for_v2_leftover_empty_sha256() {
        // ... manually write a lockfile entry with sha256 = "", call
        // verify(), assert VerifyStatus::Unverified.
    }

    #[test]
    fn verify_detects_binary_drift_when_plugin_binary_modified() {
        // ... setup with plugin (LockedPlugin populated), mutate the
        // binary, call verify(), assert VerifyStatus::BinaryDrift.
    }

    #[test]
    fn verify_returns_package_not_installed_for_unknown_name() {
        let dir = TempDir::new().unwrap();
        let scope = Scope::custom(dir.path()).unwrap();
        let unknown: PackageName = "unknown".parse().unwrap();
        let v: Version = "1.0.0".parse().unwrap();
        let err = verify(&scope, &unknown, &v).unwrap_err();
        assert!(matches!(err, VerifyError::PackageNotInstalled { .. }));
    }
}
```

NOTE: The test bodies for tests 1-5 use a helper `setup_scope_with_one_install` that the implementer writes inline. The pattern: create a temp dir with a known content tree, compute its `tree_hash`, write a `LockFile` with one package + one version using that hash, save the lockfile, return `(temp_dir, scope, name, version)`.

The `Scope::custom(path)` constructor may need to be added if it doesn't exist — check `crates/tau-pkg/src/scope.rs` for the API. If `Scope::global()` and `Scope::project(path)` are the only constructors, expand them or use one of them with the test temp dir.

- [ ] **Step 3.3: Verify**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
cargo test -p tau-pkg --all-targets verify
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-pkg --doc
```

Expected: 6 verify tests PASS; fmt/clippy/doctest clean.

- [ ] **Step 3.4: Commit + push**

```bash
git -C /Users/titouanlebocq/code/tau add crates/tau-pkg/src/verify.rs crates/tau-pkg/src/lib.rs
git -C /Users/titouanlebocq/code/tau commit -m "$(cat <<'EOF'
feat(pkg): add verify module — drift detection for tau verify

Pure library function tau_pkg::verify(scope, name, version) and
tau_pkg::verify_all(scope) that:
- Recompute the install dir's tree_hash and compare to
  LockedVersion.sha256.
- Recompute the plugin binary's sha256 and compare to
  LockedPlugin.binary_sha256 (if applicable).
- Detect missing install dirs.
- Flag v2-leftover (empty sha256) entries as Unverified
  (informational; not drift).

Returns structured VerifyReport { name, version, status } with
status variants Ok / TreeDrift / BinaryDrift / Missing /
Unverified. Exit-code mapping happens in the CLI layer (Task 6).

6 unit tests covering each variant + package-not-installed error.

Refs: docs/superpowers/specs/2026-05-01-tau-lifecycle-design.md §3

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git -C /Users/titouanlebocq/code/tau push
```

---

## Task 4: `tau_pkg::update_package` library function

**Hybrid format.**

**Files:**
- Create: `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/update.rs`
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-pkg/src/lib.rs` — declare module + re-export.

**Spec sections:** §2.

**Per-task summary:**

1. **`UpdateError` enum** with variants per spec §6.1: `LockfileLoad`, `PackageNotInstalled`, `SourceList { source: SourceListError }`, `Resolve { source: ResolveError }`, `Install { source: InstallError }`, `Uninstall { source: UninstallError }`. All `#[non_exhaustive]`, all `thiserror::Error`.

2. **`UpdateResult` struct** with fields: `from_version: Version`, `to_version: Version`, `transitive_deps_changed: Vec<(PackageName, Version)>` (added or updated transitive deps).

3. **`pub fn update_package(name, version_pin, scope, prune) -> Result<UpdateResult, UpdateError>`:**
   - Load lockfile; lookup `name` → get current `source` + `active_version`. If not found, `PackageNotInstalled`.
   - If `version_pin` is `None`: call `source_list::list_versions(&source)` (priority 5), pick highest tag matching `source.rev` constraint. Map errors via `From<SourceListError>` → `UpdateError::SourceList`.
   - If `version_pin` is `Some(v)`: validate `v` is in the listed versions; if not, `Resolve { ... }` (the resolver enforces this).
   - Call `install_with_options(&source_with_new_version, &scope, &InstallOptions { build_plugin, .. })`. Map errors → `UpdateError::Install`.
   - The new version is added to `LockedPackage.installed_versions` (cohabitation). Promote the active version to the new one (modify `LockedPackage.active_version`).
   - If `prune == true`: call `tau_pkg::uninstall(name, Some(old_active_version), scope)`. Map errors → `UpdateError::Uninstall`.
   - Compute `transitive_deps_changed` by diffing the old vs new package's `[[requires.tools]]` table and noting newly-installed transitive deps.

4. **Re-resolve transitive deps** (spec §2.2):
   - When the new version's manifest declares new `[[requires.tools]]`, the resolver runs and installs them.
   - The shared install path handles this — `install_with_options` invokes the resolver if the manifest has `requires.tools`.

5. **Unit tests (~4)** using `file://` git fixtures (mirror existing `crates/tau-pkg/tests/install_*.rs` patterns):
   - `update_package_to_latest_tag` — install v1.0; tag v1.1 in source; call `update_package(name, None, scope, false)`; assert `to_version == 1.1` and old version still in `installed_versions`.
   - `update_package_to_specific_version` — call `update_package(name, Some(v1.2), scope, false)`; assert `to_version == 1.2`.
   - `update_package_with_prune_removes_old` — call `update_package(name, None, scope, true)`; assert old version removed from disk + lockfile.
   - `update_package_unreachable_version_fails` — `Some(v9.9.9)`; assert `UpdateError::Resolve { .. }`.

6. **Verification:** `cargo test -p tau-pkg --all-targets`, plus `cargo build --workspace`, `cargo test --doc`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`.

7. **Commit message:** `feat(pkg): add update_package library function`.

8. Push.

---

## Task 5: `cmd/uninstall.rs` CLI subcommand + e2e tests

**Hybrid format.**

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/cli.rs` — add `Command::Uninstall(UninstallArgs)` variant + `UninstallArgs` struct.
- Create: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/cmd/uninstall.rs`
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/cmd/mod.rs` — declare `pub mod uninstall;`.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/lib.rs` (or main.rs) — dispatch `Command::Uninstall` to `cmd::uninstall::run(args, output).await`.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/error.rs` — add `From<tau_pkg::UninstallError>` if missing.
- Create: `/Users/titouanlebocq/code/tau/crates/tau-cli/tests/cmd_uninstall.rs`

**Spec sections:** §4, §6.

**Per-task summary:**

1. **`UninstallArgs` struct** with clap derive:
   ```rust
   pub struct UninstallArgs {
       /// Package name to uninstall.
       pub package: String,
       /// Specific version (default: all versions).
       #[arg(long)]
       pub version: Option<String>,
       /// Use the global scope (~/.tau) instead of the project scope.
       #[arg(long)]
       pub global: bool,
   }
   ```

2. **`cmd/uninstall.rs::run(args, output)` function:**
   - Resolve scope (mirror `cmd/install.rs` pattern: `if args.global { Scope::global()? } else { Scope::project_from_cwd()? }`).
   - Parse `args.package` to `PackageName`; parse `args.version` to `Option<Version>`.
   - Call `tau_pkg::uninstall(&name, version.as_ref(), &scope)`. Errors → `CliError`.
   - On success: print human or JSON output per `output.is_json()`.
   - Exit 0 on success; exit 2 on `UninstallError::NotInstalled` / `Io`.

3. **Human output** (per spec §4.2):
   ```text
   ✓ Uninstalled <pkg>@<v>.
     Removed: <scope>/packages/<pkg>/<v>
     Lockfile: <scope>/lock.toml

   If any project still depends on <pkg>:
     • Remove or update the [[agents.<id>.requires.tools]] entry
       for <pkg> in the project's tau.toml.
     • Or re-install on next run: cd <project> && tau resolve
   ```

4. **JSON output** (per spec §4.3):
   ```json
   {"event":"uninstall_started","name":"...","version":"..."}
   {"event":"removed_dir","path":"..."}
   {"event":"lockfile_updated","entries_remaining":N}
   {"event":"uninstall_completed","name":"...","version":"..."}
   ```

5. **3 e2e tests** in `tests/cmd_uninstall.rs` (mirror `cmd_install.rs` pattern):
   - `cmd_uninstall_removes_all_versions` — install once; `tau uninstall <pkg>`; assert lockfile entry gone + dir gone.
   - `cmd_uninstall_with_version_keeps_other_versions` — install two versions; `tau uninstall <pkg> --version 1.0.0`; assert v1.0.0 gone + v1.1.0 still present + active promoted.
   - `cmd_uninstall_unknown_package_exits_2` — call on a never-installed package; assert exit code 2 + error message.

6. **Hint output verification:** at least one e2e test asserts the remediation hint appears in stdout (not just stderr).

7. **Verification:** standard suite + `cargo test -p tau-cli --test cmd_uninstall`.

8. **Commit message:** `feat(cli): tau uninstall subcommand`.

9. Push.

---

## Task 6: `cmd/verify.rs` CLI subcommand + e2e tests

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` — add `Command::Verify(VerifyArgs)` + `VerifyArgs` struct.
- Create: `crates/tau-cli/src/cmd/verify.rs`
- Modify: `crates/tau-cli/src/cmd/mod.rs` — declare `pub mod verify;`.
- Modify: `crates/tau-cli/src/lib.rs` — dispatch.
- Modify: `crates/tau-cli/src/error.rs` — add `From<tau_pkg::VerifyError>`.
- Create: `crates/tau-cli/tests/cmd_verify.rs`

**Spec sections:** §3, §6.

**Per-task summary:**

1. **`VerifyArgs` struct:**
   ```rust
   pub struct VerifyArgs {
       /// Package name to verify (default: all installed packages).
       pub package: Option<String>,
       /// Specific version.
       #[arg(long)]
       pub version: Option<String>,
       /// Use global scope.
       #[arg(long)]
       pub global: bool,
   }
   ```

2. **`cmd/verify.rs::run(args, output)` function:**
   - Resolve scope.
   - If `args.package` is `Some`: call `tau_pkg::verify` for that package; if `args.version` is also Some, single (pkg, v); else iterate that package's versions.
   - If `args.package` is `None`: call `tau_pkg::verify_all(scope)`.
   - For each `VerifyReport`: emit human or JSON per `output.is_json()`.
   - Exit 0 if all `Ok` or `Unverified`; exit 2 if any drift (TreeDrift / BinaryDrift / Missing).
   - Track orphaned install dirs: walk `scope.packages_dir()` and detect dirs not in the lockfile. Emit `VerifyStatus::Orphaned` events. (This requires extending `verify.rs` slightly OR doing it in the CLI layer. PICK: do it in the CLI layer — verify.rs stays a pure-lockfile-driven function; the CLI walks for orphans separately.)

3. **Human output** (per spec §3.3):
   ```text
   verify <pkg>@1.0.0... ok
   verify <other>@2.1.0... ✗ drift (tree)
     expected: abc123...
     actual:   xyz789...
   verify <plugin>@1.2.0... ✗ drift (binary)
     path: ...
     expected: def...
     actual:   ghi...

   3 packages verified, 2 drifted.
   ```

4. **JSON output** (per spec §3.4):
   ```json
   {"event":"verify_started","total":N}
   {"event":"verify_package","name":"...","version":"...","status":"ok"}
   {"event":"verify_package","name":"...","version":"...","status":"drift","kind":"tree","expected":"...","actual":"..."}
   {"event":"verify_completed","total":N,"ok":N,"drift":M,"unverified":K}
   ```

5. **5 e2e tests** in `tests/cmd_verify.rs`:
   - `cmd_verify_clean_install_exits_0` — install fixture; `tau verify`; assert exit 0.
   - `cmd_verify_tampered_file_exits_2` — install fixture; modify a file in the install tree; `tau verify`; assert exit 2 + tree drift event.
   - `cmd_verify_tampered_binary_exits_2` — install plugin fixture; modify the binary; assert exit 2 + binary drift event.
   - `cmd_verify_missing_install_dir_exits_2` — install; `fs::remove_dir_all`; assert exit 2 + missing event.
   - `cmd_verify_v2_leftover_unverified_exits_0` — write a v2-leftover lockfile manually with empty `sha256`; `tau verify`; assert exit 0 + unverified event.
   - (Plus optional: `cmd_verify_json_mode_emits_one_event_per_line`.)

6. **Verification:** standard suite + `cargo test -p tau-cli --test cmd_verify`.

7. **Commit message:** `feat(cli): tau verify subcommand`.

8. Push.

---

## Task 7: `cmd/update.rs` CLI subcommand + e2e tests

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` — add `Command::Update(UpdateArgs)` + `UpdateArgs` struct.
- Create: `crates/tau-cli/src/cmd/update.rs`
- Modify: `crates/tau-cli/src/cmd/mod.rs` — declare.
- Modify: `crates/tau-cli/src/lib.rs` — dispatch.
- Modify: `crates/tau-cli/src/error.rs` — add `From<tau_pkg::UpdateError>`.
- Create: `crates/tau-cli/tests/cmd_update.rs`

**Spec sections:** §2, §6.

**Per-task summary:**

1. **`UpdateArgs` struct:**
   ```rust
   pub struct UpdateArgs {
       /// Package name to update.
       pub package: String,
       /// Specific version (default: latest tag).
       #[arg(long)]
       pub version: Option<String>,
       /// Remove the old active version after the new install succeeds.
       #[arg(long)]
       pub prune: bool,
       /// Use global scope.
       #[arg(long)]
       pub global: bool,
   }
   ```

2. **`cmd/update.rs::run(args, output)` function:**
   - Resolve scope.
   - Parse `args.package` → `PackageName`; `args.version` → `Option<Version>`.
   - Call `tau_pkg::update_package(&name, version.as_ref(), &scope, args.prune)`. Map errors → `CliError`.
   - On success: emit human or JSON per `output.is_json()`.
   - Exit 0 on success; exit 2 on any `UpdateError`.

3. **Human output** (per spec §2.2):
   ```text
   update <pkg>
     current: 1.0.0
     resolving: <source>
     found: 1.1.0 (latest)
     installing 1.1.0...
     ✓ installed at <scope>/packages/<pkg>/1.1.0
     promoted active: 1.0.0 → 1.1.0
   ```
   With `--prune`: append `pruning: 1.0.0\n  ✓ removed ...`.

4. **JSON output** (per spec §2.3):
   ```json
   {"event":"update_started","name":"...","current":"1.0.0"}
   {"event":"resolving","source":"..."}
   {"event":"resolved","version":"1.1.0","tag":"v1.1.0"}
   {"event":"installing","version":"1.1.0"}
   {"event":"installed","version":"1.1.0","path":"..."}
   {"event":"promoted","from":"1.0.0","to":"1.1.0"}
   {"event":"pruned","version":"1.0.0"}
   {"event":"update_completed","new_active":"1.1.0"}
   ```
   The `pruned` event only emits with `--prune`.

5. **4 e2e tests** in `tests/cmd_update.rs`:
   - `cmd_update_to_latest_tag` — install v1.0; tag v1.1 in source; `tau update <pkg>`; assert exit 0 + new active = 1.1 + old still present.
   - `cmd_update_to_specific_version` — `tau update <pkg> --version 1.2.0`; assert exit 0.
   - `cmd_update_with_prune_removes_old` — `tau update <pkg> --prune`; assert old version dir removed + lockfile updated.
   - `cmd_update_unreachable_version_exits_2` — `tau update <pkg> --version 9.9.9`; assert exit 2 + error mentions "not found" or similar.

6. **Verification:** standard suite + `cargo test -p tau-cli --test cmd_update`.

7. **Commit message:** `feat(cli): tau update subcommand`.

8. Push.

---

## Task 8: Help-text snapshot updates

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/tests/snapshots/*.snap` — insta accepts.

**Spec sections:** §7.3.

**Per-task summary:**

1. After Tasks 5-7 land the new subcommands, run the existing `tests/help_snapshots.rs` test suite. It will fail because:
   - Top-level `tau --help` now lists 3 new subcommands.
   - `tau update --help`, `tau verify --help`, `tau uninstall --help` are new help outputs that don't have snapshots yet.

2. Run:
   ```bash
   cd /Users/titouanlebocq/code/tau
   cargo insta test -p tau-cli --test help_snapshots --review
   ```
   Or manually:
   ```bash
   cargo test -p tau-cli --test help_snapshots 2>&1 | tail -40
   cargo insta accept
   ```

3. Verify the new snapshot files exist:
   ```bash
   ls crates/tau-cli/tests/snapshots/help_snapshots__update_help.snap
   ls crates/tau-cli/tests/snapshots/help_snapshots__verify_help.snap
   ls crates/tau-cli/tests/snapshots/help_snapshots__uninstall_help.snap
   ```

4. Manually verify the snapshot text reads cleanly (no awkward line wrapping, no missing flags). The existing `chat`, `run`, etc. snapshot files are templates.

5. **Verification:** standard suite — all help_snapshots tests now PASS.

6. **Commit message:** `test(cli): help snapshot updates for update / verify / uninstall`.

7. Push.

---

## Task 9: Final verification + open PR

**User-driven gate. PAUSE before this task.**

### Steps

- [ ] **Step 9.1: Full local verification**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

All must pass.

- [ ] **Step 9.2: Open the PR**

```bash
gh pr list --head feat/lifecycle-spec --json number,state,isDraft
```

If empty, create:

```bash
gh pr create --title "feat: tau update / verify / uninstall (Tier 2 priority 7)" \
  --body "$(cat <<'EOF'
## Summary

Closes ADR-0007 §1's v0.1 deferral of `uninstall`, `update`, `verify`. Three new lifecycle subcommands:

- `tau update <pkg>` — defaults to latest tag, `--version` to pin, `--prune` opt-in.
- `tau verify [<pkg>]` — whole-tree SHA-256 recompute + compare. Source-agnostic. Exit 0 / 2.
- `tau uninstall <pkg>` — thin wrapper over existing `tau_pkg::uninstall` library function. Permissive (no cascade); prints remediation hint.

## Spec / Plan

- Spec: `docs/superpowers/specs/2026-05-01-tau-lifecycle-design.md`
- Plan: `docs/superpowers/plans/2026-05-01-tau-lifecycle.md`
- ADR-0012 lands in Task 10 (post-merge follow-up commit).

## Test plan

- [x] `cargo build --workspace` green
- [x] `cargo test --workspace --all-targets` green
- [x] `cargo test --workspace --doc` green
- [x] `cargo clippy --workspace --all-targets -- -D warnings` green
- [x] `cargo fmt --all -- --check` green
- [ ] CI matrix (23 required checks) green — verifying on push

## Lockfile schema

v2 → v3 (additive: `LockedPlugin.binary_sha256`). v2 lockfiles auto-upgrade; v2-leftover entries flagged `unverified` (informational, not drift).

## Out of scope

- `tau update` (no args) batch update. Footgun-y; defer.
- `tau uninstall --force` cascade detection. Self-healing via lazy resolve.
- Cryptographic signing. Future Phase 2+ ADR.
- Per-file drift diagnosis in verify reports. v0.1 reports tree-hash mismatch only.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

If a draft already exists, mark ready: `gh pr ready <number>`.

- [ ] **Step 9.3: Capture PR URL**

```bash
gh pr view --json number,url --jq '{number, url}'
```

- [ ] **Step 9.4: PAUSE — wait for CI green before Task 10**

Use the same Bash + run_in_background poller / Monitor pattern from priorities 5-8.

---

## Task 10: ADR-0012 + ROADMAP + squash merge

**User-driven gate. PAUSE before this task.**

**Files:**
- Create: `/Users/titouanlebocq/code/tau/docs/decisions/0012-tau-lifecycle-commands.md` — full ADR (4 sections per spec §8).
- Modify: `/Users/titouanlebocq/code/tau/ROADMAP.md` — mark Tier 2 priority 7 ✅.

### Steps

- [ ] **Step 10.1: Write ADR-0012**

Mirror the structure of ADR-0011. Sections:

1. **Whole-tree SHA-256 verify (source-agnostic)** — anticipates non-git PackageSource variants; works for any future source.
2. **`tau update` defaults to latest tag** — `--version` to pin, `--prune` opt-in. Multi-version cohabitation via existing `LockedPackage.installed_versions`.
3. **`tau uninstall` is permissive** — no cascade; remediation hint guides users to project tau.toml's `[[requires.tools]]` and `tau resolve`.
4. **Verify exit codes reuse 3-bucket policy** — exit 2 on any drift (kernel trust); exit 0 on all-verified-or-unverified.

Plus invariants: hash excludes `.git/`, `target/`, `*.tau-tmp/`; binary hashes separate from source-tree hashes; lockfile schema v2 → v3 additive.

Status: Accepted, 2026-05-01.

Cross-references: ADR-0007 §1 (the deferral this closes), ADR-0007 §7 (3-bucket exit code policy reused), ADR-0009 (typed-error policy), ADR-0011 (JSON event-per-line streaming convention reused).

- [ ] **Step 10.2: Update ROADMAP**

In `ROADMAP.md`, find the Tier 2 priority 7 entry:

```markdown
7. **`tau update` / `tau verify` / `tau uninstall` subcommands.**
```

Replace with:

```markdown
7. **`tau update` / `tau verify` / `tau uninstall` subcommands** ✅ Shipped 2026-05-01 — see
   [spec](docs/superpowers/specs/2026-05-01-tau-lifecycle-design.md)
   and [ADR-0012](docs/decisions/0012-tau-lifecycle-commands.md).
   New `tau_pkg::tree_hash`, `verify`, `update` modules. Whole-tree
   SHA-256 verify is source-agnostic (anticipates future
   PackageSource variants). `tau update <pkg>` defaults to latest
   tag; `--version` to pin; `--prune` opt-in. `tau uninstall` is
   permissive with a remediation hint pointing to project
   tau.toml's `[[requires.tools]]` entries. Lockfile schema
   v2 → v3 (additive: `LockedPlugin.binary_sha256`). No new CI
   jobs (23 required checks unchanged).
```

Add to the top-of-file shipped table (after the priority 8 row added in the prior sub-project):

```markdown
| 7 | tau update / verify / uninstall ✅ | Tier 2 priority 7 — closes ADR-0007 §1 deferral. New tau_pkg::tree_hash module (walkdir + sha2; excludes .git/, target/, *.tau-tmp/; symlinks contribute target bytes). New tau_pkg::verify module returning structured VerifyReport (Ok / TreeDrift / BinaryDrift / Missing / Unverified). New tau_pkg::update_package library function composing existing source_list + resolver + install + uninstall. Three CLI subcommands: tau update (default latest tag, --version pin, --prune), tau verify (exit 0/2, --json), tau uninstall (permissive + remediation hint). Lockfile schema v2 → v3 additive (LockedPlugin.binary_sha256 field; v2-leftover entries flagged unverified, not drift). Existing tau_pkg::uninstall library function reused unchanged. New ADR-0012. No new CI jobs (23 required checks unchanged). | 2026-05-01 |
```

Update the front-matter narrative paragraph: priorities 4, 5, 6, 7, 8 are now closed. Tier 2 fully complete.

- [ ] **Step 10.3: Commit + push**

```bash
git -C /Users/titouanlebocq/code/tau add docs/decisions/0012-tau-lifecycle-commands.md ROADMAP.md
git -C /Users/titouanlebocq/code/tau commit -m "$(cat <<'EOF'
docs: ADR-0012 + ROADMAP Tier 2 priority 7 done — Tier 2 complete

Locks the 4 design decisions for the lifecycle commands sub-project:
1. Verify primitive: whole-tree SHA-256, source-agnostic
2. tau update defaults to latest tag (--version to pin, --prune opt-in)
3. tau uninstall is permissive with remediation hint
4. Verify exit codes reuse the 3-bucket policy (0 / 2)

Plus invariants: hash excludes .git/, target/, *.tau-tmp/; binary
hashes separate from source-tree hashes; lockfile schema v2 → v3
additive (v2-leftover entries flagged unverified, not drift).

Updates ROADMAP:
- Top-of-file shipped table gains a row for Tier 2 priority 7.
- Tier 2 priority 7 entry marked ✅ Shipped 2026-05-01.
- Front-matter narrative updates to reflect TIER 2 FULLY COMPLETE
  (priorities 4, 5, 6, 7, 8 all closed).

No new CI jobs; branch protection stays at 23 required checks.

Refs: docs/superpowers/specs/2026-05-01-tau-lifecycle-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git -C /Users/titouanlebocq/code/tau push
```

- [ ] **Step 10.4: Wait for CI green on the PR**

Same poller pattern as priority 8. 23 required checks must all pass.

- [ ] **Step 10.5: Squash merge**

```bash
gh pr merge --squash --delete-branch
```

- [ ] **Step 10.6: Verify branch protection unchanged**

```bash
gh api repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks/contexts | jq 'length'
```

Expected: `23`.

- [ ] **Step 10.7: Sync local main + report squash SHA**

```bash
git checkout main && git pull && git log --oneline -3
```

Report back to the user with the squash SHA.

---

## Verification standard (per task)

Each task ends with:

```bash
cargo build --workspace
cargo test -p tau-pkg --all-targets        # for tau-pkg-only tasks
cargo test -p tau-cli --all-targets        # for tau-cli-only tasks
cargo test --workspace --all-targets       # for tasks touching multiple crates
cargo test --workspace --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

For tasks touching multiple crates (2, 5, 6, 7, 9, 10), use `cargo test --workspace --all-targets`.

CI continues on push; no new jobs added; branch protection stays at 23.
