//! Conformance case: rate-limited error path.
//!
//! Asserts:
//! - The cassette returns 429s exhausting the plugin's retry budget.
//! - The result is `Err(LlmError::RateLimited { .. })`.

use std::path::Path;

use tau_plugin_test_support::cassette;
use tau_ports::{CompletionRequest, ContentBlock, LlmBackend, LlmError, LlmProviderMessage};

pub(crate) async fn run<B, F>(build_plugin: &F, cassettes_dir: &Path)
where
    B: LlmBackend,
    F: Fn(String) -> B + Send + Sync,
{
    let server = cassette::replay(cassettes_dir.join("error_rate_limited.yaml")).await;
    let plugin = build_plugin(server.uri().into());

    let mut req = CompletionRequest::new("conformance-model".into());
    req.messages
        .push(LlmProviderMessage::user(vec![ContentBlock::Text(
            "hi".into(),
        )]));
    req.max_tokens = Some(20);

    let err = match plugin.complete(req).await {
        Ok(_) => panic!("[conformance error_rate_limited] expected Err, got Ok"),
        Err(e) => e,
    };
    let LlmError::RateLimited { .. } = err else {
        panic!("[conformance error_rate_limited] expected LlmError::RateLimited, got {err:?}",);
    };
}
