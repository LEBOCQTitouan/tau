//! Translate `tau_ports::CompletionRequest` to OpenAI's
//! `/v1/chat/completions` JSON body.
//!
//! See `docs/superpowers/specs/2026-04-29-openai-plugin-design.md`
//! §4.2, §7.

use serde_json::Value;
use tau_ports::{
    CompletionRequest, ContentBlock, LlmProviderMessage, ToolChoice, ToolSpec, ToolUse,
};
use thiserror::Error;

/// Errors raised while building the OpenAI request body.
#[derive(Debug, Error)]
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

/// Build the JSON body for a `POST /v1/chat/completions` request.
///
/// `stream` controls the `"stream"` field (false for batch, true for
/// SSE streaming).
pub(crate) fn build_chat_completions_body(
    req: &CompletionRequest,
    stream: bool,
) -> Result<Value, BuildError> {
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

    // tool_choice: round-trip all four variants. Distinct from Ollama
    // which drops Required/Specific.
    if let Some(tc) = translate_tool_choice(&req.tool_choice) {
        body["tool_choice"] = tc;
    }

    // Sampling overrides go at TOP LEVEL (NOT in an `options` sub-
    // object like Ollama). Field names are OpenAI-native.
    if let Some(max) = req.max_tokens {
        body["max_tokens"] = serde_json::json!(max);
    }
    if let Some(t) = req.temperature {
        body["temperature"] = serde_json::json!(f64::from(t));
    }
    if let Some(p) = req.top_p {
        body["top_p"] = serde_json::json!(f64::from(p));
    }
    if let Some(s) = req.seed {
        body["seed"] = serde_json::json!(s);
    }
    if !req.stop_sequences.is_empty() {
        body["stop"] = serde_json::json!(req.stop_sequences);
    }

    if !req.provider_specific.is_empty() {
        tracing::debug!(
            target: "openai_plugin::request",
            keys = ?req.provider_specific.keys().collect::<Vec<_>>(),
            "ignoring provider_specific keys",
        );
    }

    Ok(body)
}

fn translate_messages(req: &CompletionRequest) -> Result<Value, BuildError> {
    let mut out: Vec<Value> = Vec::new();

    // System prompt: OpenAI places it as a leading role:system message
    // (matches Ollama; OpenAI has no top-level system field).
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
                let mut entry = serde_json::json!({ "role": "assistant" });
                // OpenAI accepts `content: null` for tool-only messages;
                // emit an empty string when text is empty AND tool_calls
                // are present (defensive: prevents some servers from
                // rejecting a missing content key).
                entry["content"] = if text.is_empty() && !tool_calls.is_empty() {
                    Value::Null
                } else {
                    Value::String(text)
                };
                if !tool_calls.is_empty() {
                    entry["tool_calls"] = Value::Array(tool_calls);
                }
                out.push(entry);
            }
            LlmProviderMessage::ToolResult {
                tool_use_id,
                content,
                is_error: _,
            } => {
                // OpenAI's tool message round-trips tool_call_id (distinct
                // from Ollama, which has no such field).
                // is_error is dropped — tools encode errors in content.
                out.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id,
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
    // Critical: OpenAI's wire format requires `arguments` to be a
    // JSON-encoded STRING, not a JSON object.
    let arguments_value = serde_json::to_value(&tu.input)?;
    let arguments_string = serde_json::to_string(&arguments_value)?;
    Ok(serde_json::json!({
        "id": tu.id,
        "type": "function",
        "function": {
            "name": tu.name,
            "arguments": arguments_string,
        },
    }))
}

fn translate_tool(spec: &ToolSpec) -> Result<Value, BuildError> {
    let parameters = serde_json::to_value(&spec.input_schema)?;
    Ok(serde_json::json!({
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": parameters,
        },
    }))
}

fn translate_tool_choice(tc: &ToolChoice) -> Option<Value> {
    match tc {
        ToolChoice::Auto => Some(Value::String("auto".into())),
        ToolChoice::None => Some(Value::String("none".into())),
        ToolChoice::Required => Some(Value::String("required".into())),
        ToolChoice::Specific { name } => Some(serde_json::json!({
            "type": "function",
            "function": { "name": name },
        })),
        _ => std::option::Option::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;
    use tau_domain::Value as DomainValue;

    fn req_with_user_text(text: &str) -> CompletionRequest {
        let mut req = CompletionRequest::new("gpt-4o-mini".into());
        req.messages = vec![LlmProviderMessage::User {
            content: vec![ContentBlock::Text(text.into())],
        }];
        req
    }

    #[test]
    fn happy_path_user_text_only() {
        let req = req_with_user_text("hello");
        let body = build_chat_completions_body(&req, false).unwrap();
        assert_eq!(body["model"], "gpt-4o-mini");
        assert_eq!(body["stream"], false);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "hello");
        assert!(body.get("tools").is_none());
        // tool_choice always emitted (Auto by default → "auto").
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn streaming_flag_propagates() {
        let req = req_with_user_text("hi");
        let body = build_chat_completions_body(&req, true).unwrap();
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn system_prompt_emitted_as_leading_role_system_message() {
        let mut req = req_with_user_text("hi");
        req.system = Some("you are concise".into());
        let body = build_chat_completions_body(&req, false).unwrap();
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "you are concise");
        assert_eq!(messages[1]["role"], "user");
        // No top-level system field.
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
        let body = build_chat_completions_body(&req, false).unwrap();
        assert_eq!(body["messages"][0]["content"], "part one part two");
    }

    #[test]
    fn assistant_tool_use_emits_tool_calls_with_real_id() {
        let mut input_obj = BTreeMap::new();
        input_obj.insert("text".into(), DomainValue::String("hi".into()));
        let tu = ToolUse::new(
            "call_abc123".into(),
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
        let body = build_chat_completions_body(&req, false).unwrap();
        let asst = &body["messages"][0];
        assert_eq!(asst["role"], "assistant");
        assert_eq!(asst["content"], "ok let me ");
        let calls = asst["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["id"], "call_abc123");
        assert_eq!(calls[0]["type"], "function");
        assert_eq!(calls[0]["function"]["name"], "echo");
    }

    #[test]
    fn assistant_tool_use_arguments_is_json_encoded_string() {
        // OpenAI wire format: `arguments` is a JSON-encoded STRING,
        // not a JSON object. Critical correctness.
        let mut input_obj = BTreeMap::new();
        input_obj.insert("text".into(), DomainValue::String("hi".into()));
        let tu = ToolUse::new(
            "call_x".into(),
            "echo".into(),
            DomainValue::Object(input_obj),
        );
        let mut req = req_with_user_text("ignored");
        req.messages = vec![LlmProviderMessage::Assistant {
            content: vec![ContentBlock::ToolUse(tu)],
        }];
        let body = build_chat_completions_body(&req, false).unwrap();
        let arguments = &body["messages"][0]["tool_calls"][0]["function"]["arguments"];
        // Must be a string, not an object.
        let s = arguments.as_str().expect("arguments is a string");
        // The string contents are valid JSON encoding the input.
        let parsed: serde_json::Value = serde_json::from_str(s).unwrap();
        assert_eq!(parsed["text"], "hi");
    }

    #[test]
    fn tool_result_message_round_trips_tool_use_id() {
        let mut req = req_with_user_text("ignored");
        req.messages = vec![LlmProviderMessage::ToolResult {
            tool_use_id: "call_xyz".into(),
            content: vec![ContentBlock::Text("42".into())],
            is_error: false,
        }];
        let body = build_chat_completions_body(&req, false).unwrap();
        let msg = &body["messages"][0];
        assert_eq!(msg["role"], "tool");
        // OpenAI rounds-trips the id (distinct from Ollama which drops it).
        assert_eq!(msg["tool_call_id"], "call_xyz");
        assert_eq!(msg["content"], "42");
    }

    #[test]
    fn tool_choice_auto_required_specific_round_trip() {
        for (variant, expected) in [
            (ToolChoice::Auto, json!("auto")),
            (ToolChoice::Required, json!("required")),
            (
                ToolChoice::Specific {
                    name: "echo".into(),
                },
                json!({"type":"function","function":{"name":"echo"}}),
            ),
        ] {
            let mut req = req_with_user_text("hi");
            req.tools = vec![tau_ports::fixtures::make_tool_spec(
                "echo".into(),
                "echo".into(),
                DomainValue::Object(Default::default()),
            )];
            req.tool_choice = variant;
            let body = build_chat_completions_body(&req, false).unwrap();
            assert_eq!(body["tool_choice"], expected);
            assert!(body.get("tools").is_some());
        }
    }

    #[test]
    fn tool_choice_none_omits_tools_array_entirely() {
        let mut req = req_with_user_text("hi");
        req.tools = vec![tau_ports::fixtures::make_tool_spec(
            "echo".into(),
            "".into(),
            DomainValue::Object(Default::default()),
        )];
        req.tool_choice = ToolChoice::None;
        let body = build_chat_completions_body(&req, false).unwrap();
        // ToolChoice::None drops `tools` even when present.
        assert!(body.get("tools").is_none());
        // tool_choice itself is still emitted as "none" (informative).
        assert_eq!(body["tool_choice"], "none");
    }

    #[test]
    fn sampling_overrides_top_level_no_options_subobject() {
        let mut req = req_with_user_text("hi");
        req.max_tokens = Some(100);
        req.temperature = Some(0.7);
        req.top_p = Some(0.9);
        req.seed = Some(42);
        req.stop_sequences = vec!["END".into()];
        let body = build_chat_completions_body(&req, false).unwrap();
        // Top-level fields, NOT under "options".
        assert!(body.get("options").is_none());
        assert_eq!(body["max_tokens"], 100);
        assert_eq!(body["temperature"], f64::from(0.7f32));
        assert_eq!(body["top_p"], f64::from(0.9f32));
        assert_eq!(body["seed"], 42);
        assert_eq!(body["stop"], json!(["END"]));
    }

    #[test]
    fn tools_emitted_with_function_wrapper() {
        let mut req = req_with_user_text("hi");
        req.tools = vec![tau_ports::fixtures::make_tool_spec(
            "echo".into(),
            "echo input".into(),
            DomainValue::Object(Default::default()),
        )];
        let body = build_chat_completions_body(&req, false).unwrap();
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "echo");
        assert_eq!(tools[0]["function"]["description"], "echo input");
        assert_eq!(tools[0]["function"]["parameters"], json!({}));
    }
}
