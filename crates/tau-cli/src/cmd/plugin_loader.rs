//! Shared helpers for loading plugins (LLM backend + tools) declared by
//! a project `[agents.<id>]` entry.
//!
//! Per spec §3.1 / §11: `cmd::run` and `cmd::chat` both resolve the
//! agent's required plugins from the per-scope lockfile, then call
//! `tau_runtime::plugin_host::load_*` to spawn the plugin processes
//! and produce kernel-ready `Arc<dyn Dyn*>` shims.
//!
//! At v0.1, only [`tau_domain::PortKind::LlmBackend`] and
//! [`tau_domain::PortKind::Tool`] are wired into the kernel. Storage
//! and Sandbox plugins are loadable by the host but not yet
//! consumed by `Runtime::run` (per spec §1.1); a project that names
//! a Storage / Sandbox package as a tool will surface a typed error
//! at `RuntimeBuilder::build` rather than silently being skipped.

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use tau_domain::PortKind;
use tau_pkg::scope::ScopeConfig;
use tau_pkg::{LockFile, LockedPlugin, Scope};
use tau_plugin_protocol::handshake::TraceContext;
use tau_runtime::plugin_host::{self, PluginHostOptions, RecorderHandle, RecordingSink};
use tau_runtime::sandbox::{build_plan, resolve_adapter, resolve_adapter_forced};
use tau_runtime::RuntimeBuilder;

use crate::config::AgentEntry;

/// Loaded plugin handles plus the [`RuntimeBuilder`] populated with
/// them. Kept as a single return value so the caller can chain
/// `.build()` directly without juggling `Arc`s.
pub(crate) struct LoadedPlugins {
    /// Builder pre-populated with the LLM backend and any tool plugins.
    /// The caller is expected to call [`RuntimeBuilder::build`] on it.
    pub(crate) builder: RuntimeBuilder,
    /// Per-plugin protocol recorders, when `--record-protocol` is set.
    /// The caller flushes them on exit via [`flush_recorders`] to drain
    /// the tokio file buffers (which otherwise discard pending writes
    /// on `Drop`).
    pub(crate) recorder_ledger: Arc<Mutex<Vec<RecorderHandle>>>,
}

/// Build a [`PluginHostOptions`] tuned for the CLI:
///
/// * If `record_protocol` is `Some(path)`, the returned options carry
///   a `RecordingSink::JsonlFile { path }` plus a fresh
///   `recorder_ledger`. The same `Arc<Mutex<Vec<RecorderHandle>>>` is
///   returned to the caller so it can flush every per-plugin recorder
///   on exit (Task 20: `--record-protocol` flush wiring).
/// * If `record_protocol` is `None`, the options carry no recording
///   sink and an empty (but still allocated, for symmetry) ledger.
/// * `force_passthrough` and `force_adapter_kind` come from the
///   global `--no-sandbox` / `--sandbox <kind>` flags (Task 7).
pub(crate) fn build_host_options(
    record_protocol: Option<&Path>,
    force_passthrough: bool,
    force_adapter_kind: Option<tau_runtime::sandbox::registry::RegistryKind>,
) -> (PluginHostOptions, Arc<Mutex<Vec<RecorderHandle>>>) {
    let ledger: Arc<Mutex<Vec<RecorderHandle>>> = Arc::new(Mutex::new(Vec::new()));
    let mut options = PluginHostOptions::default();
    if let Some(path) = record_protocol {
        options.recording = Some(RecordingSink::JsonlFile {
            path: path.to_path_buf(),
        });
        options.recorder_ledger = Some(ledger.clone());
    }
    options.force_passthrough = force_passthrough;
    options.force_adapter_kind = force_adapter_kind;
    (options, ledger)
}

/// Flush every recorder registered in `ledger`. Called by `cmd::run` /
/// `cmd::chat` after the runtime is dropped (and thus every plugin
/// process is reaped) so the JSONL recording file observes every line
/// the host wrote, even those still buffered inside the tokio
/// `tokio::fs::File` write half.
///
/// Best-effort: a poisoned mutex is logged, never bubbled.
pub(crate) async fn flush_recorders(ledger: Arc<Mutex<Vec<RecorderHandle>>>) {
    // Drain under the synchronous mutex so we don't hold it across the
    // async `flush().await` boundary (which would either require a
    // tokio::sync::Mutex or be a deadlock hazard).
    let recorders: Vec<RecorderHandle> = match ledger.lock() {
        Ok(mut g) => std::mem::take(&mut *g),
        Err(e) => {
            tracing::warn!(
                target: "tau_cli::plugin_loader",
                err = %e,
                "recorder_ledger mutex poisoned; skipping flush"
            );
            return;
        }
    };
    for r in recorders {
        r.flush().await;
    }
}

/// Resolve and load every plugin declared by `entry` against the given
/// `scope`'s lockfile.
///
/// Step-by-step:
///
/// 1. Load the per-scope lockfile.
/// 2. Look up `entry.llm_backend` as a package name. The entry must
///    have a `[plugin]` table recorded at install time
///    (`LockedPackage::plugin = Some(_)`), and must advertise
///    `provides = "llm_backend"`.
/// 3. Spawn the LLM-backend plugin via
///    [`plugin_host::load_llm_backend`].
/// 4. For each `entry.requires.tools` package name: same lookup, but
///    the plugin must advertise `provides = "tool"`. Spawn via
///    [`plugin_host::load_tool`].
/// 5. Funnel the resulting `Arc<dyn Dyn*>` shims into a
///    [`RuntimeBuilder`] via the
///    [`with_dyn_llm_backend`] / [`with_dyn_tool`] entry points
///    (Task 15).
///
/// `trace_context` is supplied by the caller so the same `run_id` /
/// `agent_id` propagate to every spawned plugin. `host_options` lets
/// the caller wire in protocol recording (Task 20) without this helper
/// growing CLI awareness.
///
/// [`with_dyn_llm_backend`]: tau_runtime::RuntimeBuilder::with_dyn_llm_backend
/// [`with_dyn_tool`]: tau_runtime::RuntimeBuilder::with_dyn_tool
pub(crate) async fn load_plugins(
    entry: &AgentEntry,
    scope: &Scope,
    trace_context: TraceContext,
    mut host_options: PluginHostOptions,
) -> anyhow::Result<LoadedPlugins> {
    let recorder_ledger = host_options
        .recorder_ledger
        .clone()
        .unwrap_or_else(|| Arc::new(Mutex::new(Vec::new())));
    let lockfile = LockFile::load(&scope.lockfile_path())
        .with_context(|| format!("loading lockfile {}", scope.lockfile_path().display()))?;

    // ---- Resolve sandbox adapter ----
    // Read the scope's [sandbox] config; fall back to defaults if config.toml
    // doesn't exist yet (e.g. freshly-created scope without an explicit config).
    let config_path = scope.config_path();
    let mut sandbox_requirements = if config_path.exists() {
        let text = std::fs::read_to_string(&config_path)
            .with_context(|| format!("reading scope config at {config_path:?}"))?;
        let scope_config = ScopeConfig::read_from_str(&text)
            .with_context(|| format!("parsing scope config at {config_path:?}"))?;
        scope_config.sandbox
    } else {
        tau_pkg::scope::SandboxRequirements::default()
    };

    // Honor --no-sandbox / --sandbox passthrough: force required_tier=None
    // so the resolver can pick passthrough, bypassing plugin-tier floors.
    if host_options.force_passthrough {
        sandbox_requirements.required_tier = tau_pkg::scope::SandboxRequiredTier::None;
    }

    // Gather plugin-side sandbox requirements from each plugin's on-disk
    // manifest (the full tau.toml carries [sandbox], unlike the lockfile's
    // PluginManifest which only carries provides/kind/bin).
    let mut plugin_sandbox_reqs: Vec<tau_domain::PluginSandboxRequirements> = Vec::new();

    // LLM backend plugin requirements
    if let Some(req) = read_plugin_sandbox_req(scope, &lockfile, &entry.llm_backend) {
        plugin_sandbox_reqs.push(req);
    }

    // Tool plugin requirements
    for tool in &entry.requires.tools {
        if let Some(req) = read_plugin_sandbox_req(scope, &lockfile, tool.name.as_str()) {
            plugin_sandbox_reqs.push(req);
        }
    }

    // When force_passthrough is set, also override any plugin-tier floors so
    // the user's explicit opt-out is honoured even when plugins declare Strict.
    let effective_plugin_reqs = if host_options.force_passthrough {
        // Replace all plugin requirements with tier=None so passthrough adapter wins.
        plugin_sandbox_reqs
            .iter()
            .map(|r| {
                let mut p = r.clone();
                p.required_tier = Some(tau_domain::PluginRequiredTier::None);
                p
            })
            .collect::<Vec<_>>()
    } else {
        plugin_sandbox_reqs
    };

    let adapter = if let Some(kind) = host_options.force_adapter_kind {
        // Force-mode: instantiate the named kind directly, probe it, accept iff Available.
        match resolve_adapter_forced(kind).await {
            Ok(a) => Arc::new(a),
            Err(e) => anyhow::bail!("--sandbox {:?} failed: {e}", kind),
        }
    } else {
        match resolve_adapter(&sandbox_requirements, &effective_plugin_reqs).await {
            Ok(a) => Arc::new(a),
            Err(e) => {
                // Plain anyhow error with Display string. Task 8 will replace
                // this with guided multi-option rendering.
                anyhow::bail!("sandbox adapter resolution failed: {e}");
            }
        }
    };
    host_options.sandbox_adapter = Some(adapter);

    // ---- LLM backend ----
    let llm_plugin = resolve_plugin(
        &lockfile,
        &entry.llm_backend,
        PortKind::LlmBackend,
        "llm_backend",
    )?;

    // Build the sandbox plan for the LLM backend plugin, using the
    // agent's capability_overrides (project-level narrowing).
    let llm_caps = read_plugin_caps_by_name(scope, &lockfile, &entry.llm_backend);
    let llm_plan = build_plan(
        &llm_caps,
        &entry.capability_overrides,
        None, // working_context — deferred
        None, // limits — deferred
    )
    .with_context(|| {
        format!(
            "building sandbox plan for LLM backend {:?}",
            entry.llm_backend
        )
    })?;

    // The agent's per-package config table (a free-form
    // `[agents.<id>.config]` block in the project tau.toml) is passed
    // through to the LLM backend plugin's handshake so the plugin can
    // read API keys, model names, etc. from it. Tools currently get
    // `null` until per-tool config selectors land (out of scope for
    // Task 19).
    let llm_config = agent_config_to_json(entry);
    let llm_backend = plugin_host::load_llm_backend(
        llm_plugin,
        llm_config,
        trace_context.clone(),
        host_options.clone(),
        Some(&llm_plan),
    )
    .await
    .with_context(|| format!("loading LLM backend plugin {:?}", entry.llm_backend))?;

    let mut builder = tau_runtime::Runtime::builder().with_dyn_llm_backend(llm_backend);

    // ---- Tools ----
    for tool in &entry.requires.tools {
        let tool_name = tool.name.as_str();
        let tool_plugin = resolve_plugin(&lockfile, tool_name, PortKind::Tool, "tool")?;

        // Per-tool capability overrides are not yet addressable in the project
        // tau.toml at v0.1 — pass empty slice. Deferred.
        let tool_caps = read_plugin_caps_by_name(scope, &lockfile, tool_name);
        let tool_plan = build_plan(&tool_caps, &[], None, None)
            .with_context(|| format!("building sandbox plan for tool plugin {tool_name:?}"))?;

        let loaded_tool = plugin_host::load_tool(
            tool_plugin,
            // Per-tool config not addressable from project tau.toml at
            // v0.1; pass `null`. Future schema may grow a
            // `[agents.<id>.tools.<name>.config]` selector.
            serde_json::Value::Null,
            trace_context.clone(),
            host_options.clone(),
            Some(&tool_plan),
        )
        .await
        .with_context(|| format!("loading tool plugin {tool_name:?}"))?;
        builder = builder.with_dyn_tool(loaded_tool);
    }

    Ok(LoadedPlugins {
        builder,
        recorder_ledger,
    })
}

/// Read the `[sandbox]` requirements from a plugin's on-disk `tau.toml`.
///
/// Returns `None` if the package is not in the lockfile or its manifest
/// cannot be read (treated as "no floor", not an error).
fn read_plugin_sandbox_req(
    scope: &Scope,
    lockfile: &LockFile,
    package_name: &str,
) -> Option<tau_domain::PluginSandboxRequirements> {
    let pkg_name: tau_domain::PackageName = package_name.parse().ok()?;
    let locked_pkg = lockfile.find(&pkg_name)?;
    let pkg_dir = scope.package_dir(&locked_pkg.name, &locked_pkg.active_version);
    match tau_pkg::read_manifest(&pkg_dir.join("tau.toml")) {
        Ok(manifest) => Some(manifest.sandbox().clone()),
        Err(_) => Some(tau_domain::PluginSandboxRequirements::default()),
    }
}

/// Read the declared [`tau_domain::Capability`] list from a plugin's
/// on-disk `tau.toml`, used to construct a [`tau_ports::SandboxPlan`].
///
/// Returns an empty vec if the package is not in the lockfile or the
/// manifest cannot be read (matches the pre-v4 lockfile behavior where
/// `required_shapes` was absent).
fn read_plugin_caps_by_name(
    scope: &Scope,
    lockfile: &LockFile,
    package_name: &str,
) -> Vec<tau_domain::Capability> {
    let Ok(pkg_name) = package_name.parse::<tau_domain::PackageName>() else {
        return Vec::new();
    };
    let Some(locked_pkg) = lockfile.find(&pkg_name) else {
        return Vec::new();
    };
    let pkg_dir = scope.package_dir(&locked_pkg.name, &locked_pkg.active_version);
    match tau_pkg::read_manifest(&pkg_dir.join("tau.toml")) {
        Ok(manifest) => manifest.capabilities().to_vec(),
        Err(_) => Vec::new(),
    }
}

/// Resolve a package name to its [`LockedPlugin`], checking that
/// `provides` matches the expected port.
///
/// Errors carry the package name and what was wrong (not installed /
/// not a plugin / wrong port) so the CLI can render an actionable
/// "agent X: ..." message.
fn resolve_plugin<'a>(
    lockfile: &'a LockFile,
    package_name: &str,
    expected_port: PortKind,
    expected_label: &'static str,
) -> anyhow::Result<&'a LockedPlugin> {
    let pkg_name: tau_domain::PackageName = package_name.parse().with_context(|| {
        format!("invalid package name {package_name:?} (must be lowercase ASCII kebab-case)")
    })?;

    let pkg = lockfile.find(&pkg_name).ok_or_else(|| {
        anyhow::anyhow!("package {package_name:?} not installed in scope (run `tau install <url>`)")
    })?;

    let plugin = pkg.plugin.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "package {package_name:?} has no [plugin] table in its tau.toml \
             (it is a data-only package; cannot be used as a {expected_label})"
        )
    })?;

    if plugin.manifest.provides != expected_port {
        anyhow::bail!(
            "package {package_name:?} declares provides = {:?} but a {expected_label} \
             ({expected_port:?}) was required",
            plugin.manifest.provides
        );
    }

    Ok(plugin)
}

/// Convert the agent's `[agents.<id>.config]` TOML sub-table to a
/// `serde_json::Value` for the plugin handshake. The plugin's handshake
/// payload accepts arbitrary JSON; TOML scalars and tables map
/// straightforwardly.
fn agent_config_to_json(entry: &AgentEntry) -> serde_json::Value {
    if entry.config.is_empty() {
        return serde_json::Value::Null;
    }
    let mut map = serde_json::Map::with_capacity(entry.config.len());
    for (k, v) in &entry.config {
        map.insert(k.clone(), toml_to_json(v.clone()));
    }
    serde_json::Value::Object(map)
}

fn toml_to_json(v: toml::Value) -> serde_json::Value {
    match v {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::Value::Number(i.into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            // Non-finite floats can't round-trip through JSON; degrade
            // to null so the handshake still succeeds (any plugin that
            // truly needs NaN/inf in its config has bigger problems).
            .unwrap_or(serde_json::Value::Null),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_to_json).collect())
        }
        toml::Value::Table(t) => {
            serde_json::Value::Object(t.into_iter().map(|(k, v)| (k, toml_to_json(v))).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use crate::config::project::{PromptEntry, RequiresEntry};

    fn make_entry(config: BTreeMap<String, toml::Value>) -> AgentEntry {
        AgentEntry {
            id: "reviewer".into(),
            display_name: "Code Reviewer".into(),
            package: "code-reviewer@^0.1".into(),
            llm_backend: "anthropic".into(),
            requires: RequiresEntry::default(),
            config,
            prompt: PromptEntry::None,
            capability_overrides: Vec::new(),
        }
    }

    #[test]
    fn agent_config_to_json_returns_null_when_empty() {
        let entry = make_entry(BTreeMap::new());
        assert_eq!(agent_config_to_json(&entry), serde_json::Value::Null);
    }

    #[test]
    fn agent_config_to_json_passes_through_scalars() {
        let mut cfg = BTreeMap::new();
        cfg.insert("model".into(), toml::Value::String("claude-3".into()));
        cfg.insert("max_tokens".into(), toml::Value::Integer(4096));
        cfg.insert("stream".into(), toml::Value::Boolean(true));
        let entry = make_entry(cfg);
        let json = agent_config_to_json(&entry);
        assert_eq!(json["model"], serde_json::json!("claude-3"));
        assert_eq!(json["max_tokens"], serde_json::json!(4096));
        assert_eq!(json["stream"], serde_json::json!(true));
    }

    #[test]
    fn agent_config_to_json_handles_nested_tables() {
        let mut inner = toml::value::Table::new();
        inner.insert("k".into(), toml::Value::String("v".into()));
        let mut cfg = BTreeMap::new();
        cfg.insert("nested".into(), toml::Value::Table(inner));
        let entry = make_entry(cfg);
        let json = agent_config_to_json(&entry);
        assert_eq!(json["nested"]["k"], serde_json::json!("v"));
    }

    #[test]
    fn toml_to_json_non_finite_float_degrades_to_null() {
        assert_eq!(
            toml_to_json(toml::Value::Float(f64::NAN)),
            serde_json::Value::Null
        );
        assert_eq!(
            toml_to_json(toml::Value::Float(f64::INFINITY)),
            serde_json::Value::Null
        );
    }
}
