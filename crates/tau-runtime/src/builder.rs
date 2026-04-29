//! Runtime kernel and its builder.
//!
//! [`Runtime`] is the immutable kernel produced by
//! [`RuntimeBuilder::build`]. Plugin instances (LLM backends, tools,
//! storages) are registered on the builder, validated at `build()`
//! time, and stored in name-keyed registries on the resulting
//! `Runtime`. The registries are read-only post-`build()`: to add or
//! remove plugins, construct a new `Runtime`.
//!
//! # Dyn-compatibility shim
//!
//! `tau_ports::{LlmBackend, Tool, Storage}` use native `async fn in
//! trait` (per ADR-0003), which makes them **not** dyn-compatible
//! under Rust 1.93. The spec's literal `Arc<dyn LlmBackend>` doesn't
//! compile.
//!
//! tau-runtime resolves this by defining dyn-compatible wrapper
//! traits ([`DynLlmBackend`], [`DynTool`], [`DynStorage`]) with
//! [`Box`]-returning futures, and a blanket impl for any
//! `T: LlmBackend + 'static` (etc.). Public `with_*` builder methods
//! take generics; the registry stores `Arc<dyn Dyn*>`. This is the
//! "boxes once at the dyn-cast boundary" pattern called out in the
//! tau-ports design doc §3.1.
//!
//! See `docs/superpowers/specs/2026-04-28-tau-runtime-design.md` §3.4
//! for the rest of the design rationale.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tau_ports::{
    CompletionRequest, CompletionResponse, CompletionStream, Key, LlmBackend, LlmError, Namespace,
    Sandbox, SandboxError, SandboxPlan, SessionContext, Storage, StorageError, Tool, ToolError,
    ToolResult, ToolSpec,
};

use crate::error::{BuildError, PluginKind};

// ---------------------------------------------------------------------------
// Dyn-compatible wrapper traits
// ---------------------------------------------------------------------------

// Boxed futures are deliberately *not* `Send`-bound: the underlying
// `async fn in trait` methods on `tau_ports::{LlmBackend, Tool,
// Storage}` don't promise `Send`-ness in their RPITIT and there is no
// `trait_variant`-generated `Send` variant at v0.1. tau-runtime's
// dispatcher will adopt a `Send`-bounded variant once one exists; for
// now, the registry is dyn-compatible but the futures are
// single-thread.
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Object-safe wrapper for [`LlmBackend`]. Used internally by
/// [`Runtime`] to store plugin instances in a `HashMap`. Plugin
/// authors implement [`LlmBackend`] directly; the blanket impl below
/// handles the dyn-cast.
pub trait DynLlmBackend: Send + Sync {
    /// Plugin-visible name (matches [`LlmBackend::name`]).
    fn name(&self) -> &str;

    /// Boxed-future wrapper for [`LlmBackend::complete`].
    fn complete<'a>(
        &'a self,
        req: CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionResponse, LlmError>>;

    /// Boxed-future wrapper for [`LlmBackend::stream`].
    fn stream<'a>(
        &'a self,
        req: CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionStream, LlmError>>;
}

impl<T: LlmBackend + 'static> DynLlmBackend for T {
    fn name(&self) -> &str {
        LlmBackend::name(self)
    }

    fn complete<'a>(
        &'a self,
        req: CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionResponse, LlmError>> {
        Box::pin(LlmBackend::complete(self, req))
    }

    fn stream<'a>(
        &'a self,
        req: CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionStream, LlmError>> {
        Box::pin(LlmBackend::stream(self, req))
    }
}

/// Object-safe wrapper for [`Tool<Session = ()>`]. v0.1 restricts to
/// stateless tools (`Session = ()`) per the spec; stateful tools are
/// reached via [`tau_ports::StatelessAdapter`] until a `DynTool`
/// extension lands (ADR-0006).
pub trait DynTool: Send + Sync {
    /// Plugin-visible name (matches [`Tool::name`]).
    fn name(&self) -> &str;

    /// JSON Schema describing the tool's input.
    fn schema(&self) -> ToolSpec;

    /// Capabilities the tool requires of the calling agent's package.
    fn capabilities(&self) -> &[tau_domain::Capability];

    /// Boxed-future wrapper for [`Tool::init`] (returns the empty
    /// session value `()` for stateless tools).
    fn init<'a>(&'a self, ctx: SessionContext) -> BoxFuture<'a, Result<(), ToolError>>;

    /// Boxed-future wrapper for [`Tool::invoke`].
    fn invoke<'a>(
        &'a self,
        session: &'a mut (),
        args: tau_domain::Value,
    ) -> BoxFuture<'a, Result<ToolResult, ToolError>>;

    /// Boxed-future wrapper for [`Tool::teardown`].
    fn teardown<'a>(&'a self, session: ()) -> BoxFuture<'a, Result<(), ToolError>>;
}

impl<T: Tool<Session = ()> + 'static> DynTool for T {
    fn name(&self) -> &str {
        Tool::name(self)
    }

    fn schema(&self) -> ToolSpec {
        Tool::schema(self)
    }

    fn capabilities(&self) -> &[tau_domain::Capability] {
        Tool::capabilities(self)
    }

    fn init<'a>(&'a self, ctx: SessionContext) -> BoxFuture<'a, Result<(), ToolError>> {
        Box::pin(Tool::init(self, ctx))
    }

    fn invoke<'a>(
        &'a self,
        session: &'a mut (),
        args: tau_domain::Value,
    ) -> BoxFuture<'a, Result<ToolResult, ToolError>> {
        Box::pin(Tool::invoke(self, session, args))
    }

    fn teardown<'a>(&'a self, session: ()) -> BoxFuture<'a, Result<(), ToolError>> {
        Box::pin(Tool::teardown(self, session))
    }
}

/// Object-safe wrapper for [`Storage`].
pub trait DynStorage: Send + Sync {
    /// Plugin-visible name (matches [`Storage::name`]).
    fn name(&self) -> &str;

    /// Boxed-future wrapper for [`Storage::get`].
    fn get<'a>(
        &'a self,
        namespace: &'a Namespace,
        key: &'a Key,
    ) -> BoxFuture<'a, Result<Option<Vec<u8>>, StorageError>>;

    /// Boxed-future wrapper for [`Storage::put`].
    fn put<'a>(
        &'a self,
        namespace: &'a Namespace,
        key: &'a Key,
        value: &'a [u8],
    ) -> BoxFuture<'a, Result<(), StorageError>>;

    /// Boxed-future wrapper for [`Storage::delete`].
    fn delete<'a>(
        &'a self,
        namespace: &'a Namespace,
        key: &'a Key,
    ) -> BoxFuture<'a, Result<bool, StorageError>>;

    /// Boxed-future wrapper for [`Storage::list`].
    fn list<'a>(
        &'a self,
        namespace: &'a Namespace,
        prefix: &'a str,
    ) -> BoxFuture<'a, Result<Vec<Key>, StorageError>>;
}

impl<T: Storage + 'static> DynStorage for T {
    fn name(&self) -> &str {
        Storage::name(self)
    }

    fn get<'a>(
        &'a self,
        namespace: &'a Namespace,
        key: &'a Key,
    ) -> BoxFuture<'a, Result<Option<Vec<u8>>, StorageError>> {
        Box::pin(Storage::get(self, namespace, key))
    }

    fn put<'a>(
        &'a self,
        namespace: &'a Namespace,
        key: &'a Key,
        value: &'a [u8],
    ) -> BoxFuture<'a, Result<(), StorageError>> {
        Box::pin(Storage::put(self, namespace, key, value))
    }

    fn delete<'a>(
        &'a self,
        namespace: &'a Namespace,
        key: &'a Key,
    ) -> BoxFuture<'a, Result<bool, StorageError>> {
        Box::pin(Storage::delete(self, namespace, key))
    }

    fn list<'a>(
        &'a self,
        namespace: &'a Namespace,
        prefix: &'a str,
    ) -> BoxFuture<'a, Result<Vec<Key>, StorageError>> {
        Box::pin(Storage::list(self, namespace, prefix))
    }
}

/// Object-safe wrapper for [`Sandbox<Handle = ()>`].
///
/// **PROVISIONAL** — mirrors [`tau_ports::Sandbox`]'s provisional
/// status. v0.1 doesn't wire `Sandbox::create` from the run loop;
/// `DynSandbox` exists so the plugin-host loader signatures
/// ([`crate::plugin_host::load_sandbox`]) can return the same kind of
/// `Arc<dyn Dyn*>` shim the kernel uses for the other ports.
///
/// Restricts to `Handle = ()` for the same reason [`DynTool`] restricts
/// to `Session = ()`: dyn-compatible erasure of a generic-handle
/// `Sandbox` requires a concrete handle type, and at v0.1 the only
/// implementation is `MockSandbox` (which uses `()`).
pub trait DynSandbox: Send + Sync {
    /// Plugin-visible name (matches [`Sandbox::name`]).
    fn name(&self) -> &str;

    /// Boxed-future wrapper for [`Sandbox::create`] with `Handle = ()`.
    fn create<'a>(&'a self, plan: SandboxPlan) -> BoxFuture<'a, Result<(), SandboxError>>;
}

impl<T: Sandbox<Handle = ()> + 'static> DynSandbox for T {
    fn name(&self) -> &str {
        Sandbox::name(self)
    }

    fn create<'a>(&'a self, plan: SandboxPlan) -> BoxFuture<'a, Result<(), SandboxError>> {
        Box::pin(Sandbox::create(self, plan))
    }
}

// ---------------------------------------------------------------------------
// Runtime + RuntimeBuilder
// ---------------------------------------------------------------------------

/// The kernel. Build with [`Runtime::builder`].
///
/// Plugin registries are immutable post-[`RuntimeBuilder::build`]. To
/// add or remove plugins, construct a new `Runtime`.
///
/// # Example
///
/// ```rust,ignore
/// // `Runtime` is `#[non_exhaustive]`; doctests can't construct via
/// // struct-literal syntax, so this example is illustrative only.
/// use tau_runtime::Runtime;
/// use tau_ports::fixtures::MockLlmBackend;
///
/// let runtime = Runtime::builder()
///     .with_llm_backend(MockLlmBackend::new("gpt-4"))
///     .build()
///     .expect("build runtime");
/// ```
#[non_exhaustive]
pub struct Runtime {
    llm_backends: HashMap<String, Arc<dyn DynLlmBackend>>,
    tools: HashMap<String, Arc<dyn DynTool>>,
    #[allow(dead_code)]
    storages: HashMap<String, Arc<dyn DynStorage>>,
    // sandboxes reserved for forward compat (not used at v0.1).
}

impl Runtime {
    /// Construct a fresh [`RuntimeBuilder`].
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::default()
    }

    /// Read-only access to the LLM-backend registry. Used by dispatch
    /// resolution helpers (Task 9) and the run loop (Task 10).
    pub(crate) fn llm_backends(&self) -> &HashMap<String, Arc<dyn DynLlmBackend>> {
        &self.llm_backends
    }

    /// Read-only access to the tool registry. Used by dispatch
    /// resolution helpers (Task 9) and the run loop (Task 10).
    pub(crate) fn tools(&self) -> &HashMap<String, Arc<dyn DynTool>> {
        &self.tools
    }

    /// Read-only access to the storage registry. Reserved for future
    /// dispatch use — at v0.1 nothing in the kernel routes through
    /// storage from the run loop.
    #[allow(dead_code)]
    pub(crate) fn storages(&self) -> &HashMap<String, Arc<dyn DynStorage>> {
        &self.storages
    }
}

/// Builder for [`Runtime`]. Plugin instances accumulate via the
/// `with_*` methods; [`RuntimeBuilder::build`] validates invariants
/// and finalizes the registries.
///
/// # Example
///
/// ```rust,ignore
/// // `RuntimeBuilder` is `#[non_exhaustive]`; doctests can't construct
/// // it via struct-literal syntax. Use [`Runtime::builder`] in
/// // production code.
/// use tau_runtime::Runtime;
/// use tau_ports::fixtures::MockLlmBackend;
///
/// let runtime = Runtime::builder()
///     .with_llm_backend(MockLlmBackend::new("gpt-4"))
///     .build()
///     .expect("build runtime");
/// ```
#[non_exhaustive]
#[derive(Default)]
pub struct RuntimeBuilder {
    llm_backends: Vec<Arc<dyn DynLlmBackend>>,
    tools: Vec<Arc<dyn DynTool>>,
    storages: Vec<Arc<dyn DynStorage>>,
}

impl RuntimeBuilder {
    /// Register an [`LlmBackend`] plugin instance. Multiple backends
    /// may be registered as long as their [`LlmBackend::name`] values
    /// are unique; collisions are reported by [`RuntimeBuilder::build`].
    ///
    /// **Deviation from spec:** the spec writes `Box<dyn LlmBackend>`,
    /// but `LlmBackend`'s native `async fn in trait` is not
    /// dyn-compatible. Accepting a generic `L: LlmBackend + 'static`
    /// keeps the public API ergonomic; the builder boxes through
    /// [`DynLlmBackend`] internally.
    pub fn with_llm_backend<L: LlmBackend + 'static>(mut self, backend: L) -> Self {
        self.llm_backends.push(Arc::new(backend));
        self
    }

    /// Register a [`Tool`] plugin instance with `Session = ()`.
    /// Multiple tools may be registered as long as their
    /// [`Tool::name`] values are unique; collisions are reported by
    /// [`RuntimeBuilder::build`].
    ///
    /// **Deviation from spec:** see [`RuntimeBuilder::with_llm_backend`]
    /// for the dyn-compatibility rationale; the same applies here.
    pub fn with_tool<T: Tool<Session = ()> + 'static>(mut self, tool: T) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }

    /// Register a [`Storage`] plugin instance. Multiple storages may
    /// be registered as long as their [`Storage::name`] values are
    /// unique; collisions are reported by [`RuntimeBuilder::build`].
    ///
    /// **Deviation from spec:** see [`RuntimeBuilder::with_llm_backend`]
    /// for the dyn-compatibility rationale; the same applies here.
    pub fn with_storage<S: Storage + 'static>(mut self, storage: S) -> Self {
        self.storages.push(Arc::new(storage));
        self
    }

    /// Validate registrations and produce a [`Runtime`].
    ///
    /// Validation:
    /// - At least one LLM backend must be registered
    ///   ([`BuildError::NoLlmBackend`] otherwise).
    /// - No name collisions within a kind
    ///   ([`BuildError::NameCollision`] otherwise).
    pub fn build(self) -> Result<Runtime, BuildError> {
        if self.llm_backends.is_empty() {
            return Err(BuildError::NoLlmBackend);
        }
        let llm_backends = collect_llm_backends_by_name(self.llm_backends)?;
        let tools = collect_tools_by_name(self.tools)?;
        let storages = collect_storages_by_name(self.storages)?;
        Ok(Runtime {
            llm_backends,
            tools,
            storages,
        })
    }
}

// Three separate collectors instead of one generic helper: closing
// over `?Sized` `dyn Trait` values fights the type system more than
// the duplication is worth at v0.1.

fn collect_llm_backends_by_name(
    backends: Vec<Arc<dyn DynLlmBackend>>,
) -> Result<HashMap<String, Arc<dyn DynLlmBackend>>, BuildError> {
    let mut map: HashMap<String, Arc<dyn DynLlmBackend>> = HashMap::with_capacity(backends.len());
    for backend in backends {
        let name = backend.name().to_string();
        if map.contains_key(&name) {
            return Err(BuildError::NameCollision {
                kind: PluginKind::LlmBackend,
                name,
            });
        }
        map.insert(name, backend);
    }
    Ok(map)
}

fn collect_tools_by_name(
    tools: Vec<Arc<dyn DynTool>>,
) -> Result<HashMap<String, Arc<dyn DynTool>>, BuildError> {
    let mut map: HashMap<String, Arc<dyn DynTool>> = HashMap::with_capacity(tools.len());
    for tool in tools {
        let name = tool.name().to_string();
        if map.contains_key(&name) {
            return Err(BuildError::NameCollision {
                kind: PluginKind::Tool,
                name,
            });
        }
        map.insert(name, tool);
    }
    Ok(map)
}

fn collect_storages_by_name(
    storages: Vec<Arc<dyn DynStorage>>,
) -> Result<HashMap<String, Arc<dyn DynStorage>>, BuildError> {
    let mut map: HashMap<String, Arc<dyn DynStorage>> = HashMap::with_capacity(storages.len());
    for storage in storages {
        let name = storage.name().to_string();
        if map.contains_key(&name) {
            return Err(BuildError::NameCollision {
                kind: PluginKind::Storage,
                name,
            });
        }
        map.insert(name, storage);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    use tau_domain::Value;
    use tau_ports::fixtures::{make_tool_spec, MockLlmBackend, MockStorage, MockTool};

    fn empty_tool_spec(name: &str) -> tau_ports::ToolSpec {
        make_tool_spec(
            name.to_string(),
            "mock tool".to_string(),
            Value::Object(Default::default()),
        )
    }

    #[test]
    fn build_with_no_llm_backend_returns_no_llm_backend() {
        let result = Runtime::builder().build();
        assert!(
            matches!(result, Err(BuildError::NoLlmBackend)),
            "expected NoLlmBackend, got Ok or other error"
        );
    }

    #[test]
    fn build_with_two_llms_same_name_returns_collision() {
        let result = Runtime::builder()
            .with_llm_backend(MockLlmBackend::new("dup"))
            .with_llm_backend(MockLlmBackend::new("dup"))
            .build();

        let Err(BuildError::NameCollision { kind, name, .. }) = result else {
            panic!("expected NameCollision, got Ok or other error")
        };
        assert_eq!(kind, PluginKind::LlmBackend);
        assert_eq!(name, "dup");
    }

    #[test]
    fn build_with_unique_llms_succeeds() {
        let runtime = Runtime::builder()
            .with_llm_backend(MockLlmBackend::new("gpt-4"))
            .with_llm_backend(MockLlmBackend::new("claude"))
            .build()
            .expect("build runtime");

        let backends = runtime.llm_backends();
        assert_eq!(backends.len(), 2);
        assert!(backends.contains_key("gpt-4"));
        assert!(backends.contains_key("claude"));
    }

    #[test]
    fn build_with_zero_tools_succeeds() {
        let runtime = Runtime::builder()
            .with_llm_backend(MockLlmBackend::new("gpt-4"))
            .build()
            .expect("build runtime");

        assert!(runtime.tools().is_empty());
    }

    #[test]
    fn build_with_two_tools_same_name_returns_collision() {
        let result = Runtime::builder()
            .with_llm_backend(MockLlmBackend::new("gpt-4"))
            .with_tool(MockTool::new("duped", empty_tool_spec("duped")))
            .with_tool(MockTool::new("duped", empty_tool_spec("duped")))
            .build();

        let Err(BuildError::NameCollision { kind, name, .. }) = result else {
            panic!("expected NameCollision, got Ok or other error")
        };
        assert_eq!(kind, PluginKind::Tool);
        assert_eq!(name, "duped");
    }

    #[test]
    fn build_with_zero_storages_succeeds() {
        let runtime = Runtime::builder()
            .with_llm_backend(MockLlmBackend::new("gpt-4"))
            .build()
            .expect("build runtime");

        assert!(runtime.storages().is_empty());
    }

    #[test]
    fn build_with_two_storages_same_name_returns_collision() {
        let result = Runtime::builder()
            .with_llm_backend(MockLlmBackend::new("gpt-4"))
            .with_storage(MockStorage::new("mem"))
            .with_storage(MockStorage::new("mem"))
            .build();

        let Err(BuildError::NameCollision { kind, name, .. }) = result else {
            panic!("expected NameCollision, got Ok or other error")
        };
        assert_eq!(kind, PluginKind::Storage);
        assert_eq!(name, "mem");
    }
}
