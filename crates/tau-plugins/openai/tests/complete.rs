//! Integration tests: OpenAIPlugin::complete against cassette replayer.

mod common;

use assert_matches::assert_matches;
use common::cassette;
use openai_plugin_lib::plugin::OpenAIPlugin;
use tau_plugin_sdk::Configure;
use tau_ports::{LlmBackend, LlmError};

#[tokio::test]
async fn complete_happy_path() {
    let server = cassette::replay("tests/cassettes/complete_happy_path.yaml").await;
    let plugin = OpenAIPlugin::from_config(common::test_config(server.uri().into())).unwrap();
    let resp = plugin.complete(common::sample_request()).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Hi there");
    let usage = resp.usage.expect("openai always returns usage");
    assert_eq!(usage.input_tokens, 12);
    assert_eq!(usage.output_tokens, 3);
    assert_eq!(server.received_requests().len(), 1);
}

#[tokio::test]
async fn complete_with_system_prompt() {
    let server = cassette::replay("tests/cassettes/complete_with_system_prompt.yaml").await;
    let plugin = OpenAIPlugin::from_config(common::test_config(server.uri().into())).unwrap();
    let mut req = common::sample_request();
    req.system = Some("you are concise".into());
    let resp = plugin.complete(req).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Concise.");

    // Verify the request body the plugin sent contained a leading
    // role:system message in messages[] (matches Ollama; OpenAI has
    // no top-level system field).
    let received = server.received_requests();
    assert_eq!(received.len(), 1);
    let body = &received[0].body;
    assert!(
        body.contains(r#""role":"system""#) && body.contains(r#""content":"you are concise""#),
        "expected leading role:system message in request body; got: {body}",
    );
    // Defensive: NO top-level system field at body root.
    assert!(
        !body.contains(r#""system":"you are concise""#),
        "expected NO top-level system field; got: {body}",
    );
}

#[tokio::test]
async fn complete_with_tools_round_trips_tool_call_id() {
    let server = cassette::replay("tests/cassettes/complete_with_tools.yaml").await;
    let plugin = OpenAIPlugin::from_config(common::test_config(server.uri().into())).unwrap();
    let mut req = common::sample_request();
    req.tools.push(tau_ports::fixtures::make_tool_spec(
        "echo".into(),
        "echo input".into(),
        tau_domain::Value::Object(Default::default()),
    ));
    let resp = plugin.complete(req).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Looking up...");
    assert_eq!(resp.tool_uses.len(), 1);
    // OpenAI provides the real tool_call id; preserved (NOT synthesized).
    assert_eq!(resp.tool_uses[0].id, "call_abc");
    assert_eq!(resp.tool_uses[0].name, "echo");
}

#[tokio::test]
async fn complete_429_then_success_retries_with_typed_rate_limited_path() {
    let server = cassette::replay("tests/cassettes/complete_429_then_success.yaml").await;
    let plugin =
        OpenAIPlugin::from_config(common::test_config_with_retry(server.uri().into(), 3, 0))
            .unwrap();
    let resp = plugin.complete(common::sample_request()).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Hi after retry");
    assert_eq!(server.received_requests().len(), 2);
}

#[tokio::test]
async fn complete_401_returns_typed_auth_error() {
    let server = cassette::replay("tests/cassettes/complete_401_auth_failure.yaml").await;
    let plugin =
        OpenAIPlugin::from_config(common::test_config_with_retry(server.uri().into(), 3, 0))
            .unwrap();
    let err = plugin.complete(common::sample_request()).await.unwrap_err();
    assert_matches!(
        &err,
        LlmError::Auth { message } => {
            assert!(message.contains("Invalid API key"), "got: {message}");
        }
    );
    // 401 must NOT retry.
    assert_eq!(server.received_requests().len(), 1);
}

#[tokio::test]
async fn complete_400_returns_typed_invalid_request() {
    let server = cassette::replay("tests/cassettes/complete_400_bad_request.yaml").await;
    let plugin =
        OpenAIPlugin::from_config(common::test_config_with_retry(server.uri().into(), 3, 0))
            .unwrap();
    let err = plugin.complete(common::sample_request()).await.unwrap_err();
    assert_matches!(
        &err,
        LlmError::InvalidRequest { reason } => {
            assert!(reason.contains("openai bad request"), "got: {reason}");
        }
    );
    // 400 must NOT retry.
    assert_eq!(server.received_requests().len(), 1);
}
