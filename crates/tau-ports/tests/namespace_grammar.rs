//! Table-driven integration tests for [`Namespace::try_new`] and
//! [`Key::try_new`] grammar.
//!
//! Complements `proptest_namespace.rs` / `proptest_key.rs` with
//! specific edge cases the property tests don't exercise: the exact
//! 1024-byte boundary (accept) and 1025-byte boundary (reject), and
//! concrete known-invalid bytes for both newtypes.
//!
//! Gated behind the `test-fixtures` feature for suite-wide consistency
//! (does not actually depend on fixtures).

#![cfg(feature = "test-fixtures")]

use tau_ports::error::{KeyError, NamespaceError};
use tau_ports::storage::{Key, Namespace};

// ---------------------------------------------------------------------------
// Namespace
// ---------------------------------------------------------------------------

/// Namespace accepts a representative set of valid inputs, including
/// the exact 1024-byte upper bound (boundary not exercised by proptest).
#[test]
fn namespace_valid_table() {
    let max_len = "a".repeat(Namespace::MAX_LEN);
    let cases: &[&str] = &[
        "a",
        "global:cache",
        "agent:01890000-0000-7000-8000-000000000001",
        "project/foo",
        "with spaces",
        "tilde~ok",
        "punct!@#$%^&*()_+-=[]{}\\|;'\",.<>/?",
        max_len.as_str(),
    ];
    for s in cases {
        assert!(Namespace::try_new(*s).is_ok(), "should accept {s:?}");
    }
}

/// Namespace rejects each known-invalid input with the specific error
/// variant, including the 1025-byte (off-by-one) boundary case.
#[test]
fn namespace_invalid_table() {
    // Empty.
    assert_eq!(Namespace::try_new(""), Err(NamespaceError::Empty));

    // Exactly one byte over the cap.
    let too_long = "a".repeat(Namespace::MAX_LEN + 1);
    assert_eq!(
        Namespace::try_new(&too_long),
        Err(NamespaceError::TooLong {
            max: Namespace::MAX_LEN,
            got: Namespace::MAX_LEN + 1,
        }),
    );

    // NUL byte at known position.
    assert_eq!(
        Namespace::try_new("foo\0bar"),
        Err(NamespaceError::InvalidByte { pos: 3 }),
    );

    // Specific control characters: TAB (0x09), LF (0x0A), CR (0x0D),
    // ESC (0x1B), DEL (0x7F).
    for (input, pos) in [
        ("foo\tbar", 3),
        ("foo\nbar", 3),
        ("foo\rbar", 3),
        ("foo\x1bbar", 3),
        ("foo\x7fbar", 3),
    ] {
        assert_eq!(
            Namespace::try_new(input),
            Err(NamespaceError::InvalidByte { pos }),
            "for input {input:?}",
        );
    }

    // Control byte at position 0 (leading).
    assert_eq!(
        Namespace::try_new("\x01rest"),
        Err(NamespaceError::InvalidByte { pos: 0 }),
    );
}

// ---------------------------------------------------------------------------
// Key
// ---------------------------------------------------------------------------

/// Key accepts valid inputs including control chars (Key permits them;
/// only NUL is rejected) and the exact 1024-byte upper bound.
#[test]
fn key_valid_table() {
    let max_len = "a".repeat(Key::MAX_LEN);
    let cases: &[&str] = &[
        "a",
        "agent:foo",
        "with\tnewlines\n",
        "with:colons:ok",
        "deep/nested/path",
        "punct!@#$%^&*()_+-=[]{}\\|;'\",.<>/?",
        "\x01\x02\x03",
        "\x7f", // DEL allowed in Key (only NUL is forbidden).
        max_len.as_str(),
    ];
    for s in cases {
        assert!(Key::try_new(*s).is_ok(), "should accept {s:?}");
    }
}

/// Key rejects each known-invalid input with the specific error
/// variant, including the 1025-byte boundary case.
#[test]
fn key_invalid_table() {
    // Empty.
    assert_eq!(Key::try_new(""), Err(KeyError::Empty));

    // Exactly one byte over the cap.
    let too_long = "a".repeat(Key::MAX_LEN + 1);
    assert_eq!(
        Key::try_new(&too_long),
        Err(KeyError::TooLong {
            max: Key::MAX_LEN,
            got: Key::MAX_LEN + 1,
        }),
    );

    // NUL byte at various positions.
    assert_eq!(Key::try_new("\0"), Err(KeyError::InvalidByte { pos: 0 }));
    assert_eq!(
        Key::try_new("foo\0bar"),
        Err(KeyError::InvalidByte { pos: 3 }),
    );
    assert_eq!(
        Key::try_new("trailing\0"),
        Err(KeyError::InvalidByte { pos: 8 }),
    );
}
