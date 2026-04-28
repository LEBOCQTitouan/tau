//! Integration test: agent completes a single turn with a text
//! response (no tool_uses) and the run loop returns
//! `RunOutcome::Completed`.

mod common;

use tau_domain::MessagePayload;
use tau_ports::fixtures::{make_completion_response, make_token_usage, MockLlmBackend};
use tau_ports::StopReason;
use tau_runtime::{RunOptions, RunOutcome, Runtime};

#[tokio::test]
async fn run_completes_with_text_response() {
    // 1. Canned LLM response: a single text block, no tool_uses, EndTurn.
    let resp = make_completion_response(
        "hello world".into(),
        Vec::new(),
        StopReason::EndTurn,
        Some(make_token_usage(5, 10)),
    );
    let llm = MockLlmBackend::new("gpt-4").with_response(resp);

    // 2. Build the runtime with just the LLM backend.
    let runtime = Runtime::builder()
        .with_llm_backend(llm)
        .build()
        .expect("build runtime");

    // 3. Compose the agent definition + manifest + initial message.
    let agent_def = common::agent_def("agent-1", "test-agent", "test-pkg@0.1.0", "gpt-4");
    let manifest = common::manifest_with_no_capabilities();
    let initial = common::user_message("Hi");

    // 4. Run.
    let outcome = runtime
        .run(agent_def, manifest, initial, RunOptions::default())
        .await
        .expect("run succeeded");

    // 5. Assert the happy-path shape.
    let RunOutcome::Completed {
        final_message,
        all_messages,
        total_turns,
        token_usage,
        ..
    } = outcome
    else {
        panic!("expected Completed, got Failed");
    };
    assert_eq!(total_turns, 1, "single LLM call, no tool_uses");
    assert_eq!(
        all_messages.len(),
        2,
        "initial user msg + assistant text response"
    );
    match &final_message.payload {
        MessagePayload::Text { content } => assert_eq!(content, "hello world"),
        other => panic!("expected Text payload, got {other:?}"),
    }
    assert_eq!(token_usage.input_tokens, 5);
    assert_eq!(token_usage.output_tokens, 10);
}
