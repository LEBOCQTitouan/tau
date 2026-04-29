//! Conformance case: auth-failure error path.
//!
//! Asserts:
//! - The cassette returns 401.
//! - The result is `Err(LlmError::Auth { .. })`.

use std::path::Path;

use tau_plugin_test_support::cassette;
use tau_ports::{CompletionRequest, ContentBlock, LlmBackend, LlmError, LlmProviderMessage};

pub(crate) async fn run<B, F>(build_plugin: &F, cassettes_dir: &Path)
where
    B: LlmBackend,
    F: Fn(String) -> B + Send + Sync,
{
    let server = cassette::replay(cassettes_dir.join("error_auth.yaml")).await;
    let plugin = build_plugin(server.uri().into());

    let mut req = CompletionRequest::new("conformance-model".into());
    req.messages
        .push(LlmProviderMessage::user(vec![ContentBlock::Text(
            "hi".into(),
        )]));
    req.max_tokens = Some(20);

    let err = match plugin.complete(req).await {
        Ok(_) => panic!("[conformance error_auth] expected Err, got Ok"),
        Err(e) => e,
    };
    let LlmError::Auth { .. } = err else {
        panic!("[conformance error_auth] expected LlmError::Auth, got {err:?}");
    };
}
