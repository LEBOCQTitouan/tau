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

#[cfg(test)]
use tau_domain::AgentInstanceId;
use tau_domain::{
    AgentDefinition, AgentStatus, Capability, FailureKind, Message, MessagePayload,
    PackageManifest, Value,
};
use tau_ports::{ContentBlock, LlmProviderMessage, ToolContent, ToolUse};
use tracing::instrument;

use crate::builder::Runtime;
use crate::capability_override::EffectiveCapability;
use crate::error::{CapabilityDenial, RuntimeError};
use crate::options::{RunOptions, TokenUsage};
use crate::outcome::RunOutcome;

impl Runtime {
    /// Run an agent with a pre-existing conversation history.
    ///
    /// Identical to [`Runtime::run`] except `history` is pre-loaded into
    /// the messages buffer for turn 1: the kernel projects `history`
    /// (plus the new `initial_message`) onto the
    /// [`CompletionRequest::messages`] list before the first LLM call.
    /// Subsequent turns evolve the buffer normally — assistant text,
    /// tool calls, tool results — exactly as in
    /// [`Runtime::run`].
    ///
    /// This is the entry point used by `tau chat` (sub-project 5) to
    /// thread REPL conversation history across user turns. Per NG6
    /// ("no persistent agent memory in core"), the kernel itself does
    /// not retain history between calls — the CLI accumulates a
    /// `Vec<Message>` and passes it back in on each turn.
    ///
    /// The loop is otherwise identical to [`Runtime::run`]:
    ///
    /// 1. Build a [`CompletionRequest`] from `history + initial_message`
    ///    on turn 1; from the evolving buffer on later turns.
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
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // RunOutcome and AgentDefinition are #[non_exhaustive]; doctests
    /// // can't construct them via struct-literal syntax. Example illustrative.
    /// use tau_runtime::{Runtime, RunOptions};
    ///
    /// let outcome = runtime
    ///     .run_with_history(agent_def, manifest, history, initial_message, RunOptions::default())
    ///     .await?;
    /// ```
    #[instrument(
        name = "runtime.agent_run",
        skip_all,
        fields(
            agent_id = %agent_def.id,
            display_name = %agent_def.display_name,
            package_id = %agent_def.package.name,
            llm_backend_name = %agent_def.llm_backend,
            max_turns = options.max_turns,
            history_len = history.len(),
        ),
    )]
    pub async fn run_with_history(
        &self,
        agent_def: AgentDefinition,
        package_manifest: PackageManifest,
        history: Vec<Message>,
        initial_message: Message,
        options: RunOptions,
    ) -> Result<RunOutcome, RuntimeError> {
        use crate::stream::RunEvent;
        use futures_core::Stream as _;
        use tau_ports::{LlmError, ToolError};

        // Delegate all agent-loop logic to run_streaming_with_history.
        // This function is now a thin stream-drainer: it consumes the
        // stream and returns the terminal RunCompleted.outcome.
        // The streaming pump (stream.rs::run_streaming_inner) is the
        // single source of truth for the agent loop.
        let stream = self
            .run_streaming_with_history(
                agent_def,
                package_manifest,
                history,
                initial_message,
                options,
            )
            .await?;
        let mut stream = Box::pin(stream);
        loop {
            let next = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await;
            match next {
                Some(RunEvent::RunCompleted { outcome }) => return Ok(outcome),
                Some(RunEvent::FatalError {
                    kind,
                    detail,
                    context_json,
                }) => {
                    // ADR-0006 error/failure dichotomy: the streaming pump
                    // emits FatalError for plugin/dispatch failures that
                    // must propagate as Err(RuntimeError) in the batch path.
                    return Err(match kind.as_str() {
                        "ToolNotRegistered" => {
                            // Parse context_json to extract tool_name and
                            // registered list (emitted by make_tool_not_registered_error).
                            let (tool_name, registered) = context_json
                                .as_deref()
                                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                                .and_then(|v| {
                                    let tn = v["tool_name"].as_str()?.to_owned();
                                    let reg: Vec<String> = v["registered"]
                                        .as_array()?
                                        .iter()
                                        .filter_map(|x| x.as_str().map(String::from))
                                        .collect();
                                    Some((tn, reg))
                                })
                                .unwrap_or_else(|| (detail.clone(), vec![]));
                            RuntimeError::ToolNotRegistered {
                                tool_name,
                                registered,
                            }
                        }
                        "Llm" => RuntimeError::Llm(LlmError::Internal { message: detail }),
                        "Tool" => RuntimeError::Tool(ToolError::Internal { message: detail }),
                        _ => RuntimeError::Internal { message: detail },
                    });
                }
                Some(_) => continue,
                None => unreachable!(
                    "run_streaming_inner must yield exactly one RunCompleted before stream end"
                ),
            }
        }
    }

    /// Run an agent through one solo-path multi-turn iteration with no
    /// prior conversation history.
    ///
    /// Thin wrapper around [`Runtime::run_with_history`] with
    /// `history = vec![]`. See `run_with_history` for the full loop
    /// semantics, error/failure dichotomy, and tracing vocabulary.
    /// Existing callers that don't need history threading should
    /// continue to use this entry point.
    pub async fn run(
        &self,
        agent_def: AgentDefinition,
        package_manifest: PackageManifest,
        initial_message: Message,
        options: RunOptions,
    ) -> Result<RunOutcome, RuntimeError> {
        self.run_with_history(
            agent_def,
            package_manifest,
            Vec::new(),
            initial_message,
            options,
        )
        .await
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
///
/// Now only called from unit tests (the agent loop moved to stream.rs).
#[cfg(test)]
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
///
/// Now only called from unit tests (the agent loop moved to stream.rs).
#[cfg(test)]
fn truncate_to_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Flatten a tool's content blocks into a single human-readable string,
/// for use as the `message` field of [`MessagePayload::ToolError`].
pub(crate) fn flatten_content_to_string(blocks: &[ToolContent]) -> String {
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
pub(crate) fn content_to_value(blocks: &[ToolContent]) -> Value {
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

/// Build the post-narrow `Capability` view that flows to plugins via
/// `SessionContext.granted_capabilities`. Capability inner variants are
/// `#[non_exhaustive]` and can't be constructed cross-crate; we serialize
/// the source, splice in the narrowed allow-list / max_bytes, and
/// deserialize back.
///
/// Failure-safe: any serialization failure falls back to `eff.source.clone()`.
/// The kernel's structural cap check still applies — narrowing is best-effort
/// at this layer; panicking on a security-enforcement path would be the
/// wrong failure mode.
pub(crate) fn narrowed_capability_for_session(eff: &EffectiveCapability) -> Capability {
    use serde_json::{json, Value as Jv};

    let source_json = match serde_json::to_value(&eff.source) {
        Ok(v) => v,
        Err(_) => return eff.source.clone(),
    };
    let mut obj = match source_json.as_object() {
        Some(m) => m.clone(),
        None => return eff.source.clone(),
    };
    if let Some(allow) = &eff.allow_override {
        // Replace the kind-appropriate field. For unknown kinds (e.g. Custom),
        // bail and return source unchanged — narrowing is unsupported.
        let field = match obj.get("kind").and_then(Jv::as_str) {
            Some("fs.read") | Some("fs.write") | Some("fs.exec") => "paths",
            Some("net.http") => "hosts",
            Some("process.spawn") => "commands",
            _ => return eff.source.clone(),
        };
        obj.insert(field.to_string(), json!(allow));
    }
    if let Some(mb) = eff.max_bytes_override {
        obj.insert("max_bytes".to_string(), json!(mb));
    }
    serde_json::from_value(Jv::Object(obj)).unwrap_or_else(|_| eff.source.clone())
}

/// Top-level capability kind string used in
/// [`CapabilityDenial::required_kind`] and the `capability.deny` event.
pub(crate) fn capability_kind_str(cap: &Capability) -> String {
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
    fn capability_kind_str_for_filesystem_read() {
        // Round-trip an `fs.read` capability through the manifest wire
        // form (variant-level `#[non_exhaustive]` blocks struct-literal
        // construction from outside `tau-domain`) and assert the
        // top-level kind projection matches the existing dot-namespaced
        // taxonomy used by `CapabilityDenial::required_kind` and the
        // `runtime.tool_filtered` event.
        #[derive(serde::Deserialize)]
        struct CapWrapper {
            cap: Capability,
        }
        let cap = toml::from_str::<CapWrapper>(
            r#"[cap]
kind = "fs.read"
paths = ["**"]
"#,
        )
        .expect("test fs.read capability TOML must parse")
        .cap;
        assert_eq!(capability_kind_str(&cap), "fs.read");
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
