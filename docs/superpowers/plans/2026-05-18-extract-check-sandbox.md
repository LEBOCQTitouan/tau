# Extract shared check_sandbox core — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pay down the Path C duplicate flagged in PR #161 by extracting three `pub(crate)` helpers into `crates/tau-cli/src/cmd/resolve_helpers.rs`, then rewriting both call sites to use them.

**Architecture:** Three small helpers — `read_sandbox_requirements_for_check`, `resolve_sandbox_check_adapter`, and `check_plugin_sandbox` returning a `SandboxPluginOutcome` enum. Both callers (`cmd/resolve.rs::run_check_sandbox`, `cmd/check/categories/sandbox.rs::run_sandbox`) shrink to output-mapping-only code. No public API change, no CLI flag change. One intentional behavior change in `tau resolve --check-sandbox` (malformed scope config now falls through to defaults instead of hard-erroring).

**Tech Stack:** Rust 2024 edition, tau-cli (`pub(crate)` boundary), unit tests inline in `resolve_helpers.rs`, regression gate via existing integration tests in `crates/tau-cli/tests/cmd_resolve_check_sandbox.rs` and `cmd_check_subcommands.rs`.

**Spec:** `docs/superpowers/specs/2026-05-18-extract-check-sandbox-design.md`

**Cargo rules (CLAUDE.md):** all cargo invocations from these tasks use `CARGO_TARGET_DIR=target/main`, `CARGO_INCREMENTAL=0`, `-p tau-cli`, wrapped with `timeout 300`. Template:
```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo test -p tau-cli
```

**Worktree:** `~/code/tau-worktrees/extract-check-sandbox`, branch `feat/extract-check-sandbox`, based on `origin/main` at `38c8b23`.

---

### Task 1: Add `SandboxPluginOutcome` enum + `check_plugin_sandbox` helper (TDD)

**Files:**
- Modify: `crates/tau-cli/src/cmd/resolve_helpers.rs` (add enum, helper, and `#[cfg(test)] mod check_sandbox_tests` block at end of file)

- [ ] **Step 1: Write three failing unit tests**

Append to `crates/tau-cli/src/cmd/resolve_helpers.rs`:

```rust
#[cfg(test)]
mod check_sandbox_tests {
    use super::*;
    use std::path::PathBuf;

    /// `MockSandbox` supports the 5 standard CapabilityShapes (fs.read,
    /// fs.write, net.http, exec, env). It is reachable via
    /// `TAU_TESTING_ALLOW_MOCK_SANDBOX=1` through `resolve_adapter`. We
    /// use it here directly to keep tests platform-independent.
    fn mock_adapter() -> tau_runtime::sandbox::SandboxAdapter {
        // Force the Mock branch of resolve_adapter via the env var so the
        // returned SandboxAdapter::Mock variant exists for the test.
        std::env::set_var("TAU_TESTING_ALLOW_MOCK_SANDBOX", "1");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let adapter = rt.block_on(async {
            tau_runtime::sandbox::resolve_adapter(
                &tau_pkg::scope::SandboxRequirements::default(),
                &[],
            )
            .await
            .expect("mock adapter")
        });
        std::env::remove_var("TAU_TESTING_ALLOW_MOCK_SANDBOX");
        adapter
    }

    fn write_manifest(dir: &std::path::Path, body: &str) -> PathBuf {
        let path = dir.join("tau.toml");
        std::fs::write(&path, body).expect("write manifest");
        path
    }

    #[test]
    fn check_plugin_sandbox_ok_for_benign_manifest() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest_path = write_manifest(
            tmp.path(),
            r#"
[package]
name = "test-plugin"
version = "0.1.0"

[[capabilities]]
kind = "fs.read"
paths = ["/tmp/**"]
"#,
        );
        let adapter = mock_adapter();
        let outcome = check_plugin_sandbox("test-plugin", &manifest_path, Some(&adapter));
        assert!(
            matches!(outcome, SandboxPluginOutcome::Ok),
            "expected Ok, got {outcome:?}"
        );
    }

    #[test]
    fn check_plugin_sandbox_manifest_unreadable_for_missing_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest_path = tmp.path().join("nonexistent.toml");
        let adapter = mock_adapter();
        let outcome = check_plugin_sandbox("ghost-plugin", &manifest_path, Some(&adapter));
        match outcome {
            SandboxPluginOutcome::ManifestUnreadable(msg) => {
                assert!(!msg.is_empty(), "expected non-empty error message");
            }
            other => panic!("expected ManifestUnreadable, got {other:?}"),
        }
    }

    #[test]
    fn check_plugin_sandbox_ok_in_fast_mode_without_adapter() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let manifest_path = write_manifest(
            tmp.path(),
            r#"
[package]
name = "fast-plugin"
version = "0.1.0"

[[capabilities]]
kind = "fs.read"
paths = ["/tmp/**"]
"#,
        );
        // adapter = None → fast mode: build_plan only.
        let outcome = check_plugin_sandbox("fast-plugin", &manifest_path, None);
        assert!(
            matches!(outcome, SandboxPluginOutcome::Ok),
            "expected Ok in fast mode, got {outcome:?}"
        );
    }
}
```

The `#[derive(Debug)]` on `SandboxPluginOutcome` is required so the `panic!("got {other:?}")` formatting works.

- [ ] **Step 2: Run tests to verify they fail (compilation error — types not defined yet)**

Run:
```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo test -p tau-cli --lib check_sandbox_tests 2>&1 | tail -20
```

Expected: compilation error referencing `SandboxPluginOutcome` and `check_plugin_sandbox` not defined.

- [ ] **Step 3: Implement `SandboxPluginOutcome` + `check_plugin_sandbox`**

Insert ABOVE the test module in `crates/tau-cli/src/cmd/resolve_helpers.rs` (also add the `use` lines near the top of the file beside existing `use` statements):

```rust
// Add to existing `use` block at top of file:
use std::path::Path;
use tau_runtime::sandbox::{
    build_plan, validate_plan_against_adapter, SandboxAdapter, SandboxValidationError,
};

// Insert near the end, just before `#[cfg(test)] mod check_sandbox_tests`:

/// Outcome of validating one plugin's sandbox plan.
///
/// Captures all error messages as owned `String`s so callers don't need
/// to thread runtime error types through their own match arms.
#[derive(Debug)]
pub(crate) enum SandboxPluginOutcome {
    /// Plan built and validated cleanly against the adapter (or built
    /// cleanly in fast mode where no adapter is given).
    Ok,
    /// `build_plan` returned an error.
    BuildPlanFailed(String),
    /// `validate_plan_against_adapter` returned one or more errors.
    ValidateFailed(Vec<SandboxValidationError>),
    /// Manifest at `<pkg>/tau.toml` could not be read.
    ManifestUnreadable(String),
}

/// Build and (optionally) validate one plugin's sandbox plan.
///
/// Reads the plugin's manifest from `manifest_path`, calls `build_plan`
/// on its declared capabilities, and (when `adapter` is `Some`) calls
/// `validate_plan_against_adapter`. When `adapter` is `None`, runs in
/// "fast mode" — only `build_plan` is exercised; on success returns `Ok`.
///
/// Never panics; never logs. All outcomes (including manifest read
/// errors and validation failures) come back through
/// [`SandboxPluginOutcome`].
pub(crate) fn check_plugin_sandbox(
    plugin_id: &str,
    manifest_path: &Path,
    adapter: Option<&SandboxAdapter>,
) -> SandboxPluginOutcome {
    let package_caps = match tau_pkg::read_manifest(manifest_path) {
        Ok(manifest) => manifest.capabilities().to_vec(),
        Err(e) => return SandboxPluginOutcome::ManifestUnreadable(e.to_string()),
    };

    let plan = match build_plan(&package_caps, &[], None, None) {
        Ok(p) => p,
        Err(e) => return SandboxPluginOutcome::BuildPlanFailed(e.to_string()),
    };

    match adapter {
        Some(adapter) => match validate_plan_against_adapter(plugin_id, &plan, adapter) {
            Ok(()) => SandboxPluginOutcome::Ok,
            Err(errors) => SandboxPluginOutcome::ValidateFailed(errors),
        },
        None => SandboxPluginOutcome::Ok,
    }
}
```

If `tempfile` is not already in `[dev-dependencies]` of `crates/tau-cli/Cargo.toml`, add it. Check first:

```bash
grep tempfile /Users/titouanlebocq/code/tau-worktrees/extract-check-sandbox/crates/tau-cli/Cargo.toml
```

If missing, add `tempfile = { workspace = true }` under `[dev-dependencies]`. (It's already a workspace dep; many other tau-cli tests use it.)

Same for `tokio` — `resolve_helpers.rs` doesn't currently use tokio at the test level, but the mock_adapter helper needs it. Confirm:

```bash
grep -E '^tokio' /Users/titouanlebocq/code/tau-worktrees/extract-check-sandbox/crates/tau-cli/Cargo.toml
```

Tokio is already a regular dependency; no Cargo.toml change needed.

- [ ] **Step 4: Run tests to verify they pass**

Run:
```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo test -p tau-cli --lib check_sandbox_tests 2>&1 | tail -20
```

Expected:
```
running 3 tests
test cmd::resolve_helpers::check_sandbox_tests::check_plugin_sandbox_manifest_unreadable_for_missing_file ... ok
test cmd::resolve_helpers::check_sandbox_tests::check_plugin_sandbox_ok_for_benign_manifest ... ok
test cmd::resolve_helpers::check_sandbox_tests::check_plugin_sandbox_ok_in_fast_mode_without_adapter ... ok

test result: ok. 3 passed; 0 failed; 0 ignored
```

- [ ] **Step 5: Commit**

```bash
git add crates/tau-cli/src/cmd/resolve_helpers.rs crates/tau-cli/Cargo.toml
git -c user.name="Titouan Le Bocq" -c user.email="lebocq.tit@gmail.com" \
  commit -m "refactor(tau-cli): add check_plugin_sandbox helper + SandboxPluginOutcome

Extracts the per-plugin sandbox-check loop body from
cmd/resolve.rs::run_check_sandbox into a pure pub(crate) function
in cmd/resolve_helpers.rs. Behavior captured by 3 unit tests.
Follow-up to #161 (Path B per the spec)."
```

---

### Task 2: Add `read_sandbox_requirements_for_check` + `resolve_sandbox_check_adapter`

**Files:**
- Modify: `crates/tau-cli/src/cmd/resolve_helpers.rs`

- [ ] **Step 1: Implement `read_sandbox_requirements_for_check`**

Add to `crates/tau-cli/src/cmd/resolve_helpers.rs` near the other new helpers. Add `tau_pkg::scope::{...}` imports to the top-of-file `use` block if not present.

```rust
// Add to top-of-file `use` block:
use tau_pkg::scope::{SandboxRequirements, ScopeConfig};
use tau_runtime::sandbox::{resolve_adapter, resolve_strict_for_validation, ResolutionError};
// (SandboxAdapter is already imported from Task 1)

// Insert above `pub(crate) enum SandboxPluginOutcome` from Task 1:

/// Read `[sandbox]` from the active scope's `config.toml`.
///
/// Returns `SandboxRequirements::default()` if the file is missing,
/// unreadable, or malformed. Errors are intentionally swallowed: the
/// `tau check config` and `tau check lockfile` categories are
/// responsible for reporting config-file issues; the sandbox check
/// should not double-report them.
pub(crate) fn read_sandbox_requirements_for_check(scope: &tau_pkg::Scope) -> SandboxRequirements {
    let path = scope.config_path();
    if !path.exists() {
        return SandboxRequirements::default();
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return SandboxRequirements::default();
    };
    match ScopeConfig::read_from_str(&text) {
        Ok(cfg) => cfg.sandbox,
        Err(_) => SandboxRequirements::default(),
    }
}

/// Resolve the sandbox adapter for check flows.
///
/// When `required_tier == None` the runtime resolver picks Passthrough,
/// which trivially accepts every plan. Check flows skip passthrough
/// and pick the highest-priority non-passthrough adapter via
/// `resolve_strict_for_validation`, so the report shows what would
/// happen if the user strengthens the requirement. If no
/// non-passthrough adapter is available on this platform, the error
/// from `resolve_strict_for_validation` propagates.
pub(crate) async fn resolve_sandbox_check_adapter(
    requirements: &SandboxRequirements,
) -> Result<SandboxAdapter, ResolutionError> {
    use tau_pkg::scope::SandboxRequiredTier;
    if matches!(requirements.required_tier, SandboxRequiredTier::None) {
        resolve_strict_for_validation().await
    } else {
        resolve_adapter(requirements, &[]).await
    }
}
```

- [ ] **Step 2: Verify the crate still compiles**

Run:
```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo check -p tau-cli 2>&1 | tail -10
```

Expected: `Finished` — no warnings about unused helpers (they're `pub(crate)` and will be called from Task 3/4; pre-tasks-3/4 they may trigger `dead_code` lint, which is fine for a single intermediate commit because the next two commits will land minutes later and exercise them).

If clippy/lint complains about dead code at this checkpoint, add `#[allow(dead_code)] // wired up by Task 3/4` to the two new items temporarily; remove the attribute in Task 4's commit.

- [ ] **Step 3: Commit**

```bash
git add crates/tau-cli/src/cmd/resolve_helpers.rs
git -c user.name="Titouan Le Bocq" -c user.email="lebocq.tit@gmail.com" \
  commit -m "refactor(tau-cli): add sandbox-check requirements + adapter helpers

Extracts the [sandbox]-from-config read and the None→strict-for-validation
adapter pivot from the two call sites into resolve_helpers.rs. Wired
up in subsequent commits."
```

---

### Task 3: Rewrite `cmd/resolve.rs::run_check_sandbox`

**Files:**
- Modify: `crates/tau-cli/src/cmd/resolve.rs` (replace `run_check_sandbox` body, lines 63-244)

- [ ] **Step 1: Replace `run_check_sandbox`**

Replace the existing `run_check_sandbox` function in `crates/tau-cli/src/cmd/resolve.rs` (current lines 63-244) with:

```rust
/// Implements `tau resolve --check-sandbox`.
///
/// Loads the lockfile, reads each installed plugin's capabilities from
/// the package manifest on disk, builds a [`tau_ports::SandboxPlan`]
/// per plugin, and validates it against the active scope's configured
/// adapter. Reports ✓ / ✗ per plugin (human-readable) or JSON events
/// when `--json` is set.
///
/// When the project's `required_tier` is `None` (passthrough would be
/// selected at runtime), `--check-sandbox` skips passthrough as a
/// validation target and instead uses the highest-priority
/// non-passthrough adapter available on the current platform. This
/// surfaces what would happen if the user strengthens their requirement.
/// If no non-passthrough adapter is available, exits 2 with a clear
/// error.
///
/// Exit 0 if all plugins pass; exit 2 if any fail or no adapter is
/// available (per ADR-0007 three-bucket exit codes).
async fn run_check_sandbox(_args: &ResolveArgs, output: &mut Output) -> anyhow::Result<()> {
    use crate::cmd::resolve_helpers::{
        check_plugin_sandbox, read_sandbox_requirements_for_check,
        resolve_sandbox_check_adapter, SandboxPluginOutcome,
    };
    use tau_pkg::scope::SandboxRequiredTier;

    // 1. Resolve the scope.
    let cwd = std::env::current_dir()?;
    let scope = tau_pkg::Scope::resolve(&cwd).context("resolving package scope")?;

    // 2. Read [sandbox] from scope config (swallows malformed-config errors;
    //    `tau check config` is the authoritative report surface).
    let sandbox_requirements = read_sandbox_requirements_for_check(&scope);

    // 3. Resolve the adapter (handles the None→strict-for-validation pivot).
    let adapter = match resolve_sandbox_check_adapter(&sandbox_requirements).await {
        Ok(a) => a,
        Err(e) => {
            let msg = match sandbox_requirements.required_tier {
                SandboxRequiredTier::None => {
                    "no non-permissive adapter available to validate against; cannot perform sandbox check".to_string()
                }
                _ => format!("no sandbox adapter available: {e}"),
            };
            if output.is_json() {
                output.json(&serde_json::json!({
                    "event": "error",
                    "reason": msg,
                }))?;
            } else {
                output.error(&msg)?;
            }
            std::process::exit(2);
        }
    };

    // 4. Load the lockfile.
    let lockfile_path = scope.lockfile_path();
    let lockfile = tau_pkg::LockFile::load(&lockfile_path)
        .with_context(|| format!("loading lockfile at {lockfile_path:?}"))?;

    let mut ok_count = 0usize;
    let mut error_count = 0usize;

    // 5. For each installed package that has a plugin entry, validate its plan.
    for pkg in &lockfile.packages {
        if pkg.plugin.is_none() {
            // Data-only package — no sandbox check needed.
            continue;
        }

        let plugin_id = pkg.name.as_str().to_owned();
        let pkg_dir = scope.package_dir(&pkg.name, &pkg.active_version);
        let manifest_path = pkg_dir.join("tau.toml");

        match check_plugin_sandbox(&plugin_id, &manifest_path, Some(&adapter)) {
            SandboxPluginOutcome::Ok => {
                ok_count += 1;
                if output.is_json() {
                    output.json(&serde_json::json!({
                        "event": "sandbox_check",
                        "plugin_id": plugin_id,
                        "status": "ok",
                    }))?;
                } else {
                    output.human(&format!("✓ {plugin_id}"))?;
                }
            }
            SandboxPluginOutcome::BuildPlanFailed(msg) => {
                error_count += 1;
                let reason = format!("build_plan failed for {plugin_id}: {msg}");
                if output.is_json() {
                    output.json(&serde_json::json!({
                        "event": "sandbox_check",
                        "plugin_id": plugin_id,
                        "status": "error",
                        "reason": reason,
                    }))?;
                } else {
                    output.error(&reason)?;
                }
            }
            SandboxPluginOutcome::ValidateFailed(errors) => {
                error_count += 1;
                for err in &errors {
                    if output.is_json() {
                        output.json(&serde_json::json!({
                            "event": "sandbox_check",
                            "plugin_id": plugin_id,
                            "status": "error",
                            "reason": err.reason,
                            "capability": format!("{:?}", err.capability.required_shape()),
                        }))?;
                    } else {
                        output.human(&format!("✗ {plugin_id}: {}", err.reason))?;
                    }
                }
            }
            SandboxPluginOutcome::ManifestUnreadable(msg) => {
                let reason = format!(
                    "could not read manifest for {plugin_id}: {msg} — skipping capability check"
                );
                if output.is_json() {
                    output.json(&serde_json::json!({
                        "event": "sandbox_check",
                        "plugin_id": plugin_id,
                        "status": "skipped",
                        "reason": reason,
                    }))?;
                } else {
                    output.warn(&reason)?;
                }
            }
        }
    }

    // 6. Summary.
    let total = ok_count + error_count;
    if output.is_json() {
        output.json(&serde_json::json!({
            "event": "summary",
            "ok": ok_count,
            "errors": error_count,
        }))?;
    } else {
        output.human(&format!(
            "{total} plugins checked: {ok_count} ok, {error_count} errors"
        ))?;
    }

    // 7. Exit code: 0 if all ok, 2 if any errors.
    if error_count > 0 {
        std::process::exit(2);
    }
    Ok(())
}
```

The top-of-file `use` block can drop these now-unused imports if any are present *only* for the deleted body. Run `cargo check` to surface them.

- [ ] **Step 2: Verify the crate still compiles + run resolve integration tests**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo test -p tau-cli --test cmd_resolve_check_sandbox 2>&1 | tail -30
```

Expected: all 9 tests pass.

If a test fails because of the intentional behavior change (config-malformedness now defaults instead of erroring), inspect the test:
- If the test asserts the old hard-error behavior on a malformed-config input: this is the case flagged in spec §8. Either (a) update the test to assert the new behavior (preferred — matches the new contract) or (b) confirm by running `grep -n 'parsing scope config' crates/tau-cli/tests/cmd_resolve_check_sandbox.rs` whether any test explicitly relies on the error.

- [ ] **Step 3: Run full tau-cli test suite to catch regressions**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-cli 2>&1 | tail -30
```

Expected: all tests pass (including unit + integration).

- [ ] **Step 4: Commit**

```bash
git add crates/tau-cli/src/cmd/resolve.rs
git -c user.name="Titouan Le Bocq" -c user.email="lebocq.tit@gmail.com" \
  commit -m "refactor(tau-cli): rewrite resolve --check-sandbox to use shared helpers

Reduces run_check_sandbox from ~180 LOC monolithic to ~110 LOC of
output-mapping logic. Adapter-pivot, requirements-read, and per-plugin
loop body now live in resolve_helpers.

Intentional behavior change: malformed scope config no longer hard-errors
out of \`tau resolve --check-sandbox\`; it falls through to default
SandboxRequirements and lets \`tau check config\` be the authoritative
report surface."
```

---

### Task 4: Rewrite `cmd/check/categories/sandbox.rs::run_sandbox`

**Files:**
- Modify: `crates/tau-cli/src/cmd/check/categories/sandbox.rs` (rewrite `run_sandbox` body, simplify imports, update module doc)

- [ ] **Step 1: Replace the file contents**

Replace `crates/tau-cli/src/cmd/check/categories/sandbox.rs` entirely with:

```rust
//! `tau check sandbox` — validate sandbox plans for each installed plugin.
//!
//! Default (full): build plan AND validate against the resolved adapter.
//! `--fast`: build plan only; skip adapter probe + validation.
//!
//! The per-plugin build/validate loop lives in
//! `crate::cmd::resolve_helpers::check_plugin_sandbox`; this module
//! handles the check-aggregator-specific output mapping (severity policy,
//! `CheckFinding` synthesis, fast-mode adapter elision).

use crate::cmd::check::result::{
    CheckCategory, CheckFinding, CheckResult, CheckStatus, FindingLocation, Severity,
};
use crate::cmd::check::runner::CheckCtx;
use crate::cmd::resolve_helpers::{
    check_plugin_sandbox, read_sandbox_requirements_for_check, resolve_sandbox_check_adapter,
    SandboxPluginOutcome,
};
use serde_json::json;

pub async fn run_sandbox(ctx: &CheckCtx) -> CheckResult {
    // project.is_none() means tau.toml is malformed — the config check
    // reports this; we just skip to avoid duplicate noise.
    if ctx.project.is_none() {
        return skipped("tau.toml malformed (see config check)");
    }

    let tau_toml_path = ctx.project_root.join("tau.toml");
    let sandbox_requirements = read_sandbox_requirements_for_check(&ctx.scope);

    // Load the lockfile. If missing or unreadable, skip — the lockfile
    // check will already report this.
    let lockfile_path = ctx.scope.lockfile_path();
    if !lockfile_path.exists() {
        return skipped("lockfile missing or unreadable (see lockfile check)");
    }
    let lockfile = match tau_pkg::LockFile::load(&lockfile_path) {
        Ok(lf) => lf,
        Err(_) => return skipped("lockfile missing or unreadable (see lockfile check)"),
    };

    // Collect only packages that have a plugin entry — data-only packages
    // don't need sandbox plans.
    let plugin_pkgs: Vec<_> = lockfile
        .packages
        .iter()
        .filter(|p| p.plugin.is_some())
        .collect();

    if plugin_pkgs.is_empty() {
        return skipped("no plugin packages in lockfile");
    }

    let mut findings: Vec<CheckFinding> = Vec::new();

    // Resolve adapter unless we're in --fast mode.
    //
    // When required_tier is None the runtime would pick Passthrough, which
    // trivially accepts every plan. We use resolve_strict_for_validation
    // via the helper, which picks the highest-priority non-passthrough
    // adapter instead to surface what would happen if the user strengthens
    // the requirement.
    let adapter_opt = if ctx.fast {
        None
    } else {
        match resolve_sandbox_check_adapter(&sandbox_requirements).await {
            Ok(a) => Some(a),
            Err(e) => {
                // No adapter available — emit an advisory warning and skip
                // validation rather than hard-failing.
                findings.push(CheckFinding {
                    category: CheckCategory::Sandbox,
                    severity: Severity::Warning,
                    rule_id: "tau.sandbox.no_adapter",
                    summary: format!("no sandbox adapter available for validation: {e}"),
                    detail: Some(
                        "Sandbox plan shapes could not be validated. \
                         Install a sandbox adapter (e.g. tau-sandbox-darwin) to enable full checks."
                            .into(),
                    ),
                    location: None,
                    remediation: None,
                    structured: json!({ "kind": "NoAdapterAvailable", "error": e.to_string() }),
                });
                return CheckResult {
                    category: CheckCategory::Sandbox,
                    status: CheckStatus::Ok, // advisory only, not a hard failure
                    findings,
                    duration: std::time::Duration::ZERO,
                };
            }
        }
    };

    for pkg in &plugin_pkgs {
        let plugin_id = pkg.name.as_str().to_owned();
        let pkg_dir = ctx.scope.package_dir(&pkg.name, &pkg.active_version);
        let manifest_path = pkg_dir.join("tau.toml");

        match check_plugin_sandbox(&plugin_id, &manifest_path, adapter_opt.as_ref()) {
            SandboxPluginOutcome::Ok => {}
            SandboxPluginOutcome::BuildPlanFailed(msg) => {
                findings.push(build_plan_finding(&plugin_id, msg, &tau_toml_path));
            }
            SandboxPluginOutcome::ValidateFailed(errors) => {
                for err in errors {
                    findings.push(CheckFinding {
                        category: CheckCategory::Sandbox,
                        severity: Severity::Error,
                        rule_id: "tau.sandbox.plan_invalid",
                        summary: format!("plugin `{plugin_id}`: {}", err.reason),
                        detail: None,
                        location: Some(FindingLocation {
                            path: tau_toml_path.clone(),
                            line: None,
                            column: None,
                        }),
                        remediation: None,
                        structured: json!({
                            "plugin_id": plugin_id,
                            "kind": "SandboxValidationFailed",
                            "reason": err.reason,
                        }),
                    });
                }
            }
            SandboxPluginOutcome::ManifestUnreadable(msg) => {
                // Fast mode preserves the prior silent-skip behavior; full
                // mode surfaces a Warning so users see why a plugin was
                // skipped without changing the result status.
                if !ctx.fast {
                    findings.push(CheckFinding {
                        category: CheckCategory::Sandbox,
                        severity: Severity::Warning,
                        rule_id: "tau.sandbox.manifest_unreadable",
                        summary: format!(
                            "could not read manifest for `{plugin_id}`: {msg} — skipping capability check"
                        ),
                        detail: None,
                        location: Some(FindingLocation {
                            path: manifest_path,
                            line: None,
                            column: None,
                        }),
                        remediation: Some("tau resolve".into()),
                        structured: json!({
                            "plugin_id": plugin_id,
                            "kind": "ManifestUnreadable",
                            "error": msg,
                        }),
                    });
                }
            }
        }
    }

    let status = if findings.iter().any(|f| f.severity == Severity::Error) {
        CheckStatus::Failed
    } else {
        CheckStatus::Ok
    };
    CheckResult {
        category: CheckCategory::Sandbox,
        status,
        findings,
        duration: std::time::Duration::ZERO,
    }
}

fn skipped(reason: &str) -> CheckResult {
    CheckResult {
        category: CheckCategory::Sandbox,
        status: CheckStatus::Skipped {
            reason: reason.into(),
        },
        findings: Vec::new(),
        duration: std::time::Duration::ZERO,
    }
}

fn build_plan_finding(
    plugin_id: &str,
    message: String,
    tau_toml_path: &std::path::Path,
) -> CheckFinding {
    CheckFinding {
        category: CheckCategory::Sandbox,
        severity: Severity::Error,
        rule_id: "tau.sandbox.plan_invalid",
        summary: format!("build_plan failed for `{plugin_id}`: {message}"),
        detail: None,
        location: Some(FindingLocation {
            path: tau_toml_path.to_path_buf(),
            line: None,
            column: None,
        }),
        remediation: None,
        structured: json!({
            "plugin_id": plugin_id,
            "kind": "BuildPlanFailed",
            "error": message,
        }),
    }
}
```

- [ ] **Step 2: Run check-side integration tests**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo test -p tau-cli --test cmd_check_subcommands 2>&1 | tail -30
```

Expected: all tests pass.

If a check-side test was hand-asserting "no manifest unreadable warning emitted" or "build_plan_finding's `error` field has full path-like content," inspect the test and the new emission; in the full-mode case the Warning finding is a *new* finding compared to before (previously full-mode skipped silently too, per resolve.rs's behavior, but the original `check/categories/sandbox.rs::run_sandbox` had no manifest-unreadable branch at all — it would just `continue` silently). The new Warning is consistent with the spec §6.1 design: full mode surfaces unreadable manifests, fast mode preserves silent-skip.

If any test snapshot needs updating, run with `INSTA_UPDATE=auto` once and inspect the snapshot diff:
```bash
INSTA_UPDATE=auto timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo test -p tau-cli --test cmd_check_subcommands 2>&1 | tail -30
```

Then `git diff` the `.snap` files — only proceed if the new Warning is in the diff and is the expected change.

- [ ] **Step 3: Run full tau-cli test suite**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-cli 2>&1 | tail -30
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/tau-cli/src/cmd/check/categories/sandbox.rs
# also include any snapshot updates if they happened:
git add crates/tau-cli/tests/snapshots/ 2>/dev/null || true
git -c user.name="Titouan Le Bocq" -c user.email="lebocq.tit@gmail.com" \
  commit -m "refactor(tau-cli): rewrite check sandbox category to use shared helpers

Reduces run_sandbox from ~270 LOC to ~150 LOC of CheckFinding-mapping
logic. The Path C duplicate flagged in the module doc comment is now
resolved.

Behavior preserved exactly for tau check sandbox. New: full mode now
emits a Warning finding when a plugin's manifest is unreadable (was
silently skipped); fast mode preserves silent-skip. Snapshot diff
should reflect only this change."
```

---

### Task 5: Final verification — fmt, clippy, full test pass

**Files:** none (verification only)

- [ ] **Step 1: fmt check**

```bash
timeout 30 cargo fmt --check 2>&1 | tail -5
```

Expected: empty output (no formatting drift).

If formatting drift is reported, run `cargo fmt` and commit:
```bash
cargo fmt
git add -A
git -c user.name="Titouan Le Bocq" -c user.email="lebocq.tit@gmail.com" \
  commit -m "style: cargo fmt"
```

- [ ] **Step 2: clippy**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main \
  cargo clippy -p tau-cli --all-targets -- -D warnings 2>&1 | tail -20
```

Expected: `Finished` with no warnings.

If clippy complains about a `#[allow(dead_code)]` left over from Task 2's intermediate state, remove it now.

- [ ] **Step 3: full tau-cli nextest**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main \
  cargo nextest run -p tau-cli 2>&1 | tail -30
```

Expected: all tests pass.

- [ ] **Step 4: doctest**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main \
  cargo test -p tau-cli --doc 2>&1 | tail -10
```

Expected: all doctests pass (nextest doesn't run doctests).

- [ ] **Step 5: push + open PR**

```bash
scripts/agent-push.sh -u origin feat/extract-check-sandbox 2>&1 | tail -10
```

(per CLAUDE.md "AGENT PUSH RULES" — direct `git push` is silently killed when the deep gate is on; `scripts/agent-push.sh` runs the gate standalone then pushes `--no-verify`.)

If the deep gate fails for unrelated reasons (worktree-gitdir gotcha, etc.) and the change is Rust-only, do NOT bypass with `--no-verify` — fix the gate or get help.

```bash
gh pr create --base main --head feat/extract-check-sandbox \
  --title "refactor(tau-cli): extract shared check_sandbox core (follow-up to #161)" \
  --body "$(cat <<'EOF'
## Summary

Pays down the Path C duplicate flagged in PR #161
(`cmd/check/categories/sandbox.rs:6-15`'s TODO comment).

Three `pub(crate)` helpers in `cmd/resolve_helpers.rs`:

- `read_sandbox_requirements_for_check` — reads `[sandbox]` from scope config, swallowing errors (tau check config is the authoritative report surface).
- `resolve_sandbox_check_adapter` — handles the `None → strict-for-validation` pivot.
- `check_plugin_sandbox` returning `SandboxPluginOutcome` — the per-plugin build/validate loop body.

Both call sites (`cmd/resolve.rs::run_check_sandbox` and
`cmd/check/categories/sandbox.rs::run_sandbox`) shrink to output-mapping-only code.

## Behavior changes

Two small intentional changes documented in the spec (§4.1 and §6.1):

1. `tau resolve --check-sandbox` no longer hard-errors on malformed scope config; falls through to default `SandboxRequirements`. Rationale: `tau check config` is the authoritative report surface.
2. `tau check sandbox` (full mode) now emits a Warning finding when a plugin manifest is unreadable; was previously silent-skip. Fast mode preserves silent-skip. Rationale: surfacing why a plugin was skipped is more useful than silence; severity stays Warning so result status is unaffected.

## Test plan

- [x] 3 new unit tests on `check_plugin_sandbox` cover Ok, ManifestUnreadable, fast-mode Ok
- [x] Existing 9 `cmd_resolve_check_sandbox` integration tests pass unchanged
- [x] Existing `cmd_check_subcommands` integration tests pass (one snapshot update for the new manifest-unreadable Warning, reviewed in commit 4)
- [x] cargo fmt, clippy -D warnings, nextest, doctest all green

Spec: `docs/superpowers/specs/2026-05-18-extract-check-sandbox-design.md`
Plan: `docs/superpowers/plans/2026-05-18-extract-check-sandbox.md`

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Capture the PR URL from the gh output.

---

## Self-review checklist (applied)

- **Spec §3 coverage:** Task 1 covers the per-plugin loop helper; Task 2 covers requirements-read + adapter-resolve helpers. ✓
- **Spec §5.1 (resolve.rs rewrite):** Task 3. ✓
- **Spec §5.2 (check/categories/sandbox.rs rewrite):** Task 4. ✓
- **Spec §6.1 (unit tests):** Task 1 ships Ok + ManifestUnreadable + fast-mode-Ok. Case 3 (ValidateFailed) is omitted per the spec's hedge: MockSandbox supports the 5 standard shapes so constructing a validate-failing manifest with `MockSandbox` requires a custom shape, which is awkward without leaking adapter internals. Existing integration tests cover ValidateFailed end-to-end. ✓
- **Spec §6.2 (regression gate):** Task 3 and Task 4 both run the corresponding integration tests. ✓
- **Spec §7 (implementation order):** Plan order matches spec order (1→2→3→4 + verification). ✓
- **Spec §8 risk: tests asserting old `parsing scope config` error:** Task 3 Step 2 contains explicit guidance on what to do if a test fails for this reason. ✓
- **No placeholders:** every step has actual code or actual commands. ✓
- **Type consistency:** `SandboxPluginOutcome` variants used in Task 3 and Task 4 match definitions in Task 1. `SandboxAdapter` is the concrete enum type, not `Arc<dyn Sandbox>`. `read_sandbox_requirements_for_check` returns `SandboxRequirements` (no `Result`). `resolve_sandbox_check_adapter` returns `Result<SandboxAdapter, ResolutionError>`. `check_plugin_sandbox` returns `SandboxPluginOutcome` (no `Result`; manifest read errors are folded into the enum). ✓
