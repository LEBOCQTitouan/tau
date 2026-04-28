//! Compiled-in mock LLM backend + echo tool, gated by `test-mock`.
//!
//! Configured via env vars (read fresh on each LLM call):
//!
//! - `TAU_MOCK_LLM_TEXT`: text the LLM "responds" with each turn.
//! - `TAU_MOCK_LLM_TOOL_USES`: comma-separated list of tool names to
//!   invoke. The mock emits these as `tool_use`s on **turn 0 only**;
//!   subsequent turns return only text so the loop terminates. Each
//!   tool_use carries `input = Value::Null`.
//! - `TAU_MOCK_LLM_STOP_REASON`: `"end_turn"` (default) | `"max_tokens"`
//!   | `"tool_use"`.
//!
//! Production builds (no features) do NOT include this module; `tau run`
//! fails with `BuildError::NoLlmBackend` until Phase 1+ plugin loading
//! lands.

use std::env;
use std::sync::{Arc, Mutex};

use tau_ports::{
    fixtures::{make_completion_response, make_tool_result, make_tool_spec, make_tool_use},
    CompletionRequest, CompletionResponse, CompletionStream, LlmBackend, LlmError, SessionContext,
    StopReason, Tool, ToolContent, ToolError, ToolResult, ToolSpec, ToolUse,
};

/// Mock [`LlmBackend`] driven by env vars. See module docs.
pub struct MockLlmBackend {
    name: String,
    /// Number of `complete` calls observed; used to gate `tool_uses` to
    /// turn 0 so the loop terminates without driving max_turns.
    invocations: Arc<Mutex<usize>>,
}

impl MockLlmBackend {
    /// Construct a fresh backend with the given registration name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            invocations: Arc::new(Mutex::new(0)),
        }
    }

    fn build_response(&self, turn: usize) -> CompletionResponse {
        let text = env::var("TAU_MOCK_LLM_TEXT").unwrap_or_default();
        let tool_uses: Vec<ToolUse> = env::var("TAU_MOCK_LLM_TOOL_USES")
            .ok()
            .and_then(|s| {
                if turn == 0 && !s.is_empty() {
                    Some(
                        s.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .enumerate()
                            .map(|(i, name)| {
                                make_tool_use(
                                    format!("call-{i}"),
                                    name.to_string(),
                                    tau_domain::Value::Null,
                                )
                            })
                            .collect::<Vec<ToolUse>>(),
                    )
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let stop_reason = match env::var("TAU_MOCK_LLM_STOP_REASON").as_deref() {
            Ok("max_tokens") => StopReason::MaxTokens,
            Ok("tool_use") => StopReason::ToolUse,
            _ => StopReason::EndTurn,
        };
        make_completion_response(text, tool_uses, stop_reason, None)
    }
}

impl LlmBackend for MockLlmBackend {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let mut lock = self
            .invocations
            .lock()
            .expect("MockLlmBackend mutex poisoned");
        let turn = *lock;
        *lock += 1;
        Ok(self.build_response(turn))
    }

    async fn stream(&self, _req: CompletionRequest) -> Result<CompletionStream, LlmError> {
        // Streaming is unused in the v0.1 run loop; surface a typed
        // error rather than panic so a future stream-mode caller gets
        // a recognisable failure.
        Err(LlmError::Internal {
            message: "MockLlmBackend does not implement stream()".into(),
        })
    }
}

/// Compiled-in echo tool: returns the input formatted as a text content
/// block. No capabilities required, so it survives the runtime's
/// capability-filter pass for any agent package.
pub struct EchoTool;

impl Tool for EchoTool {
    type Session = ();

    fn name(&self) -> &str {
        "echo"
    }

    fn schema(&self) -> ToolSpec {
        make_tool_spec(
            "echo".into(),
            "Echo the input as text".into(),
            tau_domain::Value::Object(Default::default()),
        )
    }

    async fn init(&self, _ctx: SessionContext) -> Result<(), ToolError> {
        Ok(())
    }

    async fn invoke(
        &self,
        _session: &mut (),
        input: tau_domain::Value,
    ) -> Result<ToolResult, ToolError> {
        let text = format!("{input:?}");
        Ok(make_tool_result(vec![ToolContent::Text { text }], false))
    }

    async fn teardown(&self, _session: ()) -> Result<(), ToolError> {
        Ok(())
    }
}
