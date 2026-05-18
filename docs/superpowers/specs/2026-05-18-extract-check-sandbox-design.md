# Extract shared `check_sandbox` core — design

**Date:** 2026-05-18
**Status:** Approved
**Authors:** Claude (Opus 4.7)
**Tracking:** follow-up to PR #161 (`tau check`)

## 1. Background

PR #161 (`feat: tau check — pre-flight validation aggregator`, commit
`c1c66d1`) shipped the `tau check sandbox` category by duplicating
~50 LOC of plan-build/validate logic from
`crates/tau-cli/src/cmd/resolve.rs::run_check_sandbox` into the new
`crates/tau-cli/src/cmd/check/categories/sandbox.rs::run_sandbox`. The
duplicate was flagged in-source:

```text
crates/tau-cli/src/cmd/check/categories/sandbox.rs:6-15
//! # Implementation note (Path C — adapted from resolve.rs)
//!
//! The core plan-build/validate logic is derived from
//! `crates/tau-cli/src/cmd/resolve.rs::run_check_sandbox`. That function
//! is monolithic (interleaved output formatting), so rather than surgically
//! refactoring it we duplicate the ~50 LOC of validation logic here and
//! translate failures into `CheckFinding`s instead of I/O calls.
//!
//! TODO: extract a shared helper (Path B) in a follow-up commit so that
//! resolve.rs and this module don't diverge.
```

This spec covers the follow-up.

## 2. Goal

Eliminate divergence risk by housing the shared core in
`crates/tau-cli/src/cmd/resolve_helpers.rs` (already the home for
resolve-flow helpers reused across `tau resolve`, `tau run`, `tau chat`).

**Non-goals:**

- No public API change. The helpers are `pub(crate)`.
- No behavior change. Both CLI verbs produce byte-identical output before
  and after.
- No new tests for `cmd_resolve.rs` / `cmd_check.rs` integration paths —
  existing tests are the regression gate. Only the new helpers get
  fresh unit tests.

## 3. What's shared today

Three concerns are duplicated between the two call sites:

| Concern | `resolve.rs::run_check_sandbox` | `check/categories/sandbox.rs::run_sandbox` |
|---|---|---|
| Read `[sandbox]` requirements from scope config | lines 73–83 | lines 42–56 |
| Resolve adapter (with `None → strict-for-validation` pivot) | lines 91–126 | lines 147–185 |
| Per-plugin loop (read manifest → `build_plan` → `validate`) | lines 137–223 | lines 187–255 |

The two callers differ in **output mapping**:

- `resolve.rs`: `output.json(...)` / `output.human(...)` / `output.error(...)`
  per event, then `std::process::exit(2)` on errors / adapter unavailability.
- `check/categories/sandbox.rs`: accumulates `Vec<CheckFinding>`, emits
  a `CheckResult`. Adapter unavailability is a **Warning**, not a hard
  failure (different severity policy — check-aggregator surfaces this
  as a setup hint, resolve treats it as a precondition).

These output-mapping differences are intentional and stay in their
respective callers.

## 4. Design

Add three `pub(crate)` items to
`crates/tau-cli/src/cmd/resolve_helpers.rs`. All async-free except the
adapter resolver, which already is.

### 4.1 `read_sandbox_requirements_for_check`

```rust
/// Read `[sandbox]` from the active scope's `config.toml`.
///
/// Returns `SandboxRequirements::default()` if the file is missing,
/// unreadable, or malformed. Errors are intentionally swallowed: the
/// `tau check config` and `tau check lockfile` categories are
/// responsible for reporting config-file issues; the sandbox check
/// should not double-report them. `resolve.rs` previously surfaced
/// parse errors via `anyhow::Context`; that strictness is no longer
/// useful now that `tau check config` exists as the authoritative
/// place to report config-malformedness.
pub(crate) fn read_sandbox_requirements_for_check(
    scope: &tau_pkg::Scope,
) -> tau_pkg::scope::SandboxRequirements;
```

**Behavior change note:** `resolve.rs::run_check_sandbox` currently
errors with `anyhow::Context` on config-file read/parse failure. After
extraction it will fall through to default `SandboxRequirements` like
the check aggregator does. This is the **one intentional behavior
change** in this refactor. Justification: `tau resolve --check-sandbox`
already requires a valid scope (`scope::resolve` runs first); the
authoritative config-malformedness report lives in `tau check config`.
A user running `tau resolve --check-sandbox` against a malformed
scope config now gets a sandbox-check run against default requirements
instead of an opaque "parsing scope config" error; this is strictly
more useful. If the user wants strict config validation, `tau check`
provides it.

### 4.2 `resolve_sandbox_check_adapter`

```rust
/// Resolve the sandbox adapter for check flows.
///
/// When `required_tier == None` the runtime resolver picks Passthrough,
/// which trivially accepts every plan. Check flows skip passthrough
/// and pick the highest-priority non-passthrough adapter via
/// `resolve_strict_for_validation`, so the report shows what would
/// happen if the user strengthens the requirement. If no non-passthrough
/// adapter is available on this platform, the error from
/// `resolve_strict_for_validation` propagates.
pub(crate) async fn resolve_sandbox_check_adapter(
    requirements: &tau_pkg::scope::SandboxRequirements,
) -> Result<
    tau_runtime::sandbox::SandboxAdapter,
    tau_runtime::sandbox::ResolutionError,
>;
```

Caller is responsible for mapping the error into output (resolve.rs
exits 2; check aggregator emits a Warning finding).

### 4.3 `SandboxPluginOutcome` + `check_plugin_sandbox`

```rust
/// Outcome of validating one plugin's sandbox plan.
pub(crate) enum SandboxPluginOutcome {
    /// Plan built and validated cleanly.
    Ok,
    /// `build_plan` returned an error.
    BuildPlanFailed(String),
    /// `validate_plan_against_adapter` returned errors.
    ValidateFailed(Vec<tau_runtime::sandbox::SandboxValidationError>),
    /// Manifest at `<pkg>/tau.toml` could not be read.
    ManifestUnreadable(String),
}

/// Build + validate one plugin's sandbox plan.
///
/// When `adapter` is `None`, only `build_plan` runs (fast mode).
/// On success in fast mode, returns `Ok`. On `build_plan` error,
/// returns `BuildPlanFailed`. On manifest read error, returns
/// `ManifestUnreadable`. Never panics; never logs.
pub(crate) fn check_plugin_sandbox(
    plugin_id: &str,
    manifest_path: &std::path::Path,
    adapter: Option<&tau_runtime::sandbox::SandboxAdapter>,
) -> SandboxPluginOutcome;
```

`error.to_string()` is captured eagerly inside the helper so callers
don't need to import the runtime's error types just to format them.
`SandboxValidationError` is already public in `tau-runtime::sandbox`
(it's part of the `validate_plan_against_adapter` signature) and both
callers already import it, so re-exposing it through this enum costs
nothing.

## 5. Caller rewrites

### 5.1 `cmd/resolve.rs::run_check_sandbox`

Before: ~180 LOC monolithic.

After: ~80 LOC of pure output mapping:

```rust
async fn run_check_sandbox(_args: &ResolveArgs, output: &mut Output) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let scope = tau_pkg::Scope::resolve(&cwd).context("resolving package scope")?;

    let requirements = resolve_helpers::read_sandbox_requirements_for_check(&scope);

    let adapter = match resolve_helpers::resolve_sandbox_check_adapter(&requirements).await {
        Ok(a) => a,
        Err(e) => {
            let msg = match requirements.required_tier {
                tau_pkg::scope::SandboxRequiredTier::None =>
                    "no non-permissive adapter available to validate against; cannot perform sandbox check".to_string(),
                _ => format!("no sandbox adapter available: {e}"),
            };
            emit_adapter_error(output, &msg)?;
            std::process::exit(2);
        }
    };

    let lockfile_path = scope.lockfile_path();
    let lockfile = tau_pkg::LockFile::load(&lockfile_path)
        .with_context(|| format!("loading lockfile at {lockfile_path:?}"))?;

    let mut ok_count = 0usize;
    let mut error_count = 0usize;

    for pkg in &lockfile.packages {
        if pkg.plugin.is_none() { continue; }
        let plugin_id = pkg.name.as_str().to_owned();
        let pkg_dir = scope.package_dir(&pkg.name, &pkg.active_version);
        let manifest_path = pkg_dir.join("tau.toml");

        match resolve_helpers::check_plugin_sandbox(&plugin_id, &manifest_path, Some(&adapter)) {
            SandboxPluginOutcome::Ok => {
                ok_count += 1;
                emit_plugin_ok(output, &plugin_id)?;
            }
            SandboxPluginOutcome::BuildPlanFailed(msg) => {
                error_count += 1;
                emit_plugin_error(output, &plugin_id, &format!("build_plan failed for {plugin_id}: {msg}"))?;
            }
            SandboxPluginOutcome::ValidateFailed(errors) => {
                error_count += 1;
                for err in &errors {
                    emit_plugin_validate_error(output, &plugin_id, err)?;
                }
            }
            SandboxPluginOutcome::ManifestUnreadable(msg) => {
                emit_plugin_skipped(output, &plugin_id, &format!("could not read manifest for {plugin_id}: {msg} — skipping capability check"))?;
            }
        }
    }

    emit_summary(output, ok_count, error_count)?;
    if error_count > 0 { std::process::exit(2); }
    Ok(())
}
```

Output-emit helpers (`emit_plugin_ok`, etc.) are free functions in
`resolve.rs` — they wrap the existing `output.json(...)` / `output.human(...)`
calls and stay private to this file. No deduplication of those across
modules; the **output schemas differ** by call site and that's by design.

### 5.2 `cmd/check/categories/sandbox.rs::run_sandbox`

Before: ~270 LOC.

After: ~110 LOC. Early-skip branches (`project.is_none()`, lockfile
missing, no plugins) are unchanged. The fast/full split stays; the
per-plugin loop body collapses to:

```rust
for pkg in &plugin_pkgs {
    let plugin_id = pkg.name.as_str().to_owned();
    let pkg_dir = ctx.scope.package_dir(&pkg.name, &pkg.active_version);
    let manifest_path = pkg_dir.join("tau.toml");

    let adapter_arg = if ctx.fast { None } else { Some(&adapter) };
    match resolve_helpers::check_plugin_sandbox(&plugin_id, &manifest_path, adapter_arg) {
        SandboxPluginOutcome::Ok => {}
        SandboxPluginOutcome::BuildPlanFailed(msg) => {
            findings.push(build_plan_finding(&plugin_id, msg, &tau_toml_path));
        }
        SandboxPluginOutcome::ValidateFailed(errors) => {
            for err in errors {
                findings.push(validate_finding(&plugin_id, &err, &tau_toml_path));
            }
        }
        SandboxPluginOutcome::ManifestUnreadable(msg) => {
            // Fast mode: silently skip (current behavior). Full mode:
            // Warning finding.
            if !ctx.fast {
                findings.push(manifest_unreadable_finding(&plugin_id, msg, &manifest_path));
            }
        }
    }
}
```

The `--fast` mode's existing silent-skip of unreadable manifests is
preserved.

Adapter resolution stays inline (it uses the new
`resolve_sandbox_check_adapter` helper but the surrounding
warning-finding-vs-hard-failure mapping is check-specific):

```rust
let adapter_result = resolve_helpers::resolve_sandbox_check_adapter(&sandbox_requirements).await;
let adapter = match adapter_result {
    Ok(a) => a,
    Err(e) => {
        findings.push(no_adapter_warning(&e));
        return CheckResult { /* Ok status, advisory only */ };
    }
};
```

Module-level doc comment (lines 6–15) gets the TODO removed and the
attribution rewritten to point at `resolve_helpers`.

## 6. Tests

### 6.1 New unit tests on `check_plugin_sandbox`

Added inline in `resolve_helpers.rs` `#[cfg(test)] mod tests`. Four
cases:

1. **Ok** — write a minimal manifest with a benign capability; pass a
   real `SandboxAdapter` from `resolve_strict_for_validation`; expect
   `Ok`.
2. **BuildPlanFailed** — write a manifest with a capability shape no
   adapter supports (or a malformed glob); expect `BuildPlanFailed`
   with a non-empty message.
3. **ValidateFailed** — Build a plan that succeeds but fails adapter
   validation. Concrete construction depends on what adapter tier is
   resolvable in unit-test env; if no useful asymmetry is possible
   from the unit-test env (CI runs on Linux + macOS + Windows; only
   Linux has strict adapters in unit-test scope), drop this case and
   rely on existing integration tests for coverage.
4. **ManifestUnreadable** — point at a non-existent path; expect
   `ManifestUnreadable` with a non-empty message.

If case 3 can't be constructed cheaply, ship 1+2+4 only. The point of
unit tests here is correctness of the variant mapping, not coverage of
runtime sandbox behavior (which has its own e2e suite).

### 6.2 Regression gate

Existing tests must pass unchanged:

- `crates/tau-cli/tests/cmd_resolve.rs` — `tau resolve --check-sandbox`
  integration (multiple cases including success, missing adapter,
  validation failure).
- `crates/tau-cli/tests/cmd_check.rs` — `tau check sandbox` integration
  (full + `--fast` modes).
- Snapshot tests if any (`insta` `.snap` files under `tau-cli/tests/`).

## 7. Implementation order

1. Write the three helpers in `resolve_helpers.rs` (no callers yet).
2. Add unit tests for `check_plugin_sandbox`.
3. Rewrite `cmd/resolve.rs::run_check_sandbox` to use the helpers.
4. Rewrite `cmd/check/categories/sandbox.rs::run_sandbox` to use the
   helpers; delete the TODO comment.
5. Run `cargo test -p tau-cli` and `cargo nextest run -p tau-cli` per
   CLAUDE.md cargo rules.
6. Run `cargo clippy -p tau-cli`; fix any lints introduced.
7. Commit with conventional message; open PR; let CI gate.

## 8. Risks

| Risk | Mitigation |
|---|---|
| Behavior drift in `resolve.rs` output | Existing `cmd_resolve.rs` tests assert JSON + human output; they must pass unchanged. |
| Behavior drift in `check/categories/sandbox.rs` | Existing `cmd_check.rs` tests + any insta snapshots assert the `CheckResult` shape. |
| Intentional change (config-malformedness no longer errors in `resolve --check-sandbox`) breaks a CI consumer | Search `cmd_resolve.rs` tests for any case that constructs a malformed scope config and asserts an error. If present: either keep the strictness in resolve.rs (don't extract that branch into the helper) or update the test to assert the new behavior. Discovered during implementation. |
| Adapter type signature mismatch (`Arc<dyn Sandbox>` vs `SandboxAdapter` enum) | Type-checked at compile time. `resolve_adapter` returns `SandboxAdapter`; both callers use that today; helper signature matches. |

## 9. Out of scope

- No public API change to `tau-pkg` or `tau-runtime`.
- No CLI flag changes.
- No new ADR. CLI internals refactor; no decision-record threshold per
  Constitution QG18.
- Per-tool config selectors for the sandbox check (deferred from
  serve mode v1).
