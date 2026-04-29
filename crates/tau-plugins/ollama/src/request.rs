//! Translate `tau_ports::CompletionRequest` to Ollama's `/api/chat`
//! JSON body.
//!
//! See `docs/superpowers/specs/2026-04-29-ollama-plugin-design.md`
//! §4.2, §7.1, §7.2.

use serde_json::Value;
use tau_ports::{
    CompletionRequest, ContentBlock, LlmProviderMessage, ToolChoice, ToolSpec, ToolUse,
};
use thiserror::Error;

/// Errors raised while building the Ollama request body.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub(crate) enum BuildError {
    /// A `LlmProviderMessage` variant wasn't recognized — possible if
    /// `tau-ports` adds a new variant before the plugin is updated.
    #[error("unknown LlmProviderMessage variant")]
    UnknownMessageVariant,

    /// A `ContentBlock` variant inside an Assistant message wasn't
    /// recognized — possible if `tau-ports` adds e.g. an `Image` block
    /// before the plugin is updated.
    #[error("unknown ContentBlock variant in assistant content")]
    UnknownContentBlock,

    /// Failed to convert a `tau_domain::Value` to JSON — should be
    /// infallible in practice but propagated as a typed error.
    #[error("could not serialize tool input as JSON: {0}")]
    JsonSerialize(#[from] serde_json::Error),
}

/// Build the JSON body for a `POST /api/chat` request.
///
/// `stream` controls the `"stream"` field (false for batch, true for
/// NDJSON streaming).
#[allow(dead_code)]
pub(crate) fn build_chat_body(req: &CompletionRequest, stream: bool) -> Result<Value, BuildError> {
    let mut body = serde_json::json!({
        "model": req.model,
        "messages": translate_messages(req)?,
        "stream": stream,
    });

    // Tools array. Omit entirely when no tools provided OR tool_choice
    // == None (caller explicitly disabled tools).
    if !req.tools.is_empty() && !matches!(req.tool_choice, ToolChoice::None) {
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .map(translate_tool)
                .collect::<Result<Vec<_>, _>>()?,
        );
    }

    // Ollama's /api/chat does NOT accept `tool_choice`. Drop it; warn
    // at debug for Required/Specific so caller knows what happened.
    if matches!(
        req.tool_choice,
        ToolChoice::Required | ToolChoice::Specific { .. }
    ) {
        tracing::debug!(
            target: "ollama_plugin::request",
            tool_choice = ?req.tool_choice,
            "tool_choice unsupported by Ollama /api/chat; ignoring",
        );
    }

    // Sampling overrides → options sub-object with Ollama-specific
    // field names (num_predict, NOT max_tokens).
    let mut options = serde_json::Map::new();
    if let Some(max) = req.max_tokens {
        options.insert("num_predict".into(), serde_json::json!(max));
    }
    if let Some(t) = req.temperature {
        options.insert("temperature".into(), serde_json::json!(f64::from(t)));
    }
    if let Some(p) = req.top_p {
        options.insert("top_p".into(), serde_json::json!(f64::from(p)));
    }
    if let Some(s) = req.seed {
        options.insert("seed".into(), serde_json::json!(s));
    }
    if !req.stop_sequences.is_empty() {
        options.insert("stop".into(), serde_json::json!(req.stop_sequences));
    }
    if !options.is_empty() {
        body["options"] = Value::Object(options);
    }

    if !req.provider_specific.is_empty() {
        tracing::debug!(
            target: "ollama_plugin::request",
            keys = ?req.provider_specific.keys().collect::<Vec<_>>(),
            "ignoring provider_specific keys",
        );
    }

    Ok(body)
}

fn translate_messages(req: &CompletionRequest) -> Result<Value, BuildError> {
    let mut out: Vec<Value> = Vec::new();

    // System prompt: Ollama places it as a leading role:system message
    // (NOT a top-level field like Anthropic).
    if let Some(system) = req.system.as_ref() {
        out.push(serde_json::json!({
            "role": "system",
            "content": system,
        }));
    }

    for msg in &req.messages {
        match msg {
            LlmProviderMessage::User { content } => {
                out.push(serde_json::json!({
                    "role": "user",
                    "content": flatten_text(content),
                }));
            }
            LlmProviderMessage::Assistant { content } => {
                let (text, tool_calls) = split_assistant_content(content)?;
                let mut entry = serde_json::json!({
                    "role": "assistant",
                    "content": text,
                });
                if !tool_calls.is_empty() {
                    entry["tool_calls"] = Value::Array(tool_calls);
                }
                out.push(entry);
            }
            LlmProviderMessage::ToolResult {
                tool_use_id: _,
                content,
                is_error: _,
            } => {
                // Ollama's tool message has no tool_use_id field; the
                // kernel pairs results to calls by message order.
                // is_error is also dropped — tools encode errors in
                // the content payload.
                out.push(serde_json::json!({
                    "role": "tool",
                    "content": flatten_text(content),
                }));
            }
            _ => return Err(BuildError::UnknownMessageVariant),
        }
    }
    Ok(Value::Array(out))
}

fn flatten_text(content: &[ContentBlock]) -> String {
    let mut out = String::new();
    for block in content {
        if let ContentBlock::Text(s) = block {
            out.push_str(s);
        }
    }
    out
}

fn split_assistant_content(content: &[ContentBlock]) -> Result<(String, Vec<Value>), BuildError> {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for block in content {
        match block {
            ContentBlock::Text(s) => text.push_str(s),
            ContentBlock::ToolUse(tu) => {
                tool_calls.push(tool_use_to_call(tu)?);
            }
            _ => return Err(BuildError::UnknownContentBlock),
        }
    }
    Ok((text, tool_calls))
}

fn tool_use_to_call(tu: &ToolUse) -> Result<Value, BuildError> {
    Ok(serde_json::json!({
        "function": {
            "name": tu.name,
            "arguments": serde_json::to_value(&tu.input)?,
        },
    }))
}

fn translate_tool(spec: &ToolSpec) -> Result<Value, BuildError> {
    Ok(serde_json::json!({
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": serde_json::to_value(&spec.input_schema)?,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;
    use tau_domain::Value as DomainValue;
    use tau_ports::fixtures::make_tool_spec;

    fn req_with_user_text(text: &str) -> CompletionRequest {
        let mut req = CompletionRequest::new("llama3.2".into());
        req.messages = vec![LlmProviderMessage::User {
            content: vec![ContentBlock::Text(text.into())],
        }];
        req
    }

    #[test]
    fn happy_path_user_text_only() {
        let req = req_with_user_text("hello");
        let body = build_chat_body(&req, false).unwrap();
        assert_eq!(body["model"], "llama3.2");
        assert_eq!(body["stream"], false);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "hello");
        assert!(body.get("tools").is_none());
        assert!(body.get("options").is_none());
    }

    #[test]
    fn streaming_flag_propagates() {
        let req = req_with_user_text("hi");
        let body = build_chat_body(&req, true).unwrap();
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn system_prompt_emitted_as_leading_role_system_message() {
        let mut req = req_with_user_text("hi");
        req.system = Some("you are concise".into());
        let body = build_chat_body(&req, false).unwrap();
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "you are concise");
        assert_eq!(messages[1]["role"], "user");
        // Critical: NO top-level `system` field at body root.
        assert!(body.get("system").is_none());
    }

    #[test]
    fn multi_block_user_content_concatenated_to_string() {
        let mut req = req_with_user_text("ignored");
        req.messages = vec![LlmProviderMessage::User {
            content: vec![
                ContentBlock::Text("part one ".into()),
                ContentBlock::Text("part two".into()),
            ],
        }];
        let body = build_chat_body(&req, false).unwrap();
        assert_eq!(body["messages"][0]["content"], "part one part two");
    }

    #[test]
    fn assistant_tool_use_splits_into_tool_calls_array() {
        let mut input_obj = BTreeMap::new();
        input_obj.insert("text".into(), DomainValue::String("hi".into()));
        let tu = ToolUse::new(
            "ollama-tool-0".into(),
            "echo".into(),
            DomainValue::Object(input_obj),
        );
        let mut req = req_with_user_text("ignored");
        req.messages = vec![LlmProviderMessage::Assistant {
            content: vec![
                ContentBlock::Text("ok let me ".into()),
                ContentBlock::ToolUse(tu),
            ],
        }];
        let body = build_chat_body(&req, false).unwrap();
        let asst = &body["messages"][0];
        assert_eq!(asst["role"], "assistant");
        assert_eq!(asst["content"], "ok let me ");
        let calls = asst["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["function"]["name"], "echo");
        assert_eq!(calls[0]["function"]["arguments"]["text"], "hi");
    }

    #[test]
    fn tool_result_message_has_no_tool_use_id_field() {
        let mut req = req_with_user_text("ignored");
        req.messages = vec![LlmProviderMessage::ToolResult {
            tool_use_id: "ignored-by-ollama".into(),
            content: vec![ContentBlock::Text("42".into())],
            is_error: false,
        }];
        let body = build_chat_body(&req, false).unwrap();
        let msg = &body["messages"][0];
        assert_eq!(msg["role"], "tool");
        assert_eq!(msg["content"], "42");
        // Critical: ordering pairs tool calls/results, not ids.
        assert!(msg.get("tool_use_id").is_none());
    }

    #[test]
    fn tools_array_emitted_when_non_empty() {
        let mut req = req_with_user_text("hi");
        req.tools = vec![make_tool_spec(
            "echo".into(),
            "echo back".into(),
            DomainValue::Object(Default::default()),
        )];
        let body = build_chat_body(&req, false).unwrap();
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "echo");
        assert_eq!(tools[0]["function"]["description"], "echo back");
        assert_eq!(tools[0]["function"]["parameters"], json!({}));
    }

    #[test]
    fn tool_choice_none_omits_tools_array_entirely() {
        let mut req = req_with_user_text("hi");
        req.tools = vec![make_tool_spec(
            "echo".into(),
            "".into(),
            DomainValue::Object(Default::default()),
        )];
        req.tool_choice = ToolChoice::None;
        let body = build_chat_body(&req, false).unwrap();
        // Critical: ToolChoice::None drops `tools` even when present.
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn tool_choice_specific_dropped_with_no_field_emitted() {
        let mut req = req_with_user_text("hi");
        req.tools = vec![make_tool_spec(
            "echo".into(),
            "".into(),
            DomainValue::Object(Default::default()),
        )];
        req.tool_choice = ToolChoice::Specific {
            name: "echo".into(),
        };
        let body = build_chat_body(&req, false).unwrap();
        // tools still emitted (caller wants tools available)…
        assert!(body.get("tools").is_some());
        // …but tool_choice never gets sent (Ollama doesn't accept it).
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn sampling_overrides_go_into_options_subobject_with_ollama_names() {
        let mut req = req_with_user_text("hi");
        req.max_tokens = Some(100);
        req.temperature = Some(0.7);
        req.top_p = Some(0.9);
        req.seed = Some(42);
        req.stop_sequences = vec!["END".into()];
        let body = build_chat_body(&req, false).unwrap();
        let opts = body["options"].as_object().unwrap();
        // num_predict NOT max_tokens — Ollama-specific name.
        assert_eq!(opts["num_predict"], 100);
        assert_eq!(opts["temperature"], f64::from(0.7f32));
        assert_eq!(opts["top_p"], f64::from(0.9f32));
        assert_eq!(opts["seed"], 42);
        assert_eq!(opts["stop"], json!(["END"]));
    }
}
