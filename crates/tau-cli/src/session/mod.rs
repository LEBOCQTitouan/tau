// Dead-code / unused-import warnings are expected until Tasks 5+ wire up
// this module. Suppress them so clippy -D warnings stays green.
#![allow(dead_code, unused_imports)]

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
pub mod store;

pub use id::{mint, resolve_id_prefix, SessionId};
pub use store::{
    list_sessions, SessionEntry, SessionHeader, SessionMetadata, SessionPackage, SessionReader,
    SessionWriter, SCHEMA_VERSION,
};

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
