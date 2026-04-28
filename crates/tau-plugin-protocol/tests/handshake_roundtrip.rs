//! Integration tests: handshake payload round-trip via rmp-serde.

use std::collections::BTreeMap;

use tau_domain::PortKind;
use tau_plugin_protocol::{HandshakeRequest, HandshakeResponse, MethodSchema, TraceContext};

#[test]
fn handshake_request_round_trip() {
    let req = HandshakeRequest::new(
        "1".to_string(),
        PortKind::LlmBackend,
        TraceContext::new(
            "01HXY".to_string(),
            "reviewer".to_string(),
            "abc123".to_string(),
        ),
        serde_json::json!({ "canned_text": "hello" }),
    );
    let bytes = rmp_serde::to_vec(&req).unwrap();
    let back: HandshakeRequest = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(req, back);
}

#[test]
fn handshake_response_round_trip() {
    let mut schemas = BTreeMap::new();
    schemas.insert(
        "llm.complete".to_string(),
        MethodSchema::new(
            serde_json::json!({ "type": "object" }),
            serde_json::json!({ "type": "object" }),
        ),
    );
    let resp = HandshakeResponse::new(
        "1".to_string(),
        PortKind::LlmBackend,
        "echo-llm".to_string(),
        "0.1.0".to_string(),
        vec![
            "llm.complete".to_string(),
            "llm.complete_streaming".to_string(),
        ],
        schemas,
    );
    let bytes = rmp_serde::to_vec(&resp).unwrap();
    let back: HandshakeResponse = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(resp, back);
}

#[test]
fn port_kind_serializes_as_string_through_msgpack() {
    let req = HandshakeRequest::new(
        "1".to_string(),
        PortKind::Tool,
        TraceContext::new("r".to_string(), "a".to_string(), "s".to_string()),
        serde_json::Value::Null,
    );
    let bytes = rmp_serde::to_vec(&req).unwrap();
    // Verify the wire bytes contain the literal string "tool"
    let bytes_as_string = String::from_utf8_lossy(&bytes);
    assert!(
        bytes_as_string.contains("tool"),
        "expected msgpack to contain 'tool', got: {:?}",
        bytes
    );
}
