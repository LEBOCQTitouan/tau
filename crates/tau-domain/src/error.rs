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
