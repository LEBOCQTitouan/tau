//! Handshake and shutdown payload types for the plugin protocol.
//!
//! See `docs/superpowers/specs/2026-04-28-plugin-loading-design.md`
//! §4.4 (handshake) and §4.8 (shutdown).
//!
//! These are the typed payloads exchanged via the protocol-level
//! `meta.handshake`, `meta.shutdown`, and `meta.describe` methods.
//! They serialize through `rmp-serde` when used as the `params[0]` of
//! a [`crate::Frame::Request`] (host→plugin) or as the `result` of the
//! corresponding [`crate::Frame::Response`] (plugin→host).
//!
//! All payload types are `#[non_exhaustive]` and expose explicit
//! `::new(...)` constructors so future revisions can add fields without
//! breaking callers (struct-literal construction is blocked across
//! crate boundaries by E0639).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tau_domain::PortKind;

/// Trace context propagated from host to plugin so plugin tracing
/// events tag the right run / agent / root span.
///
/// Carried inside [`HandshakeRequest`] so the plugin SDK can install a
/// tracing subscriber that injects these IDs as fields on every event.
///
/// # Example
///
/// ```ignore
/// // `TraceContext` is `#[non_exhaustive]`; cannot be constructed via
/// // struct-literal across crate boundaries (E0639). Use
/// // `TraceContext::new(...)` instead.
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceContext {
    /// Run id (typically a ULID) — identifies the host run that
    /// spawned this plugin invocation.
    pub run_id: String,
    /// Agent id — identifies the logical agent within the run.
    pub agent_id: String,
    /// Root span id — the parent span under which plugin events nest.
    pub root_span_id: String,
}

impl TraceContext {
    /// Construct a [`TraceContext`].
    pub fn new(run_id: String, agent_id: String, root_span_id: String) -> Self {
        Self {
            run_id,
            agent_id,
            root_span_id,
        }
    }
}

/// Host's `meta.handshake` request payload (the single element of the
/// request frame's `params` array).
///
/// # Example
///
/// ```ignore
/// // `HandshakeRequest` is `#[non_exhaustive]`; use
/// // `HandshakeRequest::new(...)` instead of struct-literal syntax
/// // across crate boundaries (E0639).
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandshakeRequest {
    /// Protocol version string the host speaks (currently
    /// [`PROTOCOL_VERSION`]).
    pub protocol_version: String,
    /// The port the host expects this plugin to provide.
    pub port: PortKind,
    /// Trace context for the host run requesting the plugin.
    pub trace_context: TraceContext,
    /// Free-form per-plugin configuration drawn from the host's
    /// configuration system. Defaults to `null` when absent.
    #[serde(default)]
    pub config: serde_json::Value,
}

impl HandshakeRequest {
    /// Construct a [`HandshakeRequest`].
    pub fn new(
        protocol_version: String,
        port: PortKind,
        trace_context: TraceContext,
        config: serde_json::Value,
    ) -> Self {
        Self {
            protocol_version,
            port,
            trace_context,
            config,
        }
    }
}

/// JSON Schema (or rmpv-typed schema) for one method's params and
/// result, as advertised in [`HandshakeResponse::schemas`].
///
/// # Example
///
/// ```ignore
/// // `MethodSchema` is `#[non_exhaustive]`; use `MethodSchema::new(...)`
/// // instead of struct-literal syntax across crate boundaries (E0639).
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MethodSchema {
    /// Schema for the method's `params` payload.
    pub params: serde_json::Value,
    /// Schema for the method's `result` payload.
    pub result: serde_json::Value,
}

impl MethodSchema {
    /// Construct a [`MethodSchema`].
    pub fn new(params: serde_json::Value, result: serde_json::Value) -> Self {
        Self { params, result }
    }
}

/// Plugin's `meta.handshake` response payload (the `result` field of
/// the response frame).
///
/// # Example
///
/// ```ignore
/// // `HandshakeResponse` is `#[non_exhaustive]`; use
/// // `HandshakeResponse::new(...)` instead of struct-literal syntax
/// // across crate boundaries (E0639).
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandshakeResponse {
    /// Protocol version string the plugin speaks.
    pub protocol_version: String,
    /// The port the plugin provides.
    pub provides: PortKind,
    /// Plugin name (typically the manifest `name`).
    pub plugin_name: String,
    /// Plugin version (typically the manifest `version`).
    pub plugin_version: String,
    /// Names of the methods the plugin handles, in addition to the
    /// `meta.*` protocol-level methods.
    pub methods: Vec<String>,
    /// Per-method schemas keyed by method name.
    pub schemas: BTreeMap<String, MethodSchema>,
}

impl HandshakeResponse {
    /// Construct a [`HandshakeResponse`].
    pub fn new(
        protocol_version: String,
        provides: PortKind,
        plugin_name: String,
        plugin_version: String,
        methods: Vec<String>,
        schemas: BTreeMap<String, MethodSchema>,
    ) -> Self {
        Self {
            protocol_version,
            provides,
            plugin_name,
            plugin_version,
            methods,
            schemas,
        }
    }
}

/// Method-name constants for the protocol-level `meta.*` methods.
pub mod meta {
    /// `meta.handshake` — host-initiated request, plugin responds.
    pub const HANDSHAKE_METHOD: &str = "meta.handshake";
    /// `meta.shutdown` — host-sent notification on host exit.
    pub const SHUTDOWN_METHOD: &str = "meta.shutdown";
    /// `meta.describe` — host-sent request, plugin returns method
    /// schemas (typically the same shape as
    /// [`super::HandshakeResponse::schemas`]).
    pub const DESCRIBE_METHOD: &str = "meta.describe";
}

/// Standard protocol version string (`"1"`).
pub const PROTOCOL_VERSION: &str = "1";
