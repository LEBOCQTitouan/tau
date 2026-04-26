//! Per-concern error enums for `tau-ports`.
//!
//! Each error type is `#[non_exhaustive]` so additive variants are non-breaking.
//! All errors derive `Debug + Error + Clone + PartialEq + Eq`; tests with
//! free-form `String` fields use `matches!()` to avoid brittle wording
//! comparisons.
//!
//! `LlmError`, `ToolError`, `StorageError`, and `SandboxError` are the
//! per-trait error types (added in Task 4). `NamespaceError` and `KeyError`
//! are the validation errors for the `Namespace` and `Key` newtypes.

use thiserror::Error;

/// Validation errors for `Namespace` (lands in Task 3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NamespaceError {
    /// The input was empty.
    #[error("namespace is empty")]
    Empty,
    /// The input exceeded the byte cap.
    #[error("namespace exceeds {max} bytes: got {got}")]
    TooLong {
        /// Maximum permitted length, in bytes.
        max: usize,
        /// Actual length of the input, in bytes.
        got: usize,
    },
    /// The input contained a NUL byte or control character.
    #[error("namespace contains invalid byte (NUL or control char) at position {pos}")]
    InvalidByte {
        /// Byte position in the input.
        pos: usize,
    },
}

/// Validation errors for `Key` (lands in Task 3).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyError {
    /// The input was empty.
    #[error("key is empty")]
    Empty,
    /// The input exceeded the byte cap.
    #[error("key exceeds {max} bytes: got {got}")]
    TooLong {
        /// Maximum permitted length, in bytes.
        max: usize,
        /// Actual length of the input, in bytes.
        got: usize,
    },
    /// The input contained a NUL byte.
    #[error("key contains NUL byte at position {pos}")]
    InvalidByte {
        /// Byte position in the input.
        pos: usize,
    },
}
