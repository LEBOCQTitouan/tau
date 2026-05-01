//! Markdown rendering for `tau session show`.
//!
//! Pure function: takes a parsed session (header + entries) and
//! returns a markdown string suitable for `print!` to stdout. The
//! caller (cmd/session/show.rs in Task 8) decides whether to pipe
//! through termimad for ANSI rendering or to emit raw markdown
//! (e.g., `tau session export --format md`).

#![allow(dead_code)]

use tau_domain::{Address, Message, MessagePayload};

use super::store::{SessionEntry, SessionHeader};

const BODY_PREVIEW_CHARS: usize = 200;

/// Render a session as markdown.
///
/// Header summary at the top, `---` separator, then each message in
/// order. TurnSummary entries are counted into the header's
/// "Turns:" total but not rendered inline.
pub fn render_session(header: &SessionHeader, entries: &[SessionEntry]) -> String {
    let mut out = String::new();

    let turn_count = entries
        .iter()
        .filter(|e| matches!(e, SessionEntry::TurnSummary { .. }))
        .count();

    let created_at = humantime::format_rfc3339_seconds(header.created_at).to_string();

    out.push_str(&format!("# Session {}\n", header.id));
    out.push_str(&format!(
        "**Agent:** {} ({}@{})\n",
        header.agent_id, header.package.name, header.package.version
    ));
    out.push_str(&format!("**Started:** {}\n", created_at));
    out.push_str(&format!("**Turns:** {}\n", turn_count));
    out.push_str("\n---\n\n");

    for entry in entries {
        match entry {
            SessionEntry::Message(msg) => {
                if let Some(rendered) = format_message(&header.agent_id, msg) {
                    out.push_str(&rendered);
                    out.push_str("\n\n");
                }
            }
            SessionEntry::TurnSummary { .. } => {
                // TurnSummary lines are metadata; not rendered inline at v0.1.
            }
        }
    }

    out
}

/// Render a single Message. Returns None to skip (e.g., Lifecycle).
fn format_message(agent_id: &str, msg: &Message) -> Option<String> {
    match (&msg.sender, &msg.payload) {
        (Address::User, MessagePayload::Text { content }) => Some(format!("**You:** {}", content)),
        (Address::Agent(_), MessagePayload::Text { content }) => {
            Some(format!("**{}:** {}", agent_id, content))
        }
        (Address::Agent(_), MessagePayload::ToolCall { args }) => {
            let tool_name = match &msg.recipient {
                Address::Tool(name) => name.as_str(),
                _ => "?",
            };
            let args_json = serde_json::to_string(args).unwrap_or_else(|_| "?".to_string());
            Some(format!(
                "**{}:** [calls {} with {}]",
                agent_id, tool_name, args_json
            ))
        }
        (Address::Tool(name), MessagePayload::ToolResult { body }) => {
            let body_str = serde_json::to_string(body).unwrap_or_else(|_| "?".to_string());
            let preview = if body_str.chars().count() > BODY_PREVIEW_CHARS {
                let truncated: String = body_str.chars().take(BODY_PREVIEW_CHARS).collect();
                format!("{}...", truncated)
            } else {
                body_str
            };
            Some(format!("**{}:** [returned] {}", name, preview))
        }
        (Address::Tool(name), MessagePayload::ToolError { message, .. }) => {
            Some(format!("**{}:** [error: {}]", name, message))
        }
        (_, MessagePayload::Lifecycle(_)) => None,
        (_, MessagePayload::Custom { kind, .. }) => Some(format!("*[custom: {}]*", kind)),
        (Address::System, payload) => Some(format!("*[system: {:?}]*", payload)),
        (sender, payload) => {
            // Defensive fallback for unexpected combinations.
            Some(format!("*[{:?} → {:?}]*", sender, payload))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::id::mint;
    use crate::session::store::SessionPackage;
    use std::time::UNIX_EPOCH;
    use tau_domain::{AgentInstanceId, Value};

    fn fixture_header() -> SessionHeader {
        let id = mint();
        let mut header = SessionHeader::new(
            &id,
            "coder".to_string(),
            SessionPackage {
                name: "my-coder-agent".to_string(),
                version: "1.0.0".to_string(),
                resolved_commit: "0".repeat(40),
            },
            "anthropic".to_string(),
        );
        // Pin a deterministic created_at for snapshot stability.
        // 2024-05-01T13:13:21Z (arbitrary, deterministic)
        header.created_at = UNIX_EPOCH + std::time::Duration::from_secs(1_714_566_801);
        header
    }

    fn user_text(text: &str) -> Message {
        Message::new(
            Address::User,
            Address::User,
            MessagePayload::Text {
                content: text.to_string(),
            },
        )
    }

    fn agent_text(text: &str) -> Message {
        Message::new(
            Address::Agent(AgentInstanceId::new()),
            Address::User,
            MessagePayload::Text {
                content: text.to_string(),
            },
        )
    }

    fn agent_tool_call(tool: &str, args: Value) -> Message {
        Message::new(
            Address::Agent(AgentInstanceId::new()),
            Address::Tool(tool.to_string()),
            MessagePayload::ToolCall { args },
        )
    }

    fn tool_result(tool: &str, body: Value) -> Message {
        Message::new(
            Address::Tool(tool.to_string()),
            Address::User,
            MessagePayload::ToolResult { body },
        )
    }

    #[test]
    fn render_text_only_session() {
        let header = fixture_header();
        let entries = vec![
            SessionEntry::Message(user_text("Hello")),
            SessionEntry::Message(agent_text("Hi! How can I help?")),
        ];
        let out = render_session(&header, &entries);
        assert!(out.contains("# Session"));
        assert!(out.contains("**Agent:** coder (my-coder-agent@1.0.0)"));
        assert!(out.contains("**Turns:** 0"));
        assert!(out.contains("**You:** Hello"));
        assert!(out.contains("**coder:** Hi! How can I help?"));
    }

    #[test]
    fn render_with_tool_calls() {
        use std::collections::BTreeMap;
        let header = fixture_header();
        let mut args_map = BTreeMap::new();
        args_map.insert("path".to_string(), Value::String("foo.txt".to_string()));
        let entries = vec![
            SessionEntry::Message(user_text("Read foo.txt")),
            SessionEntry::Message(agent_tool_call("fs-read", Value::Object(args_map))),
            SessionEntry::Message(tool_result(
                "fs-read",
                Value::String("file contents here".to_string()),
            )),
            SessionEntry::Message(agent_text("Done")),
        ];
        let out = render_session(&header, &entries);
        assert!(out.contains("**You:** Read foo.txt"));
        assert!(out.contains("**coder:** [calls fs-read with"));
        assert!(out.contains("**fs-read:** [returned]"));
        assert!(out.contains("**coder:** Done"));
    }

    #[test]
    fn render_counts_turn_summaries_in_header() {
        let header = fixture_header();
        let entries = vec![
            SessionEntry::Message(user_text("Hello")),
            SessionEntry::Message(agent_text("Hi")),
            SessionEntry::TurnSummary {
                turn: 1,
                stop_reason: "EndTurn".to_string(),
                input_tokens: Some(10),
                output_tokens: Some(5),
            },
            SessionEntry::Message(user_text("Tell me more")),
            SessionEntry::Message(agent_text("Sure")),
            SessionEntry::TurnSummary {
                turn: 2,
                stop_reason: "EndTurn".to_string(),
                input_tokens: Some(15),
                output_tokens: Some(8),
            },
        ];
        let out = render_session(&header, &entries);
        assert!(out.contains("**Turns:** 2"));
        // TurnSummary lines NOT rendered inline (v0.1 design).
        assert!(!out.contains("Turn 1:"));
    }
}
