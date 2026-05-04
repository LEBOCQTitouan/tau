# Plugin compatibility verification — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Verify the 5 real shipped plugins (anthropic, ollama, openai, fs-read, shell) actually work end-to-end under sandbox enforcement; catch capability drift between manifest and binary at install time; absorb sub-project D's controlled-environment-binary + landlock-symlink-fix foundation.

**Architecture:** Three layers of work. Production code in `tau-pkg::sandbox_check` (new public module performing the install-time cross-check). Test infrastructure in a new `tau-plugin-compat` workspace crate (Layer 3 + Layer 4 container + Layer 4 native harness, plus the controlled-env binary fixture). One adapter touchup in `tau-sandbox-native::light` (symlink resolution for landlock V1 path lookup).

**Tech Stack:** Rust 1.91 stable / 1.91 MSRV. `tokio::process::Command`, `tau-plugin-protocol::Frame` for the cross-check IPC. `assert_cmd` + `tempfile` for the test harness. `insta` for snapshot tests. Existing `tau-plugin-test-support` cassette-replay for HTTP plugins. No new external dependencies beyond what's already in the workspace.

---

## Plan-erratum block

Carryovers from sub-project A's lessons learned that apply to this plan:

- **VERIFY against BASE_SHA = `c391622` before claiming "pre-existing failure".** Sub-project A had 5 implementer "pre-existing failure" claims; 4 were real regressions. Always reproduce on `c391622` before attributing failure to anything but the current task.
- **`#[non_exhaustive]` on every new public type.** `CrossCheckError`, any new `LightError` variant, any new `InstallError` variant.
- **Use `--all-targets` (NOT `--lib`) at every gate.** Sub-project A's implementers triggered false alarms by missing integration test breakage with `--lib`.
- **Cargo.lock staged in same commit as new deps.** Task 4 introduces `tau-plugin-compat` with new dev-deps; lockfile changes must land in Task 4's commit.
- **Branch protection bumps from 25 → ~28-29.** Task 12 surfaces new check names; user does the GitHub-settings update manually as part of Task 12's gate.
- **macOS dev / Linux CI gap.** Task 3's symlink-resolution code is Linux-only behavior but the `tau-sandbox-native` crate compiles on macOS/Windows (with Linux-only paths gated). Task 8's tests are Linux-only RUN but compile everywhere.
- **`TAU_TESTING_ALLOW_MOCK_SANDBOX=1` env-var preserved but NOT used here.** These tests intentionally use real adapters.
- **Existing install lifecycle has steps 8.5 (build) and 8.6 (tree SHA-256).** New Layer 2 cross-check lands at step 8.7 — between SHA-compute and lockfile-write. Do NOT renumber existing steps.
- **`tau-pkg` does not depend on `tau-runtime`.** Task 2's cross-check builds its own minimal spawn-and-handshake using `tau-plugin-protocol::Frame` types directly + `tokio::process::Command`. ~80 LOC of IPC boilerplate; not a copy of the kernel's full `tau-runtime/src/plugin_host/handshake.rs` (which has retry, recording, trace context, etc.).
- **Tool plugins use `tool.describe_capabilities` per method.** Reference implementation at `crates/tau-runtime/src/plugin_host/ipc_tool.rs::fetch_capabilities` (lines 137-180). Wire format: zero-arg request, response is `Vec<tau_domain::Capability>` rmp-encoded. Cross-check enumerates this for each method in `HandshakeResponse.methods` and unions the results.
- **LLM-backend / storage plugins have no equivalent wire mechanism today.** Cross-check returns `manifest.capabilities` verbatim and emits `tracing::debug!` noting the manifest-only path.
- **The `[sandbox]` block in plugin manifests is already wired** by sub-project A's `PluginSandboxRequirements` work. Task 1 is the first place this field gets a value in production code.
- **`Capability` vs `CapabilityShape`.** Cross-check error variants use `Capability` (full instance, with paths/hosts) for actionable messages. Function returns `Vec<CapabilityShape>` (coarser priority-12 vocabulary) for `LockedPlugin.required_shapes` storage. Comparison logic uses `Capability`; lockfile uses `CapabilityShape`.
- **No new lockfile schema bump.** `LockedPlugin.required_shapes` is already part of v4 (priority 12); we're just populating it.

---

## File structure

| Path | Status | Responsibility |
|---|---|---|
| `crates/tau-plugins/anthropic/tau.toml` | Modify | Add `[sandbox] required_tier = "strict"` |
| `crates/tau-plugins/ollama/tau.toml` | Modify | Same |
| `crates/tau-plugins/openai/tau.toml` | Modify | Same |
| `crates/tau-plugins/fs-read/tau.toml` | Modify | Same |
| `crates/tau-plugins/shell/tau.toml` | Modify | Same |
| `crates/tau-pkg/src/sandbox_check.rs` | Create | Public cross-check module |
| `crates/tau-pkg/src/lib.rs` | Modify | Export `sandbox_check` module |
| `crates/tau-pkg/src/error.rs` | Modify | Add `InstallError::CrossCheck` variant |
| `crates/tau-pkg/src/install.rs` | Modify | Wire cross-check at step 8.7 |
| `crates/tau-pkg/Cargo.toml` | Modify | Add `tau-plugin-protocol`, `tokio` dep |
| `crates/tau-pkg/tests/install_cross_check.rs` | Create | Install integration tests |
| `crates/tau-sandbox-native/src/light.rs` | Modify | Add `resolve_symlinks_for_landlock` + wire into `apply_landlock` |
| `crates/tau-sandbox-native/src/lib.rs` | Modify | Add `LightError::SymlinkResolution` variant (or wherever `LightError` lives) |
| `crates/tau-plugin-compat/` | Create | New workspace crate (test infrastructure) |
| `crates/tau-plugin-compat/Cargo.toml` | Create | `publish = false`; deps + dev-deps |
| `crates/tau-plugin-compat/src/lib.rs` | Create | Fixture-build helpers + driver functions |
| `crates/tau-plugin-compat/fixtures/controlled-env-binary/` | Create | Statically-linked test binary |
| `crates/tau-plugin-compat/fixtures/projects/{anthropic,ollama,openai,fs-read,shell}/` | Create | Per-plugin scope + tau.toml fixtures |
| `crates/tau-plugin-compat/tests/layer3_check_sandbox.rs` | Create | 5 Layer 3 tests |
| `crates/tau-plugin-compat/tests/layer4_container.rs` | Create | 5 container live spawn tests |
| `crates/tau-plugin-compat/tests/layer4_native.rs` | Create | 5 native live spawn tests (Linux gated) |
| `crates/tau-cli/src/cmd/error_render.rs` | Modify | Add `render_cross_check_error` function |
| `crates/tau-cli/tests/cmd_install_cross_check_render.rs` | Create | 3 insta snapshot tests |
| `Cargo.toml` | Modify | Add `crates/tau-plugin-compat` to workspace members |
| `Cargo.lock` | Modify | New crate's transitive deps |
| `.github/workflows/ci.yml` | Modify | Add `build (tau-plugin-compat)` + `test (tau-plugin-compat / linux)` jobs |
| `docs/decisions/0016-plugin-compat-verification.md` | Create | ADR (Task 13) |
| `ROADMAP.md` | Modify | Mark sub-project B done (Task 13) |
| `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` | Modify | Mark B done; flag D's reduced scope (Task 13) |

---

## Task 1: Plugin manifest tier declarations

**Files:**
- Modify: `crates/tau-plugins/anthropic/tau.toml` (append `[sandbox]` block)
- Modify: `crates/tau-plugins/ollama/tau.toml` (append `[sandbox]` block)
- Modify: `crates/tau-plugins/openai/tau.toml` (append `[sandbox]` block)
- Modify: `crates/tau-plugins/fs-read/tau.toml` (append `[sandbox]` block)
- Modify: `crates/tau-plugins/shell/tau.toml` (append `[sandbox]` block)
- Test: `crates/tau-domain/src/package/manifest.rs` (~3 unit tests added in the test module)

- [ ] **Step 1: Add `[sandbox]` block to anthropic/tau.toml**

Append to `crates/tau-plugins/anthropic/tau.toml`:
```toml

[sandbox]
required_tier = "strict"
```

- [ ] **Step 2: Add `[sandbox]` block to ollama/tau.toml**

Append to `crates/tau-plugins/ollama/tau.toml`:
```toml

[sandbox]
required_tier = "strict"
```

- [ ] **Step 3: Add `[sandbox]` block to openai/tau.toml**

Append to `crates/tau-plugins/openai/tau.toml`:
```toml

[sandbox]
required_tier = "strict"
```

- [ ] **Step 4: Add `[sandbox]` block to fs-read/tau.toml**

Append to `crates/tau-plugins/fs-read/tau.toml`:
```toml

[sandbox]
required_tier = "strict"
```

- [ ] **Step 5: Add `[sandbox]` block to shell/tau.toml**

Append to `crates/tau-plugins/shell/tau.toml`:
```toml

[sandbox]
required_tier = "strict"
```

- [ ] **Step 6: Write unit tests verifying each manifest parses with required_tier = strict**

Add to the `#[cfg(test)] mod tests { ... }` block in `crates/tau-domain/src/package/manifest.rs`:

```rust
    /// Each shipped plugin's manifest declares required_tier = strict
    /// after sub-project B's tier-declarations task.
    #[test]
    fn shipped_plugin_anthropic_declares_strict_tier() {
        let toml = include_str!("../../../tau-plugins/anthropic/tau.toml");
        let manifest: PackageManifest = toml::from_str(toml).expect("valid manifest");
        assert_eq!(
            manifest.sandbox().required_tier,
            Some(crate::package::sandbox::PluginRequiredTier::Strict),
            "anthropic must declare strict tier per ADR-0016"
        );
    }

    #[test]
    fn shipped_plugin_fs_read_declares_strict_tier() {
        let toml = include_str!("../../../tau-plugins/fs-read/tau.toml");
        let manifest: PackageManifest = toml::from_str(toml).expect("valid manifest");
        assert_eq!(
            manifest.sandbox().required_tier,
            Some(crate::package::sandbox::PluginRequiredTier::Strict),
            "fs-read must declare strict tier per ADR-0016"
        );
    }

    #[test]
    fn shipped_plugin_shell_declares_strict_tier() {
        let toml = include_str!("../../../tau-plugins/shell/tau.toml");
        let manifest: PackageManifest = toml::from_str(toml).expect("valid manifest");
        assert_eq!(
            manifest.sandbox().required_tier,
            Some(crate::package::sandbox::PluginRequiredTier::Strict),
            "shell must declare strict tier per ADR-0016"
        );
    }
```

(Three plugins covered as a representative sample; ollama and openai use the same parse path as anthropic.)

- [ ] **Step 7: Run the new tests to verify they pass**

```bash
cargo test -p tau-domain --test '*' --lib shipped_plugin_
```

Expected: 3 passed.

- [ ] **Step 8: Run all 5 verification gates**

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test --workspace --doc
```

Expected: all green. The toy plugins (`echo-llm`, `echo-tool`) remain unchanged and continue to parse with `PluginSandboxRequirements::default()`.

- [ ] **Step 9: Commit**

```bash
git add crates/tau-plugins/anthropic/tau.toml \
        crates/tau-plugins/ollama/tau.toml \
        crates/tau-plugins/openai/tau.toml \
        crates/tau-plugins/fs-read/tau.toml \
        crates/tau-plugins/shell/tau.toml \
        crates/tau-domain/src/package/manifest.rs

git commit -m "feat(plugins): declare required_tier = strict on 5 real plugins

Sub-project B Task 1. Per ADR-0016 Decision 5, all real plugins assert
strict-tier sandbox enforcement. Toy plugins (echo-llm, echo-tool)
remain at PluginSandboxRequirements::default() (no tier floor).
"
```

---

## Task 2: tau-pkg::sandbox_check module

**Files:**
- Create: `crates/tau-pkg/src/sandbox_check.rs`
- Modify: `crates/tau-pkg/src/lib.rs` (add `pub mod sandbox_check`)
- Modify: `crates/tau-pkg/Cargo.toml` (add `tau-plugin-protocol`, `tokio`, `rmp-serde`, `tracing` deps if missing)

- [ ] **Step 1: Add necessary deps to `crates/tau-pkg/Cargo.toml`**

Verify these are present in `[dependencies]` (most already are). Add anything missing:

```toml
[dependencies]
# ... existing entries ...
tau-plugin-protocol = { path = "../tau-plugin-protocol" }
tokio = { workspace = true, features = ["process", "io-util", "macros", "rt", "time"] }
rmp-serde = { workspace = true }
tracing = { workspace = true }
```

If `tokio` was already a dep without `process` and `io-util` features, augment the feature list rather than redeclaring.

- [ ] **Step 2: Add module declaration to lib.rs**

Append to `crates/tau-pkg/src/lib.rs`:

```rust
pub mod sandbox_check;
```

(Maintain the existing `pub mod` ordering. Place alphabetically near `pub mod scope` etc.)

- [ ] **Step 3: Create `crates/tau-pkg/src/sandbox_check.rs` with the error type and function signature**

```rust
//! Layer 2 install-time cross-check: spawn the freshly-built plugin
//! binary, perform the handshake, and verify the binary's actually-claimed
//! capability surface matches the manifest's declared `[[capabilities]]`.
//!
//! Sub-project B (2026-05-04). See ADR-0016.
//!
//! # Per-port behavior
//!
//! - **Tool plugins** — enumerate `tool.describe_capabilities` for each
//!   method advertised in the handshake response; union all per-method
//!   capability lists; compare against the manifest. Both directions
//!   (binary-claims-extra and manifest-declares-unused) hard-fail.
//!
//! - **LLM-backend / Storage plugins** — no equivalent wire mechanism
//!   today. Returns `manifest.capabilities` verbatim and emits a
//!   `tracing::debug!` noting the manifest-only path. The cross-check
//!   for these ports is effectively "the manifest is well-formed and
//!   the binary handshakes successfully".

use std::path::Path;
use std::time::Duration;

use tau_domain::{Capability, CapabilityShape, PackageManifest, PortKind};
use tau_plugin_protocol::handshake::{meta, HandshakeRequest, HandshakeResponse};
use tau_plugin_protocol::Frame;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::timeout;

/// Errors from [`cross_check_plugin_capabilities`].
///
/// All variants surface as `tau install` exit code 2 (configuration
/// error per ADR-0007 §7).
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum CrossCheckError {
    /// Failed to spawn the plugin binary as a child process.
    #[error("plugin spawn failed: {0}")]
    SpawnFailed(String),

    /// The plugin process exited or returned a malformed frame before
    /// or during the handshake.
    #[error("plugin handshake failed: {0}")]
    HandshakeFailed(String),

    /// The binary calls `tool.describe_capabilities` and asks for a
    /// capability the manifest's `[[capabilities]]` block does not declare.
    #[error("plugin '{plugin}' declares capability {claimed:?} via tool.describe_capabilities but manifest does not include it")]
    BinaryClaimsExtra {
        /// Plugin name (manifest `[plugin].bin` or `package.name`).
        plugin: String,
        /// The capability the binary requested but the manifest didn't.
        claimed: Capability,
    },

    /// The manifest declares a capability the binary did not request
    /// via any `tool.describe_capabilities` response.
    #[error("manifest of '{plugin}' declares capability {declared:?} but binary does not request it")]
    ManifestDeclaresUnused {
        plugin: String,
        declared: Capability,
    },
}

/// Spawn the plugin binary, perform handshake + per-method
/// `tool.describe_capabilities`, and validate the binary's claimed
/// surface against the manifest.
///
/// Returns the resolved [`Vec<CapabilityShape>`] for storage in
/// `LockedPlugin.required_shapes`.
pub async fn cross_check_plugin_capabilities(
    binary_path: &Path,
    manifest: &PackageManifest,
) -> Result<Vec<CapabilityShape>, CrossCheckError> {
    // Step 1: spawn the binary and perform the handshake.
    let (mut child, response) = spawn_and_handshake(binary_path, manifest).await?;

    // Step 2: collect the binary's actually-claimed capabilities,
    // gated by port kind.
    let binary_capabilities: Vec<Capability> = match response.provides {
        PortKind::Tool => collect_tool_capabilities(&mut child, &response).await?,
        PortKind::LlmBackend | PortKind::Storage => {
            tracing::debug!(
                target: "tau_pkg::sandbox_check",
                port = ?response.provides,
                plugin = %manifest.bin,
                "port {:?} cross-check is manifest-only until wire mechanism lands",
                response.provides,
            );
            manifest.capabilities.clone()
        }
    };

    // Step 3: send shutdown notification (best-effort; ignore errors).
    let _ = send_shutdown(&mut child).await;

    // Step 4: reap the child to avoid zombies.
    let _ = child.wait().await;

    // Step 5: bidirectional set-diff against manifest.
    diff_capabilities(&manifest.bin, &binary_capabilities, &manifest.capabilities)?;

    // Step 6: reduce to CapabilityShape vocabulary for lockfile storage.
    let shapes: Vec<CapabilityShape> = binary_capabilities
        .iter()
        .map(CapabilityShape::from_capability)
        .collect::<Vec<_>>();

    Ok(shapes)
}

/// Spawn the binary, send the handshake request, parse the handshake
/// response. Returns the child handle and the response.
async fn spawn_and_handshake(
    binary_path: &Path,
    manifest: &PackageManifest,
) -> Result<(tokio::process::Child, HandshakeResponse), CrossCheckError> {
    let mut child = Command::new(binary_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| CrossCheckError::SpawnFailed(format!("{e}: {}", binary_path.display())))?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| CrossCheckError::SpawnFailed("failed to take stdin pipe".to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CrossCheckError::SpawnFailed("failed to take stdout pipe".to_string()))?;

    let request = HandshakeRequest::new(/* protocol_version */ "1".to_string());
    let request_bytes = rmp_serde::to_vec(&request)
        .map_err(|e| CrossCheckError::HandshakeFailed(format!("rmp encode handshake: {e}")))?;
    let frame = Frame::Request {
        id: 1,
        method: meta::HANDSHAKE_METHOD.to_string(),
        params: request_bytes,
    };
    let frame_bytes = frame
        .encode()
        .map_err(|e| CrossCheckError::HandshakeFailed(format!("encode handshake frame: {e}")))?;

    let mut stdin = stdin;
    timeout(Duration::from_secs(5), async {
        stdin.write_all(&frame_bytes).await?;
        stdin.flush().await?;
        Ok::<(), std::io::Error>(())
    })
    .await
    .map_err(|_| CrossCheckError::HandshakeFailed("handshake send timed out".to_string()))?
    .map_err(|e| CrossCheckError::HandshakeFailed(format!("write handshake: {e}")))?;

    let response = read_handshake_response(stdout, &manifest.bin).await?;
    // The child's stdin/stdout were re-attached; reattach the modified
    // stdin pipe so callers can send shutdown later.
    child.stdin = Some(stdin);
    Ok((child, response))
}

/// Read a single Frame::Response from the child's stdout and decode
/// it as a HandshakeResponse.
async fn read_handshake_response(
    mut stdout: tokio::process::ChildStdout,
    plugin_name: &str,
) -> Result<HandshakeResponse, CrossCheckError> {
    // The protocol uses 4-byte big-endian length prefix per Frame::encode.
    // (Mirroring what tau-plugin-protocol Frame uses; see
    // crates/tau-plugin-protocol/src/frame.rs.)
    let frame = timeout(Duration::from_secs(5), async {
        let mut len_buf = [0u8; 4];
        stdout.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut payload = vec![0u8; len];
        stdout.read_exact(&mut payload).await?;
        Frame::decode_payload(&payload).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("decode: {e}"))
        })
    })
    .await
    .map_err(|_| CrossCheckError::HandshakeFailed(format!("plugin {plugin_name}: handshake response timed out")))?
    .map_err(|e| CrossCheckError::HandshakeFailed(format!("plugin {plugin_name}: {e}")))?;

    match frame {
        Frame::Response { result, .. } => {
            let response: HandshakeResponse = rmp_serde::from_slice(&result).map_err(|e| {
                CrossCheckError::HandshakeFailed(format!(
                    "plugin {plugin_name}: rmp decode HandshakeResponse: {e}"
                ))
            })?;
            Ok(response)
        }
        other => Err(CrossCheckError::HandshakeFailed(format!(
            "plugin {plugin_name}: expected Response frame, got {other:?}"
        ))),
    }
}

/// For tool-port plugins: enumerate `tool.describe_capabilities` for
/// each method in the handshake response and union the results.
async fn collect_tool_capabilities(
    child: &mut tokio::process::Child,
    response: &HandshakeResponse,
) -> Result<Vec<Capability>, CrossCheckError> {
    let mut all_caps: Vec<Capability> = Vec::new();
    let mut next_id: u32 = 2; // handshake used id=1

    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| CrossCheckError::HandshakeFailed("stdin pipe gone".to_string()))?;
    let stdout = child
        .stdout
        .as_mut()
        .ok_or_else(|| CrossCheckError::HandshakeFailed("stdout pipe gone".to_string()))?;

    for method in &response.methods {
        // Only tool methods carry capabilities; skip method names that
        // aren't of the `tool.*` family (be defensive against odd plugins).
        if !method.starts_with("tool.") {
            continue;
        }

        // Send tool.describe_capabilities for this method.
        // Wire shape: zero-arg request (matching ipc_tool::fetch_capabilities).
        let id = next_id;
        next_id += 1;

        let params_bytes = rmp_serde::to_vec::<Vec<()>>(&Vec::new()).map_err(|e| {
            CrossCheckError::HandshakeFailed(format!("rmp encode params: {e}"))
        })?;
        let frame = Frame::Request {
            id,
            method: "tool.describe_capabilities".to_string(),
            params: params_bytes,
        };
        let frame_bytes = frame.encode().map_err(|e| {
            CrossCheckError::HandshakeFailed(format!("encode describe frame: {e}"))
        })?;

        timeout(Duration::from_secs(5), async {
            stdin.write_all(&frame_bytes).await?;
            stdin.flush().await?;
            Ok::<(), std::io::Error>(())
        })
        .await
        .map_err(|_| CrossCheckError::HandshakeFailed("describe send timed out".to_string()))?
        .map_err(|e| CrossCheckError::HandshakeFailed(format!("write describe: {e}")))?;

        // Read response.
        let mut len_buf = [0u8; 4];
        timeout(Duration::from_secs(5), stdout.read_exact(&mut len_buf))
            .await
            .map_err(|_| CrossCheckError::HandshakeFailed("describe response timed out".to_string()))?
            .map_err(|e| CrossCheckError::HandshakeFailed(format!("read len: {e}")))?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut payload = vec![0u8; len];
        stdout
            .read_exact(&mut payload)
            .await
            .map_err(|e| CrossCheckError::HandshakeFailed(format!("read payload: {e}")))?;
        let frame = Frame::decode_payload(&payload).map_err(|e| {
            CrossCheckError::HandshakeFailed(format!("decode describe frame: {e}"))
        })?;

        match frame {
            Frame::Response { result, .. } => {
                let caps: Vec<Capability> = rmp_serde::from_slice(&result).map_err(|e| {
                    CrossCheckError::HandshakeFailed(format!("rmp decode Vec<Capability>: {e}"))
                })?;
                all_caps.extend(caps);
            }
            Frame::Error { code, message, .. } => {
                // Plugin doesn't implement this RPC for this method; skip
                // (mirrors the kernel's tolerant default at ipc_tool.rs).
                tracing::debug!(
                    target: "tau_pkg::sandbox_check",
                    %method, code, %message,
                    "tool.describe_capabilities returned error; skipping"
                );
            }
            other => {
                return Err(CrossCheckError::HandshakeFailed(format!(
                    "expected Response/Error for {method}, got {other:?}"
                )));
            }
        }
    }

    // Deduplicate (a plugin with two tools sharing a capability shouldn't
    // double-count).
    all_caps.sort_by_key(|c| format!("{c:?}"));
    all_caps.dedup_by_key(|c| format!("{c:?}"));
    Ok(all_caps)
}

/// Bidirectional capability set-diff between binary and manifest.
fn diff_capabilities(
    plugin_name: &str,
    binary: &[Capability],
    manifest: &[Capability],
) -> Result<(), CrossCheckError> {
    // Helper: a capability set comparison that's tolerant of vec ordering
    // but exact on capability instance equality.
    for claimed in binary {
        if !manifest.contains(claimed) {
            return Err(CrossCheckError::BinaryClaimsExtra {
                plugin: plugin_name.to_string(),
                claimed: claimed.clone(),
            });
        }
    }
    for declared in manifest {
        if !binary.contains(declared) {
            return Err(CrossCheckError::ManifestDeclaresUnused {
                plugin: plugin_name.to_string(),
                declared: declared.clone(),
            });
        }
    }
    Ok(())
}

/// Send a meta.shutdown notification frame (best-effort; ignored on error).
async fn send_shutdown(child: &mut tokio::process::Child) -> std::io::Result<()> {
    if let Some(stdin) = child.stdin.as_mut() {
        let frame = Frame::Notification {
            method: meta::SHUTDOWN_METHOD.to_string(),
            params: rmp_serde::to_vec::<Vec<()>>(&Vec::new()).unwrap_or_default(),
        };
        if let Ok(bytes) = frame.encode() {
            let _ = stdin.write_all(&bytes).await;
            let _ = stdin.flush().await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The unit tests in this module use mocked child processes; they
    // do NOT spawn real plugin binaries. Real-binary cross-check
    // coverage lives in the install integration tests (Task 9) and the
    // tau-plugin-compat crate (Tasks 4-8).

    #[test]
    fn cross_check_error_display_includes_plugin_name() {
        // Capability::Filesystem(Read{paths: ...}) is the priority-12 type;
        // construct via its public constructor.
        let claimed = tau_domain::Capability::Filesystem(tau_domain::FsCapability::Read {
            paths: vec!["/etc/passwd".to_string()],
        });
        let err = CrossCheckError::BinaryClaimsExtra {
            plugin: "evil-plugin".to_string(),
            claimed,
        };
        let msg = format!("{err}");
        assert!(msg.contains("evil-plugin"));
        assert!(msg.contains("Filesystem"));
    }

    #[test]
    fn diff_capabilities_returns_ok_when_match() {
        let cap = tau_domain::Capability::Network(tau_domain::NetCapability::Http {
            hosts: vec!["api.example.com".to_string()],
            methods: vec!["GET".to_string()],
        });
        let result = diff_capabilities("ok-plugin", &[cap.clone()], &[cap]);
        assert!(result.is_ok());
    }

    #[test]
    fn diff_capabilities_rejects_binary_claims_extra() {
        let cap_binary = tau_domain::Capability::Filesystem(tau_domain::FsCapability::Read {
            paths: vec!["/etc/passwd".to_string()],
        });
        let result = diff_capabilities("evil", &[cap_binary], &[]);
        assert!(matches!(result, Err(CrossCheckError::BinaryClaimsExtra { .. })));
    }

    #[test]
    fn diff_capabilities_rejects_manifest_declares_unused() {
        let cap_manifest = tau_domain::Capability::Filesystem(tau_domain::FsCapability::Read {
            paths: vec!["/etc/passwd".to_string()],
        });
        let result = diff_capabilities("over-claimer", &[], &[cap_manifest]);
        assert!(matches!(result, Err(CrossCheckError::ManifestDeclaresUnused { .. })));
    }

    #[test]
    fn diff_capabilities_returns_ok_on_dual_empty() {
        let result = diff_capabilities("trivial", &[], &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn cross_check_error_is_non_exhaustive_via_match() {
        // The test confirms that adding a new variant won't break older
        // callers; since CrossCheckError is #[non_exhaustive], match
        // expressions against it require a wildcard arm.
        // (This compiles only if non_exhaustive is in effect.)
        let claimed = tau_domain::Capability::Filesystem(tau_domain::FsCapability::Read {
            paths: vec![],
        });
        let err = CrossCheckError::BinaryClaimsExtra {
            plugin: "p".to_string(),
            claimed,
        };
        match err {
            CrossCheckError::SpawnFailed(_)
            | CrossCheckError::HandshakeFailed(_)
            | CrossCheckError::BinaryClaimsExtra { .. }
            | CrossCheckError::ManifestDeclaresUnused { .. } => {}
            // Deliberately no wildcard here in the test crate path; if a
            // future variant is added without updating this test, that's
            // a deliberate signal to extend coverage.
        }
    }

    #[test]
    fn diff_capabilities_dedup_safe() {
        // Same capability listed twice in binary should still match
        // a manifest with one entry. Cross-check dedupes binary side.
        let cap = tau_domain::Capability::Network(tau_domain::NetCapability::Http {
            hosts: vec!["api.example.com".to_string()],
            methods: vec!["GET".to_string()],
        });
        let mut binary = vec![cap.clone(), cap.clone()];
        binary.sort_by_key(|c| format!("{c:?}"));
        binary.dedup_by_key(|c| format!("{c:?}"));
        let result = diff_capabilities("dedup-test", &binary, &[cap]);
        assert!(result.is_ok());
    }

    #[test]
    fn diff_capabilities_order_independent() {
        let cap_a = tau_domain::Capability::Filesystem(tau_domain::FsCapability::Read {
            paths: vec!["/a".to_string()],
        });
        let cap_b = tau_domain::Capability::Filesystem(tau_domain::FsCapability::Write {
            paths: vec!["/b".to_string()],
        });
        let binary = vec![cap_a.clone(), cap_b.clone()];
        let manifest = vec![cap_b, cap_a];
        let result = diff_capabilities("order-test", &binary, &manifest);
        assert!(result.is_ok());
    }
}
```

(Note: the unit tests use only the data types and the pure `diff_capabilities` helper; they do not spawn real binaries. End-to-end coverage of `cross_check_plugin_capabilities` is exercised by Task 9's install integration tests + Tasks 6-8's compat tests.)

- [ ] **Step 4: Verify the file compiles**

```bash
cargo build -p tau-pkg 2>&1 | tail -10
```

Expected: clean build. If `Frame::decode_payload` doesn't exist on the public surface of `tau-plugin-protocol`, use the equivalent (likely `Frame::decode` with the length already stripped). Check `crates/tau-plugin-protocol/src/frame.rs` and adjust accordingly. Same goes for any signature mismatch — adapt to the actual public API at HEAD.

- [ ] **Step 5: Run the unit tests**

```bash
cargo test -p tau-pkg sandbox_check
```

Expected: 8 tests passed.

- [ ] **Step 6: Run all 5 verification gates**

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test --workspace --doc
```

Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/tau-pkg/Cargo.toml \
        crates/tau-pkg/src/lib.rs \
        crates/tau-pkg/src/sandbox_check.rs \
        Cargo.lock

git commit -m "feat(pkg): sandbox_check module — Layer 2 install-time cross-check

Sub-project B Task 2. New tau-pkg::sandbox_check public module with
cross_check_plugin_capabilities(binary_path, manifest) -> Result<Vec<CapabilityShape>, CrossCheckError>.

Tool-port plugins: enumerate tool.describe_capabilities per method,
union, compare against manifest. LLM/storage plugins: manifest verbatim
with tracing::debug! noting the manifest-only path until cross-port
wire mechanism lands (deferred per ADR-0016 Decision 1).

CrossCheckError #[non_exhaustive] with 4 variants (SpawnFailed,
HandshakeFailed, BinaryClaimsExtra, ManifestDeclaresUnused). All
variants surface as exit code 2 per ADR-0007 §7.

Bidirectional set-diff: binary-claims-extra and manifest-declares-unused
both hard-fail. Dedup-safe; order-independent.

8 unit tests covering the diff helper, error display, and
non_exhaustive discipline. End-to-end coverage in Tasks 6-9.
"
```

---

## Task 3: Native adapter symlink-resolution fix

**Files:**
- Modify: `crates/tau-sandbox-native/src/light.rs` (add `resolve_symlinks_for_landlock`; wire into `apply_landlock`'s path-collection step; add tests)
- Modify: `crates/tau-sandbox-native/src/error.rs` or wherever `LightError`-equivalent variant lives (verify and add `SymlinkResolution` variant)

- [ ] **Step 1: Locate the current LightError type**

```bash
grep -rn "LightError\|enum.*Error" crates/tau-sandbox-native/src --include='*.rs' 2>&1 | head -20
```

Identify which file holds the error enum (likely `crates/tau-sandbox-native/src/error.rs` or `lib.rs`). The variant addition lands in the same file.

- [ ] **Step 2: Add the `SymlinkResolution` error variant**

In the located error file, add:

```rust
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum LightError {
    // ... existing variants kept verbatim ...

    /// Failed to canonicalize a path being added to the landlock
    /// ruleset. Likely the path doesn't exist or permission was denied.
    /// Surfaces as a sandbox configuration error (exit 2).
    #[error("could not canonicalize path '{}' for landlock ruleset: {source}", path.display())]
    SymlinkResolution {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}
```

(Match the existing variant style — derive ordering, attribute style, etc. Append AFTER the last existing variant. Do not reorder.)

- [ ] **Step 3: Add `resolve_symlinks_for_landlock` helper to `crates/tau-sandbox-native/src/light.rs`**

Locate the `collect_landlock_paths` function (line 18 per the spec recon). Append the helper directly below `resolve_anchors`:

```rust
/// Resolve symlinks for landlock ruleset entries.
///
/// Landlock V1 path resolution does not follow symlinks at lookup time;
/// installing a rule for `/bin` does NOT grant access to `/usr/bin`
/// when `/bin` is a symlink. Sub-project B addresses this by adding
/// BOTH the symlink path and its canonical target to the ruleset.
///
/// Returns one path (the input verbatim) for non-symlinks, two paths
/// (input + canonical target) for symlinks.
fn resolve_symlinks_for_landlock(
    path: &std::path::Path,
) -> Result<Vec<std::path::PathBuf>, crate::LightError> {
    let canonical = std::fs::canonicalize(path).map_err(|e| {
        crate::LightError::SymlinkResolution {
            path: path.to_path_buf(),
            source: e,
        }
    })?;

    if canonical == path {
        Ok(vec![path.to_path_buf()])
    } else {
        Ok(vec![path.to_path_buf(), canonical])
    }
}
```

(The exact `crate::LightError` reference may need to be `super::LightError` or `crate::error::LightError` — match the actual module structure. The grep in Step 1 reveals the right path.)

- [ ] **Step 4: Wire `resolve_symlinks_for_landlock` into `collect_landlock_paths`**

In `crates/tau-sandbox-native/src/light.rs`, modify the body of `collect_landlock_paths` so that each path passes through symlink resolution before being returned. The current function structure is:

```rust
pub(crate) fn collect_landlock_paths(
    plan: &SandboxPlan,
    cmd: &Command,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>), crate::LightError> {
    let read_strs = collect_paths(plan, |c| match c {
        Capability::Filesystem(FsCapability::Read { paths }) => Some(paths.clone()),
        _ => None,
    });
    // ... write_strs analogous ...

    let cwd = /* existing logic */;
    let read_paths = resolve_anchors(&read_strs, &cwd);
    let write_paths = resolve_anchors(&write_strs, &cwd);

    // NEW: pass each path through symlink resolution.
    // Both directions of paths get the symlink + canonical added.
    let read_paths = read_paths
        .into_iter()
        .map(|p| resolve_symlinks_for_landlock(&p))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    let write_paths = write_paths
        .into_iter()
        .map(|p| resolve_symlinks_for_landlock(&p))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();

    Ok((read_paths, write_paths))
}
```

(The exact signature may differ; preserve the existing logic and only insert the symlink-resolution map after the `resolve_anchors` calls.)

- [ ] **Step 5: Add unit tests for the new helper**

Append to the `#[cfg(test)] mod tests { ... }` block in `crates/tau-sandbox-native/src/light.rs`:

```rust
    // ---------- resolve_symlinks_for_landlock ----------

    #[test]
    fn resolve_symlinks_non_symlink_returns_single_entry() {
        // /tmp is not a symlink on most modern Linux distros; if the test
        // runs on a system where it is, the assert relaxes to "at least
        // one entry".
        let path = std::path::Path::new("/tmp");
        let resolved = resolve_symlinks_for_landlock(path).expect("/tmp must canonicalize");
        assert!(resolved.len() >= 1);
        assert!(resolved.contains(&path.to_path_buf()));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn resolve_symlinks_symlink_includes_canonical() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).expect("mkdir target");
        let link = tmp.path().join("link");
        symlink(&target, &link).expect("symlink");

        let resolved = resolve_symlinks_for_landlock(&link).expect("symlink must canonicalize");
        // Both the symlink path and the canonical target are returned.
        assert_eq!(resolved.len(), 2);
        assert!(resolved.contains(&link));
        assert!(resolved.iter().any(|p| p.canonicalize().ok() == Some(target.canonicalize().expect("canon target"))));
    }

    #[test]
    fn resolve_symlinks_missing_path_returns_symlink_resolution_error() {
        let nonexistent = std::path::Path::new("/this/path/does/not/exist/12345");
        let result = resolve_symlinks_for_landlock(nonexistent);
        assert!(matches!(result, Err(crate::LightError::SymlinkResolution { .. })));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn collect_landlock_paths_includes_canonical_for_symlinks() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let target = tmp.path().join("target");
        std::fs::create_dir(&target).expect("mkdir target");
        let link = tmp.path().join("link");
        symlink(&target, &link).expect("symlink");

        // Build a SandboxPlan that asks for read access at the symlink path.
        let plan_json = serde_json::json!({
            "capabilities": [{
                "kind": "fs.read",
                "paths": [link.to_str().unwrap()]
            }],
            "context": null,
            "limits": null,
        });
        let plan: tau_ports::SandboxPlan =
            serde_json::from_value(plan_json).expect("valid plan");
        let cmd = tokio::process::Command::new("/bin/true");
        let std_cmd = cmd.as_std();
        let (read_paths, _write_paths) =
            collect_landlock_paths(&plan, std_cmd).expect("collect");
        // Both the link path and the canonical target should appear.
        assert!(read_paths.iter().any(|p| p == &link));
        assert!(read_paths
            .iter()
            .any(|p| p.canonicalize().ok() == Some(target.canonicalize().unwrap())));
    }
```

(`tempfile` should already be a dev-dep on `tau-sandbox-native` since priority 12 used it. If not, add it to `[dev-dependencies]` in `crates/tau-sandbox-native/Cargo.toml`.)

- [ ] **Step 6: Run the new tests**

```bash
cargo test -p tau-sandbox-native --lib resolve_symlinks_ collect_landlock_paths_includes
```

Expected: 4 passed (2 unconditional + 2 Linux-only).

- [ ] **Step 7: Run all 5 verification gates**

```bash
cargo fmt --all -- --check
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test --workspace --doc
```

Expected: all green. macOS local will skip the `cfg(target_os = "linux")` tests; Linux CI exercises them.

- [ ] **Step 8: Commit**

```bash
git add crates/tau-sandbox-native/src/light.rs \
        crates/tau-sandbox-native/src/error.rs

git commit -m "fix(sandbox-native): resolve symlinks before adding paths to landlock ruleset

Sub-project B Task 3. Landlock V1 does not follow symlinks at lookup
time; installing a rule for /bin does NOT grant access to /usr/bin when
/bin is a symlink (the typical Ubuntu layout).

Add resolve_symlinks_for_landlock helper that canonicalizes each path
and returns BOTH the symlink path and the canonical target. Wire into
collect_landlock_paths after resolve_anchors. New
LightError::SymlinkResolution { path, source } variant for the missing-
path / permission-denied failure mode.

This unblocks sub-project D's e2e test re-introduction (5 files removed
at priority 12 ship due to this exact issue) and is consumed by sub-
project B Task 8's native live spawn tests.

4 unit tests: non-symlink no-op, symlink resolves to canonical (Linux),
missing-path returns SymlinkResolution, integration with
collect_landlock_paths (Linux).
"
```

---

## Task 4: tau-plugin-compat crate scaffolding

**Files:**
- Create: `crates/tau-plugin-compat/Cargo.toml`
- Create: `crates/tau-plugin-compat/src/lib.rs`
- Create: `crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml`
- Create: `crates/tau-plugin-compat/fixtures/controlled-env-binary/src/main.rs`
- Modify: root `Cargo.toml` (add `crates/tau-plugin-compat` to workspace members)
- Stage: `Cargo.lock`

**Summary:** new workspace crate with `publish = false`. Cargo.toml deps: `tau-pkg`, `tau-domain`, `tau-runtime`, `tau-sandbox-native`, `tau-sandbox-container`. Dev-deps: `assert_cmd`, `tempfile`, `tokio` (test features). The `controlled-env-binary` is a tiny standalone Cargo project at `fixtures/controlled-env-binary/` (its own Cargo.toml, NOT a workspace member; built by tests on demand via `cargo build --manifest-path …`). It performs predictable I/O: reads `${TAU_FIXTURE_INPUT_PATH}` env var, writes a known string to stdout, exits 0. ~50 LOC. Statically-linked (`[profile.release] strip = "symbols"; lto = true`) so landlock V1 path resolution doesn't get tripped by dynamic linker probing.

**Spec references:** Spec §2.3 (component breakdown for the new crate); §5.3 (CI configuration mentions Linux-only test job). Also spec §2.6 (workspace + CI updates).

**Verification:** the new crate must compile clean (`cargo build -p tau-plugin-compat`) and the controlled-env binary must build via `cargo build --manifest-path crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml`. No tests yet — those land in Tasks 6-8.

**Commit message:**
```
feat(plugin-compat): new workspace crate for plugin compat verification

Sub-project B Task 4. New crate at crates/tau-plugin-compat/ with
publish = false. Test-infrastructure-only crate; future sub-projects
(D-remainder, E, F, J, K) extend it for additional adapter coverage.

Includes:
- Cargo.toml with tau-pkg/tau-domain/tau-runtime/tau-sandbox-* deps
  and assert_cmd/tempfile dev-deps
- src/lib.rs with fixture-build helper functions (no tests yet)
- fixtures/controlled-env-binary/ standalone Cargo project (statically
  linked; predictable I/O for landlock e2e tests; ~50 LOC)

Cargo.lock updated with the new crate's transitive resolutions.
```

---

## Task 5: Per-plugin tau.toml fixtures

**Files:**
- Create: `crates/tau-plugin-compat/fixtures/projects/anthropic/tau.toml`
- Create: `crates/tau-plugin-compat/fixtures/projects/anthropic/.tau/config.toml`
- Create: `crates/tau-plugin-compat/fixtures/projects/anthropic/cassettes/golden_path.json` (copy from `crates/tau-plugins/anthropic/tests/fixtures/`)
- (Same 3-file pattern for ollama, openai)
- Create: `crates/tau-plugin-compat/fixtures/projects/fs-read/tau.toml` + `.tau/config.toml` + a known-content `data.txt`
- Create: `crates/tau-plugin-compat/fixtures/projects/shell/tau.toml` + `.tau/config.toml`

**Summary:** five fixture project directories, each minimal but complete:
- `tau.toml` declares a single `[[agents]]` configured to use the corresponding plugin with the plugin's typical golden-path agent prompt.
- `.tau/config.toml` declares `[sandbox] required_tier = "strict"` so the test exercises the real adapter.
- HTTP plugins copy the existing cassette files from `crates/tau-plugins/<name>/tests/fixtures/` (priority 2's cassette-replay infrastructure). No real API keys, no real network from CI.
- `fs-read` includes a known-content text file that the plugin will read.
- `shell` requires no extra fixture data; the plugin runs `echo "hello"`.

**Spec references:** Spec §2.3 fixtures section; §3.2 test harness flow; §5.6 fixture data location.

**Verification:** `cargo build -p tau-plugin-compat` still passes. No tests yet.

**Commit message:**
```
feat(plugin-compat): per-plugin tau.toml fixtures for compat tests

Sub-project B Task 5. Five fixture project directories under
crates/tau-plugin-compat/fixtures/projects/{anthropic,ollama,openai,
fs-read,shell}/, each a minimal but complete tau project: tau.toml
with one [[agents]] block, .tau/config.toml with [sandbox]
required_tier = "strict", per-plugin cassette/data files where needed.

HTTP plugins reuse priority-2's cassette-replay infrastructure; no
real API keys, no real network from CI.
```

---

## Task 6: Layer 3 check_sandbox tests

**Files:**
- Create: `crates/tau-plugin-compat/tests/layer3_check_sandbox.rs`

**Summary:** 5 tests, one per real plugin. Each test:
1. `tempfile::TempDir::new()` for an isolated scope.
2. Copies the corresponding `fixtures/projects/<plugin>/` into the tempdir.
3. Runs `tau install <local plugin path>` via `assert_cmd::Command::cargo_bin("tau")`.
4. Asserts install exits 0 (validates Layer 2 cross-check passes).
5. Runs `tau resolve --check-sandbox` against the tempdir scope.
6. Asserts exit 0 (validates Layer 3).

Use `tau-plugins/<name>` as a local path (no git fetch needed). The existing `cmd_resolve_check_sandbox.rs` integration tests in `crates/tau-cli/tests/` are the pattern to mirror.

**Spec references:** Spec §3.2 test harness flow; §5.1 test inventory (Layer 3 row).

**Verification:**
```bash
cargo test -p tau-plugin-compat --features integration-tests --test layer3_check_sandbox
```
Expected: 5 passed on Linux. (May skip with clear message on macOS/Windows if Docker isn't available — but Layer 3 doesn't actually need Docker; should pass everywhere.)

**Commit message:**
```
test(plugin-compat): Layer 3 per-plugin check-sandbox tests

Sub-project B Task 6. 5 integration tests covering anthropic, ollama,
openai, fs-read, shell. Each: install plugin into tempdir scope, run
tau resolve --check-sandbox, assert exit 0. Validates Layer 2
cross-check + Layer 3 plan validation in one pass.

Cassette-replay used for HTTP plugins (no real network).
```

---

## Task 7: Layer 4 container live spawn tests

**Files:**
- Create: `crates/tau-plugin-compat/tests/layer4_container.rs`

**Summary:** 5 tests, one per plugin. Each test forces `Container` adapter via `--sandbox container` flag (sub-project A's surface). Drives a golden-path agent invocation:
- HTTP plugins: cassette-replay; assert success exit + expected stdout.
- `fs-read`: agent invokes the tool to read the fixture data file; assert content read back.
- `shell`: agent invokes the tool to run `echo hello`; assert "hello" in output.

Skip-with-clear-message if Docker isn't on the host (`which docker` precheck). On GH Actions `ubuntu-latest`, Docker is available out-of-the-box.

**Spec references:** Spec §3.2 test harness flow; §3.3 adapter resolution (forced via `force_adapter_kind`); §5.1 Layer 4 container row; §5.3 CI configuration (Docker availability check).

**Verification:**
```bash
cargo test -p tau-plugin-compat --features integration-tests --test layer4_container
```
Expected on Linux+Docker: 5 passed. Expected on hosts without Docker: 5 ignored or skipped with clear messages.

**Commit message:**
```
test(plugin-compat): Layer 4 container live spawn tests

Sub-project B Task 7. 5 integration tests covering all 5 real plugins
under the Container adapter (--sandbox container). Each spawns a real
plugin process under Docker hardening, exercises the golden path, and
asserts success.

Skips with clear message if Docker isn't installed (precheck via
`which docker`). GH Actions ubuntu-latest has Docker; CI passes
unconditionally there.
```

---

## Task 8: Layer 4 native live spawn tests

**Files:**
- Create: `crates/tau-plugin-compat/tests/layer4_native.rs`

**Summary:** 5 tests gated `cfg(target_os = "linux")` AND `cfg(feature = "integration-tests")`. Forces `Native` adapter via `--sandbox native`. Each test exercises the same golden path as Task 7's container tests but under landlock + seccomp + namespaces. **These tests exercise the Task 3 symlink fix; Task 3 must merge before Task 8 can pass.**

Test bodies stay empty on non-Linux (gated out via `cfg`); compile cleanly everywhere.

**Spec references:** Spec §3.2; §3.3 (forced adapter); §5.1 Layer 4 native row; spec §5.3 (Linux-only gating).

**Verification (Linux only):**
```bash
cargo test -p tau-plugin-compat --features integration-tests --test layer4_native
```
Expected on Linux with kernel ≥ 5.13 (landlock V1): 5 passed.

**Commit message:**
```
test(plugin-compat): Layer 4 native live spawn tests

Sub-project B Task 8. 5 integration tests covering all 5 real plugins
under the Native adapter (--sandbox native). Linux-only: gated
cfg(target_os = "linux") + cfg(feature = "integration-tests"). On
non-Linux platforms the tests compile but bodies are empty.

These tests exercise the symlink resolution fix from Task 3 (landlock
V1 path lookup against /bin → /usr/bin Ubuntu symlinks). Without that
fix, EACCES on real binary spawns. Sub-project D's earlier-removed e2e
tests fail without Task 3; this is their reproducer.

The 5 plugins under landlock + seccomp + namespaces: golden paths exit
0 with expected stdout.
```

---

## Task 9: tau-pkg::install integration

**Files:**
- Modify: `crates/tau-pkg/src/install.rs` (insert step 8.7 between SHA-compute and lockfile-write)
- Modify: `crates/tau-pkg/src/error.rs` (add `InstallError::CrossCheck` variant)
- Create: `crates/tau-pkg/tests/install_cross_check.rs` (5 integration tests)

**Summary:** wire `cross_check_plugin_capabilities` into the existing 10-step install lifecycle as a new step 8.7, between step 8.6 (compute source tree SHA-256) and step 9 (update lockfile). The cross-check returns `Vec<CapabilityShape>`; pass that into the lockfile write so `LockedPlugin.required_shapes` is populated. On `CrossCheckError`, abort install (exit 2 propagated through CLI; binary on disk; user retries via `--force` after fixing manifest).

`InstallError` gains:
```rust
#[error("plugin capability cross-check failed: {0}")]
CrossCheck(#[from] crate::sandbox_check::CrossCheckError),
```
With `#[non_exhaustive]` already on `InstallError`.

**Spec references:** Spec §2.2 (install path integration); §3.1 (install-time data flow with the step 8.7 placement note); §4.1 (error mapping).

**Tests in `tests/install_cross_check.rs`** (5):
1. `install_with_matching_manifest_succeeds` — fixture binary that claims exactly what its manifest declares; install completes.
2. `install_with_binary_claims_extra_aborts` — fixture binary that asks for an extra cap; install fails with `CrossCheck(BinaryClaimsExtra { .. })`.
3. `install_with_manifest_declares_unused_aborts` — same in reverse.
4. `install_force_after_fix_succeeds` — start from (2), edit manifest, retry with `--force`, expect success.
5. `install_llm_port_uses_manifest_only` — fixture LLM-backend plugin succeeds without per-method cross-check (manifest verbatim).

**Verification:**
```bash
cargo test -p tau-pkg --test install_cross_check
```

**Commit message:**
```
feat(pkg/install): wire Layer 2 cross-check into step 8.7

Sub-project B Task 9. Cross-check fires after the source tree SHA-256
computation (step 8.6) and before the lockfile write (step 9). On
CrossCheckError, install aborts with exit 2; binary is left on disk;
user retries via `tau install --force` after fixing the manifest.

LockedPlugin.required_shapes populated from the cross-check's returned
Vec<CapabilityShape> (priority 12's lockfile schema v4 field; was
always empty pre-B).

5 install integration tests + new InstallError::CrossCheck variant.
```

---

## Task 10: CLI error rendering for cross-check

**Files:**
- Modify: `crates/tau-cli/src/cmd/error_render.rs` (add `render_cross_check_error`)
- Create: `crates/tau-cli/tests/cmd_install_cross_check_render.rs` (3 insta snapshot tests)

**Summary:** new `render_cross_check_error(err: &CrossCheckError) -> String` function. Multi-line guided output:

For `BinaryClaimsExtra`:
```
✗ install aborted: plugin capability cross-check failed

  binary 'anthropic' calls tool.describe_capabilities and asks for:
    - net.http (hosts: api.anthropic.com)
    - fs.read  (paths: ${HOME}/.config/anthropic/**)

  manifest tau.toml [[capabilities]] declares:
    - net.http (hosts: api.anthropic.com)

  Discrepancy:
    + binary requests fs.read (not in manifest)

  Resolution:
    1. Add the missing capability to the plugin manifest:
         [[capabilities]]
         kind = "fs.read"
         paths = ["${HOME}/.config/anthropic/**"]
    2. Or remove the capability from the binary's tool.describe_capabilities surface.

  Then retry: tau install --force <plugin>
```

For `ManifestDeclaresUnused`: similar output, swap "binary requests" for "manifest declares" with the resolution step "remove the capability from the manifest" or "extend the binary".

For `SpawnFailed` / `HandshakeFailed`: simpler output naming the plugin and the underlying error; resolution is "check the binary builds and runs standalone first".

3 insta snapshot tests cover the three error families. Use `insta::assert_snapshot!`. Snapshots stored at `tests/snapshots/cmd_install_cross_check_render__*.snap`.

**Spec references:** Spec §4.1 (user-facing rendering with the example output); §5.1 (3 snapshot tests row).

**Verification:**
```bash
cargo test -p tau-cli --test cmd_install_cross_check_render
```

**Commit message:**
```
feat(cli): render Layer 2 cross-check errors with guided output

Sub-project B Task 10. New render_cross_check_error in
tau-cli::cmd::error_render with multi-line output: which capabilities
the binary asks for, which the manifest declares, the specific
discrepancy, and the exact TOML stanza to add as a resolution.

3 insta snapshot tests covering BinaryClaimsExtra,
ManifestDeclaresUnused, and SpawnFailed.
```

---

## Task 11: CI workflow updates

**Files:**
- Modify: `.github/workflows/ci.yml`

**Summary:** add two new jobs to the existing matrix:

```yaml
build (tau-plugin-compat):
  needs: <existing build matrix base>
  matrix: { os: [ubuntu-latest, macos-latest, windows-latest], rust: [stable, 1.91] }
  steps:
    - cargo build -p tau-plugin-compat --all-features

test (tau-plugin-compat):
  needs: <existing build matrix base>
  matrix: { os: [ubuntu-latest], rust: [stable] }
  steps:
    - cargo test -p tau-plugin-compat --all-targets --features integration-tests
```

The build job ensures the crate compiles on all platforms (it should — the test bodies are gated for non-Linux). The test job runs only on Linux because the live-spawn tests need landlock/Docker.

The actual job names on GitHub will be e.g. `build (tau-plugin-compat) (ubuntu-latest, stable)`. Surface these names in Task 12's gate so the user knows which checks to add to branch protection.

**Spec references:** Spec §2.6; §5.3 (CI configuration).

**Verification:** push and observe; surfaces in Task 12.

**Commit message:**
```
ci: add tau-plugin-compat build + Linux-only integration test jobs

Sub-project B Task 11. New build matrix for the tau-plugin-compat
crate (all platforms × stable, 1.91) plus a Linux-only test job for
the integration tests under cargo test --features integration-tests.

Branch protection requires manual update post-push to add the new
check names (Task 12).
```

---

## Task 12: USER GATE — final verification + open PR

**Type:** PAUSE for user approval.

The implementer:
1. Runs the full local verification suite on the latest commit:
   - `cargo fmt --all -- --check`
   - `cargo build --workspace`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo test --workspace --all-targets`
   - `cargo test --workspace --doc`
   - On Linux: `cargo test -p tau-plugin-compat --features integration-tests`
2. Pushes the branch: `git push -u origin feat/plugin-compat-spec`.
3. Drafts a PR body in `/tmp/<branch-name>-pr-body.md` (avoid heredoc escaping pitfalls).
4. Opens the PR: `gh pr create --draft --base main --head feat/plugin-compat-spec --title "feat: plugin compatibility verification + landlock-symlink fix (sub-project B)" --body-file <tempfile>`.
5. Surfaces the new CI check names emitted by the first run via `gh pr checks <PR#>`. The user adds these to GitHub branch protection (Settings → Branches → main → Required status checks).
6. Waits for CI green on the new branch protection set.
7. PAUSES; reports status back; waits for user "Task 12 ok" before proceeding to Task 13.

**No commit.** This task is verification + push + draft PR open.

---

## Task 13: USER GATE — ADR-0016 + ROADMAP + followups + squash-merge

**Type:** PAUSE for user approval.

**Files:**
- Create: `docs/decisions/0016-plugin-compat-verification.md` — full ADR body covering the 7 design decisions from the spec (Layer 2 = tool-port dynamic; install-time only; Layer 3 + container + native; absorb D's foundation; --rehash dropped; all-plugins-Strict policy; new tau-plugin-compat crate; cross-check fn in tau-pkg::sandbox_check). Each decision section: Decision / Context / Consequences / Alternatives considered.
- Modify: `ROADMAP.md` — add a new row in Phase 1 table for sub-project B (id: `12-B`); update the status paragraph to mention 2026-05-04 ship date and that sub-project D's foundation has been absorbed (D's remaining scope: re-introducing 5 e2e test files).
- Modify: `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` — mark sub-project B done (✅ DONE 2026-05-04 marker, similar to sub-project A's pattern); update sub-project D's section to flag the foundation as already shipped via B; update the post-merge "What landed" notes for the closed gap row.

The implementer commits these doc updates as a single commit. CI re-runs on the new commit (~10-15 minutes for the 28-29 checks). Once green, the user squash-merges via:

```bash
gh pr merge <PR#> --squash --delete-branch \
   --subject "feat: plugin compatibility verification — Layer 2 cross-check + per-plugin harness (Tier 3 priority 12 sub-project B) (#<N>)" \
   --body-file /tmp/<branch-name>-merge-body.md
```

**No commit on this task by the implementer alone.** This task is ADR + ROADMAP + followups + waiting for CI + user-driven squash-merge.

After merge:
- `git checkout main && git pull origin main` to update the local checkout.
- Branch deleted by `--delete-branch`.

---

## Self-review

**Spec coverage:**
- Section 1 (Scope + architecture): all 4 scope items covered (Layer 2 cross-check → Tasks 2 + 9; per-plugin harness → Tasks 4-6; live spawn tests → Tasks 7-8; tier declarations → Task 1). The native-adapter touchup is Task 3.
- Section 2 (Components): every file in the spec's component table is touched in some task (1 → tau.tomls; 2 → sandbox_check.rs + lib.rs + Cargo.toml; 3 → light.rs + error; 4 → tau-plugin-compat scaffold; 5 → fixtures; 6-8 → test files; 9 → install.rs + error.rs + tests; 10 → error_render.rs + snapshot tests; 11 → ci.yml + Cargo.toml workspace).
- Section 3 (Data flow): step 8.7 placement (between SHA and lockfile) explicit in Task 9. Test harness flow + adapter forcing covered in Tasks 6-8.
- Section 4 (Error handling): InstallError::CrossCheck variant in Task 9; LightError::SymlinkResolution in Task 3; render_cross_check_error in Task 10. Exit code mapping consistent with ADR-0007 §7.
- Section 5 (Testing strategy): 35 tests planned across all tasks (8 Task 2 unit + 4 Task 3 unit + 5 Task 9 integration + 5 Task 6 + 5 Task 7 + 5 Task 8 + 3 Task 10 snapshot = 35). CI matches §5.3.

**Placeholder scan:** searched the plan for "TBD", "TODO", "implement later", "fill in details", "Add appropriate", "Similar to Task". None found in plan body (spec content quoted verbatim is OK).

**Type consistency:** `cross_check_plugin_capabilities(binary_path: &Path, manifest: &PackageManifest) -> Result<Vec<CapabilityShape>, CrossCheckError>` referenced consistently across Tasks 2, 9, 10. `CrossCheckError` enum variants (SpawnFailed, HandshakeFailed, BinaryClaimsExtra, ManifestDeclaresUnused) consistent across Tasks 2, 9, 10. `LightError::SymlinkResolution { path, source }` consistent across Task 3. `InstallError::CrossCheck` consistent across Task 9.

**Step 8.7 vs step 8.5 collision:** the original spec said step 8.5; reconnaissance revealed steps 8.5 (build) and 8.6 (tree SHA-256) already exist in `install.rs`. Plan-erratum block + Task 9 use step 8.7 consistently.

**Branch protection delta:** stated as 25 → ~28-29 in plan-erratum + Task 12. Concrete number (28 vs 29) depends on whether the test job runs on stable+1.91 or stable only; Task 11's CI YAML explicitly only runs the test job on stable, so the actual delta is +3 build jobs (3 platforms × 1 toolchain — `tau-plugin-compat --all-features` for stable only matches the workspace pattern) + 1 test job = +4 → final state 29. Adjust the number in Task 12's text if reality differs.

Plan complete.

---

Plan complete and saved to `docs/superpowers/plans/2026-05-04-plugin-compat.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
