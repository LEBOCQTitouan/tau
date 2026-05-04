# Sandbox activation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Activate the sandboxing infrastructure shipped in priority 12 (ADR-0014) for every `tau chat` / `tau run` invocation by default; replace the chain-based adapter selection with declarative requirements + an internal adapter registry + a Bazel-style resolver; add plugin-side `[sandbox]` requirements; ship `--no-sandbox` / `--sandbox <kind>` CLI flags and the `tau sandbox status` / `tau sandbox setup` diagnostic + scaffolding subcommands.

**Architecture:** Project's `<scope>/config.toml` declares `[sandbox] required_tier` (and optional `required_shapes`); plugins' `tau.toml` declares optional `[sandbox] required_tier` for security-critical loaders; the runtime ships an internal `AdapterRegistry` with Native/Container/Remote-stub/Passthrough metadata; `resolve_adapter` filters the registry by detected platform → probe → tier match → shape match → plugin-tier-floor and picks the highest-priority survivor; the resolved `Arc<SandboxAdapter>` lives on `PluginHostOptions.sandbox_adapter` and is threaded through every plugin spawn; failure cases produce structured guided multi-option errors.

**Tech Stack:** Rust 2021, workspace edition; `serde` + `toml` for the schema migration; `tracing` for once-per-process migration warnings (already a tau-pkg dep); existing `landlock = "0.4"`, `seccompiler = "0.5"`, `nix = "0.29"`, `tau-sandbox-native`, `tau-sandbox-container` deps from priority 12; `assert_cmd` + `tempfile` for CLI integration tests; no new external workspace deps.

---

## Plan-erratum block

Apply preemptively across all tasks:

- **macOS dev / Linux CI gap:** the priority-12 native adapter modules (`light.rs`, `probe.rs`, `strict.rs`) are `#[cfg(target_os = "linux")]`-gated; this sub-project does NOT touch those files. The new `passthrough.rs` adapter is cross-platform and will compile on every CI matrix slot. If a sub-task ever needs to touch Linux-only code, verify the change against a Linux toolchain (e.g., `cargo check --target x86_64-unknown-linux-gnu`) before pushing — the priority-12 lesson was that 5 distinct Linux-only bugs hid in plain sight on macOS.

- **`#[non_exhaustive]` discipline:** all new public types — `SandboxRequirements`, `PluginSandboxRequirements`, `AdapterRegistration`, `ResolutionError`, `ResolutionRejection`, `PassthroughSandbox` — get `#[non_exhaustive]`. Doctests on `#[non_exhaustive]` types must be `ignore`-marked.

- **CRITICAL — verify against false alarms:** during priority 12, implementer agents reported "pre-existing failures" 5 times; 4 of those claims were false (the failures were genuine regressions introduced by the task). Before reporting any test or build failure as "pre-existing", you MUST run the same gate against `BASE_SHA = ee43bc9` (the spec commit) and confirm the failure exists there too. Run `git stash && git checkout ee43bc9 && cargo <gate>` and capture the result; only then claim pre-existing. Don't trust the heuristic "this looks unrelated to my changes."

- **Schema migration v2 → v3:** the `[sandbox]` block in `<scope>/config.toml` was added in priority 12 (schema v1 → v2). This sub-project bumps v2 → v3, dropping `chain` + `minimum_tier` and replacing with `required_tier` + `required_shapes`. v2 configs auto-migrate at load time with a `tracing::warn!` (best-effort: derive `required_tier` from `minimum_tier` if set, else default to `"strict"`; ignore chain entries). v1 configs (no `[sandbox]` section) auto-default to `required_tier = "strict"` with no warning — they were never sandboxing-aware.

- **Existing chain code → new model migration:** `tau-runtime::sandbox::chain` is replaced with `tau-runtime::sandbox::resolver` (new file) + `tau-runtime::sandbox::registry` (new file). The existing `SandboxAdapter` enum (Native/Container/Mock variants) is moved unchanged into the resolver module; its `impl Sandbox` and inherent methods stay. The 8 unit tests currently in `chain.rs` are rewritten in the new module against the registry-based resolver surface (Task 5). The `chain.rs` file itself is deleted.

- **Mock adapter handling:** `tau_ports::fixtures::MockSandbox` stays where it is (sub-project H from the followups doc handles its prod-binary cleanup — out of scope here). The `TAU_TESTING_ALLOW_MOCK_SANDBOX=1` env-var gate from priority 12 task 10 is preserved unchanged. The new resolver does NOT include Mock in the registry; instead, when the env var is set, `resolve_adapter` synthesizes a Mock adapter and returns it directly (priority 12's pattern — Mock is opt-in only, never silent fallback). When the env var is unset, Mock is unreachable from CLI integration tests (existing CI behavior; tests that need Mock set the env var explicitly via `assert_cmd`).

- **Test fixture pattern (priorities 5/6/7/10/11/12 carryover):** all CLI integration tests use `assert_cmd::Command::cargo_bin("tau")` + `tempfile::TempDir`. Mirror existing patterns in `crates/tau-cli/tests/cmd_resolve_check_sandbox.rs` and `crates/tau-cli/tests/cmd_chat.rs`.

- **Three-bucket exit codes (ADR-0007 §7):** sandbox configuration error → exit 2; resolver `NoAdapterMatches` → exit 2; plugin-tier mismatch surfaced from `tau resolve --check-sandbox` → exit 2; sandbox runtime violation by a plugin → `ToolError` (recoverable; agent loop continues to next tool, unchanged from priority 12).

- **JSON event-per-line streaming convention (ADR-0011 carryover):** if `tau sandbox status --json` ships, follow the same per-line event pattern as `tau resolve --check-sandbox --json` already does.

- **Existing schema v1 → v2 auto-upgrade (priority 12)** must NOT break. v1 → v3 transparent (v1 has no `[sandbox]` section; default to `required_tier = "strict"`). v2 → v3 is the migration with the chain warn. The `proptest_scope_config.rs` test that broke during priority 12 task 7 is the canary — keep it green.

- **`tracing` dep on `tau-pkg`** was added in priority 12 for the lockfile schema v3→v4 warn. Reuse for the scope-config v2→v3 migration warn.

- **`PluginHostOptions.sandbox_adapter` field** is added in Task 6 (this sub-project), populated by `tau-cli::cmd::plugin_loader::load_plugins`. The four `load_*` call sites in `plugin_host/mod.rs` continue to receive `Option<&SandboxPlan>` per priority 12 task 9; this sub-project doesn't change those signatures, only stops passing `None` from the CLI.

- **Cargo.lock fixup discipline:** if any task adds a new external dep, include `Cargo.lock` in the same commit. None of the tasks here are expected to add external deps; reuses existing `tau-domain`, `tau-ports`, `tau-pkg`, `tau-runtime`, `tau-sandbox-native`, `tau-sandbox-container`, `tau-cli` infrastructure.

- **Branch protection stays at 25 required checks.** No new CI matrix slots, no new jobs, no new `[[test]]` workspace members that need separate CI invocations.

- **Doctest hygiene:** all rustdoc `///` examples on `#[non_exhaustive]` types use `ignore` fences. If a code example demonstrates the schema (e.g., showing a TOML snippet for `[sandbox]`), use `text` fenced blocks rather than `rust` so rustdoc doesn't try to compile them.

- **`build_plan` + `validate_plan_against_adapter` from priority 12 are reused as-is.** This sub-project does NOT change Layer 3 validation logic; it changes adapter SELECTION (resolver replaces `select_adapter`) and adds plugin-tier checking as a filter inside the resolver.

---

## File structure

| File | Status | Responsibility |
|---|---|---|
| `crates/tau-pkg/src/scope.rs` | modify | Bump schema v2→v3; replace `SandboxConfig` with `SandboxRequirements`; auto-migrate v2 entries with warn. |
| `crates/tau-pkg/tests/proptest_scope_config.rs` | modify | Update `supported: 2` → `supported: 3` and add v2-migration round-trip test. |
| `crates/tau-domain/src/package/manifest.rs` | modify | Add `PluginSandboxRequirements` field on `UncheckedManifest`; expose accessor on `PackageManifest`. |
| `crates/tau-domain/src/package/sandbox.rs` | create | New module owning `PluginSandboxRequirements` type + serde wiring. |
| `crates/tau-domain/src/package/mod.rs` | modify | Add `pub mod sandbox;` + re-export. |
| `crates/tau-domain/src/lib.rs` | modify | Re-export `PluginSandboxRequirements`. |
| `crates/tau-runtime/src/sandbox/registry.rs` | create | `AdapterRegistration` struct + `REGISTRY` static + `detect_platform()`. |
| `crates/tau-runtime/src/sandbox/passthrough.rs` | create | `PassthroughSandbox` adapter. |
| `crates/tau-runtime/src/sandbox/resolver.rs` | create | `resolve_adapter` + `ResolutionError` + `ResolutionRejection` + `SandboxRequirementsResolved`. |
| `crates/tau-runtime/src/sandbox/chain.rs` | delete | Logic relocated; `SandboxAdapter` enum moves into `resolver.rs`. |
| `crates/tau-runtime/src/sandbox/mod.rs` | modify | Re-export new modules; drop `chain` re-exports. |
| `crates/tau-runtime/src/plugin_host/mod.rs` | modify | Add `sandbox_adapter: Option<Arc<SandboxAdapter>>` field on `PluginHostOptions`. |
| `crates/tau-cli/src/cli.rs` | modify | Add global `--no-sandbox` and `--sandbox <kind>` flags on `Cli`; add `Sandbox(SandboxArgs)` subcommand variant; add `SandboxArgs` enum (Status / Setup). |
| `crates/tau-cli/src/cmd/plugin_loader.rs` | modify | Read scope's `[sandbox]`, build `SandboxRequirements`, call `resolve_adapter`, build per-plugin `SandboxPlan`, set `PluginHostOptions.sandbox_adapter`, call `load_*` with `Some(&plan)`. |
| `crates/tau-cli/src/cmd/sandbox.rs` | create | `tau sandbox status` and `tau sandbox setup` subcommand handlers. |
| `crates/tau-cli/src/cmd/mod.rs` | modify | Wire `sandbox` module + dispatch. |
| `crates/tau-cli/src/cmd/error_render.rs` | create | Guided multi-option renderer for `ResolutionError` + plugin-tier mismatches. |
| `crates/tau-cli/src/cmd/resolve.rs` | modify | Extend `run_check_sandbox` to surface plugin-tier mismatches; handle passthrough-skip-to-next-strict logic. |
| `crates/tau-cli/tests/cmd_resolve_check_sandbox.rs` | modify | Rewrite `[sandbox] chain = [...]` fixtures → `[sandbox] required_tier = "..."`; add plugin-tier-mismatch test. |
| `crates/tau-cli/tests/cmd_sandbox_status.rs` | create | Integration tests for `tau sandbox status`. |
| `crates/tau-cli/tests/cmd_sandbox_setup.rs` | create | Integration tests for `tau sandbox setup`. |
| `crates/tau-cli/tests/cmd_no_sandbox_flag.rs` | create | Integration tests for `--no-sandbox` and `--sandbox <kind>`. |
| `crates/tau-cli/tests/snapshots/help_snapshots__resolve_help.snap` | modify | Re-snapshot after the `--check-sandbox` extension surfaces plugin tier (only if help text changes). |
| `crates/tau-cli/tests/snapshots/help_snapshots__cli_help.snap` | modify | Re-snapshot after adding `--no-sandbox`/`--sandbox`/`tau sandbox` to the top-level help. |
| `docs/decisions/0015-sandbox-activation.md` | create | ADR-0015 with the 6 D-decisions. |
| `ROADMAP.md` | modify | Mark sub-project A done; reference ADR-0015. |
| `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` | modify | Mark Sub-project A as DONE; cross-reference ADR-0015. |

---

## Tasks

### Task 1: scope config schema v2 → v3 (`SandboxRequirements`)

**Why first:** every other task references `tau_pkg::scope::SandboxRequirements`. The schema change has to land first so subsequent tasks have a stable type to import.

**Files:**
- Modify: `crates/tau-pkg/src/scope.rs`
- Modify: `crates/tau-pkg/tests/proptest_scope_config.rs`
- Test: inline `#[cfg(test)]` module in `crates/tau-pkg/src/scope.rs`

- [ ] **Step 1: Read the existing file to confirm anchors.**

Run: `grep -n "MAX_SUPPORTED_SCOPE_CONFIG_SCHEMA_VERSION\|pub struct SandboxConfig\|pub struct SandboxAdapterConfig\|pub enum SandboxAdapterKind\|pub enum SandboxMinimumTier\|pub struct ScopeConfig" crates/tau-pkg/src/scope.rs`

Expected output:
```
28:pub const MAX_SUPPORTED_SCOPE_CONFIG_SCHEMA_VERSION: u32 = 2;
49:pub struct SandboxConfig {
80:pub struct SandboxAdapterConfig {
104:pub enum SandboxAdapterKind {
120:pub enum SandboxMinimumTier {
151:pub struct ScopeConfig {
```

- [ ] **Step 2: Write failing tests (TDD red).**

Append at the end of `crates/tau-pkg/src/scope.rs`'s existing `#[cfg(test)] mod tests` block (find it via `grep -n "^mod tests" crates/tau-pkg/src/scope.rs`). Add these eight tests:

```rust
#[test]
fn sandbox_requirements_default_is_strict() {
    let req = SandboxRequirements::default();
    assert_eq!(req.required_tier, SandboxRequiredTier::Strict);
    assert!(req.required_shapes.is_empty());
}

#[test]
fn scope_config_default_sandbox_is_strict() {
    let cfg = ScopeConfig::new(ScopeKind::Project);
    assert_eq!(cfg.schema_version, 3);
    assert_eq!(cfg.sandbox.required_tier, SandboxRequiredTier::Strict);
}

#[test]
fn v3_config_round_trips_through_toml() {
    let cfg = ScopeConfig::new(ScopeKind::Project);
    let toml = cfg.to_toml_string().unwrap();
    let parsed = ScopeConfig::read_from_str(&toml).unwrap();
    assert_eq!(cfg, parsed);
}

#[test]
fn v3_config_with_explicit_required_shapes() {
    let toml = r#"
schema_version = 3
kind = "project"
created_at = "2026-05-04T00:00:00Z"
created_by_tau_version = "0.0.0"

[sandbox]
required_tier = "light"
required_shapes = ["filesystem-read", "network-http"]
"#;
    let parsed = ScopeConfig::read_from_str(toml).unwrap();
    assert_eq!(parsed.sandbox.required_tier, SandboxRequiredTier::Light);
    assert_eq!(parsed.sandbox.required_shapes.len(), 2);
}

#[test]
fn v2_config_with_chain_auto_migrates_to_v3() {
    let toml = r#"
schema_version = 2
kind = "project"
created_at = "2026-05-04T00:00:00Z"
created_by_tau_version = "0.0.0"

[sandbox]
chain = [{ kind = "native" }, { kind = "container" }]
minimum_tier = "strict"
"#;
    let parsed = ScopeConfig::read_from_str(toml).unwrap();
    assert_eq!(parsed.schema_version, 3, "schema_version should bump to 3 in memory");
    assert_eq!(parsed.sandbox.required_tier, SandboxRequiredTier::Strict);
}

#[test]
fn v2_config_without_minimum_tier_defaults_to_strict() {
    let toml = r#"
schema_version = 2
kind = "project"
created_at = "2026-05-04T00:00:00Z"
created_by_tau_version = "0.0.0"

[sandbox]
chain = [{ kind = "native" }]
"#;
    let parsed = ScopeConfig::read_from_str(toml).unwrap();
    assert_eq!(parsed.sandbox.required_tier, SandboxRequiredTier::Strict);
}

#[test]
fn v1_config_loads_with_default_v3_sandbox() {
    let toml = r#"
schema_version = 1
kind = "project"
created_at = "2026-05-04T00:00:00Z"
created_by_tau_version = "0.0.0"
"#;
    let parsed = ScopeConfig::read_from_str(toml).unwrap();
    assert_eq!(parsed.sandbox.required_tier, SandboxRequiredTier::Strict);
}

#[test]
fn schema_too_new_rejected() {
    let toml = r#"
schema_version = 999
kind = "project"
created_at = "2026-05-04T00:00:00Z"
created_by_tau_version = "0.0.0"
"#;
    let err = ScopeConfig::read_from_str(toml).unwrap_err();
    assert!(matches!(err, ScopeError::ConfigSchemaTooNew { found: 999, supported: 3 }));
}
```

- [ ] **Step 3: Run the tests to confirm RED.**

Run: `cargo test -p tau-pkg --lib sandbox_requirements_default_is_strict scope_config_default_sandbox_is_strict v3_config_round_trips_through_toml v3_config_with_explicit_required_shapes v2_config_with_chain_auto_migrates_to_v3 v2_config_without_minimum_tier_defaults_to_strict v1_config_loads_with_default_v3_sandbox schema_too_new_rejected`
Expected: compile errors — `SandboxRequirements`, `SandboxRequiredTier` not defined; `schema_version = 3` mismatches `MAX_SUPPORTED_SCOPE_CONFIG_SCHEMA_VERSION = 2`.

- [ ] **Step 4: Bump the schema version constant.**

Edit `crates/tau-pkg/src/scope.rs` line 28:

```rust
/// Maximum `ScopeConfig::schema_version` this tau version recognizes.
/// A `config.toml` with a higher value rejects with
/// [`ScopeError::ConfigSchemaTooNew`].
pub const MAX_SUPPORTED_SCOPE_CONFIG_SCHEMA_VERSION: u32 = 3;
```

- [ ] **Step 5: Replace `SandboxConfig` + supporting types with `SandboxRequirements`.**

Replace the block from `pub struct SandboxConfig` (line 47) through `pub enum SandboxMinimumTier` (line 127, end of brace) with:

```rust
/// Per-scope sandbox requirements.
///
/// Lives under `[sandbox]` in `<scope>/config.toml`. Schema version 3.
///
/// Replaces the schema-v2 `SandboxConfig` (which used `chain` +
/// `minimum_tier`). v2 configs auto-migrate at load time with a
/// `tracing::warn!`. See [`SandboxRequirements::deserialize_with_migration`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SandboxRequirements {
    /// Minimum sandbox tier this project requires. Defaults to `Strict`.
    /// Setting `None` is the persistent opt-out (allows passthrough to satisfy).
    #[serde(default)]
    pub required_tier: SandboxRequiredTier,
    /// Explicit shape requirements. Optional. When empty, the resolver
    /// auto-derives the union of shapes from each plugin's declared
    /// capabilities.
    #[serde(default)]
    pub required_shapes: Vec<tau_domain::CapabilityShape>,
}

impl SandboxRequirements {
    /// Construct with an explicit tier. Shapes default to empty
    /// (auto-derive at resolution time).
    pub fn with_tier(required_tier: SandboxRequiredTier) -> Self {
        Self {
            required_tier,
            required_shapes: Vec::new(),
        }
    }
}

/// Required sandbox tier for a project (or plugin). Mirrors
/// `tau_ports::SandboxTier` but lives at the config layer (we don't
/// depend on tau-ports from tau-pkg). The runtime maps these to
/// `SandboxTier`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxRequiredTier {
    /// No tier requirement (allows passthrough to satisfy).
    None,
    /// Filesystem isolation at minimum.
    Light,
    /// Full strict tier required.
    Strict,
}

impl Default for SandboxRequiredTier {
    fn default() -> Self {
        SandboxRequiredTier::Strict
    }
}
```

- [ ] **Step 6: Update `ScopeConfig.sandbox` field type and `ScopeConfig::new` schema version.**

Find the `pub struct ScopeConfig { ... pub sandbox: SandboxConfig, ... }` and change the field type:

```rust
    /// Sandbox requirements. Defaults to `required_tier = "strict"` and
    /// empty `required_shapes` (auto-derive at resolution time).
    #[serde(default, deserialize_with = "deserialize_sandbox_with_migration")]
    pub sandbox: SandboxRequirements,
```

In `impl ScopeConfig` find `pub fn new` and update it:

```rust
    /// Construct a new `ScopeConfig` with the current time, the current
    /// crate version, schema version 3, and default sandbox requirements.
    pub fn new(kind: ScopeKind) -> Self {
        Self {
            schema_version: MAX_SUPPORTED_SCOPE_CONFIG_SCHEMA_VERSION,
            kind,
            created_at: SystemTime::now(),
            created_by_tau_version: env!("CARGO_PKG_VERSION").to_owned(),
            defaults: BTreeMap::new(),
            sandbox: SandboxRequirements::default(),
        }
    }
```

Also update the rustdoc example assertion `assert!(toml.contains("schema_version = 3"));` (was `2`).

- [ ] **Step 7: Add the v2 → v3 migration deserializer.**

Insert after the `SandboxRequiredTier` impl block:

```rust
/// `serde` `deserialize_with` handler for the `sandbox` field of `ScopeConfig`.
///
/// Detects v2 schema (`chain` + `minimum_tier` keys) and best-effort
/// migrates to v3 (`required_tier`). Emits a once-per-process
/// `tracing::warn!` on migration. v3 configs (with `required_tier`)
/// pass through unchanged.
fn deserialize_sandbox_with_migration<'de, D>(
    deserializer: D,
) -> Result<SandboxRequirements, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    // Permissive raw shape that accepts both v2 and v3 keys.
    #[derive(Deserialize)]
    struct Raw {
        // v3 fields
        #[serde(default)]
        required_tier: Option<SandboxRequiredTier>,
        #[serde(default)]
        required_shapes: Option<Vec<tau_domain::CapabilityShape>>,
        // v2 fields (best-effort migration)
        #[serde(default)]
        chain: Option<toml::Value>,
        #[serde(default)]
        minimum_tier: Option<SandboxRequiredTier>, // same names, same serde
    }

    let raw = Raw::deserialize(deserializer)?;

    // v3 path: required_tier is the canonical signal.
    if raw.required_tier.is_some() || raw.required_shapes.is_some() {
        if raw.chain.is_some() || raw.minimum_tier.is_some() {
            warn_v2_v3_mixed();
        }
        return Ok(SandboxRequirements {
            required_tier: raw.required_tier.unwrap_or_default(),
            required_shapes: raw.required_shapes.unwrap_or_default(),
        });
    }

    // v2 path: derive required_tier from minimum_tier (or default to Strict);
    // ignore chain entries (the v3 resolver handles platform matching).
    if raw.chain.is_some() || raw.minimum_tier.is_some() {
        warn_v2_migration();
        return Ok(SandboxRequirements {
            required_tier: raw.minimum_tier.unwrap_or_default(),
            required_shapes: Vec::new(),
        });
    }

    // Empty `[sandbox]` block — pure default.
    Ok(SandboxRequirements::default())
}

fn warn_v2_migration() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        tracing::warn!(
            "scope config uses deprecated v2 [sandbox] schema (chain + minimum_tier). \
             Auto-migrating to v3 (required_tier derived from minimum_tier). \
             Run `tau sandbox setup` to rewrite the config in v3 form."
        );
    });
}

fn warn_v2_v3_mixed() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        tracing::warn!(
            "scope config mixes v2 ([sandbox] chain/minimum_tier) and v3 (required_tier) keys. \
             Honoring v3 keys; ignoring v2 keys."
        );
    });
}
```

- [ ] **Step 8: Update the SandboxConfig-based existing tests in scope.rs.**

Find the existing tests that reference `SandboxConfig`, `SandboxAdapterConfig`, `SandboxAdapterKind`, `SandboxMinimumTier`. They typically read `cfg.sandbox.chain` or build a chain via `SandboxConfig::with_chain(...)`. Each such test must be either:

- (a) deleted if its purpose was schema-v2-only, OR
- (b) rewritten against `SandboxRequirements` if the test exercises generic schema behavior.

Run: `grep -n "SandboxConfig\|SandboxAdapterConfig\|SandboxAdapterKind\|SandboxMinimumTier" crates/tau-pkg/src/scope.rs`

For every match outside the deleted struct definitions: rewrite or remove. Concretely, the tests `sandbox_config_default_is_empty_chain`, `sandbox_config_round_trips_through_toml`, `unknown_adapter_kind_rejected`, `mixed_adapter_chain_round_trips` are schema-v2-specific; delete them. The test `v1_config_loads_with_empty_sandbox` is replaced by the new `v1_config_loads_with_default_v3_sandbox` from Step 2.

- [ ] **Step 9: Update `proptest_scope_config.rs` for schema v3.**

Edit `crates/tau-pkg/tests/proptest_scope_config.rs`:

- Find `supported: 2` and change to `supported: 3`.
- Find the `let cfg = ScopeConfig::new(ScopeKind::Project)` (or similar) round-trip block; if it asserts `schema_version == 2`, change to `schema_version == 3`.
- Add a new test `v2_chain_config_round_trips_via_migration`:

```rust
#[test]
fn v2_chain_config_round_trips_via_migration() {
    let v2_toml = r#"
schema_version = 2
kind = "project"
created_at = "2026-05-04T00:00:00Z"
created_by_tau_version = "0.0.0"

[sandbox]
chain = [{ kind = "native" }]
minimum_tier = "light"
"#;
    let parsed = ScopeConfig::read_from_str(v2_toml).expect("v2 should auto-migrate");
    assert_eq!(parsed.sandbox.required_tier.to_string(), "light");
    let re_serialized = parsed.to_toml_string().expect("re-serialize");
    assert!(re_serialized.contains("schema_version = 3"));
    assert!(re_serialized.contains("required_tier = \"light\""));
}
```

- [ ] **Step 10: Run the tests to confirm GREEN.**

Run: `cargo test -p tau-pkg --lib`
Expected: all sandbox-related tests pass; no compile errors.

Run: `cargo test -p tau-pkg --tests`
Expected: `proptest_scope_config` tests pass.

- [ ] **Step 11: Run workspace gates.**

Run in order:
- `cargo build --workspace`
- `cargo test --workspace --all-targets`  ← MUST use `--all-targets`, not `--lib`
- `cargo test --doc`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`

If any downstream crate breaks (likely candidates: `tau-runtime/src/sandbox/chain.rs` which imports `SandboxConfig`), the failures are EXPECTED — Task 5 deletes that file. Acknowledge the breakage but do NOT fix it in this task; the workspace gate will be red until Task 5 lands. Run the macro-level command:

`cargo build --workspace 2>&1 | grep -E "^(error|warning):" | head -20`

Confirm the only errors trace back to `chain.rs` referencing the now-removed `SandboxConfig`.

- [ ] **Step 12: Commit.**

Stage exactly: `crates/tau-pkg/src/scope.rs`, `crates/tau-pkg/tests/proptest_scope_config.rs`.

Run: `git add crates/tau-pkg/src/scope.rs crates/tau-pkg/tests/proptest_scope_config.rs`

Run:
```
git commit -m "feat(pkg): scope config schema v2 -> v3 (SandboxRequirements)

Replace SandboxConfig (chain + minimum_tier) with SandboxRequirements
(required_tier + required_shapes). v2 configs auto-migrate via a
serde deserialize_with handler that detects v2 keys and emits a
once-per-process tracing::warn. v1 configs (no [sandbox] block)
default to required_tier = strict.

Schema version constant bumped 2 -> 3. Existing v1 -> v2 auto-upgrade
preserved.

This task is the foundation for sub-project A (sandbox activation);
the workspace will not build until task 5 deletes the obsolete
chain.rs that imports the now-removed SandboxConfig type."
```

(No `Cargo.lock` changes — no new external deps in this task.)

---

### Task 2: plugin manifest `[sandbox]` block (`PluginSandboxRequirements`)

**Why second:** plugins need to declare `required_tier` so the resolver can filter adapters by plugin floor. Independent of Task 1 in code (lives in `tau-domain`, not `tau-pkg`), but ordered second because the resolver in Task 5 needs both Task 1 (project requirements) and Task 2 (plugin requirements) types available.

**Files:**
- Create: `crates/tau-domain/src/package/sandbox.rs`
- Modify: `crates/tau-domain/src/package/mod.rs`
- Modify: `crates/tau-domain/src/package/manifest.rs`
- Modify: `crates/tau-domain/src/lib.rs`
- Test: inline `#[cfg(test)] mod tests` in the new `sandbox.rs` + extension to `manifest.rs`'s existing `mod manifest_tests`.

- [ ] **Step 1: Confirm anchors.**

Run: `grep -n "pub struct UncheckedManifest\|pub struct PackageManifest\|pub mod " crates/tau-domain/src/package/mod.rs crates/tau-domain/src/package/manifest.rs`

Expected output includes:
```
crates/tau-domain/src/package/mod.rs:3:pub mod capability;
crates/tau-domain/src/package/mod.rs:4:pub mod manifest;
crates/tau-domain/src/package/mod.rs:5:pub mod plugin;
crates/tau-domain/src/package/mod.rs:6:pub mod source;
crates/tau-domain/src/package/manifest.rs:203:pub struct UncheckedManifest {
crates/tau-domain/src/package/manifest.rs:247:pub struct PackageManifest(UncheckedManifest);
```

- [ ] **Step 2: Create `crates/tau-domain/src/package/sandbox.rs`.**

```rust
//! Plugin-side sandbox requirements declared in `tau.toml`'s `[sandbox]`
//! table. Optional; absent means the plugin asserts no tier or shape
//! floor and is satisfied by any adapter.

use crate::package::capability::CapabilityShape;

/// Plugin-side sandbox requirements.
///
/// A plugin can declare `[sandbox] required_tier = "strict"` in its
/// `tau.toml` to refuse loading when the host can only deliver weaker
/// enforcement (e.g., passthrough). Symmetric to project-side
/// [`tau_pkg::scope::SandboxRequirements`].
///
/// Both fields are optional with `#[serde(default)]`. A plugin with no
/// `[sandbox]` block parses to `PluginSandboxRequirements::default()`,
/// which imposes no floor.
///
/// # Example
///
/// ```ignore
/// // PluginSandboxRequirements is `#[non_exhaustive]`; construct via
/// // serde from a TOML manifest, not via struct literal.
/// use tau_domain::PluginSandboxRequirements;
/// let _ = PluginSandboxRequirements::default();
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PluginSandboxRequirements {
    /// Minimum sandbox tier this plugin requires. `None` means no
    /// floor; any adapter is acceptable. The serialized values are
    /// `"none"`, `"light"`, `"strict"`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub required_tier: Option<PluginRequiredTier>,
    /// Additional shape requirements beyond what the plugin's declared
    /// capabilities imply. Optional; the resolver auto-derives the
    /// shape set from the plugin's `[capabilities]` block when this is
    /// empty.
    #[cfg_attr(feature = "serde", serde(default))]
    pub required_shapes: Vec<CapabilityShape>,
}

/// Tier value usable in plugin manifests. Mirrors
/// `tau_pkg::scope::SandboxRequiredTier` shape; defined here to keep
/// `tau-domain` free of `tau-pkg` dependencies.
///
/// The runtime maps `PluginRequiredTier` to `tau_ports::SandboxTier`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum PluginRequiredTier {
    /// No floor; any tier acceptable.
    None,
    /// Filesystem isolation at minimum.
    Light,
    /// Full strict tier required.
    Strict,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "serde")]
    #[test]
    fn plugin_sandbox_requirements_default_is_unconstrained() {
        let req = PluginSandboxRequirements::default();
        assert!(req.required_tier.is_none());
        assert!(req.required_shapes.is_empty());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn plugin_sandbox_requirements_round_trip_strict() {
        let toml = r#"
required_tier = "strict"
"#;
        let parsed: PluginSandboxRequirements = toml::from_str(toml).unwrap();
        assert_eq!(parsed.required_tier, Some(PluginRequiredTier::Strict));
        assert!(parsed.required_shapes.is_empty());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn plugin_sandbox_requirements_with_explicit_shapes() {
        let toml = r#"
required_tier = "light"
required_shapes = ["filesystem-read", "network-http"]
"#;
        let parsed: PluginSandboxRequirements = toml::from_str(toml).unwrap();
        assert_eq!(parsed.required_tier, Some(PluginRequiredTier::Light));
        assert_eq!(parsed.required_shapes.len(), 2);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn plugin_sandbox_requirements_empty_block_round_trip() {
        let toml = "";
        let parsed: PluginSandboxRequirements = toml::from_str(toml).unwrap_or_default();
        assert!(parsed.required_tier.is_none());
    }

    #[test]
    fn plugin_required_tier_ordering() {
        assert!(PluginRequiredTier::None < PluginRequiredTier::Light);
        assert!(PluginRequiredTier::Light < PluginRequiredTier::Strict);
    }
}
```

- [ ] **Step 3: Wire the new module in `crates/tau-domain/src/package/mod.rs`.**

Add `pub mod sandbox;` next to the existing `pub mod capability;`.

Add re-exports near the existing `pub use ...` block:

```rust
pub use sandbox::{PluginRequiredTier, PluginSandboxRequirements};
```

- [ ] **Step 4: Add the `sandbox` field to `UncheckedManifest` and the accessor on `PackageManifest`.**

In `crates/tau-domain/src/package/manifest.rs`, find `pub struct UncheckedManifest { ... pub plugin: Option<PluginManifest>, }` and add a new field BEFORE the closing brace:

```rust
    /// Plugin-side sandbox requirements declared via `[sandbox]` table.
    ///
    /// Optional. Default = `PluginSandboxRequirements::default()` (no
    /// tier floor; auto-derived shapes). See [`PluginSandboxRequirements`].
    #[cfg_attr(feature = "serde", serde(default))]
    pub sandbox: crate::package::sandbox::PluginSandboxRequirements,
```

In `impl PackageManifest`, add an accessor method after the existing `plugin()` accessor:

```rust
    /// Plugin-side sandbox requirements (from `[sandbox]` table).
    pub fn sandbox(&self) -> &crate::package::sandbox::PluginSandboxRequirements {
        &self.0.sandbox
    }
```

- [ ] **Step 5: Re-export from `crates/tau-domain/src/lib.rs`.**

Find the existing re-export of package types (typically `pub use package::manifest::{...};`) and add:

```rust
pub use package::sandbox::{PluginRequiredTier, PluginSandboxRequirements};
```

- [ ] **Step 6: Add an integration test inside `manifest.rs::manifest_tests`.**

Find the existing `#[cfg(test)] mod manifest_tests` block (around line 335). Add:

```rust
    #[cfg(feature = "serde")]
    #[test]
    fn manifest_with_sandbox_block_parses() {
        let toml = r#"
name = "my-plugin"
version = "0.1.0"
description = "test"
authors = []
source = { type = "local", path = "/tmp/x" }
kind = "tool"

[[capabilities]]
kind = "fs.read"
paths = ["/tmp/**"]

[plugin]
provides = "tool"
kind = "rust-cargo"
bin = "my-plugin"

[sandbox]
required_tier = "strict"
"#;
        let unchecked: UncheckedManifest = toml::from_str(toml).expect("parse");
        let manifest = unchecked.validate().expect("validate");
        assert_eq!(
            manifest.sandbox().required_tier,
            Some(crate::package::sandbox::PluginRequiredTier::Strict)
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn manifest_without_sandbox_block_defaults() {
        let toml = r#"
name = "my-plugin"
version = "0.1.0"
description = "test"
authors = []
source = { type = "local", path = "/tmp/x" }
kind = "tool"

[plugin]
provides = "tool"
kind = "rust-cargo"
bin = "my-plugin"
"#;
        let unchecked: UncheckedManifest = toml::from_str(toml).expect("parse");
        let manifest = unchecked.validate().expect("validate");
        assert!(manifest.sandbox().required_tier.is_none());
    }
```

(The exact `source = { type = "local", path = "/tmp/x" }` syntax depends on `PackageSource`'s real serde format; check with `grep -n "pub enum PackageSource\|impl.*Deserialize.*for PackageSource" crates/tau-domain/src/package/source.rs` and adapt the literal in the test if the syntax differs.)

- [ ] **Step 7: Run the new tests to confirm GREEN.**

Run: `cargo test -p tau-domain --lib`
Expected: 5 sandbox tests + 2 manifest tests pass.

- [ ] **Step 8: Run workspace gates.**

Run: `cargo build --workspace && cargo test --workspace --all-targets && cargo test --doc && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`

Note: `cargo build --workspace` may still fail because of Task 1's chain.rs breakage. That's expected; Task 5 fixes it. Check that no NEW breakage was introduced by THIS task by comparing the list of error files: `cargo build --workspace 2>&1 | grep "^error" | sort -u` should report ONLY `chain.rs` errors (carried over from Task 1).

- [ ] **Step 9: Commit.**

Stage exactly: `crates/tau-domain/src/package/sandbox.rs`, `crates/tau-domain/src/package/mod.rs`, `crates/tau-domain/src/package/manifest.rs`, `crates/tau-domain/src/lib.rs`.

Run: `git add crates/tau-domain/src/package/sandbox.rs crates/tau-domain/src/package/mod.rs crates/tau-domain/src/package/manifest.rs crates/tau-domain/src/lib.rs`

Run:
```
git commit -m "feat(domain): plugin manifest [sandbox] block (PluginSandboxRequirements)

Add optional [sandbox] table to plugin tau.toml manifests, with
required_tier (None / Light / Strict) and required_shapes fields.
Plugins that don't declare a [sandbox] block parse to
PluginSandboxRequirements::default(), imposing no tier or shape
floor — preserving v0.1 manifest compatibility.

Symmetric to project-side SandboxRequirements (Task 1). The runtime
resolver (Task 5) checks every plugin's required_tier against the
resolved adapter's delivered tier as part of Layer 3 validation."
```

(No `Cargo.lock` changes — no new external deps.)

---

### Task 3: adapter registry + Passthrough adapter

**Why third:** the resolver in Task 5 walks the registry. The registry has to exist before the resolver can be written.

**Files:**
- Create: `crates/tau-runtime/src/sandbox/registry.rs`
- Create: `crates/tau-runtime/src/sandbox/passthrough.rs`
- Modify: `crates/tau-runtime/src/sandbox/mod.rs`
- Test: inline `#[cfg(test)]` in both new files.

- [ ] **Step 1: Confirm anchors.**

Run: `grep -n "^pub mod\|^pub use" crates/tau-runtime/src/sandbox/mod.rs`

Expected:
```
1:pub mod chain;
... (other re-exports)
```

- [ ] **Step 2: Create `crates/tau-runtime/src/sandbox/passthrough.rs`.**

```rust
//! Passthrough sandbox adapter — no isolation; explicit opt-out path.
//!
//! Selected only when the project's `required_tier` is `None` OR the
//! `--no-sandbox` CLI flag is set. The default chain (Native + Container)
//! does NOT include passthrough; selection is always explicit.

use std::process::Command;

use tau_domain::{CapabilityShape, CapabilityShapeSet};
use tau_ports::{
    Sandbox, SandboxError, SandboxHandle, SandboxPlan, SandboxProbe, SandboxTier,
};

/// Passthrough sandbox adapter (no isolation).
///
/// Implements [`tau_ports::Sandbox`]:
/// - `probe()` returns `Available { tier: None, details: "passthrough (no isolation)" }`.
/// - `supported_shapes()` returns the union of all known shapes (so any
///   Layer-3 shape check passes).
/// - `validate_plan(_)` always returns `Ok(())`.
/// - `wrap_spawn(_, _)` is a no-op; returns `SandboxHandle::noop()`.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct PassthroughSandbox;

impl PassthroughSandbox {
    /// Construct a fresh passthrough adapter.
    pub fn new() -> Self {
        Self
    }
}

impl Sandbox for PassthroughSandbox {
    fn name(&self) -> &str {
        "passthrough"
    }

    async fn probe(&self) -> SandboxProbe {
        SandboxProbe::Available {
            tier: SandboxTier::None,
            details: "passthrough (no isolation)".to_owned(),
        }
    }

    fn supported_shapes(&self) -> CapabilityShapeSet {
        let mut set = CapabilityShapeSet::new();
        set.insert(CapabilityShape::FilesystemRead);
        set.insert(CapabilityShape::FilesystemWrite);
        set.insert(CapabilityShape::ProcessExec);
        set.insert(CapabilityShape::NetworkHttp);
        set.insert(CapabilityShape::AgentSpawn);
        set
    }

    fn validate_plan(&self, _plan: &SandboxPlan) -> Result<(), SandboxError> {
        Ok(())
    }

    async fn wrap_spawn(
        &self,
        _plan: &SandboxPlan,
        _cmd: &mut Command,
    ) -> Result<SandboxHandle, SandboxError> {
        Ok(SandboxHandle::noop())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn probe_reports_available_with_tier_none() {
        let p = PassthroughSandbox::new();
        let probe = p.probe().await;
        match probe {
            SandboxProbe::Available { tier, details } => {
                assert_eq!(tier, SandboxTier::None);
                assert!(details.contains("passthrough"));
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn supported_shapes_includes_all_known() {
        let p = PassthroughSandbox::new();
        let shapes = p.supported_shapes();
        assert!(shapes.contains(&CapabilityShape::FilesystemRead));
        assert!(shapes.contains(&CapabilityShape::FilesystemWrite));
        assert!(shapes.contains(&CapabilityShape::ProcessExec));
        assert!(shapes.contains(&CapabilityShape::NetworkHttp));
        assert!(shapes.contains(&CapabilityShape::AgentSpawn));
    }

    #[test]
    fn validate_plan_always_ok() {
        let p = PassthroughSandbox::new();
        let plan = SandboxPlan::new(vec![], None, None);
        assert!(p.validate_plan(&plan).is_ok());
    }

    #[tokio::test]
    async fn wrap_spawn_returns_noop_handle() {
        let p = PassthroughSandbox::new();
        let plan = SandboxPlan::new(vec![], None, None);
        let mut cmd = Command::new("/bin/true");
        let _h = p.wrap_spawn(&plan, &mut cmd).await.expect("wrap_spawn");
        // No assertion on the handle itself — Drop is what matters; the
        // cleanup closure is None so Drop is a no-op.
    }
}
```

- [ ] **Step 3: Create `crates/tau-runtime/src/sandbox/registry.rs`.**

```rust
//! Internal adapter registry — not user-facing config.
//!
//! Each registered adapter declares: kind, supported platforms, supported
//! tiers, supported shapes, priority, and a constructor function. The
//! resolver ([`crate::sandbox::resolver::resolve_adapter`]) walks the
//! registry, filters by detected platform / probe / tier / shape /
//! plugin-tier-floor, and picks the highest-priority survivor.
//!
//! New adapters are added via tau's source code (or, in Phase 2, via the
//! tau target triple registry sub-project); users do NOT write registry
//! entries.

use tau_domain::{CapabilityShape, CapabilityShapeSet};
use tau_ports::SandboxTier;

/// Set of platforms an adapter applies to.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformSet {
    /// Linux only (e.g., `tau-sandbox-native` requires landlock).
    LinuxOnly,
    /// Linux, macOS, and Windows (e.g., container adapter requires
    /// docker/podman binary; the binary may or may not be present, but
    /// the adapter could in principle work on any of these).
    Multi,
    /// Any platform.
    Any,
}

impl PlatformSet {
    /// Does this set include the given platform name (`"linux"`,
    /// `"macos"`, `"windows"`)?
    pub fn includes(&self, platform: &str) -> bool {
        match self {
            PlatformSet::Any => true,
            PlatformSet::Multi => {
                matches!(platform, "linux" | "macos" | "windows")
            }
            PlatformSet::LinuxOnly => platform == "linux",
        }
    }
}

/// Detect the current platform name.
pub fn detect_platform() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        "unknown"
    }
}

/// Opaque adapter kind identifier in the registry. Each value
/// corresponds to one adapter family (Native, Container, Remote,
/// Passthrough). Internal — users never write these.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegistryKind {
    /// Linux landlock + seccomp + namespaces (`tau-sandbox-native`).
    Native,
    /// docker / podman shell-out (`tau-sandbox-container`).
    Container,
    /// Remote sandbox (Vercel Sandbox / Sandcastle / etc). Phase 2.
    Remote,
    /// No isolation; explicit opt-out (this crate's `passthrough` module).
    Passthrough,
}

impl RegistryKind {
    /// Adapter name as surfaced in logs and error messages.
    pub fn name(&self) -> &'static str {
        match self {
            RegistryKind::Native => "native",
            RegistryKind::Container => "container",
            RegistryKind::Remote => "remote",
            RegistryKind::Passthrough => "passthrough",
        }
    }
}

/// One entry in the adapter registry.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterRegistration {
    /// Adapter kind.
    pub kind: RegistryKind,
    /// Which platforms this adapter applies to.
    pub platforms: PlatformSet,
    /// Tiers this adapter can deliver (in increasing order).
    pub tiers_supported: &'static [SandboxTier],
    /// Shapes this adapter can enforce. Use [`registered_shapes`] to
    /// materialize as a `CapabilityShapeSet`.
    pub shapes_supported: &'static [CapabilityShape],
    /// Priority for tie-breaking (higher = preferred when multiple
    /// candidates pass filtering).
    pub priority: u32,
}

/// Convert `&'static [CapabilityShape]` to a `CapabilityShapeSet`.
pub fn shapes_set(shapes: &'static [CapabilityShape]) -> CapabilityShapeSet {
    let mut set = CapabilityShapeSet::new();
    for s in shapes {
        set.insert(s.clone());
    }
    set
}

/// Reusable shape lists.
mod shape_lists {
    use tau_domain::CapabilityShape;

    pub(crate) const ALL_SHAPES: &[CapabilityShape] = &[
        CapabilityShape::FilesystemRead,
        CapabilityShape::FilesystemWrite,
        CapabilityShape::ProcessExec,
        CapabilityShape::NetworkHttp,
        CapabilityShape::AgentSpawn,
    ];

    pub(crate) const FS_AND_EXEC_AND_NET: &[CapabilityShape] = &[
        CapabilityShape::FilesystemRead,
        CapabilityShape::FilesystemWrite,
        CapabilityShape::ProcessExec,
        CapabilityShape::NetworkHttp,
    ];
}

/// The registry. Static; populated at compile time. Users do NOT modify
/// this; new adapters are added via tau's source code.
pub static REGISTRY: &[AdapterRegistration] = &[
    AdapterRegistration {
        kind: RegistryKind::Native,
        platforms: PlatformSet::LinuxOnly,
        tiers_supported: &[SandboxTier::Light, SandboxTier::Strict],
        shapes_supported: shape_lists::FS_AND_EXEC_AND_NET,
        priority: 100,
    },
    AdapterRegistration {
        kind: RegistryKind::Container,
        platforms: PlatformSet::Multi,
        tiers_supported: &[SandboxTier::Strict],
        shapes_supported: shape_lists::FS_AND_EXEC_AND_NET,
        priority: 50,
    },
    AdapterRegistration {
        kind: RegistryKind::Remote,
        platforms: PlatformSet::Any,
        tiers_supported: &[SandboxTier::Strict],
        shapes_supported: shape_lists::FS_AND_EXEC_AND_NET,
        priority: 25,
    },
    AdapterRegistration {
        kind: RegistryKind::Passthrough,
        platforms: PlatformSet::Any,
        tiers_supported: &[SandboxTier::None],
        shapes_supported: shape_lists::ALL_SHAPES,
        priority: 0,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_four_entries() {
        assert_eq!(REGISTRY.len(), 4);
    }

    #[test]
    fn registry_kinds_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for entry in REGISTRY {
            assert!(
                seen.insert(entry.kind),
                "duplicate kind {:?} in registry",
                entry.kind
            );
        }
    }

    #[test]
    fn priority_ordering_native_first_passthrough_last() {
        let native = REGISTRY.iter().find(|r| r.kind == RegistryKind::Native).unwrap();
        let passthrough = REGISTRY.iter().find(|r| r.kind == RegistryKind::Passthrough).unwrap();
        assert!(native.priority > passthrough.priority);
    }

    #[test]
    fn native_is_linux_only() {
        let native = REGISTRY.iter().find(|r| r.kind == RegistryKind::Native).unwrap();
        assert!(native.platforms.includes("linux"));
        assert!(!native.platforms.includes("macos"));
        assert!(!native.platforms.includes("windows"));
    }

    #[test]
    fn container_is_multi_platform() {
        let c = REGISTRY.iter().find(|r| r.kind == RegistryKind::Container).unwrap();
        assert!(c.platforms.includes("linux"));
        assert!(c.platforms.includes("macos"));
        assert!(c.platforms.includes("windows"));
    }

    #[test]
    fn passthrough_supports_all_shapes() {
        let p = REGISTRY.iter().find(|r| r.kind == RegistryKind::Passthrough).unwrap();
        let set = shapes_set(p.shapes_supported);
        assert!(set.contains(&CapabilityShape::FilesystemRead));
        assert!(set.contains(&CapabilityShape::AgentSpawn));
    }

    #[test]
    fn passthrough_only_delivers_tier_none() {
        let p = REGISTRY.iter().find(|r| r.kind == RegistryKind::Passthrough).unwrap();
        assert_eq!(p.tiers_supported, &[SandboxTier::None]);
    }

    #[test]
    fn detect_platform_returns_known() {
        let p = detect_platform();
        assert!(matches!(p, "linux" | "macos" | "windows" | "unknown"));
    }

    #[test]
    fn registry_kind_names_are_lowercase_kebab() {
        assert_eq!(RegistryKind::Native.name(), "native");
        assert_eq!(RegistryKind::Container.name(), "container");
        assert_eq!(RegistryKind::Remote.name(), "remote");
        assert_eq!(RegistryKind::Passthrough.name(), "passthrough");
    }
}
```

- [ ] **Step 4: Wire the new modules in `crates/tau-runtime/src/sandbox/mod.rs`.**

Add (just below the existing `pub mod chain;` line — chain stays for now; Task 5 deletes it):

```rust
pub mod passthrough;
pub mod registry;
```

- [ ] **Step 5: Run unit tests to confirm GREEN.**

Run: `cargo test -p tau-runtime --lib sandbox::passthrough sandbox::registry`
Expected: 4 passthrough tests + 9 registry tests pass. Total 13 new tests.

- [ ] **Step 6: Run workspace gates.**

`cargo build --workspace && cargo test --workspace --all-targets && cargo test --doc && cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`

The build will still fail on `chain.rs` (carried over from Task 1). That's expected. Confirm no NEW errors.

- [ ] **Step 7: Commit.**

Stage exactly: `crates/tau-runtime/src/sandbox/passthrough.rs`, `crates/tau-runtime/src/sandbox/registry.rs`, `crates/tau-runtime/src/sandbox/mod.rs`.

Run:
```
git add crates/tau-runtime/src/sandbox/passthrough.rs \
        crates/tau-runtime/src/sandbox/registry.rs \
        crates/tau-runtime/src/sandbox/mod.rs

git commit -m "feat(runtime): adapter registry + passthrough sandbox

Add internal AdapterRegistry (Native, Container, Remote, Passthrough)
with platform / tier / shape / priority metadata. Static; users do
not write registry entries.

Add PassthroughSandbox: implements tau_ports::Sandbox with no
isolation; probe reports Available with tier=None; supported_shapes
returns all known shapes; validate_plan always Ok; wrap_spawn is a
no-op. Selected only when project required_tier is None or
--no-sandbox is set; never silent fallback.

Foundation for the resolver (next task), which filters the registry
by detected platform, probe result, required tier, required shapes,
and plugin tier floor."
```

(No `Cargo.lock` changes.)

---

### Task 4: ResolutionError + ResolutionRejection types

**Spec section:** §3 (Resolver) + §6 (Guided error messages).

**Files (create / modify):**
- Create: `crates/tau-runtime/src/sandbox/resolution_error.rs` — error taxonomy.
- Modify: `crates/tau-runtime/src/sandbox/mod.rs` — re-export.

**Summary:**
Define `ResolutionError` with three variants (`NoAdapterMatches { tried, platform, required_tier }`, `PluginTierMismatch { plugin, required, delivered }`, `ConfigError { message }`) plus `ResolutionRejection` enum (`PlatformMismatch`, `ProbeUnavailable(String)`, `TierTooLow { delivered, required }`, `ShapesUnsupported { missing: CapabilityShapeSet }`, `PluginTierTooLow { plugin, required }`). All `#[non_exhaustive]`. Implement `Display` + `Error`. Include 5 unit tests covering: error rendering, variant construction, multi-tried list rendering, tier mismatch rendering, plugin mismatch rendering.

**Verification:** standard 5-gate workspace verification. Build still red on chain.rs until Task 5; verify only chain.rs errors carry over.

**Commit:** `feat(runtime): ResolutionError taxonomy for sandbox adapter resolution`. Stage only the new file + mod.rs.

---

### Task 5: resolver (`resolve_adapter`) + retire `chain.rs`

**Spec section:** §3 (Resolver) + plan-erratum "Existing chain code → new model migration".

**Files (create / modify / delete):**
- Create: `crates/tau-runtime/src/sandbox/resolver.rs` — the new `resolve_adapter` function + `SandboxAdapter` enum (relocated from chain.rs unchanged).
- Modify: `crates/tau-runtime/src/sandbox/mod.rs` — replace `pub mod chain;` with `pub mod resolver;`; re-export `resolve_adapter`, `SandboxAdapter`, `SandboxRequirementsResolved`.
- Delete: `crates/tau-runtime/src/sandbox/chain.rs`.

**Summary:**
The resolver implements the Bazel-style filter pipeline from spec §3:

1. `detect_platform()` → string (from registry).
2. For each `AdapterRegistration` in `REGISTRY`:
   - Filter by platform match (push `ResolutionRejection::PlatformMismatch` on miss).
   - Instantiate the adapter (via a per-kind `instantiate()` helper that mirrors the old chain.rs `instantiate()`; supports `RegistryKind::{Native, Container, Remote, Passthrough}` and consults the `TAU_TESTING_ALLOW_MOCK_SANDBOX` env var to inject Mock when set).
   - Probe → `SandboxProbe::Available { tier, .. }` or `Unavailable { reason }`.
   - Filter by `delivered_tier >= effective_required_tier` (where `effective_required_tier = max(project.required_tier, plugins.iter().filter_map(|p| p.required_tier).max())`).
   - Filter by `required_shapes.is_subset_of(adapter.supported_shapes)`.
   - Filter by every plugin's `required_tier <= delivered_tier`.
3. Sort surviving candidates by `priority` (descending).
4. Return the highest-priority match. If none, return `ResolutionError::NoAdapterMatches { tried, platform, required_tier }`.

Move the existing `SandboxAdapter` enum from chain.rs into resolver.rs — same variants (`Native`, `Container`, `Mock`), same `impl Sandbox`, same inherent methods. Add a `Passthrough(PassthroughSandbox)` variant. Relocate `instantiate()`, `parse_tier_str()`, `parse_container_runtime()` helpers from chain.rs to resolver.rs.

Rewrite the 8 chain-tests against the resolver:

- `default_requirements_resolves_to_some_adapter` (Linux: native, macOS+Docker: container, etc.) — env-dependent but deterministic about "produces SOME Ok or NoAdapterMatches".
- `mock_explicit_via_env_var_resolves_to_mock` (set `TAU_TESTING_ALLOW_MOCK_SANDBOX=1`).
- `required_tier_strict_with_only_passthrough_unsatisfiable`.
- `required_tier_none_resolves_to_passthrough_when_no_other_match`.
- `plugin_tier_floor_strict_rejects_passthrough`.
- `unknown_platform_includes_all_in_tried_list`.
- `parse_tier_str_recognizes_known_values`.
- `parse_container_runtime_recognizes_known`.

After the file deletion + replacement: `cargo build --workspace` should compile clean (the chain.rs error from Task 1 disappears).

**Verification:** standard 5-gate workspace verification. Build now GREEN. No new errors.

**Commit:** `feat(runtime): resolver — declarative requirements + adapter registry filter pipeline`. Stage only the new file + mod.rs + the chain.rs deletion.

---

### Task 6: `PluginHostOptions.sandbox_adapter` + activation in `plugin_loader.rs`

**Spec section:** §1 (Project schema), §2 (Registry), §3 (Resolver), §7 (Data flow).

**Files (modify):**
- `crates/tau-runtime/src/plugin_host/mod.rs` — add `sandbox_adapter: Option<Arc<SandboxAdapter>>` field to `PluginHostOptions`. Modify the four `load_*` call sites in this file to internally zip `options.sandbox_adapter` with the per-call `Option<&SandboxPlan>` and pass `Option<(&SandboxPlan, &SandboxAdapter)>` to `spawn_and_handshake`.
- `crates/tau-cli/src/cmd/plugin_loader.rs` — read `scope.config.sandbox`, build `SandboxRequirements`, collect plugin manifests' `PluginSandboxRequirements`, call `resolve_adapter`, build `SandboxPlan` per plugin via the existing `tau_runtime::sandbox::build_plan`, set `host_options.sandbox_adapter = Some(Arc::new(adapter))`, pass `Some(&plan)` to each `load_*`.

**Summary:**
This is the load-bearing activation step. After Task 6:
- `tau chat my-agent` resolves the adapter and spawns plugins under it.
- `tau run my-agent ...` does the same.
- `tau plugin describe` continues to pass `None` for sandbox (per the existing TODO; describe is a meta-introspection path that doesn't run plugin code beyond the handshake).

Pseudocode shape for `load_plugins`:

```rust
let scope_config = scope.read_config()?;
let plugin_manifests: Vec<&PluginSandboxRequirements> =
    [llm_plugin, &tool_plugins[..]].iter().map(|p| p.manifest.sandbox()).collect();
let adapter = match resolve_adapter(&scope_config.sandbox, &plugin_manifests).await {
    Ok(a) => Arc::new(a),
    Err(e) => {
        // Render the guided error (Task 8); exit 2.
        return Err(crate::cmd::error_render::render_resolution_error(e).into());
    }
};
host_options.sandbox_adapter = Some(adapter);

// For each plugin:
let plan = tau_runtime::sandbox::build_plan(...)?;
plugin_host::load_llm_backend(plugin, config, ctx, host_options.clone(), Some(&plan)).await?;
```

Add 4 integration tests in a new `crates/tau-cli/tests/cmd_plugin_loader_sandbox.rs` (or extend an existing test file) that exercise: passthrough explicit-opt-in via env var, native-required-but-unavailable produces guided error, default scope config (no `[sandbox]` section) resolves to native on Linux / container on macOS / passthrough only when explicitly required `none`, plugin with `required_tier = "strict"` blocks passthrough resolution.

**Verification:** standard 5-gate workspace verification. Both `cargo test --workspace --all-targets` and `--doc` must pass. Verify `tau chat` smoke-test against an existing fixture project.

**Commit:** `feat(runtime,cli): activate sandbox at plugin spawn pipeline`. Stage `plugin_host/mod.rs`, `plugin_loader.rs`, the new integration tests file.

---

### Task 7: `--no-sandbox` and `--sandbox <kind>` CLI flags

**Spec section:** §5 (CLI surface).

**Files (modify):**
- `crates/tau-cli/src/cli.rs` — add global flags on `Cli`:
  - `pub no_sandbox: bool` (`#[arg(long, global = true)]`).
  - `pub sandbox: Option<SandboxKindArg>` (`#[arg(long, global = true, value_enum)]`).
  - Add `enum SandboxKindArg { Native, Container, Passthrough }` (clap `ValueEnum`).
- `crates/tau-cli/src/cmd/plugin_loader.rs` — when `--no-sandbox` OR `--sandbox passthrough` is set, force `required_tier = SandboxRequiredTier::None` AND bypass plugin-tier checks before calling `resolve_adapter`. When `--sandbox <kind>` is set, force the resolver to instantiate ONLY that kind, probe it, and accept iff `Available`; emit a clear "--sandbox native is not applicable on macOS" error otherwise.
- Tests: `crates/tau-cli/tests/cmd_no_sandbox_flag.rs` (new). 5 integration tests: `--no-sandbox` smokes, `--sandbox passthrough` equivalence, `--sandbox native` on macOS errors, `--sandbox container` overrides scope config, `--no-sandbox` bypasses plugin tier check.
- `crates/tau-cli/tests/snapshots/help_snapshots__cli_help.snap` — re-snapshot via `cargo insta accept` after the help text changes.

**Verification:** standard 5-gate. Snapshot regeneration documented in the commit body so the next implementer reviewing the diff understands.

**Commit:** `feat(cli): --no-sandbox and --sandbox <kind> global flags`.

---

### Task 8: guided error renderer for `ResolutionError`

**Spec section:** §6 (Guided error messages).

**Files (create / modify):**
- Create: `crates/tau-cli/src/cmd/error_render.rs` — `render_resolution_error(err: ResolutionError) -> String` and `render_plugin_tier_mismatch(err: PluginTierMismatch) -> String`. Plain string output (no clap or color libs at v0.2). Multi-line with the structure from spec §6: required, detected platform, per-adapter status, options to proceed.
- Modify: `crates/tau-cli/src/cmd/mod.rs` — add `pub mod error_render;`.
- Test: 4 snapshot tests in `crates/tau-cli/src/cmd/error_render.rs` using `insta::assert_snapshot!` (already a dev-dep). Cover: native-only-required-but-mac, container-needed-no-docker, plugin-strict-passthrough-resolved, all-three-stacked.

**Verification:** standard 5-gate. The new snapshot files at `crates/tau-cli/src/cmd/snapshots/error_render__*.snap` land with this commit; `cargo insta accept` after authoring.

**Commit:** `feat(cli): guided multi-option error renderer for sandbox resolution`.

---

### Task 9: `tau sandbox status` subcommand

**Spec section:** §5 (CLI surface — `tau sandbox status`).

**Files (create / modify):**
- Modify: `crates/tau-cli/src/cli.rs` — add `Sandbox(SandboxArgs)` to `enum Command` and define `enum SandboxArgs { Status, Setup(SandboxSetupArgs) }`.
- Create: `crates/tau-cli/src/cmd/sandbox.rs` — `pub async fn run(args: &SandboxArgs, output: &mut Output) -> anyhow::Result<()>` dispatching to `run_status` or `run_setup`. `run_status` reads scope config, calls `resolve_adapter` in dry-run mode (returns `Result` but does not error out — surfaces both Ok and Err in the report), formats the multi-section status report from spec §5. `tau sandbox status` always exits 0; configuration errors are RENDERED in the report rather than turned into exit codes.
- Modify: `crates/tau-cli/src/cmd/mod.rs` — add `pub mod sandbox;` + dispatch arm.
- Test: `crates/tau-cli/tests/cmd_sandbox_status.rs` (new). 4 tests: status against default scope, status with explicit `[sandbox]` block, status when no adapter matches (still exits 0; just shows the failure), status with plugin-tier mismatch.

**Verification:** standard 5-gate.

**Commit:** `feat(cli): tau sandbox status diagnostic subcommand`.

---

### Task 10: `tau sandbox setup` subcommand (interactive + non-interactive)

**Spec section:** §5 (CLI surface — `tau sandbox setup`).

**Files (modify / create):**
- Modify: `crates/tau-cli/src/cli.rs` — define `struct SandboxSetupArgs { #[arg(long)] tier: Option<SandboxKindArg /* tier-only variant */>, #[arg(long)] non_interactive: bool }`.
- Modify: `crates/tau-cli/src/cmd/sandbox.rs` — `run_setup` implementation. Interactive mode reads stdin via `dialoguer` IF that's already a workspace dep, ELSE via raw `std::io::stdin().read_line` (plain prompts, simple). Non-interactive mode skips prompts and writes the tier directly. Both modes update `<scope>/config.toml` via `ScopeConfig::read_from_str` → mutate `sandbox.required_tier` → `to_toml_string` → atomic write (via existing `tau-pkg::scope` write helpers if any; else `tempfile::NamedTempFile::persist`).
- Test: `crates/tau-cli/tests/cmd_sandbox_setup.rs` (new). 5 tests: non-interactive `--tier strict` writes config, non-interactive `--tier none` writes config, non-interactive twice idempotent, interactive mode skipped (require explicit non-interactive in tests; interactive is unstested here per "tests must not block on stdin"), invalid tier rejected.

Avoid adding a new dep if possible — the prompt UX can be plain `eprint!` + `read_line` for v0.2.

**Verification:** standard 5-gate.

**Commit:** `feat(cli): tau sandbox setup interactive + non-interactive`.

---

### Task 11: `tau resolve --check-sandbox` extension

**Spec section:** §3 (Resolver) + §5 (CLI) + spec's "passthrough skip-to-next-strict" behavior.

**Files (modify):**
- `crates/tau-cli/src/cmd/resolve.rs` — extend `run_check_sandbox` to also surface plugin-tier mismatches. When the resolved adapter is `Passthrough` (because `required_tier = None`), the `--check-sandbox` command MUST NOT validate against passthrough (every plan trivially passes); instead, walk the registry and pick the highest-priority NON-passthrough adapter; validate against that. If no non-passthrough adapter is available, error: `"no non-permissive adapter available to validate against; cannot perform sandbox check"` exit 2.
- `crates/tau-cli/tests/cmd_resolve_check_sandbox.rs` — rewrite the existing `[sandbox] chain = [...]` fixtures to `[sandbox] required_tier = "..."`. Add 3 new tests: `check_sandbox_skips_passthrough_to_native`, `check_sandbox_errors_when_only_passthrough_in_chain`, `check_sandbox_surfaces_plugin_tier_mismatch`.

**Verification:** standard 5-gate. Re-snapshot `help_snapshots__resolve_help.snap` if the help text changed (likely yes since the command is mentioned).

**Commit:** `feat(cli): tau resolve --check-sandbox surfaces plugin-tier mismatches`.

---

### Task 12: PAUSE — final local verification + open PR (USER GATE)

The implementer agent runs the full local verification suite, confirms everything is green, opens a draft PR, but does NOT merge.

- [ ] **Step 1: Run the complete local verification suite.** All must pass:
  - `cargo build --workspace`
  - `cargo test --workspace --all-targets`
  - `cargo test --doc`
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`

- [ ] **Step 2: Push the branch.** `git push -u origin feat/sandbox-activation-spec`

- [ ] **Step 3: Open a draft PR via gh.** Use `gh pr create --draft --base main --head feat/sandbox-activation-spec --title "feat: sandbox activation — declarative requirements + adapter registry + guided setup"` and pass the body via `--body-file` pointing at a tempfile (avoid heredocs that previously triggered security hooks). Body sections: Summary, Architecture, Out-of-scope (cross-reference followups doc + Phase 2 sub-projects), Test plan, Linked spec.

- [ ] **Step 4: Wait for CI green.** Monitor with `gh pr checks <pr#>` until 25 required checks pass. **Do NOT merge.** Pause for the user.

---

### Task 13: PAUSE — ADR-0015 + ROADMAP + squash merge (USER GATE)

After CI green and user approval, write the documentation deliverables, push the docs commit, mark PR ready, squash-merge.

- [ ] **Step 1: Create `docs/decisions/0015-sandbox-activation.md`.** Body covers the 6 D-decisions from the spec (Default activation; Adapter on PluginHostOptions; Hard refuse with two-granularity opt-out; Passthrough adapter; Replace chain with declarative requirements + registry + resolver; Plugin-side tier declarations). Each decision section: Context → Decision → Consequences → Alternatives considered. Add a Vision section pointing back at `docs/explanation/tau-as-language.md` and reaffirming the Phase 2 sub-projects A-G.

- [ ] **Step 2: Update `ROADMAP.md`.** Mark sub-project A done in the Phase 1 priority 12 row's followups link. Reference ADR-0015. The ROADMAP's existing Phase 2 stub (sub-projects A-G from the priority-12 squash) stays; this sub-project is part of priority 12's followup work, not Phase 2.

- [ ] **Step 3: Update the followups doc** `docs/superpowers/specs/2026-05-03-sandboxing-followups.md`. Strike sub-project A's row from "outstanding"; add a brief note that ADR-0015 supersedes the chain-based design proposed there.

- [ ] **Step 4: Commit + push docs.** `git add docs/decisions/0015-sandbox-activation.md ROADMAP.md docs/superpowers/specs/2026-05-03-sandboxing-followups.md && git commit -m "docs(adr): ADR-0015 sandbox activation — declarative requirements + registry + guided setup" && git push`.

- [ ] **Step 5: Wait for CI green.** `gh pr checks <pr#>` until 25/25.

- [ ] **Step 6: Mark PR ready.** `gh pr ready <pr#>`.

- [ ] **Step 7: Squash-merge.** After user approval: `gh pr merge <pr#> --squash --delete-branch`.

- [ ] **Step 8: Post-merge verification.** `git checkout main && git pull && cargo build --workspace && cargo test --workspace --all-targets`.

---

## Self-review

**Spec coverage:**
- §1 (Project requirements / scope schema v3) → Task 1.
- §2 (Adapter registry) → Task 3.
- §3 (Resolver) → Tasks 4 + 5.
- §4 (Plugin manifest schema) → Task 2.
- §5 (CLI surface — `--no-sandbox`, `--sandbox <kind>`, `tau sandbox status`, `tau sandbox setup`) → Tasks 7 + 9 + 10.
- §6 (Guided error messages) → Task 8.
- §7 (Data flow) → Task 6 (the activation site that wires the data flow).
- §8 (Telemetry) → folded into Task 5's resolver implementation (tracing::info on success, warn on passthrough selection).
- D1 (ON by default + `--no-sandbox`) → Tasks 6 + 7.
- D2 (PluginHostOptions field) → Task 6.
- D3 (Hard refuse + two-granularity opt-out) → Tasks 6 + 7 + 8.
- D4 (Passthrough adapter) → Task 3.
- D5 (Declarative requirements + registry + resolver) → Tasks 1 + 3 + 4 + 5.
- D6 (Plugin-side tier declarations) → Tasks 2 + 5 (resolver consumes them).

**Placeholder scan:** searched for "TBD", "TODO", "fill in", "implement later", "similar to" — none found. Tasks 4, 6-13 are intentionally summary-format per the writing-plans skill arguments; each carries a clear file list, summary, verification, and commit message.

**Type consistency:** `SandboxRequirements` (Task 1), `SandboxRequiredTier` (Task 1), `PluginSandboxRequirements` (Task 2), `PluginRequiredTier` (Task 2), `RegistryKind` (Task 3), `PlatformSet` (Task 3), `AdapterRegistration` (Task 3), `PassthroughSandbox` (Task 3), `ResolutionError` (Task 4), `ResolutionRejection` (Task 4), `SandboxAdapter` (Task 5; relocated from chain.rs), `resolve_adapter` (Task 5), `PluginHostOptions.sandbox_adapter` (Task 6), `--no-sandbox` / `--sandbox <kind>` (Task 7), `render_resolution_error` (Task 8), `tau sandbox status` (Task 9), `tau sandbox setup` (Task 10), `--check-sandbox` extension (Task 11). Names match across tasks.

**Plumbing carryovers:** schema v2 → v3 with auto-migrate warn (Task 1); `#[non_exhaustive]` on every public type; `tracing` warn discipline; `cargo test --workspace --all-targets` (NOT `--lib`) at every gate; verify-against-base-SHA before claiming "pre-existing"; CI matrix unchanged at 25 checks; `TAU_TESTING_ALLOW_MOCK_SANDBOX=1` env var preserved for CLI integration tests; `build_plan` + `validate_plan_against_adapter` (priority 12) reused as-is; `wrap_spawn` integration in plugin_host (priority 12 task 9) unchanged; `Mock` stays in `tau-ports/fixtures.rs`.
