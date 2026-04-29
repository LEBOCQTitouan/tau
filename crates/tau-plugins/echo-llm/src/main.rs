//! Toy `LlmBackend` plugin that replays canned responses from config.
//!
//! Used by tau-cli integration tests to exercise the plugin loading
//! mechanism end-to-end without depending on a real LLM provider.
//!
//! # Configuration
//!
//! Configurable via the handshake `config` field (set in
//! `[agents.<id>.config]` of the project tau.toml):
//!
//! - `canned_text: String` — single canned text returned by every
//!   `llm.complete` call. Default: empty string.
//! - `script: Vec<String>` — multi-turn script. Indexed by an internal
//!   atomic counter that increments on each `complete` call. If the
//!   counter exceeds the script length, falls back to `canned_text`.
//! - `crash_after_handshake: bool` — if `true`, panic at the start of
//!   any `complete`/`stream` call. The handshake itself completes; the
//!   panic surfaces at first dispatch. Used by failure-path tests.
//! - `delay_response_ms: Option<u64>` — sleep this many milliseconds
//!   before responding. Used by handshake/timeout tests.
//! - `error_on_method: Option<String>` — return `Err(LlmError::Internal)`
//!   when this method is called (e.g. `"llm.complete"` or
//!   `"llm.stream"`).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde::Deserialize;
use tau_plugin_sdk::{run_llm_backend_with_config, ConfigError, Configure, SdkError};
use tau_ports::{
    batch_to_stream, fixtures::make_completion_response, CompletionRequest, CompletionResponse,
    CompletionStream, LlmBackend, LlmError, StopReason,
};

/// Static configuration consumed from the handshake `config` field.
#[derive(Debug, Default, Deserialize)]
struct EchoConfig {
    /// Single canned text returned by every `llm.complete` call.
    #[serde(default)]
    canned_text: String,
    /// Multi-turn script; indexed by an atomic turn counter.
    #[serde(default)]
    script: Vec<String>,
    /// If `true`, panic at the start of any `complete`/`stream` call.
    #[serde(default)]
    crash_after_handshake: bool,
    /// Sleep this many milliseconds before responding. Used by
    /// handshake/timeout tests.
    #[serde(default)]
    delay_response_ms: Option<u64>,
    /// Return `Err(LlmError::Internal)` when this method is called.
    #[serde(default)]
    error_on_method: Option<String>,
}

/// Toy `LlmBackend` plugin.
struct EchoLlm {
    config: EchoConfig,
    turn: AtomicUsize,
}

impl Configure for EchoLlm {
    type Config = EchoConfig;

    fn from_config(config: Self::Config) -> Result<Self, ConfigError> {
        Ok(EchoLlm {
            config,
            turn: AtomicUsize::new(0),
        })
    }
}

impl EchoLlm {
    /// Apply the test-only side effects (`crash_after_handshake`,
    /// `error_on_method`, `delay_response_ms`) and produce the next
    /// canned text. Shared by `complete` and `stream`.
    async fn next_text(&self, method: &str) -> Result<String, LlmError> {
        if self.config.crash_after_handshake {
            panic!("echo-llm crash_after_handshake = true (test-only mode)");
        }
        if self.config.error_on_method.as_deref() == Some(method) {
            return Err(LlmError::Internal {
                message: format!("echo-llm error_on_method test mode tripped on {method}"),
            });
        }
        if let Some(ms) = self.config.delay_response_ms {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
        let i = self.turn.fetch_add(1, Ordering::Relaxed);
        let text = self
            .config
            .script
            .get(i)
            .cloned()
            .unwrap_or_else(|| self.config.canned_text.clone());
        Ok(text)
    }
}

impl LlmBackend for EchoLlm {
    fn name(&self) -> &str {
        "echo-llm"
    }

    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let text = self.next_text("llm.complete").await?;
        Ok(make_completion_response(
            text,
            Vec::new(),
            StopReason::EndTurn,
            None,
        ))
    }

    async fn stream(&self, _req: CompletionRequest) -> Result<CompletionStream, LlmError> {
        let text = self.next_text("llm.stream").await?;
        let resp = make_completion_response(text, Vec::new(), StopReason::EndTurn, None);
        Ok(batch_to_stream(resp))
    }
}

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_llm_backend_with_config::<EchoLlm>(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")).await
}
