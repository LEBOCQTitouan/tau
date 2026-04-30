//! Test fixtures for tau-ports. Gated behind the `test-fixtures` feature.
//!
//! Downstream crates depend via:
//!
//! ```toml
//! [dev-dependencies]
//! tau-ports = { workspace = true, features = ["test-fixtures"] }
//! ```
//!
//! Each mock implements its corresponding trait with configurable
//! canned responses and recorded invocations. Interior mutability is
//! provided by [`std::sync::Mutex`] so the mocks satisfy `Send + Sync`
//! and can be stored in the runtime's plugin registry.
//!
//! Mocks expose enough surface for tau-runtime, tau-pkg, and future
//! plugin-author tests to verify trait-driven behavior without spinning
//! up real LLM providers or database backends. Production builds
//! (without the feature) do not pull this code.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::SystemTime;

use tau_domain::{AgentInstanceId, Value};
use uuid::Uuid;

use crate::error::{LlmError, SandboxError, StorageError, ToolError};
use crate::llm::{
    batch_to_stream, CompletionChunk, CompletionRequest, CompletionResponse, CompletionStream,
    LlmBackend, LlmProviderMessage, StopReason, TokenUsage, ToolChoice, ToolSpec, ToolUse,
};
use crate::sandbox::{Sandbox, SandboxPlan};
use crate::storage::{Key, Namespace, Storage};
use crate::tool::{SessionContext, Tool, ToolContent, ToolResult};

// ---------------------------------------------------------------------------
// Construction helpers for `#[non_exhaustive]` types
// ---------------------------------------------------------------------------
//
// External integration tests cannot construct `#[non_exhaustive]` types via
// struct-literal syntax (E0639). These helpers live here so that downstream
// crates (and tau-ports' own integration tests) can build canonical
// `CompletionResponse`, `ToolUse`, and `TokenUsage` values when the
// `test-fixtures` feature is enabled.

/// Build a [`CompletionResponse`] without struct-literal syntax. Used by
/// integration tests that can't construct `#[non_exhaustive]` types.
pub fn make_completion_response(
    text: String,
    tool_uses: Vec<ToolUse>,
    stop_reason: StopReason,
    usage: Option<TokenUsage>,
) -> CompletionResponse {
    CompletionResponse {
        text,
        tool_uses,
        stop_reason,
        usage,
    }
}

/// Build a [`ToolUse`] without struct-literal syntax.
pub fn make_tool_use(id: String, name: String, input: Value) -> ToolUse {
    ToolUse { id, name, input }
}

/// Build a [`TokenUsage`] without struct-literal syntax.
pub fn make_token_usage(input_tokens: u32, output_tokens: u32) -> TokenUsage {
    TokenUsage {
        input_tokens,
        output_tokens,
    }
}

/// Build a minimal [`CompletionRequest`] without struct-literal syntax.
///
/// Optional fields default to `None` / empty / [`ToolChoice::Auto`].
/// Used by integration tests that need to feed canonical requests to a
/// [`MockLlmBackend`] or similar.
pub fn make_completion_request(model: String) -> CompletionRequest {
    CompletionRequest {
        model,
        system: None,
        messages: Vec::<LlmProviderMessage>::new(),
        tools: Vec::<ToolSpec>::new(),
        max_tokens: None,
        temperature: None,
        top_p: None,
        seed: None,
        tool_choice: ToolChoice::Auto,
        stop_sequences: Vec::new(),
        provider_specific: BTreeMap::new(),
    }
}

/// Build a [`ToolSpec`] without struct-literal syntax.
pub fn make_tool_spec(name: String, description: String, input_schema: Value) -> ToolSpec {
    ToolSpec {
        name,
        description,
        input_schema,
    }
}

/// Build a [`ToolResult`] without struct-literal syntax.
pub fn make_tool_result(content: Vec<ToolContent>, is_error: bool) -> ToolResult {
    ToolResult { content, is_error }
}

/// Build a [`SessionContext`] without struct-literal syntax. Provided
/// for tests in tau-runtime and elsewhere; tau-ports callers should
/// use [`SessionContext::new`] in production code.
///
/// `granted_capabilities` defaults to empty. To set a grant, use the
/// builder: `make_session_context(...).with_granted_capabilities(caps)`.
pub fn make_session_context(
    agent_instance_id: AgentInstanceId,
    session_id: Uuid,
    deadline: Option<SystemTime>,
) -> SessionContext {
    SessionContext::new(agent_instance_id, session_id, deadline)
}

// ---------------------------------------------------------------------------
// MockLlmBackend
// ---------------------------------------------------------------------------

/// Mock [`LlmBackend`] with configurable canned responses.
///
/// Records each [`LlmBackend::complete`] / [`LlmBackend::stream`] invocation
/// for later inspection via [`MockLlmBackend::invocations`]. Interior
/// mutability is provided by [`Mutex`] so the mock is `Send + Sync`.
///
/// # Configuration
///
/// - [`MockLlmBackend::with_response`] sets the canned [`CompletionResponse`]
///   returned by `complete()`. If unset, `complete()` returns an empty
///   default response (no text, no tool uses, [`StopReason::EndTurn`]).
/// - [`MockLlmBackend::with_chunks`] sets the canned chunks emitted by
///   `stream()`. If unset, `stream()` derives chunks from the canned
///   response via [`batch_to_stream`].
///
/// # Example
///
/// ```ignore
/// // Illustrative; depends on `#[non_exhaustive]` types so external
/// // doctests cannot construct them via struct-literal syntax.
/// use tau_ports::fixtures::MockLlmBackend;
///
/// let backend = MockLlmBackend::new("mock-llm");
/// // ... drive backend.complete(...) / backend.stream(...) ...
/// let recorded = backend.invocations();
/// assert!(recorded.is_empty()); // none yet
/// ```
pub struct MockLlmBackend {
    name: String,
    state: Mutex<MockLlmState>,
}

struct MockLlmState {
    response: Option<CompletionResponse>,
    chunks: Option<Vec<CompletionChunk>>,
    invocations: Vec<CompletionRequest>,
}

impl MockLlmBackend {
    /// Create a fresh mock with no canned responses configured.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            state: Mutex::new(MockLlmState {
                response: None,
                chunks: None,
                invocations: Vec::new(),
            }),
        }
    }

    /// Set the canned response returned by [`LlmBackend::complete`].
    ///
    /// Also seeds [`LlmBackend::stream`] when no chunks have been
    /// configured: `stream()` then derives chunks from this response
    /// via [`batch_to_stream`].
    pub fn with_response(self, resp: CompletionResponse) -> Self {
        {
            let mut state = self.state.lock().expect("MockLlmBackend mutex poisoned");
            state.response = Some(resp);
        }
        self
    }

    /// Set the canned chunks emitted by [`LlmBackend::stream`].
    pub fn with_chunks(self, chunks: Vec<CompletionChunk>) -> Self {
        {
            let mut state = self.state.lock().expect("MockLlmBackend mutex poisoned");
            state.chunks = Some(chunks);
        }
        self
    }

    /// Read recorded invocations in the order they were issued.
    pub fn invocations(&self) -> Vec<CompletionRequest> {
        self.state
            .lock()
            .expect("MockLlmBackend mutex poisoned")
            .invocations
            .clone()
    }
}

impl LlmBackend for MockLlmBackend {
    fn name(&self) -> &str {
        &self.name
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let mut state = self.state.lock().expect("MockLlmBackend mutex poisoned");
        state.invocations.push(req);
        Ok(state.response.clone().unwrap_or(CompletionResponse {
            text: String::new(),
            tool_uses: Vec::new(),
            stop_reason: StopReason::EndTurn,
            usage: None,
        }))
    }

    async fn stream(&self, req: CompletionRequest) -> Result<CompletionStream, LlmError> {
        let chunks = {
            let mut state = self.state.lock().expect("MockLlmBackend mutex poisoned");
            state.invocations.push(req);
            match state.chunks.clone() {
                Some(chunks) => chunks,
                None => {
                    // Fall back to batch_to_stream of the canned response.
                    let resp = state.response.clone().unwrap_or(CompletionResponse {
                        text: String::new(),
                        tool_uses: Vec::new(),
                        stop_reason: StopReason::EndTurn,
                        usage: None,
                    });
                    return Ok(batch_to_stream(resp));
                }
            }
        };
        Ok(Box::pin(VecChunkStream {
            items: chunks.into_iter(),
        }))
    }
}

/// Adapter from a `Vec<CompletionChunk>` to a [`CompletionStream`]. Used
/// by [`MockLlmBackend::stream`] when explicit chunks are configured.
struct VecChunkStream {
    items: std::vec::IntoIter<CompletionChunk>,
}

impl futures_core::Stream for VecChunkStream {
    type Item = Result<CompletionChunk, LlmError>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(self.get_mut().items.next().map(Ok))
    }
}

// ---------------------------------------------------------------------------
// MockTool
// ---------------------------------------------------------------------------

/// Mock [`Tool`] that records invocations and returns a canned
/// [`ToolResult`] (or [`ToolError`]).
///
/// `Session = ()`: the mock is stateless. Configure via
/// [`MockTool::with_result`] (default success path) or
/// [`MockTool::with_error`] (error path; takes precedence over a
/// configured result).
///
/// # Example
///
/// ```ignore
/// // Illustrative; depends on `#[non_exhaustive]` types so external
/// // doctests cannot construct them via struct-literal syntax.
/// use tau_ports::fixtures::MockTool;
/// use tau_ports::llm::ToolSpec;
/// use tau_domain::Value;
///
/// let spec = ToolSpec {
///     name: "echo".into(),
///     description: "echo".into(),
///     input_schema: Value::Object(Default::default()),
/// };
/// let tool = MockTool::new("echo", spec);
/// ```
pub struct MockTool {
    name: String,
    schema: ToolSpec,
    state: Mutex<MockToolState>,
}

struct MockToolState {
    result: Option<ToolResult>,
    error: Option<ToolError>,
    invocations: Vec<Value>,
}

impl MockTool {
    /// Create a fresh mock with no canned outcome configured. Calling
    /// `invoke` on an unconfigured mock returns a default success
    /// response with empty content.
    pub fn new(name: &str, schema: ToolSpec) -> Self {
        Self {
            name: name.to_string(),
            schema,
            state: Mutex::new(MockToolState {
                result: None,
                error: None,
                invocations: Vec::new(),
            }),
        }
    }

    /// Set the canned [`ToolResult`] returned by [`Tool::invoke`] when
    /// no error is configured.
    pub fn with_result(self, result: ToolResult) -> Self {
        {
            let mut state = self.state.lock().expect("MockTool mutex poisoned");
            state.result = Some(result);
        }
        self
    }

    /// Configure the mock to return `Err(error)` from [`Tool::invoke`].
    /// Takes precedence over a result configured via
    /// [`MockTool::with_result`].
    pub fn with_error(self, error: ToolError) -> Self {
        {
            let mut state = self.state.lock().expect("MockTool mutex poisoned");
            state.error = Some(error);
        }
        self
    }

    /// Read recorded invocation arguments in the order they were
    /// issued.
    pub fn invocations(&self) -> Vec<Value> {
        self.state
            .lock()
            .expect("MockTool mutex poisoned")
            .invocations
            .clone()
    }
}

impl Tool for MockTool {
    type Session = ();

    fn name(&self) -> &str {
        &self.name
    }

    fn schema(&self) -> ToolSpec {
        self.schema.clone()
    }

    async fn init(&self, _ctx: SessionContext) -> Result<Self::Session, ToolError> {
        Ok(())
    }

    async fn invoke(
        &self,
        _session: &mut Self::Session,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        let mut state = self.state.lock().expect("MockTool mutex poisoned");
        state.invocations.push(args);
        if let Some(error) = state.error.clone() {
            return Err(error);
        }
        Ok(state.result.clone().unwrap_or(ToolResult {
            content: Vec::new(),
            is_error: false,
        }))
    }

    async fn teardown(&self, _session: Self::Session) -> Result<(), ToolError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MockStorage
// ---------------------------------------------------------------------------

/// Mock [`Storage`] backed by an in-memory `BTreeMap`.
///
/// All operations route through a [`Mutex`]-guarded map so the mock is
/// `Send + Sync`. Use this in tests that need a working KV store
/// without spinning up a real backend.
///
/// # Example
///
/// ```ignore
/// // Illustrative; uses async + non-doctest-friendly setup.
/// use tau_ports::fixtures::MockStorage;
///
/// let storage = MockStorage::new("mem");
/// // ... drive storage.put(...) / storage.get(...) ...
/// ```
pub struct MockStorage {
    name: String,
    inner: Mutex<BTreeMap<(Namespace, Key), Vec<u8>>>,
}

impl MockStorage {
    /// Create an empty in-memory storage mock.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            inner: Mutex::new(BTreeMap::new()),
        }
    }
}

impl Storage for MockStorage {
    fn name(&self) -> &str {
        &self.name
    }

    async fn get(&self, namespace: &Namespace, key: &Key) -> Result<Option<Vec<u8>>, StorageError> {
        let map = self.inner.lock().expect("MockStorage mutex poisoned");
        Ok(map.get(&(namespace.clone(), key.clone())).cloned())
    }

    async fn put(
        &self,
        namespace: &Namespace,
        key: &Key,
        value: &[u8],
    ) -> Result<(), StorageError> {
        let mut map = self.inner.lock().expect("MockStorage mutex poisoned");
        map.insert((namespace.clone(), key.clone()), value.to_vec());
        Ok(())
    }

    async fn delete(&self, namespace: &Namespace, key: &Key) -> Result<bool, StorageError> {
        let mut map = self.inner.lock().expect("MockStorage mutex poisoned");
        Ok(map.remove(&(namespace.clone(), key.clone())).is_some())
    }

    async fn list(&self, namespace: &Namespace, prefix: &str) -> Result<Vec<Key>, StorageError> {
        let map = self.inner.lock().expect("MockStorage mutex poisoned");
        Ok(map
            .iter()
            .filter(|((ns, _), _)| ns == namespace)
            .filter(|((_, k), _)| k.as_str().starts_with(prefix))
            .map(|((_, k), _)| k.clone())
            .collect())
    }
}

// ---------------------------------------------------------------------------
// MockSandbox
// ---------------------------------------------------------------------------

/// **PROVISIONAL** — Mock [`Sandbox`] with no-op handles.
///
/// `Handle = ()`: [`Sandbox::create`] returns `Ok(())` for any plan.
/// The [`Sandbox`] trait itself is provisional at v0.1 (see
/// `sandbox.rs` module docs); this mock will likely require breaking
/// changes alongside the trait when Phase-1 sandboxing lands.
///
/// # Example
///
/// ```ignore
/// // Illustrative; uses async + non-doctest-friendly setup.
/// use tau_ports::fixtures::MockSandbox;
///
/// let sandbox = MockSandbox::new("mem");
/// ```
pub struct MockSandbox {
    name: String,
}

impl MockSandbox {
    /// Create a fresh mock sandbox.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl Sandbox for MockSandbox {
    type Handle = ();

    fn name(&self) -> &str {
        &self.name
    }

    async fn create(&self, _plan: SandboxPlan) -> Result<Self::Handle, SandboxError> {
        Ok(())
    }
}
