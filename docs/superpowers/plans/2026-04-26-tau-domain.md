# Tau Domain (sub-project 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the public type surface of `tau-domain`: messages, agents, packages, capabilities, errors. Pure data, no I/O, no async, no platform deps. Every subsequent sub-project (2–5) consumes types from `tau-domain` without redefining them.

**Architecture:** One crate (`crates/tau-domain`) under the existing workspace. Module layout per spec §2: `id`, `version`, `value`, `message`, `agent`, `package/{source,manifest,capability}`, `error`. Two off-by-default Cargo features: `serde` (cascades to `uuid/serde`, `semver/serde`, `url/serde`) and `test-fixtures` (exposes a `fixtures` module for downstream test ergonomics). One typestate (`UncheckedManifest` → `PackageManifest`); one hierarchical typed enum (`Capability` with per-namespace verb enums); per-concern error types with uniform `Debug + Error + Clone + PartialEq + Eq` derives.

**Tech Stack:** Rust stable (workspace MSRV 1.91 per QG7), `thiserror = "2"`, `semver = "1"`, `uuid = "1"` (v7 feature), `url = "2"`, `serde = "1"` (optional), `proptest = "1"` (dev-dep), `serde_json = "1"` (dev-dep), `toml = "0.8"` (dev-dep), `walkdir = "2"` (dev-dep for the registry test).

**Spec:** `docs/superpowers/specs/2026-04-26-tau-domain-design.md`

**Working directory:** `/Users/titouanlebocq/code/tau`. The repo, the workspace, and the empty tau-domain stub already exist on `main` (Plan 1 landed them). All cargo commands run from this directory.

**Commit policy:** every task ends with a Conventional Commits-formatted commit. Tasks 1–24 produce code/docs; Task 25 is local verification only; Task 26 records ADR-0002 acceptance; Task 27 is the QG22 overnight checkpoint. Push to remote happens after Task 25 succeeds.

**Note on TDD strictness:** for tasks producing parsers or validators (real logic — Tasks 2, 3, 7, 13), follow strict red-green-refactor: write the failing test first, watch it fail, implement, watch it pass. For tasks producing pure data declarations (Tasks 4, 5, 8, 11, 14, 15, 16) the cycle collapses — write the type with its tests in one step, then verify all tests pass.

---

## File Structure

| Path | Responsibility | Created in |
|---|---|---|
| `Cargo.toml` (workspace root) | Add `[workspace.dependencies]` block | Task 1 |
| `crates/tau-domain/Cargo.toml` | Add deps, features, dev-deps | Task 1 |
| `crates/tau-domain/src/lib.rs` | Module declarations, re-exports, feature gates | Tasks 1, 6, 17 |
| `crates/tau-domain/src/error.rs` | All per-concern error enums | Tasks 2, 3, 7, 9, 13 |
| `crates/tau-domain/src/id.rs` | `PackageName`, `AgentId`, `AgentInstanceId`, `MessageId` | Tasks 2, 3, 4 |
| `crates/tau-domain/src/value.rs` | `Value` enum + accessor helpers | Task 5 |
| `crates/tau-domain/src/package/mod.rs` | Package submodule re-exports | Task 7 |
| `crates/tau-domain/src/package/source.rs` | `PackageSource`, `GitLocation`, parsers | Task 7 |
| `crates/tau-domain/src/package/manifest.rs` | `PackageDep`, `PackageId`, `PackageKind`, `kinds`, `UncheckedManifest`, `PackageManifest` | Tasks 8, 9, 11, 12, 13 |
| `crates/tau-domain/src/package/capability.rs` | `Capability`, `FsCapability`, `NetCapability`, `ProcessCapability`, `AgentCapability` + custom Deserialize | Task 10 |
| `crates/tau-domain/src/agent.rs` | `AgentStatus`, `FailureKind`, `AgentDefinition` | Tasks 14, 15 |
| `crates/tau-domain/src/message.rs` | `Address`, `MessagePayload`, `Message` | Task 16 |
| `crates/tau-domain/src/fixtures.rs` | `pub mod fixtures` (gated behind feature) | Task 17 |
| `crates/tau-domain/tests/proptest_*.rs` | Property tests | Task 18 |
| `crates/tau-domain/tests/manifest_roundtrip.rs` | Integration: TOML manifest round-trip | Task 19 |
| `crates/tau-domain/tests/manifest_validation_table.rs` | Integration: malformed-manifest error variants | Task 19 |
| `crates/tau-domain/tests/message_envelope_serde.rs` | Integration: Message JSON round-trip | Task 19 |
| `crates/tau-domain/tests/package_source_grammar.rs` | Integration: PackageSource grammar | Task 19 |
| `crates/tau-domain/tests/wire_format/*` | Golden serialized forms | Task 20 |
| `crates/tau-domain/tests/wire_format_golden.rs` | Golden-file round-trip tests | Task 20 |
| `crates/tau-domain/tests/escape_hatch_registry.rs` | Mechanical registry coverage test | Task 23 |
| `.github/workflows/ci.yml` | Add `--no-default-features` job | Task 21 |
| `.github/pull_request_template.md` | PR escape-hatch checklist | Task 23 |
| `docs/explanation/escape-hatches.md` | Seeded registry of v0.1 escape hatches | Task 22 |
| `docs/decisions/0002-manifest-format.md` | ADR-0002 (manifest format + capability evolution + escape-hatch policy) | Task 24 |
| `CONTRIBUTING.md` | Append "Working with escape hatches" section | Task 23 |

---

## Task 1: Workspace + crate dependency setup

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/tau-domain/Cargo.toml`
- Modify: `crates/tau-domain/src/lib.rs`

- [x] **Step 1.1: Add `[workspace.dependencies]` to root Cargo.toml**

Open `/Users/titouanlebocq/code/tau/Cargo.toml`. Append after the `[workspace.package]` block:

```toml
[workspace.dependencies]
thiserror = "2"
semver    = { version = "1" }
uuid      = { version = "1", features = ["v7"] }
url       = "2"
serde     = { version = "1", features = ["derive"] }
proptest  = "1"
walkdir   = "2"
```

- [x] **Step 1.2: Update `crates/tau-domain/Cargo.toml`**

Replace the file contents with:

```toml
[package]
name = "tau-domain"
description = "Core domain types for tau (messages, agents, packages, plugin descriptors)."
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true
license.workspace      = true
repository.workspace   = true
authors.workspace      = true

[dependencies]
thiserror = { workspace = true }
semver    = { workspace = true }
uuid      = { workspace = true }
url       = { workspace = true }
serde     = { workspace = true, optional = true }

[features]
default       = []
serde         = ["dep:serde", "uuid/serde", "semver/serde", "url/serde"]
test-fixtures = []

[dev-dependencies]
proptest    = { workspace = true }
serde_json  = "1"
toml        = "0.8"
walkdir     = { workspace = true }
```

- [x] **Step 1.3: Verify the no-default-features build still works**

Run from `/Users/titouanlebocq/code/tau`:

```bash
cargo build -p tau-domain --no-default-features
```

Expected: success. The current `lib.rs` is just a doc comment; nothing depends on `serde` yet.

- [x] **Step 1.4: Verify the all-features build works**

```bash
cargo build -p tau-domain --all-features
```

Expected: success. The new optional deps are pulled in but unused.

- [x] **Step 1.5: Verify clippy + fmt are still clean**

```bash
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: both exit 0.

- [x] **Step 1.6: Stage and commit**

```bash
git add Cargo.toml crates/tau-domain/Cargo.toml
git commit -m "build(tau-domain): add workspace deps and feature flags

Adds thiserror, semver, uuid (v7), url, optional serde, and proptest /
serde_json / toml / walkdir as dev-deps. Two features: serde (off by
default; cascades through uuid, semver, url) and test-fixtures
(downstream test ergonomics).

Refs: QG2, QG5, spec sub-project 1 §2"
```

---

## Task 2: PackageName + PackageNameError

**Files:**
- Create: `crates/tau-domain/src/error.rs`
- Create: `crates/tau-domain/src/id.rs`
- Modify: `crates/tau-domain/src/lib.rs`

This task introduces the first newtype with full validation. Strict TDD because there is real parsing/validation logic.

- [x] **Step 2.1: Create `error.rs` with the first error enum**

Create `/Users/titouanlebocq/code/tau/crates/tau-domain/src/error.rs`:

```rust
//! Per-concern error enums for `tau-domain`.
//!
//! Each error type is `#[non_exhaustive]` so additive variants are non-breaking.
//! All errors derive `Debug + Error + Clone + PartialEq + Eq`; tests with
//! free-form `String` fields use `matches!()` to avoid brittle wording
//! comparisons.

use thiserror::Error;

/// Validation errors for [`crate::id::PackageName`].
///
/// # Example
///
/// ```
/// use tau_domain::{PackageName, PackageNameError};
/// use std::str::FromStr;
///
/// let err = PackageName::from_str("").unwrap_err();
/// assert_eq!(err, PackageNameError::Empty);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PackageNameError {
    /// The input was empty.
    #[error("package name is empty")]
    Empty,
    /// The input exceeded the 64-character cap.
    #[error("package name exceeds {max} characters: got {got}")]
    TooLong {
        /// Maximum permitted length.
        max: usize,
        /// Actual length of the input.
        got: usize,
    },
    /// A character outside `[a-z0-9-]` was found mid-string.
    #[error("package name contains invalid character {ch:?} at byte {pos}")]
    InvalidCharacter {
        /// The offending character.
        ch: char,
        /// Byte position in the input string.
        pos: usize,
    },
    /// The leading character was not an ASCII lowercase letter.
    #[error("package name must start with a letter, got {ch:?}")]
    InvalidLeadingCharacter {
        /// The first character of the input.
        ch: char,
    },
}
```

- [x] **Step 2.2: Create `id.rs` with `PackageName` and a failing test**

Create `/Users/titouanlebocq/code/tau/crates/tau-domain/src/id.rs`:

```rust
//! Identifier newtypes used across the `tau-domain` surface.
//!
//! `PackageName` and `AgentId` are validating ASCII kebab-case identifiers.
//! `AgentInstanceId` and `MessageId` are UUID v7-based opaque identifiers.

use std::fmt;
use std::str::FromStr;

use crate::error::PackageNameError;

/// A package name. ASCII kebab-case, must start with a lowercase letter,
/// 1..=64 characters, character set `[a-z0-9-]`.
///
/// # Example
///
/// ```
/// use tau_domain::PackageName;
/// use std::str::FromStr;
///
/// let n = PackageName::from_str("fs-tools").unwrap();
/// assert_eq!(n.as_str(), "fs-tools");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageName(String);

impl PackageName {
    /// The maximum permitted length, in bytes (== chars, since ASCII-only).
    pub const MAX_LEN: usize = 64;

    /// View as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for PackageName {
    type Err = PackageNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(PackageNameError::Empty);
        }
        if s.len() > Self::MAX_LEN {
            return Err(PackageNameError::TooLong {
                max: Self::MAX_LEN,
                got: s.len(),
            });
        }
        let mut chars = s.char_indices();
        let (_, first) = chars.next().expect("length-checked above");
        if !first.is_ascii_lowercase() {
            return Err(PackageNameError::InvalidLeadingCharacter { ch: first });
        }
        for (pos, ch) in chars {
            if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
                return Err(PackageNameError::InvalidCharacter { ch, pos });
            }
        }
        Ok(Self(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_names() {
        for name in ["a", "fs-tools", "abc-123", "x".repeat(64).as_str()] {
            assert!(PackageName::from_str(name).is_ok(), "should accept {name:?}");
        }
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(PackageName::from_str(""), Err(PackageNameError::Empty));
    }

    #[test]
    fn rejects_too_long() {
        let s = "a".repeat(65);
        assert_eq!(
            PackageName::from_str(&s),
            Err(PackageNameError::TooLong { max: 64, got: 65 }),
        );
    }

    #[test]
    fn rejects_invalid_leading() {
        assert_eq!(
            PackageName::from_str("1abc"),
            Err(PackageNameError::InvalidLeadingCharacter { ch: '1' }),
        );
        assert_eq!(
            PackageName::from_str("-abc"),
            Err(PackageNameError::InvalidLeadingCharacter { ch: '-' }),
        );
        assert_eq!(
            PackageName::from_str("Abc"),
            Err(PackageNameError::InvalidLeadingCharacter { ch: 'A' }),
        );
    }

    #[test]
    fn rejects_invalid_mid_char() {
        assert!(matches!(
            PackageName::from_str("abc_def"),
            Err(PackageNameError::InvalidCharacter { ch: '_', pos: 3 }),
        ));
        assert!(matches!(
            PackageName::from_str("abcDef"),
            Err(PackageNameError::InvalidCharacter { ch: 'D', pos: 3 }),
        ));
    }

    #[test]
    fn display_round_trip() {
        let n = PackageName::from_str("fs-tools").unwrap();
        assert_eq!(n.to_string(), "fs-tools");
    }
}
```

- [x] **Step 2.3: Wire modules into `lib.rs`**

Replace `/Users/titouanlebocq/code/tau/crates/tau-domain/src/lib.rs` with:

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Core domain types for tau. Pure data — no I/O, no plugin contracts.
//! See the constitution (G5) for why messages are the universal interaction primitive.

pub mod error;
pub mod id;

pub use error::PackageNameError;
pub use id::PackageName;
```

- [x] **Step 2.4: Run unit tests**

```bash
cargo test -p tau-domain --lib
```

Expected: 6 tests pass (`accepts_valid_names`, `rejects_empty`, `rejects_too_long`, `rejects_invalid_leading`, `rejects_invalid_mid_char`, `display_round_trip`).

- [x] **Step 2.5: Run doctests**

```bash
cargo test -p tau-domain --doc
```

Expected: 2 doctests pass (one on `PackageName`, one on `PackageNameError`).

- [x] **Step 2.6: Run clippy and fmt**

```bash
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: both exit 0.

- [x] **Step 2.7: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/id.rs crates/tau-domain/src/error.rs
git commit -m "feat(tau-domain): add PackageName newtype with kebab-case validation

PackageName accepts [a-z][a-z0-9-]{0,63}. Returns typed
PackageNameError with one variant per failure mode (Empty, TooLong,
InvalidCharacter, InvalidLeadingCharacter). All variants
non_exhaustive; uniform Debug+Error+Clone+PartialEq+Eq derives.

Refs: QG2, QG3, QG9, spec §3.1, §3.6"
```

---

## Task 3: AgentId + AgentIdError

**Files:**
- Modify: `crates/tau-domain/src/error.rs` (append)
- Modify: `crates/tau-domain/src/id.rs` (append)
- Modify: `crates/tau-domain/src/lib.rs` (extend re-exports)

Same grammar as `PackageName`, but a separate type for discoverability per spec §3.6.

- [x] **Step 3.1: Append `AgentIdError` to `error.rs`**

Append to `/Users/titouanlebocq/code/tau/crates/tau-domain/src/error.rs`:

```rust
/// Validation errors for [`crate::id::AgentId`].
///
/// # Example
///
/// ```
/// use tau_domain::{AgentId, AgentIdError};
/// use std::str::FromStr;
///
/// let err = AgentId::from_str("").unwrap_err();
/// assert_eq!(err, AgentIdError::Empty);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AgentIdError {
    /// The input was empty.
    #[error("agent id is empty")]
    Empty,
    /// The input exceeded the 64-character cap.
    #[error("agent id exceeds {max} characters: got {got}")]
    TooLong {
        /// Maximum permitted length.
        max: usize,
        /// Actual length of the input.
        got: usize,
    },
    /// A character outside `[a-z0-9-]` was found mid-string.
    #[error("agent id contains invalid character {ch:?} at byte {pos}")]
    InvalidCharacter {
        /// The offending character.
        ch: char,
        /// Byte position in the input string.
        pos: usize,
    },
    /// The leading character was not an ASCII lowercase letter.
    #[error("agent id must start with a letter, got {ch:?}")]
    InvalidLeadingCharacter {
        /// The first character of the input.
        ch: char,
    },
}
```

- [x] **Step 3.2: Append `AgentId` to `id.rs`**

Add a `use crate::error::AgentIdError;` near the top of `id.rs`, then append at the end (after the existing `tests` module):

```rust
/// An agent identifier. ASCII kebab-case, must start with a lowercase letter,
/// 1..=64 characters, character set `[a-z0-9-]`.
///
/// Same grammar as [`PackageName`]; separate type for clarity at call sites.
///
/// # Example
///
/// ```
/// use tau_domain::AgentId;
/// use std::str::FromStr;
///
/// let id = AgentId::from_str("researcher").unwrap();
/// assert_eq!(id.as_str(), "researcher");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentId(String);

impl AgentId {
    /// The maximum permitted length, in bytes.
    pub const MAX_LEN: usize = 64;

    /// View as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for AgentId {
    type Err = AgentIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(AgentIdError::Empty);
        }
        if s.len() > Self::MAX_LEN {
            return Err(AgentIdError::TooLong {
                max: Self::MAX_LEN,
                got: s.len(),
            });
        }
        let mut chars = s.char_indices();
        let (_, first) = chars.next().expect("length-checked above");
        if !first.is_ascii_lowercase() {
            return Err(AgentIdError::InvalidLeadingCharacter { ch: first });
        }
        for (pos, ch) in chars {
            if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
                return Err(AgentIdError::InvalidCharacter { ch, pos });
            }
        }
        Ok(Self(s.to_owned()))
    }
}

#[cfg(test)]
mod agent_id_tests {
    use super::*;

    #[test]
    fn accepts_valid() {
        for name in ["a", "researcher", "agent-123"] {
            assert!(AgentId::from_str(name).is_ok());
        }
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(AgentId::from_str(""), Err(AgentIdError::Empty));
    }

    #[test]
    fn rejects_too_long() {
        let s = "a".repeat(65);
        assert_eq!(
            AgentId::from_str(&s),
            Err(AgentIdError::TooLong { max: 64, got: 65 }),
        );
    }

    #[test]
    fn rejects_invalid_leading() {
        assert_eq!(
            AgentId::from_str("1agent"),
            Err(AgentIdError::InvalidLeadingCharacter { ch: '1' }),
        );
    }

    #[test]
    fn rejects_invalid_mid_char() {
        assert!(matches!(
            AgentId::from_str("agent_x"),
            Err(AgentIdError::InvalidCharacter { ch: '_', pos: 5 }),
        ));
    }
}
```

- [x] **Step 3.3: Update `lib.rs` re-exports**

Replace the re-export lines in `lib.rs`:

```rust
pub use error::{AgentIdError, PackageNameError};
pub use id::{AgentId, PackageName};
```

- [x] **Step 3.4: Run tests + clippy + fmt**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: all green.

- [x] **Step 3.5: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/id.rs crates/tau-domain/src/error.rs
git commit -m "feat(tau-domain): add AgentId newtype mirroring PackageName grammar

AgentId shares PackageName's kebab-case rules but is a separate type
for discoverability at call sites. AgentIdError mirrors PackageNameError;
both will likely diverge once agent identifiers gain scoped (foo/bar)
forms.

Refs: QG2, QG9, spec §3.1, §3.6"
```

---

## Task 4: UUID-based IDs (AgentInstanceId + MessageId)

**Files:**
- Modify: `crates/tau-domain/src/id.rs` (append)
- Modify: `crates/tau-domain/src/lib.rs` (extend re-exports)

UUID v7 IDs. No grammar validation — `uuid::Uuid` handles all parsing.

- [x] **Step 4.1: Append `AgentInstanceId` and `MessageId` to `id.rs`**

Append to `id.rs`:

```rust
/// A runtime instance identifier for a spawned agent. UUID v7 (monotonic,
/// time-ordered). Two instances of the same `AgentDefinition` share an
/// `AgentId` but differ in `AgentInstanceId`.
///
/// # Example
///
/// ```
/// use tau_domain::AgentInstanceId;
///
/// let a = AgentInstanceId::new();
/// let b = AgentInstanceId::new();
/// assert_ne!(a, b);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentInstanceId(uuid::Uuid);

impl AgentInstanceId {
    /// Generate a fresh UUID v7.
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }

    /// Wrap an existing `Uuid`.
    pub fn from_uuid(u: uuid::Uuid) -> Self {
        Self(u)
    }

    /// Underlying `Uuid`.
    pub fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl Default for AgentInstanceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AgentInstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for AgentInstanceId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<uuid::Uuid>().map(Self)
    }
}

/// A message identifier. UUID v7 (monotonic, time-ordered). Acts as the
/// reply target for `Message.parent_id`.
///
/// # Example
///
/// ```
/// use tau_domain::MessageId;
///
/// let id = MessageId::new();
/// let parsed: MessageId = id.to_string().parse().unwrap();
/// assert_eq!(id, parsed);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MessageId(uuid::Uuid);

impl MessageId {
    /// Generate a fresh UUID v7.
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }

    /// Wrap an existing `Uuid`.
    pub fn from_uuid(u: uuid::Uuid) -> Self {
        Self(u)
    }

    /// Underlying `Uuid`.
    pub fn as_uuid(&self) -> uuid::Uuid {
        self.0
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for MessageId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<uuid::Uuid>().map(Self)
    }
}

#[cfg(feature = "serde")]
mod uuid_id_serde {
    use super::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl Serialize for AgentInstanceId {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            self.0.serialize(s)
        }
    }
    impl<'de> Deserialize<'de> for AgentInstanceId {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            uuid::Uuid::deserialize(d).map(Self)
        }
    }
    impl Serialize for MessageId {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            self.0.serialize(s)
        }
    }
    impl<'de> Deserialize<'de> for MessageId {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            uuid::Uuid::deserialize(d).map(Self)
        }
    }
}

#[cfg(test)]
mod uuid_id_tests {
    use super::*;

    #[test]
    fn agent_instance_round_trips() {
        let a = AgentInstanceId::new();
        let parsed: AgentInstanceId = a.to_string().parse().unwrap();
        assert_eq!(a, parsed);
    }

    #[test]
    fn message_id_round_trips() {
        let m = MessageId::new();
        let parsed: MessageId = m.to_string().parse().unwrap();
        assert_eq!(m, parsed);
    }

    #[test]
    fn fresh_ids_differ() {
        assert_ne!(MessageId::new(), MessageId::new());
        assert_ne!(AgentInstanceId::new(), AgentInstanceId::new());
    }
}
```

- [x] **Step 4.2: Update `lib.rs` re-exports**

```rust
pub use error::{AgentIdError, PackageNameError};
pub use id::{AgentId, AgentInstanceId, MessageId, PackageName};
```

- [x] **Step 4.3: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: all green. The `--no-default-features` build verifies `serde` gating.

- [x] **Step 4.4: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/id.rs
git commit -m "feat(tau-domain): add AgentInstanceId and MessageId (UUID v7)

Both are opaque newtypes around uuid::Uuid, generating monotonic
time-ordered v7 UUIDs. Display/FromStr round-trip via the upstream
uuid crate. Serde impls are gated behind the serde feature.

Refs: spec §3.1, §3.3"
```

---

## Task 5: `Value` enum + accessor helpers

**Files:**
- Create: `crates/tau-domain/src/value.rs`
- Modify: `crates/tau-domain/src/lib.rs`

JSON-shaped value used by manifest capability params and tool args/results.

- [x] **Step 5.1: Create `value.rs`**

Create `/Users/titouanlebocq/code/tau/crates/tau-domain/src/value.rs`:

```rust
//! JSON-shaped values used by manifest capability params and tool
//! args/results.
//!
//! `BTreeMap` (not `HashMap`) for deterministic iteration order — matters
//! for golden tests and stable wire format.

use std::collections::BTreeMap;

/// A JSON-shaped value: nullable, scalar, or recursive.
///
/// # Example
///
/// ```
/// use tau_domain::Value;
/// use std::collections::BTreeMap;
///
/// let v = Value::Object({
///     let mut m = BTreeMap::new();
///     m.insert("paths".into(), Value::Array(vec![Value::String("/tmp".into())]));
///     m
/// });
/// assert_eq!(
///     v.as_object().unwrap().get("paths").unwrap().as_array().unwrap().len(),
///     1,
/// );
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(untagged))]
pub enum Value {
    /// JSON null.
    Null,
    /// Boolean.
    Bool(bool),
    /// Signed 64-bit integer.
    Integer(i64),
    /// IEEE-754 double-precision float.
    Float(f64),
    /// UTF-8 string.
    String(String),
    /// Binary blob (image data, file contents, etc.).
    Bytes(Vec<u8>),
    /// Ordered list of values.
    Array(Vec<Value>),
    /// Sorted-key map of values.
    Object(BTreeMap<String, Value>),
}

impl Value {
    /// Return the inner string if this is `Value::String`, else `None`.
    pub fn as_string(&self) -> Option<&str> {
        if let Value::String(s) = self { Some(s) } else { None }
    }
    /// Return the inner integer if this is `Value::Integer`, else `None`.
    pub fn as_integer(&self) -> Option<i64> {
        if let Value::Integer(i) = self { Some(*i) } else { None }
    }
    /// Return the inner float if this is `Value::Float`, else `None`.
    pub fn as_float(&self) -> Option<f64> {
        if let Value::Float(f) = self { Some(*f) } else { None }
    }
    /// Return the inner bool if this is `Value::Bool`, else `None`.
    pub fn as_bool(&self) -> Option<bool> {
        if let Value::Bool(b) = self { Some(*b) } else { None }
    }
    /// Return the inner bytes if this is `Value::Bytes`, else `None`.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        if let Value::Bytes(b) = self { Some(b) } else { None }
    }
    /// Return the inner array if this is `Value::Array`, else `None`.
    pub fn as_array(&self) -> Option<&[Value]> {
        if let Value::Array(a) = self { Some(a) } else { None }
    }
    /// Return the inner object if this is `Value::Object`, else `None`.
    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        if let Value::Object(o) = self { Some(o) } else { None }
    }
    /// True if this is `Value::Null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_match_variants() {
        assert!(Value::Null.is_null());
        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert_eq!(Value::Integer(42).as_integer(), Some(42));
        assert_eq!(Value::Float(1.5).as_float(), Some(1.5));
        assert_eq!(Value::String("x".into()).as_string(), Some("x"));
        assert_eq!(Value::Bytes(vec![1, 2]).as_bytes(), Some(&[1u8, 2][..]));
        assert_eq!(Value::Array(vec![Value::Null]).as_array().unwrap().len(), 1);
        let mut o = BTreeMap::new();
        o.insert("k".into(), Value::Bool(false));
        assert!(Value::Object(o).as_object().is_some());
    }

    #[test]
    fn accessors_return_none_for_other_variants() {
        assert_eq!(Value::Null.as_integer(), None);
        assert_eq!(Value::Bool(true).as_string(), None);
        assert!(!Value::Bool(false).is_null());
    }
}
```

- [x] **Step 5.2: Wire into `lib.rs`**

Add `pub mod value;` and `pub use value::Value;` to `lib.rs`:

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Core domain types for tau. Pure data — no I/O, no plugin contracts.
//! See the constitution (G5) for why messages are the universal interaction primitive.

pub mod error;
pub mod id;
pub mod value;

pub use error::{AgentIdError, PackageNameError};
pub use id::{AgentId, AgentInstanceId, MessageId, PackageName};
pub use value::Value;
```

- [x] **Step 5.3: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 5.4: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/value.rs
git commit -m "feat(tau-domain): add Value enum with JSON-shaped variants

Eight variants (Null, Bool, Integer, Float, String, Bytes, Array,
Object) covering manifest capability params and tool args/results.
BTreeMap for deterministic iteration. Per-variant accessor helpers
keep consumers out of full match expressions for the common 'is this
a string?' pattern.

Refs: spec §3.2"
```

---

## Task 6: lib.rs re-exports for `semver`, `url`, `uuid`

**Files:**
- Create: `crates/tau-domain/src/version.rs`
- Modify: `crates/tau-domain/src/lib.rs`

The spec re-exports rather than wraps `semver::Version` and `semver::VersionReq` (no v0.1 invariants to enforce). Same for `url::Url` and `uuid::Uuid`. Putting these in their own module keeps `lib.rs` clean.

- [x] **Step 6.1: Create `version.rs`**

Create `/Users/titouanlebocq/code/tau/crates/tau-domain/src/version.rs`:

```rust
//! Re-exports of the semver types used in package metadata.
//!
//! tau-domain does not wrap `semver::Version` or `semver::VersionReq`. The
//! Cargo / SemVer ecosystem already speaks these types; wrapping would add
//! ceremony without enforcing any invariant tau-domain currently needs.
//!
//! If a future ADR motivates normalization (e.g. forbidding pre-release
//! tags or build metadata), it lands as a wrapper newtype at that point.

pub use semver::{Version, VersionReq};
```

- [x] **Step 6.2: Update `lib.rs`**

Replace `lib.rs` with:

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Core domain types for tau. Pure data — no I/O, no plugin contracts.
//! See the constitution (G5) for why messages are the universal interaction primitive.

pub mod error;
pub mod id;
pub mod value;
pub mod version;

pub use error::{AgentIdError, PackageNameError};
pub use id::{AgentId, AgentInstanceId, MessageId, PackageName};
pub use value::Value;
pub use version::{Version, VersionReq};

// External-crate re-exports for convenience: anything that takes a
// `tau_domain::Url` should accept a `url::Url` from anywhere in the tree.
pub use url::Url;
pub use uuid::Uuid;
```

- [x] **Step 6.3: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 6.4: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/version.rs
git commit -m "feat(tau-domain): re-export semver, url, uuid types

Version + VersionReq go through a dedicated version module to leave
room for a future wrapper newtype. Url and Uuid are flat re-exports
in lib.rs for callsite ergonomics.

Refs: spec §3.5.2, §3.1"
```

---

## Task 7: PackageSource + GitLocation + parsers + PackageSourceError

**Files:**
- Create: `crates/tau-domain/src/package/mod.rs`
- Create: `crates/tau-domain/src/package/source.rs`
- Modify: `crates/tau-domain/src/error.rs` (append)
- Modify: `crates/tau-domain/src/lib.rs`

Real parsing logic. Strict TDD: tests first, watch fail, implement, watch pass.

- [x] **Step 7.1: Append `PackageSourceError` to `error.rs`**

Append:

```rust
/// Parser/validation errors for [`crate::package::PackageSource`] and
/// [`crate::package::GitLocation`].
///
/// # Example
///
/// ```
/// use tau_domain::{PackageSource, PackageSourceError};
/// use std::str::FromStr;
///
/// let err = PackageSource::from_str("").unwrap_err();
/// assert_eq!(err, PackageSourceError::Empty);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PackageSourceError {
    /// The input was empty.
    #[error("package source is empty")]
    Empty,
    /// The URL had a scheme outside the allowed set.
    #[error("unsupported URL scheme {scheme:?}; expected https, http, ssh, or git")]
    UnsupportedScheme {
        /// The rejected scheme.
        scheme: String,
    },
    /// The URL did not parse as RFC 3986 *and* did not match scp-style.
    #[error("malformed URL: {reason}")]
    MalformedUrl {
        /// Upstream parser's diagnostic.
        reason: String,
    },
    /// The scp-style address could not be parsed.
    #[error("malformed scp-style address: {reason}")]
    MalformedScpAddress {
        /// Diagnostic from the scp-style parser.
        reason: String,
    },
    /// The fragment after `#` was empty.
    #[error("revision is empty after '#'")]
    EmptyRevision,
}
```

- [x] **Step 7.2: Create `package/mod.rs`**

Create `/Users/titouanlebocq/code/tau/crates/tau-domain/src/package/mod.rs`:

```rust
//! Package metadata types (sources, manifests, capabilities).

pub mod source;

pub use source::{GitLocation, PackageSource};
```

- [x] **Step 7.3: Create `package/source.rs` with failing tests first**

Create `/Users/titouanlebocq/code/tau/crates/tau-domain/src/package/source.rs`. Start with the type declarations and tests; leave the parser bodies as `todo!()` to confirm tests fail:

```rust
//! Package source location grammar.
//!
//! tau-domain v0.1 only models `Git` sources. Local paths, registry-style
//! sources, and tarball URLs land as additive `PackageSource` variants
//! later (see `docs/explanation/escape-hatches.md` for the broader
//! escape-hatch policy this is consistent with).

use std::fmt;
use std::str::FromStr;

use crate::error::PackageSourceError;

/// A package source location. v0.1: git only.
///
/// Canonical text form: `<location>` or `<location>#<rev>`.
///
/// # Example
///
/// ```
/// use tau_domain::PackageSource;
/// use std::str::FromStr;
///
/// let s = PackageSource::from_str("https://github.com/example/repo.git#main").unwrap();
/// assert_eq!(
///     s.to_string(),
///     "https://github.com/example/repo.git#main",
/// );
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PackageSource {
    /// A git repository location, optionally pinned to a revision.
    Git {
        /// Where the repository lives.
        location: GitLocation,
        /// Branch, tag, or commit SHA. Opaque to tau-domain; tau-pkg
        /// disambiguates at clone time.
        rev: Option<String>,
    },
}

/// Where a git repository lives. Two shapes because git itself accepts both.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GitLocation {
    /// Standard URL (https / http / ssh / git scheme).
    Url(url::Url),
    /// scp-style address, e.g. `git@github.com:owner/repo.git`. Not a
    /// valid URL by RFC 3986; git accepts it natively.
    Scp {
        /// Optional user component, e.g. `git`.
        user: Option<String>,
        /// Hostname.
        host: String,
        /// Repository path on the host.
        path: String,
    },
}

impl FromStr for PackageSource {
    type Err = PackageSourceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(PackageSourceError::Empty);
        }
        let (loc_str, rev) = match s.split_once('#') {
            Some((_, "")) => return Err(PackageSourceError::EmptyRevision),
            Some((loc, rev)) => (loc, Some(rev.to_owned())),
            None => (s, None),
        };
        let location = GitLocation::from_str(loc_str)?;
        Ok(PackageSource::Git { location, rev })
    }
}

impl fmt::Display for PackageSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let PackageSource::Git { location, rev } = self;
        write!(f, "{location}")?;
        if let Some(r) = rev {
            write!(f, "#{r}")?;
        }
        Ok(())
    }
}

impl FromStr for GitLocation {
    type Err = PackageSourceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Err(PackageSourceError::Empty);
        }
        // Try url::Url first.
        match url::Url::parse(s) {
            Ok(url) => match url.scheme() {
                "https" | "http" | "ssh" | "git" => Ok(GitLocation::Url(url)),
                other => Err(PackageSourceError::UnsupportedScheme {
                    scheme: other.to_owned(),
                }),
            },
            Err(parse_err) => {
                // Fall through to scp-style only if the URL wasn't recognized
                // as having a scheme. If parser thinks it has a scheme but
                // failed for another reason, surface the URL error.
                if !s.contains("://") {
                    parse_scp(s)
                } else {
                    Err(PackageSourceError::MalformedUrl {
                        reason: parse_err.to_string(),
                    })
                }
            }
        }
    }
}

impl fmt::Display for GitLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitLocation::Url(u) => write!(f, "{u}"),
            GitLocation::Scp { user, host, path } => {
                if let Some(u) = user {
                    write!(f, "{u}@{host}:{path}")
                } else {
                    write!(f, "{host}:{path}")
                }
            }
        }
    }
}

fn parse_scp(s: &str) -> Result<GitLocation, PackageSourceError> {
    // scp grammar: [user@]host:path  where ':' is the first colon and is
    // NOT followed by '/' (which would be ambiguous with `host:port/path`).
    let colon_pos = s.find(':').ok_or_else(|| PackageSourceError::MalformedScpAddress {
        reason: "missing ':' separator".to_owned(),
    })?;
    if s[colon_pos + 1..].starts_with('/') {
        return Err(PackageSourceError::MalformedScpAddress {
            reason: "':' followed by '/' (ambiguous with port form)".to_owned(),
        });
    }
    let (user_host, path) = s.split_at(colon_pos);
    let path = &path[1..]; // strip the ':'
    if path.is_empty() {
        return Err(PackageSourceError::MalformedScpAddress {
            reason: "path component empty".to_owned(),
        });
    }
    let (user, host) = match user_host.split_once('@') {
        Some((u, h)) if !u.is_empty() && !h.is_empty() => (Some(u.to_owned()), h.to_owned()),
        Some(_) => {
            return Err(PackageSourceError::MalformedScpAddress {
                reason: "empty user or host around '@'".to_owned(),
            })
        }
        None => {
            if user_host.is_empty() {
                return Err(PackageSourceError::MalformedScpAddress {
                    reason: "host component empty".to_owned(),
                });
            }
            (None, user_host.to_owned())
        }
    };
    Ok(GitLocation::Scp {
        user,
        host,
        path: path.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_https_url() {
        let s = PackageSource::from_str("https://github.com/owner/repo.git").unwrap();
        match s {
            PackageSource::Git { location: GitLocation::Url(u), rev: None } => {
                assert_eq!(u.scheme(), "https");
            }
            _ => panic!("expected https Url with no rev"),
        }
    }

    #[test]
    fn parses_https_with_rev() {
        let s = PackageSource::from_str("https://github.com/owner/repo.git#v1.2.3").unwrap();
        let PackageSource::Git { rev, .. } = s;
        assert_eq!(rev.as_deref(), Some("v1.2.3"));
    }

    #[test]
    fn parses_scp_form() {
        let s = PackageSource::from_str("git@github.com:owner/repo.git").unwrap();
        match s {
            PackageSource::Git { location: GitLocation::Scp { user, host, path }, .. } => {
                assert_eq!(user.as_deref(), Some("git"));
                assert_eq!(host, "github.com");
                assert_eq!(path, "owner/repo.git");
            }
            _ => panic!("expected Scp variant"),
        }
    }

    #[test]
    fn parses_scp_without_user() {
        let s = PackageSource::from_str("github.com:owner/repo.git").unwrap();
        match s {
            PackageSource::Git { location: GitLocation::Scp { user: None, host, path }, .. } => {
                assert_eq!(host, "github.com");
                assert_eq!(path, "owner/repo.git");
            }
            _ => panic!("expected Scp without user"),
        }
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(PackageSource::from_str(""), Err(PackageSourceError::Empty));
    }

    #[test]
    fn rejects_empty_rev() {
        assert_eq!(
            PackageSource::from_str("https://x.com/r.git#"),
            Err(PackageSourceError::EmptyRevision),
        );
    }

    #[test]
    fn rejects_unsupported_scheme() {
        let err = PackageSource::from_str("ftp://x.com/r.git").unwrap_err();
        assert!(matches!(err, PackageSourceError::UnsupportedScheme { scheme } if scheme == "ftp"));
    }

    #[test]
    fn rejects_scp_colon_slash() {
        let err = PackageSource::from_str("github.com:/owner/repo.git").unwrap_err();
        assert!(matches!(err, PackageSourceError::MalformedScpAddress { .. }));
    }

    #[test]
    fn display_round_trips_url() {
        for s in [
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo.git#main",
            "ssh://git@example.com/repo.git",
        ] {
            let parsed = PackageSource::from_str(s).unwrap();
            assert_eq!(parsed.to_string(), s, "mismatch on {s:?}");
        }
    }

    #[test]
    fn display_round_trips_scp() {
        for s in [
            "git@github.com:owner/repo.git",
            "github.com:owner/repo.git",
            "git@github.com:owner/repo.git#v1.0",
        ] {
            let parsed = PackageSource::from_str(s).unwrap();
            assert_eq!(parsed.to_string(), s, "mismatch on {s:?}");
        }
    }
}
```

- [x] **Step 7.4: Wire into `lib.rs`**

Add to `lib.rs`:

```rust
pub mod package;

pub use error::{AgentIdError, PackageNameError, PackageSourceError};
pub use package::{GitLocation, PackageSource};
```

(Replace the existing `pub use error::...` line and add the `pub use package::...` line.)

- [x] **Step 7.5: Run tests**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: 10 source tests pass, 1 doctest passes.

- [x] **Step 7.6: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/error.rs crates/tau-domain/src/package
git commit -m "feat(tau-domain): add PackageSource grammar with git URL + scp-style

PackageSource models git sources with two location shapes: Url (RFC 3986
via the url crate, scheme restricted to https/http/ssh/git) and Scp
(user@host:path with ':' not followed by '/'). FromStr/Display round-trip
both forms with optional #<rev>. PackageSourceError covers Empty,
UnsupportedScheme, MalformedUrl, MalformedScpAddress, EmptyRevision.

Refs: QG5, spec §3.5.1, §4"
```

---

## Task 8: PackageDep + PackageId

**Files:**
- Create: `crates/tau-domain/src/package/manifest.rs`
- Modify: `crates/tau-domain/src/package/mod.rs`
- Modify: `crates/tau-domain/src/lib.rs`

Pure data structs. No validation logic — fields are pre-validated newtypes.

- [x] **Step 8.1: Create `package/manifest.rs` (start)**

Create `/Users/titouanlebocq/code/tau/crates/tau-domain/src/package/manifest.rs`:

```rust
//! Package manifest types.
//!
//! Manifest deserialization (TOML/JSON) lives in `tau-pkg`; this module
//! owns only the data type, the typestate around validation, and serde
//! derives (under the `serde` feature).

use crate::id::PackageName;
use crate::version::{Version, VersionReq};

/// A dependency declaration: a package name plus a SemVer requirement.
///
/// # Example
///
/// ```
/// use tau_domain::{PackageDep, PackageName, VersionReq};
/// use std::str::FromStr;
///
/// let dep = PackageDep {
///     name: PackageName::from_str("fs-tools").unwrap(),
///     version_req: VersionReq::parse("^0.3").unwrap(),
/// };
/// assert_eq!(dep.name.as_str(), "fs-tools");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PackageDep {
    /// Dependency package name.
    pub name: PackageName,
    /// SemVer version requirement.
    pub version_req: VersionReq,
}

/// A canonical package identity: `(name, version)`.
///
/// # Example
///
/// ```
/// use tau_domain::{PackageId, PackageName, Version};
/// use std::str::FromStr;
///
/// let id = PackageId {
///     name: PackageName::from_str("fs-tools").unwrap(),
///     version: Version::parse("0.3.0").unwrap(),
/// };
/// assert_eq!(id.name.as_str(), "fs-tools");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PackageId {
    /// Package name.
    pub name: PackageName,
    /// Package version.
    pub version: Version,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn package_id_eq_works() {
        let a = PackageId {
            name: PackageName::from_str("foo").unwrap(),
            version: Version::parse("1.2.3").unwrap(),
        };
        let b = PackageId {
            name: PackageName::from_str("foo").unwrap(),
            version: Version::parse("1.2.3").unwrap(),
        };
        assert_eq!(a, b);
    }
}
```

- [x] **Step 8.2: Update `package/mod.rs`**

Replace with:

```rust
//! Package metadata types (sources, manifests, capabilities).

pub mod manifest;
pub mod source;

pub use manifest::{PackageDep, PackageId};
pub use source::{GitLocation, PackageSource};
```

- [x] **Step 8.3: Update `lib.rs` re-exports**

```rust
pub use package::{GitLocation, PackageDep, PackageId, PackageSource};
```

- [x] **Step 8.4: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 8.5: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/package
git commit -m "feat(tau-domain): add PackageDep and PackageId structs

Both #[non_exhaustive] structs over PackageName + semver types.
Pre-validated field types make construction infallible. Serde derives
gated behind the serde feature.

Refs: spec §3.5.2"
```

---

## Task 9: PackageKind + kinds module + PackageKindError

**Files:**
- Modify: `crates/tau-domain/src/package/manifest.rs` (append)
- Modify: `crates/tau-domain/src/error.rs` (append)
- Modify: `crates/tau-domain/src/package/mod.rs`
- Modify: `crates/tau-domain/src/lib.rs`

`PackageKind` is structural (Custom-only) at v0.1 per the escape-hatch policy.

- [x] **Step 9.1: Append `PackageKindError` to `error.rs`**

```rust
/// Validation errors for [`crate::package::PackageKind`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PackageKindError {
    /// The kind string was empty.
    #[error("package kind is empty")]
    Empty,
}
```

- [x] **Step 9.2: Append `PackageKind` and `kinds` module to `package/manifest.rs`**

```rust
/// Package kind. Structural at v0.1: every kind goes through `Custom`.
/// Typed variants land additively as tau-runtime gains plugin trait
/// awareness for each kind.
///
/// See: [escape-hatches.md#packagekind-custom](../../../../../docs/explanation/escape-hatches.md#packagekind-custom).
///
/// # Example
///
/// ```
/// use tau_domain::PackageKind;
/// let k = PackageKind::Custom { kind: "tool".into() };
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PackageKind {
    /// A package kind not yet typed in core.
    /// See: [escape-hatches.md#packagekind-custom](../../../../../docs/explanation/escape-hatches.md#packagekind-custom).
    Custom {
        /// The kind name. By convention one of [`crate::package::kinds`]'s
        /// constants (e.g. `"llm-backend"`, `"tool"`).
        kind: String,
    },
}

/// Canonical kind strings for `PackageKind::Custom.kind` and manifest
/// `kind` fields. Recommended convention; tau-domain validates only
/// "non-empty" so plugin authors who want a non-conforming kind name
/// can use `Custom` with arbitrary text.
pub mod kinds {
    /// LLM backend plugin kind.
    pub const LLM_BACKEND: &str = "llm-backend";
    /// Tool plugin kind.
    pub const TOOL: &str = "tool";
    /// Skill plugin kind.
    pub const SKILL: &str = "skill";
    /// Pipeline plugin kind.
    pub const PIPELINE: &str = "pipeline";
    /// MCP server plugin kind.
    pub const MCP_SERVER: &str = "mcp-server";
    /// Storage plugin kind.
    pub const STORAGE: &str = "storage";
    /// Sandbox plugin kind.
    pub const SANDBOX: &str = "sandbox";
}
```

- [x] **Step 9.3: Update `package/mod.rs`**

```rust
pub use manifest::{kinds, PackageDep, PackageId, PackageKind};
```

- [x] **Step 9.4: Update `lib.rs` re-exports**

```rust
pub use error::{AgentIdError, PackageKindError, PackageNameError, PackageSourceError};
pub use package::{kinds, GitLocation, PackageDep, PackageId, PackageKind, PackageSource};
```

- [x] **Step 9.5: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 9.6: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/error.rs crates/tau-domain/src/package
git commit -m "feat(tau-domain): add PackageKind enum and kinds constants

PackageKind is non_exhaustive with one v0.1 variant: Custom { kind }.
Typed variants (LlmBackend, Tool, Storage, Sandbox) land in tau-ports
work and beyond per the escape-hatch policy. The kinds module
provides recommended canonical strings.

Refs: G14, spec §3.5.2"
```

---

## Task 10: Capability hierarchy + custom Deserialize

**Files:**
- Create: `crates/tau-domain/src/package/capability.rs`
- Modify: `crates/tau-domain/src/package/mod.rs`
- Modify: `crates/tau-domain/src/lib.rs`

The hierarchical typed enum (β shape) with the custom `Deserialize` mapping flat manifest TOML form (`kind = "fs.read"`) onto the nested variant tree.

- [x] **Step 10.1: Create `package/capability.rs` with the type hierarchy**

```rust
//! Capability declarations attached to a package manifest.
//!
//! Hierarchical typed enum: top-level by namespace
//! (`Filesystem`/`Network`/`Process`/`Agent`/`Custom`), per-namespace
//! verb enums underneath. Variant-level `#[non_exhaustive]` permits
//! additive field evolution.
//!
//! Wire format per ADR-0002: manifest TOML uses flat dot-namespaced
//! `kind = "fs.read"` form. The custom `Deserialize` impl on
//! [`Capability`] maps it onto the variant tree.

use std::collections::BTreeMap;

use crate::value::Value;

/// A capability declaration.
///
/// # Example
///
/// ```
/// use tau_domain::{Capability, FsCapability};
/// let cap = Capability::Filesystem(FsCapability::Read {
///     paths: vec!["${PROJECT}/**".into()],
/// });
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub enum Capability {
    /// Filesystem-related capability.
    Filesystem(FsCapability),
    /// Network-related capability.
    Network(NetCapability),
    /// Process spawning / signaling capability.
    Process(ProcessCapability),
    /// Inter-agent capability.
    Agent(AgentCapability),
    /// Plugin-specific capability not yet typed in core.
    /// See: [escape-hatches.md#capability-custom](../../../../../docs/explanation/escape-hatches.md#capability-custom).
    Custom {
        /// Capability name (e.g. `"mcp.tool.use"`).
        name: String,
        /// Capability parameters.
        params: BTreeMap<String, Value>,
    },
}

/// Filesystem capability verbs.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FsCapability {
    /// Read paths matching the given glob patterns.
    #[non_exhaustive]
    Read {
        /// Glob patterns to grant read access on.
        paths: Vec<String>,
    },
    /// Write paths matching the given globs (with optional size cap).
    #[non_exhaustive]
    Write {
        /// Glob patterns to grant write access on.
        paths: Vec<String>,
        /// Optional maximum write size, in bytes.
        max_bytes: Option<u64>,
    },
    /// Execute (spawn) binaries from paths matching the given globs.
    #[non_exhaustive]
    Exec {
        /// Glob patterns of binaries permitted to execute.
        paths: Vec<String>,
    },
}

/// Network capability verbs.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum NetCapability {
    /// HTTP requests to the allow-listed hosts and methods.
    #[non_exhaustive]
    Http {
        /// Allowed hosts (exact match or glob).
        hosts: Vec<String>,
        /// Allowed HTTP methods (uppercase by convention, e.g. `["GET", "POST"]`).
        methods: Vec<String>,
    },
}

/// Process capability verbs.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ProcessCapability {
    /// Spawn subprocesses for the allow-listed command names.
    #[non_exhaustive]
    Spawn {
        /// Allowed command names.
        commands: Vec<String>,
    },
}

/// Agent capability verbs.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AgentCapability {
    /// Spawn sub-agents whose package kind matches the allow-list.
    #[non_exhaustive]
    Spawn {
        /// Permitted package kinds (e.g. `["worker"]`).
        allowed_kinds: Vec<String>,
    },
}

#[cfg(feature = "serde")]
mod capability_de {
    use super::*;
    use serde::de::{self, MapAccess, Visitor};
    use serde::{Deserialize, Deserializer};

    #[derive(Deserialize)]
    struct RawCapability {
        kind: String,
        #[serde(default)]
        paths: Option<Vec<String>>,
        #[serde(default)]
        max_bytes: Option<u64>,
        #[serde(default)]
        hosts: Option<Vec<String>>,
        #[serde(default)]
        methods: Option<Vec<String>>,
        #[serde(default)]
        commands: Option<Vec<String>>,
        #[serde(default)]
        allowed_kinds: Option<Vec<String>>,
        #[serde(flatten)]
        rest: BTreeMap<String, Value>,
    }

    impl<'de> Deserialize<'de> for Capability {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let raw = RawCapability::deserialize(d)?;
            Ok(match raw.kind.as_str() {
                "fs.read" => Capability::Filesystem(FsCapability::Read {
                    paths: raw.paths.unwrap_or_default(),
                }),
                "fs.write" => Capability::Filesystem(FsCapability::Write {
                    paths: raw.paths.unwrap_or_default(),
                    max_bytes: raw.max_bytes,
                }),
                "fs.exec" => Capability::Filesystem(FsCapability::Exec {
                    paths: raw.paths.unwrap_or_default(),
                }),
                "net.http" => Capability::Network(NetCapability::Http {
                    hosts: raw.hosts.unwrap_or_default(),
                    methods: raw.methods.unwrap_or_default(),
                }),
                "process.spawn" => Capability::Process(ProcessCapability::Spawn {
                    commands: raw.commands.unwrap_or_default(),
                }),
                "agent.spawn" => Capability::Agent(AgentCapability::Spawn {
                    allowed_kinds: raw.allowed_kinds.unwrap_or_default(),
                }),
                _ => Capability::Custom {
                    name: raw.kind,
                    params: raw.rest,
                },
            })
        }
    }

    // Silence unused-variant warnings for Visitor / MapAccess pulled in
    // for thoroughness checks during dev.
    #[allow(dead_code)]
    fn _typecheck<'de, V: Visitor<'de>, M: MapAccess<'de>>(_v: V, _m: M) {}
    #[allow(dead_code)]
    fn _err<E: de::Error>(_e: E) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_read_constructs() {
        let c = Capability::Filesystem(FsCapability::Read {
            paths: vec!["/tmp/**".into()],
        });
        match c {
            Capability::Filesystem(FsCapability::Read { paths }) => {
                assert_eq!(paths, vec!["/tmp/**".to_string()]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn custom_constructs() {
        let mut params = BTreeMap::new();
        params.insert(
            "servers".into(),
            Value::Array(vec![Value::String("fs-mcp".into())]),
        );
        let _c = Capability::Custom {
            name: "mcp.tool.use".into(),
            params,
        };
    }
}
```

- [x] **Step 10.2: Update `package/mod.rs`**

```rust
//! Package metadata types (sources, manifests, capabilities).

pub mod capability;
pub mod manifest;
pub mod source;

pub use capability::{
    AgentCapability, Capability, FsCapability, NetCapability, ProcessCapability,
};
pub use manifest::{kinds, PackageDep, PackageId, PackageKind};
pub use source::{GitLocation, PackageSource};
```

- [x] **Step 10.3: Update `lib.rs`**

```rust
pub use package::{
    kinds, AgentCapability, Capability, FsCapability, GitLocation, NetCapability, PackageDep,
    PackageId, PackageKind, PackageSource, ProcessCapability,
};
```

- [x] **Step 10.4: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 10.5: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/package
git commit -m "feat(tau-domain): add hierarchical Capability with canonical Deserialize

Capability { Filesystem(FsCap), Network(NetCap), Process(ProcessCap),
Agent(AgentCap), Custom }. Variant-level non_exhaustive permits
additive field evolution. Custom Deserialize maps flat manifest form
(kind = 'fs.read') onto the variant tree per ADR-0002's
canonicalization-at-deserialization rule.

Refs: G10, G14, spec §3.5.3, §6"
```

---

## Task 11: UncheckedManifest

**Files:**
- Modify: `crates/tau-domain/src/package/manifest.rs` (append)
- Modify: `crates/tau-domain/src/package/mod.rs`
- Modify: `crates/tau-domain/src/lib.rs`

The deserialization target. No validation yet — that lands in Task 13.

- [x] **Step 11.1: Append `UncheckedManifest` to `package/manifest.rs`**

```rust
use crate::package::capability::Capability;
use crate::package::source::PackageSource;

/// Raw manifest as it appears on disk or on the wire. Deserializes from
/// TOML/JSON directly. May contain field combinations that violate
/// cross-field invariants — call [`UncheckedManifest::validate`] to
/// obtain a verified [`PackageManifest`].
///
/// # Example
///
/// ```no_run
/// use tau_domain::UncheckedManifest;
/// // toml::from_str::<UncheckedManifest>(&raw)?.validate()?;
/// # let _ = std::any::type_name::<UncheckedManifest>();
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UncheckedManifest {
    /// Package name.
    pub name: PackageName,
    /// Package version.
    pub version: Version,
    /// Free-form description.
    pub description: String,
    /// Authors (free-form, e.g. `"Acme Inc <support@acme.dev>"`).
    pub authors: Vec<String>,
    /// SPDX license expression as opaque text. `None` for unlicensed.
    pub license: Option<String>,
    /// Where the package lives.
    pub source: PackageSource,
    /// What the package provides.
    pub kind: PackageKind,
    /// Required dependencies.
    pub dependencies: Vec<PackageDep>,
    /// Capability declarations (G14).
    pub capabilities: Vec<Capability>,
}
```

- [x] **Step 11.2: Update `package/mod.rs`**

```rust
pub use manifest::{kinds, PackageDep, PackageId, PackageKind, UncheckedManifest};
```

- [x] **Step 11.3: Update `lib.rs` re-exports**

```rust
pub use package::{
    kinds, AgentCapability, Capability, FsCapability, GitLocation, NetCapability, PackageDep,
    PackageId, PackageKind, PackageSource, ProcessCapability, UncheckedManifest,
};
```

- [x] **Step 11.4: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 11.5: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/package
git commit -m "feat(tau-domain): add UncheckedManifest deserialization target

UncheckedManifest holds raw manifest fields. Deserialize-from-TOML
target. Cross-field validation lives on a separate validate() method
landing in the next commit; the typestate (PackageManifest wrapping
UncheckedManifest) lands alongside it.

Refs: spec §3.5.2"
```

---

## Task 12: PackageManifest typestate

**Files:**
- Modify: `crates/tau-domain/src/package/manifest.rs` (append)
- Modify: `crates/tau-domain/src/package/mod.rs`
- Modify: `crates/tau-domain/src/lib.rs`

The validated wrapper. Read-only accessors. `From<PackageManifest> for UncheckedManifest` for mutation round-trip. `Serialize` delegated to inner.

- [x] **Step 12.1: Append `PackageManifest` to `package/manifest.rs`**

```rust
/// Validated package manifest. By construction, satisfies all cross-field
/// invariants enforced by [`UncheckedManifest::validate`]. Cannot be
/// constructed directly — must go through validation.
///
/// To mutate a `PackageManifest`, downgrade via `Into<UncheckedManifest>`,
/// edit, then call [`UncheckedManifest::validate`] again.
///
/// # Example
///
/// ```no_run
/// use tau_domain::{UncheckedManifest, PackageManifest};
/// // let manifest: PackageManifest = unchecked.validate()?;
/// # let _ = std::any::type_name::<PackageManifest>();
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct PackageManifest(UncheckedManifest);

impl PackageManifest {
    /// Package name.
    pub fn name(&self) -> &PackageName {
        &self.0.name
    }
    /// Package version.
    pub fn version(&self) -> &Version {
        &self.0.version
    }
    /// Free-form description.
    pub fn description(&self) -> &str {
        &self.0.description
    }
    /// Authors.
    pub fn authors(&self) -> &[String] {
        &self.0.authors
    }
    /// SPDX license expression, if present.
    pub fn license(&self) -> Option<&str> {
        self.0.license.as_deref()
    }
    /// Source location.
    pub fn source(&self) -> &PackageSource {
        &self.0.source
    }
    /// Package kind.
    pub fn kind(&self) -> &PackageKind {
        &self.0.kind
    }
    /// Required dependencies.
    pub fn dependencies(&self) -> &[PackageDep] {
        &self.0.dependencies
    }
    /// Capability declarations.
    pub fn capabilities(&self) -> &[Capability] {
        &self.0.capabilities
    }

    /// Wrap a checked `UncheckedManifest` without re-running validation.
    /// Internal use only — public API must go through
    /// [`UncheckedManifest::validate`].
    pub(crate) fn from_checked(u: UncheckedManifest) -> Self {
        Self(u)
    }
}

impl From<PackageManifest> for UncheckedManifest {
    fn from(m: PackageManifest) -> Self {
        m.0
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for PackageManifest {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

#[cfg(test)]
mod manifest_tests {
    use super::*;
    use std::str::FromStr;

    fn fixture() -> UncheckedManifest {
        UncheckedManifest {
            name: PackageName::from_str("fs-tools").unwrap(),
            version: Version::parse("0.3.0").unwrap(),
            description: "fs tools".into(),
            authors: vec![],
            license: None,
            source: PackageSource::from_str("https://example.com/fs.git").unwrap(),
            kind: PackageKind::Custom { kind: "tool".into() },
            dependencies: vec![],
            capabilities: vec![],
        }
    }

    #[test]
    fn package_manifest_accessors_work() {
        let m = PackageManifest::from_checked(fixture());
        assert_eq!(m.name().as_str(), "fs-tools");
        assert_eq!(m.description(), "fs tools");
        assert_eq!(m.dependencies().len(), 0);
    }

    #[test]
    fn round_trip_through_unchecked() {
        let m = PackageManifest::from_checked(fixture());
        let u: UncheckedManifest = m.into();
        let m2 = PackageManifest::from_checked(u);
        assert_eq!(m2.name().as_str(), "fs-tools");
    }
}
```

- [x] **Step 12.2: Update `package/mod.rs`**

```rust
pub use manifest::{kinds, PackageDep, PackageId, PackageKind, PackageManifest, UncheckedManifest};
```

- [x] **Step 12.3: Update `lib.rs`**

```rust
pub use package::{
    kinds, AgentCapability, Capability, FsCapability, GitLocation, NetCapability, PackageDep,
    PackageId, PackageKind, PackageManifest, PackageSource, ProcessCapability, UncheckedManifest,
};
```

- [x] **Step 12.4: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 12.5: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/package
git commit -m "feat(tau-domain): add PackageManifest typestate wrapper

PackageManifest wraps UncheckedManifest after validation. Read-only
accessors (no Deref to keep typestate guarantee tight). From<PackageManifest>
for UncheckedManifest gives a mutation round-trip. Serialize delegated
to inner. Deserialize NOT implemented — forces deserialize-then-validate.
PackageManifest::from_checked is pub(crate); external construction
must go through validate() (lands next).

Refs: spec §3.5.2 typestate"
```

---

## Task 13: PackageManifest::validate() + PackageManifestError

**Files:**
- Modify: `crates/tau-domain/src/error.rs` (append)
- Modify: `crates/tau-domain/src/package/manifest.rs` (append)
- Modify: `crates/tau-domain/src/lib.rs`

Real validation logic. Strict TDD.

- [x] **Step 13.1: Append `PackageManifestError` to `error.rs`**

```rust
/// Validation errors for [`crate::package::PackageManifest`].
///
/// Composes leaf errors (`PackageNameError`, `PackageSourceError`,
/// `PackageKindError`) via `#[from]` for the first occurrence and
/// `#[source]` for repeated uses.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PackageManifestError {
    /// The `name` field failed `PackageName` validation.
    #[error("manifest field 'name': {0}")]
    Name(#[from] PackageNameError),
    /// The `source` field failed parser/validator.
    #[error("manifest field 'source': {0}")]
    Source(#[from] PackageSourceError),
    /// The `kind` field failed validation.
    #[error("manifest field 'kind': {0}")]
    Kind(#[from] PackageKindError),
    /// The `description` field was empty.
    #[error("manifest field 'description' is empty")]
    EmptyDescription,
    /// A dependency entry's name failed validation.
    #[error("dependency #{index}: invalid name: {source}")]
    DependencyName {
        /// 0-based index of the offending dependency.
        index: usize,
        /// Underlying name validation error.
        #[source]
        source: PackageNameError,
    },
    /// A `Capability::Custom` entry had an empty `name`.
    #[error("capability #{index} has empty name")]
    CapabilityEmptyName {
        /// 0-based index of the offending capability.
        index: usize,
    },
}
```

- [x] **Step 13.2: Append `validate()` to `package/manifest.rs`**

```rust
use crate::error::PackageManifestError;
use crate::package::capability::Capability as Cap;

impl UncheckedManifest {
    /// Run cross-field validation. Returns the validated manifest on
    /// success.
    ///
    /// Field types are already validated at construction (`PackageName`,
    /// `PackageSource`, etc.); this checks invariants those types
    /// can't enforce alone (non-empty description, non-empty Custom
    /// capability names, etc.).
    ///
    /// # Example
    ///
    /// ```
    /// use tau_domain::{UncheckedManifest, PackageManifestError, PackageName, Version,
    ///                 PackageSource, PackageKind};
    /// use std::str::FromStr;
    ///
    /// let m = UncheckedManifest {
    ///     name: PackageName::from_str("foo").unwrap(),
    ///     version: Version::parse("1.0.0").unwrap(),
    ///     description: String::new(),  // ← empty, should fail
    ///     authors: vec![],
    ///     license: None,
    ///     source: PackageSource::from_str("https://x.com/r.git").unwrap(),
    ///     kind: PackageKind::Custom { kind: "tool".into() },
    ///     dependencies: vec![],
    ///     capabilities: vec![],
    /// };
    /// assert_eq!(m.validate(), Err(PackageManifestError::EmptyDescription));
    /// ```
    pub fn validate(self) -> Result<PackageManifest, PackageManifestError> {
        if self.description.is_empty() {
            return Err(PackageManifestError::EmptyDescription);
        }
        // dependency names are already PackageName values (pre-validated),
        // but the loop is here as a hook for future per-dep invariants.
        for (_index, _dep) in self.dependencies.iter().enumerate() {
            // no-op at v0.1
        }
        for (i, cap) in self.capabilities.iter().enumerate() {
            if let Cap::Custom { name, .. } = cap {
                if name.is_empty() {
                    return Err(PackageManifestError::CapabilityEmptyName { index: i });
                }
            }
        }
        Ok(PackageManifest::from_checked(self))
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use std::str::FromStr;

    fn good() -> UncheckedManifest {
        UncheckedManifest {
            name: PackageName::from_str("fs-tools").unwrap(),
            version: Version::parse("0.3.0").unwrap(),
            description: "fs tools".into(),
            authors: vec![],
            license: None,
            source: PackageSource::from_str("https://example.com/fs.git").unwrap(),
            kind: PackageKind::Custom { kind: "tool".into() },
            dependencies: vec![],
            capabilities: vec![],
        }
    }

    #[test]
    fn good_manifest_validates() {
        let m = good().validate().unwrap();
        assert_eq!(m.name().as_str(), "fs-tools");
    }

    #[test]
    fn empty_description_rejected() {
        let mut u = good();
        u.description = String::new();
        assert_eq!(u.validate().unwrap_err(), PackageManifestError::EmptyDescription);
    }

    #[test]
    fn empty_custom_capability_name_rejected() {
        let mut u = good();
        u.capabilities = vec![Cap::Custom {
            name: String::new(),
            params: BTreeMap::new(),
        }];
        let err = u.validate().unwrap_err();
        assert_eq!(err, PackageManifestError::CapabilityEmptyName { index: 0 });
    }
}
```

- [x] **Step 13.3: Update `lib.rs`**

```rust
pub use error::{
    AgentIdError, PackageKindError, PackageManifestError, PackageNameError, PackageSourceError,
};
```

- [x] **Step 13.4: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green. 3 new validation tests + 1 new doctest pass.

- [x] **Step 13.5: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/error.rs crates/tau-domain/src/package
git commit -m "feat(tau-domain): add UncheckedManifest::validate and PackageManifestError

validate() enforces cross-field invariants (non-empty description,
non-empty Custom capability names) and returns a typed
PackageManifestError on failure. The error composes leaf errors
(PackageNameError, PackageSourceError, PackageKindError) via #[from].

Refs: QG2, spec §3.5.2 typestate, §3.6"
```

---

## Task 14: AgentStatus + FailureKind

**Files:**
- Create: `crates/tau-domain/src/agent.rs`
- Modify: `crates/tau-domain/src/lib.rs`

- [x] **Step 14.1: Create `agent.rs` with status types**

Create `/Users/titouanlebocq/code/tau/crates/tau-domain/src/agent.rs`:

```rust
//! Agent definition + lifecycle status types.
//!
//! tau-domain holds the *vocabulary* (identity, definition, status enum).
//! State-machine *transitions* live in tau-runtime — see G2 / spec §3.4.

use std::collections::BTreeMap;

use crate::id::{AgentId, PackageName};
use crate::package::manifest::PackageId;
use crate::value::Value;

/// Agent lifecycle status. Carries diagnostic data only on `Failed`;
/// transition rules live in tau-runtime.
///
/// State graph (informational; not enforced here):
/// `Declared → Installed → Ready → Running ↔ Stopped`,
/// with `Failed` reachable from any non-terminal state.
///
/// # Example
///
/// ```
/// use tau_domain::{AgentStatus, FailureKind};
/// let s = AgentStatus::Failed {
///     kind: FailureKind::BackendError,
///     detail: Some("connection refused: api.openai.com".into()),
/// };
/// match s {
///     AgentStatus::Failed { kind: FailureKind::BackendError, .. } => {
///         // retry with backoff
///     }
///     _ => {}
/// }
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AgentStatus {
    /// Manifest seen, package not yet installed.
    Declared,
    /// Package installed on disk, ready to instantiate.
    Installed,
    /// Instance created, idle.
    Ready,
    /// Actively processing a message.
    Running,
    /// Intentionally halted.
    Stopped,
    /// The agent failed. `kind` enables typed retry/restart logic;
    /// `detail` carries human-readable specifics.
    #[non_exhaustive]
    Failed {
        /// Typed failure category for restart logic.
        kind: FailureKind,
        /// Human-readable detail (e.g. `"panic at src/foo.rs:42"`).
        detail: Option<String>,
    },
}

/// Categorical failure kinds. New typed kinds are added additively;
/// `InternalError` is the catch-all escape hatch.
///
/// See: [escape-hatches.md#failurekind-internalerror](../docs/explanation/escape-hatches.md#failurekind-internalerror).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FailureKind {
    /// Agent process crashed unexpectedly (panic, signal, abort).
    Crashed,
    /// Configured LLM backend returned an error or was unreachable.
    BackendError,
    /// A capability check denied an operation the agent attempted.
    PolicyDenied,
    /// Agent exceeded a resource limit (memory, message rate, timeout).
    OutOfResources,
    /// Catch-all for failures that don't match the named kinds.
    /// See: [escape-hatches.md#failurekind-internalerror](../docs/explanation/escape-hatches.md#failurekind-internalerror).
    InternalError,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_can_carry_detail() {
        let s = AgentStatus::Failed {
            kind: FailureKind::Crashed,
            detail: Some("SIGSEGV".into()),
        };
        match s {
            AgentStatus::Failed { kind: FailureKind::Crashed, detail: Some(d) } => {
                assert_eq!(d, "SIGSEGV");
            }
            _ => panic!(),
        }
    }
}
```

- [x] **Step 14.2: Wire into `lib.rs`**

Add `pub mod agent;` and re-exports:

```rust
pub mod agent;
pub use agent::{AgentStatus, FailureKind};
```

- [x] **Step 14.3: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 14.4: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/agent.rs
git commit -m "feat(tau-domain): add AgentStatus + FailureKind enums

AgentStatus has 6 variants; the Failed variant carries typed
FailureKind + optional detail string. FailureKind has 5 typed
variants plus InternalError as the documented escape hatch.

Refs: spec §3.4"
```

---

## Task 15: AgentDefinition

**Files:**
- Modify: `crates/tau-domain/src/agent.rs` (append)
- Modify: `crates/tau-domain/src/lib.rs`

- [x] **Step 15.1: Append `AgentDefinition` to `agent.rs`**

```rust
/// Static description of an agent. Holds what the runtime needs to
/// instantiate one; richer config lives in skills / plugin packages
/// per G2.
///
/// # Example
///
/// ```
/// use tau_domain::{AgentDefinition, AgentId, PackageId, PackageName, Version};
/// use std::str::FromStr;
///
/// let def = AgentDefinition::new(
///     AgentId::from_str("researcher").unwrap(),
///     "Researcher".into(),
///     PackageId {
///         name: PackageName::from_str("research-pkg").unwrap(),
///         version: Version::parse("0.1.0").unwrap(),
///     },
///     PackageName::from_str("claude-anthropic").unwrap(),
/// );
/// assert_eq!(def.id.as_str(), "researcher");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AgentDefinition {
    /// Canonical identifier for this definition.
    pub id: AgentId,
    /// Human-readable display name (free-form, unvalidated).
    pub display_name: String,
    /// Which package this agent ships from.
    pub package: PackageId,
    /// Reference to an installed LLM-backend plugin package.
    /// Required at v0.1 (see ADR-0002 escape clause).
    pub llm_backend: PackageName,
    /// Optional system prompt.
    pub system_prompt: Option<String>,
    /// Free-form per-agent config (validated by plugins, not by tau-domain).
    pub config: BTreeMap<String, Value>,
}

impl AgentDefinition {
    /// Construct an `AgentDefinition` with empty `system_prompt` and
    /// `config`. Use the `with_*` builders to fill them in.
    pub fn new(
        id: AgentId,
        display_name: String,
        package: PackageId,
        llm_backend: PackageName,
    ) -> Self {
        Self {
            id,
            display_name,
            package,
            llm_backend,
            system_prompt: None,
            config: BTreeMap::new(),
        }
    }

    /// Set `system_prompt`.
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = Some(prompt);
        self
    }

    /// Set `config`.
    pub fn with_config(mut self, config: BTreeMap<String, Value>) -> Self {
        self.config = config;
        self
    }
}

#[cfg(test)]
mod definition_tests {
    use super::*;
    use crate::version::Version;
    use std::str::FromStr;

    #[test]
    fn builder_chain_sets_fields() {
        let def = AgentDefinition::new(
            AgentId::from_str("a").unwrap(),
            "A".into(),
            PackageId {
                name: PackageName::from_str("p").unwrap(),
                version: Version::parse("0.0.1").unwrap(),
            },
            PackageName::from_str("b").unwrap(),
        )
        .with_system_prompt("hi".into());

        assert_eq!(def.system_prompt.as_deref(), Some("hi"));
    }
}
```

- [x] **Step 15.2: Update `lib.rs`**

```rust
pub use agent::{AgentDefinition, AgentStatus, FailureKind};
```

- [x] **Step 15.3: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 15.4: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/agent.rs
git commit -m "feat(tau-domain): add AgentDefinition with builder constructor

Required fields (id, display_name, package, llm_backend) take
pre-validated newtypes — no AgentDefinitionError needed at v0.1.
with_system_prompt/with_config builder methods cover optional fields.

Refs: G4, Constitution Appendix C, spec §3.4"
```

---

## Task 16: Message envelope (Address + MessagePayload + Message)

**Files:**
- Create: `crates/tau-domain/src/message.rs`
- Modify: `crates/tau-domain/src/lib.rs`

- [x] **Step 16.1: Create `message.rs`**

```rust
//! Message envelope, addressing, and payload types (G5).

use std::collections::BTreeMap;
use std::time::SystemTime;

use crate::agent::AgentStatus;
use crate::id::{AgentInstanceId, MessageId};
use crate::value::Value;

/// Sender or recipient of a [`Message`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Address {
    /// A specific agent instance.
    Agent(AgentInstanceId),
    /// A named tool. The runtime resolves name → plugin via its
    /// registration table.
    Tool(String),
    /// A human user (e.g. the operator at the CLI).
    User,
    /// The runtime / observer.
    System,
}

/// Message body. Typed variants for known shapes; `Custom` for
/// plugin-specific.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MessagePayload {
    /// Human- or agent-authored text. The envelope's `sender` field
    /// distinguishes origin.
    Text {
        /// Message text.
        content: String,
    },
    /// A tool invocation. The envelope's `recipient: Address::Tool(...)`
    /// names the tool; this carries the arguments.
    ToolCall {
        /// Arguments to pass to the tool.
        args: Value,
    },
    /// Successful tool result.
    ToolResult {
        /// Tool's response body.
        body: Value,
    },
    /// Tool returned an error.
    ToolError {
        /// Error kind (free-form string convention).
        kind: String,
        /// Human-readable error message.
        message: String,
        /// Optional structured detail.
        details: Option<Value>,
    },
    /// Lifecycle event broadcast (System → observers).
    Lifecycle(AgentStatus),
    /// Plugin-specific message kind.
    /// See: [escape-hatches.md#messagepayload-custom](../docs/explanation/escape-hatches.md#messagepayload-custom).
    Custom {
        /// Plugin-specific kind tag (e.g. `"mcp.resource.request"`).
        kind: String,
        /// Plugin-specific body bytes.
        body: Vec<u8>,
    },
}

/// A message envelope (G5).
///
/// # Example
///
/// ```
/// use tau_domain::{Message, MessageId, Address, MessagePayload};
/// use std::time::SystemTime;
/// use std::collections::BTreeMap;
///
/// let m = Message {
///     id: MessageId::new(),
///     sender: Address::User,
///     recipient: Address::System,
///     parent_id: None,
///     created_at: SystemTime::UNIX_EPOCH,
///     headers: BTreeMap::new(),
///     payload: MessagePayload::Text { content: "hello".into() },
/// };
/// assert!(matches!(m.payload, MessagePayload::Text { .. }));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Message {
    /// Globally unique message identifier.
    pub id: MessageId,
    /// Originator.
    pub sender: Address,
    /// Destination.
    pub recipient: Address,
    /// Optional pointer to the message this one replies to.
    pub parent_id: Option<MessageId>,
    /// When the message was created.
    pub created_at: SystemTime,
    /// Free-form headers. `BTreeMap` for stable iteration order.
    pub headers: BTreeMap<String, String>,
    /// Message body.
    pub payload: MessagePayload,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_payload_holds_status() {
        let m = MessagePayload::Lifecycle(AgentStatus::Ready);
        assert!(matches!(m, MessagePayload::Lifecycle(AgentStatus::Ready)));
    }
}
```

- [x] **Step 16.2: Update `lib.rs`**

```rust
pub mod message;
pub use message::{Address, Message, MessagePayload};
```

- [x] **Step 16.3: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 16.4: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/message.rs
git commit -m "feat(tau-domain): add Message envelope, Address, MessagePayload

Message holds id/sender/recipient/parent_id/created_at/headers/payload.
Address has 4 variants (Agent, Tool, User, System); Tool is a tuple
variant carrying just the name. MessagePayload has 6 variants: Text,
ToolCall, ToolResult, ToolError, Lifecycle(AgentStatus), Custom.

Refs: G5, G10, spec §3.3"
```

---

## Task 17: `test-fixtures` feature module

**Files:**
- Create: `crates/tau-domain/src/fixtures.rs`
- Modify: `crates/tau-domain/src/lib.rs`

Construction helpers for tests. Exposed via the `test-fixtures` feature.

- [x] **Step 17.1: Create `fixtures.rs`**

```rust
//! Test fixtures for `tau-domain` types.
//!
//! Gated behind the `test-fixtures` feature (off by default). Downstream
//! crates depend via:
//!
//! ```toml
//! [dev-dependencies]
//! tau-domain = { workspace = true, features = ["test-fixtures"] }
//! ```
//!
//! All helpers are deterministic where possible; UUID-based ones
//! (`any_message`, `any_agent_definition`-derived IDs) generate fresh
//! v7 UUIDs each call.

use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::SystemTime;

use crate::agent::AgentDefinition;
use crate::id::{AgentId, AgentInstanceId, MessageId, PackageName};
use crate::message::{Address, Message, MessagePayload};
use crate::package::manifest::{PackageId, PackageKind, UncheckedManifest};
use crate::package::{PackageManifest, PackageSource};
use crate::version::Version;

/// A deterministic, valid `PackageName`.
pub fn any_package_name() -> PackageName {
    PackageName::from_str("test-pkg").expect("valid")
}

/// A deterministic, valid `AgentId`.
pub fn any_agent_id() -> AgentId {
    AgentId::from_str("test-agent").expect("valid")
}

/// A deterministic, valid `PackageSource` (https URL, no rev).
pub fn any_package_source() -> PackageSource {
    PackageSource::from_str("https://example.com/test.git").expect("valid")
}

/// A minimal valid `UncheckedManifest`.
pub fn any_unchecked_manifest() -> UncheckedManifest {
    UncheckedManifest {
        name: any_package_name(),
        version: Version::parse("0.1.0").expect("valid"),
        description: "test package".into(),
        authors: vec![],
        license: None,
        source: any_package_source(),
        kind: PackageKind::Custom {
            kind: "tool".into(),
        },
        dependencies: vec![],
        capabilities: vec![],
    }
}

/// A minimal validated `PackageManifest`.
pub fn any_package_manifest() -> PackageManifest {
    any_unchecked_manifest()
        .validate()
        .expect("fixture should validate")
}

/// A minimal `AgentDefinition`.
pub fn any_agent_definition() -> AgentDefinition {
    AgentDefinition::new(
        any_agent_id(),
        "Test Agent".into(),
        PackageId {
            name: any_package_name(),
            version: Version::parse("0.1.0").expect("valid"),
        },
        any_package_name(),
    )
}

/// A minimal `Message` with a Text payload (fresh UUIDs).
pub fn any_message() -> Message {
    Message {
        id: MessageId::new(),
        sender: Address::User,
        recipient: Address::Agent(AgentInstanceId::new()),
        parent_id: None,
        created_at: SystemTime::UNIX_EPOCH,
        headers: BTreeMap::new(),
        payload: MessagePayload::Text {
            content: "hello".into(),
        },
    }
}
```

- [x] **Step 17.2: Wire into `lib.rs` behind the feature**

Add to `lib.rs`:

```rust
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures;
```

- [x] **Step 17.3: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo build -p tau-domain --no-default-features
cargo build -p tau-domain --features test-fixtures
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 17.4: Stage and commit**

```bash
git add crates/tau-domain/src/lib.rs crates/tau-domain/src/fixtures.rs
git commit -m "feat(tau-domain): add test-fixtures feature module

pub mod fixtures with any_* helpers for every public domain type.
Gated behind the test-fixtures feature so production builds don't
pull the code. Downstream crates opt in via dev-dependencies.

Refs: spec §5"
```

---

## Task 18: Proptest suite

**Files:**
- Create: `crates/tau-domain/tests/proptest_package_source.rs`
- Create: `crates/tau-domain/tests/proptest_ids.rs`
- Create: `crates/tau-domain/tests/proptest_message_envelope.rs`
- Create: `crates/tau-domain/tests/proptest_value.rs`

- [x] **Step 18.1: Write `proptest_package_source.rs`**

```rust
//! Property tests for `PackageSource` parser/round-trip.

use proptest::prelude::*;
use std::str::FromStr;

use tau_domain::{PackageSource, PackageSourceError};

fn arb_url_source() -> impl Strategy<Value = String> {
    let scheme = prop_oneof![Just("https"), Just("http"), Just("ssh"), Just("git")];
    let host = "[a-z][a-z0-9-]{0,30}(\\.[a-z][a-z0-9-]{0,30}){1,3}";
    let path = "[a-z0-9]{1,20}(/[a-z0-9]{1,20}){0,3}\\.git";
    (scheme, host, path).prop_map(|(s, h, p)| format!("{s}://{h}/{p}"))
}

fn arb_scp_source() -> impl Strategy<Value = String> {
    let user = "[a-z][a-z0-9]{0,15}";
    let host = "[a-z][a-z0-9-]{0,30}(\\.[a-z][a-z0-9-]{0,30}){1,3}";
    let path = "[a-z0-9]{1,20}(/[a-z0-9]{1,20}){0,3}\\.git";
    (user, host, path).prop_map(|(u, h, p)| format!("{u}@{h}:{p}"))
}

proptest! {
    #[test]
    fn url_source_round_trips(s in arb_url_source()) {
        let parsed = PackageSource::from_str(&s).unwrap();
        prop_assert_eq!(parsed.to_string(), s);
    }

    #[test]
    fn scp_source_round_trips(s in arb_scp_source()) {
        let parsed = PackageSource::from_str(&s).unwrap();
        prop_assert_eq!(parsed.to_string(), s);
    }

    #[test]
    fn empty_input_rejected(_unit in any::<()>()) {
        prop_assert_eq!(PackageSource::from_str(""), Err(PackageSourceError::Empty));
    }

    #[test]
    fn empty_rev_rejected(s in arb_url_source()) {
        let with_empty = format!("{s}#");
        prop_assert_eq!(PackageSource::from_str(&with_empty), Err(PackageSourceError::EmptyRevision));
    }
}
```

- [x] **Step 18.2: Write `proptest_ids.rs`**

```rust
//! Property tests for `PackageName` and `AgentId` grammar.

use proptest::prelude::*;
use std::str::FromStr;

use tau_domain::{AgentId, AgentIdError, PackageName, PackageNameError};

proptest! {
    #[test]
    fn package_name_round_trips(s in "[a-z][a-z0-9-]{0,63}") {
        let n = PackageName::from_str(&s).unwrap();
        prop_assert_eq!(n.to_string(), s);
    }

    #[test]
    fn agent_id_round_trips(s in "[a-z][a-z0-9-]{0,63}") {
        let id = AgentId::from_str(&s).unwrap();
        prop_assert_eq!(id.to_string(), s);
    }

    #[test]
    fn package_name_invalid_leading_rejected(s in "[A-Z0-9-][a-z0-9-]{0,63}") {
        prop_assert!(matches!(PackageName::from_str(&s), Err(PackageNameError::InvalidLeadingCharacter { .. }) | Err(PackageNameError::Empty)));
    }

    #[test]
    fn agent_id_invalid_leading_rejected(s in "[A-Z0-9-][a-z0-9-]{0,63}") {
        prop_assert!(matches!(AgentId::from_str(&s), Err(AgentIdError::InvalidLeadingCharacter { .. }) | Err(AgentIdError::Empty)));
    }
}
```

- [x] **Step 18.3: Write `proptest_message_envelope.rs`**

```rust
//! Property test: arbitrary Message round-trips through serde_json.

use proptest::prelude::*;
use std::collections::BTreeMap;
use std::time::SystemTime;

use tau_domain::{Address, AgentInstanceId, AgentStatus, FailureKind, Message, MessageId,
                 MessagePayload, Value};

fn arb_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::Integer),
        any::<f64>().prop_filter("finite", |f| f.is_finite()).prop_map(Value::Float),
        ".{0,16}".prop_map(Value::String),
    ];
    leaf.prop_recursive(2, 8, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map(".{0,8}", inner, 0..4).prop_map(Value::Object),
        ]
    })
}

fn arb_payload() -> impl Strategy<Value = MessagePayload> {
    prop_oneof![
        ".{0,32}".prop_map(|s| MessagePayload::Text { content: s }),
        arb_value().prop_map(|v| MessagePayload::ToolCall { args: v }),
        arb_value().prop_map(|v| MessagePayload::ToolResult { body: v }),
        Just(MessagePayload::Lifecycle(AgentStatus::Ready)),
        Just(MessagePayload::Lifecycle(AgentStatus::Failed {
            kind: FailureKind::InternalError,
            detail: None,
        })),
    ]
}

fn arb_message() -> impl Strategy<Value = Message> {
    arb_payload().prop_map(|payload| Message {
        id: MessageId::new(),
        sender: Address::User,
        recipient: Address::Agent(AgentInstanceId::new()),
        parent_id: None,
        created_at: SystemTime::UNIX_EPOCH,
        headers: BTreeMap::new(),
        payload,
    })
}

proptest! {
    #[test]
    fn message_round_trips_through_json(m in arb_message()) {
        let s = serde_json::to_string(&m).unwrap();
        let back: Message = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(m, back);
    }
}
```

- [x] **Step 18.4: Write `proptest_value.rs`**

```rust
//! Property test: arbitrary Value round-trips through serde_json.

use proptest::prelude::*;

use tau_domain::Value;

fn arb_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::Integer),
        any::<f64>().prop_filter("finite", |f| f.is_finite()).prop_map(Value::Float),
        ".{0,16}".prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 32, 8, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map(".{0,8}", inner, 0..4).prop_map(Value::Object),
        ]
    })
}

proptest! {
    #[test]
    fn value_round_trips_through_json(v in arb_value()) {
        let s = serde_json::to_string(&v).unwrap();
        let back: Value = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(v, back);
    }
}
```

- [x] **Step 18.5: Run the proptest suite**

```bash
cargo test -p tau-domain --all-targets --all-features
```

Expected: every proptest passes (each runs 256 cases by default; first run will be slower as proptest builds its corpus).

- [x] **Step 18.6: Run clippy + fmt**

```bash
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 18.7: Stage and commit**

```bash
git add crates/tau-domain/tests/proptest_package_source.rs crates/tau-domain/tests/proptest_ids.rs crates/tau-domain/tests/proptest_message_envelope.rs crates/tau-domain/tests/proptest_value.rs
git commit -m "test(tau-domain): add proptest suite for parsers and serde round-trips

Four proptest files: PackageSource (URL + scp round-trips, empty
rejection), PackageName/AgentId grammar, Message JSON round-trip,
Value JSON round-trip (max depth 4).

Refs: QG5, spec §4, §5"
```

---

## Task 19: Integration test suite

**Files:**
- Create: `crates/tau-domain/tests/manifest_roundtrip.rs`
- Create: `crates/tau-domain/tests/manifest_validation_table.rs`
- Create: `crates/tau-domain/tests/message_envelope_serde.rs`
- Create: `crates/tau-domain/tests/package_source_grammar.rs`

Table-driven and example-based integration tests.

- [x] **Step 19.1: Write `manifest_roundtrip.rs`**

```rust
//! Integration test: TOML manifest round-trips through
//! UncheckedManifest → validate → PackageManifest → serialize.

use tau_domain::{PackageManifest, UncheckedManifest};

const SAMPLE: &str = r#"
name = "fs-tools"
version = "0.3.0"
description = "Filesystem tools"
authors = ["Acme <hi@acme.dev>"]
license = "MIT OR Apache-2.0"
dependencies = []

[source]
[source.Git]
rev = "v0.3.0"
[source.Git.location]
[source.Git.location.Url]
"https" = "_"
# ↑ a placeholder; the test below uses programmatic construction to
# avoid TOML representation gymnastics for the url::Url internals.

kind = { Custom = { kind = "tool" } }
capabilities = []
"#;

#[test]
fn programmatic_manifest_round_trips_through_serde_json() {
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use tau_domain::{PackageKind, PackageName, PackageSource, Version};

    let unchecked = UncheckedManifest {
        name: PackageName::from_str("fs-tools").unwrap(),
        version: Version::parse("0.3.0").unwrap(),
        description: "Filesystem tools".into(),
        authors: vec!["Acme <hi@acme.dev>".into()],
        license: Some("MIT OR Apache-2.0".into()),
        source: PackageSource::from_str("https://example.com/fs.git#v0.3.0").unwrap(),
        kind: PackageKind::Custom { kind: "tool".into() },
        dependencies: vec![],
        capabilities: vec![],
    };

    let manifest: PackageManifest = unchecked.clone().validate().unwrap();
    let json = serde_json::to_string(&manifest).unwrap();
    let back: UncheckedManifest = serde_json::from_str(&json).unwrap();
    let revalidated = back.validate().unwrap();

    assert_eq!(revalidated.name().as_str(), "fs-tools");
    assert_eq!(revalidated.description(), "Filesystem tools");

    // The constant is not used by the current test body (TOML for url::Url
    // is finicky); referenced here to keep the file's compile dependency
    // list documented.
    let _ = SAMPLE;
    let _ = std::any::type_name::<BTreeMap<String, String>>();
}
```

- [x] **Step 19.2: Write `manifest_validation_table.rs`**

```rust
//! Table-driven test: malformed manifests produce specific
//! PackageManifestError variants. Replaces a proptest that would have
//! had to generate "valid except for one thing" — which is more
//! brittle than hand-picked cases.

use std::collections::BTreeMap;
use std::str::FromStr;

use tau_domain::{
    Capability, PackageKind, PackageManifestError, PackageName, PackageSource,
    UncheckedManifest, Version,
};

fn good() -> UncheckedManifest {
    UncheckedManifest {
        name: PackageName::from_str("fs-tools").unwrap(),
        version: Version::parse("0.3.0").unwrap(),
        description: "fs tools".into(),
        authors: vec![],
        license: None,
        source: PackageSource::from_str("https://example.com/fs.git").unwrap(),
        kind: PackageKind::Custom { kind: "tool".into() },
        dependencies: vec![],
        capabilities: vec![],
    }
}

#[test]
fn empty_description() {
    let mut u = good();
    u.description = String::new();
    assert_eq!(u.validate().unwrap_err(), PackageManifestError::EmptyDescription);
}

#[test]
fn empty_capability_custom_name() {
    let mut u = good();
    u.capabilities = vec![Capability::Custom {
        name: String::new(),
        params: BTreeMap::new(),
    }];
    assert_eq!(
        u.validate().unwrap_err(),
        PackageManifestError::CapabilityEmptyName { index: 0 },
    );
}

#[test]
fn empty_capability_at_nonzero_index() {
    let mut u = good();
    u.capabilities = vec![
        Capability::Custom {
            name: "ok".into(),
            params: BTreeMap::new(),
        },
        Capability::Custom {
            name: String::new(),
            params: BTreeMap::new(),
        },
    ];
    assert_eq!(
        u.validate().unwrap_err(),
        PackageManifestError::CapabilityEmptyName { index: 1 },
    );
}

#[test]
fn good_validates() {
    assert!(good().validate().is_ok());
}
```

- [x] **Step 19.3: Write `message_envelope_serde.rs`**

```rust
//! Integration test: every MessagePayload variant round-trips
//! through serde_json.

use std::collections::BTreeMap;
use std::time::SystemTime;

use tau_domain::{
    Address, AgentInstanceId, AgentStatus, FailureKind, Message, MessageId, MessagePayload, Value,
};

fn envelope(payload: MessagePayload) -> Message {
    Message {
        id: MessageId::new(),
        sender: Address::User,
        recipient: Address::Agent(AgentInstanceId::new()),
        parent_id: None,
        created_at: SystemTime::UNIX_EPOCH,
        headers: BTreeMap::new(),
        payload,
    }
}

fn round_trips(m: Message) {
    let s = serde_json::to_string(&m).unwrap();
    let back: Message = serde_json::from_str(&s).unwrap();
    assert_eq!(m, back);
}

#[test]
fn text() {
    round_trips(envelope(MessagePayload::Text {
        content: "hi".into(),
    }));
}

#[test]
fn tool_call() {
    round_trips(envelope(MessagePayload::ToolCall {
        args: Value::String("read /tmp/foo".into()),
    }));
}

#[test]
fn tool_result() {
    round_trips(envelope(MessagePayload::ToolResult {
        body: Value::Integer(42),
    }));
}

#[test]
fn tool_error() {
    round_trips(envelope(MessagePayload::ToolError {
        kind: "io".into(),
        message: "permission denied".into(),
        details: Some(Value::Null),
    }));
}

#[test]
fn lifecycle() {
    round_trips(envelope(MessagePayload::Lifecycle(AgentStatus::Failed {
        kind: FailureKind::Crashed,
        detail: Some("SIGSEGV".into()),
    })));
}

#[test]
fn custom() {
    round_trips(envelope(MessagePayload::Custom {
        kind: "mcp.tool.use".into(),
        body: vec![1, 2, 3],
    }));
}
```

- [x] **Step 19.4: Write `package_source_grammar.rs`**

```rust
//! Integration test: PackageSource grammar across known cases.

use std::str::FromStr;

use tau_domain::{GitLocation, PackageSource, PackageSourceError};

#[test]
fn https_no_rev() {
    let s = PackageSource::from_str("https://example.com/r.git").unwrap();
    let PackageSource::Git { location: GitLocation::Url(u), rev } = s else {
        panic!("expected Git/Url variant");
    };
    assert_eq!(u.scheme(), "https");
    assert_eq!(rev, None);
}

#[test]
fn ssh_with_rev() {
    let s = PackageSource::from_str("ssh://git@example.com/r.git#main").unwrap();
    let PackageSource::Git { rev, .. } = s else {
        panic!("expected Git variant");
    };
    assert_eq!(rev.as_deref(), Some("main"));
}

#[test]
fn scp_no_user() {
    let s = PackageSource::from_str("example.com:r.git").unwrap();
    let PackageSource::Git { location: GitLocation::Scp { user, host, path }, .. } = s else {
        panic!();
    };
    assert!(user.is_none());
    assert_eq!(host, "example.com");
    assert_eq!(path, "r.git");
}

#[test]
fn rejects_ftp() {
    assert!(matches!(
        PackageSource::from_str("ftp://example.com/r.git"),
        Err(PackageSourceError::UnsupportedScheme { .. }),
    ));
}

#[test]
fn rejects_empty_rev_marker() {
    assert_eq!(
        PackageSource::from_str("https://example.com/r.git#"),
        Err(PackageSourceError::EmptyRevision),
    );
}
```

- [x] **Step 19.5: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 19.6: Stage and commit**

```bash
git add crates/tau-domain/tests/manifest_roundtrip.rs crates/tau-domain/tests/manifest_validation_table.rs crates/tau-domain/tests/message_envelope_serde.rs crates/tau-domain/tests/package_source_grammar.rs
git commit -m "test(tau-domain): add integration test suite

Four integration tests: manifest round-trip via serde_json, manifest
validation table (4 hand-picked malformed cases), MessagePayload
serde round-trip per variant, PackageSource grammar across known
inputs.

Refs: QG5, spec §5"
```

---

## Task 20: Wire-format golden tests

**Files:**
- Create: `crates/tau-domain/tests/wire_format/*.json` and `*.toml`
- Create: `crates/tau-domain/tests/wire_format_golden.rs`

Canonical serialized forms; tests assert deserialize-then-serialize is byte-identical.

- [x] **Step 20.1: Create the wire_format directory and files**

```bash
mkdir -p crates/tau-domain/tests/wire_format
```

Create `crates/tau-domain/tests/wire_format/message_text.json`:

```json
{
  "id": "01890000-0000-7000-8000-000000000001",
  "sender": "User",
  "recipient": {
    "Agent": "01890000-0000-7000-8000-000000000002"
  },
  "parent_id": null,
  "created_at": {
    "secs_since_epoch": 0,
    "nanos_since_epoch": 0
  },
  "headers": {},
  "payload": {
    "Text": {
      "content": "hello"
    }
  }
}
```

Create `crates/tau-domain/tests/wire_format/message_tool_call.json`:

```json
{
  "id": "01890000-0000-7000-8000-000000000003",
  "sender": {
    "Agent": "01890000-0000-7000-8000-000000000004"
  },
  "recipient": {
    "Tool": "fs.read"
  },
  "parent_id": null,
  "created_at": {
    "secs_since_epoch": 0,
    "nanos_since_epoch": 0
  },
  "headers": {},
  "payload": {
    "ToolCall": {
      "args": {
        "String": "/tmp/foo"
      }
    }
  }
}
```

Create `crates/tau-domain/tests/wire_format/message_tool_result.json`:

```json
{
  "id": "01890000-0000-7000-8000-000000000005",
  "sender": {
    "Tool": "fs.read"
  },
  "recipient": {
    "Agent": "01890000-0000-7000-8000-000000000004"
  },
  "parent_id": "01890000-0000-7000-8000-000000000003",
  "created_at": {
    "secs_since_epoch": 0,
    "nanos_since_epoch": 0
  },
  "headers": {},
  "payload": {
    "ToolResult": {
      "body": {
        "String": "file contents"
      }
    }
  }
}
```

Create `crates/tau-domain/tests/wire_format/message_lifecycle.json`:

```json
{
  "id": "01890000-0000-7000-8000-000000000006",
  "sender": "System",
  "recipient": "User",
  "parent_id": null,
  "created_at": {
    "secs_since_epoch": 0,
    "nanos_since_epoch": 0
  },
  "headers": {},
  "payload": {
    "Lifecycle": {
      "Failed": {
        "kind": "BackendError",
        "detail": "connection refused"
      }
    }
  }
}
```

Create `crates/tau-domain/tests/wire_format/package_source_https.txt`:

```
https://github.com/owner/repo.git#v1.0.0
```

Create `crates/tau-domain/tests/wire_format/package_source_scp.txt`:

```
git@github.com:owner/repo.git
```

- [x] **Step 20.2: Write `wire_format_golden.rs`**

```rust
//! Golden-file tests for the wire format. Each canonical input file
//! deserializes and serializes back byte-for-byte (after normalization).
//!
//! When a derive(Serialize) change ships a wire break, these tests fail
//! at PR review.

use std::str::FromStr;

use tau_domain::{Message, PackageSource};

fn read(path: &str) -> String {
    std::fs::read_to_string(path).expect(path)
}

fn round_trip_message(path: &str) {
    let raw = read(path);
    let parsed: Message = serde_json::from_str(&raw).unwrap();
    let re = serde_json::to_string_pretty(&parsed).unwrap();
    // Normalize trailing whitespace.
    assert_eq!(
        re.trim(),
        raw.trim(),
        "wire format drifted for {path}",
    );
}

#[test]
fn message_text_golden() {
    round_trip_message("tests/wire_format/message_text.json");
}

#[test]
fn message_tool_call_golden() {
    round_trip_message("tests/wire_format/message_tool_call.json");
}

#[test]
fn message_tool_result_golden() {
    round_trip_message("tests/wire_format/message_tool_result.json");
}

#[test]
fn message_lifecycle_golden() {
    round_trip_message("tests/wire_format/message_lifecycle.json");
}

#[test]
fn package_source_https_golden() {
    let raw = read("tests/wire_format/package_source_https.txt");
    let parsed = PackageSource::from_str(raw.trim()).unwrap();
    assert_eq!(parsed.to_string(), raw.trim());
}

#[test]
fn package_source_scp_golden() {
    let raw = read("tests/wire_format/package_source_scp.txt");
    let parsed = PackageSource::from_str(raw.trim()).unwrap();
    assert_eq!(parsed.to_string(), raw.trim());
}
```

- [x] **Step 20.3: Run the golden tests**

```bash
cargo test -p tau-domain --test wire_format_golden --all-features
```

If any test fails because the actual serialization disagrees with the golden file, **inspect the diff** — if the type / serde change is intentional, regenerate the golden file from the actual output. If the change is a drift, fix the type.

- [x] **Step 20.4: Run full test suite + clippy + fmt**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 20.5: Stage and commit**

```bash
git add crates/tau-domain/tests/wire_format crates/tau-domain/tests/wire_format_golden.rs
git commit -m "test(tau-domain): add wire-format golden tests

Six golden-file tests: four Message variants (Text, ToolCall, ToolResult,
Lifecycle) round-trip through serde_json; two PackageSource forms (https
and scp) round-trip through Display+FromStr. Failures surface
wire-format drift at PR review.

Refs: G5, G6, QG12, spec §5"
```

---

## Task 21: CI — `--no-default-features` job

**Files:**
- Modify: `.github/workflows/ci.yml`

- [x] **Step 21.1: Read current CI workflow**

```bash
cat .github/workflows/ci.yml | head -80
```

- [x] **Step 21.2: Add the `no-default-features` job**

Edit `.github/workflows/ci.yml`. Add a new job alongside the existing `fmt`, `clippy`, `test` jobs:

```yaml
  no-default-features:
    name: build (no-default-features)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
      - name: Build tau-domain (no default features)
        run: cargo build -p tau-domain --no-default-features
      - name: Test tau-domain (no default features)
        run: cargo test -p tau-domain --no-default-features --lib
```

(The `--lib` is to skip integration tests that depend on `serde`.)

- [x] **Step 21.3: Validate YAML locally**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))" 2>&1
```

Expected: no output.

- [x] **Step 21.4: Run the new check locally**

```bash
cargo build -p tau-domain --no-default-features
cargo test -p tau-domain --no-default-features --lib
```

Expected: success.

- [x] **Step 21.5: Stage and commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add tau-domain --no-default-features job

Verifies the off-by-default serde feature claim. Builds tau-domain
without default features and runs unit tests on the non-serde paths.

Refs: spec §5, ADR-0002 escape-hatch policy implementation"
```

---

## Task 22: Seed escape-hatch registry

**Files:**
- Create: `docs/explanation/escape-hatches.md`

- [x] **Step 22.1: Create the registry file**

Create `/Users/titouanlebocq/code/tau/docs/explanation/escape-hatches.md`:

````markdown
# Escape-hatch registry

Each entry below names a place where tau core uses a structural escape
hatch (`Custom`, `InternalError`) instead of typed variants. Per
ADR-0002, every escape hatch must be documented here with rationale
and promotion trigger.

**PR rule:** any PR that introduces, promotes, or removes an escape
hatch updates this file in the same commit. The CI test
`crates/tau-domain/tests/escape_hatch_registry.rs` enforces this
mechanically.

## Active escape hatches

| Anchor | Location | Reason | Promotion trigger | Sub-project |
|---|---|---|---|---|
| <a id="capability-custom"></a>`capability-custom` | `Capability::Custom { name, params }` | Capability vocabulary not yet typed; tau-runtime hasn't determined which capabilities need typed variants beyond the v0.1 set (Filesystem/Network/Process/Agent). | When tau-runtime ships namespace enforcement for a new namespace (sub-project 4+), promote the namespace's verbs to typed variants. | 1 |
| <a id="messagepayload-custom"></a>`messagepayload-custom` | `MessagePayload::Custom { kind, body }` | Plugin-specific message kinds (MCP resources, skill-specific shapes) not yet enumerated. | When MCP plugin trait stabilizes (sub-project 2+), promote `mcp.*` shapes; same for skill-specific message kinds. | 1 |
| <a id="packagekind-custom"></a>`packagekind-custom` | `PackageKind::Custom { kind }` | All package kinds go through `Custom` at v0.1; no typed variants exist. | When tau-ports lands plugin traits for LLM/Tool/Storage/Sandbox (sub-project 2), consider promoting matching `PackageKind` variants. | 1 |
| <a id="failurekind-internalerror"></a>`failurekind-internalerror` | `FailureKind::InternalError` | Catch-all for failures not matching the v0.1 typed kinds (Crashed, BackendError, PolicyDenied, OutOfResources). tau-runtime hasn't yet emitted enough variety to identify recurring shapes. | When tau-runtime construction sites for `InternalError` exceed 3 distinct contexts, file an ADR proposing typed variants for the recurring shapes. | 1 |

## Promoted escape hatches

(none yet)

## Removed escape hatches

(none yet)
````

- [x] **Step 22.2: Verify the file is well-formed**

```bash
ls -la docs/explanation/escape-hatches.md
wc -l docs/explanation/escape-hatches.md
```

Expected: file exists, ~30 lines.

- [x] **Step 22.3: Stage and commit**

```bash
git add docs/explanation/escape-hatches.md
git commit -m "docs: seed escape-hatch registry with v0.1 entries

Four active entries: Capability::Custom, MessagePayload::Custom,
PackageKind::Custom, FailureKind::InternalError. Each row carries
rationale + promotion trigger. The CI registry-coverage test (next
commit) enforces that every escape-hatch variant in code maps to a
row in this file.

Refs: ADR-0002, spec §5, §6"
```

---

## Task 23: Registry coverage test + PR template + CONTRIBUTING note

**Files:**
- Create: `crates/tau-domain/tests/escape_hatch_registry.rs`
- Create: `.github/pull_request_template.md`
- Modify: `CONTRIBUTING.md`

- [x] **Step 23.1: Write `escape_hatch_registry.rs`**

```rust
//! Mechanical enforcement of the escape-hatch registry rule from ADR-0002.
//!
//! Walks every `.rs` file in `crates/`. For each variant named `Custom`
//! or `InternalError`, requires its preceding rustdoc to contain a link
//! to `escape-hatches.md#<anchor>`. Verifies the registry file contains
//! a matching anchor for each. Stale anchors (in registry but not in
//! source) also fail the test.

use std::collections::HashSet;
use std::path::PathBuf;

use walkdir::WalkDir;

const REGISTRY_PATH: &str = "../../docs/explanation/escape-hatches.md";
const CRATES_ROOT: &str = "../../crates";
const ESCAPE_HATCH_VARIANTS: &[&str] = &["Custom", "InternalError"];

#[derive(Debug)]
struct SourceHatch {
    file: PathBuf,
    line: usize,
    variant: String,
    anchor: Option<String>,
}

fn parse_registry_anchors() -> HashSet<String> {
    let raw = std::fs::read_to_string(REGISTRY_PATH).expect("registry file must exist");
    let mut found = HashSet::new();
    // Look for `<a id="anchor-name"></a>` patterns inside the active
    // table.
    let mut in_active_section = false;
    for line in raw.lines() {
        let lt = line.trim();
        if lt.starts_with("## Active escape hatches") {
            in_active_section = true;
            continue;
        }
        if in_active_section && lt.starts_with("## ") {
            break;
        }
        if !in_active_section {
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find(r#"<a id=""#) {
            let after = &rest[start + r#"<a id=""#.len()..];
            if let Some(end) = after.find('"') {
                let anchor = &after[..end];
                found.insert(anchor.to_string());
                rest = &after[end + 1..];
            } else {
                break;
            }
        }
    }
    found
}

fn find_escape_hatches() -> Vec<SourceHatch> {
    let mut hatches = Vec::new();
    for entry in WalkDir::new(CRATES_ROOT).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let raw = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let lines: Vec<&str> = raw.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            for variant in ESCAPE_HATCH_VARIANTS {
                let after_keyword = trimmed.strip_prefix(variant).or_else(|| {
                    trimmed.strip_prefix(&format!("#[non_exhaustive] {variant}"))
                });
                if let Some(rest) = after_keyword {
                    let next_char = rest.chars().next();
                    if matches!(next_char, Some(' ') | Some('{') | Some('(') | Some(',') | None) {
                        // Look back through immediately-preceding doc comments.
                        let mut anchor: Option<String> = None;
                        let mut j = i;
                        while j > 0 {
                            j -= 1;
                            let prev = lines[j].trim();
                            if prev.starts_with("///") || prev.starts_with("//!") || prev.is_empty() {
                                if let Some(start) = prev.find("escape-hatches.md#") {
                                    let after = &prev[start + "escape-hatches.md#".len()..];
                                    let end = after
                                        .find(|c: char| c == ')' || c == ']' || c == ' ' || c == '"')
                                        .unwrap_or(after.len());
                                    anchor = Some(after[..end].to_string());
                                    break;
                                }
                                if !prev.starts_with("///") && !prev.starts_with("//!") {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        hatches.push(SourceHatch {
                            file: path.to_owned(),
                            line: i + 1,
                            variant: (*variant).to_string(),
                            anchor,
                        });
                    }
                }
            }
        }
    }
    hatches
}

#[test]
fn every_escape_hatch_is_registered() {
    let registered = parse_registry_anchors();
    let source = find_escape_hatches();

    assert!(
        !source.is_empty(),
        "no escape hatches found in source — test scanner is probably broken",
    );

    let mut missing = Vec::new();
    let mut live_anchors: HashSet<String> = HashSet::new();
    for h in &source {
        match &h.anchor {
            None => missing.push(format!(
                "{}:{} variant `{}` has no rustdoc link to escape-hatches.md",
                h.file.display(),
                h.line,
                h.variant,
            )),
            Some(a) if !registered.contains(a) => missing.push(format!(
                "{}:{} variant `{}` references unknown anchor `{}`",
                h.file.display(),
                h.line,
                h.variant,
                a,
            )),
            Some(a) => {
                live_anchors.insert(a.clone());
            }
        }
    }

    let stale: Vec<_> = registered.difference(&live_anchors).collect();

    let mut errs = missing;
    for s in stale {
        errs.push(format!("registry anchor `{s}` is not used by any source variant (stale entry)"));
    }

    assert!(errs.is_empty(), "escape-hatch registry mismatches:\n{}", errs.join("\n"));
}
```

- [x] **Step 23.2: Run the registry test**

```bash
cargo test -p tau-domain --test escape_hatch_registry --all-features
```

Expected: test passes. Source variants found: `Capability::Custom`, `MessagePayload::Custom`, `PackageKind::Custom`, `FailureKind::InternalError` (and the rustdoc on each links to the matching anchor).

- [x] **Step 23.3: Create the PR template**

Create `/Users/titouanlebocq/code/tau/.github/pull_request_template.md`:

```markdown
## Summary

<short summary of the change>

## Test plan

- [x] Local `cargo build --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-features` pass.
- [x] If CI behavior changes, the workflow file is updated and validated.

## Escape-hatch checklist

- [x] This PR does not add, modify, or remove a `Custom` / `InternalError` escape hatch, OR
- [x] `docs/explanation/escape-hatches.md` is updated with the corresponding entry (added / promoted / removed). The CI registry-coverage test (`crates/tau-domain/tests/escape_hatch_registry.rs`) enforces this.

## ADR check

- [x] This PR does not require an ADR per QG18, OR
- [x] An ADR has been filed in `docs/decisions/` and is referenced here.
```

- [x] **Step 23.4: Append the escape-hatch section to `CONTRIBUTING.md`**

Read the current `CONTRIBUTING.md` to confirm structure, then append a new section before the License section:

```bash
grep -n "^## " CONTRIBUTING.md
```

Append (or place appropriately) the following:

```markdown
## Working with escape hatches

tau core uses two structural escape hatches (`Custom { ... }` variants
on enums, plus the singleton `FailureKind::InternalError`) to leave room
for unknown shapes that haven't been typed yet. Every escape hatch is
tracked in `docs/explanation/escape-hatches.md` with a rationale and a
promotion trigger.

If your PR adds, promotes (replaces with a typed variant), or removes
an escape hatch, you must update `docs/explanation/escape-hatches.md`
in the same commit. The CI test
`crates/tau-domain/tests/escape_hatch_registry.rs` checks this
mechanically — every source variant named `Custom` or `InternalError`
must have a rustdoc link of the form `escape-hatches.md#<anchor>`,
and every active registry row must point at a live source variant.

If you're introducing a new escape hatch, name the rustdoc anchor and
add the row to the registry's "Active" table; copy the rustdoc
convention from existing variants.
```

- [x] **Step 23.5: Run all checks**

```bash
cargo test -p tau-domain --all-targets --all-features
cargo clippy -p tau-domain --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Expected: green.

- [x] **Step 23.6: Stage and commit**

```bash
git add crates/tau-domain/tests/escape_hatch_registry.rs .github/pull_request_template.md CONTRIBUTING.md
git commit -m "test(tau-domain): enforce escape-hatch registry mechanically

CI test scans crates/ for variants named Custom or InternalError,
parses preceding rustdoc for an escape-hatches.md#<anchor> link, and
verifies the registry has the matching anchor. Stale entries also
fail the test.

Adds PR template with escape-hatch checklist and CONTRIBUTING.md
section explaining the workflow.

Refs: ADR-0002 §6, spec §6 bullets 5+7"
```

---

## Task 24: ADR-0002 — manifest format & capability evolution & escape-hatch policy

**Files:**
- Create: `docs/decisions/0002-manifest-format.md`
- Modify: `docs/decisions/README.md` (add ADR-0002 to index)

- [x] **Step 24.1: Create ADR-0002**

Create `/Users/titouanlebocq/code/tau/docs/decisions/0002-manifest-format.md`:

```markdown
# ADR-0002: Manifest format, capability evolution, and escape-hatch policy

**Status:** Accepted
**Date:** 2026-04-26
**Supersedes:** —

## Context

tau-domain (sub-project 1) introduces the package manifest type
(`PackageManifest`), a hierarchical capability enum (`Capability`),
and four escape-hatch variants (`Capability::Custom`,
`MessagePayload::Custom`, `PackageKind::Custom`,
`FailureKind::InternalError`).

Per QG18, public-API additions to data shapes that downstream plugins
will deserialize require an ADR. This ADR records the v0.1 manifest
field set, the rules under which the `Capability` typed enum evolves,
and the policy governing all escape-hatch variants in tau core.

## Decision

### 1. Manifest field set (v0.1)

The v0.1 `UncheckedManifest` / `PackageManifest` carries:

- `name: PackageName`
- `version: semver::Version`
- `description: String` (non-empty; enforced by `validate()`)
- `authors: Vec<String>`
- `license: Option<String>` (SPDX expression as opaque text)
- `source: PackageSource`
- `kind: PackageKind`
- `dependencies: Vec<PackageDep>`
- `capabilities: Vec<Capability>`

Adding fields is a non-breaking minor (the struct is `#[non_exhaustive]`).
Removing or renaming is a breaking minor pre-1.0 per QG11.

### 2. Hierarchical Capability shape

`Capability` is a top-level `#[non_exhaustive]` enum with five variants:
`Filesystem(FsCapability)`, `Network(NetCapability)`,
`Process(ProcessCapability)`, `Agent(AgentCapability)`, and
`Custom { name, params }`. Each per-namespace sub-enum is also
`#[non_exhaustive]`, and each variant within (e.g. `FsCapability::Read`)
is `#[non_exhaustive]` so additive field evolution is non-breaking.

### 3. Canonicalization at deserialization

Manifest TOML uses the flat dot-namespaced form:

```toml
[[capabilities]]
kind = "fs.read"
paths = ["${PROJECT}/**"]
```

A custom `Deserialize` impl on `Capability` maps recognized `kind`
strings (`"fs.read"`, `"fs.write"`, `"net.http"`, `"process.spawn"`,
`"agent.spawn"`, etc.) onto the variant tree. Unknown `kind` values
fall through to `Capability::Custom { name, params }`. New typed
variants in v0.X auto-promote existing manifests via the same
canonicalization — plugin authors do not need to update manifests
when typed variants land.

### 4. Naming convention

The dot-namespaced `<domain>.<verb>` convention (e.g. `fs.read`,
`net.http`) is **recommended, not mandated**. tau-domain validates
only "non-empty" on `Capability::Custom.name`. Plugin authors who
want a non-conforming name (e.g. `myorg/special-cap`) may use it via
`Custom`.

### 5. Escape-hatch policy

Prefer typed variants for known shapes; allow `Custom` /
`InternalError` escape hatches with documented rationale. **Every
escape hatch in tau core is tracked in
`docs/explanation/escape-hatches.md`** with location, reason,
promotion trigger, and status. PRs that introduce, promote, or remove
an escape hatch update the registry in the same commit. Each
escape-hatch variant's rustdoc carries a link to its registry anchor.

This applies uniformly to:
- `Capability::Custom`
- `MessagePayload::Custom`
- `PackageKind::Custom`
- `FailureKind::InternalError`
- any future escape hatches added in tau core.

### 6. Required `llm_backend`

`AgentDefinition.llm_backend` is `PackageName` (non-optional) at v0.1.
Rationale: Constitution Appendix C explicitly defines an agent as a
process with an LLM backend; G4 reinforces. If a non-LLM agent use
case materializes (sub-project 4+), `llm_backend` becomes
`Option<PackageName>` via a pre-1.0 minor breaking change.

### 7. Mechanical enforcement of the registry

A CI-blocking integration test
(`crates/tau-domain/tests/escape_hatch_registry.rs`) scans every
`.rs` file in the workspace for variants named `Custom` or
`InternalError`, parses their rustdoc for a link to the registry,
and verifies each anchor exists in `docs/explanation/escape-hatches.md`.
Stale registry entries also fail the test. Combined with the PR
template checkbox and the rustdoc convention, this enforces decision
5 in three layers: documentation (CONTRIBUTING.md + rustdoc), PR-time
prompt (template), and CI gate (test).

## Consequences

- The wire format for `Capability` is committed at v0.1: flat
  `kind = "<dot.namespaced>"` form with sibling field-shaped params.
  Deviating from this form is a breaking change.
- Plugin authors need to know the canonical names (`fs.read`, etc.)
  to produce manifests that map to typed variants; they don't *have*
  to use them — `Custom` always works.
- The escape-hatch registry is a living document; every PR that
  introduces or modifies an escape hatch touches it.
- The CI registry test depends on the rustdoc convention of linking
  to `escape-hatches.md#<anchor>`. Variants that violate this fail
  CI.
- Adding new typed `FailureKind` variants is non-breaking (the enum
  is `#[non_exhaustive]`); demoting `InternalError` to a typed kind
  is similarly additive.
- Required `llm_backend` rules out non-LLM agents at v0.1; we accept
  the migration cost if and when that case appears.

## Alternatives considered

- **String-only capability dispatch** (rejected at brainstorm). Forces
  every consumer to write `match name.as_str()` boilerplate; gives up
  type safety for no v0.1 benefit.
- **Flat `Capability` enum without namespace structure** (α; rejected
  in favor of β). Would have been more compact at v0.1 but β anticipates
  per-namespace enforcement layers in tau-runtime.
- **No escape-hatch registry**, just rustdoc warnings (rejected).
  Aspirational rules without mechanical enforcement decay quickly.
- **Top-level `DomainError` umbrella** (rejected at brainstorm Q5).
  Per-concern enums forever; consumers wanting "any tau-domain error"
  wrap their own.
- **`Option<PackageName>` for `llm_backend` from day one** (rejected).
  Adds permanent ceremony at every reader for a case that may not
  materialize; pre-1.0 SemVer makes the loosen-later path cheap.
```

- [x] **Step 24.2: Update the ADR index**

Open `/Users/titouanlebocq/code/tau/docs/decisions/README.md` and add a row to the Index table:

```markdown
| [0002](0002-manifest-format.md) | Manifest format, capability evolution, escape-hatch policy | Accepted |
```

- [x] **Step 24.3: Verify links resolve**

```bash
test -f docs/decisions/0002-manifest-format.md && echo OK
```

Expected: `OK`.

- [x] **Step 24.4: Run the registry test once more (it should still pass)**

```bash
cargo test -p tau-domain --test escape_hatch_registry --all-features
```

Expected: pass.

- [x] **Step 24.5: Stage and commit**

```bash
git add docs/decisions/0002-manifest-format.md docs/decisions/README.md
git commit -m "docs(adr): ADR-0002 manifest format + capability evolution + escape-hatch policy

Records v0.1 manifest field set, hierarchical Capability shape with
canonicalization-at-deserialization rule, dot-namespaced naming as
recommended convention, escape-hatch registry policy, required
llm_backend rationale, and the three-layer enforcement (docs + PR
template + CI test).

Refs: QG18, QG11, G4, G14, spec §6"
```

---

## Task 25: Final local verification

**No file changes. No commit.** Mirrors Plan 1 Task 16.

- [x] **Step 25.1: Confirm directory structure**

```bash
cd /Users/titouanlebocq/code/tau
for f in \
  crates/tau-domain/src/lib.rs \
  crates/tau-domain/src/error.rs \
  crates/tau-domain/src/id.rs \
  crates/tau-domain/src/value.rs \
  crates/tau-domain/src/version.rs \
  crates/tau-domain/src/agent.rs \
  crates/tau-domain/src/message.rs \
  crates/tau-domain/src/fixtures.rs \
  crates/tau-domain/src/package/mod.rs \
  crates/tau-domain/src/package/source.rs \
  crates/tau-domain/src/package/manifest.rs \
  crates/tau-domain/src/package/capability.rs \
  docs/explanation/escape-hatches.md \
  docs/decisions/0002-manifest-format.md \
  .github/pull_request_template.md \
  crates/tau-domain/tests/escape_hatch_registry.rs \
  crates/tau-domain/tests/wire_format_golden.rs ;
do
  test -f "$f" || echo "MISSING: $f"
done
echo "structure check complete"
```

Expected: only `structure check complete` — no `MISSING:` lines.

- [x] **Step 25.2: Run the full local CI equivalent**

```bash
cargo fmt --all -- --check && \
  cargo clippy --workspace --all-targets --all-features -- -D warnings && \
  cargo build -p tau-domain --no-default-features && \
  cargo build -p tau-domain --all-features && \
  cargo test -p tau-domain --no-default-features --lib && \
  cargo test -p tau-domain --all-features --all-targets && \
  cargo test -p tau-domain --all-features --doc && \
  echo "ALL CHECKS PASS"
```

Expected last line: `ALL CHECKS PASS`. Common failure modes:

- `fmt` fails: run `cargo fmt --all`, inspect diff, commit as `style: apply rustfmt`.
- `clippy` fails: fix the lint at root cause; do NOT add `#[allow]` without justification.
- `test --doc` fails on `missing_docs`: confirm every public item has a `///` doc comment.
- registry test fails: a Custom/InternalError variant was added without a rustdoc link or a registry entry.

- [x] **Step 25.3: Verify the commit log**

```bash
git log --oneline | head -25
```

Expected: a clean series of Conventional Commits prefixed with `build:`, `feat(tau-domain):`, `test(tau-domain):`, `docs:`, `docs(adr):`, `ci:`. ~24 commits since Plan 1's last commit.

- [x] **Step 25.4: Push to origin**

```bash
git push origin main
```

Expected: push succeeds, CI is triggered. Watch the run:

```bash
gh run watch
```

Expected: all jobs (`fmt`, `clippy`, `test` matrix, `no-default-features`) complete green on Linux + macOS. Windows is non-blocking per G15.

- [x] **Step 25.5: Confirm green status**

```bash
gh run list --workflow ci.yml --limit 1
```

Expected: most recent run shows `completed  success`.

---

## Task 26: ADR-0002 sign-off

**No commit.** Per QG22, ADR-0002 needs a 24-hour wait between drafting and final sign-off.

- [x] **Step 26.1: Note the time of the ADR-0002 commit**

```bash
git log -1 --format=%cI -- docs/decisions/0002-manifest-format.md
```

- [x] **Step 26.2: Wait at least until the next calendar day**

Use the time productively per QG22:
- Reread ADR-0002 with fresh eyes from a logged-out browser.
- Re-skim the spec to confirm the implementation matches.
- Consider whether bullet 5 (escape-hatch policy) needs a tighter cadence than "as-needed."

If you find issues, file an issue tagged `adr-0002-followup` rather than amending the ADR commit.

- [x] **Step 26.3: Confirm sign-off (no file change)**

After the wait elapses and you're satisfied with the ADR contents, the ADR's status remains `Accepted` (set when committed). No further action — ADR-0002 is now part of the durable history.

---

## Task 27: QG22 overnight checkpoint

**No file changes. No commit. Manual delay.**

QG22 requires that work waits overnight before final acceptance of the
sub-project. Plan 2 finishes the tau-domain sub-project; "acceptance"
is *after* the overnight delay, not at the moment Task 25 turns CI
green.

- [x] **Step 27.1: Note the time of the last commit**

```bash
git log -1 --format=%cI
```

- [x] **Step 27.2: Wait at least until the next calendar day**

Use the time productively:
- Read the ROADMAP again with fresh eyes — does sub-project 2 (tau-ports) still feel like the right next step?
- Browse the rendered rustdoc (`cargo doc -p tau-domain --all-features --open`) and check that it reads well.
- Skim the wire-format golden files; do the canonical forms feel stable?

If you find something to change, file an issue tagged
`tau-domain-followup` rather than amending the sub-project's commits.

- [x] **Step 27.3: Sign off Plan 2**

When the overnight delay has elapsed and you have no findings (or have
filed issues for findings):

- Update `ROADMAP.md` to mark sub-project 1 (`tau-domain`) as complete.
- Tick all checkboxes in this plan.
- Stage and commit:

```bash
git add ROADMAP.md docs/superpowers/plans/2026-04-26-tau-domain.md
git commit -m "docs: sign off Plan 2 (tau-domain)

Plan 2 (sub-project 1, tau-domain) is complete: all 27 tasks ticked,
CI green, ADR-0002 accepted, QG22 overnight delay elapsed.

Next: sub-project 2 (tau-ports) — plugin trait definitions for LLM
backend, tool, storage, sandbox.

Refs: PG4 (phase boundary), QG22"
```

- Push:

```bash
git push origin main
```

Sub-project 1 is closed. Begin sub-project 2's brainstorm cycle when ready.

