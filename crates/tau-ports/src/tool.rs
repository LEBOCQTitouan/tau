//! Tool port — `kind = "tool"` plugin contracts.
//!
//! This module exports the [`Tool`] trait, the supporting data types
//! ([`SessionContext`], [`ToolResult`], [`ToolContent`]) exchanged
//! between tau-runtime and tool-plugin adapters, and the
//! [`StatelessTool`] / [`StatelessAdapter`] pair for the common
//! stateless case.

use std::time::SystemTime;

use tau_domain::{AgentInstanceId, Value};
use uuid::Uuid;

use crate::error::ToolError;
use crate::llm::ToolSpec;

/// Per-session context handed to a tool plugin when the runtime opens
/// a new session (i.e. at [`Tool::init`]).
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

/// Result of a single [`Tool::invoke`] call.
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

/// Trait implemented by `kind = "tool"` plugins.
///
/// Stateful by design — see [`StatelessAdapter`] for the common
/// stateless case.
///
/// # Error semantics
///
/// `Err(ToolError)` means *the tool itself failed to run* (session
/// unhealthy, contract violation, internal bug). `Ok(ToolResult { is_error: true, ... })`
/// means *the tool ran but the operation reports an error to the LLM*
/// (file not found, HTTP failure, etc.). The runtime treats these
/// differently: errors may trigger retry/agent-stop; semantic failures
/// are surfaced to the agent's LLM via `MessagePayload::ToolError`.
#[allow(async_fn_in_trait)]
pub trait Tool: Send + Sync {
    /// Per-session state. Use `()` for stateless tools (or use [`StatelessAdapter`]).
    type Session: Send + 'static;

    /// Stable name used for routing. SemVer-stable surface.
    fn name(&self) -> &str;

    /// JSON Schema describing the tool's input. Used both for runtime
    /// validation and for surfacing to the LLM via
    /// `CompletionRequest.tools`.
    fn schema(&self) -> ToolSpec;

    /// Capabilities this tool requires the calling agent's package to declare.
    /// Default: empty (tool is unrestricted; any agent can call it).
    ///
    /// The runtime checks: for every capability in this list, the agent's
    /// package manifest must contain at least one capability that satisfies
    /// it. See `tau_runtime::capability::check_capabilities` for the
    /// satisfies-relation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // `Capability` is `#[non_exhaustive]`; declared via the manifest path.
    /// use tau_domain::Capability;
    /// use tau_ports::Tool;
    ///
    /// struct MyFileTool;
    /// // impl Tool for MyFileTool {
    /// //     fn capabilities(&self) -> &[Capability] { &[] }
    /// //     // ... other methods ...
    /// // }
    /// # let _ = std::any::type_name::<MyFileTool>();
    /// ```
    fn capabilities(&self) -> &[tau_domain::Capability] {
        &[]
    }

    /// Open a session. Called once before any `invoke`.
    async fn init(&self, ctx: SessionContext) -> Result<Self::Session, ToolError>;

    /// Perform a single tool call within an open session.
    async fn invoke(
        &self,
        session: &mut Self::Session,
        args: Value,
    ) -> Result<ToolResult, ToolError>;

    /// Close the session gracefully. If the runtime drops the session
    /// future (cancellation), `teardown` is NOT called — plugin authors
    /// put critical cleanup in `Drop`.
    async fn teardown(&self, session: Self::Session) -> Result<(), ToolError>;
}

/// Simpler trait for stateless tools. Implement this and wrap with
/// [`StatelessAdapter`] to satisfy [`Tool`] with `Session = ()`.
///
/// Most tools (filesystem read, HTTP fetch, calculator, search APIs,
/// MCP) are stateless and should use this trait. Tools that need
/// per-session state (browser drivers, database connections, GPU
/// handles) should implement [`Tool`] directly.
#[allow(async_fn_in_trait)]
pub trait StatelessTool: Send + Sync {
    /// Stable name used for routing. SemVer-stable surface.
    fn name(&self) -> &str;
    /// JSON Schema describing the tool's input. Used both for runtime
    /// validation and for surfacing to the LLM via
    /// `CompletionRequest.tools`.
    fn schema(&self) -> ToolSpec;
    /// Perform a single tool call. Stateless: no session is threaded
    /// through.
    async fn invoke(&self, args: Value) -> Result<ToolResult, ToolError>;
}

/// Newtype that adapts a [`StatelessTool`] to satisfy [`Tool`] with
/// `Session = ()`.
///
/// # Example
///
/// ```ignore
/// // StatelessAdapter wraps a StatelessTool to give it a Session lifecycle.
/// // Plugin author writes:
/// // let tool = StatelessAdapter(MyTool);
/// // runtime.register_tool(Box::new(tool));
/// ```
pub struct StatelessAdapter<T: StatelessTool>(pub T);

impl<T: StatelessTool> Tool for StatelessAdapter<T> {
    type Session = ();

    fn name(&self) -> &str {
        self.0.name()
    }

    fn schema(&self) -> ToolSpec {
        self.0.schema()
    }

    async fn init(&self, _: SessionContext) -> Result<(), ToolError> {
        Ok(())
    }

    async fn invoke(&self, _: &mut (), args: Value) -> Result<ToolResult, ToolError> {
        self.0.invoke(args).await
    }

    async fn teardown(&self, _: ()) -> Result<(), ToolError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tau_domain::Value;

    struct EchoTool;
    impl StatelessTool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn schema(&self) -> ToolSpec {
            ToolSpec {
                name: "echo".into(),
                description: "echo args back".into(),
                input_schema: Value::Object(Default::default()),
            }
        }
        async fn invoke(&self, args: Value) -> Result<ToolResult, ToolError> {
            Ok(ToolResult {
                content: vec![ToolContent::Json { data: args }],
                is_error: false,
            })
        }
    }

    #[tokio::test]
    #[allow(clippy::let_unit_value)] // Session = () for the adapter; we
                                     // bind it explicitly to exercise the full Tool lifecycle.
    async fn stateless_adapter_round_trip() {
        let tool = StatelessAdapter(EchoTool);
        assert_eq!(Tool::name(&tool), "echo");

        let mut session = tool
            .init(SessionContext {
                agent_instance_id: tau_domain::AgentInstanceId::new(),
                session_id: uuid::Uuid::now_v7(),
                deadline: None,
            })
            .await
            .unwrap();

        let result = tool
            .invoke(&mut session, Value::String("hi".into()))
            .await
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content.len(), 1);

        tool.teardown(session).await.unwrap();
    }
}
