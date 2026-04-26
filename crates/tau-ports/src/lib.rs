#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Port (trait) definitions for tau's hexagonal architecture. Adapters in
//! tau-infra implement these traits.
//!
//! tau-ports defines four trait families:
//!
//! - [`llm::LlmBackend`] — LLM provider plugins (`kind = "llm-backend"`).
//! - `tool::Tool` — tool plugins (`kind = "tool"`).
//! - `storage::Storage` — storage plugins (`kind = "storage"`).
//! - `sandbox::Sandbox` — sandbox plugins (`kind = "sandbox"`); see the
//!   module docs for the v0.1 PROVISIONAL caveat.
//!
//! See `docs/decisions/0003-tau-ports.md` for the design rationale.

pub mod error;
pub mod llm;
pub mod storage;

pub use error::{KeyError, LlmError, NamespaceError, SandboxError, StorageError, ToolError};
pub use llm::{
    CompletionChunk, CompletionRequest, CompletionResponse, CompletionStream, ContentBlock,
    LlmBackend, LlmProviderMessage, StopReason, TokenUsage, ToolChoice, ToolSpec, ToolUse,
};
pub use storage::{Key, Namespace};
