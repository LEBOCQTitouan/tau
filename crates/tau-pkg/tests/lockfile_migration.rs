//! Round-trip migration tests for historical lockfile schema versions.
//!
//! Lockfile schema has walked v1 → v6 across recent sub-projects (most
//! recently v4 → v5 → v6 via PR #64 / PR #102). Each bump is a one-way
//! door for users: they install on tau v1.x, then upgrade to v1.y, then
//! `LockFile::load` must auto-migrate their on-disk lockfile without
//! data loss or panic.
//!
//! The migrations are purely additive — every new field carries
//! `#[serde(default)]` so older lockfiles deserialize cleanly — but
//! "purely additive" is a contract that's easy to break inadvertently
//! by, say, replacing `#[serde(default)]` with a required field, or
//! adding a non-Default-able type. This file pins that contract.
//!
//! Each test:
//!   1. Writes a minimal hand-authored lockfile pinned at an older
//!      `schema_version`.
//!   2. Calls [`LockFile::load`] — must succeed without warnings
//!      printed to stderr (warnings exist for pre-v5 skill packages but
//!      our fixtures are skill-free).
//!   3. Asserts `schema_version` is bumped to [`MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION`].
//!   4. Asserts the package count + first-package fields survive intact.
//!   5. Asserts new-since-the-old-version fields default to `None` /
//!      empty as documented.
//!
//! See `crates/tau-pkg/src/lockfile.rs` (the
//! [`LockFile::load`] migration block) for the implementation under test.

use std::path::Path;

use tau_pkg::lockfile::{LockFile, MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION};

/// Minimal hand-authored v4 lockfile.
///
/// Distinguishing v4 features: NO `[package.skill]` and NO
/// `synthesized_from` on any `[[package]]`. Single package with one
/// installed version. `source` is the `PackageSource` string form per
/// ADR-0005 (no nested tables).
const V4_MINIMAL: &str = r#"
schema_version = 4
generated_by_tau_version = "0.0.0"
generated_at = "2026-04-01T00:00:00Z"

[[package]]
name = "example"
active_version = "1.0.0"
source = "https://example.com/x/y.git"

[[package.versions]]
version = "1.0.0"
resolved_commit = "0000000000000000000000000000000000000000"
sha256 = ""
installed_at = "2026-04-01T00:00:00Z"
"#;

/// Minimal hand-authored v5 lockfile.
///
/// Distinguishing v5 features: `[package.skill]` is optionally present
/// (we include one to exercise the field), `synthesized_from` is NOT.
const V5_MINIMAL: &str = r#"
schema_version = 5
generated_by_tau_version = "0.0.0"
generated_at = "2026-05-13T00:00:00Z"

[[package]]
name = "example-skill"
active_version = "1.0.0"
source = "https://example.com/x/y.git"

[[package.versions]]
version = "1.0.0"
resolved_commit = "1111111111111111111111111111111111111111"
sha256 = ""
installed_at = "2026-05-13T00:00:00Z"

[package.skill]
content_sha256 = "abc123"

[package.skill.frontmatter]
name = "example-skill"
description = "An example skill"
"#;

/// Minimal hand-authored v6 lockfile WITH the v6-only
/// `synthesized_from` field exercised.
const V6_MINIMAL: &str = r#"
schema_version = 6
generated_by_tau_version = "0.0.0"
generated_at = "2026-05-16T00:00:00Z"

[[package]]
name = "anthropic-skill"
active_version = "1.0.0"
source = "https://example.com/x/y.git"
synthesized_from = "anthropic"

[[package.versions]]
version = "1.0.0"
resolved_commit = "2222222222222222222222222222222222222222"
sha256 = ""
installed_at = "2026-05-16T00:00:00Z"

[package.skill]
content_sha256 = "def456"

[package.skill.frontmatter]
name = "anthropic-skill"
description = "Anthropic-format skill"
"#;

/// Write a lockfile body to a temp file and load it via the public
/// `LockFile::load` entry point (the same path that runs on every
/// install).
fn load_from_fixture_body(body: &str) -> LockFile {
    let tmp = tempfile::Builder::new()
        .prefix("tau-lockfile-migration-")
        .tempdir()
        .expect("tempdir");
    let path = tmp.path().join("tau-lock.toml");
    std::fs::write(&path, body).expect("write fixture");
    LockFile::load(&path).expect("LockFile::load must succeed for a valid historical fixture")
}

#[test]
fn v4_minimal_loads_and_migrates_to_current() {
    let lf = load_from_fixture_body(V4_MINIMAL);

    assert_eq!(
        lf.schema_version, MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION,
        "schema_version must be auto-bumped to the current max"
    );
    assert_eq!(lf.packages.len(), 1, "package count must survive migration");

    let pkg = &lf.packages[0];
    assert_eq!(pkg.name.as_str(), "example");
    assert_eq!(pkg.active_version.to_string(), "1.0.0");
    assert_eq!(pkg.installed_versions.len(), 1);

    // v4 → v5 default: `skill = None`.
    assert!(
        pkg.skill.is_none(),
        "v4 lockfile entries must deserialize with skill=None"
    );
    // v5 → v6 default: `synthesized_from = None`.
    assert!(
        pkg.synthesized_from.is_none(),
        "v4 lockfile entries must deserialize with synthesized_from=None"
    );
}

#[test]
fn v5_minimal_loads_and_migrates_to_current() {
    let lf = load_from_fixture_body(V5_MINIMAL);

    assert_eq!(lf.schema_version, MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION);
    assert_eq!(lf.packages.len(), 1);

    let pkg = &lf.packages[0];
    assert_eq!(pkg.name.as_str(), "example-skill");

    // v5 entries CAN have `skill` populated. Verify it survives.
    let skill = pkg
        .skill
        .as_ref()
        .expect("v5 fixture includes [package.skill]; must survive load");
    assert_eq!(skill.content_sha256, "abc123");
    assert_eq!(skill.frontmatter.name, "example-skill");
    assert_eq!(skill.frontmatter.description, "An example skill");

    // v5 → v6 default: `synthesized_from = None`.
    assert!(
        pkg.synthesized_from.is_none(),
        "v5 lockfile entries must deserialize with synthesized_from=None"
    );
}

#[test]
fn v6_minimal_with_synthesized_from_loads() {
    let lf = load_from_fixture_body(V6_MINIMAL);

    assert_eq!(lf.schema_version, MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION);
    let pkg = &lf.packages[0];
    assert!(
        pkg.synthesized_from.is_some(),
        "v6 fixture sets synthesized_from = \"anthropic\"; must survive load"
    );
}

#[test]
fn round_trip_v4_save_after_load_writes_current_schema() {
    let tmp = tempfile::Builder::new()
        .prefix("tau-lockfile-roundtrip-")
        .tempdir()
        .expect("tempdir");
    let path = tmp.path().join("tau-lock.toml");

    // Write a v4 fixture, load it (auto-migrates), save it back, read
    // raw bytes, and confirm the file now records the current schema
    // version. This is the user-visible behaviour: their pre-upgrade
    // lockfile is silently rewritten on the next install.
    std::fs::write(&path, V4_MINIMAL).expect("write v4 fixture");
    let lf = LockFile::load(&path).expect("load v4");
    lf.save(&path).expect("save migrated lockfile");

    let reloaded_text = std::fs::read_to_string(&path).expect("re-read");
    let expected = format!("schema_version = {MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION}");
    assert!(
        reloaded_text.contains(&expected),
        "saved lockfile must record the current schema_version; got: {reloaded_text}"
    );

    // And it must reload cleanly at the new version.
    let lf2 = LockFile::load(&path).expect("reload after save");
    assert_eq!(lf2.schema_version, MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION);
    assert_eq!(lf2.packages.len(), 1);
}

#[test]
fn unknown_future_schema_version_is_rejected() {
    // Sanity check: the migration path is permissive on "older than
    // me" but strict on "newer than me". A lockfile claiming
    // schema_version = 999 must NOT load — otherwise a future tau
    // version's fields would be lost silently on the current binary's
    // re-save.
    let tmp = tempfile::Builder::new()
        .prefix("tau-lockfile-future-")
        .tempdir()
        .expect("tempdir");
    let path = tmp.path().join("tau-lock.toml");
    std::fs::write(
        &path,
        "schema_version = 999\n\
         generated_by_tau_version = \"9.9.9\"\n\
         generated_at = \"2030-01-01T00:00:00Z\"\n",
    )
    .expect("write future fixture");

    let err = LockFile::load(&path).expect_err("schema_version=999 must be rejected");
    let msg = format!("{err}");
    assert!(
        msg.contains("999") || msg.to_lowercase().contains("schema"),
        "rejection error must mention the version or 'schema'; got: {msg}"
    );
}

// Suppress "unused" warning if `Path` doesn't end up referenced
// elsewhere in this file under future edits.
const _: fn(&Path) = |_| ();
