//! Property tests for the `batch_to_stream` / `stream_to_batch`
//! round-trip on [`tau_ports::llm::CompletionResponse`].
//!
//! Generates an arbitrary canonical `CompletionResponse`, converts it
//! to a `CompletionStream` via [`batch_to_stream`], reassembles via
//! [`stream_to_batch`], and asserts the result is observationally
//! equivalent: text matches, tool_uses match in length + per-entry id
//! / name / input, stop_reason matches, usage matches.
//!
//! Gated behind the `test-fixtures` feature because the test
//! constructs [`CompletionResponse`] / [`ToolUse`] / [`TokenUsage`]
//! (`#[non_exhaustive]`) via factory helpers exposed from
//! `tau_ports::fixtures`.
//!
//! `Value::Float` is excluded from the input strategy: floats don't
//! implement `Eq`, and even with bit-exact preservation we'd have to
//! special-case NaN / subnormals. The chunk round-trip exercises text
//! + structural fields, not float wire-format quirks.

#![cfg(feature = "test-fixtures")]

use proptest::prelude::*;

use tau_domain::Value;
use tau_ports::fixtures::{make_completion_response, make_token_usage, make_tool_use};
use tau_ports::llm::{
    batch_to_stream, stream_to_batch, CompletionResponse, StopReason, TokenUsage, ToolUse,
};

/// Strategy producing a `tau_domain::Value` with no `Float` leaves.
///
/// Floats are excluded because `Value` derives `PartialEq` but not
/// `Eq` (per `Value::Float(f64)`); proptest's structural equality
/// works fine but we'd need to filter NaN. Skipping floats keeps the
/// test focused on structural round-trip.
fn arb_value_no_float() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::Integer),
        ".{0,16}".prop_map(Value::String),
        prop::collection::vec(any::<u8>(), 0..16).prop_map(Value::Bytes),
    ];
    leaf.prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map(".{0,8}", inner, 0..4).prop_map(Value::Object),
        ]
    })
}

fn arb_stop_reason() -> impl Strategy<Value = StopReason> {
    prop_oneof![
        Just(StopReason::EndTurn),
        Just(StopReason::MaxTokens),
        Just(StopReason::StopSequence),
        Just(StopReason::ToolUse),
        Just(StopReason::Error),
    ]
}

fn arb_token_usage() -> impl Strategy<Value = TokenUsage> {
    (any::<u32>(), any::<u32>())
        .prop_map(|(input_tokens, output_tokens)| make_token_usage(input_tokens, output_tokens))
}

fn arb_tool_use() -> impl Strategy<Value = ToolUse> {
    (".{0,16}", ".{0,16}", arb_value_no_float())
        .prop_map(|(id, name, input)| make_tool_use(id, name, input))
}

fn arb_completion_response() -> impl Strategy<Value = CompletionResponse> {
    (
        ".{0,32}",
        prop::collection::vec(arb_tool_use(), 0..4),
        arb_stop_reason(),
        proptest::option::of(arb_token_usage()),
    )
        .prop_map(|(text, tool_uses, stop_reason, usage)| {
            make_completion_response(text, tool_uses, stop_reason, usage)
        })
}

proptest! {
    /// `batch_to_stream` then `stream_to_batch` reassembles to an
    /// observationally-equivalent `CompletionResponse`.
    #[test]
    fn completion_response_round_trips(resp in arb_completion_response()) {
        let original_text = resp.text.clone();
        let original_tool_uses = resp.tool_uses.clone();
        let original_stop_reason = resp.stop_reason;
        let original_usage = resp.usage;

        // Drive the round-trip on a tokio runtime (the helper is async).
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let resp2 = runtime
            .block_on(async move {
                let stream = batch_to_stream(resp);
                stream_to_batch(stream).await
            })
            .expect("stream_to_batch should succeed");

        prop_assert_eq!(resp2.text, original_text);
        prop_assert_eq!(resp2.tool_uses.len(), original_tool_uses.len());
        for (actual, expected) in resp2.tool_uses.iter().zip(original_tool_uses.iter()) {
            prop_assert_eq!(&actual.id, &expected.id);
            prop_assert_eq!(&actual.name, &expected.name);
            // Value derives PartialEq (not Eq) — works here because the
            // strategy excludes Float, so we can't generate NaN.
            prop_assert!(actual.input == expected.input);
        }
        prop_assert_eq!(resp2.stop_reason, original_stop_reason);
        prop_assert_eq!(resp2.usage, original_usage);
    }
}
