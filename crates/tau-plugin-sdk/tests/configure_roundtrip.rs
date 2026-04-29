//! Integration test: drive `run_llm_backend_with_config_with_io` end-to-end
//! via a `FakeStdioPeer`, asserting the runner threads
//! `HandshakeRequest.config` through to the plugin's
//! [`Configure::from_config`] and that the constructed plugin responds
//! to `llm.complete` with values derived from that config.

use serde::Deserialize;

use tau_domain::PortKind;
use tau_plugin_protocol::{
    handshake::{meta, HandshakeRequest, HandshakeResponse, TraceContext},
    test_support::FakeStdioPeer,
    Frame, PROTOCOL_VERSION,
};
use tau_plugin_sdk::{run_llm_backend_with_config_with_io, ConfigError, Configure, SdkError};
use tau_ports::{
    fixtures::{make_completion_request, make_completion_response, make_token_usage},
    CompletionRequest, CompletionResponse, CompletionStream, LlmBackend, LlmError, StopReason,
};

/// Config consumed by [`EchoPlugin`] from the handshake.
#[derive(Deserialize)]
struct EchoConfig {
    canned_text: String,
    /// Optional usage hint; we just round-trip it through the response
    /// so the test can verify config wiring beyond a single string.
    #[serde(default)]
    output_tokens: Option<u32>,
}

/// Plugin that constructs itself from [`EchoConfig`] and echoes the
/// canned text back in `llm.complete`.
struct EchoPlugin {
    canned_text: String,
    output_tokens: Option<u32>,
}

impl Configure for EchoPlugin {
    type Config = EchoConfig;

    fn from_config(config: Self::Config) -> Result<Self, ConfigError> {
        if config.canned_text.is_empty() {
            return Err(ConfigError::MissingField("canned_text"));
        }
        Ok(EchoPlugin {
            canned_text: config.canned_text,
            output_tokens: config.output_tokens,
        })
    }
}

impl LlmBackend for EchoPlugin {
    fn name(&self) -> &str {
        "echo-plugin"
    }

    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let usage = self.output_tokens.map(|out| make_token_usage(0, out));
        Ok(make_completion_response(
            self.canned_text.clone(),
            Vec::new(),
            StopReason::EndTurn,
            usage,
        ))
    }

    async fn stream(&self, _req: CompletionRequest) -> Result<CompletionStream, LlmError> {
        // Streaming is not exercised by this test; surface a typed error
        // if the host happens to call us.
        Err(LlmError::Internal {
            message: "test plugin doesn't implement streaming".to_string(),
        })
    }
}

#[tokio::test]
async fn run_llm_backend_with_config_uses_handshake_config() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();

    // Spawn the runner with EchoPlugin's Configure shape.
    let runner = tokio::spawn(async move {
        run_llm_backend_with_config_with_io::<_, _, EchoPlugin>(
            &mut sut_reader,
            &mut sut_writer,
            "echo-llm",
            "0.1.0",
        )
        .await
    });

    // ---- Step 1: handshake with non-empty config ----
    let req = HandshakeRequest::new(
        PROTOCOL_VERSION.to_string(),
        PortKind::LlmBackend,
        TraceContext::new("r".into(), "a".into(), "s".into()),
        serde_json::json!({
            "canned_text": "hello from config",
            "output_tokens": 17,
        }),
    );
    let params_bytes = rmp_serde::to_vec(&vec![&req]).unwrap();
    let handshake_frame = Frame::Request {
        id: 1,
        method: meta::HANDSHAKE_METHOD.to_string(),
        params: params_bytes,
    };
    peer.writer
        .write_frame(&handshake_frame.encode().unwrap())
        .await
        .unwrap();

    let body = peer.reader.next_frame().await.unwrap().unwrap();
    let frame = Frame::decode(&body).unwrap();
    let Frame::Response { id, error, result } = frame else {
        panic!("expected handshake response, got {frame:?}")
    };
    assert_eq!(id, 1);
    assert!(error.is_none(), "handshake error: {error:?}");
    let resp: HandshakeResponse = rmp_serde::from_slice(&result.unwrap()).unwrap();
    assert_eq!(resp.provides, PortKind::LlmBackend);
    assert_eq!(resp.plugin_name, "echo-llm");

    // ---- Step 2: send `llm.complete`, expect canned_text from config ----
    let complete_req = make_completion_request("test-model".to_string());
    let params_bytes = rmp_serde::to_vec(&vec![&complete_req]).unwrap();
    let complete_frame = Frame::Request {
        id: 2,
        method: "llm.complete".to_string(),
        params: params_bytes,
    };
    peer.writer
        .write_frame(&complete_frame.encode().unwrap())
        .await
        .unwrap();

    let body = peer.reader.next_frame().await.unwrap().unwrap();
    let frame = Frame::decode(&body).unwrap();
    let Frame::Response { id, error, result } = frame else {
        panic!("expected llm.complete response, got {frame:?}")
    };
    assert_eq!(id, 2);
    assert!(error.is_none(), "complete error: {error:?}");
    let resp: CompletionResponse = rmp_serde::from_slice(&result.unwrap()).unwrap();
    assert_eq!(resp.text, "hello from config");
    assert_eq!(resp.stop_reason, StopReason::EndTurn);
    assert_eq!(resp.usage, Some(make_token_usage(0, 17)));

    // ---- Step 3: shutdown ----
    let shutdown_frame = Frame::Notification {
        method: meta::SHUTDOWN_METHOD.to_string(),
        params: rmp_serde::to_vec::<Vec<()>>(&Vec::new()).unwrap(),
    };
    peer.writer
        .write_frame(&shutdown_frame.encode().unwrap())
        .await
        .unwrap();

    runner.await.expect("runner task join").expect("runner ok");
}

#[tokio::test]
async fn run_llm_backend_with_config_propagates_missing_field() {
    let (mut peer, mut sut_reader, mut sut_writer) = FakeStdioPeer::new();

    let runner = tokio::spawn(async move {
        run_llm_backend_with_config_with_io::<_, _, EchoPlugin>(
            &mut sut_reader,
            &mut sut_writer,
            "echo-llm",
            "0.1.0",
        )
        .await
    });

    // Send handshake with empty `canned_text` so `from_config` returns
    // ConfigError::MissingField.
    let req = HandshakeRequest::new(
        PROTOCOL_VERSION.to_string(),
        PortKind::LlmBackend,
        TraceContext::new("r".into(), "a".into(), "s".into()),
        serde_json::json!({ "canned_text": "" }),
    );
    let params_bytes = rmp_serde::to_vec(&vec![&req]).unwrap();
    let handshake_frame = Frame::Request {
        id: 1,
        method: meta::HANDSHAKE_METHOD.to_string(),
        params: params_bytes,
    };
    peer.writer
        .write_frame(&handshake_frame.encode().unwrap())
        .await
        .unwrap();

    // The runner first sends back the handshake response (success: the
    // handshake itself is valid; from_config runs after).
    let _handshake_body = peer.reader.next_frame().await.unwrap().unwrap();

    // The runner should bail with SdkError::Configure(ConfigError::MissingField).
    let outcome = runner.await.expect("runner task join");
    match outcome {
        Err(SdkError::Configure(ConfigError::MissingField(field))) => {
            assert_eq!(field, "canned_text");
        }
        other => panic!("expected SdkError::Configure(MissingField), got {other:?}"),
    }
}
