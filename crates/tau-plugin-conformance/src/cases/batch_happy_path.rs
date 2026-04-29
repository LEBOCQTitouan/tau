//! Conformance case: batch happy path.
//!
//! Asserts:
//! - `complete()` returns Ok.
//! - `text` is non-empty.
//! - `stop_reason` is one of `EndTurn | MaxTokens | StopSequence`.

use std::path::Path;

use tau_plugin_test_support::cassette;
use tau_ports::{CompletionRequest, ContentBlock, LlmBackend, LlmProviderMessage, StopReason};

pub(crate) async fn run<B, F>(build_plugin: &F, cassettes_dir: &Path)
where
    B: LlmBackend,
    F: Fn(String) -> B + Send + Sync,
{
    let server = cassette::replay(cassettes_dir.join("batch_happy_path.yaml")).await;
    let plugin = build_plugin(server.uri().into());

    let mut req = CompletionRequest::new("conformance-model".into());
    req.messages
        .push(LlmProviderMessage::user(vec![ContentBlock::Text(
            "hi".into(),
        )]));
    req.max_tokens = Some(20);

    let resp = match plugin.complete(req).await {
        Ok(r) => r,
        Err(e) => panic!("[conformance batch_happy_path] complete() failed: {e:?}"),
    };
    assert!(
        !resp.text.is_empty(),
        "[conformance batch_happy_path] expected non-empty text",
    );
    assert!(
        matches!(
            resp.stop_reason,
            StopReason::EndTurn | StopReason::MaxTokens | StopReason::StopSequence,
        ),
        "[conformance batch_happy_path] expected stop_reason in \
         {{EndTurn, MaxTokens, StopSequence}}, got {:?}",
        resp.stop_reason,
    );
}
