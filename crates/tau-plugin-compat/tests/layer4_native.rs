//! Layer 4 native live spawn tests — sub-project D Tasks 5 + 7.
//!
//! Each test installs a real plugin binary into a tempdir scope, then
//! drives a golden-path agent invocation under the Native adapter
//! (`--sandbox native`) which engages landlock + seccomp + namespaces.
//! These tests exercise Task 3's symlink-resolution fix
//! (`resolve_symlinks_for_landlock`) — landlock V1 path lookup against
//! Ubuntu's `/bin → /usr/bin` symlinks.
//!
//! # v0.1 scope
//!
//! Two tool-plugin tests (shell + fs-read) are implemented in Task 5.
//! Three HTTP cassette-replay LLM backend tests (anthropic + ollama +
//! openai) are implemented in Task 7.  The HTTP tests start a
//! `CassetteServer` in the test process, configure the plugin binary to
//! connect to `127.0.0.1:<random_port>` via the `base_url` config field,
//! and verify the response matches the cassette's expected outcome.
//!
//! The native adapter's v0.1 over-permissive netns inheritance (when
//! `Network(Http)` is in the plan; per priority-12 net.rs design) makes
//! `127.0.0.1` reachable from the sandboxed plugin process.
//!
//! # Linux-only
//!
//! The `tau-sandbox-native` adapter is Linux-only. This file is gated
//! with `cfg(target_os = "linux")` so non-Linux platforms compile
//! cleanly without the test bodies needing platform-specific code paths.

#![cfg(feature = "integration-tests")]
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tau_domain::{AgentInstanceId, Capability, PluginKind, PluginManifest, PortKind};
use tau_pkg::LockedPlugin;
use tau_ports::{CompletionRequest, ContentBlock, LlmProviderMessage, SandboxPlan, SessionContext};
use tau_runtime::sandbox::registry::RegistryKind;
use tau_runtime::sandbox::{resolve_adapter_forced, SandboxProbe};
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Locate the pre-built shell plugin binary.
///
/// The binary lives in the workspace's target/release directory (or the
/// CARGO_TARGET_DIR override). The test requires that the binary was built
/// before the test runs:
///   cargo build -p tau-plugins-shell --release
///
/// Resolution order mirrors `sandbox_native.rs` in tau-runtime tests.
fn locate_shell_bin() -> PathBuf {
    locate_plugin_bin("shell-plugin")
}

/// Locate the pre-built fs-read plugin binary.
fn locate_fs_read_bin() -> PathBuf {
    locate_plugin_bin("fs-read-plugin")
}

fn locate_plugin_bin(bin_name: &str) -> PathBuf {
    // 1. CARGO_TARGET_DIR override (our CLAUDE.md CARGO rule).
    if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
        let candidate = Path::new(&target_dir).join("release").join(bin_name);
        if candidate.exists() {
            return candidate;
        }
        // Also try absolute path form.
        let abs = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(&target_dir)
            .join("release")
            .join(bin_name);
        if abs.exists() {
            return abs;
        }
    }

    // 2. Workspace-root default target dir.
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    workspace_root.join("target").join("release").join(bin_name)
}

/// Construct a `LockedPlugin` pointing at the given binary path.
fn make_locked_plugin(bin_name: &str, binary_path: PathBuf) -> LockedPlugin {
    let manifest = PluginManifest::new(PortKind::Tool, PluginKind::RustCargo, bin_name.to_string());
    LockedPlugin::new(
        manifest,
        binary_path,
        std::time::SystemTime::UNIX_EPOCH,
        String::new(),
    )
}

/// Construct a `LockedPlugin` for an LLM-backend plugin.
fn make_llm_locked_plugin(bin_name: &str, binary_path: PathBuf) -> LockedPlugin {
    let manifest = PluginManifest::new(
        PortKind::LlmBackend,
        PluginKind::RustCargo,
        bin_name.to_string(),
    );
    LockedPlugin::new(
        manifest,
        binary_path,
        std::time::SystemTime::UNIX_EPOCH,
        String::new(),
    )
}

/// Build a test `SessionContext` with the given granted capabilities.
fn make_session_context_with_caps(caps: Vec<Capability>) -> SessionContext {
    SessionContext::new(AgentInstanceId::new(), tau_domain::Uuid::new_v4(), None)
        .with_granted_capabilities(caps)
}

/// Resolve the native sandbox adapter or skip the test.
///
/// Returns `None` (and prints skip message) if the native adapter is
/// unavailable on this host (e.g. kernel < 5.13 without landlock).
async fn resolve_native_or_skip() -> Option<tau_runtime::sandbox::SandboxAdapter> {
    match resolve_adapter_forced(RegistryKind::Native).await {
        Ok(adapter) => {
            if matches!(adapter.probe().await, SandboxProbe::Unavailable { .. }) {
                eprintln!("SKIP: native adapter probe returned Unavailable");
                None
            } else {
                Some(adapter)
            }
        }
        Err(e) => {
            eprintln!("SKIP: native adapter unavailable: {e}");
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Test 1: shell plugin — echo hello
// ---------------------------------------------------------------------------

/// Install the shell plugin binary, spawn it under the native sandbox adapter,
/// invoke `shell.call({command: "echo", args: ["hello"]})`, and assert that
/// "hello" appears in the result.
///
/// This exercises:
/// - `resolve_adapter_forced(RegistryKind::Native)`
/// - `driver::spawn_tool_under_sandbox` -> `plugin_host::load_tool`
/// - The native adapter's `wrap_spawn` pipeline (landlock + seccomp)
/// - Task 3's `resolve_symlinks_for_landlock` fix: `/bin/echo` ->
///   `/usr/bin/echo` on Ubuntu
/// - The shell plugin's `SessionContext.granted_capabilities` path
///   admission check (process.spawn allow-list)
#[tokio::test]
async fn shell_layer4_native_runs_echo_hello() {
    // 1. Locate the pre-built shell plugin binary.  The CI workflow must
    //    have compiled it beforehand.
    let bin_path = locate_shell_bin();
    if !bin_path.exists() {
        eprintln!(
            "SKIP: shell-plugin binary not found at {}; \
             run `cargo build -p tau-plugins-shell --release` first",
            bin_path.display()
        );
        return;
    }

    // 2. Resolve the native sandbox adapter, skip gracefully on hosts without
    //    landlock/seccomp (macOS, old kernels).
    let adapter = match resolve_native_or_skip().await {
        Some(a) => a,
        None => return,
    };

    // 3. Build the SandboxPlan.  Shell plugin needs process.spawn capability
    //    so the native adapter allows exec(echo). The tempdir itself doesn't
    //    need fs.read -- we're just executing a binary.
    //
    //    The Capability enum variants are #[non_exhaustive]; build via JSON
    //    deserialization (same pattern as shell's capabilities() builder).
    let spawn_cap: Capability = serde_json::from_value(serde_json::json!({
        "kind": "process.spawn",
        "commands": ["echo"]
    }))
    .expect("process.spawn capability JSON must be valid");

    let plan = SandboxPlan::new(vec![spawn_cap.clone()], None, None);

    // 4. Synthesise a LockedPlugin for the shell binary.
    let plugin = make_locked_plugin("shell-plugin", bin_path);

    // 5. Spawn under the native sandbox via the driver.
    let dyn_tool = tau_plugin_compat::driver::spawn_tool_under_sandbox(
        &plugin,
        serde_json::json!({}),
        Some(Arc::new(adapter)),
        Some(&plan),
    )
    .await;

    let dyn_tool = match dyn_tool {
        Ok(t) => t,
        Err(e) => {
            panic!("spawn shell-plugin under native adapter failed: {e:?}");
        }
    };

    // 6. Build a SessionContext granting process.spawn for "echo".
    //    The shell plugin's init() extracts allowed_commands from
    //    granted_capabilities; without this grant, invoke() returns BadArgs.
    let ctx = make_session_context_with_caps(vec![spawn_cap]);
    let mut session = ();

    // 7. Invoke shell.call({command: "echo", args: ["hello"]}).
    let result = dyn_tool
        .invoke(
            &ctx,
            &mut session,
            serde_json::from_value(serde_json::json!({
                "command": "echo",
                "args": ["hello"]
            }))
            .expect("tool args must deserialize"),
        )
        .await
        .expect("shell.call must succeed");

    // 8. Assert "hello" appears somewhere in the result.
    let result_debug = format!("{result:?}");
    assert!(
        result_debug.contains("hello"),
        "expected 'hello' in shell.call result; got: {result_debug}"
    );

    // Also assert it was not an error result.
    assert!(
        !result.is_error,
        "shell.call returned is_error=true; result: {result_debug}"
    );
}

// ---------------------------------------------------------------------------
// Test 2: fs-read plugin — read a data file
// ---------------------------------------------------------------------------

/// Install the fs-read plugin binary, spawn it under the native sandbox
/// adapter, write a data.txt into a tempdir, invoke
/// `fs_read.call({path: <data.txt>})`, and assert the content is read back.
///
/// This exercises:
/// - `resolve_adapter_forced(RegistryKind::Native)` + `SandboxPlan` with
///   `FsCapability::Read` allowing the tempdir.
/// - The native adapter's landlock enforcement for file reads.
/// - Task 3's symlink-resolution fix: `/tmp` may be a symlink on Ubuntu.
/// - The fs-read plugin's glob-based path admission check.
#[tokio::test]
async fn fs_read_layer4_native_reads_data_file() {
    // 1. Locate the pre-built fs-read-plugin binary.
    let bin_path = locate_fs_read_bin();
    if !bin_path.exists() {
        eprintln!(
            "SKIP: fs-read-plugin binary not found at {}; \
             run `cargo build -p tau-plugins-fs-read --release` first",
            bin_path.display()
        );
        return;
    }

    // 2. Resolve the native sandbox adapter, skip gracefully.
    let adapter = match resolve_native_or_skip().await {
        Some(a) => a,
        None => return,
    };

    // 3. Write the data fixture into a tempdir.
    let scope = TempDir::new().expect("tempdir creation must succeed");
    let data_path = scope.path().join("data.txt");
    let data_content = "layer4-native-fs-read-fixture";
    std::fs::write(&data_path, data_content).expect("write data.txt must succeed");

    // The fs-read plugin needs an fs.read capability granting access to the
    // tempdir. Use a glob that covers the whole tempdir.
    let tmpdir_glob = format!("{}/**", scope.path().display());

    let fs_read_cap: Capability = serde_json::from_value(serde_json::json!({
        "kind": "fs.read",
        "paths": [tmpdir_glob.clone()]
    }))
    .expect("fs.read capability JSON must be valid");

    let plan = SandboxPlan::new(vec![fs_read_cap.clone()], None, None);

    // 4. Synthesise a LockedPlugin for the fs-read binary.
    let plugin = make_locked_plugin("fs-read-plugin", bin_path);

    // 5. Spawn under the native sandbox via the driver.
    let dyn_tool = tau_plugin_compat::driver::spawn_tool_under_sandbox(
        &plugin,
        serde_json::json!({}),
        Some(Arc::new(adapter)),
        Some(&plan),
    )
    .await;

    let dyn_tool = match dyn_tool {
        Ok(t) => t,
        Err(e) => {
            panic!("spawn fs-read-plugin under native adapter failed: {e:?}");
        }
    };

    // 6. Build a SessionContext granting fs.read for the tempdir glob.
    let ctx = make_session_context_with_caps(vec![fs_read_cap]);
    let mut session = ();

    // 7. Invoke fs_read.call({path: <data_path>}).
    let data_path_str = data_path
        .to_str()
        .expect("data path must be valid UTF-8")
        .to_string();
    let result = dyn_tool
        .invoke(
            &ctx,
            &mut session,
            serde_json::from_value(serde_json::json!({
                "path": data_path_str
            }))
            .expect("tool args must deserialize"),
        )
        .await
        .expect("fs_read.call must succeed");

    // 8. Assert the result contains the file content (base64-encoded).
    //    The fs-read plugin returns {"contents": "<base64>", "size": <n>}.
    //    We verify is_error=false and that the result is non-empty.
    assert!(
        !result.is_error,
        "fs_read.call returned is_error=true; result: {result:?}"
    );
    assert!(
        !result.content.is_empty(),
        "fs_read.call returned empty content; result: {result:?}"
    );
    // The plugin base64-encodes the file; verify the result debug contains
    // something (the fixture content is short enough to verify round-trip
    // by checking the encoded form is present).
    let result_debug = format!("{result:?}");
    // base64 of "layer4-native-fs-read-fixture"
    let expected_b64 = base64_encode(data_content.as_bytes());
    assert!(
        result_debug.contains(&expected_b64),
        "expected base64-encoded content '{expected_b64}' in fs_read.call result; \
         got: {result_debug}"
    );
}

/// Minimal base64 encoding for the test fixture assertion.
/// Avoids importing the base64 crate into the test binary directly
/// (tau-plugin-compat doesn't depend on it).
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i] as usize;
        let b1 = if i + 1 < input.len() {
            input[i + 1] as usize
        } else {
            0
        };
        let b2 = if i + 2 < input.len() {
            input[i + 2] as usize
        } else {
            0
        };
        output.push(ALPHABET[(b0 >> 2)] as char);
        output.push(ALPHABET[((b0 & 0x3) << 4) | (b1 >> 4)] as char);
        if i + 1 < input.len() {
            output.push(ALPHABET[((b1 & 0xf) << 2) | (b2 >> 6)] as char);
        } else {
            output.push('=');
        }
        if i + 2 < input.len() {
            output.push(ALPHABET[b2 & 0x3f] as char);
        } else {
            output.push('=');
        }
        i += 3;
    }
    output
}

// ---------------------------------------------------------------------------
// Helpers for HTTP cassette-replay tests (Task 7)
// ---------------------------------------------------------------------------

/// Return the workspace root (two levels above this crate's manifest dir).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Build a minimal `CompletionRequest` that matches the cassette's "say hi"
/// fixture.  The model string is provider-specific; pass whatever the
/// cassette's response carries.
fn make_cassette_completion_request(model: &str) -> CompletionRequest {
    let mut req = CompletionRequest::new(model.to_string());
    req.messages
        .push(LlmProviderMessage::user(vec![ContentBlock::Text(
            "say hi".into(),
        )]));
    req.max_tokens = Some(20);
    req
}

/// Build the `net.http` capability that allows the plugin process to
/// reach back to `127.0.0.1` (the in-process cassette server).
fn make_net_http_localhost_cap() -> Capability {
    serde_json::from_value(serde_json::json!({
        "kind": "net.http",
        "hosts": ["127.0.0.1", "localhost"],
        "methods": ["POST", "GET"]
    }))
    .expect("net.http capability JSON must be valid")
}

// ---------------------------------------------------------------------------
// Test 3: anthropic plugin — cassette-replay via native sandbox
// ---------------------------------------------------------------------------

/// Spin up a `CassetteServer` for the anthropic happy-path cassette, install
/// the anthropic plugin binary, configure it to connect to the cassette server
/// via `base_url` in the JSON config, spawn under the native sandbox adapter,
/// invoke `DynLlmBackend::complete`, and assert the response text matches the
/// cassette's expected "Hi there" reply.
///
/// This exercises:
/// - `tau_plugin_test_support::cassette::replay` loading a YAML cassette.
/// - `base_url` config field passing through `spawn_llm_under_sandbox` to the
///   plugin binary process (no real Anthropic API; no real API key needed in
///   env because `api_key` is set directly in the config JSON).
/// - The native adapter's `Network(Http)` plan allowing `127.0.0.1` from
///   within the sandboxed process (v0.1 over-permissive netns inheritance).
/// - `DynLlmBackend::complete` round-trip returning `CompletionResponse.text`.
///
/// Skips if: (a) landlock/native adapter unavailable, (b) anthropic-plugin
/// binary not yet built.
#[tokio::test]
async fn anthropic_layer4_native_completes_via_cassette() {
    // 1. Locate the pre-built anthropic plugin binary.
    let bin_path = locate_plugin_bin("anthropic-plugin");
    if !bin_path.exists() {
        eprintln!(
            "SKIP: anthropic-plugin binary not found at {}; \
             run `cargo build -p tau-plugins-anthropic --release` first",
            bin_path.display()
        );
        return;
    }

    // 2. Resolve the native sandbox adapter, skip gracefully on hosts without
    //    landlock/seccomp (macOS, old kernels).
    let adapter = match resolve_native_or_skip().await {
        Some(a) => a,
        None => return,
    };

    // 3. Start a cassette-replay HTTP server from the anthropic happy-path
    //    cassette.  The server binds to 127.0.0.1:<random_port>.
    let cassette_path = workspace_root()
        .join("crates/tau-plugins/anthropic/tests/cassettes/complete_happy_path.yaml");
    let cassette_server = tau_plugin_test_support::cassette::replay(&cassette_path).await;
    let api_base = cassette_server.uri().to_string();

    // 4. Build the SandboxPlan with Network(Http) capability for localhost so
    //    the sandboxed plugin process can reach the cassette server.
    let net_cap = make_net_http_localhost_cap();
    let plan = SandboxPlan::new(vec![net_cap], None, None);

    // 5. Synthesise a LockedPlugin for the anthropic binary (LlmBackend port).
    let plugin = make_llm_locked_plugin("anthropic-plugin", bin_path);

    // 6. Build the plugin config JSON, pointing base_url at the cassette
    //    server.  Set api_key directly (test-only path) so the plugin
    //    doesn't require a real ANTHROPIC_API_KEY env var.
    let config = serde_json::json!({
        "base_url": api_base,
        "api_key": "sk-ant-test"
    });

    // 7. Spawn the anthropic plugin under the native sandbox via the driver.
    let dyn_llm = tau_plugin_compat::driver::spawn_llm_under_sandbox(
        &plugin,
        config,
        Some(Arc::new(adapter)),
        Some(&plan),
    )
    .await;

    let dyn_llm = match dyn_llm {
        Ok(b) => b,
        Err(e) => {
            panic!("spawn anthropic-plugin under native adapter failed: {e:?}");
        }
    };

    // 8. Invoke complete with a request that matches the cassette.
    let req = make_cassette_completion_request("claude-3-5-haiku-latest");
    let result = dyn_llm.complete(req).await;

    // 9. Assert the response matches the cassette's expected outcome.
    let resp = result.expect("anthropic complete via cassette must succeed");
    assert_eq!(
        resp.text, "Hi there",
        "expected cassette response 'Hi there'; got: {:?}",
        resp.text
    );

    // Verify the cassette server received exactly one request.
    let received = cassette_server.received_requests();
    assert_eq!(
        received.len(),
        1,
        "cassette server should have received exactly 1 request; got: {}",
        received.len()
    );
}

// ---------------------------------------------------------------------------
// Test 4: ollama plugin — cassette-replay via native sandbox
// ---------------------------------------------------------------------------

/// Spin up a `CassetteServer` for the ollama happy-path cassette, install
/// the ollama plugin binary, configure it to connect to the cassette server
/// via `base_url` in the JSON config, spawn under the native sandbox adapter,
/// invoke `DynLlmBackend::complete`, and assert the response text matches the
/// cassette's expected "Hi there" reply.
///
/// Ollama doesn't require an API key; we set `bearer_token_env` to an
/// definitely-unset name so the test is insulated from ambient env vars.
///
/// Skips if: (a) landlock/native adapter unavailable, (b) ollama-plugin
/// binary not yet built.
#[tokio::test]
async fn ollama_layer4_native_completes_via_cassette() {
    // 1. Locate the pre-built ollama plugin binary.
    let bin_path = locate_plugin_bin("ollama-plugin");
    if !bin_path.exists() {
        eprintln!(
            "SKIP: ollama-plugin binary not found at {}; \
             run `cargo build -p tau-plugins-ollama --release` first",
            bin_path.display()
        );
        return;
    }

    // 2. Resolve the native sandbox adapter.
    let adapter = match resolve_native_or_skip().await {
        Some(a) => a,
        None => return,
    };

    // 3. Start the cassette-replay server.
    let cassette_path =
        workspace_root().join("crates/tau-plugins/ollama/tests/cassettes/complete_happy_path.yaml");
    let cassette_server = tau_plugin_test_support::cassette::replay(&cassette_path).await;
    let api_base = cassette_server.uri().to_string();

    // 4. Build the SandboxPlan with Network(Http) for localhost.
    let net_cap = make_net_http_localhost_cap();
    let plan = SandboxPlan::new(vec![net_cap], None, None);

    // 5. Synthesise the LockedPlugin (LlmBackend port).
    let plugin = make_llm_locked_plugin("ollama-plugin", bin_path);

    // 6. Build the config JSON.  Ollama has no required API key; set
    //    bearer_token_env to an unset name so no Authorization header is
    //    injected (correct for the happy-path cassette).
    let config = serde_json::json!({
        "base_url": api_base,
        "bearer_token_env": "OLLAMA_BEARER_TOKEN_DEFINITELY_NOT_SET_FOR_TESTS"
    });

    // 7. Spawn under the native sandbox.
    let dyn_llm = tau_plugin_compat::driver::spawn_llm_under_sandbox(
        &plugin,
        config,
        Some(Arc::new(adapter)),
        Some(&plan),
    )
    .await;

    let dyn_llm = match dyn_llm {
        Ok(b) => b,
        Err(e) => {
            panic!("spawn ollama-plugin under native adapter failed: {e:?}");
        }
    };

    // 8. Invoke complete.
    let req = make_cassette_completion_request("llama3.2");
    let result = dyn_llm.complete(req).await;

    // 9. Assert response matches cassette.
    let resp = result.expect("ollama complete via cassette must succeed");
    assert_eq!(
        resp.text, "Hi there",
        "expected cassette response 'Hi there'; got: {:?}",
        resp.text
    );

    let received = cassette_server.received_requests();
    assert_eq!(
        received.len(),
        1,
        "cassette server should have received exactly 1 request; got: {}",
        received.len()
    );
}

// ---------------------------------------------------------------------------
// Test 5: openai plugin — cassette-replay via native sandbox
// ---------------------------------------------------------------------------

/// Spin up a `CassetteServer` for the openai happy-path cassette, install
/// the openai plugin binary, configure it to connect to the cassette server
/// via `base_url` in the JSON config, spawn under the native sandbox adapter,
/// invoke `DynLlmBackend::complete`, and assert the response text matches the
/// cassette's expected "Hi there" reply.
///
/// Skips if: (a) landlock/native adapter unavailable, (b) openai-plugin
/// binary not yet built.
#[tokio::test]
async fn openai_layer4_native_completes_via_cassette() {
    // 1. Locate the pre-built openai plugin binary.
    let bin_path = locate_plugin_bin("openai-plugin");
    if !bin_path.exists() {
        eprintln!(
            "SKIP: openai-plugin binary not found at {}; \
             run `cargo build -p tau-plugins-openai --release` first",
            bin_path.display()
        );
        return;
    }

    // 2. Resolve the native sandbox adapter.
    let adapter = match resolve_native_or_skip().await {
        Some(a) => a,
        None => return,
    };

    // 3. Start the cassette-replay server.
    let cassette_path =
        workspace_root().join("crates/tau-plugins/openai/tests/cassettes/complete_happy_path.yaml");
    let cassette_server = tau_plugin_test_support::cassette::replay(&cassette_path).await;
    let api_base = cassette_server.uri().to_string();

    // 4. Build the SandboxPlan with Network(Http) for localhost.
    let net_cap = make_net_http_localhost_cap();
    let plan = SandboxPlan::new(vec![net_cap], None, None);

    // 5. Synthesise the LockedPlugin (LlmBackend port).
    let plugin = make_llm_locked_plugin("openai-plugin", bin_path);

    // 6. Build the config JSON.  Set api_key directly (test-only path).
    let config = serde_json::json!({
        "base_url": api_base,
        "api_key": "sk-test"
    });

    // 7. Spawn under the native sandbox.
    let dyn_llm = tau_plugin_compat::driver::spawn_llm_under_sandbox(
        &plugin,
        config,
        Some(Arc::new(adapter)),
        Some(&plan),
    )
    .await;

    let dyn_llm = match dyn_llm {
        Ok(b) => b,
        Err(e) => {
            panic!("spawn openai-plugin under native adapter failed: {e:?}");
        }
    };

    // 8. Invoke complete.
    let req = make_cassette_completion_request("gpt-4o-mini");
    let result = dyn_llm.complete(req).await;

    // 9. Assert response matches cassette.
    let resp = result.expect("openai complete via cassette must succeed");
    assert_eq!(
        resp.text, "Hi there",
        "expected cassette response 'Hi there'; got: {:?}",
        resp.text
    );

    let received = cassette_server.received_requests();
    assert_eq!(
        received.len(),
        1,
        "cassette server should have received exactly 1 request; got: {}",
        received.len()
    );
}
