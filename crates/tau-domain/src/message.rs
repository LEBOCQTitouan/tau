//! Message envelope, addressing, and payload types (G5).

use std::collections::BTreeMap;
use std::time::SystemTime;

use crate::agent::AgentStatus;
use crate::id::{AgentInstanceId, MessageId};
use crate::value::Value;

/// Sender or recipient of a [`Message`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Address {
    /// A specific agent instance.
    Agent(AgentInstanceId),
    /// A named tool. The runtime resolves name → plugin via its
    /// registration table.
    Tool(String),
    /// A human user (e.g. the operator at the CLI).
    User,
    /// The runtime / observer.
    System,
}

/// Message body. Typed variants for known shapes; `Custom` for
/// plugin-specific.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MessagePayload {
    /// Human- or agent-authored text. The envelope's `sender` field
    /// distinguishes origin.
    Text {
        /// Message text.
        content: String,
    },
    /// A tool invocation. The envelope's `recipient: Address::Tool(...)`
    /// names the tool; this carries the arguments.
    ToolCall {
        /// Arguments to pass to the tool.
        args: Value,
    },
    /// Successful tool result.
    ToolResult {
        /// Tool's response body.
        body: Value,
    },
    /// Tool returned an error.
    ToolError {
        /// Error kind (free-form string convention).
        kind: String,
        /// Human-readable error message.
        message: String,
        /// Optional structured detail.
        details: Option<Value>,
    },
    /// Lifecycle event broadcast (System → observers).
    Lifecycle(AgentStatus),
    /// Plugin-specific message kind.
    /// See: [escape-hatches.md#messagepayload-custom](../docs/explanation/escape-hatches.md#messagepayload-custom).
    Custom {
        /// Plugin-specific kind tag (e.g. `"mcp.resource.request"`).
        kind: String,
        /// Plugin-specific body bytes.
        body: Vec<u8>,
    },
}

/// A message envelope (G5).
///
/// # Example
///
/// ```ignore
/// // E0639: `#[non_exhaustive]` blocks struct-literal construction from
/// // outside the crate. Internal callers (and the unit test in this
/// // module) construct `Message { .. }` directly.
/// use tau_domain::{Message, MessageId, Address, MessagePayload};
/// use std::time::SystemTime;
/// use std::collections::BTreeMap;
///
/// let m = Message {
///     id: MessageId::new(),
///     sender: Address::User,
///     recipient: Address::System,
///     parent_id: None,
///     created_at: SystemTime::UNIX_EPOCH,
///     headers: BTreeMap::new(),
///     payload: MessagePayload::Text { content: "hello".into() },
/// };
/// assert!(matches!(m.payload, MessagePayload::Text { .. }));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Message {
    /// Globally unique message identifier.
    pub id: MessageId,
    /// Originator.
    pub sender: Address,
    /// Destination.
    pub recipient: Address,
    /// Optional pointer to the message this one replies to.
    pub parent_id: Option<MessageId>,
    /// When the message was created.
    pub created_at: SystemTime,
    /// Free-form headers. `BTreeMap` for stable iteration order.
    pub headers: BTreeMap<String, String>,
    /// Message body.
    pub payload: MessagePayload,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_payload_holds_status() {
        let m = MessagePayload::Lifecycle(AgentStatus::Ready);
        assert!(matches!(m, MessagePayload::Lifecycle(AgentStatus::Ready)));
    }
}
