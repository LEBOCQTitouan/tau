//! JSON-RPC error codes used by serve mode.
//!
//! Standard JSON-RPC 2.0 codes plus tau-namespaced codes in the
//! "Server error" reserved range (-32000 to -32099) per spec §6.

// Standard JSON-RPC 2.0 codes.
/// Invalid JSON received on the wire.
pub const PARSE_ERROR: i32 = -32700;
/// Not a valid JSON-RPC 2.0 object.
pub const INVALID_REQUEST: i32 = -32600;
/// Method does not exist or is not available.
pub const METHOD_NOT_FOUND: i32 = -32601;
/// Invalid method parameter(s).
pub const INVALID_PARAMS: i32 = -32602;
/// Internal JSON-RPC error.
pub const INTERNAL_ERROR: i32 = -32603;

// Tau-namespaced (-32000..-32099).
/// Handshake `protocol_version` not supported.
pub const HANDSHAKE_MISMATCH: i32 = -32000;
/// Request was cancelled by client.
pub const CANCELLED: i32 = -32001;
/// Non-`meta.*` call before handshake completed.
pub const HANDSHAKE_REQUIRED: i32 = -32002;
/// `meta.handshake` called after a successful handshake.
pub const ALREADY_HANDSHAKEN: i32 = -32003;
/// `max_concurrent_runs` cap reached.
pub const SERVER_BUSY: i32 = -32004;
/// RuntimeBuilder build error (reserved for future `runtime.reload`).
pub const PROJECT_ERROR: i32 = -32005;
/// Generic `RuntimeError` not covered by a more specific code.
pub const RUNTIME_ERROR: i32 = -32006;
/// `RuntimeError::CapabilityDenied`.
pub const CAPABILITY_DENIED: i32 = -32007;
/// Tool plugin returned error.
pub const TOOL_ERROR: i32 = -32008;
/// LLM backend plugin returned error.
pub const LLM_ERROR: i32 = -32009;
/// `agent_id` not in this project.
pub const UNKNOWN_AGENT: i32 = -32010;
