//! Integration test: every `MessagePayload` variant round-trips through
//! `serde_json`.
//!
//! Constructs base messages via `tau_domain::fixtures::any_message()` and
//! overwrites the public `payload` field. Direct struct-literal construction
//! is blocked from outside the crate (E0639) because `Message` is
//! `#[non_exhaustive]`. The same goes for the `AgentStatus::Failed` variant,
//! which we obtain by deserializing its public wire format.

#![cfg(feature = "test-fixtures")]

use tau_domain::fixtures::any_message;
use tau_domain::{AgentStatus, Message, MessagePayload, Value};

fn envelope(payload: MessagePayload) -> Message {
    let mut m = any_message();
    m.payload = payload;
    m
}

fn round_trips(m: Message) {
    let s = serde_json::to_string(&m).unwrap();
    let back: Message = serde_json::from_str(&s).unwrap();
    assert_eq!(m, back);
}

#[test]
fn text() {
    round_trips(envelope(MessagePayload::Text {
        content: "hi".into(),
    }));
}

#[test]
fn tool_call() {
    round_trips(envelope(MessagePayload::ToolCall {
        args: Value::String("read /tmp/foo".into()),
    }));
}

#[test]
fn tool_result() {
    round_trips(envelope(MessagePayload::ToolResult {
        body: Value::Integer(42),
    }));
}

#[test]
fn tool_error() {
    // `details: Option<Value>` — `Some(Value::Null)` is NOT round-trip stable
    // because serde_json's default `Option` deserializer maps a wire `null`
    // back to `None`. Use a non-null payload so round-trip equality holds.
    round_trips(envelope(MessagePayload::ToolError {
        kind: "io".into(),
        message: "permission denied".into(),
        details: Some(Value::String("path: /etc/shadow".into())),
    }));
}

#[test]
fn tool_error_none_details() {
    round_trips(envelope(MessagePayload::ToolError {
        kind: "io".into(),
        message: "permission denied".into(),
        details: None,
    }));
}

#[test]
fn lifecycle_ready() {
    round_trips(envelope(MessagePayload::Lifecycle(AgentStatus::Ready)));
}

#[test]
fn lifecycle_failed() {
    // `AgentStatus::Failed` is variant-level `#[non_exhaustive]`; the
    // serde representation is the public wire format, so deserialize.
    let failed: AgentStatus =
        serde_json::from_str(r#"{"Failed":{"kind":"Crashed","detail":"SIGSEGV"}}"#)
            .expect("Failed variant deserializes");
    round_trips(envelope(MessagePayload::Lifecycle(failed)));
}

#[test]
fn custom() {
    round_trips(envelope(MessagePayload::Custom {
        kind: "mcp.tool.use".into(),
        body: vec![1, 2, 3],
    }));
}
