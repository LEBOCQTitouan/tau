//! Property test: arbitrary Message round-trips through serde_json.
//!
//! Note: `Message` and `AgentStatus::Failed` are `#[non_exhaustive]`, so
//! they cannot be struct-literal-constructed from outside the crate. We
//! use the `test-fixtures` feature for a base `Message` (mutating the
//! `pub` `payload` field) and deserialize a `Failed` lifecycle payload
//! from a JSON literal.

#![cfg(feature = "test-fixtures")]

use proptest::prelude::*;

use tau_domain::fixtures::any_message;
use tau_domain::{AgentStatus, Message, MessagePayload, Value};

fn arb_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::Integer),
        any::<f64>()
            .prop_filter("normal or zero", |f| f.is_finite()
                && (f.is_normal() || *f == 0.0))
            .prop_map(Value::Float),
        ".{0,16}".prop_map(Value::String),
    ];
    leaf.prop_recursive(2, 8, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
            prop::collection::btree_map(".{0,8}", inner, 0..4).prop_map(Value::Object),
        ]
    })
}

fn failed_status() -> AgentStatus {
    // `AgentStatus::Failed` is variant-level `#[non_exhaustive]`; the
    // serde representation is the public wire format, so deserialize.
    serde_json::from_str(r#"{"Failed":{"kind":"InternalError","detail":null}}"#)
        .expect("Failed variant deserializes")
}

fn arb_payload() -> impl Strategy<Value = MessagePayload> {
    let failed = failed_status();
    prop_oneof![
        ".{0,32}".prop_map(|s| MessagePayload::Text { content: s }),
        arb_value().prop_map(|v| MessagePayload::ToolCall { args: v }),
        arb_value().prop_map(|v| MessagePayload::ToolResult { body: v }),
        Just(MessagePayload::Lifecycle(AgentStatus::Ready)),
        Just(MessagePayload::Lifecycle(failed)),
    ]
}

fn arb_message() -> impl Strategy<Value = Message> {
    arb_payload().prop_map(|payload| {
        // `Message` is `#[non_exhaustive]`; build via fixture and overwrite
        // the public `payload` field.
        let mut m = any_message();
        m.payload = payload;
        m
    })
}

proptest! {
    #[test]
    fn message_round_trips_through_json(m in arb_message()) {
        let s = serde_json::to_string(&m).unwrap();
        let back: Message = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(m, back);
    }
}
