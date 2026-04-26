//! Integration tests for [`tau_ports::tool::StatelessAdapter`] driven
//! through the [`Tool`] interface.
//!
//! Defines a minimal `EchoStatelessTool` and wraps it in
//! [`StatelessAdapter`]. Exercises the full stateful lifecycle (`name`,
//! `schema`, `init`, `invoke`, `teardown`) and asserts the adapter
//! delegates `invoke` to the underlying [`StatelessTool`] correctly.
//!
//! Gated behind the `test-fixtures` feature for parity with the rest of
//! the integration suite (this file does not strictly require fixtures
//! but the suite-wide convention is one feature gate).

#![cfg(feature = "test-fixtures")]

use std::time::{Duration, SystemTime};

use tau_domain::{AgentInstanceId, Value};
use tau_ports::error::ToolError;
use tau_ports::fixtures::{make_session_context, make_tool_result, make_tool_spec};
use tau_ports::llm::ToolSpec;
use tau_ports::tool::{
    SessionContext, StatelessAdapter, StatelessTool, Tool, ToolContent, ToolResult,
};
use uuid::Uuid;

/// Minimal stateless tool: echoes the input args back as a JSON content
/// block. Returns `is_error: false` always.
struct EchoStatelessTool;

impl StatelessTool for EchoStatelessTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn schema(&self) -> ToolSpec {
        make_tool_spec(
            "echo".into(),
            "echo args back".into(),
            Value::Object(Default::default()),
        )
    }

    async fn invoke(&self, args: Value) -> Result<ToolResult, ToolError> {
        Ok(make_tool_result(
            vec![ToolContent::Json { data: args }],
            false,
        ))
    }
}

/// Build a `SessionContext` via the fixture factory (the struct is
/// `#[non_exhaustive]` so external integration tests can't use
/// struct-literal construction).
fn make_ctx() -> SessionContext {
    make_session_context(
        AgentInstanceId::new(),
        Uuid::now_v7(),
        Some(SystemTime::now() + Duration::from_secs(30)),
    )
}

/// `StatelessAdapter` exposes the wrapped tool's `name` and `schema`,
/// runs `init` returning `()`, delegates `invoke` to the inner tool,
/// and `teardown` is a no-op.
#[tokio::test]
#[allow(clippy::let_unit_value)] // Session = () for the adapter; we
                                 // bind it explicitly to exercise the full Tool lifecycle.
async fn stateless_adapter_full_lifecycle() {
    let tool = StatelessAdapter(EchoStatelessTool);

    // name + schema delegate to the underlying tool.
    assert_eq!(Tool::name(&tool), "echo");
    let spec = Tool::schema(&tool);
    assert_eq!(spec.name, "echo");
    assert_eq!(spec.description, "echo args back");

    // init returns Ok(()).
    let mut session = tool.init(make_ctx()).await.expect("init should succeed");

    // invoke round-trips the input.
    let result = tool
        .invoke(&mut session, Value::String("hi".into()))
        .await
        .expect("invoke should succeed");
    assert!(!result.is_error);
    assert_eq!(result.content.len(), 1);
    let ToolContent::Json { data } = &result.content[0] else {
        panic!("expected JSON content, got {:?}", result.content[0]);
    };
    assert!(*data == Value::String("hi".into()));

    // teardown returns Ok(()).
    tool.teardown(session)
        .await
        .expect("teardown should succeed");
}

/// Multiple sequential invocations against the same session each
/// produce independent results.
#[tokio::test]
#[allow(clippy::let_unit_value)] // Session = () for the adapter.
async fn stateless_adapter_repeated_invoke() {
    let tool = StatelessAdapter(EchoStatelessTool);
    let mut session = tool.init(make_ctx()).await.expect("init");

    for i in 0..3 {
        let arg = Value::Integer(i);
        let result = tool
            .invoke(&mut session, arg.clone())
            .await
            .expect("invoke");
        assert!(!result.is_error);
        let ToolContent::Json { data } = &result.content[0] else {
            panic!("expected JSON content");
        };
        assert!(*data == arg);
    }

    tool.teardown(session).await.expect("teardown");
}
