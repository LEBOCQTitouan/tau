//! Integration tests: OllamaPlugin::complete against cassette replayer.

mod common;

use common::cassette;
use ollama_plugin_lib::plugin::OllamaPlugin;
use tau_plugin_sdk::Configure;
use tau_ports::{LlmBackend, LlmError};

#[tokio::test]
async fn complete_happy_path() {
    let server = cassette::replay("tests/cassettes/complete_happy_path.yaml").await;
    let plugin = OllamaPlugin::from_config(common::test_config(server.uri().into())).unwrap();
    let resp = plugin.complete(common::sample_request()).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Hi there");
    let usage = resp.usage.expect("ollama returned both counts");
    assert_eq!(usage.input_tokens, 12);
    assert_eq!(usage.output_tokens, 3);
    assert_eq!(server.received_requests().len(), 1);
}

#[tokio::test]
async fn complete_with_system_prompt() {
    let server = cassette::replay("tests/cassettes/complete_with_system_prompt.yaml").await;
    let plugin = OllamaPlugin::from_config(common::test_config(server.uri().into())).unwrap();
    let mut req = common::sample_request();
    req.system = Some("you are concise".into());
    let resp = plugin.complete(req).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Concise.");

    // Verify the request body the plugin sent contained a leading
    // role:system message in the messages array (Ollama-specific
    // shape — NOT a top-level `system` field like Anthropic).
    let received = server.received_requests();
    assert_eq!(received.len(), 1);
    // The serializer may order fields differently; check both key orderings.
    assert!(
        received[0]
            .body
            .contains(r#""role":"system","content":"you are concise""#)
            || received[0]
                .body
                .contains(r#""content":"you are concise","role":"system""#),
        "expected leading role:system message in request body; got: {}",
        received[0].body,
    );
    // Also assert the body does NOT contain a top-level "system" field
    // (defensive — Ollama doesn't accept that shape).
    assert!(
        !received[0].body.contains(r#""system":"you are concise""#),
        "expected NO top-level system field; got: {}",
        received[0].body,
    );
}

#[tokio::test]
async fn complete_with_tools_synthesizes_tool_use_id() {
    let server = cassette::replay("tests/cassettes/complete_with_tools.yaml").await;
    let plugin = OllamaPlugin::from_config(common::test_config(server.uri().into())).unwrap();
    let mut req = common::sample_request();
    // ToolSpec is #[non_exhaustive]; use the test-fixtures helper.
    req.tools.push(tau_ports::fixtures::make_tool_spec(
        "echo".into(),
        "echo input".into(),
        tau_domain::Value::Object(Default::default()),
    ));
    let resp = plugin.complete(req).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Looking up...");
    assert_eq!(resp.tool_uses.len(), 1);
    // Ollama doesn't include a tool_call.id; the parser synthesizes:
    assert_eq!(resp.tool_uses[0].id, "ollama-tool-0");
    assert_eq!(resp.tool_uses[0].name, "echo");
}

#[tokio::test]
async fn complete_503_model_loading_then_success_retries() {
    // THE LOAD-BEARING OLLAMA RETRY CASE:
    // 2× 503 (model loading) then 200; assert all 3 attempts were
    // received and the eventual success path returned the right text.
    let server =
        cassette::replay("tests/cassettes/complete_503_model_loading_then_success.yaml").await;
    let plugin =
        OllamaPlugin::from_config(common::test_config_with_retry(server.uri().into(), 3, 0))
            .unwrap();
    let resp = plugin.complete(common::sample_request()).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Hi after model load");
    assert_eq!(
        server.received_requests().len(),
        3,
        "should have 3 attempts (initial + 2 retries on 503)"
    );
}

#[tokio::test]
async fn complete_404_model_not_pulled_includes_remediation_hint() {
    let server = cassette::replay("tests/cassettes/complete_404_model_not_pulled.yaml").await;
    let plugin =
        OllamaPlugin::from_config(common::test_config_with_retry(server.uri().into(), 3, 0))
            .unwrap();
    let err = plugin.complete(common::sample_request()).await.unwrap_err();
    let LlmError::InvalidRequest { ref reason } = err else {
        panic!("expected InvalidRequest, got {err:?}");
    };
    assert!(
        reason.contains("ollama pull"),
        "expected `ollama pull` remediation hint; got: {reason}"
    );
    // 404 must NOT retry.
    assert_eq!(server.received_requests().len(), 1, "404 must not retry");
}

#[tokio::test]
async fn complete_400_bad_request_does_not_retry() {
    let server = cassette::replay("tests/cassettes/complete_400_bad_request.yaml").await;
    let plugin =
        OllamaPlugin::from_config(common::test_config_with_retry(server.uri().into(), 3, 0))
            .unwrap();
    let err = plugin.complete(common::sample_request()).await.unwrap_err();
    let LlmError::InvalidRequest { ref reason } = err else {
        panic!("expected InvalidRequest, got {err:?}");
    };
    assert!(
        reason.contains("ollama bad request") || reason.contains("bad request"),
        "unexpected error reason: {reason}"
    );
    assert_eq!(server.received_requests().len(), 1, "400 must not retry");
}
