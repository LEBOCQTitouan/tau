//! Conformance case: batch tool-use round-trip.
//!
//! Asserts:
//! - `complete()` returns Ok.
//! - `tool_uses.len() == 1`.
//! - `id` is non-empty.
//! - `name` is non-empty.
//! - `input` is a JSON object (not null/scalar).

use std::path::Path;

use tau_plugin_test_support::cassette;
use tau_ports::{CompletionRequest, ContentBlock, LlmBackend, LlmProviderMessage};

pub(crate) async fn run<B, F>(build_plugin: &F, cassettes_dir: &Path)
where
    B: LlmBackend,
    F: Fn(String) -> B + Send + Sync,
{
    let server = cassette::replay(cassettes_dir.join("batch_with_tools.yaml")).await;
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

    let resp = match plugin.complete(req).await {
        Ok(r) => r,
        Err(e) => panic!("[conformance batch_with_tools] complete() failed: {e:?}"),
    };
    assert_eq!(
        resp.tool_uses.len(),
        1,
        "[conformance batch_with_tools] expected exactly 1 tool_use, got {}",
        resp.tool_uses.len(),
    );
    let tu = &resp.tool_uses[0];
    assert!(
        !tu.id.is_empty(),
        "[conformance batch_with_tools] tool_use id was empty"
    );
    assert!(
        !tu.name.is_empty(),
        "[conformance batch_with_tools] tool_use name was empty"
    );
    assert!(
        matches!(tu.input, tau_domain::Value::Object(_)),
        "[conformance batch_with_tools] expected tool_use.input to be Object, got {:?}",
        tu.input,
    );
}
