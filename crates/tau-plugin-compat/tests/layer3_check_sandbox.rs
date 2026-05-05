//! Layer 3 per-plugin compat tests — sub-project B Task 6.
//!
//! For each of the 5 real shipped plugins, write a synthetic install
//! state (real plugin manifest + lockfile entry + project tau.toml +
//! scope config with strict tier), then run `tau resolve --check-sandbox`
//! and assert exit 0.
//!
//! These tests verify Layer 3 only — `--check-sandbox` is read-only and
//! does NOT spawn the plugin binaries. Layer 4 (real-spawn) coverage
//! lives in `tests/layer4_container.rs` and `tests/layer4_native.rs`.
//!
//! Tests skip gracefully if no strict-capable sandbox adapter is available
//! on the host (e.g. macOS without Docker).

#![cfg(feature = "integration-tests")]

use std::path::Path;

use tempfile::TempDir;

const SCHEMA_V3_HEADER: &str = "schema_version = 3\nkind = \"project\"\ncreated_at = \"2026-05-04T00:00:00Z\"\ncreated_by_tau_version = \"0.0.0\"\n";

fn write_scope_config(scope_root: &Path) {
    let dot_tau = scope_root.join(".tau");
    std::fs::create_dir_all(&dot_tau).unwrap();
    std::fs::write(
        dot_tau.join("config.toml"),
        format!("{SCHEMA_V3_HEADER}\n[sandbox]\nrequired_tier = \"strict\"\n"),
    )
    .unwrap();
}

fn write_project_tau_toml_llm(scope_root: &Path, plugin: &str) {
    let body = format!(
        r#"[project]
name = "compat-test-{plugin}"

[agents.tester]
display_name = "{plugin} compat test agent"
package      = "{plugin}@^0.1"
llm_backend  = "{plugin}"
"#
    );
    std::fs::write(scope_root.join("tau.toml"), body).unwrap();
}

fn write_project_tau_toml_tool(scope_root: &Path, plugin: &str) {
    let body = format!(
        r#"[project]
name = "compat-test-{plugin}"

[agents.tester]
display_name = "{plugin} compat test agent"
package      = "demo@^0.1"
llm_backend  = "anthropic"

[[agents.tester.requires.tools]]
name    = "{plugin}"
source  = "https://example.com/{plugin}.git"
"#
    );
    std::fs::write(scope_root.join("tau.toml"), body).unwrap();
}

/// Strip top-level `name = `, `version = `, and `description = ` lines from a
/// plugin manifest, leaving the `[plugin]`, `[[capabilities]]`, and `[sandbox]`
/// blocks intact for use in a synthesized `PackageManifest`.
fn strip_top_level_metadata(plugin_toml: &str) -> String {
    plugin_toml
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("name =")
                && !trimmed.starts_with("name=")
                && !trimmed.starts_with("version =")
                && !trimmed.starts_with("version=")
                && !trimmed.starts_with("description =")
                && !trimmed.starts_with("description=")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Copy the REAL plugin manifest from `crates/tau-plugins/<plugin>/tau.toml`
/// and write a synthesized `PackageManifest`-compatible file into the
/// synthetic install tree at `<scope>/.tau/packages/<plugin>/<version>/tau.toml`.
fn install_synthetic_plugin(scope_root: &Path, plugin: &str, version: &str) {
    let pkg_dir = scope_root
        .join(".tau")
        .join("packages")
        .join(plugin)
        .join(version);
    std::fs::create_dir_all(&pkg_dir).unwrap();

    // Resolve <repo>/crates/tau-plugins/<plugin>/tau.toml relative to this
    // crate's manifest dir (crates/tau-plugin-compat/).
    let plugin_toml_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tau-plugins")
        .join(plugin)
        .join("tau.toml");

    let manifest_body = std::fs::read_to_string(&plugin_toml_src)
        .unwrap_or_else(|e| panic!("read {}: {e}", plugin_toml_src.display()));

    let pkg_kind = if matches!(plugin, "anthropic" | "ollama" | "openai") {
        "llm-backend"
    } else {
        "tool"
    };

    // Build a synthesized full PackageManifest by prepending the fields that
    // the thin plugin tau.toml lacks (`authors`, `source`, `kind`,
    // `dependencies`) and appending the rest of the plugin manifest (which
    // already contains `[plugin]`, `[[capabilities]]`, and `[sandbox]`).
    let synthesized = format!(
        r#"name = "{plugin}"
version = "{version}"
description = "Real plugin manifest from crates/tau-plugins/{plugin}/tau.toml"
authors = []
source = "https://example.com/{plugin}.git"
kind = "{pkg_kind}"
dependencies = []

{rest}
"#,
        rest = strip_top_level_metadata(&manifest_body)
    );

    std::fs::write(pkg_dir.join("tau.toml"), synthesized).unwrap();
}

fn write_lockfile(scope_root: &Path, plugin: &str, version: &str) {
    let zero_sha = "0".repeat(40);
    let now = "2026-05-04T00:00:00Z";

    let provides = if matches!(plugin, "anthropic" | "ollama" | "openai") {
        "llm_backend"
    } else {
        "tool"
    };

    let bin = format!("{plugin}-plugin");

    let lockfile = format!(
        r#"schema_version = 4
generated_by_tau_version = "0.0.0"
generated_at = "{now}"

[[package]]
name = "{plugin}"
active_version = "{version}"
source = "https://example.com/{plugin}.git"

[[package.versions]]
version = "{version}"
resolved_commit = "{zero_sha}"
sha256 = ""
installed_at = "{now}"

[package.plugin]
binary_path = "/nonexistent/{plugin}"
built_at = "{now}"

[package.plugin.manifest]
provides = "{provides}"
kind = "rust-cargo"
bin = "{bin}"
"#
    );
    std::fs::write(scope_root.join("tau-lock.toml"), lockfile).unwrap();
}

/// Locate the `tau` binary.
///
/// Cargo sets `CARGO_BIN_EXE_tau` only when the test crate itself declares
/// the `tau` binary (i.e. in `tau-cli`'s own integration tests). For
/// cross-crate integration tests like these, we locate the binary via the
/// workspace-root-relative target path instead.
///
/// Resolution order:
/// 1. `CARGO_BIN_EXE_tau` — set by cargo when the binary is part of the
///    current compilation unit (e.g. in `tau-cli`'s own tests). This path
///    is used first so that running `-p tau-plugin-compat` in a workspace
///    `cargo test --all` invocation picks up the already-compiled artifact.
/// 2. `$CARGO_TARGET_DIR/debug/tau` — used when `CARGO_TARGET_DIR` is set
///    by the caller (our CLAUDE.md rule: `CARGO_TARGET_DIR=target/agent-*`).
/// 3. `$CARGO_MANIFEST_DIR/../../target/debug/tau` — workspace-root
///    fallback for `cargo test -p tau-plugin-compat` without an explicit
///    target dir override.
fn tau_bin() -> std::path::PathBuf {
    // 1. Direct env var (set when tau binary is in the current build unit).
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_tau") {
        return std::path::PathBuf::from(p);
    }

    // 2. CARGO_TARGET_DIR override (our CLAUDE.md CARGO rule).
    if let Ok(target_dir) = std::env::var("CARGO_TARGET_DIR") {
        let candidate = std::path::Path::new(&target_dir).join("debug").join("tau");
        if candidate.exists() {
            return candidate;
        }
        // Also try the absolute path form (CARGO_TARGET_DIR may be relative).
        let abs = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(&target_dir)
            .join("debug")
            .join("tau");
        if abs.exists() {
            return abs;
        }
    }

    // 3. Workspace-root default target dir.
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    workspace_root.join("target").join("debug").join("tau")
}

/// Run `tau resolve --check-sandbox` against the given scope directory and
/// return the process output.
fn run_check_sandbox(scope_root: &Path) -> std::process::Output {
    std::process::Command::new(tau_bin())
        .arg("resolve")
        .arg("--check-sandbox")
        .current_dir(scope_root)
        // Explicitly remove mock injection — these tests use the real adapter
        // resolution path per ADR-0016.
        .env_remove("TAU_TESTING_ALLOW_MOCK_SANDBOX")
        .output()
        .expect("tau resolve --check-sandbox spawn")
}

/// Assert the output is successful, or skip with a clear message if no
/// adapter satisfying `required_tier = "strict"` is available on this host.
fn assert_ok_or_skip(out: &std::process::Output, plugin: &str) {
    if out.status.success() {
        return;
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);

    // On macOS without Docker (and without a Linux native sandbox),
    // the resolver returns "no adapter" or "NoAdapterMatches". Treat
    // those as a graceful skip rather than a test failure.
    if stderr.contains("no adapter")
        || stderr.contains("NoAdapterMatches")
        || stderr.contains("no sandbox adapter available")
        || stderr.contains("no non-permissive adapter available")
        || stdout.contains("no adapter")
        || stdout.contains("NoAdapterMatches")
    {
        eprintln!(
            "SKIP: {plugin} layer3 check skipped — no strict-capable adapter on this host\nstderr: {stderr}"
        );
        return;
    }

    panic!("{plugin} --check-sandbox failed:\nstdout={stdout}\nstderr={stderr}");
}

// ---- Test 1: anthropic -------------------------------------------------------

#[test]
fn anthropic_layer3_check_sandbox_passes() {
    let scope = TempDir::new().unwrap();
    write_scope_config(scope.path());
    write_project_tau_toml_llm(scope.path(), "anthropic");
    install_synthetic_plugin(scope.path(), "anthropic", "0.1.0");
    write_lockfile(scope.path(), "anthropic", "0.1.0");

    let out = run_check_sandbox(scope.path());
    assert_ok_or_skip(&out, "anthropic");
}

// ---- Test 2: ollama ----------------------------------------------------------

#[test]
fn ollama_layer3_check_sandbox_passes() {
    let scope = TempDir::new().unwrap();
    write_scope_config(scope.path());
    write_project_tau_toml_llm(scope.path(), "ollama");
    install_synthetic_plugin(scope.path(), "ollama", "0.1.0");
    write_lockfile(scope.path(), "ollama", "0.1.0");

    let out = run_check_sandbox(scope.path());
    assert_ok_or_skip(&out, "ollama");
}

// ---- Test 3: openai ----------------------------------------------------------

#[test]
fn openai_layer3_check_sandbox_passes() {
    let scope = TempDir::new().unwrap();
    write_scope_config(scope.path());
    write_project_tau_toml_llm(scope.path(), "openai");
    install_synthetic_plugin(scope.path(), "openai", "0.1.0");
    write_lockfile(scope.path(), "openai", "0.1.0");

    let out = run_check_sandbox(scope.path());
    assert_ok_or_skip(&out, "openai");
}

// ---- Test 4: fs-read ---------------------------------------------------------

#[test]
fn fs_read_layer3_check_sandbox_passes() {
    let scope = TempDir::new().unwrap();
    write_scope_config(scope.path());
    write_project_tau_toml_tool(scope.path(), "fs-read");
    install_synthetic_plugin(scope.path(), "fs-read", "0.1.0");
    write_lockfile(scope.path(), "fs-read", "0.1.0");

    let out = run_check_sandbox(scope.path());
    assert_ok_or_skip(&out, "fs-read");
}

// ---- Test 5: shell -----------------------------------------------------------

#[test]
fn shell_layer3_check_sandbox_passes() {
    let scope = TempDir::new().unwrap();
    write_scope_config(scope.path());
    write_project_tau_toml_tool(scope.path(), "shell");
    install_synthetic_plugin(scope.path(), "shell", "0.1.0");
    write_lockfile(scope.path(), "shell", "0.1.0");

    let out = run_check_sandbox(scope.path());
    assert_ok_or_skip(&out, "shell");
}
