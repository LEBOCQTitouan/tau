//! Streaming agent runs. Realizes ADR-0006 §5 deferral closure
//! (Tier 2 priority 8).
//!
//! `Runtime::run_streaming` (added in Task 6) yields a
//! `Stream<Item = RunEvent>` as the agent loop progresses — text
//! deltas as the LLM types, tool calls as the LLM commits to them,
//! tool results as dispatch finishes. The terminal `RunCompleted`
//! event carries the final `RunOutcome` (success or failure).
//!
//! See `docs/superpowers/specs/2026-04-30-streaming-design.md` and
//! ADR-0011 (added in Task 12).

use std::sync::Arc;

use futures_core::Stream;
use tau_domain::{
    Address, AgentDefinition, AgentInstanceId, Message, MessagePayload, PackageManifest, Value,
};
use tau_ports::{CompletionChunk, CompletionRequest, LlmError, StopReason, TokenUsage, ToolResult};
use tracing::{debug, info, warn};

use crate::builder::DynLlmBackend;
use crate::options::RunOptions;
use crate::outcome::RunOutcome;

/// Streaming event from `Runtime::run_streaming`.
///
/// Always terminates with exactly one `RunCompleted`; intermediate
/// events are unbounded per agent run. See spec §4.2 for the full
/// pump invariants.
///
/// Per ADR-0011:
/// - Every `ToolCallStarted` is followed by either a matching
///   `ToolCallCompleted` (same `id`) before the next `TurnCompleted`,
///   OR a terminal `RunCompleted { outcome: Failed }` if dispatch
///   crashed mid-flight.
/// - `TurnCompleted` arrives only after the turn's LLM `Finish` AND
///   all that turn's tool dispatches resolved.
/// - Stream order preserves LLM source order; the kernel never
///   reorders events.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum RunEvent {
    /// LLM emitted a text fragment. Concatenate with previous deltas
    /// for the running assistant message text.
    TextDelta {
        /// Text fragment to append.
        delta: String,
    },

    /// LLM emitted a complete `tool_use` block. Fires immediately
    /// when the kernel sees `CompletionChunk::ToolUse` — BEFORE the
    /// tool is dispatched. Display intent: "agent wants to call X
    /// with args Y". The matching `ToolCallCompleted` fires after
    /// dispatch finishes.
    ToolCallStarted {
        /// Provider-supplied tool-use id; correlates with
        /// `ToolCallCompleted.id`.
        id: String,
        /// Tool name.
        name: String,
        /// Args the LLM emitted.
        args: Value,
    },

    /// Tool dispatch finished. Fires after `Tool::invoke` returns,
    /// regardless of success/failure. Carries the tool result OR a
    /// validation/dispatch error message.
    ToolCallCompleted {
        /// Matches the `id` from `ToolCallStarted`.
        id: String,
        /// Tool name.
        name: String,
        /// `Ok(ToolResult)` on success; `Err(reason)` for validation
        /// failures or other recoverable errors. Plugin-crash-class
        /// errors don't surface here — they terminate the run via
        /// `RunCompleted`.
        result: Result<ToolResult, String>,
    },

    /// One turn of the agent loop completed. The LLM's `Finish`
    /// chunk arrived AND any tool calls within the turn finished
    /// dispatching.
    TurnCompleted {
        /// Why the turn ended (per LLM-reported `StopReason`).
        stop_reason: StopReason,
        /// Token usage for this turn. `None` if the provider did
        /// not report.
        usage: Option<TokenUsage>,
        /// Turn number (1-indexed) within the run.
        turn: u32,
    },

    /// Terminal event. Always exactly one per stream. After this
    /// fires, the stream returns `None`.
    RunCompleted {
        /// Final outcome — same shape as `Runtime::run` returns.
        outcome: RunOutcome,
    },
}

/// Build the stream of `RunEvent`s for a single agent run. Happy
/// path: drains the LLM stream, yields `TextDelta` per chunk, then
/// `TurnCompleted` + `RunCompleted` once `Finish` arrives. No tool
/// dispatch in this commit (Task 4 adds it).
///
/// Constructed inputs are pre-validated by the caller in Task 6
/// (`Runtime::run_streaming`); here we trust them.
#[allow(dead_code)] // wired up by Task 6
pub(crate) fn run_streaming_inner(
    backend: Arc<dyn DynLlmBackend>,
    agent_def: AgentDefinition,
    _package_manifest: PackageManifest,
    history: Vec<Message>,
    initial_message: Message,
    options: RunOptions,
) -> impl Stream<Item = RunEvent> + 'static {
    async_stream::stream! {
        let agent_instance_id = AgentInstanceId::new();
        let mut messages: Vec<Message> = Vec::with_capacity(history.len() + 1);
        messages.extend(history);
        messages.push(initial_message);
        let mut total_turns: u32 = 0;
        let mut aggregated_tokens = crate::options::TokenUsage::default();

        info!(name = "runtime.streaming_run_started");

        // max_turns guard: text-only path runs exactly one turn (Task 4 adds
        // the tool-dispatch loop that iterates until Finish without ToolUse).
        if options.max_turns == 0 {
            yield make_max_turns_outcome(messages, total_turns, aggregated_tokens, options.max_turns);
            return;
        }

        total_turns += 1;
        debug!(name = "runtime.streaming_turn_started", turn = total_turns);

        let mut request = CompletionRequest::new(agent_def.llm_backend.as_str().into());
        request.system = agent_def.system_prompt.clone();
        request.messages = crate::run::agent_messages_to_provider_messages(&messages);
        request.tools = Vec::new();

        let mut llm_stream = match backend.stream(request).await {
            Ok(s) => s,
            Err(llm_err) => {
                warn!(name = "runtime.streaming_llm_open_failed");
                yield make_llm_error_outcome(llm_err, messages, total_turns, aggregated_tokens);
                return;
            }
        };

        let mut accumulated_text = String::new();
        let mut turn_stop_reason: Option<StopReason> = None;
        let mut turn_usage: Option<TokenUsage> = None;

        // CompletionStream is Pin<Box<dyn Stream + Send>>; .as_mut() gives Pin<&mut S>.
        loop {
            let next = std::future::poll_fn(|cx| llm_stream.as_mut().poll_next(cx)).await;
            match next {
                None => break,
                Some(Ok(CompletionChunk::Text { delta })) => {
                    accumulated_text.push_str(&delta);
                    yield RunEvent::TextDelta { delta };
                }
                Some(Ok(CompletionChunk::ToolUse(_))) => {
                    // Task 4 handles this. Happy-path text-only tests don't trigger it.
                    warn!(name = "runtime.streaming_tool_use_unhandled_in_task_3");
                }
                Some(Ok(CompletionChunk::Finish { stop_reason, usage })) => {
                    turn_stop_reason = Some(stop_reason);
                    turn_usage = usage;
                    break;
                }
                Some(Err(llm_err)) => {
                    warn!(name = "runtime.streaming_llm_chunk_err");
                    yield make_llm_error_outcome(
                        llm_err,
                        messages,
                        total_turns,
                        aggregated_tokens,
                    );
                    return;
                }
                // CompletionChunk is #[non_exhaustive]; ignore unknown variants.
                Some(Ok(_)) => {}
            }
        }

        if !accumulated_text.is_empty() {
            let agent_addr = Address::Agent(agent_instance_id);
            messages.push(Message::new(
                agent_addr,
                Address::User,
                MessagePayload::Text {
                    content: accumulated_text.clone(),
                },
            ));
        }

        if let Some(usage) = turn_usage {
            aggregated_tokens.input_tokens = aggregated_tokens
                .input_tokens
                .saturating_add(u64::from(usage.input_tokens));
            aggregated_tokens.output_tokens = aggregated_tokens
                .output_tokens
                .saturating_add(u64::from(usage.output_tokens));
        }

        yield RunEvent::TurnCompleted {
            stop_reason: turn_stop_reason.unwrap_or(StopReason::EndTurn),
            usage: turn_usage,
            turn: total_turns,
        };

        // No tool dispatch yet (Task 4). End the run after the first Finish.
        let final_message = messages
            .last()
            .cloned()
            .expect("messages contains at least the initial user message");
        yield RunEvent::RunCompleted {
            outcome: RunOutcome::Completed {
                final_message,
                all_messages: messages,
                total_turns,
                token_usage: aggregated_tokens,
            },
        };
    }
}

#[allow(dead_code)] // wired up by Task 4
fn make_llm_error_outcome(
    llm_err: LlmError,
    messages: Vec<Message>,
    total_turns: u32,
    token_usage: crate::options::TokenUsage,
) -> RunEvent {
    use tau_domain::{AgentStatus, FailureKind};
    let detail = format!("{llm_err}");
    RunEvent::RunCompleted {
        outcome: RunOutcome::Failed {
            status: AgentStatus::failed(FailureKind::BackendError, Some(detail)),
            all_messages: messages,
            total_turns,
            token_usage,
        },
    }
}

#[allow(dead_code)] // wired up by Task 5
fn make_max_turns_outcome(
    messages: Vec<Message>,
    total_turns: u32,
    token_usage: crate::options::TokenUsage,
    max_turns: u32,
) -> RunEvent {
    use tau_domain::{AgentStatus, FailureKind};
    RunEvent::RunCompleted {
        outcome: RunOutcome::Failed {
            status: AgentStatus::failed(
                FailureKind::OutOfResources,
                Some(format!("max_turns ({max_turns}) reached")),
            ),
            all_messages: messages,
            total_turns,
            token_usage,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tau_ports::fixtures::{make_token_usage, make_tool_result};

    #[test]
    fn run_event_text_delta_clone_preserves_delta() {
        let e = RunEvent::TextDelta {
            delta: "Hello".into(),
        };
        let cloned = e.clone();
        let RunEvent::TextDelta { delta } = cloned else {
            panic!("expected TextDelta")
        };
        assert_eq!(delta, "Hello");
    }

    #[test]
    fn run_event_tool_call_started_clone_preserves_fields() {
        let e = RunEvent::ToolCallStarted {
            id: "call_1".into(),
            name: "fs-read".into(),
            args: Value::Null,
        };
        let cloned = e.clone();
        let RunEvent::ToolCallStarted { id, name, .. } = cloned else {
            panic!("expected ToolCallStarted")
        };
        assert_eq!(id, "call_1");
        assert_eq!(name, "fs-read");
    }

    #[test]
    fn run_event_tool_call_completed_carries_result() {
        let e = RunEvent::ToolCallCompleted {
            id: "call_1".into(),
            name: "fs-read".into(),
            result: Ok(make_tool_result(vec![], false)),
        };
        let RunEvent::ToolCallCompleted { result, .. } = e else {
            panic!("expected ToolCallCompleted")
        };
        assert!(result.is_ok());
    }

    #[test]
    fn run_event_tool_call_completed_carries_error_reason() {
        let e = RunEvent::ToolCallCompleted {
            id: "call_1".into(),
            name: "fs-read".into(),
            result: Err("validation failed".into()),
        };
        let RunEvent::ToolCallCompleted { result, .. } = e else {
            panic!("expected ToolCallCompleted")
        };
        let Err(reason) = result else {
            panic!("expected Err")
        };
        assert_eq!(reason, "validation failed");
    }

    #[test]
    fn run_event_turn_completed_carries_stop_reason_and_usage() {
        let e = RunEvent::TurnCompleted {
            stop_reason: StopReason::ToolUse,
            usage: Some(make_token_usage(10, 5)),
            turn: 3,
        };
        let RunEvent::TurnCompleted {
            stop_reason,
            usage,
            turn,
        } = e
        else {
            panic!("expected TurnCompleted")
        };
        assert_eq!(stop_reason, StopReason::ToolUse);
        assert_eq!(turn, 3);
        assert!(usage.is_some());
    }

    // ---- Task 3 tests: run_streaming_inner ----

    use std::sync::Arc;
    use tau_domain::{
        Address, AgentDefinition, AgentId, Message, MessagePayload, PackageId, PackageName, Version,
    };
    use tau_ports::{
        CompletionChunk, CompletionRequest, CompletionResponse, CompletionStream, LlmBackend,
        LlmError, StopReason as PortsStopReason, TokenUsage as PortsTokenUsage,
    };

    use crate::builder::DynLlmBackend;
    use crate::options::RunOptions;

    /// Scripted LLM that emits a fixed sequence of CompletionChunk via stream().
    struct ScriptedLlm {
        chunks: std::sync::Mutex<Option<Vec<Result<CompletionChunk, LlmError>>>>,
    }

    impl ScriptedLlm {
        fn new(chunks: Vec<Result<CompletionChunk, LlmError>>) -> Self {
            Self {
                chunks: std::sync::Mutex::new(Some(chunks)),
            }
        }
    }

    impl LlmBackend for ScriptedLlm {
        fn name(&self) -> &str {
            "scripted-llm"
        }

        async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
            unimplemented!("ScriptedLlm streams only")
        }

        async fn stream(&self, _req: CompletionRequest) -> Result<CompletionStream, LlmError> {
            let chunks = self
                .chunks
                .lock()
                .expect("lock poisoned")
                .take()
                .ok_or_else(|| LlmError::Internal {
                    message: "ScriptedLlm: stream() called twice".into(),
                })?;
            Ok(Box::pin(async_stream::stream! {
                for c in chunks {
                    yield c;
                }
            }))
        }
    }

    fn agent_def() -> AgentDefinition {
        use std::str::FromStr;
        let pkg = PackageId::new(
            PackageName::from_str("test-pkg").unwrap(),
            Version::parse("0.1.0").unwrap(),
        );
        AgentDefinition::new(
            AgentId::from_str("test-agent").unwrap(),
            "test".to_string(),
            pkg,
            PackageName::from_str("scripted-llm").unwrap(),
        )
    }

    fn manifest_with_no_capabilities() -> tau_domain::PackageManifest {
        use tau_domain::UncheckedManifest;
        let toml_str = r#"
            name = "test-pkg"
            version = "0.1.0"
            description = "test package"
            authors = []
            source = "https://example.com/test.git"
            kind = "tool"
            dependencies = []
            capabilities = []
        "#;
        let unchecked: UncheckedManifest = toml::from_str(toml_str).unwrap();
        unchecked.validate().unwrap()
    }

    fn user_msg(text: &str) -> Message {
        Message::new(
            Address::User,
            Address::User,
            MessagePayload::Text {
                content: text.to_string(),
            },
        )
    }

    async fn collect_events(
        mut stream: impl futures_core::Stream<Item = RunEvent> + Unpin,
    ) -> Vec<RunEvent> {
        use std::pin::Pin;
        let mut out = Vec::new();
        loop {
            let next = std::future::poll_fn(|cx| Pin::new(&mut stream).poll_next(cx)).await;
            match next {
                None => break,
                Some(e) => out.push(e),
            }
        }
        out
    }

    #[tokio::test]
    async fn happy_path_text_only_yields_text_delta_then_turn_completed_then_run_completed() {
        let llm: Arc<dyn DynLlmBackend> = Arc::new(ScriptedLlm::new(vec![
            Ok(CompletionChunk::Text {
                delta: "Hello ".into(),
            }),
            Ok(CompletionChunk::Text {
                delta: "world".into(),
            }),
            Ok(CompletionChunk::Finish {
                stop_reason: PortsStopReason::EndTurn,
                usage: Some(PortsTokenUsage::new(10, 5)),
            }),
        ]));

        let stream = run_streaming_inner(
            llm,
            agent_def(),
            manifest_with_no_capabilities(),
            vec![],
            user_msg("hi"),
            RunOptions::default(),
        );
        let events = collect_events(Box::pin(stream)).await;

        assert_eq!(events.len(), 4, "got events: {events:#?}");
        let RunEvent::TextDelta { delta } = &events[0] else {
            panic!("expected TextDelta, got {:?}", events[0])
        };
        assert_eq!(delta, "Hello ");
        let RunEvent::TextDelta { delta } = &events[1] else {
            panic!("expected TextDelta, got {:?}", events[1])
        };
        assert_eq!(delta, "world");
        assert!(matches!(events[2], RunEvent::TurnCompleted { .. }));
        assert!(matches!(events[3], RunEvent::RunCompleted { .. }));
    }

    #[tokio::test]
    async fn llm_error_mid_stream_yields_run_completed_failed() {
        let llm: Arc<dyn DynLlmBackend> = Arc::new(ScriptedLlm::new(vec![
            Ok(CompletionChunk::Text {
                delta: "Hello".into(),
            }),
            Err(LlmError::Internal {
                message: "provider blew up".into(),
            }),
        ]));

        let stream = run_streaming_inner(
            llm,
            agent_def(),
            manifest_with_no_capabilities(),
            vec![],
            user_msg("hi"),
            RunOptions::default(),
        );
        let events = collect_events(Box::pin(stream)).await;

        assert_eq!(events.len(), 2, "got events: {events:#?}");
        let RunEvent::RunCompleted { outcome } = &events[1] else {
            panic!("expected RunCompleted, got {:?}", events[1])
        };
        assert!(matches!(outcome, RunOutcome::Failed { .. }));
    }

    #[tokio::test]
    async fn llm_open_failure_yields_run_completed_failed_with_no_intermediate_events() {
        struct FailingLlm;
        impl LlmBackend for FailingLlm {
            fn name(&self) -> &str {
                "failing-llm"
            }
            async fn complete(
                &self,
                _r: CompletionRequest,
            ) -> Result<CompletionResponse, LlmError> {
                unimplemented!()
            }
            async fn stream(&self, _r: CompletionRequest) -> Result<CompletionStream, LlmError> {
                Err(LlmError::Internal {
                    message: "open failed".into(),
                })
            }
        }

        let llm: Arc<dyn DynLlmBackend> = Arc::new(FailingLlm);

        let stream = run_streaming_inner(
            llm,
            agent_def(),
            manifest_with_no_capabilities(),
            vec![],
            user_msg("hi"),
            RunOptions::default(),
        );
        let events = collect_events(Box::pin(stream)).await;

        assert_eq!(events.len(), 1, "got events: {events:#?}");
        assert!(matches!(events[0], RunEvent::RunCompleted { .. }));
    }
}
