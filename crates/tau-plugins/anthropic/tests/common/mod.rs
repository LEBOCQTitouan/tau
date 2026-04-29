//! Test helpers shared across the integration test files in
//! `crates/tau-plugins/anthropic/tests/`.

#![allow(dead_code)]

pub mod cassette;

use anthropic_plugin_lib::config::AnthropicConfig;
use tau_ports::{CompletionRequest, CompletionResponse, ContentBlock, LlmProviderMessage};

/// Build a small CompletionRequest used as the canonical input across
/// happy-path tests. Tests that need to vary fields clone + mutate.
pub fn sample_request() -> CompletionRequest {
    let mut req = CompletionRequest::new("claude-3-5-haiku-latest".into());
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

/// Build an `AnthropicConfig` pointing at the given base_url with a
/// fake `sk-ant-` API key. Used by cassette tests that point at the
/// local replayer.
pub fn test_config(base_url: String) -> AnthropicConfig {
    // `AnthropicConfig` is `#[non_exhaustive]`: from outside the crate
    // we can't use struct-literal construction. Build via Default and
    // mutate.
    let mut cfg = AnthropicConfig::default();
    cfg.api_key = Some("sk-ant-test".into());
    cfg.base_url = base_url;
    cfg
}

/// As `test_config` but with retry overrides for retry-loop tests.
pub fn test_config_with_retry(
    base_url: String,
    max_attempts: u32,
    base_delay_ms: u64,
) -> AnthropicConfig {
    let mut cfg = test_config(base_url);
    cfg.retry.max_attempts = max_attempts;
    cfg.retry.base_delay_ms = base_delay_ms;
    cfg
}
