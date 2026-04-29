//! Integration tests: AnthropicPlugin::complete against cassette replayer.

mod common;

use anthropic_plugin_lib::plugin::AnthropicPlugin;
use common::cassette;
use tau_plugin_sdk::Configure;
use tau_ports::{LlmBackend, LlmError};

#[tokio::test]
async fn complete_happy_path() {
    let server = cassette::replay("tests/cassettes/complete_happy_path.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config(server.uri().into())).unwrap();
    let resp = plugin.complete(common::sample_request()).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Hi there");
    let usage = resp.usage.expect("anthropic always returns usage");
    assert_eq!(usage.input_tokens, 12);
    assert_eq!(usage.output_tokens, 3);
    assert_eq!(server.received_requests().len(), 1);
}

#[tokio::test]
async fn complete_with_system_prompt() {
    let server = cassette::replay("tests/cassettes/complete_with_system_prompt.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config(server.uri().into())).unwrap();
    let mut req = common::sample_request();
    req.system = Some("you are concise".into());
    let resp = plugin.complete(req).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Concise.");

    // Verify the request body the plugin sent contained the top-level
    // "system" field with our value.
    let received = server.received_requests();
    assert_eq!(received.len(), 1);
    assert!(
        received[0].body.contains(r#""system":"you are concise""#),
        "expected system field in request body; got: {}",
        received[0].body,
    );
}

#[tokio::test]
async fn complete_with_tools() {
    let server = cassette::replay("tests/cassettes/complete_with_tools.yaml").await;
    let plugin = AnthropicPlugin::from_config(common::test_config(server.uri().into())).unwrap();
    let mut req = common::sample_request();
    // `ToolSpec` is `#[non_exhaustive]`, so external compilation-unit
    // tests can't use struct-literal construction. Use the
    // `tau-ports` test-fixtures helper.
    req.tools.push(tau_ports::fixtures::make_tool_spec(
        "echo".into(),
        "echo input".into(),
        tau_domain::Value::Object(Default::default()),
    ));
    let resp = plugin.complete(req).await.unwrap();
    assert_eq!(common::extract_text(&resp), "Looking up...");
    assert_eq!(resp.tool_uses.len(), 1);
    assert_eq!(resp.tool_uses[0].id, "toolu_01");
    assert_eq!(resp.tool_uses[0].name, "echo");
}

#[tokio::test]
async fn complete_429_then_success() {
    let server = cassette::replay("tests/cassettes/complete_429_then_success.yaml").await;
    let plugin =
        AnthropicPlugin::from_config(common::test_config_with_retry(server.uri().into(), 3, 0))
            .unwrap();
    let resp = plugin.complete(common::sample_request()).await.unwrap();
    assert!(common::extract_text(&resp).contains("Hi after retry"));
    assert_eq!(server.received_requests().len(), 3);
}

#[tokio::test]
async fn complete_429_exhausted_returns_internal_error() {
    let server = cassette::replay("tests/cassettes/complete_429_exhausted.yaml").await;
    let plugin =
        AnthropicPlugin::from_config(common::test_config_with_retry(server.uri().into(), 3, 0))
            .unwrap();
    let err = plugin.complete(common::sample_request()).await.unwrap_err();
    let LlmError::Internal { ref message } = err else {
        panic!("expected Internal, got {err:?}");
    };
    assert!(
        message.contains("rate limited") || message.contains("retries exhausted"),
        "unexpected error message: {message}",
    );
    assert_eq!(server.received_requests().len(), 3);
}

#[tokio::test]
async fn complete_401_auth_failure_does_not_retry() {
    let server = cassette::replay("tests/cassettes/complete_401_auth_failure.yaml").await;
    let plugin =
        AnthropicPlugin::from_config(common::test_config_with_retry(server.uri().into(), 3, 0))
            .unwrap();
    let err = plugin.complete(common::sample_request()).await.unwrap_err();
    let LlmError::Internal { ref message } = err else {
        panic!("expected Internal, got {err:?}");
    };
    assert!(
        message.contains("auth failure"),
        "unexpected error message: {message}",
    );
    assert_eq!(server.received_requests().len(), 1, "401 must not retry");
}

#[tokio::test]
async fn complete_400_bad_request_does_not_retry() {
    let server = cassette::replay("tests/cassettes/complete_400_bad_request.yaml").await;
    let plugin =
        AnthropicPlugin::from_config(common::test_config_with_retry(server.uri().into(), 3, 0))
            .unwrap();
    let err = plugin.complete(common::sample_request()).await.unwrap_err();
    let LlmError::Internal { ref message } = err else {
        panic!("expected Internal, got {err:?}");
    };
    assert!(
        message.contains("bad request"),
        "unexpected error message: {message}",
    );
    assert_eq!(server.received_requests().len(), 1, "400 must not retry");
}
