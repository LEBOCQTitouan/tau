//! Tool port — `kind = "tool"` plugin contracts.
//!
//! This module currently exports the supporting data types
//! ([`SessionContext`], [`ToolResult`], [`ToolContent`]) exchanged
//! between tau-runtime and tool-plugin adapters. The `Tool` trait
//! (T9) and the `StatelessTool` / `StatelessAdapter` pair (T10) land
//! in subsequent tasks.

use std::time::SystemTime;

use tau_domain::{AgentInstanceId, Value};
use uuid::Uuid;

/// Per-session context handed to a tool plugin when the runtime opens
/// a new session (i.e. at `Tool::init`, added in T9).
///
/// `SessionContext` is `#[non_exhaustive]`: external callers cannot
/// construct it via struct-literal syntax. Construction is performed
/// by tau-runtime; fields are `pub` so plugin authors and in-tree code
/// can pattern-match on the context.
///
/// # Example
///
/// ```ignore
/// // Struct-literal construction is forbidden externally because
/// // `SessionContext` is `#[non_exhaustive]`. The example here is
/// // illustrative; tau-runtime constructs values of this type.
/// use std::time::{Duration, SystemTime};
/// use tau_domain::AgentInstanceId;
/// use tau_ports::tool::SessionContext;
/// use uuid::Uuid;
///
/// let ctx = SessionContext {
///     agent_instance_id: AgentInstanceId::new(),
///     session_id: Uuid::new_v4(),
///     deadline: Some(SystemTime::now() + Duration::from_secs(30)),
/// };
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct SessionContext {
    /// Identity of the agent instance opening this tool session.
    pub agent_instance_id: AgentInstanceId,
    /// Unique identifier for this session, distinct from
    /// `agent_instance_id` because a single agent may open multiple
    /// concurrent sessions against the same tool.
    pub session_id: Uuid,
    /// Optional wall-clock deadline by which the session should
    /// complete. `None` defers to runtime defaults.
    pub deadline: Option<SystemTime>,
}

/// Result of a single `Tool::invoke` call (added in T9).
///
/// Mirrors the MCP tool-result shape: a list of typed content blocks
/// plus an `is_error` flag. The flag distinguishes "the tool ran but
/// reports an error to the LLM" (e.g. file-not-found, HTTP 500) from
/// [`crate::ToolError`], which signals "the tool itself failed to run"
/// (session unhealthy, contract violation, internal bug). The runtime
/// surfaces semantic errors to the agent's LLM via the
/// `MessagePayload::ToolError` envelope; trait-method errors may
/// trigger retry or agent-stop.
///
/// `ToolResult` is `#[non_exhaustive]`: external callers cannot
/// construct it via struct-literal syntax.
///
/// # Example
///
/// ```ignore
/// // Illustrative; `ToolResult` is `#[non_exhaustive]` so external
/// // callers must build it via the data-types builder added alongside
/// // `Tool` in T9.
/// use tau_ports::tool::{ToolContent, ToolResult};
///
/// let ok = ToolResult {
///     content: vec![ToolContent::Text { text: "done".into() }],
///     is_error: false,
/// };
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// Multi-block content describing the tool's output.
    pub content: Vec<ToolContent>,
    /// Whether the tool ran but reports a semantic error to the LLM.
    /// Distinct from [`crate::ToolError`], which signals a trait-method
    /// failure.
    pub is_error: bool,
}

/// One content block within a [`ToolResult`].
///
/// v0.1 admits [`ToolContent::Text`] and [`ToolContent::Json`] only.
/// The enum is `#[non_exhaustive]` to admit additive variants for
/// image, audio, and resource references without a major bump.
///
/// # Example
///
/// ```ignore
/// // Illustrative; `ToolContent` is `#[non_exhaustive]` so external
/// // callers must build it via the data-types builder added alongside
/// // `Tool` in T9.
/// use tau_domain::Value;
/// use tau_ports::tool::ToolContent;
///
/// let blocks = vec![
///     ToolContent::Text { text: "hello".into() },
///     ToolContent::Json { data: Value::String("world".into()) },
/// ];
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ToolContent {
    /// Plain-text content.
    Text {
        /// The text payload.
        text: String,
    },
    /// Structured JSON content, carried as a `tau_domain::Value`.
    Json {
        /// The structured payload.
        data: Value,
    },
    // Future: ImageRef { ... }, AudioRef { ... }, ResourceRef { ... }
}
