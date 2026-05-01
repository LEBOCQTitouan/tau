# REPL Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `tau chat` session persistence (auto-save to JSONL files at `<scope>/sessions/<uuid>.jsonl`) plus a new `tau session` subcommand group (`list`, `show`, `delete`, `export`) and `tau chat --resume <id>` with strict-mode drift validation.

**Architecture:** New `tau-cli/src/session/` module owns JSONL I/O (`store.rs`), UUID v7 ids with prefix resolution (`id.rs`), markdown rendering for `tau session show` (`render.rs`), and shared types (`mod.rs`). The existing `tau chat` REPL turn handler at `crates/tau-cli/src/cmd/chat.rs` gains a `SessionWriter` side-effect (auto-save per turn) and a `--resume <id>` branch (load history → seed REPL). New `cmd/session/` subcommand directory holds `list`, `show`, `delete`, `export` handlers. No tau-runtime changes — runtime stays stateless across CLI invocations per ADR-0006 NG6.

**Tech Stack:** Rust 2021, `uuid = "1"` with `v7` feature (already a workspace dep), existing `serde_json`, `rustyline`, `termimad`, `tempfile` (dev), `assert_cmd` (dev). `tau-domain::Message` already derives `Serialize/Deserialize` behind the `serde` feature.

---

## Plan-erratum (carryover constraints)

Apply preemptively. Do NOT re-derive.

- **Cargo.lock fixup discipline (priority-6 carryover):** if any task adds a new dep that isn't already in the workspace's lockfile, include `Cargo.lock` in the same commit. Task 1 adds `uuid = { workspace = true }` to `tau-cli`'s deps; uuid is already in `Cargo.lock` (used transitively via `tau-domain`), so no `Cargo.lock` change expected. Verify with `git status` after the dep add.

- **`#[non_exhaustive]` discipline:** ALL new public types (`SessionId`, `SessionHeader`, `SessionEntry`, `SessionMetadata`, `SessionError`) get `#[non_exhaustive]`. Doctests on `#[non_exhaustive]` types must be `ignore`-marked.

- **`uuid` crate is already a workspace dep** at `Cargo.toml:44` (`uuid = { version = "1", features = ["v7"] }`). Task 1 just adds it to `crates/tau-cli/Cargo.toml [dependencies]` as `uuid = { workspace = true }`.

- **`tau_domain::Message` derives `Serialize/Deserialize`** behind the `serde` feature. Confirmed enabled in `crates/tau-cli/Cargo.toml:16` (`tau-domain = { workspace = true, features = ["serde"] }`).

- **`Scope::state_path()` returns `&Path`** at `crates/tau-pkg/src/scope.rs:313`. The path is `<scope>/.tau` for project scope and `~/.tau` for global. Sessions go to `<scope.state_path()>/sessions/`.

- **The existing tau chat REPL** is at `crates/tau-cli/src/cmd/chat.rs`. The turn handler at lines 250-307 (post-priority-8 streaming) has TWO branches: streaming (`run_streaming_with_history`) and batch (`run_with_history` via `--no-stream`). Both branches return `RunOutcome::Completed.all_messages` containing the full history including the current turn. Task 5's modification: after a turn returns successfully, compute the delta `&all_messages[history.len()..]` (the suffix that wasn't already in `history` before the call) and call `session_writer.append_messages(delta)?` for each new message.

- **REPL `history: Vec<Message>` lifecycle:** the existing REPL maintains `history` as the running conversation. Auto-save MUST not double-write: messages flow in once per turn (the delta described above). After append, the REPL updates `history = run_outcome.all_messages` as it does today. The session file mirrors `history` after each turn.

- **Resume flow (Task 6):** load JSONL → extract messages → seed `history: Vec<Message>` → enter the existing REPL loop. The first turn after resume calls `runtime.run_streaming_with_history(..., history, new_user_message, ..)` exactly as a fresh turn would. The streaming/batch branches don't need to know they're resuming.

- **`/clear` deprecation:** when the user types `/clear`, print exactly:
  ```
  /clear was removed. Exit (/exit) and re-run `tau chat <agent>` for a fresh session.
  ```
  Continue the REPL loop (no quit). This is one new branch in the existing slash-command match. Existing `/exit`, `/help`, `/history` unchanged. New `/info` branch added.

- **SessionReader robustness:** the session writer holds an open file handle for the REPL's duration. On crash or non-`/exit` termination (Ctrl-C, SIGKILL), the file may end with a partial line. `SessionReader` MUST skip a trailing malformed line gracefully and emit a `tracing::warn!` event named `"session.partial_line_skipped"`.

- **`--ephemeral` path:** when set, the `SessionWriter` is `None` and no file is created. The `/info` slash command shows "(ephemeral; not saved)". The REPL flow is identical otherwise.

- **Slash commands are parsed at the top of each REPL loop iteration** in the existing rustyline-based loop. New slash commands are line-by-line additions to the existing match.

- **JSON event-per-line streaming convention (ADR-0011 carryover):** `tau session list --json` emits one JSON object per stdout line via the existing `Output::json` helper (see priorities 5/6/8). `tau session show --json` is JSONL passthrough (cat-equivalent of the file).

- **Test fixture pattern (priority 5/6/7 carryover):** all CLI integration tests use `assert_cmd::Command::cargo_bin("tau")` + `tempfile::TempDir`. Mirror existing `cmd_uninstall.rs` / `cmd_install.rs` patterns. The shared test infrastructure is in `crates/tau-cli/tests/common/mod.rs`.

- **Three-bucket exit codes (ADR-0007 §7):** session not-found → 2; resume drift without `--force` → 2; ambiguous prefix → 2; success → 0. Reuse the existing `CliError`/`anyhow` → exit-code dispatch.

- **Insta snapshot updates:** Tasks 5, 6, 7, 8, 9, 10 each touch one or more help snapshots. Top-level `tau --help` will need updating once `tau session` subcommand group is declared (Task 7 most likely). Per-command help snapshots (`tau chat --help`, `tau session --help`, `tau session list --help`, etc.) snapshotted in Tasks 7-10. Use `INSTA_UPDATE=always cargo test -p tau-cli --test help_snapshots` to accept; or `cargo insta accept` if the workflow is set up.

- **NO new CI jobs.** No new workspace member; no new external service in CI. Branch protection stays at 23 required checks.

- **No tau-runtime changes.** All work is in `tau-cli`. Per ADR-0006 NG6, persistent agent memory is a CLI concern, not a runtime responsibility.

---

## File structure

| Path | Status | Purpose |
|------|--------|---------|
| `crates/tau-cli/Cargo.toml` | Modify | Task 1: add `uuid = { workspace = true }` to `[dependencies]`. |
| `crates/tau-cli/src/session/mod.rs` | Create | Task 1: module root + public type re-exports + `SessionError` enum (skeleton, populated across Tasks 1-3). |
| `crates/tau-cli/src/session/id.rs` | Create | Task 1: `SessionId` (UUID v7 wrapper), `mint()`, `resolve_id_prefix()`. |
| `crates/tau-cli/src/session/store.rs` | Create | Task 2: `SessionHeader`, `SessionEntry`, `SessionWriter`, `SessionReader`. Task 3: extend with `list_sessions()`, `SessionMetadata`. |
| `crates/tau-cli/src/session/render.rs` | Create | Task 4: pure markdown render of a parsed session. |
| `crates/tau-cli/src/lib.rs` | Modify | Task 1: declare `pub mod session;` (or `mod session;` if internal). Task 7-10: dispatch new `Command::Session` variant. |
| `crates/tau-cli/src/cli.rs` | Modify | Task 5: add `ChatArgs.resume: Option<String>`, `ChatArgs.force: bool`, `ChatArgs.ephemeral: bool`. Task 7: add `Command::Session(SessionArgs)` variant + the `SessionArgs` enum-of-subcommands. |
| `crates/tau-cli/src/cmd/chat.rs` | Modify | Task 5: auto-save side-effect, `--ephemeral` branch, `/info` slash command, `/clear` deprecation. Task 6: `--resume <id>` branch. |
| `crates/tau-cli/src/cmd/session/mod.rs` | Create | Task 7: subcommand dispatch (`list` / `show` / `delete` / `export`). |
| `crates/tau-cli/src/cmd/session/list.rs` | Create | Task 7: `tau session list` handler. |
| `crates/tau-cli/src/cmd/session/show.rs` | Create | Task 8: `tau session show` handler. |
| `crates/tau-cli/src/cmd/session/delete.rs` | Create | Task 9: `tau session delete` handler. |
| `crates/tau-cli/src/cmd/session/export.rs` | Create | Task 10: `tau session export` handler. |
| `crates/tau-cli/src/cmd/mod.rs` | Modify | Task 7: declare `pub mod session;`. |
| `crates/tau-cli/tests/cmd_chat_resume.rs` | Create | Task 5/6: e2e tests for auto-save + resume + drift. |
| `crates/tau-cli/tests/cmd_session_list.rs` | Create | Task 7: e2e tests. |
| `crates/tau-cli/tests/cmd_session_show.rs` | Create | Task 8: e2e tests. |
| `crates/tau-cli/tests/cmd_session_delete.rs` | Create | Task 9: e2e tests. |
| `crates/tau-cli/tests/cmd_session_export.rs` | Create | Task 10: e2e tests. |
| `crates/tau-cli/tests/snapshots/help_snapshots__*.snap` | Modify/Create | Tasks 5-10: insta accepts for new flags + new subcommand group. |
| `docs/decisions/0013-repl-persistence.md` | Create (Task 12) | Full ADR locking the 5 design decisions. |
| `ROADMAP.md` | Modify (Task 12) | Mark Tier 3 priority 11 ✅. |

---

## Task 1: `tau-cli::session::id` module + SessionError skeleton

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/Cargo.toml` — add `uuid = { workspace = true }`.
- Create: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/mod.rs` (module root + SessionError).
- Create: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/id.rs` (SessionId + helpers).
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/lib.rs` — declare `mod session;` (private until Task 5+).

### Steps

- [ ] **Step 1.1: Add `uuid` to tau-cli's deps**

Edit `/Users/titouanlebocq/code/tau/crates/tau-cli/Cargo.toml`. In `[dependencies]`, add (alphabetically, near `tracing` or `toml`):

```toml
# UUID v7 for session ids (ADR-0013 / Tier 3 priority 11).
uuid               = { workspace = true }
```

- [ ] **Step 1.2: Verify build**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
```

Expected: PASS.

- [ ] **Step 1.3: Create the session module root**

Create `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/mod.rs`:

```rust
//! Session storage for `tau chat` REPL persistence (ADR-0013 / Tier 3
//! priority 11).
//!
//! Sessions are JSONL files at `<scope>/sessions/<uuid>.jsonl`:
//! header line first, then `{"type":"message",...}` and
//! `{"type":"turn_summary",...}` lines appended per turn.
//!
//! The `id` module owns UUID v7 generation + prefix resolution.
//! The `store` module owns file I/O (`SessionWriter`, `SessionReader`,
//! `list_sessions`). The `render` module owns markdown rendering for
//! `tau session show`.

use std::path::PathBuf;

pub mod id;

pub use id::{mint, resolve_id_prefix, SessionId};

/// Errors returned by the session storage layer.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// Session id (or prefix) not found in the scope.
    #[error("session {id_or_prefix:?} not found in {scope_path}")]
    NotFound {
        /// What the user typed.
        id_or_prefix: String,
        /// The scope's sessions directory.
        scope_path: PathBuf,
    },

    /// Multiple sessions match the prefix.
    #[error("session prefix {prefix:?} is ambiguous: matches {candidates:?}")]
    AmbiguousPrefix {
        /// Prefix the user typed.
        prefix: String,
        /// All candidate ids (8-char prefixes).
        candidates: Vec<String>,
    },

    /// Session header is missing or malformed.
    #[error("session {id} has invalid header: {detail}")]
    InvalidHeader {
        /// Session id (or filename if header missing entirely).
        id: String,
        /// Free-form description of the problem.
        detail: String,
    },

    /// Schema version is unsupported.
    #[error("session {id} has unsupported schema {schema}; supported: {supported}")]
    UnsupportedSchema {
        /// Session id.
        id: String,
        /// What the file declared.
        schema: u32,
        /// What this binary supports.
        supported: u32,
    },

    /// Resume drift detected (without --force).
    #[error("session {id} drift: {field} was {expected:?}, now {actual:?}")]
    AgentDrift {
        /// Session id.
        id: String,
        /// Drifted field name (e.g. `"package.version"`).
        field: String,
        /// What the session header recorded.
        expected: String,
        /// What the current state resolves to.
        actual: String,
    },

    /// Filesystem I/O error.
    #[error("io error at {path}: {message}")]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Human-readable error message.
        message: String,
    },

    /// JSON parse error.
    #[error("parse error at {path}:{line}: {message}")]
    Parse {
        /// Path involved.
        path: PathBuf,
        /// Line number (1-indexed).
        line: usize,
        /// Human-readable error message.
        message: String,
    },
}
```

- [ ] **Step 1.4: Create `id.rs`**

Create `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/id.rs`:

```rust
//! UUID v7 session ids with prefix resolution.
//!
//! v7 = timestamp-prefixed UUIDs (sortable lexicographically by
//! creation time). Matches the `AgentInstanceId::new()` precedent in
//! `tau_domain`. CLI accepts shortened prefixes (≥8 chars); resolution
//! finds the longest unique match.

use std::fs;
use std::path::Path;

use uuid::Uuid;

use super::SessionError;

/// Minimum prefix length the CLI will accept.
pub const MIN_PREFIX_LEN: usize = 8;

/// A session id wrapping a UUID v7.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(Uuid);

impl SessionId {
    /// Wrap a raw `Uuid` (used by parsers/tests).
    pub fn from_uuid(u: Uuid) -> Self {
        Self(u)
    }

    /// Underlying UUID.
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Full 36-char canonical form.
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    /// 8-char prefix used for displays and stem of the JSONL filename.
    pub fn short(&self) -> String {
        self.0.to_string()[..MIN_PREFIX_LEN].to_string()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for SessionId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// Mint a new session id (UUID v7 — timestamp-prefixed, sortable).
pub fn mint() -> SessionId {
    SessionId(Uuid::now_v7())
}

/// Resolve a user-supplied id-or-prefix to an exact `SessionId` by
/// scanning `<sessions_dir>/*.jsonl`.
///
/// - Exact 36-char match: short-circuits.
/// - 8+ char prefix: matches against canonical filenames; one hit
///   = success, multiple = `AmbiguousPrefix`, zero = `NotFound`.
/// - Anything else: `NotFound`.
///
/// `sessions_dir` is `<scope.state_path()>/sessions`. If the dir does
/// not exist, returns `NotFound`.
pub fn resolve_id_prefix(
    sessions_dir: &Path,
    id_or_prefix: &str,
) -> Result<SessionId, SessionError> {
    // Exact UUID? Skip the directory walk.
    if let Ok(uuid) = id_or_prefix.parse::<Uuid>() {
        let target = sessions_dir.join(format!("{uuid}.jsonl"));
        if target.exists() {
            return Ok(SessionId(uuid));
        }
        return Err(SessionError::NotFound {
            id_or_prefix: id_or_prefix.to_string(),
            scope_path: sessions_dir.to_path_buf(),
        });
    }

    if id_or_prefix.len() < MIN_PREFIX_LEN {
        return Err(SessionError::NotFound {
            id_or_prefix: id_or_prefix.to_string(),
            scope_path: sessions_dir.to_path_buf(),
        });
    }

    if !sessions_dir.exists() {
        return Err(SessionError::NotFound {
            id_or_prefix: id_or_prefix.to_string(),
            scope_path: sessions_dir.to_path_buf(),
        });
    }

    let entries = fs::read_dir(sessions_dir).map_err(|e| SessionError::Io {
        path: sessions_dir.to_path_buf(),
        message: format!("listing sessions dir: {e}"),
    })?;

    let mut matches: Vec<Uuid> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| SessionError::Io {
            path: sessions_dir.to_path_buf(),
            message: format!("reading dir entry: {e}"),
        })?;
        let Some(name) = entry.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".jsonl") else {
            continue;
        };
        if !stem.starts_with(id_or_prefix) {
            continue;
        }
        if let Ok(uuid) = stem.parse::<Uuid>() {
            matches.push(uuid);
        }
    }

    match matches.len() {
        0 => Err(SessionError::NotFound {
            id_or_prefix: id_or_prefix.to_string(),
            scope_path: sessions_dir.to_path_buf(),
        }),
        1 => Ok(SessionId(matches.into_iter().next().unwrap())),
        _ => {
            let candidates = matches
                .iter()
                .map(|u| u.to_string()[..MIN_PREFIX_LEN].to_string())
                .collect();
            Err(SessionError::AmbiguousPrefix {
                prefix: id_or_prefix.to_string(),
                candidates,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn touch_session(dir: &Path, id: &str) {
        fs::write(dir.join(format!("{id}.jsonl")), b"{}\n").unwrap();
    }

    #[test]
    fn mint_returns_v7_uuid() {
        let id1 = mint();
        let id2 = mint();
        // v7 → timestamp-prefixed; sortable; non-equal across calls.
        assert_ne!(id1, id2);
        assert_eq!(id1.as_str().len(), 36);
        assert_eq!(id1.short().len(), 8);
    }

    #[test]
    fn resolve_exact_match_succeeds() {
        let td = TempDir::new().unwrap();
        let id = mint();
        touch_session(td.path(), &id.as_str());
        let got = resolve_id_prefix(td.path(), &id.as_str()).unwrap();
        assert_eq!(got, id);
    }

    #[test]
    fn resolve_prefix_match_succeeds() {
        let td = TempDir::new().unwrap();
        let id = mint();
        touch_session(td.path(), &id.as_str());
        let prefix = &id.as_str()[..10];
        let got = resolve_id_prefix(td.path(), prefix).unwrap();
        assert_eq!(got, id);
    }

    #[test]
    fn resolve_short_prefix_returns_not_found() {
        let td = TempDir::new().unwrap();
        let id = mint();
        touch_session(td.path(), &id.as_str());
        // Less than MIN_PREFIX_LEN chars.
        let err = resolve_id_prefix(td.path(), "abc").unwrap_err();
        assert!(matches!(err, SessionError::NotFound { .. }));
    }

    #[test]
    fn resolve_unknown_id_returns_not_found() {
        let td = TempDir::new().unwrap();
        let id = mint();
        touch_session(td.path(), &id.as_str());
        let err = resolve_id_prefix(td.path(), "00000000").unwrap_err();
        assert!(matches!(err, SessionError::NotFound { .. }));
    }

    #[test]
    fn resolve_ambiguous_prefix_returns_candidates() {
        let td = TempDir::new().unwrap();
        // Two ids that share a known prefix. Use UUIDs constructed by
        // hand so we can guarantee the shared first byte.
        let a = "01234567-0000-7000-8000-000000000001";
        let b = "01234567-0000-7000-8000-000000000002";
        touch_session(td.path(), a);
        touch_session(td.path(), b);
        let err = resolve_id_prefix(td.path(), "01234567").unwrap_err();
        let SessionError::AmbiguousPrefix { candidates, .. } = err else {
            panic!("expected AmbiguousPrefix")
        };
        assert_eq!(candidates.len(), 2);
    }
}
```

- [ ] **Step 1.5: Wire module into lib.rs**

Edit `/Users/titouanlebocq/code/tau/crates/tau-cli/src/lib.rs`. Find an existing `mod` block. Add (alphabetically among mods):

```rust
mod session;
```

(Private — types are pub-from-the-module, but the module itself is internal until later tasks reach for it via `crate::session::...`.)

- [ ] **Step 1.6: Verify**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
cargo test -p tau-cli --all-targets session::id
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-cli --doc
```

Expected: build PASS; 6 unit tests PASS; fmt/clippy/doctest clean.

- [ ] **Step 1.7: Verify Cargo.lock state**

```bash
git -C /Users/titouanlebocq/code/tau status --short
```

`Cargo.lock` should NOT be modified (uuid is already a workspace dep used transitively). If it IS modified, include it in the commit per the priority-6 carryover rule.

- [ ] **Step 1.8: Commit + push**

```bash
git -C /Users/titouanlebocq/code/tau add crates/tau-cli/Cargo.toml crates/tau-cli/src/session/mod.rs crates/tau-cli/src/session/id.rs crates/tau-cli/src/lib.rs
# also Cargo.lock if `git status` shows it modified
git -C /Users/titouanlebocq/code/tau commit -m "$(cat <<'EOF'
feat(cli): add session::id module — UUID v7 + prefix resolution

Foundation for `tau chat --resume` (Tier 3 priority 11). Adds:

- SessionId type wrapping uuid::Uuid (UUID v7, timestamp-prefixed).
- mint() helper using Uuid::now_v7() (matches AgentInstanceId
  precedent in tau-domain).
- resolve_id_prefix(sessions_dir, id_or_prefix) — accepts exact
  UUIDs OR ≥8-char prefixes; resolves to longest unique match.
  NotFound / AmbiguousPrefix error variants.
- SessionError enum skeleton (Io / Parse / NotFound / AmbiguousPrefix
  / InvalidHeader / UnsupportedSchema / AgentDrift).

6 unit tests: mint produces unique v7 UUIDs, exact match, prefix
match, short-prefix rejection, unknown-id NotFound, ambiguous prefix
returns candidates.

Adds uuid = { workspace = true } to tau-cli's deps. Cargo.lock
unchanged (uuid was already pulled in transitively via tau-domain).

Refs: docs/superpowers/specs/2026-05-01-repl-persistence-design.md §2

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git -C /Users/titouanlebocq/code/tau push -u origin feat/repl-persistence-spec
```

---

## Task 2: `tau-cli::session::store` — SessionHeader, SessionEntry, SessionWriter, SessionReader

**Files:**
- Create: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/store.rs`
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/mod.rs` — add `pub mod store;` and re-exports.

### Steps

- [ ] **Step 2.1: Add re-exports to mod.rs**

Edit `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/mod.rs`. After the existing `pub use id::...;` line, add:

```rust
pub mod store;

pub use store::{SessionEntry, SessionHeader, SessionPackage, SessionReader, SessionWriter};
```

- [ ] **Step 2.2: Create store.rs**

Create `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/store.rs`:

```rust
//! JSONL session file I/O.
//!
//! - `SessionHeader` is line 1 of every session file. Schema version,
//!   id, agent metadata, package metadata, llm backend.
//! - `SessionEntry` enumerates the line types found after the header
//!   (`Message` and `TurnSummary`).
//! - `SessionWriter` opens an append-only handle; per-turn calls
//!   append message + turn_summary lines.
//! - `SessionReader` parses an existing file into header + entries.
//!   Tolerates a trailing malformed line (logs a `tracing::warn!` and
//!   continues), so a crashed REPL can still resume.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use tau_domain::Message;

use super::id::SessionId;
use super::SessionError;

/// Current session-file schema version. Bump on breaking changes only.
pub const SCHEMA_VERSION: u32 = 1;

/// Package metadata recorded in the session header for drift checks.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionPackage {
    /// Package name (e.g. `"my-coder-agent"`).
    pub name: String,
    /// Resolved semver at session-creation time.
    pub version: String,
    /// 40-char git commit SHA at session-creation time.
    pub resolved_commit: String,
}

/// Session header — line 1 of every session file.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionHeader {
    /// Schema discriminator: always `"header"`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Schema version. Bump on breaking changes.
    pub schema: u32,
    /// Full UUID v7 string.
    pub id: String,
    /// RFC 3339 timestamp at session creation.
    pub created_at: String,
    /// Named entry in the project tau.toml (e.g. `"coder"`).
    pub agent_id: String,
    /// Package metadata for drift checks.
    pub package: SessionPackage,
    /// LLM backend package name (e.g. `"anthropic"`).
    pub llm_backend: String,
    /// User-supplied title (deferred polish; v0.1 always None).
    #[serde(default)]
    pub title: Option<String>,
}

impl SessionHeader {
    /// Construct a fresh header for a new session.
    pub fn new(
        id: &SessionId,
        agent_id: String,
        package: SessionPackage,
        llm_backend: String,
    ) -> Self {
        Self {
            kind: "header".to_string(),
            schema: SCHEMA_VERSION,
            id: id.as_str(),
            created_at: humantime::format_rfc3339_seconds(SystemTime::now()).to_string(),
            agent_id,
            package,
            llm_backend,
            title: None,
        }
    }
}

/// Parsed entries that follow the header in a session file.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum SessionEntry {
    /// A conversation message (`{"type":"message","msg":<Message>}`).
    Message(Message),
    /// Per-turn metadata (`{"type":"turn_summary","turn":N,...}`).
    TurnSummary {
        /// 1-indexed turn number.
        turn: u32,
        /// Stop reason as a string (e.g. `"EndTurn"`, `"ToolUse"`).
        stop_reason: String,
        /// Optional input/output token counts; `None` if absent.
        input_tokens: Option<u64>,
        /// Optional output tokens.
        output_tokens: Option<u64>,
    },
}

/// Append-only writer. Owns an open file handle for the REPL's
/// duration.
pub struct SessionWriter {
    file: File,
    path: PathBuf,
}

impl SessionWriter {
    /// Create a new session file at `<sessions_dir>/<id>.jsonl`.
    /// Writes the header line as the first line. Creates
    /// `<sessions_dir>` if missing.
    pub fn create(
        sessions_dir: &Path,
        id: &SessionId,
        header: &SessionHeader,
    ) -> Result<Self, SessionError> {
        std::fs::create_dir_all(sessions_dir).map_err(|e| SessionError::Io {
            path: sessions_dir.to_path_buf(),
            message: format!("creating sessions dir: {e}"),
        })?;
        let path = sessions_dir.join(format!("{}.jsonl", id.as_str()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| SessionError::Io {
                path: path.clone(),
                message: format!("creating session file: {e}"),
            })?;

        let header_line = serde_json::to_string(header).map_err(|e| SessionError::Io {
            path: path.clone(),
            message: format!("serializing header: {e}"),
        })?;
        file.write_all(header_line.as_bytes()).map_err(|e| SessionError::Io {
            path: path.clone(),
            message: format!("writing header: {e}"),
        })?;
        file.write_all(b"\n").map_err(|e| SessionError::Io {
            path: path.clone(),
            message: format!("writing header newline: {e}"),
        })?;

        Ok(Self { file, path })
    }

    /// Open an existing session file in append mode (used after
    /// resume).
    pub fn open_append(path: &Path) -> Result<Self, SessionError> {
        let file = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|e| SessionError::Io {
                path: path.to_path_buf(),
                message: format!("opening for append: {e}"),
            })?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }

    /// Append one message line.
    pub fn append_message(&mut self, msg: &Message) -> Result<(), SessionError> {
        #[derive(Serialize)]
        struct Wire<'a> {
            #[serde(rename = "type")]
            kind: &'static str,
            msg: &'a Message,
        }
        let w = Wire {
            kind: "message",
            msg,
        };
        let line = serde_json::to_string(&w).map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("serializing message: {e}"),
        })?;
        self.file.write_all(line.as_bytes()).map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("writing message: {e}"),
        })?;
        self.file.write_all(b"\n").map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("writing message newline: {e}"),
        })?;
        Ok(())
    }

    /// Append several message lines (one per turn's worth of new
    /// messages).
    pub fn append_messages(&mut self, msgs: &[Message]) -> Result<(), SessionError> {
        for m in msgs {
            self.append_message(m)?;
        }
        Ok(())
    }

    /// Append a turn-summary line.
    pub fn append_turn_summary(
        &mut self,
        turn: u32,
        stop_reason: &str,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
    ) -> Result<(), SessionError> {
        #[derive(Serialize)]
        struct Wire<'a> {
            #[serde(rename = "type")]
            kind: &'static str,
            turn: u32,
            stop_reason: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            input_tokens: Option<u64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            output_tokens: Option<u64>,
        }
        let w = Wire {
            kind: "turn_summary",
            turn,
            stop_reason,
            input_tokens,
            output_tokens,
        };
        let line = serde_json::to_string(&w).map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("serializing turn_summary: {e}"),
        })?;
        self.file.write_all(line.as_bytes()).map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("writing turn_summary: {e}"),
        })?;
        self.file.write_all(b"\n").map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("writing turn_summary newline: {e}"),
        })?;
        Ok(())
    }

    /// Flush and close (consumes self).
    pub fn close(mut self) -> Result<(), SessionError> {
        self.file.flush().map_err(|e| SessionError::Io {
            path: self.path.clone(),
            message: format!("flushing on close: {e}"),
        })?;
        Ok(())
    }

    /// Path to the session file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Parser for an existing session file.
pub struct SessionReader;

impl SessionReader {
    /// Open and parse a session file. Returns the header + every
    /// non-header entry. Skips a trailing malformed line gracefully
    /// (logs `tracing::warn!`).
    pub fn read(path: &Path) -> Result<(SessionHeader, Vec<SessionEntry>), SessionError> {
        let file = File::open(path).map_err(|e| SessionError::Io {
            path: path.to_path_buf(),
            message: format!("opening for read: {e}"),
        })?;
        let reader = BufReader::new(file);

        let mut lines = reader.lines();
        let header_line = lines.next().ok_or_else(|| SessionError::InvalidHeader {
            id: file_stem(path),
            detail: "empty file".to_string(),
        })?;
        let header_line = header_line.map_err(|e| SessionError::Io {
            path: path.to_path_buf(),
            message: format!("reading header line: {e}"),
        })?;
        let header: SessionHeader =
            serde_json::from_str(&header_line).map_err(|e| SessionError::InvalidHeader {
                id: file_stem(path),
                detail: format!("parsing header JSON: {e}"),
            })?;

        if header.kind != "header" {
            return Err(SessionError::InvalidHeader {
                id: file_stem(path),
                detail: format!("first line type is {:?}, expected \"header\"", header.kind),
            });
        }
        if header.schema != SCHEMA_VERSION {
            return Err(SessionError::UnsupportedSchema {
                id: header.id.clone(),
                schema: header.schema,
                supported: SCHEMA_VERSION,
            });
        }

        let mut entries = Vec::new();
        let mut line_no = 1usize;
        let collected: Vec<_> = lines.collect();
        let total = collected.len();
        for (idx, line) in collected.into_iter().enumerate() {
            line_no = idx + 2;
            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    return Err(SessionError::Io {
                        path: path.to_path_buf(),
                        message: format!("reading line {line_no}: {e}"),
                    });
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            match parse_entry(&line) {
                Ok(entry) => entries.push(entry),
                Err(_) if idx + 1 == total => {
                    tracing::warn!(
                        name = "session.partial_line_skipped",
                        path = %path.display(),
                        line_no = line_no,
                        "trailing malformed line skipped (likely crashed mid-write)"
                    );
                }
                Err(e) => {
                    return Err(SessionError::Parse {
                        path: path.to_path_buf(),
                        line: line_no,
                        message: e,
                    });
                }
            }
        }

        Ok((header, entries))
    }
}

fn parse_entry(line: &str) -> Result<SessionEntry, String> {
    #[derive(Deserialize)]
    struct Discriminator {
        #[serde(rename = "type")]
        kind: String,
    }
    let disc: Discriminator =
        serde_json::from_str(line).map_err(|e| format!("discriminator: {e}"))?;
    match disc.kind.as_str() {
        "message" => {
            #[derive(Deserialize)]
            struct Wire {
                msg: Message,
            }
            let w: Wire = serde_json::from_str(line).map_err(|e| format!("message body: {e}"))?;
            Ok(SessionEntry::Message(w.msg))
        }
        "turn_summary" => {
            #[derive(Deserialize)]
            struct Wire {
                turn: u32,
                stop_reason: String,
                #[serde(default)]
                input_tokens: Option<u64>,
                #[serde(default)]
                output_tokens: Option<u64>,
            }
            let w: Wire =
                serde_json::from_str(line).map_err(|e| format!("turn_summary body: {e}"))?;
            Ok(SessionEntry::TurnSummary {
                turn: w.turn,
                stop_reason: w.stop_reason,
                input_tokens: w.input_tokens,
                output_tokens: w.output_tokens,
            })
        }
        other => Err(format!("unknown entry type: {other}")),
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("<unknown>")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tau_domain::{Address, MessagePayload};
    use tempfile::TempDir;

    fn fixture_header(id: &SessionId) -> SessionHeader {
        SessionHeader::new(
            id,
            "coder".to_string(),
            SessionPackage {
                name: "my-coder-agent".to_string(),
                version: "1.0.0".to_string(),
                resolved_commit: "0".repeat(40),
            },
            "anthropic".to_string(),
        )
    }

    fn user_msg(text: &str) -> Message {
        Message::new(
            Address::User,
            Address::User,
            MessagePayload::Text {
                content: text.to_string(),
            },
        )
    }

    #[test]
    fn writer_create_writes_header_line() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let header = fixture_header(&id);
        let writer = SessionWriter::create(td.path(), &id, &header).unwrap();
        let path = writer.path().to_path_buf();
        writer.close().unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<_> = contents.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains(r#""type":"header""#));
    }

    #[test]
    fn writer_append_message_adds_one_line() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let header = fixture_header(&id);
        let mut writer = SessionWriter::create(td.path(), &id, &header).unwrap();
        let path = writer.path().to_path_buf();
        writer.append_message(&user_msg("hello")).unwrap();
        writer.close().unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents.lines().count(), 2);
    }

    #[test]
    fn writer_append_turn_summary_serializes_optional_fields() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let header = fixture_header(&id);
        let mut writer = SessionWriter::create(td.path(), &id, &header).unwrap();
        let path = writer.path().to_path_buf();
        writer.append_turn_summary(1, "EndTurn", Some(10), Some(5)).unwrap();
        writer.close().unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        let last = contents.lines().last().unwrap();
        assert!(last.contains(r#""input_tokens":10"#));
        assert!(last.contains(r#""output_tokens":5"#));
    }

    #[test]
    fn reader_round_trips_header_and_messages() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let header_in = fixture_header(&id);
        let mut writer = SessionWriter::create(td.path(), &id, &header_in).unwrap();
        let path = writer.path().to_path_buf();
        writer.append_message(&user_msg("hello")).unwrap();
        writer.append_message(&user_msg("world")).unwrap();
        writer.append_turn_summary(1, "EndTurn", Some(7), Some(3)).unwrap();
        writer.close().unwrap();

        let (header_out, entries) = SessionReader::read(&path).unwrap();
        assert_eq!(header_in, header_out);
        assert_eq!(entries.len(), 3);
        assert!(matches!(entries[0], SessionEntry::Message(_)));
        assert!(matches!(entries[1], SessionEntry::Message(_)));
        let SessionEntry::TurnSummary { turn, .. } = &entries[2] else {
            panic!("expected TurnSummary")
        };
        assert_eq!(*turn, 1);
    }

    #[test]
    fn reader_rejects_unsupported_schema() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let path = td.path().join(format!("{}.jsonl", id.as_str()));
        std::fs::write(
            &path,
            r#"{"type":"header","schema":99,"id":"x","created_at":"x","agent_id":"x","package":{"name":"x","version":"x","resolved_commit":"x"},"llm_backend":"x"}
"#,
        )
        .unwrap();
        let err = SessionReader::read(&path).unwrap_err();
        assert!(matches!(err, SessionError::UnsupportedSchema { .. }));
    }

    #[test]
    fn reader_skips_trailing_malformed_line() {
        let td = TempDir::new().unwrap();
        let id = crate::session::id::mint();
        let header = fixture_header(&id);
        let mut writer = SessionWriter::create(td.path(), &id, &header).unwrap();
        let path = writer.path().to_path_buf();
        writer.append_message(&user_msg("hello")).unwrap();
        writer.close().unwrap();
        // Append a partial line as if a crash happened mid-write.
        let mut handle = OpenOptions::new().append(true).open(&path).unwrap();
        handle.write_all(b"{not json").unwrap();
        drop(handle);

        let (_, entries) = SessionReader::read(&path).unwrap();
        // First message survives; trailing malformed line skipped.
        assert_eq!(entries.len(), 1);
    }
}
```

NOTE: the imports above assume `tau_domain::{Address, MessagePayload, Message}` are publicly available from the test's perspective. Confirm by inspecting `crates/tau-domain/src/message.rs` for the public surface; adjust constructors if `Message::new` has a different signature.

NOTE: `humantime` is a dep of `tau-pkg` (humantime-serde at root workspace). Verify it's in `tau-cli`'s deps or transitively available; if `humantime::format_rfc3339_seconds` isn't reachable, replace with `chrono::Utc::now().to_rfc3339()` (chrono is also a workspace dep) or with `time::OffsetDateTime::now_utc().format(&Rfc3339)` (time is in workspace too). Pick whichever is already in `tau-cli`'s deps.

- [ ] **Step 2.3: Verify**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
cargo test -p tau-cli --all-targets session::store
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-cli --doc
```

Expected: 6 unit tests PASS; fmt/clippy/doctest clean.

- [ ] **Step 2.4: Commit + push**

```bash
git -C /Users/titouanlebocq/code/tau add crates/tau-cli/src/session/mod.rs crates/tau-cli/src/session/store.rs
git -C /Users/titouanlebocq/code/tau commit -m "$(cat <<'EOF'
feat(cli): add session::store module — JSONL writer + reader

JSONL session file I/O for `tau chat` persistence (Tier 3 priority
11). Adds:

- SessionPackage / SessionHeader / SessionEntry types
  (#[non_exhaustive] + serde-derived).
- SessionWriter: create(), open_append(), append_message(),
  append_messages(), append_turn_summary(), close(). Holds an open
  file handle for the REPL's duration.
- SessionReader::read(path) -> (header, Vec<entries>). Rejects
  unsupported schema versions (UnsupportedSchema). Skips a trailing
  malformed line gracefully (logs tracing::warn!
  "session.partial_line_skipped") so a crashed REPL is still
  resumable.

Schema version 1; bump on breaking changes.

6 unit tests: header line write, message append count, turn_summary
optional fields, full round-trip, schema rejection, partial-line
recovery.

Refs: docs/superpowers/specs/2026-05-01-repl-persistence-design.md §1

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git -C /Users/titouanlebocq/code/tau push
```

---

## Task 3: `tau-cli::session::store::list_sessions` + `SessionMetadata`

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/store.rs` — add `SessionMetadata` struct + `list_sessions(scope) -> Vec<SessionMetadata>`.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/mod.rs` — re-export `SessionMetadata`, `list_sessions`.

### Steps

- [ ] **Step 3.1: Extend store.rs with `SessionMetadata` type**

Append to `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/store.rs` (after the `SessionReader` impl, before `#[cfg(test)] mod tests`):

```rust
/// Lightweight session summary used by `tau session list`.
///
/// Built from the file's header line + a count of subsequent
/// non-header lines. Reads the whole file (cheap for typical
/// session sizes < 10MB).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SessionMetadata {
    /// Full session id.
    pub id: String,
    /// 8-char id prefix.
    pub short: String,
    /// Agent named entry from project tau.toml.
    pub agent_id: String,
    /// Created-at timestamp (RFC 3339, copied from header).
    pub created_at: String,
    /// Number of `message` + `turn_summary` lines (best-effort; on
    /// read errors falls back to `0`).
    pub turn_count: u32,
    /// Optional title (always None at v0.1).
    pub title: Option<String>,
    /// Path to the session file.
    pub path: PathBuf,
}

/// List session metadata for every `*.jsonl` in `<sessions_dir>`.
///
/// Sort is descending by `created_at`. `agent_filter` filters by
/// `header.agent_id` if `Some(name)`. Files that fail to parse the
/// header line are silently skipped (logged at `warn`).
pub fn list_sessions(
    sessions_dir: &Path,
    agent_filter: Option<&str>,
) -> Result<Vec<SessionMetadata>, SessionError> {
    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(sessions_dir).map_err(|e| SessionError::Io {
        path: sessions_dir.to_path_buf(),
        message: format!("listing sessions dir: {e}"),
    })?;

    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| SessionError::Io {
            path: sessions_dir.to_path_buf(),
            message: format!("reading dir entry: {e}"),
        })?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }

        let (header, count) = match SessionReader::read(&path) {
            Ok((h, entries)) => (h, entries.len() as u32),
            Err(e) => {
                tracing::warn!(
                    name = "session.list_skipped",
                    path = %path.display(),
                    error = %e,
                    "skipping malformed session file"
                );
                continue;
            }
        };

        if let Some(filter) = agent_filter {
            if header.agent_id != filter {
                continue;
            }
        }

        let short = header.id[..super::id::MIN_PREFIX_LEN].to_string();
        out.push(SessionMetadata {
            id: header.id,
            short,
            agent_id: header.agent_id,
            created_at: header.created_at,
            turn_count: count,
            title: header.title,
            path,
        });
    }

    // Descending by created_at (RFC 3339 strings sort
    // chronologically when zero-padded).
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}
```

- [ ] **Step 3.2: Re-export from mod.rs**

Edit `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/mod.rs`. Update the re-export line:

```rust
pub use store::{
    list_sessions, SessionEntry, SessionHeader, SessionMetadata, SessionPackage, SessionReader,
    SessionWriter,
};
```

- [ ] **Step 3.3: Add 3 unit tests**

Append inside the existing `#[cfg(test)] mod tests` block in `store.rs`:

```rust
    #[test]
    fn list_sessions_empty_dir_returns_empty() {
        let td = TempDir::new().unwrap();
        let result = list_sessions(td.path(), None).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn list_sessions_returns_descending_by_created_at() {
        let td = TempDir::new().unwrap();
        let id_a = crate::session::id::mint();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let id_b = crate::session::id::mint();
        let header_a = fixture_header(&id_a);
        let header_b = fixture_header(&id_b);

        SessionWriter::create(td.path(), &id_a, &header_a)
            .unwrap()
            .close()
            .unwrap();
        SessionWriter::create(td.path(), &id_b, &header_b)
            .unwrap()
            .close()
            .unwrap();

        let result = list_sessions(td.path(), None).unwrap();
        assert_eq!(result.len(), 2);
        // b is newer, so it comes first.
        assert_eq!(result[0].id, id_b.as_str());
        assert_eq!(result[1].id, id_a.as_str());
    }

    #[test]
    fn list_sessions_filters_by_agent() {
        let td = TempDir::new().unwrap();
        let id_a = crate::session::id::mint();
        let id_b = crate::session::id::mint();

        let mut header_a = fixture_header(&id_a);
        header_a.agent_id = "coder".into();
        let mut header_b = fixture_header(&id_b);
        header_b.agent_id = "notes".into();

        SessionWriter::create(td.path(), &id_a, &header_a)
            .unwrap()
            .close()
            .unwrap();
        SessionWriter::create(td.path(), &id_b, &header_b)
            .unwrap()
            .close()
            .unwrap();

        let result = list_sessions(td.path(), Some("coder")).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].agent_id, "coder");
    }
```

- [ ] **Step 3.4: Verify**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
cargo test -p tau-cli --all-targets session::store
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-cli --doc
```

Expected: 9 store tests pass (6 from Task 2 + 3 new).

- [ ] **Step 3.5: Commit + push**

```bash
git -C /Users/titouanlebocq/code/tau add crates/tau-cli/src/session/store.rs crates/tau-cli/src/session/mod.rs
git -C /Users/titouanlebocq/code/tau commit -m "$(cat <<'EOF'
feat(cli): add session::store::list_sessions + SessionMetadata

`tau session list` foundation. Walks <sessions_dir>/*.jsonl, parses
each file's header (best-effort; skips malformed with a tracing
warn), sorts descending by created_at, and optionally filters by
agent_id.

SessionMetadata carries: id, 8-char short prefix, agent_id,
created_at, turn_count (count of post-header lines), title (always
None at v0.1), path.

3 new unit tests: empty dir → empty result, descending sort by
created_at, agent filter applied.

Refs: docs/superpowers/specs/2026-05-01-repl-persistence-design.md §1, §5.3

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git -C /Users/titouanlebocq/code/tau push
```

---

## Task 4: `tau-cli::session::render` — markdown render for `tau session show`

**Hybrid format.**

**Files:**
- Create: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/render.rs`
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/session/mod.rs` — add `pub mod render;` and re-export `render_session`.

**Spec sections:** §5.3 ("`tau session show <id>` (human, markdown via termimad)").

### Per-task summary

1. **Function:** `pub fn render_session(header: &SessionHeader, entries: &[SessionEntry]) -> String`. Pure, no I/O. Returns a markdown string suitable for `print!` to stdout (rendered by termimad downstream OR consumed verbatim by `tau session export --format md`).

2. **Output shape** (per spec §5.3 example):
   ```
   # Session <full-uuid>
   **Agent:** <agent_id> (<package.name>@<package.version>)
   **Started:** <created_at>
   **Turns:** <turn_count>

   ---

   **You:** <text>

   **<agent_id>:** [calls <tool> with <args-json>]

   **<tool>:** [returned content]

   **<agent_id>:** <text>
   ```
   - User text payloads → `**You:** <text>`.
   - Agent text payloads → `**<agent_id>:** <text>`.
   - ToolCall payloads → `**<agent_id>:** [calls <tool_name> with <args-json>]`.
   - ToolResult payloads → `**<tool_name>:** [returned content]` (or first 200 chars + ellipsis if long).
   - ToolError payloads → `**<tool_name>:** [error: <message>]`.
   - TurnSummary entries → optional one-line `*Turn N: <stop_reason>, <input_tokens>+<output_tokens> tokens*` between turns.

3. **Helper functions:**
   - `format_message(agent_id: &str, msg: &Message) -> String` — handles all `MessagePayload` variants. Lifecycle variants (rare in chat sessions) get a debug-formatted fallback.
   - Use the `Address` enum to distinguish User/Agent/Tool senders.

4. **3 unit tests:**
   - `render_text_only_session` — header + 2 user/agent text turns. Snapshot the output.
   - `render_with_tool_calls` — text + tool_use + tool_result + text. Snapshot.
   - `render_with_turn_summary_lines` — same plus turn_summary entries inserted between turns.

5. **Tests use inline string assertions or `insta::assert_snapshot!`** (insta is already a dev-dep). Pick whichever fits the codebase's pattern; `cmd_install.rs` and friends use insta extensively. Insta snapshot files would go to `crates/tau-cli/src/snapshots/` if they're inline tests OR `tests/snapshots/` if they're integration tests. Stick with inline `assert_eq!` for v0.1 to avoid managing yet another set of insta files.

6. **Verification:** standard suite + `cargo test -p tau-cli session::render`.

7. **Commit message:** `feat(cli): add session::render — markdown for tau session show`.

8. Push.

---

## Task 5: `cmd::chat` integration — auto-save, --ephemeral, /info, drop /clear

**Hybrid format.**

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/cli.rs` — add `ChatArgs.ephemeral: bool` (the `--resume` and `--force` flags land in Task 6).
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/cmd/chat.rs` — auto-save side-effect, /info, /clear deprecation.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/lib.rs` — `mod session;` → `pub(crate) mod session;` if Task 1 used `mod session;` (Task 5 is the first internal consumer).
- Create: `/Users/titouanlebocq/code/tau/crates/tau-cli/tests/cmd_chat_persistence.rs` — 3 integration tests for auto-save behavior. (Resume tests land in Task 6's e2e file.)

**Spec sections:** §4 (auto-save default), §6 (slash command revision).

### Per-task summary

1. **`ChatArgs.ephemeral: bool`** with clap `#[arg(long)]`. Doc: "Don't persist this session to disk; in-memory only."

2. **Inside `cmd/chat.rs::run` (or equivalent entry function):**
   - Mint a new `SessionId` early (via `session::id::mint()`).
   - Resolve scope as today.
   - Resolve agent definition + package metadata (existing flow gives you agent_id + package.name + package.version + resolved_commit + llm_backend).
   - Build a `SessionHeader::new(...)`.
   - Open `SessionWriter::create(scope.state_path().join("sessions"), &id, &header)?` UNLESS `args.ephemeral` is true. Wrap in `Option<SessionWriter>`.
   - Print one of:
     ```
     ✓ Session: <short-prefix> (<scope kind>)
     ⚠ Ephemeral session — not saved to disk
     ```

3. **REPL turn handler (lines 250-307 area):**
   - The existing flow calls `runtime.run_streaming_with_history(...)` (or `run_with_history` if `--no-stream`). Both return a `RunOutcome` with `all_messages: Vec<Message>`.
   - On success, BEFORE updating `history`:
     ```rust
     let new_messages = &all_messages[history.len()..];
     if let Some(writer) = session_writer.as_mut() {
         writer.append_messages(new_messages)?;
         // Append turn summary if available
         if let Some(usage) = token_usage_for_this_turn {
             writer.append_turn_summary(turn_number, &format!("{stop_reason:?}"), Some(usage.input_tokens), Some(usage.output_tokens))?;
         }
     }
     ```
   - Then `history = all_messages.clone();` as today.
   - On error: `history` stays as-is (the user's prompt was already appended via the runtime's internal flow OR by the chat REPL itself — verify which; if the user message was appended to `history` BEFORE the runtime call, also append it to the writer BEFORE the runtime call so partial turns survive a crash mid-runtime).

4. **Slash command handlers** (in the existing match):
   - `/info`: print
     ```
     Session: <full-uuid> [(ephemeral; not saved)]
     File: <path-or-blank>
     Turns: <count>
     Started: <header.created_at>
     Agent: <agent_id> (<package.name>@<package.version>)
     ```
     For ephemeral: skip the File line; suffix the id with "(ephemeral; not saved)".
   - `/clear` (existing branch, replace content): print
     ```
     /clear was removed. Exit (/exit) and re-run `tau chat <agent>` for a fresh session.
     ```
     Continue the loop (no quit, no in-memory clear).

5. **On REPL exit (`/exit` branch)**:
   - If `Some(writer)`: `writer.close()?` then print `Session saved. Resume with: tau chat <agent> --resume <short-prefix>`.
   - If `None` (ephemeral): print `Session discarded.`.

6. **3 integration tests** in `tests/cmd_chat_persistence.rs`:
   - `chat_creates_session_file` — `tau chat echo` (echo-llm fixture) with one prompt; assert `<scope>/sessions/*.jsonl` contains exactly 1 file with a header line + ≥1 message lines.
   - `chat_ephemeral_writes_no_file` — `tau chat echo --ephemeral`; assert `<scope>/sessions/` is empty (or doesn't exist).
   - `chat_clear_prints_deprecation_message` — drive stdin with `/clear\n/exit\n`; assert stdout contains "/clear was removed".

7. **Help snapshot updates:** `tau chat --help` snapshot regenerates (new `--ephemeral` flag).

8. **Verification:** standard suite + `cargo test -p tau-cli --test cmd_chat_persistence` + `cargo test -p tau-cli --test help_snapshots`.

9. **Commit message:** `feat(cli): tau chat auto-save + /info slash command + drop /clear`.

10. Push.

---

## Task 6: `cmd::chat --resume <id>` — drift validation, history replay, --force

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` — add `ChatArgs.resume: Option<String>`, `ChatArgs.force: bool`.
- Modify: `crates/tau-cli/src/cmd/chat.rs` — resume branch.
- Create: `crates/tau-cli/tests/cmd_chat_resume.rs` — 4 e2e tests.

**Spec sections:** §3.1 (drift detection algorithm), §5.1 (chat surface).

### Per-task summary

1. **`ChatArgs.resume: Option<String>`** + `ChatArgs.force: bool`. Clap docs: `resume = "Resume an existing session (id or 8+ char prefix)"`, `force = "Override drift detection on resume"`.

2. **At chat startup, BEFORE minting a new session id:**
   - If `args.resume.is_some()`:
     - `let sessions_dir = scope.state_path().join("sessions");`
     - `let id = session::id::resolve_id_prefix(&sessions_dir, args.resume.as_ref().unwrap())?;`
     - `let path = sessions_dir.join(format!("{}.jsonl", id.as_str()));`
     - `let (header, entries) = session::store::SessionReader::read(&path)?;`
     - **Drift validation** (UNLESS `args.force`):
       - Resolve current state from project tau.toml: agent_def, package_manifest, llm_backend.
       - Compare header fields to current:
         - `header.agent_id` vs `agent_def.id` (or whatever the named entry is)
         - `header.package.name` vs `package_manifest.name`
         - `header.package.version` vs `package_manifest.version`
         - `header.llm_backend` vs `agent_def.llm_backend`
       - On any mismatch: return `SessionError::AgentDrift { id, field, expected, actual }` and exit 2 with the printed error from spec §3.1.
     - With `--force`: print warning `⚠ resuming with X@Y (session was X@Z)` for any drifted field, then proceed.
     - Seed `history: Vec<Message> = entries.iter().filter_map(|e| match e { SessionEntry::Message(m) => Some(m.clone()), _ => None }).collect();`
     - Reopen the file in append mode: `let writer = SessionWriter::open_append(&path)?;`
     - Print `✓ Resumed session <short-prefix> (<turn_count> turns, last activity <duration> ago)`.
   - Else (no resume): mint new id and proceed as Task 5 set up.

3. **REPL loop is unchanged from Task 5.** The first turn after resume calls the runtime exactly like a fresh turn — `history` is just pre-populated.

4. **4 e2e tests** in `cmd_chat_resume.rs`:
   - `chat_resume_loads_history` — start a session via `tau chat echo`, exit, then `tau chat echo --resume <prefix>`; assert the second invocation's transcript shows the prior conversation context.
   - `chat_resume_strict_drift_exits_2` — create a session via `tau chat echo`, then mutate the project tau.toml to point the agent at a different package version; `tau chat echo --resume <prefix>` (without --force) → exit 2; stderr contains "drift".
   - `chat_resume_force_bypasses_drift` — same setup; `--force` flag → exit 0 + warning printed.
   - `chat_resume_unknown_id_exits_2` — `tau chat echo --resume 00000000` → exit 2; stderr contains "not found".

5. **Help snapshot:** `tau chat --help` regenerates (new `--resume`, `--force` flags).

6. **Verification:** standard suite + `cargo test -p tau-cli --test cmd_chat_resume`.

7. **Commit message:** `feat(cli): tau chat --resume <id> with strict drift validation`.

8. Push.

---

## Task 7: `tau session list` subcommand + e2e tests

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` — add `Command::Session(SessionArgs)` variant + `SessionArgs` enum-of-subcommands (clap `Subcommand`-derived) with the `List` variant. Other variants (`Show`, `Delete`, `Export`) land in Tasks 8-10.
- Create: `crates/tau-cli/src/cmd/session/mod.rs` — dispatch.
- Create: `crates/tau-cli/src/cmd/session/list.rs` — handler.
- Modify: `crates/tau-cli/src/cmd/mod.rs` — declare `pub mod session;`.
- Modify: `crates/tau-cli/src/lib.rs` — dispatch `Command::Session(...)`.
- Create: `crates/tau-cli/tests/cmd_session_list.rs` — 3+ e2e tests.

**Spec sections:** §5.2, §5.3.

### Per-task summary

1. **CLI shape** (mirror `tau plugin` group):
   ```rust
   #[derive(Args, Debug)]
   pub struct SessionArgs {
       #[command(subcommand)]
       pub action: SessionAction,
   }

   #[derive(Subcommand, Debug)]
   pub enum SessionAction {
       /// List sessions in the current scope.
       List(SessionListArgs),
       // Show, Delete, Export added in tasks 8-10.
   }

   #[derive(Args, Debug)]
   pub struct SessionListArgs {
       /// Filter by agent name.
       pub agent: Option<String>,
       /// Use global scope.
       #[arg(long)]
       pub global: bool,
       /// Maximum sessions to display (default 20).
       #[arg(long, default_value_t = 20)]
       pub limit: usize,
       /// Disable the limit; show all.
       #[arg(long)]
       pub all: bool,
   }
   ```

2. **`cmd/session/list.rs::run(args, output)`:**
   - Resolve scope (`if args.global { Scope::global()? } else { Scope::resolve(&cwd)? }`).
   - `let dir = scope.state_path().join("sessions");`
   - `let mut metas = session::list_sessions(&dir, args.agent.as_deref())?;`
   - Apply limit: `if !args.all { metas.truncate(args.limit); }`
   - Emit human or JSON per `output.is_json()`.

3. **Human output** (per spec §5.3):
   ```
   ID        AGENT  CREATED            TURNS  TITLE
   e8b97f2c  coder  2026-05-01 14:33      2  -
   a3c1f8d4  notes  2026-04-30 09:12      8  -
   ```
   Pad columns; "-" for None title. If empty: print "No sessions in <scope> scope.".

4. **JSON output** (per spec §5.3):
   ```
   {"event":"sessions","total":N,"limit":L}
   {"event":"session","id":"...","prefix":"...","agent":"...","created_at":"...","turns":N,"title":null}
   ...
   ```
   Use the existing `output.json(&serde_json::json!(...))?` helper.

5. **3+ e2e tests:**
   - `session_list_empty_returns_zero` — fresh scope; `tau session list`; exit 0; stdout: "No sessions".
   - `session_list_multiple_returns_descending` — write 2 sessions to scope manually; `tau session list`; assert both appear in descending order.
   - `session_list_filter_by_agent` — write 2 sessions with different agent_ids; `tau session list <agent>`; assert only matching appears.
   - `session_list_json_emits_one_event_per_line` — `tau session list --json`; parse stdout line-by-line, assert valid JSON with `event` field.

6. **Help snapshots:** `tau --help` (top-level adds `session`), `tau session --help` (new), `tau session list --help` (new). Run `INSTA_UPDATE=always cargo test -p tau-cli --test help_snapshots` to accept; add corresponding `snapshot_session_help` and `snapshot_session_list_help` test functions to `tests/help_snapshots.rs`.

7. **Verification:** standard suite + `cargo test -p tau-cli --test cmd_session_list`.

8. **Commit message:** `feat(cli): tau session list subcommand`.

9. Push.

---

## Task 8: `tau session show` subcommand + e2e tests

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` — add `Show(SessionShowArgs)` variant.
- Create: `crates/tau-cli/src/cmd/session/show.rs` — handler.
- Modify: `crates/tau-cli/src/cmd/session/mod.rs` — dispatch.
- Create: `crates/tau-cli/tests/cmd_session_show.rs` — 3+ e2e tests.

**Spec sections:** §5.2, §5.3.

### Per-task summary

1. **`SessionShowArgs`:** `id: String`, `--global: bool`, `--json: bool` (or use `output.is_json()` from the global Output flag — pick whichever matches the existing pattern).

2. **`cmd/session/show.rs::run`:**
   - Resolve scope.
   - `let dir = scope.state_path().join("sessions");`
   - `let sid = session::id::resolve_id_prefix(&dir, &args.id)?;`
   - `let path = dir.join(format!("{}.jsonl", sid.as_str()));`
   - `let (header, entries) = session::store::SessionReader::read(&path)?;`
   - If `output.is_json()`: passthrough — print the file contents line-by-line via `output.json()` for each parsed line, OR `print!("{}", fs::read_to_string(&path)?)` for true byte-passthrough. Pick passthrough for simplicity.
   - Else: `print!("{}", session::render::render_session(&header, &entries));`

3. **3+ e2e tests:**
   - `session_show_renders_markdown` — write a session manually; `tau session show <id>`; stdout contains agent_id, "**You:**", message text.
   - `session_show_json_passthrough` — `tau session show <id> --json`; stdout byte-equals file contents.
   - `session_show_unknown_id_exits_2` — `tau session show 00000000` → exit 2; stderr contains "not found".
   - `session_show_ambiguous_prefix_exits_2` — write 2 sessions with shared 8-char prefix; `tau session show <shared-prefix>` → exit 2; stderr lists candidates.

4. **Help snapshot:** `tau session show --help`. Add `snapshot_session_show_help` test function.

5. **Verification:** standard suite + `cargo test -p tau-cli --test cmd_session_show`.

6. **Commit message:** `feat(cli): tau session show subcommand`.

7. Push.

---

## Task 9: `tau session delete` subcommand + e2e tests

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` — add `Delete(SessionDeleteArgs)` variant.
- Create: `crates/tau-cli/src/cmd/session/delete.rs` — handler.
- Modify: `crates/tau-cli/src/cmd/session/mod.rs` — dispatch.
- Create: `crates/tau-cli/tests/cmd_session_delete.rs` — 3+ e2e tests.

**Spec sections:** §5.2, §5.3.

### Per-task summary

1. **`SessionDeleteArgs`:** `id: String`, `--global: bool`, `--force: bool`.

2. **`cmd/session/delete.rs::run`:**
   - Resolve scope + resolve_id_prefix → exact id + path.
   - Load header for the confirmation prompt (read header line only, fast).
   - If NOT `args.force`: print `About to delete session <prefix> (<agent>, <turns> turns, <created_at>).\nContinue? [y/N] ` and read a line from stdin. Accept `y` / `Y` / `yes`; everything else → abort with exit 0 and "Aborted." message.
   - `fs::remove_file(&path)?`. Print `✓ Deleted.`.
   - JSON mode: emit `{"event":"deleted","id":"..."}` (no prompt; `--force` is implicit).

3. **3+ e2e tests:**
   - `session_delete_with_force_removes_file` — write session; `tau session delete <id> --force`; assert file gone.
   - `session_delete_prompt_yes_removes_file` — write session; pipe `y\n` to stdin; assert file gone.
   - `session_delete_prompt_no_keeps_file` — pipe `n\n`; assert file still exists.
   - `session_delete_unknown_id_exits_2` — `tau session delete 00000000 --force` → exit 2.

4. **Help snapshot:** `tau session delete --help`. Add `snapshot_session_delete_help`.

5. **Verification:** standard suite + `cargo test -p tau-cli --test cmd_session_delete`.

6. **Commit message:** `feat(cli): tau session delete subcommand`.

7. Push.

---

## Task 10: `tau session export` subcommand + e2e tests

**Hybrid format.**

**Files:**
- Modify: `crates/tau-cli/src/cli.rs` — add `Export(SessionExportArgs)` variant.
- Create: `crates/tau-cli/src/cmd/session/export.rs` — handler.
- Modify: `crates/tau-cli/src/cmd/session/mod.rs` — dispatch.
- Create: `crates/tau-cli/tests/cmd_session_export.rs` — 3+ e2e tests.

**Spec sections:** §5.2.

### Per-task summary

1. **`SessionExportArgs`:** `id: String`, `--format: ExportFormat` (clap `ValueEnum`: `Jsonl` | `Md` | `Json`; default `Jsonl`), `--global: bool`.

2. **`cmd/session/export.rs::run`:**
   - Resolve scope + id + path (same as show).
   - Branch on format:
     - **`Jsonl`**: `print!("{}", fs::read_to_string(&path)?);` (passthrough).
     - **`Md`**: `let (h, e) = SessionReader::read(&path)?; print!("{}", session::render::render_session(&h, &e));` (same as `show` human mode but no header summary suppression).
     - **`Json`**: `let (header, entries) = SessionReader::read(&path)?; let envelope = serde_json::json!({"header": header, "messages": [...messages...], "turn_summaries": [...summaries...]}); println!("{}", serde_json::to_string_pretty(&envelope)?);` — single envelope JSON object.

3. **3+ e2e tests:**
   - `session_export_jsonl_passthrough` — write session; `tau session export <id> --format jsonl`; stdout byte-equals file.
   - `session_export_md_renders_markdown` — `--format md`; stdout contains "**You:**" etc.
   - `session_export_json_envelope` — `--format json`; parse stdout as single JSON; assert `header.id` and `messages` array present.
   - `session_export_unknown_id_exits_2`.

4. **Help snapshot:** `tau session export --help`. Add `snapshot_session_export_help`.

5. **Verification:** standard suite + `cargo test -p tau-cli --test cmd_session_export`.

6. **Commit message:** `feat(cli): tau session export subcommand`.

7. Push.

---

## Task 11: Final verification + open PR

**User-driven gate. PAUSE before this task.**

### Steps

- [ ] **Step 11.1: Full local verification**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

All must pass. If anything fails, fix it before opening the PR.

- [ ] **Step 11.2: Open the PR**

```bash
gh pr list --head feat/repl-persistence-spec --json number,state,isDraft
```

If empty, create:

```bash
gh pr create --title "feat: REPL persistence — tau chat --resume + tau session group (Tier 3 priority 11)" \
  --body "$(cat <<'EOF'
## Summary

Closes ADR-0007 §11's deferral of `/save`/`/load` (auto-save makes them moot) and ADR-0006 §16's "REPL persistence" reservation.

- Sessions auto-save to JSONL files at `<scope>/sessions/<uuid>.jsonl`. `--ephemeral` opts out (in-memory only).
- `tau chat <agent> --resume <id>` (or 8+ char prefix) resumes a session with strict-mode drift validation; `--force` overrides.
- New `tau session` subcommand group: `list`, `show`, `delete`, `export` (formats: jsonl, md, json).
- New `/info` REPL slash command (prints session id, file path, turn count). `/clear` removed (incoherent with persistence; replaced by `/exit` + re-run for fresh session).

## Spec / Plan

- Spec: `docs/superpowers/specs/2026-05-01-repl-persistence-design.md`
- Plan: `docs/superpowers/plans/2026-05-01-repl-persistence.md`
- ADR-0013 lands in Task 12 (post-merge follow-up commit).

## No tau-runtime changes

Per ADR-0006 NG6, persistent agent memory is a CLI concern. The runtime stays stateless across CLI invocations.

## Test plan

- [x] `cargo build --workspace` green
- [x] `cargo test --workspace --all-targets` green
- [x] `cargo test --workspace --doc` green
- [x] `cargo clippy --workspace --all-targets -- -D warnings` green
- [x] `cargo fmt --all -- --check` green
- [ ] CI matrix (23 required checks) green — verifying

## Out of scope (deferred)

- Cross-session full-text search. SQLite is the planned migration target if/when this becomes a requirement.
- `/title <name>` slash command (nice-to-have polish).
- Auto-prune by age or count.
- Session encryption.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 11.3: Capture PR URL**

```bash
gh pr view --json number,url --jq '{number, url}'
```

- [ ] **Step 11.4: PAUSE — wait for CI green before Task 12**

Same Bash + Monitor pattern from priorities 7/8.

---

## Task 12: ADR-0013 + ROADMAP + squash merge

**User-driven gate. PAUSE before this task.**

**Files:**
- Create: `/Users/titouanlebocq/code/tau/docs/decisions/0013-repl-persistence.md` — full ADR.
- Modify: `/Users/titouanlebocq/code/tau/ROADMAP.md` — mark Tier 3 priority 11 ✅.

### Steps

- [ ] **Step 12.1: Write ADR-0013**

Mirror the structure of ADR-0012. Sections:

1. **JSONL storage format** — append-only, one file per session. SQLite is the planned migration target if cross-session search becomes a requirement.
2. **UUID v7 with prefix resolution** — sortable; CLI accepts ≥8-char prefixes.
3. **Strict-mode resume + `--force`** — agent + package + LLM backend match; mismatch errors with field name.
4. **Auto-save default with `--ephemeral` opt-out** — every chat session persists from turn 1.
5. **`tau chat` + `tau session` CLI split** — resume on chat, archive management in a new `tau session` group.

Plus invariants:
- Per-scope storage at `<scope.state_path()>/sessions/`.
- No tau-runtime changes (NG6 preserved).
- `/clear` removed (replaced by `/exit` + re-run).
- Schema v1 baseline; future bumps require ADR amendment.

Status: Accepted, 2026-05-01.

Cross-references: ADR-0006 §16 (the deferral this closes), ADR-0006 NG6 (no persistent agent memory in core), ADR-0007 §11 (slash command surface; `/save`/`/load` deferred), ADR-0007 §7 (3-bucket exit codes), ADR-0009 (typed-error policy), ADR-0011 (JSON event-per-line), ADR-0012 (CLI subcommand-group precedent).

- [ ] **Step 12.2: Update ROADMAP**

Find the Tier 3 priority 11 entry. Replace with:

```markdown
11. **REPL persistence** (`tau chat --resume <id>`) ✅ Shipped 2026-05-01 — see
    [spec](docs/superpowers/specs/2026-05-01-repl-persistence-design.md)
    and [ADR-0013](docs/decisions/0013-repl-persistence.md).
    Sessions auto-save to JSONL files at `<scope>/sessions/<uuid>.jsonl`.
    `--ephemeral` opts out. `tau chat <agent> --resume <id-or-prefix>`
    with strict-mode drift validation (`--force` overrides). New
    `tau session` subcommand group (list, show, delete, export). New
    `/info` REPL slash command; `/clear` removed (replaced by `/exit`
    + re-run). No tau-runtime changes (NG6 preserved). No new CI jobs
    (23 required checks unchanged).
```

Add to the top-of-file shipped table (after the priority 7 row from ADR-0012):

```markdown
| 11 | REPL persistence ✅ | Tier 3 priority 11 — closes ADR-0006 §16 + ADR-0007 §11 deferrals. New tau-cli/src/session module: SessionId (UUID v7), SessionWriter / SessionReader (JSONL), list_sessions, render_session. Auto-save default with --ephemeral opt-out. tau chat --resume <id-or-prefix> with strict drift validation (agent + package.name + package.version + llm_backend match), --force overrides. New tau session subcommand group (list, show, delete, export with jsonl/md/json formats). /clear removed (incoherent with persistence); /info added. Schema v1 baseline. No tau-runtime changes. New ADR-0013. No new CI jobs (23 required checks unchanged). | 2026-05-01 |
```

Update the front-matter narrative paragraph: priority 11 is now closed; Tier 3 has 3 priorities remaining (9 multi-agent orchestration, 10 workflow runner, 12 sandboxing).

- [ ] **Step 12.3: Commit + push**

```bash
git -C /Users/titouanlebocq/code/tau add docs/decisions/0013-repl-persistence.md ROADMAP.md
git -C /Users/titouanlebocq/code/tau commit -m "$(cat <<'EOF'
docs: ADR-0013 + ROADMAP Tier 3 priority 11 done

Locks the 5 design decisions for REPL persistence:
1. JSONL storage format (source-agnostic; SQLite is migration target
   if cross-session search lands)
2. UUID v7 ids with ≥8-char prefix resolution
3. Strict-mode resume + --force opt-out
4. Auto-save default with --ephemeral opt-out
5. tau chat + tau session CLI split

Plus invariants: per-scope storage at <scope>/sessions/; no
tau-runtime changes (ADR-0006 NG6 preserved); /clear removed;
schema v1 baseline.

Updates ROADMAP:
- Top-of-file shipped table gains a row for Tier 3 priority 11.
- Tier 3 priority 11 entry marked ✅ Shipped 2026-05-01.
- Front-matter narrative: Tier 3 has 3 priorities remaining
  (9, 10, 12).

No new CI jobs; branch protection stays at 23 required checks.

Refs: docs/superpowers/specs/2026-05-01-repl-persistence-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git -C /Users/titouanlebocq/code/tau push
```

- [ ] **Step 12.4: Wait for CI green on the PR**

Same poller pattern.

- [ ] **Step 12.5: Squash merge**

```bash
gh pr merge --squash --delete-branch
```

- [ ] **Step 12.6: Verify branch protection unchanged**

```bash
gh api repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks/contexts | jq 'length'
```

Expected: `23`.

- [ ] **Step 12.7: Sync local main + report squash SHA**

```bash
git checkout main && git pull && git log --oneline -3
```

---

## Verification standard (per task)

Each task ends with:

```bash
cargo build --workspace
cargo test -p tau-cli --all-targets        # for tau-cli-only tasks
cargo test --workspace --all-targets       # for tasks touching multiple crates
cargo test --workspace --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Tasks 5-10 also touch `help_snapshots`; run `cargo test -p tau-cli --test help_snapshots` and accept any new snapshots with `cargo insta accept` (or `INSTA_UPDATE=always cargo test ...`).

CI continues on push; no new jobs added; branch protection stays at 23.
