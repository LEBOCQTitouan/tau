//! Conformance case: streaming text response.
//!
//! Asserts:
//! - `stream()` returns Ok.
//! - At least one `Text` chunk arrives.
//! - Exactly one `Finish` chunk arrives.
//! - `Finish` is the last chunk.

use std::path::Path;

use futures_util::StreamExt;
use tau_plugin_test_support::cassette;
use tau_ports::{CompletionChunk, CompletionRequest, ContentBlock, LlmBackend, LlmProviderMessage};

pub(crate) async fn run<B, F>(build_plugin: &F, cassettes_dir: &Path)
where
    B: LlmBackend,
    F: Fn(String) -> B + Send + Sync,
{
    let server = cassette::replay(cassettes_dir.join("streaming_text.yaml")).await;
    let plugin = build_plugin(server.uri().into());

    let mut req = CompletionRequest::new("conformance-model".into());
    req.messages
        .push(LlmProviderMessage::user(vec![ContentBlock::Text(
            "hi".into(),
        )]));
    req.max_tokens = Some(20);

    let mut stream = match plugin.stream(req).await {
        Ok(s) => s,
        Err(e) => panic!("[conformance streaming_text] stream() failed: {e:?}"),
    };

    let mut chunks = Vec::new();
    while let Some(item) = stream.next().await {
        chunks.push(item);
    }

    let mut text_count = 0;
    let mut finish_count = 0;
    let mut finish_position = None;
    for (i, c) in chunks.iter().enumerate() {
        match c {
            Ok(CompletionChunk::Text { .. }) => text_count += 1,
            Ok(CompletionChunk::Finish { .. }) => {
                finish_count += 1;
                finish_position = Some(i);
            }
            Ok(_) => {}
            Err(e) => {
                panic!("[conformance streaming_text] chunk {i} was Err: {e:?}");
            }
        }
    }
    assert!(
        text_count >= 1,
        "[conformance streaming_text] expected >= 1 Text chunk, got {text_count}",
    );
    assert_eq!(
        finish_count, 1,
        "[conformance streaming_text] expected exactly 1 Finish chunk, got {finish_count}",
    );
    assert_eq!(
        finish_position,
        Some(chunks.len() - 1),
        "[conformance streaming_text] Finish must be the last chunk",
    );
}
