//! Integration tests for [`tau_ports::fixtures::MockTool`].
//!
//! Asserts that the mock satisfies the [`Tool`] contract:
//! - `invoke` returns the configured canned [`ToolResult`] when no
//!   error is set.
//! - `invoke` returns the configured error (taking precedence over a
//!   canned result).
//! - Each `invoke` records the args in order.
//! - `init`/`teardown` succeed for the unit-session shape.
//!
//! Gated behind the `test-fixtures` feature: imports `MockTool`.

#![cfg(feature = "test-fixtures")]

use std::time::{Duration, SystemTime};

use tau_domain::{AgentInstanceId, Value};
use tau_ports::error::ToolError;
use tau_ports::fixtures::{make_session_context, make_tool_result, make_tool_spec, MockTool};
use tau_ports::llm::ToolSpec;
use tau_ports::tool::{SessionContext, Tool, ToolContent};
use uuid::Uuid;

fn make_spec(name: &str) -> ToolSpec {
    make_tool_spec(
        name.into(),
        format!("mock-{name}"),
        Value::Object(Default::default()),
    )
}

fn make_ctx() -> SessionContext {
    make_session_context(
        AgentInstanceId::new(),
        Uuid::now_v7(),
        Some(SystemTime::now() + Duration::from_secs(30)),
    )
}

/// `invoke` returns the configured canned [`ToolResult`].
#[tokio::test]
#[allow(clippy::let_unit_value)] // Session = () for MockTool; we
                                 // bind it explicitly to exercise the full Tool lifecycle.
async fn invoke_returns_canned_result() {
    let canned = make_tool_result(
        vec![ToolContent::Text {
            text: "hello".into(),
        }],
        false,
    );
    let tool = MockTool::new("echo", make_spec("echo")).with_result(canned);

    assert_eq!(Tool::name(&tool), "echo");
    assert_eq!(Tool::schema(&tool).name, "echo");

    let mut session = tool.init(make_ctx()).await.expect("init");
    let result = tool
        .invoke(&mut session, Value::String("ignored".into()))
        .await
        .expect("invoke");

    assert!(!result.is_error);
    assert_eq!(result.content.len(), 1);
    assert!(matches!(&result.content[0], ToolContent::Text { text } if text == "hello"));

    tool.teardown(session).await.expect("teardown");
}

/// `invoke` returns the configured error when one is set; the error
/// takes precedence over any canned result.
#[tokio::test]
#[allow(clippy::let_unit_value)] // Session = () for MockTool.
async fn invoke_returns_canned_error() {
    let canned_result = make_tool_result(Vec::new(), false);
    let canned_error = ToolError::BadArgs {
        reason: "missing field".into(),
    };
    let tool = MockTool::new("err", make_spec("err"))
        .with_result(canned_result)
        .with_error(canned_error.clone());

    let mut session = tool.init(make_ctx()).await.expect("init");
    let err = tool
        .invoke(&mut session, Value::Null)
        .await
        .expect_err("should error");
    assert_eq!(err, canned_error);
}

/// `invoke` records each invocation's args in order.
#[tokio::test]
#[allow(clippy::let_unit_value)] // Session = () for MockTool.
async fn invocations_are_recorded_in_order() {
    let tool = MockTool::new("rec", make_spec("rec"));

    let mut session = tool.init(make_ctx()).await.expect("init");

    let _ = tool
        .invoke(&mut session, Value::Integer(1))
        .await
        .expect("c1");
    let _ = tool
        .invoke(&mut session, Value::String("two".into()))
        .await
        .expect("c2");
    let _ = tool
        .invoke(&mut session, Value::Bool(false))
        .await
        .expect("c3");

    let recorded = tool.invocations();
    assert_eq!(recorded.len(), 3);
    assert!(recorded[0] == Value::Integer(1));
    assert!(recorded[1] == Value::String("two".into()));
    assert!(recorded[2] == Value::Bool(false));

    tool.teardown(session).await.expect("teardown");
}

/// An unconfigured `MockTool` returns a default empty success result.
#[tokio::test]
#[allow(clippy::let_unit_value)] // Session = () for MockTool.
async fn unconfigured_returns_default_success() {
    let tool = MockTool::new("default", make_spec("default"));
    let mut session = tool.init(make_ctx()).await.expect("init");
    let result = tool
        .invoke(&mut session, Value::Null)
        .await
        .expect("invoke");
    assert!(!result.is_error);
    assert!(result.content.is_empty());
}
