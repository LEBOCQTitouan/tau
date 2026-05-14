//! Multi-turn `MockLlmBackend` fixture for tau-cli orchestration pattern tests.
//!
//! **Duplication notice:** This file is a copy of
//! `crates/tau-runtime/tests/common/mock_llm.rs`. The canonical fixture lives
//! in tau-runtime's test tree. tau-cli's integration tests cannot import from
//! tau-runtime's test tree (Cargo does not expose test helpers as a crate),
//! so the simplest correct path (option b per the T9 implementer notes) is
//! to maintain a copy here. The file is ~180 LOC; if it diverges significantly
//! a shared test-support crate (option c) is the right next step.
//!
//! Skills-4 T9: un-ignore 5 pattern test skeletons (`cmd_orchestration.rs`).
//!
//! # Builder API
//!
//! ```ignore
//! let backend = MockLlmBackend::new("llm")
//!     .add_tool_call("task.create", serde_json::json!({"description": "do X"}))
//!     .add_tool_call("agent.worker.spawn", serde_json::json!({"message": "do X", "grant": []}))
//!     .add_text("done");
//! ```

use std::collections::VecDeque;
use std::sync::Mutex;

use tau_domain::Value;
use tau_ports::fixtures::{make_completion_response, make_token_usage};
use tau_ports::{
    batch_to_stream, CompletionRequest, CompletionResponse, CompletionStream, LlmBackend, LlmError,
    StopReason, ToolUse,
};

/// A scripted turn for `MockLlmBackend`.
///
/// Each `MockTurn` maps to one call to `LlmBackend::complete` or
/// `LlmBackend::stream`.
#[derive(Debug, Clone)]
pub enum MockTurn {
    /// Plain text response with no tool use. Produces
    /// `StopReason::EndTurn`.
    Text {
        /// The text the model "returns".
        text: String,
    },
    /// A tool-call response. Produces `StopReason::ToolUse`.
    ToolCall {
        /// Tool name (e.g. `"task.create"`).
        name: String,
        /// Tool arguments.
        args: Value,
    },
    /// End-of-script marker: returns an empty text response with
    /// `StopReason::EndTurn`. Callers add this to make it explicit that
    /// no more turns are expected.
    End,
}

/// Multi-turn scripted LLM backend for integration tests.
///
/// Returns scripted turns in FIFO order. Each `complete()` / `stream()`
/// call pops one `MockTurn` and records the incoming `CompletionRequest`.
///
/// Returns `LlmError::Internal` if the script is exhausted.
#[derive(Debug)]
pub struct MockLlmBackend {
    name: String,
    turns: Mutex<VecDeque<MockTurn>>,
    received_requests: Mutex<Vec<CompletionRequest>>,
}

impl MockLlmBackend {
    /// Create a new backend with the given name and an empty turn queue.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            turns: Mutex::new(VecDeque::new()),
            received_requests: Mutex::new(Vec::new()),
        }
    }

    /// Push a scripted turn onto the queue. Returns `self` for chaining.
    pub fn add_turn(self, turn: MockTurn) -> Self {
        self.turns
            .lock()
            .expect("MockLlmBackend turns mutex poisoned")
            .push_back(turn);
        self
    }

    /// Convenience: add a plain-text turn.
    pub fn add_text(self, text: &str) -> Self {
        self.add_turn(MockTurn::Text {
            text: text.to_string(),
        })
    }

    /// Convenience: add a tool-call turn using a `serde_json::Value` for args.
    ///
    /// Converts to `tau_domain::Value` via serde round-trip. Panics on
    /// conversion failure — these are test fixtures, so any failure is a bug.
    pub fn add_tool_call_json(self, name: &str, args: serde_json::Value) -> Self {
        let tau_args: Value =
            serde_json::from_value(args).expect("mock tool call args must round-trip to tau Value");
        self.add_turn(MockTurn::ToolCall {
            name: name.to_string(),
            args: tau_args,
        })
    }

    /// Convenience: add an explicit end-of-script turn.
    pub fn add_end(self) -> Self {
        self.add_turn(MockTurn::End)
    }

    /// Return all `CompletionRequest`s this backend has received, in order.
    pub fn received_requests(&self) -> Vec<CompletionRequest> {
        self.received_requests
            .lock()
            .expect("MockLlmBackend received_requests mutex poisoned")
            .clone()
    }

    /// Record the incoming request + pop the next scripted turn.
    fn pop_next(&self, req: &CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.received_requests
            .lock()
            .expect("MockLlmBackend received_requests mutex poisoned")
            .push(req.clone());

        let turn = self
            .turns
            .lock()
            .expect("MockLlmBackend turns mutex poisoned")
            .pop_front()
            .ok_or_else(|| LlmError::Internal {
                message: format!("MockLlmBackend({:?}): script exhausted", self.name),
            })?;

        Ok(match turn {
            MockTurn::Text { text } => make_completion_response(
                text,
                vec![],
                StopReason::EndTurn,
                Some(make_token_usage(10, 10)),
            ),
            MockTurn::ToolCall { name, args } => {
                let id = format!("tu_{name}");
                make_completion_response(
                    String::new(),
                    vec![ToolUse::new(id, name, args)],
                    StopReason::ToolUse,
                    Some(make_token_usage(10, 10)),
                )
            }
            MockTurn::End => make_completion_response(
                String::new(),
                vec![],
                StopReason::EndTurn,
                Some(make_token_usage(1, 0)),
            ),
        })
    }
}

impl LlmBackend for MockLlmBackend {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.pop_next(&req)
    }

    async fn stream(&self, req: CompletionRequest) -> Result<CompletionStream, LlmError> {
        let resp = self.pop_next(&req)?;
        Ok(batch_to_stream(resp))
    }
}
