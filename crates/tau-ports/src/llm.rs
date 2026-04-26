//! LLM-backend port — `kind = "llm-backend"` plugin contracts.
//!
//! This module defines the [`LlmBackend`] trait, the [`CompletionStream`]
//! type alias, and the data types exchanged between tau-runtime and
//! plugin adapters. The `batch_to_stream` / `stream_to_batch` /
//! `ToolUseAccumulator` helpers land in T7.
//!
//! # Layer separation: `LlmProviderMessage` vs `tau_domain::Message`
//!
//! tau distinguishes the agent's **universal message envelope**
//! (`tau_domain::Message`, used for inbox/outbox routing between agents,
//! tools, and the runtime) from the **LLM-call shape**
//! ([`LlmProviderMessage`], used to construct a single completion
//! request to a provider).
//!
//! - `tau_domain::Message` carries an [`AgentInstanceId`] sender, an
//!   [`Address`] recipient, a payload, a timestamp, and a message id.
//!   It is the universal envelope that tau-runtime routes.
//! - [`LlmProviderMessage`] carries only the role
//!   (`User` / `Assistant` / `ToolResult`) and a list of
//!   [`ContentBlock`]s. It is the over-the-wire shape an `LlmBackend`
//!   plugin serialises into a provider-specific completion call.
//!
//! tau-runtime mediates between the two: it consumes
//! `tau_domain::Message`s from agent inboxes, projects them into a
//! `Vec<LlmProviderMessage>`, and hands the result to the
//! `LlmBackend`. Plugins never see `tau_domain::Message` directly.
//!
//! [`AgentInstanceId`]: tau_domain::AgentInstanceId
//! [`Address`]: tau_domain::Address

use std::collections::BTreeMap;
use std::pin::Pin;

use futures_core::Stream;
use tau_domain::Value;

use crate::error::LlmError;

/// Parameters for a single completion request to an `LlmBackend`.
///
/// `CompletionRequest` is `#[non_exhaustive]`: external callers cannot
/// construct it via struct-literal syntax. Construction is gated through
/// a builder (added alongside the trait in T6); fields are `pub` so
/// in-tree code and tests can pattern-match on the parameters.
///
/// # Example
///
/// ```ignore
/// // Struct-literal construction is forbidden externally because
/// // `CompletionRequest` is `#[non_exhaustive]`. The example here is
/// // illustrative; real callers use the builder added in T6.
/// use tau_ports::llm::{CompletionRequest, ToolChoice};
///
/// let req = CompletionRequest {
///     model: "claude-3-5-sonnet".into(),
///     system: Some("You are helpful.".into()),
///     messages: vec![],
///     tools: vec![],
///     max_tokens: Some(1024),
///     temperature: Some(0.7),
///     top_p: None,
///     seed: None,
///     tool_choice: ToolChoice::Auto,
///     stop_sequences: vec![],
///     provider_specific: Default::default(),
/// };
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    /// Model identifier (provider-specific; e.g. `"claude-3-5-sonnet"`).
    pub model: String,
    /// Optional system prompt prepended before `messages`.
    pub system: Option<String>,
    /// Conversation history as a sequence of provider-shaped messages.
    pub messages: Vec<LlmProviderMessage>,
    /// Tool specifications advertised to the model.
    pub tools: Vec<ToolSpec>,
    /// Maximum tokens to generate. `None` defers to the provider default.
    pub max_tokens: Option<u32>,
    /// Sampling temperature. `None` defers to the provider default.
    pub temperature: Option<f32>,
    /// Nucleus-sampling cutoff. `None` defers to the provider default.
    pub top_p: Option<f32>,
    /// Deterministic-sampling seed. `None` defers to the provider default.
    pub seed: Option<u64>,
    /// Tool-selection policy. Defaults to [`ToolChoice::Auto`].
    pub tool_choice: ToolChoice,
    /// Custom stop sequences. Empty defers to the provider default.
    pub stop_sequences: Vec<String>,
    /// Provider-specific parameters not yet typed in core (e.g. `top_k`,
    /// `presence_penalty`, `response_format`).
    ///
    /// This is a registered escape hatch. See:
    /// [escape-hatches.md#completionrequest-provider-specific](../docs/explanation/escape-hatches.md#completionrequest-provider-specific).
    pub provider_specific: BTreeMap<String, Value>,
}

/// Tool-selection policy for a [`CompletionRequest`].
///
/// Defaults to [`ToolChoice::Auto`]: the model decides whether to call
/// a tool.
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ToolChoice {
    /// Model decides whether to call a tool (default).
    #[default]
    Auto,
    /// Model must call at least one tool.
    Required,
    /// Model must not call any tool.
    None,
    /// Model must call the named tool.
    Specific {
        /// Name of the required tool. Must match a [`ToolSpec::name`]
        /// in [`CompletionRequest::tools`].
        name: String,
    },
}

/// One message in the LLM-call shape, distinct from the agent-runtime
/// `tau_domain::Message` envelope. See module-level docs for the layer
/// separation.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum LlmProviderMessage {
    /// User-authored message content.
    User {
        /// Multi-block content; v0.1 admits [`ContentBlock::Text`] and
        /// [`ContentBlock::ToolUse`] only.
        content: Vec<ContentBlock>,
    },
    /// Assistant-authored message content.
    Assistant {
        /// Multi-block content; v0.1 admits [`ContentBlock::Text`] and
        /// [`ContentBlock::ToolUse`] only.
        content: Vec<ContentBlock>,
    },
    /// Result of a previously-issued tool call, fed back to the model.
    ToolResult {
        /// Identifier matching the [`ToolUse::id`] of the call this
        /// result answers.
        tool_use_id: String,
        /// Multi-block content describing the tool result.
        content: Vec<ContentBlock>,
        /// Whether the tool reported an error.
        is_error: bool,
    },
}

/// One content block within an [`LlmProviderMessage`] or
/// [`CompletionResponse`].
///
/// v0.1 admits [`ContentBlock::Text`] and [`ContentBlock::ToolUse`]
/// only. The enum is `#[non_exhaustive]` to admit additive variants for
/// image, audio, document, and reasoning blocks without a major bump.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ContentBlock {
    /// Plain-text content.
    Text(String),
    /// A tool-use request from the assistant.
    ToolUse(ToolUse),
}

/// Batch (non-streaming) completion result.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    /// Concatenated assistant text. May be empty if the model only
    /// emitted tool-use blocks.
    pub text: String,
    /// Tool-use blocks emitted in order.
    pub tool_uses: Vec<ToolUse>,
    /// Why the response stopped.
    pub stop_reason: StopReason,
    /// Token-usage report. `None` if the provider did not return one.
    pub usage: Option<TokenUsage>,
}

/// One streamed event from a `CompletionStream`.
///
/// Plugin authors are responsible for buffering provider-specific
/// streaming representations into the shape below: `Text` deltas are
/// forwarded incrementally, `ToolUse` blocks are emitted only when
/// fully assembled, and exactly one terminal `Finish` is emitted at end
/// of stream. See `ToolUseAccumulator` (added in T7) for the
/// JSON-delta-buffering helper.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum CompletionChunk {
    /// Streamed text delta to append to the assistant response.
    Text {
        /// Text fragment to append.
        delta: String,
    },
    /// A tool-use block emitted once fully assembled by the plugin.
    ToolUse(ToolUse),
    /// Final marker. Emitted exactly once at end of stream.
    Finish {
        /// Why the stream ended.
        stop_reason: StopReason,
        /// Token-usage report. `None` if the provider did not return one.
        usage: Option<TokenUsage>,
    },
}

/// One tool-use request emitted by the model.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ToolUse {
    /// Provider-supplied identifier; round-tripped to
    /// [`LlmProviderMessage::ToolResult::tool_use_id`].
    pub id: String,
    /// Name of the tool the model wants to call. Matches a
    /// [`ToolSpec::name`] in the originating request.
    pub name: String,
    /// Arguments to the tool, as a `tau_domain::Value`.
    pub input: Value,
}

/// Specification of a tool the model may call.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// Tool name. Must be unique within a [`CompletionRequest::tools`].
    pub name: String,
    /// Human-readable description used by the model to decide when to
    /// invoke the tool.
    pub description: String,
    /// JSON Schema describing the tool's input. Round-trips through
    /// `tau_domain::Value`'s serde representation.
    pub input_schema: Value,
}

/// Reason a completion stopped.
///
/// `StopReason::Error` indicates the stream completed but reported an
/// error mid-flight; this is distinct from [`crate::LlmError`], which
/// indicates the trait method itself failed.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// Model finished naturally.
    EndTurn,
    /// Hit the `max_tokens` cap.
    MaxTokens,
    /// Matched one of the configured stop sequences.
    StopSequence,
    /// Model emitted a tool-use block and is awaiting its result.
    ToolUse,
    /// Stream completed but reported an error mid-flight (distinct
    /// from [`crate::LlmError`], which is a trait-method failure).
    Error,
}

/// Token-usage report for a completion.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TokenUsage {
    /// Tokens consumed from the input (system + messages + tools).
    pub input_tokens: u32,
    /// Tokens generated in the response.
    pub output_tokens: u32,
}

/// Boxed dyn-stream type at the runtime registry boundary. Returned from
/// [`LlmBackend::stream`].
pub type CompletionStream = Pin<Box<dyn Stream<Item = Result<CompletionChunk, LlmError>> + Send>>;

/// Trait implemented by `kind = "llm-backend"` plugins.
///
/// Native `async fn in trait` (Rust 1.75+; tau MSRV is 1.91). Both
/// `complete` and `stream` are required to avoid mutual-recursion
/// footguns; helpers in [the helpers section] make the inverse
/// implementation a one-liner for plugin authors who only have one
/// path natively.
///
/// `Send + Sync` so the runtime can store impls in a multi-task plugin
/// registry.
///
/// The `async_fn_in_trait` lint is suppressed: tau-ports intentionally
/// uses native `async fn in trait` (no `async-trait` macro, no boxed
/// future per call). tau-runtime boxes once at the dyn-cast boundary
/// where it stores `Arc<dyn LlmBackend>` in the plugin registry. See
/// spec §3.1 design call "Native `async fn in trait`" and ADR-0003.
#[allow(async_fn_in_trait)]
pub trait LlmBackend: Send + Sync {
    /// Plugin-visible name (matches the package name; for diagnostics).
    fn name(&self) -> &str;

    /// Make a batch completion request.
    ///
    /// Plugin authors with batch-only SDKs implement natively.
    /// Plugin authors with streaming SDKs call
    /// `stream_to_batch(self.stream(req).await?)` (helper in Task 7).
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError>;

    /// Make a streaming completion request.
    ///
    /// Plugin authors with streaming SDKs implement natively.
    /// Plugin authors with batch-only SDKs call
    /// `Ok(batch_to_stream(self.complete(req).await?))` (helper in Task 7).
    async fn stream(&self, req: CompletionRequest) -> Result<CompletionStream, LlmError>;
}
