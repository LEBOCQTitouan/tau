//! Test helpers shared across the integration test files in
//! `crates/tau-plugins/ollama/tests/`.

#![allow(dead_code)]

pub mod cassette;

use ollama_plugin_lib::config::OllamaConfig;
use tau_ports::{CompletionRequest, CompletionResponse, ContentBlock, LlmProviderMessage};

/// Build a small CompletionRequest used as the canonical input across
/// happy-path tests. Tests that need to vary fields clone + mutate.
pub fn sample_request() -> CompletionRequest {
    let mut req = CompletionRequest::new("llama3.2".into());
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

/// Build an `OllamaConfig` pointing at the given base_url with NO
/// bearer token set. Used by cassette tests that point at the local
/// replayer. The `bearer_token_env` is set to a definitely-unset name
/// so cassette tests are insulated from an ambient real
/// `OLLAMA_BEARER_TOKEN` env var.
pub fn test_config(base_url: String) -> OllamaConfig {
    // `OllamaConfig` is `#[non_exhaustive]`: from outside the crate
    // we can't use struct-literal construction. Build via Default and
    // mutate.
    let mut cfg = OllamaConfig::default();
    cfg.base_url = base_url;
    cfg.bearer_token_env = "OLLAMA_BEARER_TOKEN_DEFINITELY_NOT_SET_FOR_TESTS".into();
    cfg
}

/// As `test_config` but with retry overrides for retry-loop tests.
pub fn test_config_with_retry(
    base_url: String,
    max_attempts: u32,
    base_delay_ms: u64,
) -> OllamaConfig {
    let mut cfg = test_config(base_url);
    cfg.retry.max_attempts = max_attempts;
    cfg.retry.base_delay_ms = base_delay_ms;
    cfg
}
