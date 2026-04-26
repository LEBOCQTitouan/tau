//! Property tests for [`tau_ports::storage::Namespace`] grammar.
//!
//! Generates valid printable-ASCII inputs and checks round-trip
//! through `Namespace::try_new` + `as_str` + `try_new`. Generates
//! several classes of invalid inputs and asserts the specific
//! [`NamespaceError`] variant.
//!
//! These tests don't construct any `#[non_exhaustive]` types from
//! outside the crate; they exercise only the public `try_new` /
//! `as_str` surface, so no `test-fixtures` feature is required.

use proptest::prelude::*;

use tau_ports::storage::Namespace;
use tau_ports::NamespaceError;

proptest! {
    /// Any printable-ASCII byte (0x20..=0x7E), 1..=1024 bytes long, is
    /// accepted, and `as_str` round-trips back through `try_new`.
    #[test]
    fn namespace_round_trips_printable_ascii(s in "[\\x20-\\x7e]{1,1024}") {
        let ns = Namespace::try_new(s.clone())
            .expect("printable-ASCII 1..=1024 should be accepted");
        prop_assert_eq!(ns.as_str(), s.as_str());

        // Round-trip via as_str + try_new.
        let ns2 = Namespace::try_new(ns.as_str().to_string()).unwrap();
        prop_assert_eq!(ns, ns2);
    }

    /// The empty input is rejected with [`NamespaceError::Empty`].
    #[test]
    fn namespace_rejects_empty(_unused in 0..1u32) {
        prop_assert_eq!(Namespace::try_new(""), Err(NamespaceError::Empty));
    }

    /// Inputs with length 1025..=2000 are rejected with
    /// [`NamespaceError::TooLong`].
    #[test]
    fn namespace_rejects_too_long(len in 1025usize..=2000) {
        let s = "a".repeat(len);
        let err = Namespace::try_new(s).unwrap_err();
        prop_assert_eq!(err, NamespaceError::TooLong { max: 1024, got: len });
    }

    /// Inserting a NUL byte at any position in an otherwise-valid input
    /// is rejected with [`NamespaceError::InvalidByte`] at that position.
    #[test]
    fn namespace_rejects_nul_byte(
        prefix in "[\\x20-\\x7e]{0,32}",
        suffix in "[\\x20-\\x7e]{0,32}",
    ) {
        let mut s = String::with_capacity(prefix.len() + 1 + suffix.len());
        s.push_str(&prefix);
        s.push('\0');
        s.push_str(&suffix);
        let err = Namespace::try_new(s).unwrap_err();
        prop_assert_eq!(err, NamespaceError::InvalidByte { pos: prefix.len() });
    }

    /// Inserting a control byte (0x01..=0x1F) at any position is rejected
    /// with [`NamespaceError::InvalidByte`] at that position.
    #[test]
    fn namespace_rejects_control_byte(
        prefix in "[\\x20-\\x7e]{0,32}",
        suffix in "[\\x20-\\x7e]{0,32}",
        ctrl in 0x01u8..=0x1F,
    ) {
        let mut s = String::with_capacity(prefix.len() + 1 + suffix.len());
        s.push_str(&prefix);
        s.push(ctrl as char);
        s.push_str(&suffix);
        let err = Namespace::try_new(s).unwrap_err();
        prop_assert_eq!(err, NamespaceError::InvalidByte { pos: prefix.len() });
    }

    /// Inserting a DEL byte (0x7F) at any position is rejected with
    /// [`NamespaceError::InvalidByte`] at that position.
    #[test]
    fn namespace_rejects_del_byte(
        prefix in "[\\x20-\\x7e]{0,32}",
        suffix in "[\\x20-\\x7e]{0,32}",
    ) {
        let mut s = String::with_capacity(prefix.len() + 1 + suffix.len());
        s.push_str(&prefix);
        s.push('\x7f');
        s.push_str(&suffix);
        let err = Namespace::try_new(s).unwrap_err();
        prop_assert_eq!(err, NamespaceError::InvalidByte { pos: prefix.len() });
    }
}
