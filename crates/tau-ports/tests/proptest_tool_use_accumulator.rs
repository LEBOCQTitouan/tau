//! Property tests for [`tau_ports::ToolUseAccumulator`].
//!
//! Generates a valid JSON object, splits it into 1..=10 chunks at
//! arbitrary code-point boundaries, appends each chunk via
//! [`ToolUseAccumulator::append`], and finalizes via
//! [`ToolUseAccumulator::finalize_with`] using `serde_json::from_str`
//! as the parser. Asserts the parsed `Value` matches the original.
//!
//! Includes a corner case: zero appends (empty buffer) → `finalize_with`
//! fails with [`LlmError::Stream`] (empty input is not valid JSON).
//!
//! Gated behind the `test-fixtures` feature because the test uses
//! [`tau_ports::fixtures::make_tool_use`] for the corner-case
//! comparison helper. (The `try_new` / accumulator surface itself is
//! always public, but unifying both proptest files behind one feature
//! keeps the test-matrix simple.)

#![cfg(feature = "test-fixtures")]

use proptest::prelude::*;

use tau_domain::Value;
use tau_ports::{LlmError, ToolUseAccumulator};

/// Strategy producing a `Value::Object` whose values are simple
/// (string / integer / bool / null) leaves. Restricting to a flat
/// object keeps the JSON deterministic-shaped while still exercising
/// the buffer-split logic. Floats are excluded for the same reason as
/// `proptest_chunk_roundtrip.rs`: `Value` derives `PartialEq` but
/// allows NaN under `Float`.
fn arb_simple_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::Integer),
        // Restrict string charset to alphanumeric to dodge JSON-escape
        // edge cases inside the proptest's split-at-arbitrary-boundary
        // logic; the accumulator itself doesn't care about content,
        // it just buffers bytes.
        "[a-zA-Z0-9]{0,16}".prop_map(Value::String),
    ]
}

fn arb_simple_object() -> impl Strategy<Value = Value> {
    prop::collection::btree_map("[a-zA-Z][a-zA-Z0-9]{0,7}", arb_simple_value(), 0..6)
        .prop_map(Value::Object)
}

/// Split a string into `n` non-empty chunks at arbitrary char
/// boundaries. Returns at least 1 chunk; if the input is shorter than
/// `n`, returns one chunk per character.
fn split_string(s: &str, n: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    let n = n.max(1).min(chars.len());
    let mut out: Vec<String> = Vec::with_capacity(n);
    let chunk_size = chars.len().div_ceil(n);
    for chunk in chars.chunks(chunk_size) {
        out.push(chunk.iter().collect());
    }
    out
}

proptest! {
    /// A valid JSON object, split into `n` chunks (1..=10), round-trips
    /// through the accumulator and back to the original `Value`.
    #[test]
    fn accumulator_round_trips_split_json(
        v in arb_simple_object(),
        n in 1usize..=10,
    ) {
        let json = serde_json::to_string(&v).expect("serialize Value");
        let chunks = split_string(&json, n);

        let mut acc = ToolUseAccumulator::new("toolu_pt".into(), "search".into());
        for chunk in &chunks {
            acc.append(chunk);
        }

        // The buffer should be the concatenation of all chunks (i.e. the
        // original JSON).
        prop_assert_eq!(acc.input_buffer(), json.as_str());

        let tu = acc
            .finalize_with(|s| serde_json::from_str::<Value>(s).map_err(|e| e.to_string()))
            .expect("finalize_with should succeed for valid JSON");

        prop_assert_eq!(&tu.id, "toolu_pt");
        prop_assert_eq!(&tu.name, "search");
        // Value derives PartialEq (not Eq); strategy excludes Float so
        // structural equality holds.
        prop_assert!(tu.input == v);
    }

    /// Corner case: zero appends (empty buffer) → `finalize_with`
    /// returns `Err(LlmError::Stream)` because the empty string is not
    /// valid JSON.
    #[test]
    fn accumulator_empty_buffer_fails(_unused in 0..1u32) {
        let acc = ToolUseAccumulator::new("toolu_empty".into(), "search".into());
        prop_assert_eq!(acc.input_buffer(), "");

        let err = acc
            .finalize_with(|s| serde_json::from_str::<Value>(s).map_err(|e| e.to_string()))
            .expect_err("empty buffer should not parse");

        prop_assert!(matches!(err, LlmError::Stream { .. }), "got {err:?}");
    }
}
