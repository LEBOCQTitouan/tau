//! Method name string constants for the v1 protocol.
//!
//! Per spec §5: 5 methods + 1 server-initiated notification.

/// Required first call. Establishes protocol version.
pub const META_HANDSHAKE: &str = "meta.handshake";

/// Liveness check.
pub const META_PING: &str = "meta.ping";

/// Batch run.
pub const RUNTIME_RUN: &str = "runtime.run";

/// Streaming run.
pub const RUNTIME_RUN_STREAMING: &str = "runtime.run_streaming";

/// Cancel an in-flight call by id.
pub const RUNTIME_CANCEL: &str = "runtime.cancel";

/// Server-initiated event during a streaming run.
pub const RUNTIME_EVENT: &str = "runtime.event";
