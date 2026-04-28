//! Agent multi-turn run loop. The kernel surface — see spec §3.7.
//!
//! [`Runtime::run`] receives an initial [`Message`], drives the LLM
//! backend and tool plugins through a turn loop bounded by
//! [`RunOptions::max_turns`], applies capability checks before each
//! tool call, and returns a [`RunOutcome`].
//!
//! # Error vs failure dichotomy (ADR-0006)
//!
//! - Plugin/dispatch failures (LLM error, tool error, missing backend)
//!   bubble up as `Err(RuntimeError)`. The agent terminates abnormally.
//! - Agent-level failures — capability denied, max turns reached —
//!   are reported as `Ok(RunOutcome::Failed { status, .. })` with
//!   `status = AgentStatus::Failed { kind, .. }`. The conversation
//!   history is preserved for inspection.
//!
//! # Tracing
//!
//! Per spec §3.9 the run loop emits a fixed vocabulary of events
//! (`runtime.run_started`, `runtime.turn_started`, `llm.request_built`,
//! …) under named spans (`runtime.agent_run`, `runtime.turn`,
//! `llm.complete`, `dispatch.tool`, `capability.check`,
//! `tool.session_open`, `tool.invoke`, `tool.session_close`).
//! Sensitive-data discipline: arguments and message content never
//! travel above DEBUG; full content is TRACE-only and otherwise
//! truncated to a 256-char preview.

use std::collections::BTreeMap;

use tau_domain::{
    AgentDefinition, AgentInstanceId, AgentStatus, Capability, FailureKind, Message,
    MessagePayload, PackageManifest, Value,
};
use tau_ports::{
    CompletionRequest, ContentBlock, LlmProviderMessage, SessionContext, ToolContent, ToolResult,
    ToolSpec, ToolUse,
};
use tracing::{debug, debug_span, info, info_span, instrument, trace, warn, Instrument};

use crate::builder::Runtime;
use crate::capability::check_capabilities;
use crate::error::{CapabilityDenial, RuntimeError};
use crate::options::{RunOptions, TokenUsage};
use crate::outcome::RunOutcome;

/// Maximum length of a logged preview for free-form arguments and text
/// at DEBUG level. See module-level "sensitive-data discipline" docs.
const PREVIEW_CHARS: usize = 256;

impl Runtime {
    /// Run an agent through one solo-path multi-turn iteration.
    ///
    /// The loop is:
    ///
    /// 1. Build a [`CompletionRequest`] from the conversation so far.
    /// 2. Call the LLM backend.
    /// 3. Append the assistant text to the history.
    /// 4. If no tool_uses were emitted, return
    ///    [`RunOutcome::Completed`].
    /// 5. Otherwise, for each tool_use: capability-check, dispatch
    ///    through the registered tool plugin, append the result to the
    ///    history, and resume from step 1.
    /// 6. After [`RunOptions::max_turns`] iterations without
    ///    completion, return [`RunOutcome::Failed`] with
    ///    [`FailureKind::OutOfResources`].
    ///
    /// `Ok(RunOutcome::Failed)` is returned in two cases: capability
    /// denial ([`FailureKind::PolicyDenied`]) and max turns reached
    /// ([`FailureKind::OutOfResources`]). Plugin and dispatch errors
    /// bubble up as `Err(RuntimeError)` per ADR-0006.
    #[instrument(
        name = "runtime.agent_run",
        skip_all,
        fields(
            agent_id = %agent_def.id,
            display_name = %agent_def.display_name,
            package_id = %agent_def.package.name,
            llm_backend_name = %agent_def.llm_backend,
            max_turns = options.max_turns,
        ),
    )]
    pub async fn run(
        &self,
        agent_def: AgentDefinition,
        package_manifest: PackageManifest,
        initial_message: Message,
        options: RunOptions,
    ) -> Result<RunOutcome, RuntimeError> {
        info!(name = "runtime.run_started");

        let granted: &[Capability] = package_manifest.capabilities();
        debug!(
            name = "runtime.capability_set_loaded",
            count = granted.len()
        );

        // The instance-id is generated once per `run()` call and used
        // for every assistant-authored `Message` and every
        // `SessionContext`. `AgentId` (compile-time agent identity) is
        // not interchangeable with `AgentInstanceId` (per-run UUID v7).
        let agent_instance_id = AgentInstanceId::new();

        let backend = self
            .resolve_llm_backend(agent_def.id.as_str(), agent_def.llm_backend.as_str())?
            .clone();

        let mut messages: Vec<Message> = build_initial_messages(initial_message);
        let mut total_turns: u32 = 0;
        let mut aggregated_tokens = TokenUsage::default();

        let tool_specs: Vec<ToolSpec> = self.tools().values().map(|t| t.schema()).collect();

        while total_turns < options.max_turns {
            total_turns += 1;
            let _turn_span = debug_span!("runtime.turn", turn = total_turns).entered();
            debug!(name = "runtime.turn_started", turn = total_turns);

            // ----- LLM call ---------------------------------------------------
            let mut request = CompletionRequest::new(agent_def.llm_backend.as_str().into());
            request.system = agent_def.system_prompt.clone();
            request.messages = agent_messages_to_provider_messages(&messages);
            request.tools = tool_specs.clone();
            debug!(
                name = "llm.request_built",
                messages = request.messages.len(),
                tools = request.tools.len(),
            );

            let response = {
                let llm_span = info_span!("llm.complete");
                backend.complete(request).instrument(llm_span).await?
            };

            debug!(
                name = "llm.response_received",
                text_len = response.text.len(),
                tool_uses = response.tool_uses.len(),
                stop_reason = ?response.stop_reason,
            );
            trace!(
                name = "llm.stop_reason",
                reason = ?response.stop_reason,
            );

            if let Some(usage) = response.usage {
                let input = u64::from(usage.input_tokens);
                let output = u64::from(usage.output_tokens);
                aggregated_tokens.input_tokens =
                    aggregated_tokens.input_tokens.saturating_add(input);
                aggregated_tokens.output_tokens =
                    aggregated_tokens.output_tokens.saturating_add(output);
                debug!(
                    name = "llm.token_usage",
                    input_tokens = input,
                    output_tokens = output,
                );
            }

            // ----- Append assistant turn -------------------------------------
            append_assistant_response(
                &mut messages,
                &response.text,
                &response.tool_uses,
                &agent_instance_id,
            );

            // ----- No tool_uses → terminate ----------------------------------
            if response.tool_uses.is_empty() {
                debug!(name = "runtime.loop_terminated", reason = "end_turn");
                let final_message = messages
                    .last()
                    .cloned()
                    .expect("messages contains at least the initial user message");
                info!(
                    name = "runtime.run_completed",
                    total_turns,
                    all_messages = messages.len(),
                );
                return Ok(RunOutcome::Completed {
                    final_message,
                    all_messages: messages,
                    total_turns,
                    token_usage: aggregated_tokens,
                });
            }

            // ----- Per-tool dispatch -----------------------------------------
            for tool_use in &response.tool_uses {
                debug!(
                    name = "llm.tool_use_emitted",
                    id = %tool_use.id,
                    tool_name = %tool_use.name,
                );

                let tool = {
                    let _dispatch_span =
                        debug_span!("dispatch.tool", tool_name = %tool_use.name).entered();
                    let tool = self.resolve_tool(&tool_use.name)?.clone();
                    debug!(name = "dispatch.tool_resolved", tool_name = %tool_use.name);
                    tool
                };

                // ----- Capability check --------------------------------------
                let cap_decision: Option<CapabilityDenial> = {
                    let _cap_span =
                        debug_span!("capability.check", tool_name = %tool_use.name).entered();
                    let required: &[Capability] = tool.capabilities();
                    trace!(name = "capability.required_loaded", count = required.len());
                    trace!(name = "capability.granted_loaded", count = granted.len());
                    let missing = check_capabilities(granted, required);
                    trace!(
                        name = "capability.satisfies_check",
                        satisfied = missing.is_none()
                    );
                    match missing {
                        None => {
                            trace!(name = "capability.allow", tool_name = %tool_use.name);
                            None
                        }
                        Some(cap) => {
                            let kind = capability_kind_str(cap);
                            warn!(
                                name = "capability.deny",
                                tool_name = %tool_use.name,
                                missing_kind = %kind,
                            );
                            Some(CapabilityDenial {
                                agent_id: agent_def.id.to_string(),
                                package_id: agent_def.package.name.to_string(),
                                tool_name: tool_use.name.clone(),
                                required_kind: kind,
                                required_detail: format!("{cap:?}"),
                            })
                        }
                    }
                };

                if let Some(denial) = cap_decision {
                    let outcome = build_policy_denied_outcome(
                        denial,
                        messages,
                        total_turns,
                        aggregated_tokens,
                    );
                    info!(name = "runtime.run_failed", kind = "policy_denied");
                    return Ok(outcome);
                }

                // ----- Append the tool-call message --------------------------
                let agent_addr = tau_domain::Address::Agent(agent_instance_id);
                let tool_addr = tau_domain::Address::Tool(tool_use.name.clone());
                messages.push(Message::new(
                    agent_addr.clone(),
                    tool_addr.clone(),
                    MessagePayload::ToolCall {
                        args: tool_use.input.clone(),
                    },
                ));
                trace!(name = "message.added", kind = "tool_call");

                // ----- Open a session ----------------------------------------
                {
                    let session_open_span =
                        info_span!("tool.session_open", tool_name = %tool_use.name);
                    let ctx = SessionContext::new(agent_instance_id, uuid::Uuid::new_v4(), None);
                    if let Err(err) = tool.init(ctx).instrument(session_open_span).await {
                        warn!(
                            name = "tool.session_open_failed",
                            tool_name = %tool_use.name,
                        );
                        return Err(RuntimeError::from(err));
                    }
                }

                // ----- Invoke -----------------------------------------------
                let tool_result: ToolResult = {
                    let invoke_span = info_span!("tool.invoke", tool_name = %tool_use.name);
                    let preview = preview_value(&tool_use.input, PREVIEW_CHARS);
                    debug!(
                        name = "tool.args_received",
                        tool_name = %tool_use.name,
                        args_preview = %preview,
                    );
                    // v0.1 passthrough; the helper is a hook for the
                    // Phase-1 schema-validation pass.
                    let _validated = deserialize_tool_args(
                        &tool_use.input,
                        &tool_use.name,
                        agent_def.llm_backend.as_str(),
                    )?;
                    let outcome = tool
                        .invoke(&mut (), tool_use.input.clone())
                        .instrument(invoke_span)
                        .await;
                    match outcome {
                        Ok(r) => {
                            debug!(
                                name = "tool.result_received",
                                tool_name = %tool_use.name,
                                is_error = r.is_error,
                                content_blocks = r.content.len(),
                            );
                            r
                        }
                        Err(err) => {
                            warn!(
                                name = "tool.invoke_failed",
                                tool_name = %tool_use.name,
                            );
                            // Best-effort teardown so the plugin gets a
                            // chance to clean up before we abort. Errors
                            // here are swallowed: the original `err`
                            // is the more useful diagnostic.
                            let _ = tool.teardown(()).await;
                            return Err(RuntimeError::from(err));
                        }
                    }
                };

                // ----- Close the session ------------------------------------
                {
                    let session_close_span =
                        info_span!("tool.session_close", tool_name = %tool_use.name);
                    if let Err(err) = tool.teardown(()).instrument(session_close_span).await {
                        warn!(
                            name = "tool.session_close_failed",
                            tool_name = %tool_use.name,
                        );
                        return Err(RuntimeError::from(err));
                    }
                }

                // ----- Append the tool-result message -----------------------
                let result_payload = if tool_result.is_error {
                    MessagePayload::ToolError {
                        kind: "tool_runtime_error".into(),
                        message: flatten_content_to_string(&tool_result.content),
                        details: None,
                    }
                } else {
                    MessagePayload::ToolResult {
                        body: content_to_value(&tool_result.content),
                    }
                };
                messages.push(Message::new(tool_addr, agent_addr, result_payload));
                trace!(
                    name = "message.added",
                    kind = if tool_result.is_error {
                        "tool_error"
                    } else {
                        "tool_result"
                    },
                );
            }

            debug!(name = "runtime.turn_completed", turn = total_turns);
        }

        // ----- max_turns reached -------------------------------------------
        warn!(
            name = "runtime.max_turns_reached",
            max_turns = options.max_turns
        );
        let outcome = RunOutcome::Failed {
            status: AgentStatus::failed(
                FailureKind::OutOfResources,
                Some(format!("max_turns ({}) reached", options.max_turns)),
            ),
            all_messages: messages,
            total_turns,
            token_usage: aggregated_tokens,
        };
        info!(name = "runtime.run_failed", kind = "out_of_resources");
        Ok(outcome)
    }

    /// Convenience: [`Runtime::run`] with [`RunOptions::default`].
    pub async fn run_default(
        &self,
        agent_def: AgentDefinition,
        package_manifest: PackageManifest,
        initial_message: Message,
    ) -> Result<RunOutcome, RuntimeError> {
        self.run(
            agent_def,
            package_manifest,
            initial_message,
            RunOptions::default(),
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Internal helpers (named per the plan; each has a unit test below)
// ---------------------------------------------------------------------------

/// Wrap the single initial message in the conversation history. v0.1
/// is trivial (`vec![initial]`); the helper exists so the run loop
/// reads cleanly and so future bootstrap steps (system-prompt
/// projection, scratchpad seeding) have a stable hook point.
pub(crate) fn build_initial_messages(initial: Message) -> Vec<Message> {
    vec![initial]
}

/// Validate tool-call arguments emitted by the LLM against the tool's
/// expected shape.
///
/// **v0.1: passthrough.** Returns the input as-is. Schema-driven
/// validation (per the tool's [`ToolSpec::input_schema`]) is deferred
/// to Phase 1; the helper exists so the run loop has a single, named
/// failure boundary that can later raise
/// [`RuntimeError::PluginContractViolation`] without further
/// restructuring. Unit-tested for shape; spec-level violations are
/// surfaced by integration tests in Tasks 11-16.
pub(crate) fn deserialize_tool_args<'a>(
    value: &'a Value,
    _tool_name: &str,
    _llm_backend_name: &str,
) -> Result<&'a Value, RuntimeError> {
    Ok(value)
}

/// Build the `RunOutcome::Failed { kind: PolicyDenied, .. }` returned
/// when [`check_capabilities`] rejects a tool invocation. Centralizes
/// the construction so the run loop's denial branch reads cleanly.
pub(crate) fn build_policy_denied_outcome(
    denial: CapabilityDenial,
    all_messages: Vec<Message>,
    total_turns: u32,
    token_usage: TokenUsage,
) -> RunOutcome {
    RunOutcome::Failed {
        status: AgentStatus::failed(FailureKind::PolicyDenied, Some(format!("{denial}"))),
        all_messages,
        total_turns,
        token_usage,
    }
}

// ---------------------------------------------------------------------------
// Translation helpers (also internal)
// ---------------------------------------------------------------------------

/// Project the agent's [`Message`] history onto the LLM-call shape.
///
/// Per `tau_ports::llm` module-level docs, `tau_domain::Message`
/// (universal envelope) and [`LlmProviderMessage`] (provider call
/// shape) are intentionally distinct. This function is the single
/// projection point in the kernel.
///
/// v0.1 mapping:
///
/// | Sender / payload                 | Provider role | Block(s)                          |
/// | -------------------------------- | ------------- | --------------------------------- |
/// | `User` / `Text`                  | `User`        | `Text`                            |
/// | `Agent` / `Text`                 | `Assistant`   | `Text`                            |
/// | `Agent` / `ToolCall`             | `Assistant`   | `ToolUse` (id derived from name)  |
/// | `Tool` / `ToolResult` or Error   | `ToolResult`  | `Text` (flattened body)           |
/// | other                            | (skipped)     | —                                 |
///
/// Lifecycle/Custom payloads are skipped — they are not part of the
/// agent↔LLM dialogue at v0.1.
pub(crate) fn agent_messages_to_provider_messages(history: &[Message]) -> Vec<LlmProviderMessage> {
    let mut out = Vec::with_capacity(history.len());
    for m in history {
        match (&m.sender, &m.payload) {
            (tau_domain::Address::User, MessagePayload::Text { content }) => {
                out.push(LlmProviderMessage::user(vec![ContentBlock::Text(
                    content.clone(),
                )]));
            }
            (tau_domain::Address::Agent(_), MessagePayload::Text { content }) => {
                out.push(LlmProviderMessage::assistant(vec![ContentBlock::Text(
                    content.clone(),
                )]));
            }
            (tau_domain::Address::Agent(_), MessagePayload::ToolCall { args }) => {
                // `tool_use.id` round-trips into the provider's
                // `tool_use_id`; v0.1 derives it from the message id so
                // a follow-up `ToolResult` can be paired by the backend.
                let tool_name = match &m.recipient {
                    tau_domain::Address::Tool(name) => name.clone(),
                    _ => String::new(),
                };
                out.push(LlmProviderMessage::assistant(vec![ContentBlock::ToolUse(
                    ToolUse::new(format!("toolu_{}", m.id), tool_name, args.clone()),
                )]));
            }
            (tau_domain::Address::Tool(_), MessagePayload::ToolResult { body }) => {
                out.push(LlmProviderMessage::tool_result(
                    format!("toolu_{}", m.id),
                    vec![ContentBlock::Text(value_to_preview_string(body))],
                    false,
                ));
            }
            (
                tau_domain::Address::Tool(_),
                MessagePayload::ToolError {
                    kind: _,
                    message,
                    details: _,
                },
            ) => {
                out.push(LlmProviderMessage::tool_result(
                    format!("toolu_{}", m.id),
                    vec![ContentBlock::Text(message.clone())],
                    true,
                ));
            }
            // Lifecycle / Custom / cross-talk patterns: not part of the
            // v0.1 agent↔LLM dialogue. Skip silently — the integration
            // tests assert the projected shape, and additive semantics
            // are non-breaking.
            _ => {}
        }
    }
    out
}

/// Append the assistant's response (text + tool_uses) to the history.
///
/// Mints one `Message` per logical assistant action:
///
/// - One [`MessagePayload::Text`] message (only if `text` is non-empty).
/// - The tool_uses themselves are NOT pushed here — they become
///   `MessagePayload::ToolCall` messages later in the dispatch loop,
///   one per `tool_use`. This keeps the history's cause-effect order
///   consistent: text before any tool call, then the tool call paired
///   immediately with its result.
pub(crate) fn append_assistant_response(
    history: &mut Vec<Message>,
    text: &str,
    tool_uses: &[ToolUse],
    agent_id: &AgentInstanceId,
) {
    if !text.is_empty() {
        history.push(Message::new(
            tau_domain::Address::Agent(*agent_id),
            tau_domain::Address::User,
            MessagePayload::Text {
                content: text.to_owned(),
            },
        ));
    } else if tool_uses.is_empty() {
        // Defensive: the LLM returned neither text nor tool_uses.
        // Spec says this shouldn't happen, but we still need a
        // recognisable assistant turn in the history so callers'
        // `final_message` is the assistant's empty response (not the
        // initial user prompt). Push an empty-text message rather than
        // synthesising a richer placeholder.
        history.push(Message::new(
            tau_domain::Address::Agent(*agent_id),
            tau_domain::Address::User,
            MessagePayload::Text {
                content: String::new(),
            },
        ));
    }
}

// ---------------------------------------------------------------------------
// Small format/utility helpers (file-private)
// ---------------------------------------------------------------------------

/// Truncate `s` to at most `n` characters (Unicode scalar values),
/// preserving UTF-8 boundaries. Returns an owned `String`. Used for
/// DEBUG-level previews.
fn truncate_to_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Format a [`Value`] into a single short string preview for logging.
fn preview_value(v: &Value, n: usize) -> String {
    let raw = match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) => format!("\"{s}\""),
        Value::Bytes(b) => format!("<{} bytes>", b.len()),
        Value::Array(_) | Value::Object(_) => format!("{v:?}"),
        // `Value` is `#[non_exhaustive]`; future variants degrade to a
        // generic Debug-format preview rather than mis-classifying.
        _ => format!("{v:?}"),
    };
    truncate_to_chars(&raw, n)
}

/// Flatten a tool's content blocks into a single human-readable string,
/// for use as the `message` field of [`MessagePayload::ToolError`].
fn flatten_content_to_string(blocks: &[ToolContent]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            ToolContent::Text { text } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
            ToolContent::Json { data } => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&value_to_preview_string(data));
            }
            // `ToolContent` is `#[non_exhaustive]`; future variants
            // contribute nothing to the preview at v0.1.
            _ => {}
        }
    }
    out
}

/// Build a [`Value`] from a tool's content blocks. v0.1 rule:
///
/// - exactly one [`ToolContent::Json { data }`] → return `data` directly;
/// - everything else → wrap into an `Object { content: Array of strings/values }`.
fn content_to_value(blocks: &[ToolContent]) -> Value {
    if blocks.len() == 1 {
        if let ToolContent::Json { data } = &blocks[0] {
            return data.clone();
        }
    }
    let arr: Vec<Value> = blocks
        .iter()
        .map(|b| match b {
            ToolContent::Text { text } => Value::String(text.clone()),
            ToolContent::Json { data } => data.clone(),
            _ => Value::Null,
        })
        .collect();
    let mut obj = BTreeMap::new();
    obj.insert("content".to_string(), Value::Array(arr));
    Value::Object(obj)
}

/// Compact preview string for a [`Value`]. Used for `LlmProviderMessage`
/// projection of a tool result body (the LLM-call shape carries text,
/// not structured Value, in v0.1's content-block surface).
fn value_to_preview_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Bytes(b) => format!("<{} bytes>", b.len()),
        Value::Array(_) | Value::Object(_) => format!("{v:?}"),
        // `Value` is `#[non_exhaustive]`; future variants degrade to a
        // generic Debug-format string rather than mis-classifying.
        _ => format!("{v:?}"),
    }
}

/// Top-level capability kind string used in
/// [`CapabilityDenial::required_kind`] and the `capability.deny` event.
fn capability_kind_str(cap: &Capability) -> String {
    use tau_domain::{AgentCapability, FsCapability, NetCapability, ProcessCapability};
    match cap {
        Capability::Filesystem(FsCapability::Read { .. }) => "fs.read".into(),
        Capability::Filesystem(FsCapability::Write { .. }) => "fs.write".into(),
        Capability::Filesystem(FsCapability::Exec { .. }) => "fs.exec".into(),
        Capability::Network(NetCapability::Http { .. }) => "net.http".into(),
        Capability::Process(ProcessCapability::Spawn { .. }) => "process.spawn".into(),
        Capability::Agent(AgentCapability::Spawn { .. }) => "agent.spawn".into(),
        Capability::Custom { name, .. } => name.clone(),
        // `Capability` is `#[non_exhaustive]`; future variants degrade
        // to a generic tag — additive evolution must not silently
        // mis-classify them.
        _ => "unknown".into(),
    }
}

// ---------------------------------------------------------------------------
// Unit tests for the named helpers (per Task 10's plan).
//
// Integration tests for `Runtime::run` itself live in tests/ (Tasks
// 11-16); they exercise the entire run loop and tracing emission.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use tau_domain::{Address, AgentInstanceId, MessagePayload};

    fn user_text_message(content: &str) -> Message {
        Message::new(
            Address::User,
            Address::Agent(AgentInstanceId::new()),
            MessagePayload::Text {
                content: content.into(),
            },
        )
    }

    // -------------------- build_initial_messages --------------------

    #[test]
    fn build_initial_messages_wraps_initial_in_singleton_vec() {
        let m = user_text_message("hello");
        let out = build_initial_messages(m.clone());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, m.id);
    }

    // -------------------- deserialize_tool_args --------------------

    #[test]
    fn deserialize_tool_args_passthrough_for_object() {
        let mut obj = BTreeMap::new();
        obj.insert("q".into(), Value::String("hello".into()));
        let v = Value::Object(obj);
        let got = deserialize_tool_args(&v, "search", "mock-llm").expect("passthrough");
        assert_eq!(got, &v);
    }

    #[test]
    fn deserialize_tool_args_passthrough_for_null() {
        // v0.1 is unconditional passthrough; richer shape-checks are
        // deferred to Phase 1 per the helper's doc comment.
        let v = Value::Null;
        let got = deserialize_tool_args(&v, "noop", "mock-llm").expect("passthrough");
        assert_eq!(got, &v);
    }

    // -------------------- build_policy_denied_outcome --------------------

    #[test]
    fn build_policy_denied_outcome_carries_denial_in_status() {
        let denial = CapabilityDenial {
            agent_id: "agent-x".into(),
            package_id: "pkg-y".into(),
            tool_name: "file_read".into(),
            required_kind: "fs.read".into(),
            required_detail: "Filesystem(Read { paths: [\"/etc/passwd\"] })".into(),
        };
        let out = build_policy_denied_outcome(denial, vec![], 3, TokenUsage::default());

        let RunOutcome::Failed {
            status,
            total_turns,
            token_usage,
            all_messages,
        } = out
        else {
            panic!("expected Failed");
        };
        let AgentStatus::Failed { kind, detail, .. } = status else {
            panic!("expected AgentStatus::Failed")
        };
        assert_eq!(kind, FailureKind::PolicyDenied);
        let detail = detail.expect("detail must be set");
        assert!(detail.contains("agent-x"), "got: {detail}");
        assert!(detail.contains("file_read"), "got: {detail}");
        assert!(detail.contains("fs.read"), "got: {detail}");
        assert_eq!(total_turns, 3);
        assert_eq!(token_usage, TokenUsage::default());
        assert!(all_messages.is_empty());
    }

    // -------------------- helper unit tests (smoke) --------------------

    #[test]
    fn agent_messages_to_provider_messages_maps_user_text_to_user_role() {
        let history = vec![user_text_message("hi")];
        let provider = agent_messages_to_provider_messages(&history);
        assert_eq!(provider.len(), 1);
        match &provider[0] {
            LlmProviderMessage::User { content } => {
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ContentBlock::Text(t) => assert_eq!(t, "hi"),
                    other => panic!("expected Text, got {other:?}"),
                }
            }
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn agent_messages_to_provider_messages_skips_lifecycle() {
        let m = Message::new(
            Address::System,
            Address::User,
            MessagePayload::Lifecycle(AgentStatus::Ready),
        );
        let provider = agent_messages_to_provider_messages(&[m]);
        assert!(provider.is_empty());
    }

    #[test]
    fn append_assistant_response_appends_only_text_when_present() {
        let mut history: Vec<Message> = vec![];
        let agent_id = AgentInstanceId::new();
        append_assistant_response(&mut history, "out", &[], &agent_id);
        assert_eq!(history.len(), 1);
        match (&history[0].sender, &history[0].payload) {
            (Address::Agent(id), MessagePayload::Text { content }) => {
                assert_eq!(*id, agent_id);
                assert_eq!(content, "out");
            }
            other => panic!("expected Agent / Text, got {other:?}"),
        }
    }

    #[test]
    fn append_assistant_response_no_text_no_tool_uses_pushes_empty_assistant_message() {
        // Defensive: an empty completion still produces an assistant
        // turn so callers' `final_message` doesn't accidentally surface
        // the user's prompt as the response.
        let mut history: Vec<Message> = vec![];
        let agent_id = AgentInstanceId::new();
        append_assistant_response(&mut history, "", &[], &agent_id);
        assert_eq!(history.len(), 1);
        match (&history[0].sender, &history[0].payload) {
            (Address::Agent(id), MessagePayload::Text { content }) => {
                assert_eq!(*id, agent_id);
                assert!(content.is_empty());
            }
            other => panic!("expected Agent / empty Text, got {other:?}"),
        }
    }

    #[test]
    fn truncate_to_chars_respects_utf8_boundaries() {
        // 3 bytes per "é" character in UTF-8; naïve byte-slicing would
        // panic on a non-boundary cut.
        let s = "éééééé"; // 6 chars, 12 bytes
        assert_eq!(truncate_to_chars(s, 3), "ééé");
        assert_eq!(truncate_to_chars(s, 100), "éééééé");
        assert_eq!(truncate_to_chars(s, 0), "");
    }

    #[test]
    fn capability_kind_str_for_custom_variant() {
        // `Capability::Custom` is structural (no inner non_exhaustive
        // variant), so we can construct it directly from outside
        // tau-domain. The other top-level variants wrap variant-level
        // `#[non_exhaustive]` enums (`FsCapability::Read { .. }` etc.)
        // and aren't reachable here without TOML deserialization;
        // their `kind_str` projection is exercised by the integration
        // tests in Tasks 11-16, where `check_capabilities` provides
        // them naturally.
        let mut params = BTreeMap::new();
        params.insert("servers".into(), Value::Null);
        let cap = Capability::Custom {
            name: "mcp.tool.use".into(),
            params,
        };
        assert_eq!(capability_kind_str(&cap), "mcp.tool.use");
    }
}
