//! Conformance case: streaming tool-use round-trip.
//!
//! Asserts:
//! - `stream()` returns Ok.
//! - At least one `ToolUse` chunk arrives BEFORE `Finish`.
//! - `tu.input` is a JSON object.

use std::path::Path;

use futures_util::StreamExt;
use tau_plugin_test_support::cassette;
use tau_ports::{CompletionChunk, CompletionRequest, ContentBlock, LlmBackend, LlmProviderMessage};

pub(crate) async fn run<B, F>(build_plugin: &F, cassettes_dir: &Path)
where
    B: LlmBackend,
    F: Fn(String) -> B + Send + Sync,
{
    let server = cassette::replay(cassettes_dir.join("streaming_tool_use.yaml")).await;
    let plugin = build_plugin(server.uri().into());

    let mut req = CompletionRequest::new("conformance-model".into());
    req.messages
        .push(LlmProviderMessage::user(vec![ContentBlock::Text(
            "use echo".into(),
        )]));
    req.max_tokens = Some(20);
    req.tools.push(tau_ports::fixtures::make_tool_spec(
        "echo".into(),
        "echo input".into(),
        tau_domain::Value::Object(Default::default()),
    ));

    let mut stream = match plugin.stream(req).await {
        Ok(s) => s,
        Err(e) => panic!("[conformance streaming_tool_use] stream() failed: {e:?}"),
    };

    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item);
    }

    let mut tool_uses_before_finish: Vec<&tau_ports::ToolUse> = Vec::new();
    let mut saw_finish = false;
    for c in &chunks {
        match c {
            Ok(CompletionChunk::ToolUse(tu)) => {
                if !saw_finish {
                    tool_uses_before_finish.push(tu);
                }
            }
            Ok(CompletionChunk::Finish { .. }) => {
                saw_finish = true;
            }
            Ok(_) => {}
            Err(e) => panic!("[conformance streaming_tool_use] chunk was Err: {e:?}"),
        }
    }
    assert!(
        !tool_uses_before_finish.is_empty(),
        "[conformance streaming_tool_use] expected >= 1 ToolUse chunk before Finish",
    );
    let tu = tool_uses_before_finish[0];
    assert!(
        matches!(tu.input, tau_domain::Value::Object(_)),
        "[conformance streaming_tool_use] expected tool_use.input to be Object, got {:?}",
        tu.input,
    );
}
