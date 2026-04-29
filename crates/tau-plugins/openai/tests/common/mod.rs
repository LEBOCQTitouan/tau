//! Test helpers shared across the integration test files in
//! `crates/tau-plugins/openai/tests/`.

#![allow(dead_code)]

// `cassette` is provided by the shared `tau-plugin-test-support` crate.
// Re-exported under the local name so existing test imports
// (`use common::cassette;`) keep working.
pub mod cassette {
    #[allow(unused_imports)]
    pub use tau_plugin_test_support::cassette::*;
}

use openai_plugin_lib::config::OpenAIConfig;
use tau_ports::{CompletionRequest, CompletionResponse, ContentBlock, LlmProviderMessage};

/// Build a small CompletionRequest used as the canonical input across
/// happy-path tests.
pub fn sample_request() -> CompletionRequest {
    let mut req = CompletionRequest::new("gpt-4o-mini".into());
    req.messages
        .push(LlmProviderMessage::user(vec![ContentBlock::Text(
            "say hi".into(),
        )]));
    req.max_tokens = Some(20);
    req
}

/// Extract the `text` field from a `CompletionResponse`.
pub fn extract_text(resp: &CompletionResponse) -> &str {
    &resp.text
}

/// Build an `OpenAIConfig` pointing at the given `base_url` with a
/// fake `sk-test` API key. Used by cassette tests.
pub fn test_config(base_url: String) -> OpenAIConfig {
    let mut cfg = OpenAIConfig::default();
    cfg.api_key = Some("sk-test".into());
    cfg.base_url = base_url;
    cfg
}

/// As `test_config` but with retry overrides for retry-loop tests.
pub fn test_config_with_retry(
    base_url: String,
    max_attempts: u32,
    base_delay_ms: u64,
) -> OpenAIConfig {
    let mut cfg = test_config(base_url);
    cfg.retry.max_attempts = max_attempts;
    cfg.retry.base_delay_ms = base_delay_ms;
    cfg
}
