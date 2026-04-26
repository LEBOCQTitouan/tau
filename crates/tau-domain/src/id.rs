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
        let max_len = "x".repeat(64);
        for name in ["a", "fs-tools", "abc-123", max_len.as_str()] {
            assert!(
                PackageName::from_str(name).is_ok(),
                "should accept {name:?}"
            );
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
