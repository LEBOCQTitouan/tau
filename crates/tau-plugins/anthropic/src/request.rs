//! Build the JSON body for `POST /v1/messages` from a tau-ports
//! `CompletionRequest`.
//!
//! Per spec §4.2 / §7.1 / §7.2:
//! - `req.system: Option<String>` maps to Anthropic's top-level `system`.
//! - Each `LlmProviderMessage` translates per its variant.
//! - `req.tools: Vec<ToolSpec>` maps to Anthropic's `tools[*]`.
//! - `req.tool_choice` maps per the variant table.
//! - `req.provider_specific` is ignored in v0.1 with a debug-level log.

use serde::ser::Error as _;
use serde_json::Value as JsonValue;
use tau_ports::{CompletionRequest, ContentBlock, LlmProviderMessage, ToolChoice, ToolSpec};
use thiserror::Error;

/// Errors produced while building an Anthropic request body. Plugin-internal;
/// converted to [`tau_ports::LlmError::Internal`] in `plugin.rs`.
#[non_exhaustive]
#[derive(Debug, Error)]
pub(crate) enum BuildError {
    /// Failed to serialize a `tau_domain::Value` field (e.g.
    /// `ToolSpec::input_schema` or `ToolUse::input`) to JSON.
    #[error("serialize input value: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Build the Anthropic `POST /v1/messages` body from a tau-ports
/// `CompletionRequest`. The `stream` flag adds `"stream": true` for the
/// streaming endpoint.
pub(crate) fn build_messages_body(
    req: &CompletionRequest,
    stream: bool,
) -> Result<JsonValue, BuildError> {
    if !req.provider_specific.is_empty() {
        let keys: Vec<&str> = req.provider_specific.keys().map(String::as_str).collect();
        tracing::debug!(
            target: "anthropic_plugin::request",
            keys = ?keys,
            "ignoring provider_specific keys (v0.1 doesn't pass-through)"
        );
    }

    let mut body = serde_json::json!({
        "model": req.model,
        "messages": req.messages.iter()
            .map(translate_message)
            .collect::<Result<Vec<_>, BuildError>>()?,
        "max_tokens": req.max_tokens.unwrap_or(4096),
    });

    if let Some(system) = &req.system {
        body["system"] = JsonValue::String(system.clone());
    }

    // Tools array + tool_choice: omit BOTH when ToolChoice::None per
    // spec plan-erratum (tau-ports doc says "Model must not call any tool";
    // Anthropic enforces this by not advertising the tools).
    if !matches!(req.tool_choice, ToolChoice::None) && !req.tools.is_empty() {
        body["tools"] = JsonValue::Array(
            req.tools
                .iter()
                .map(translate_tool)
                .collect::<Result<Vec<_>, BuildError>>()?,
        );
        body["tool_choice"] = translate_tool_choice(&req.tool_choice);
    }

    if let Some(t) = req.temperature {
        body["temperature"] = serde_json::json!(t);
    }
    if let Some(p) = req.top_p {
        body["top_p"] = serde_json::json!(p);
    }
    if !req.stop_sequences.is_empty() {
        body["stop_sequences"] = serde_json::json!(req.stop_sequences);
    }

    if stream {
        body["stream"] = JsonValue::Bool(true);
    }

    Ok(body)
}

fn translate_message(msg: &LlmProviderMessage) -> Result<JsonValue, BuildError> {
    match msg {
        LlmProviderMessage::User { content } => Ok(serde_json::json!({
            "role": "user",
            "content": content.iter()
                .map(translate_content_block)
                .collect::<Result<Vec<_>, BuildError>>()?,
        })),
        LlmProviderMessage::Assistant { content } => Ok(serde_json::json!({
            "role": "assistant",
            "content": content.iter()
                .map(translate_content_block)
                .collect::<Result<Vec<_>, BuildError>>()?,
        })),
        LlmProviderMessage::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            // Anthropic's tool_result content can be a string OR an array.
            // For v0.1 we always emit an array of {type: "text", text: ...}
            // entries (matching the inner ContentBlock vec).
            let content_array: Vec<JsonValue> = content
                .iter()
                .map(translate_content_block)
                .collect::<Result<Vec<_>, BuildError>>()?;
            Ok(serde_json::json!({
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": content_array,
                    "is_error": is_error,
                }],
            }))
        }
        // `LlmProviderMessage` is `#[non_exhaustive]`. The match above
        // covers all variants known at v0.1; this fallback exists only to
        // remain forward-compatible if tau-ports adds new variants later.
        _ => Err(BuildError::Serialize(serde_json::Error::custom(
            "unknown LlmProviderMessage variant",
        ))),
    }
}

fn translate_content_block(block: &ContentBlock) -> Result<JsonValue, BuildError> {
    match block {
        ContentBlock::Text(text) => Ok(serde_json::json!({
            "type": "text",
            "text": text,
        })),
        ContentBlock::ToolUse(tu) => Ok(serde_json::json!({
            "type": "tool_use",
            "id": tu.id,
            "name": tu.name,
            "input": serde_json::to_value(&tu.input)?,
        })),
        // `ContentBlock` is `#[non_exhaustive]`. Same forward-compat
        // rationale as `translate_message`.
        _ => Err(BuildError::Serialize(serde_json::Error::custom(
            "unknown ContentBlock variant",
        ))),
    }
}

fn translate_tool(spec: &ToolSpec) -> Result<JsonValue, BuildError> {
    Ok(serde_json::json!({
        "name": spec.name,
        "description": spec.description,
        "input_schema": serde_json::to_value(&spec.input_schema)?,
    }))
}

fn translate_tool_choice(choice: &ToolChoice) -> JsonValue {
    match choice {
        ToolChoice::Auto => serde_json::json!({"type": "auto"}),
        ToolChoice::Required => serde_json::json!({"type": "any"}),
        ToolChoice::Specific { name } => serde_json::json!({"type": "tool", "name": name}),
        // ToolChoice::None handled in build_messages_body (omitted there).
        ToolChoice::None => JsonValue::Null,
        // `ToolChoice` is `#[non_exhaustive]`; future variants degrade
        // gracefully to "no tool_choice override" rather than failing.
        _ => JsonValue::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tau_domain::Value;
    use tau_ports::fixtures::make_tool_spec;
    use tau_ports::ToolUse;

    fn user_text(text: &str) -> LlmProviderMessage {
        LlmProviderMessage::user(vec![ContentBlock::Text(text.into())])
    }

    fn sample_request(model: &str) -> CompletionRequest {
        let mut req = CompletionRequest::new(model.into());
        req.messages.push(user_text("hi"));
        req
    }

    fn echo_tool() -> ToolSpec {
        make_tool_spec(
            "echo".into(),
            "echoes".into(),
            Value::Object(Default::default()),
        )
    }

    fn unwrap_obj(v: &JsonValue) -> &serde_json::Map<String, JsonValue> {
        v.as_object().expect("expected object")
    }

    #[test]
    fn builds_minimal_body() {
        let req = sample_request("claude-3-5-haiku-latest");
        let body = build_messages_body(&req, false).unwrap();
        let obj = unwrap_obj(&body);
        assert_eq!(obj.get("model").unwrap(), "claude-3-5-haiku-latest");
        assert_eq!(obj.get("max_tokens").unwrap(), 4096);
        assert!(obj.get("system").is_none());
        assert!(obj.get("tools").is_none());
        assert!(obj.get("tool_choice").is_none());
        assert!(obj.get("stream").is_none());
    }

    #[test]
    fn omits_system_when_none() {
        let req = sample_request("m");
        let body = build_messages_body(&req, false).unwrap();
        assert!(unwrap_obj(&body).get("system").is_none());
    }

    #[test]
    fn includes_system_when_some() {
        let mut req = sample_request("m");
        req.system = Some("you are concise".into());
        let body = build_messages_body(&req, false).unwrap();
        assert_eq!(unwrap_obj(&body).get("system").unwrap(), "you are concise",);
    }

    #[test]
    fn omits_tools_array_when_empty() {
        let req = sample_request("m");
        let body = build_messages_body(&req, false).unwrap();
        let obj = unwrap_obj(&body);
        assert!(obj.get("tools").is_none());
        assert!(obj.get("tool_choice").is_none());
    }

    #[test]
    fn omits_tools_array_when_tool_choice_is_none() {
        let mut req = sample_request("m");
        req.tools.push(echo_tool());
        req.tool_choice = ToolChoice::None;
        let body = build_messages_body(&req, false).unwrap();
        let obj = unwrap_obj(&body);
        assert!(
            obj.get("tools").is_none(),
            "tools omitted when ToolChoice::None"
        );
        assert!(obj.get("tool_choice").is_none());
    }

    #[test]
    fn tool_choice_auto_round_trips() {
        let mut req = sample_request("m");
        req.tools.push(echo_tool());
        // ToolChoice::Auto is the default; no override needed.
        let body = build_messages_body(&req, false).unwrap();
        assert_eq!(
            unwrap_obj(&body).get("tool_choice").unwrap(),
            &json!({"type": "auto"}),
        );
    }

    #[test]
    fn tool_choice_required_maps_to_any() {
        let mut req = sample_request("m");
        req.tools.push(echo_tool());
        req.tool_choice = ToolChoice::Required;
        let body = build_messages_body(&req, false).unwrap();
        assert_eq!(
            unwrap_obj(&body).get("tool_choice").unwrap(),
            &json!({"type": "any"}),
        );
    }

    #[test]
    fn tool_choice_specific_includes_name() {
        let mut req = sample_request("m");
        req.tools.push(echo_tool());
        req.tool_choice = ToolChoice::Specific {
            name: "echo".into(),
        };
        let body = build_messages_body(&req, false).unwrap();
        assert_eq!(
            unwrap_obj(&body).get("tool_choice").unwrap(),
            &json!({"type": "tool", "name": "echo"}),
        );
    }

    #[test]
    fn translates_user_message_text_block() {
        let req = sample_request("m");
        let body = build_messages_body(&req, false).unwrap();
        let messages = unwrap_obj(&body)
            .get("messages")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(messages.len(), 1);
        let msg = messages[0].as_object().unwrap();
        assert_eq!(msg.get("role").unwrap(), "user");
        let content = msg.get("content").unwrap().as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0].get("type").unwrap(), "text");
        assert_eq!(content[0].get("text").unwrap(), "hi");
    }

    #[test]
    fn translates_assistant_message_with_tool_use_block() {
        let mut req = CompletionRequest::new("m".into());
        req.messages.push(LlmProviderMessage::assistant(vec![
            ContentBlock::Text("calling tool".into()),
            ContentBlock::ToolUse(ToolUse::new(
                "tu_01".into(),
                "echo".into(),
                Value::Object(Default::default()),
            )),
        ]));
        let body = build_messages_body(&req, false).unwrap();
        let messages = unwrap_obj(&body)
            .get("messages")
            .unwrap()
            .as_array()
            .unwrap();
        let content = messages[0].get("content").unwrap().as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0].get("type").unwrap(), "text");
        assert_eq!(content[1].get("type").unwrap(), "tool_use");
        assert_eq!(content[1].get("id").unwrap(), "tu_01");
        assert_eq!(content[1].get("name").unwrap(), "echo");
    }

    #[test]
    fn translates_tool_result_message() {
        let mut req = CompletionRequest::new("m".into());
        req.messages.push(LlmProviderMessage::tool_result(
            "tu_01".into(),
            vec![ContentBlock::Text("result".into())],
            false,
        ));
        let body = build_messages_body(&req, false).unwrap();
        let messages = unwrap_obj(&body)
            .get("messages")
            .unwrap()
            .as_array()
            .unwrap();
        let msg = messages[0].as_object().unwrap();
        assert_eq!(msg.get("role").unwrap(), "user");
        let content = msg.get("content").unwrap().as_array().unwrap();
        assert_eq!(content[0].get("type").unwrap(), "tool_result");
        assert_eq!(content[0].get("tool_use_id").unwrap(), "tu_01");
        assert_eq!(content[0].get("is_error").unwrap(), false);
    }

    #[test]
    fn passes_through_sampling_overrides() {
        let mut req = sample_request("m");
        req.temperature = Some(0.7);
        req.top_p = Some(0.9);
        req.stop_sequences = vec!["\n\n".into()];
        let body = build_messages_body(&req, false).unwrap();
        let obj = unwrap_obj(&body);
        // f32 round-trip via serde_json widens to f64; assert with a
        // tolerance instead of bit-exact equality.
        let temp = obj.get("temperature").unwrap().as_f64().unwrap();
        assert!((temp - 0.7).abs() < 1e-6, "temperature was {temp}");
        let top_p = obj.get("top_p").unwrap().as_f64().unwrap();
        assert!((top_p - 0.9).abs() < 1e-6, "top_p was {top_p}");
        let stops = obj.get("stop_sequences").unwrap().as_array().unwrap();
        assert_eq!(stops, &vec![json!("\n\n")]);
    }

    #[test]
    fn sets_stream_true_when_requested() {
        let req = sample_request("m");
        let body = build_messages_body(&req, true).unwrap();
        assert_eq!(unwrap_obj(&body).get("stream").unwrap(), true);
    }

    #[test]
    fn default_max_tokens_is_4096() {
        let req = sample_request("m");
        let body = build_messages_body(&req, false).unwrap();
        assert_eq!(unwrap_obj(&body).get("max_tokens").unwrap(), 4096);
    }

    #[test]
    fn explicit_max_tokens_overrides_default() {
        let mut req = sample_request("m");
        req.max_tokens = Some(20);
        let body = build_messages_body(&req, false).unwrap();
        assert_eq!(unwrap_obj(&body).get("max_tokens").unwrap(), 20);
    }
}
