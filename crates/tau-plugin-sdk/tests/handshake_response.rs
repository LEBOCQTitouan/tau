//! Integration tests for `tau_plugin_sdk::handshake::drive_handshake`.

use std::collections::BTreeMap;

use tau_domain::PortKind;
use tau_plugin_protocol::{
    handshake::{meta, HandshakeRequest, HandshakeResponse, MethodSchema, TraceContext},
    test_support::FakeStdioPeer,
    Frame, PROTOCOL_VERSION,
};
use tau_plugin_sdk::{
    handshake::{drive_handshake, PluginMeta},
    SdkError,
};

#[tokio::test]
async fn drive_handshake_happy_path() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();

    // Spawn the SUT-side handshake driver.
    let plugin_meta = PluginMeta::new(
        "echo-llm".to_string(),
        "0.1.0".to_string(),
        PortKind::LlmBackend,
        vec!["llm.complete".to_string()],
        {
            let mut schemas = BTreeMap::new();
            schemas.insert(
                "llm.complete".to_string(),
                MethodSchema::new(serde_json::json!({}), serde_json::json!({})),
            );
            schemas
        },
    );

    let driver_task = tokio::spawn(async move {
        drive_handshake(&mut sut_reader, &mut sut_writer, plugin_meta).await
    });

    // Peer (acting as host) sends a handshake request.
    let req = HandshakeRequest::new(
        PROTOCOL_VERSION.to_string(),
        PortKind::LlmBackend,
        TraceContext::new("r".into(), "a".into(), "s".into()),
        serde_json::json!({ "canned_text": "hi" }),
    );
    let params_bytes = rmp_serde::to_vec(&vec![&req]).unwrap();
    let request_frame = Frame::Request {
        id: 1,
        method: meta::HANDSHAKE_METHOD.to_string(),
        params: params_bytes,
    };
    peer.writer
        .write_frame(&request_frame.encode().unwrap())
        .await
        .unwrap();

    // Peer reads the plugin's response.
    let response_body = peer.reader.next_frame().await.unwrap().unwrap();
    let response_frame = Frame::decode(&response_body).unwrap();
    let Frame::Response { id, error, result } = response_frame else {
        panic!("expected Response")
    };
    assert_eq!(id, 1);
    assert!(error.is_none());
    let result_bytes = result.unwrap();
    let parsed_resp: HandshakeResponse = rmp_serde::from_slice(&result_bytes).unwrap();
    assert_eq!(parsed_resp.provides, PortKind::LlmBackend);
    assert_eq!(parsed_resp.plugin_name, "echo-llm");
    assert_eq!(parsed_resp.plugin_version, "0.1.0");
    assert_eq!(parsed_resp.methods, vec!["llm.complete"]);

    // Driver task should complete successfully.
    let driver_result = driver_task.await.unwrap().unwrap();
    assert_eq!(driver_result, req);
}

#[tokio::test]
async fn drive_handshake_port_mismatch() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();

    let plugin_meta = PluginMeta::new(
        "echo-llm".to_string(),
        "0.1.0".to_string(),
        PortKind::LlmBackend, // Plugin says LlmBackend
        vec![],
        BTreeMap::new(),
    );

    let driver_task = tokio::spawn(async move {
        drive_handshake(&mut sut_reader, &mut sut_writer, plugin_meta).await
    });

    // Peer requests a Tool plugin, not LlmBackend -> mismatch.
    let req = HandshakeRequest::new(
        PROTOCOL_VERSION.to_string(),
        PortKind::Tool,
        TraceContext::new("r".into(), "a".into(), "s".into()),
        serde_json::Value::Null,
    );
    let params_bytes = rmp_serde::to_vec(&vec![&req]).unwrap();
    let request_frame = Frame::Request {
        id: 2,
        method: meta::HANDSHAKE_METHOD.to_string(),
        params: params_bytes,
    };
    peer.writer
        .write_frame(&request_frame.encode().unwrap())
        .await
        .unwrap();

    // Peer reads the error response.
    let response_body = peer.reader.next_frame().await.unwrap().unwrap();
    let response_frame = Frame::decode(&response_body).unwrap();
    let Frame::Response { id, error, result } = response_frame else {
        panic!("expected Response")
    };
    assert_eq!(id, 2);
    assert!(error.is_some());
    let envelope = error.unwrap();
    assert!(envelope.message.contains("port mismatch"));
    assert!(result.is_none());

    // Driver task should return SdkError::HandshakePortMismatch.
    let driver_result = driver_task.await.unwrap();
    assert!(matches!(
        driver_result,
        Err(SdkError::HandshakePortMismatch { .. })
    ));
}

#[tokio::test]
async fn drive_handshake_eof_before_request_returns_handshake_missing() {
    let (peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();
    let plugin_meta = PluginMeta::new(
        "echo-llm".to_string(),
        "0.1.0".to_string(),
        PortKind::LlmBackend,
        vec![],
        BTreeMap::new(),
    );

    // Drop the peer to close both halves -> SUT sees EOF before any frame.
    drop(peer);

    let result = drive_handshake(&mut sut_reader, &mut sut_writer, plugin_meta).await;
    assert!(matches!(result, Err(SdkError::HandshakeMissing)));
}
