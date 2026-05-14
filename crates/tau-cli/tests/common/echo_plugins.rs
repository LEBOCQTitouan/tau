//! Test helper: build the workspace `echo-llm` and `echo-tool` plugin
//! binaries once per test session and expose their absolute paths.
//!
//! Used by the Task-21 real-spawn integration tests under
//! `crates/tau-cli/tests/cmd_*.rs` — each one calls
//! [`ensure_echo_plugins_built`] (or one of the convenience wrappers)
//! to obtain a binary path it can drop into a synthesized
//! `tau-lock.toml`'s `[package.plugin]` table.
//!
//! # Why a session-cached build?
//!
//! `cargo build --release -p echo-llm -p echo-tool` is the slow step
//! (~10 s cold, ~0.5 s warm). Caching via [`OnceLock`] amortizes that
//! cost across every test in the binary that needs the echoes; without
//! it, each `#[test]` would pay the build tax even though `cargo`
//! itself is idempotent. `Once::call_once` plus `OnceLock::set` keep
//! this race-free across the rayon-style harness threads tokio_test
//! and `cargo test` use.
//!
//! # Cross-platform binary extension
//!
//! On Windows the cargo-emitted binaries carry `.exe`; everywhere else
//! they don't. We probe both forms and pick whichever exists, so the
//! same helper is portable to Windows CI without per-test `cfg!` guards.

#![allow(dead_code)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::{Once, OnceLock};

static BUILD_ONCE: Once = Once::new();
static ECHO_LLM: OnceLock<PathBuf> = OnceLock::new();
static ECHO_TOOL: OnceLock<PathBuf> = OnceLock::new();

/// Build `echo-llm` and `echo-tool` in release mode (idempotent across
/// the test session) and return paths to the resulting binaries.
///
/// The first caller drives `cargo build --release -p echo-llm -p
/// echo-tool` and resolves the binary paths; subsequent callers pick
/// up the cached values without re-spawning cargo.
///
/// Panics if `cargo build` itself fails or the resolved binaries
/// don't actually exist on disk — both indicate a broken local
/// workspace and there's no recovery path inside a test.
pub fn ensure_echo_plugins_built() -> (&'static PathBuf, &'static PathBuf) {
    BUILD_ONCE.call_once(|| {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let status = Command::new(&cargo)
            .args(["build", "--release", "-p", "echo-llm", "-p", "echo-tool"])
            .status()
            .expect("spawning `cargo build` for echo plugins");
        assert!(
            status.success(),
            "`cargo build --release -p echo-llm -p echo-tool` failed (status {status:?})"
        );

        let target_dir = locate_target_dir();
        let release_dir = target_dir.join("release");
        let echo_llm = pick_binary(&release_dir, "echo-llm");
        let echo_tool = pick_binary(&release_dir, "echo-tool");

        ECHO_LLM
            .set(echo_llm)
            .expect("ensure_echo_plugins_built called more than once concurrently");
        ECHO_TOOL
            .set(echo_tool)
            .expect("ensure_echo_plugins_built called more than once concurrently");
    });
    (
        ECHO_LLM
            .get()
            .expect("ECHO_LLM populated by BUILD_ONCE.call_once"),
        ECHO_TOOL
            .get()
            .expect("ECHO_TOOL populated by BUILD_ONCE.call_once"),
    )
}

/// Convenience: return the path to the cached `echo-llm` binary.
pub fn echo_llm_binary() -> &'static PathBuf {
    ensure_echo_plugins_built().0
}

/// Convenience: return the path to the cached `echo-tool` binary.
pub fn echo_tool_binary() -> &'static PathBuf {
    ensure_echo_plugins_built().1
}

/// Locate the workspace `target/` directory by:
///
/// 1. Honoring `$CARGO_TARGET_DIR` if set.
/// 2. Falling back to walking up from `CARGO_MANIFEST_DIR` (which
///    points at `crates/tau-cli/`) until a `target/` directory is
///    found. This handles both the workspace-root layout
///    (`<root>/target/`) and any `--target-dir` overrides cargo
///    may have applied.
fn locate_target_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("CARGO_TARGET_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for ancestor in manifest_dir.ancestors() {
        let candidate = ancestor.join("target");
        if candidate.is_dir() {
            return candidate;
        }
    }
    panic!(
        "could not locate target/ directory; walked up from {} \
         without finding a `target/` sibling. Set CARGO_TARGET_DIR \
         or run `cargo build` first.",
        manifest_dir.display()
    );
}

/// Pick the existing binary file for `bin_name` under `release_dir`,
/// trying both the bare and `.exe`-suffixed forms so the helper is
/// portable to Windows runners.
fn pick_binary(release_dir: &std::path::Path, bin_name: &str) -> PathBuf {
    let bare = release_dir.join(bin_name);
    if bare.exists() {
        return canonicalize(&bare);
    }
    let with_exe = release_dir.join(format!("{bin_name}.exe"));
    if with_exe.exists() {
        return canonicalize(&with_exe);
    }
    panic!(
        "expected built binary at {} (or {}); did `cargo build --release` succeed?",
        bare.display(),
        with_exe.display()
    );
}

/// Resolve to an absolute, symlink-free path. The lockfile that
/// `setup_echo_project` writes carries this string verbatim, and the
/// `tau` subprocess that consumes it runs with `current_dir` set to a
/// per-test tempdir — so a *relative* binary path here resolves
/// against the tempdir at spawn time and fails with `ENOENT`. The
/// failure was previously hidden by `CARGO_TARGET_DIR` not being set
/// in CI (default workspace target dir is absolute) but surfaces on
/// any dev machine running tests with a relative `CARGO_TARGET_DIR`
/// (and intermittently on macOS CI runners depending on directory
/// resolution).
fn canonicalize(p: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|e| panic!("canonicalize {}: {e}", p.display()))
}
