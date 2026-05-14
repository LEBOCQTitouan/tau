//! Integration tests: kernel-level operational failures of `Runtime::run`.
//!
//! Per ADR-0006, kernel-level failures (plugin/dispatch problems) flow
//! through `Err(RuntimeError)` rather than `Ok(RunOutcome::Failed)`.
//! Three scenarios are exercised:
//!
//! - `llm_backend_not_registered`: agent's `llm_backend` doesn't match
//!   any registered backend.
//! - `tool_not_registered`: the LLM emits a tool_use for an unknown tool.
//! - `plugin_contract_violation`: deferred to Phase 1 (currently
//!   unreachable; see the test's `#[ignore]` rationale).

mod common;

use std::str::FromStr;

use tau_domain::{PackageName, Value};
use tau_ports::fixtures::{
    make_completion_response, make_token_usage, make_tool_use, MockLlmBackend,
};
use tau_ports::{
    CompletionRequest, CompletionResponse, CompletionStream, LlmBackend, LlmError, StopReason,
};
use tau_runtime::{RunOptions, Runtime, RuntimeError};

use assert_matches::assert_matches;

/// Single-shot LLM (re-used across the tests in this file).
struct OneShotLlm {
    name: String,
    response: CompletionResponse,
}

impl LlmBackend for OneShotLlm {
    fn name(&self) -> &str {
        &self.name
    }
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Ok(self.response.clone())
    }
    async fn stream(&self, req: CompletionRequest) -> Result<CompletionStream, LlmError> {
        let resp = self.complete(req).await?;
        Ok(tau_ports::batch_to_stream(resp))
    }
}

#[tokio::test]
async fn llm_backend_not_registered() {
    // Register one backend...
    let runtime = Runtime::builder()
        .with_llm_backend(MockLlmBackend::new("different-backend"))
        .build()
        .expect("build runtime");

    // ...but ask the runtime to drive an agent that wants a different one.
    let mut agent_def = common::agent_def("agent-1", "test-agent", "test-pkg@0.1.0", "gpt-4");
    agent_def.llm_backend = PackageName::from_str("missing-backend").expect("valid name");
    let manifest = common::manifest_with_no_capabilities();
    let initial = common::user_message("hello");

    let result = runtime
        .run(agent_def, manifest, initial, RunOptions::default())
        .await;

    let err = result.unwrap_err();
    assert_matches!(
        err,
        RuntimeError::LlmBackendNotRegistered {
            agent_id, backend, ..
        } => {
            assert_eq!(agent_id, "agent-1");
            assert_eq!(backend, "missing-backend");
        }
    );
}

#[tokio::test]
async fn tool_not_registered() {
    // LLM tells the runtime to call a tool that isn't registered.
    let response = make_completion_response(
        String::new(),
        vec![make_tool_use(
            "u1".into(),
            "nonexistent".into(),
            Value::Null,
        )],
        StopReason::ToolUse,
        Some(make_token_usage(2, 1)),
    );
    let llm = OneShotLlm {
        name: "gpt-4".into(),
        response,
    };

    let runtime = Runtime::builder()
        .with_llm_backend(llm)
        // Intentionally NO `with_tool` calls.
        .build()
        .expect("build runtime");

    let agent_def = common::agent_def("agent-1", "test-agent", "test-pkg@0.1.0", "gpt-4");
    let manifest = common::manifest_with_no_capabilities();
    let initial = common::user_message("call the missing tool");

    let result = runtime
        .run(agent_def, manifest, initial, RunOptions::default())
        .await;

    let err = result.unwrap_err();
    assert_matches!(
        err,
        RuntimeError::ToolNotRegistered {
            tool_name,
            registered,
            ..
        } => {
            assert_eq!(tool_name, "nonexistent");
            assert!(
                registered.is_empty(),
                "no tools registered; got {registered:?}"
            );
        }
    );
}

#[tokio::test]
#[ignore = "deserialize_tool_args is a passthrough at v0.1; PluginContractViolation \
            triggers land in Phase 1 with schema validation. See Task 10 commit \
            2562996 and ADR-0006 for the deferral."]
async fn plugin_contract_violation() {
    // Phase-1 placeholder. Once `deserialize_tool_args` performs
    // JSON-schema validation against `ToolSpec::input_schema`, a
    // malformed `tool_use.input` will surface here as
    // `RuntimeError::PluginContractViolation`. At v0.1 the helper is
    // an unconditional passthrough (run.rs §`deserialize_tool_args`),
    // so there is no code path to exercise yet.
}
