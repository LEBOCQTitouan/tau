//! Property tests for [`tau_ports::storage::Key`] grammar.
//!
//! Keys allow control characters and arbitrary UTF-8 — only NUL bytes
//! are forbidden. Strategies are correspondingly broader than for
//! [`Namespace`]: arbitrary printable-ASCII *and* control characters
//! are valid.
//!
//! These tests don't construct any `#[non_exhaustive]` types from
//! outside the crate; they exercise only the public `try_new` /
//! `as_str` surface, so no `test-fixtures` feature is required.

use proptest::prelude::*;

use tau_ports::storage::Key;
use tau_ports::KeyError;

proptest! {
    /// Any non-NUL byte sequence (0x01..=0xFF, 1..=1024 bytes), if it
    /// happens to be valid UTF-8, is accepted, and `as_str` round-trips.
    /// We restrict the strategy to a regex that excludes NUL only.
    #[test]
    fn key_round_trips_non_nul(s in "[\\x01-\\x7f]{1,1024}") {
        let k = Key::try_new(s.clone()).expect("non-NUL 1..=1024 should be accepted");
        prop_assert_eq!(k.as_str(), s.as_str());

        // Round-trip via as_str + try_new.
        let k2 = Key::try_new(k.as_str().to_string()).unwrap();
        prop_assert_eq!(k, k2);
    }

    /// Keys may contain control characters that namespaces reject
    /// (newlines, tabs, DEL).
    #[test]
    fn key_accepts_control_characters(
        prefix in "[\\x20-\\x7e]{0,32}",
        ctrl in prop_oneof![Just(0x09u8), Just(0x0Au8), Just(0x0Du8), Just(0x1Bu8), Just(0x7Fu8)],
        suffix in "[\\x20-\\x7e]{0,32}",
    ) {
        let mut s = String::with_capacity(prefix.len() + 1 + suffix.len());
        s.push_str(&prefix);
        s.push(ctrl as char);
        s.push_str(&suffix);
        prop_assert!(Key::try_new(s).is_ok());
    }

    /// The empty input is rejected with [`KeyError::Empty`].
    #[test]
    fn key_rejects_empty(_unused in 0..1u32) {
        prop_assert_eq!(Key::try_new(""), Err(KeyError::Empty));
    }

    /// Inputs with length 1025..=2000 are rejected with
    /// [`KeyError::TooLong`].
    #[test]
    fn key_rejects_too_long(len in 1025usize..=2000) {
        let s = "a".repeat(len);
        let err = Key::try_new(s).unwrap_err();
        prop_assert_eq!(err, KeyError::TooLong { max: 1024, got: len });
    }

    /// Inserting a NUL byte at any position is rejected with
    /// [`KeyError::InvalidByte`] at that position.
    #[test]
    fn key_rejects_nul_byte(
        prefix in "[\\x01-\\x7f]{0,32}",
        suffix in "[\\x01-\\x7f]{0,32}",
    ) {
        let mut s = String::with_capacity(prefix.len() + 1 + suffix.len());
        s.push_str(&prefix);
        s.push('\0');
        s.push_str(&suffix);
        let err = Key::try_new(s).unwrap_err();
        prop_assert_eq!(err, KeyError::InvalidByte { pos: prefix.len() });
    }
}
