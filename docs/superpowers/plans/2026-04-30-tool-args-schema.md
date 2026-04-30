# Tool-Args Schema Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Validate every tool-call's arguments against the tool's declared `ToolSpec.input_schema` at the kernel boundary, lighting up `BuildError::ToolSchemaInvalid` (registration-time) and `ToolError::BadArgs` (invoke-time) per ADR-0006 §3 deferral closure.

**Architecture:** New `tau-runtime::tool_args` module owns the validator type. `RuntimeBuilder::build()` compiles each registered tool's schema once via the `jsonschema` crate; failure → `BuildError::ToolSchemaInvalid`. The existing `deserialize_tool_args` passthrough at `run.rs:417` is replaced with `validate_tool_args`; on failure, the call site writes a `MessagePayload::ToolError` directly and `continue`s the inner `for tool_use` loop so the agent loop survives and the LLM self-corrects on the next turn. Every BadArgs error message MUST contain the original args, the full declared schema, and the specific issue (the MANDATORY rule from spec §4.6).

**Tech Stack:** Rust 2021, `jsonschema = "0.46"` (new workspace dep), `serde_json` (already a dep), thiserror.

---

## Plan-erratum (carryover constraints)

Apply preemptively. Do NOT re-derive.

- **`BuildError` is `#[non_exhaustive]`.** Add `ToolSchemaInvalid { tool_name: String, detail: String }` variant (additive non-breaking).

- **`RuntimeError::PluginContractViolation` stays RESERVED** for a future runtime path (out-of-process plugin lying in handshake). v0.1 of this sub-project does NOT light it up — the trigger paths are `BuildError::ToolSchemaInvalid` (build) + `ToolError::BadArgs` (invoke).

- **`ToolError::BadArgs { reason }` is reused** (no schema change). The new `reason` content is constrained by the MANDATORY template — every error from validation MUST contain `"You sent:"`, `"Expected (input_schema):"`, and `"Specific issue"` literal substrings.

- **`Tool` trait is UNCHANGED.** Plugins continue declaring `input_schema` via `Tool::schema()` exactly as today.

- **`RuntimeBuilder.tools` storage shape changes:** `Vec<Arc<dyn DynTool>>` → still `Vec<Arc<dyn DynTool>>` (the builder accumulator), but `Runtime.tools` becomes `HashMap<String, RegisteredTool>` where `RegisteredTool { tool: Arc<dyn DynTool>, validator: ToolArgsValidator }`. The `collect_tools_by_name` helper at `builder.rs:467-486` is the place that compiles schemas and packages each tool with its validator.

- **`Runtime::tools()` accessor** (kernel-internal `pub(crate)` at `builder.rs:310-312`) signature stays returning `&HashMap<String, Arc<dyn DynTool>>` for the existing call sites in `run.rs` (where it's used for tool resolution by name). To preserve this without breaking call sites, we either: (a) keep `Runtime.tools` as `HashMap<String, Arc<dyn DynTool>>` and store validators in a parallel `HashMap<String, ToolArgsValidator>` field; OR (b) change `Runtime.tools` to `HashMap<String, RegisteredTool>` and add a separate `pub(crate) fn validators()` accessor while changing `tools()` to a derived view. Option **(a) is simpler** — two parallel maps avoid disturbing existing consumers. Implementer picks (a).

- **`ToolArgsValidator` is `pub(crate)`** (kernel-internal); has `compiled: Option<jsonschema::JSONSchema>` (None = opt-out for empty/null schema) and `declared_schema_json: String` (kept for the MANDATORY-rule template).

- **Empty schemas opt out cleanly:** `compile(Value::Null)` → `Ok(Self { compiled: None, .. })`; `compile(Value::Object(empty))` → same. Validation on opt-out is a no-op.

- **Existing `Err(invoke_err)` arm at `run.rs:436-447`** stays unchanged — it `return`s `Err(RuntimeError::from(err))` on real plugin invocation crashes. Validation failure is a SEPARATE short-circuit path that skips `Tool::invoke` entirely, writes `MessagePayload::ToolError` directly to the conversation, and `continue`s the inner loop. Do NOT route validation through the existing arm; do NOT propagate via `?`.

- **`tau_domain::Value` ↔ `serde_json::Value` conversion:** they share shape but are distinct types. `tau_domain::Value` has a `serde` feature already enabled in tau-runtime's Cargo.toml. Convert via `serde_json::to_value(&tau_domain_value)` for the validator. The schema itself is also `tau_domain::Value`; convert the same way.

- **`jsonschema` API:** `JSONSchema::options().with_draft(Draft::Draft7).compile(&serde_json::Value)` returns `Result<JSONSchema, ValidationError>`. To validate, call `JSONSchema::validate(&value)` returning `Result<(), impl Iterator<Item = ValidationError>>`. The error type carries `instance_path` (jsonpath-style location) and Display-able description.

- **Doctests on `#[non_exhaustive]` types must be `ignore`-marked.** `cargo test --all-targets` does NOT include doctests; verify with `cargo test --doc` separately.

- **NO new CI jobs.** No new workspace member; no new external service in CI. Branch protection stays at 23 required checks.

---

## File structure

| Path | Status | Purpose |
|------|--------|---------|
| `Cargo.toml` (root) | Modify | Add `jsonschema = "0.46"` to `[workspace.dependencies]` |
| `crates/tau-runtime/Cargo.toml` | Modify | Add `jsonschema = { workspace = true }` |
| `crates/tau-runtime/src/tool_args.rs` | Create | `ToolArgsValidator`, `SchemaCompileError`, `validate_tool_args`. ~150 LOC + ~12 unit tests |
| `crates/tau-runtime/src/lib.rs` | Modify | Declare `pub(crate) mod tool_args;` |
| `crates/tau-runtime/src/builder.rs` | Modify | Add validator compilation in `collect_tools_by_name`; add `Runtime.tool_validators` parallel field; `Runtime` struct gains accessor |
| `crates/tau-runtime/src/error.rs` | Modify | Add `BuildError::ToolSchemaInvalid { tool_name, detail }` variant + display test |
| `crates/tau-runtime/src/run.rs` | Modify | Replace `deserialize_tool_args` call with `validate_tool_args`; add short-circuit path on validation failure (write `MessagePayload::ToolError` + `continue`) |
| `crates/tau-runtime/tests/tool_args_validation_e2e.rs` | Create | 3 e2e scenarios (gated `#![cfg(unix)]`) |
| `docs/decisions/0010-tool-args-schema-validation.md` | Create (Task 7) | New ADR locking the 5 design decisions |
| `ROADMAP.md` | Modify (Task 7) | Mark Tier 2 priority 6 ✅ Shipped |

---

## Task 1: jsonschema workspace dep + tau-runtime integration

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/Cargo.toml`
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/Cargo.toml`

### Steps

- [ ] **Step 1.1: Verify the latest jsonschema version**

```bash
cargo search jsonschema | head -3
```
Expected output includes a line like `jsonschema = "0.46.3"` (current latest at branch creation; pin to the major `"0.46"` for forward-compat within the major).

- [ ] **Step 1.2: Add jsonschema to workspace deps**

Edit `/Users/titouanlebocq/code/tau/Cargo.toml`. Find `[workspace.dependencies]` block (around lines 33-63 — the existing list with `globset = "0.4"` and `semver = "1"`). Add immediately after `globset`:

```toml
jsonschema      = "0.46"
```

Pin to the major to allow patch upgrades. Place alphabetically near other lower-case crate deps.

- [ ] **Step 1.3: Add the dep to tau-runtime**

Edit `/Users/titouanlebocq/code/tau/crates/tau-runtime/Cargo.toml`. Find `[dependencies]` (after the existing `globset` line added in priority 4). Add:

```toml
# JSON Schema validation for tool args at the kernel boundary.
# Realizes ADR-0010 (Tier 2 priority 6).
jsonschema          = { workspace = true }
```

- [ ] **Step 1.4: Verify the dep tree compiles**

```bash
cargo build --workspace
```
Expected: PASS. The dep is present but not yet consumed — tau-runtime's lib.rs doesn't `use jsonschema` yet. Just confirms the version resolves and downloads.

If `cargo build` reports a feature-gate error, drop the version pin to a compatible tier (e.g., `"0.45"`); the implementer should pin to whatever `cargo search` reported.

- [ ] **Step 1.5: Verify tau-domain serde feature is enabled in tau-runtime**

```bash
grep "tau-domain" /Users/titouanlebocq/code/tau/crates/tau-runtime/Cargo.toml
```
Expected: `tau-domain          = { workspace = true, features = ["serde"] }`. If the feature is missing, add `"serde"` to its features array — Task 2's validator depends on `tau_domain::Value: Serialize`.

- [ ] **Step 1.6: Run full verification**

```bash
cargo build --workspace
cargo test -p tau-runtime --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-runtime --doc
```
Expected: all PASS. The dep is dormant (not yet used) so behavior is unchanged.

- [ ] **Step 1.7: Commit**

```bash
git add Cargo.toml crates/tau-runtime/Cargo.toml
git commit -m "$(cat <<'EOF'
build(runtime): add jsonschema 0.46 workspace dep

Foundation for tool-args schema validation (Tier 2 priority 6).
The crate is added to [workspace.dependencies] and pulled into
tau-runtime; not yet consumed at this commit (Task 2 wires it up).

Pinned to "0.46" major to allow patch upgrades within the major.

Refs: docs/superpowers/specs/2026-04-30-tool-args-schema-design.md §4

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 1.8: Push**

```bash
git push
```

---

## Task 2: `tau-runtime::tool_args` module — validator + MANDATORY-rule template

**Files:**
- Create: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/tool_args.rs`
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/lib.rs` — declare `pub(crate) mod tool_args;`

### Steps

- [ ] **Step 2.1: Declare the module in lib.rs**

In `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/lib.rs`, find the existing module declarations (search for `pub(crate) mod capability;` or similar). Add immediately after them:

```rust
pub(crate) mod tool_args;
```

Alphabetically near `capability_override` if convenient.

- [ ] **Step 2.2: Create tool_args.rs with full content**

Create `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/tool_args.rs` with this EXACT content:

```rust
//! Tool-args schema validation at the kernel boundary.
//!
//! Validates every tool-call's arguments against the tool's declared
//! `ToolSpec.input_schema` (JSON Schema Draft 7) before invoking
//! `Tool::invoke`. Validators are pre-compiled once at
//! `RuntimeBuilder::build()` so per-invoke cost stays in the tens of
//! microseconds.
//!
//! Two failure paths:
//!   - Schema malformed at registration → `BuildError::ToolSchemaInvalid`
//!     (returned by `compile`; surfaced by the builder).
//!   - Args mismatch at invoke → `ToolError::BadArgs` carrying a
//!     self-instructive reason that includes the full declared
//!     `input_schema` plus the original args plus the specific issue.
//!
//! See `docs/superpowers/specs/2026-04-30-tool-args-schema-design.md`
//! and ADR-0010.

use jsonschema::{Draft, JSONSchema};
use tau_domain::Value;
use tau_ports::ToolError;

/// Compiled validator for a single tool's `input_schema`.
///
/// Built once at `RuntimeBuilder::build()` per registered tool.
/// `compile()` rejects malformed schemas; `validate()` rejects
/// non-conforming runtime args with a self-instructive error string.
#[non_exhaustive]
pub(crate) struct ToolArgsValidator {
    /// Compiled jsonschema instance. `None` = tool opted out (empty
    /// schema or `Value::Null`); validation is a no-op.
    compiled: Option<JSONSchema>,
    /// The original input_schema as declared, kept for inclusion in
    /// BadArgs error messages (the MANDATORY rule).
    declared_schema_json: String,
}

impl ToolArgsValidator {
    /// Compile from a tool's declared `input_schema`. Returns
    /// `Err(SchemaCompileError)` if the schema is not a valid Draft 7
    /// schema.
    ///
    /// Empty (`Value::Null` or empty `Value::Object`) opts out — the
    /// returned validator passes everything.
    pub(crate) fn compile(input_schema: &Value) -> Result<Self, SchemaCompileError> {
        let schema_json = serde_json::to_value(input_schema).map_err(|e| {
            SchemaCompileError {
                kind: format!("internal: schema serialization failed: {e}"),
                schema_excerpt: String::new(),
            }
        })?;
        let declared_schema_json = serde_json::to_string(&schema_json).unwrap_or_default();

        // Opt-out: null or empty object schema accepts everything.
        let is_opt_out = match &schema_json {
            serde_json::Value::Null => true,
            serde_json::Value::Object(map) => map.is_empty(),
            _ => false,
        };
        if is_opt_out {
            return Ok(Self {
                compiled: None,
                declared_schema_json,
            });
        }

        let compiled = JSONSchema::options()
            .with_draft(Draft::Draft7)
            .compile(&schema_json)
            .map_err(|err| SchemaCompileError {
                kind: format!("schema compile failed: {err}"),
                schema_excerpt: declared_schema_json.chars().take(200).collect(),
            })?;
        Ok(Self {
            compiled: Some(compiled),
            declared_schema_json,
        })
    }

    /// Validate runtime args against the compiled schema. Returns
    /// `Err(reason)` on failure where `reason` follows the MANDATORY
    /// template from ADR-0010 §4: every error contains `"You sent:"`,
    /// `"Expected (input_schema):"`, and `"Specific issue"` substrings.
    ///
    /// On opt-out (empty/null schema), validation is a no-op and
    /// returns `Ok(())`.
    pub(crate) fn validate(&self, args: &Value, tool_name: &str) -> Result<(), String> {
        let Some(compiled) = &self.compiled else {
            return Ok(());
        };
        let args_json = serde_json::to_value(args).map_err(|e| {
            format!("internal: args serialization failed: {e}")
        })?;
        if let Err(errors) = compiled.validate(&args_json) {
            let issues: Vec<String> = errors
                .map(|e| format!("  {}: {}", e.instance_path, e))
                .collect();
            let args_repr = serde_json::to_string(&args_json)
                .unwrap_or_else(|_| "<unprintable>".into());
            return Err(format!(
                "{tool_name}: args validation failed.\n\n\
                 You sent: {args_repr}\n\n\
                 Expected (input_schema):\n{schema}\n\n\
                 Specific issue(s):\n{issues}",
                schema = self.declared_schema_json,
                issues = issues.join("\n"),
            ));
        }
        Ok(())
    }
}

/// Error returned when a tool's `input_schema` fails to compile as a
/// valid Draft 7 schema. Surfaced by `RuntimeBuilder::build()` as
/// `BuildError::ToolSchemaInvalid`.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub(crate) struct SchemaCompileError {
    /// jsonschema's diagnostic.
    pub kind: String,
    /// First 200 chars of the schema for context.
    pub schema_excerpt: String,
}

/// Validate runtime args via the kernel's pre-compiled validator.
/// Replaces the v0.1 `deserialize_tool_args` passthrough at `run.rs:417`.
///
/// Returns `Err(ToolError::BadArgs { reason })` on validation failure.
/// The caller in `run.rs` is responsible for routing this through the
/// validation-failure short-circuit (write MessagePayload::ToolError
/// to the conversation; `continue` the inner loop) — DO NOT propagate
/// via `?`, which would terminate the run.
#[allow(dead_code)] // wired up by Task 4 (run.rs call-site integration)
pub(crate) fn validate_tool_args<'a>(
    value: &'a Value,
    tool_name: &str,
    validator: &ToolArgsValidator,
) -> Result<&'a Value, ToolError> {
    match validator.validate(value, tool_name) {
        Ok(()) => Ok(value),
        Err(reason) => Err(ToolError::BadArgs { reason }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Build a tau_domain::Value JSON Schema from a `serde_json::json!` literal.
    fn schema(json: serde_json::Value) -> Value {
        let s = serde_json::to_string(&json).expect("schema serializes");
        serde_json::from_str(&s).expect("schema round-trips through tau_domain::Value")
    }

    /// Build a tau_domain::Value args object from a `serde_json::json!` literal.
    fn args(json: serde_json::Value) -> Value {
        let s = serde_json::to_string(&json).expect("args serialize");
        serde_json::from_str(&s).expect("args round-trip through tau_domain::Value")
    }

    // -------- compile --------

    #[test]
    fn compile_happy_path_returns_some_compiled() {
        let s = schema(serde_json::json!({
            "type": "object",
            "properties": { "x": { "type": "string" } },
            "required": ["x"]
        }));
        let v = ToolArgsValidator::compile(&s).expect("valid schema");
        assert!(v.compiled.is_some());
    }

    #[test]
    fn compile_null_schema_opts_out() {
        let v = ToolArgsValidator::compile(&Value::Null).expect("null opts out");
        assert!(v.compiled.is_none());
    }

    #[test]
    fn compile_empty_object_schema_opts_out() {
        let s = Value::Object(BTreeMap::new());
        let v = ToolArgsValidator::compile(&s).expect("empty opts out");
        assert!(v.compiled.is_none());
    }

    #[test]
    fn compile_malformed_schema_returns_error() {
        // "type": "objectt" is a typo — not a valid Draft 7 type.
        let s = schema(serde_json::json!({ "type": "objectt" }));
        let err = ToolArgsValidator::compile(&s).expect_err("malformed");
        assert!(
            err.kind.contains("compile"),
            "expected compile-failure kind; got: {}",
            err.kind
        );
        assert!(
            !err.schema_excerpt.is_empty(),
            "schema_excerpt should be populated"
        );
    }

    // -------- validate happy path / opt-out --------

    #[test]
    fn validate_happy_path() {
        let s = schema(serde_json::json!({
            "type": "object",
            "properties": { "x": { "type": "string" } },
            "required": ["x"]
        }));
        let v = ToolArgsValidator::compile(&s).unwrap();
        let a = args(serde_json::json!({ "x": "hello" }));
        v.validate(&a, "test-tool").expect("matches");
    }

    #[test]
    fn validate_opt_out_passes_anything() {
        let v = ToolArgsValidator::compile(&Value::Null).unwrap();
        let a = args(serde_json::json!({ "literally": "anything" }));
        v.validate(&a, "test-tool").expect("opt-out passes");
    }

    // -------- validate failure cases (each asserts the MANDATORY template) --------

    fn assert_mandatory_template(reason: &str, tool_name: &str) {
        assert!(
            reason.contains(tool_name),
            "reason should contain tool name; got: {reason}"
        );
        assert!(
            reason.contains("You sent:"),
            "reason MUST contain 'You sent:'; got: {reason}"
        );
        assert!(
            reason.contains("Expected (input_schema):"),
            "reason MUST contain 'Expected (input_schema):'; got: {reason}"
        );
        assert!(
            reason.contains("Specific issue"),
            "reason MUST contain 'Specific issue'; got: {reason}"
        );
    }

    #[test]
    fn validate_missing_required_field_includes_template() {
        let s = schema(serde_json::json!({
            "type": "object",
            "properties": { "x": { "type": "string" } },
            "required": ["x"]
        }));
        let v = ToolArgsValidator::compile(&s).unwrap();
        let a = args(serde_json::json!({})); // missing "x"
        let reason = v.validate(&a, "test-tool").expect_err("missing required");
        assert_mandatory_template(&reason, "test-tool");
    }

    #[test]
    fn validate_type_mismatch_includes_template() {
        let s = schema(serde_json::json!({
            "type": "object",
            "properties": { "x": { "type": "string" } },
            "required": ["x"]
        }));
        let v = ToolArgsValidator::compile(&s).unwrap();
        let a = args(serde_json::json!({ "x": 42 })); // integer, not string
        let reason = v.validate(&a, "test-tool").expect_err("type mismatch");
        assert_mandatory_template(&reason, "test-tool");
    }

    #[test]
    fn validate_integer_out_of_range_includes_template() {
        let s = schema(serde_json::json!({
            "type": "object",
            "properties": { "n": { "type": "integer", "minimum": 1, "maximum": 10 } },
            "required": ["n"]
        }));
        let v = ToolArgsValidator::compile(&s).unwrap();
        let a = args(serde_json::json!({ "n": 999 }));
        let reason = v.validate(&a, "test-tool").expect_err("out of range");
        assert_mandatory_template(&reason, "test-tool");
    }

    // -------- validate_tool_args wrapper --------

    #[test]
    fn validate_tool_args_returns_bad_args_on_failure() {
        let s = schema(serde_json::json!({
            "type": "object",
            "properties": { "x": { "type": "string" } },
            "required": ["x"]
        }));
        let v = ToolArgsValidator::compile(&s).unwrap();
        let a = args(serde_json::json!({}));
        let err = validate_tool_args(&a, "test-tool", &v).expect_err("missing required");
        let ToolError::BadArgs { reason } = err else {
            panic!("expected ToolError::BadArgs, got: {err:?}");
        };
        assert_mandatory_template(&reason, "test-tool");
    }

    #[test]
    fn validate_tool_args_returns_value_on_success() {
        let s = schema(serde_json::json!({
            "type": "object",
            "properties": { "x": { "type": "string" } }
        }));
        let v = ToolArgsValidator::compile(&s).unwrap();
        let a = args(serde_json::json!({ "x": "hello" }));
        let got = validate_tool_args(&a, "test-tool", &v).expect("happy path");
        assert_eq!(got, &a);
    }
}
```

- [ ] **Step 2.3: Verify**

```bash
cargo build --workspace
cargo test -p tau-runtime --all-targets tool_args
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-runtime --doc
```

Expected: build PASS; 12 unit tests PASS; fmt/clippy/doctest clean.

If clippy flags `#[allow(dead_code)]` on `validate_tool_args` — the comment justifies it (Task 4 wires it up). Leave it.

- [ ] **Step 2.4: Commit**

```bash
git add crates/tau-runtime/src/tool_args.rs crates/tau-runtime/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(runtime): add tool_args module — schema validator + MANDATORY-rule template

ToolArgsValidator pre-compiles a tool's declared input_schema (Draft 7,
via jsonschema crate) and validates runtime args. Empty/null schemas
opt out cleanly (compile returns None; validate is a no-op).

Every BadArgs reason from validation MUST contain the literal
substrings "You sent:", "Expected (input_schema):", and "Specific issue"
— the MANDATORY rule from spec §4.6 / ADR-0010 §4. Locked at the
validator boundary so callers can't short-circuit it.

12 unit tests covering compile happy path, null/empty opt-out,
malformed schema rejection, validate happy path, missing required,
type mismatch, integer out-of-range, and the validate_tool_args
wrapper. Each failure-case test explicitly asserts the MANDATORY
template via a shared helper.

Refs: docs/superpowers/specs/2026-04-30-tool-args-schema-design.md §4

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 2.5: Push**

```bash
git push
```

---

## Task 3: builder integration — compile schemas at `build()`

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/error.rs` — add `BuildError::ToolSchemaInvalid` variant + display test.
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/builder.rs` — `Runtime` struct gains a parallel `tool_validators` field; `collect_tools_by_name` compiles each schema; `Runtime::tool_validators()` accessor added; `build()` pipeline updated.

### Steps

- [ ] **Step 3.1: Add `BuildError::ToolSchemaInvalid` variant**

In `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/error.rs`, find the `BuildError` enum (around line 62-85). Add this variant BEFORE the `Internal` arm (which should remain the catch-all last variant):

```rust
    /// A registered tool's declared `input_schema` is not a valid
    /// Draft 7 JSON Schema. Surfaced at `RuntimeBuilder::build()`
    /// via `crate::tool_args::ToolArgsValidator::compile`. Realizes
    /// ADR-0010 (Tier 2 priority 6).
    #[error("tool {tool_name:?}: input_schema is not a valid Draft 7 schema: {detail}")]
    ToolSchemaInvalid {
        /// The tool name whose schema failed to compile.
        tool_name: String,
        /// Diagnostic from the schema compiler.
        detail: String,
    },
```

- [ ] **Step 3.2: Add the display test**

In `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/error.rs`, find the existing test module (search for `runtime_error_plugin_contract_violation_display`). Add a sibling test:

```rust
    #[test]
    fn build_error_tool_schema_invalid_display() {
        let err = BuildError::ToolSchemaInvalid {
            tool_name: "shell".into(),
            detail: "type 'objectt' is not valid".into(),
        };
        let s = format!("{err}");
        assert!(s.contains("shell"), "got: {s}");
        assert!(s.contains("input_schema"), "got: {s}");
        assert!(s.contains("Draft 7"), "got: {s}");
        assert!(s.contains("'objectt'"), "got: {s}");
    }
```

- [ ] **Step 3.3: Add `Runtime.tool_validators` parallel field**

In `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/builder.rs`, find the `Runtime` struct (around line 287-294):

```rust
#[non_exhaustive]
pub struct Runtime {
    llm_backends: HashMap<String, Arc<dyn DynLlmBackend>>,
    tools: HashMap<String, Arc<dyn DynTool>>,
    #[allow(dead_code)]
    storages: HashMap<String, Arc<dyn DynStorage>>,
}
```

Add a parallel validator field. Replace with:

```rust
#[non_exhaustive]
pub struct Runtime {
    llm_backends: HashMap<String, Arc<dyn DynLlmBackend>>,
    tools: HashMap<String, Arc<dyn DynTool>>,
    /// Pre-compiled input_schema validators, keyed by tool name. One
    /// entry per registered tool (in 1:1 correspondence with `tools`).
    /// Built once at `RuntimeBuilder::build()` per ADR-0010.
    tool_validators: HashMap<String, crate::tool_args::ToolArgsValidator>,
    #[allow(dead_code)]
    storages: HashMap<String, Arc<dyn DynStorage>>,
}
```

- [ ] **Step 3.4: Add the `Runtime::tool_validators()` accessor**

In the same file, find the `impl Runtime` block (around line 296-321). Add after `pub(crate) fn tools(&self)`:

```rust
    /// Read-only access to the per-tool input_schema validators. Used
    /// by the run loop's call-site integration in `run.rs` (replaces
    /// the v0.1 `deserialize_tool_args` passthrough). Realizes ADR-0010.
    pub(crate) fn tool_validators(&self) -> &HashMap<String, crate::tool_args::ToolArgsValidator> {
        &self.tool_validators
    }
```

- [ ] **Step 3.5: Update `collect_tools_by_name` to compile schemas**

In `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/builder.rs`, find `collect_tools_by_name` (around line 467-486). It currently returns `Result<HashMap<String, Arc<dyn DynTool>>, BuildError>`. We need a sibling that ALSO returns the validators. Replace `collect_tools_by_name` with a version that returns BOTH maps:

```rust
fn collect_tools_by_name(
    tools: Vec<Arc<dyn DynTool>>,
) -> Result<
    (
        HashMap<String, Arc<dyn DynTool>>,
        HashMap<String, crate::tool_args::ToolArgsValidator>,
    ),
    BuildError,
> {
    let mut tool_map: HashMap<String, Arc<dyn DynTool>> = HashMap::with_capacity(tools.len());
    let mut validator_map: HashMap<String, crate::tool_args::ToolArgsValidator> =
        HashMap::with_capacity(tools.len());
    for tool in tools {
        let name = tool.name().to_string();
        if tool_map.contains_key(&name) {
            return Err(BuildError::NameCollision {
                kind: PluginKind::Tool,
                name,
            });
        }
        // Compile the input_schema once at build time; failure surfaces
        // as BuildError::ToolSchemaInvalid before any LLM round-trip.
        let spec = tool.schema();
        let validator = crate::tool_args::ToolArgsValidator::compile(&spec.input_schema)
            .map_err(|e| BuildError::ToolSchemaInvalid {
                tool_name: name.clone(),
                detail: format!("{}; excerpt: {}", e.kind, e.schema_excerpt),
            })?;
        tool_map.insert(name.clone(), tool);
        validator_map.insert(name, validator);
    }
    Ok((tool_map, validator_map))
}
```

- [ ] **Step 3.6: Update `build()` to consume the new return shape**

In the same file, find `pub fn build(self)` (around line 431-443). Replace the body with the new collector return:

```rust
    pub fn build(self) -> Result<Runtime, BuildError> {
        if self.llm_backends.is_empty() {
            return Err(BuildError::NoLlmBackend);
        }
        let llm_backends = collect_llm_backends_by_name(self.llm_backends)?;
        let (tools, tool_validators) = collect_tools_by_name(self.tools)?;
        let storages = collect_storages_by_name(self.storages)?;
        Ok(Runtime {
            llm_backends,
            tools,
            tool_validators,
            storages,
        })
    }
```

- [ ] **Step 3.7: Add 3 unit tests**

In `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/builder.rs`, find the existing `#[cfg(test)] mod tests` block. Add these tests (inside the same module):

```rust
    /// A test-only DynTool whose schema we control — used to test
    /// build-time schema validation without touching the existing
    /// production plugins.
    struct TestSchemaTool {
        name: &'static str,
        schema: tau_domain::Value,
    }

    impl crate::builder::DynTool for TestSchemaTool {
        fn name(&self) -> &str {
            self.name
        }

        fn schema(&self) -> tau_ports::ToolSpec {
            tau_ports::fixtures::make_tool_spec(
                self.name.into(),
                "test".into(),
                self.schema.clone(),
            )
        }

        fn capabilities(&self) -> &[tau_domain::Capability] {
            &[]
        }

        fn init<'a>(
            &'a self,
            _ctx: tau_ports::SessionContext,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), tau_ports::ToolError>> + 'a>>
        {
            Box::pin(async { Ok(()) })
        }

        fn invoke<'a>(
            &'a self,
            _ctx: &'a tau_ports::SessionContext,
            _session: &'a mut (),
            _args: tau_domain::Value,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<tau_ports::ToolResult, tau_ports::ToolError>> + 'a>,
        > {
            Box::pin(async {
                Ok(tau_ports::fixtures::make_tool_result(vec![], false))
            })
        }

        fn teardown<'a>(
            &'a self,
            _session: (),
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), tau_ports::ToolError>> + 'a>>
        {
            Box::pin(async { Ok(()) })
        }
    }

    fn schema(json: serde_json::Value) -> tau_domain::Value {
        let s = serde_json::to_string(&json).expect("schema serializes");
        serde_json::from_str(&s).expect("schema round-trips")
    }

    fn mock_llm() -> tau_ports::fixtures::MockLlmBackend {
        tau_ports::fixtures::MockLlmBackend::new("mock-llm")
    }

    #[test]
    fn build_compiles_each_tools_input_schema() {
        let tool = TestSchemaTool {
            name: "echo",
            schema: schema(serde_json::json!({
                "type": "object",
                "properties": { "text": { "type": "string" } }
            })),
        };
        let runtime = Runtime::builder()
            .with_llm_backend(mock_llm())
            .with_dyn_tool(std::sync::Arc::new(tool))
            .build()
            .expect("build succeeds with valid schema");
        assert!(
            runtime.tool_validators().contains_key("echo"),
            "validator stored under tool name"
        );
    }

    #[test]
    fn build_rejects_tool_with_malformed_schema() {
        let tool = TestSchemaTool {
            name: "broken",
            schema: schema(serde_json::json!({ "type": "objectt" })), // typo
        };
        let err = Runtime::builder()
            .with_llm_backend(mock_llm())
            .with_dyn_tool(std::sync::Arc::new(tool))
            .build()
            .expect_err("build fails with malformed schema");
        let BuildError::ToolSchemaInvalid { tool_name, detail } = err else {
            panic!("expected BuildError::ToolSchemaInvalid, got: {err:?}");
        };
        assert_eq!(tool_name, "broken");
        assert!(detail.contains("compile"), "detail: {detail}");
    }

    #[test]
    fn build_handles_empty_schema_as_opt_out() {
        let tool = TestSchemaTool {
            name: "any-args",
            schema: schema(serde_json::json!({})),
        };
        let runtime = Runtime::builder()
            .with_llm_backend(mock_llm())
            .with_dyn_tool(std::sync::Arc::new(tool))
            .build()
            .expect("build succeeds with empty schema");
        assert!(
            runtime.tool_validators().contains_key("any-args"),
            "validator stored even on opt-out"
        );
    }
```

The `with_dyn_tool` method on `RuntimeBuilder` is the API for accepting `Arc<dyn DynTool>` directly (matching priority 3's e2e adapter pattern). If the test reports `with_dyn_tool` doesn't exist as a public method, look for the spelling that does (`with_dyn_tool` is the established convention from priority 3's `tool_plugin_e2e.rs`).

- [ ] **Step 3.8: Verify**

```bash
cargo build --workspace
cargo test -p tau-runtime --all-targets builder
cargo test -p tau-runtime --all-targets tool_args
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p tau-runtime --doc
```

Expected: build PASS; 3 new builder tests PASS; existing builder tests still pass; tool_args tests still pass.

If existing tests fail because their expected-shape `Runtime { llm_backends, tools, storages }` no longer matches (now has `tool_validators` field), update those tests' struct-literal patterns. Likely the existing builder tests use `Runtime::builder().build()` rather than struct-literal, so this should be a non-issue.

- [ ] **Step 3.9: Commit**

```bash
git add crates/tau-runtime/src/error.rs crates/tau-runtime/src/builder.rs
git commit -m "$(cat <<'EOF'
feat(runtime): compile tool input_schemas at RuntimeBuilder::build()

Builder now compiles each registered tool's ToolSpec.input_schema
once at build time via tool_args::ToolArgsValidator::compile.
Failure surfaces as BuildError::ToolSchemaInvalid (additive
non-breaking variant) — catches plugin-author typos before any LLM
round-trip.

Runtime gains a parallel `tool_validators: HashMap<String,
ToolArgsValidator>` field plus a pub(crate) accessor. The existing
`tools()` accessor signature is unchanged so the run loop's tool
resolution by name keeps working.

Empty schemas (Value::Null or Value::Object(empty)) opt out cleanly:
compile returns Some(validator) but with compiled = None, and
validate is a no-op for those tools.

3 builder unit tests + 1 error display test.

Refs: docs/superpowers/specs/2026-04-30-tool-args-schema-design.md §4.2, §4.3, §5.2

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 3.10: Push**

```bash
git push
```

---

## Task 4: `run.rs` call-site integration

**Hybrid format.**

**Files:**
- Modify: `/Users/titouanlebocq/code/tau/crates/tau-runtime/src/run.rs` — replace `deserialize_tool_args` call with `validate_tool_args`; add validation-failure short-circuit (writes `MessagePayload::ToolError` and `continue`s the inner `for tool_use` loop).

**Spec sections:** §4.4, §4.5.

**Per-task summary:**

1. **Locate** the existing call site at `crates/tau-runtime/src/run.rs:415-421`:
   ```rust
   // v0.1 passthrough; the helper is a hook for the
   // Phase-1 schema-validation pass.
   let _validated = deserialize_tool_args(
       &tool_use.input,
       &tool_use.name,
       agent_def.llm_backend.as_str(),
   )?;
   ```

2. **Replace** with the new validate call, including the short-circuit on failure:
   ```rust
   // Validate the LLM's args against the tool's declared input_schema
   // (ADR-0010). Validation failure is recoverable: write a structured
   // ToolError into the conversation and continue to the next tool_use
   // so the LLM gets to self-correct. We do NOT propagate via `?`
   // (which would terminate the run via RuntimeError::Tool).
   let validator = self
       .tool_validators()
       .get(tool_use.name.as_str())
       .expect("tool_validators is in 1:1 correspondence with tools (Task 3 invariant)");
   match crate::tool_args::validate_tool_args(
       &tool_use.input,
       &tool_use.name,
       validator,
   ) {
       Ok(_validated) => { /* fall through to Tool::invoke as before */ }
       Err(tool_ports::ToolError::BadArgs { reason }) => {
           warn!(
               name = "tool.args_validation_failed",
               tool_name = %tool_use.name,
           );
           // Best-effort teardown so the plugin gets a chance to
           // clean up, mirroring the existing Err(invoke_err) arm at
           // line 444-445. Errors here are swallowed.
           let _ = tool.teardown(()).await;
           // Write the validation error into the conversation as a
           // MessagePayload::ToolError so the LLM sees it next turn.
           messages.push(Message::new(
               tool_addr.clone(),
               agent_addr.clone(),
               MessagePayload::ToolError {
                   kind: "tool_args_validation".into(),
                   message: reason,
                   details: None,
               },
           ));
           trace!(name = "message.added", kind = "tool_error");
           continue;  // advance to next tool_use; the loop survives
       }
       Err(other) => {
           // validate_tool_args only emits BadArgs in v0.1 — defensive.
           let _ = tool.teardown(()).await;
           return Err(RuntimeError::from(other));
       }
   }
   ```

3. **Delete** the now-unused `deserialize_tool_args` free function (around `run.rs:560-570`) and its tests (around `run.rs:880-905`). Replace the doc comment that mentioned the v0.1 passthrough — search for "Phase-1 schema-validation pass" and remove that paragraph.

4. **Confirm** the `tool_addr` and `agent_addr` bindings are in scope at the call site. From earlier inspection they should be — the per-tool-call loop has those addresses set up around line 350-360. If they're not, look at where the success path's `messages.push(Message::new(tool_addr, agent_addr, ...))` happens (line 476) and bind them earlier.

5. **Update the existing `match outcome` arm** at run.rs:436-447 — leave it ALONE. The plan-erratum block called this out: real plugin invocation crashes still terminate the run via `Err(RuntimeError::from(err))`. Validation failures take a parallel path that doesn't touch this arm.

6. **Verification:**
   ```bash
   cargo build --workspace
   cargo test --workspace --all-targets
   cargo test --workspace --doc
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   ```
   Expected: ALL PASS. Existing run.rs unit tests should still pass (the passthrough was a no-op; validation now fires for any test that constructs a real Runtime with real tools, but the test fixtures use empty/null schemas which opt out).

7. **Commit message:**
   ```
   feat(runtime): wire validate_tool_args into the dispatch loop

   Replaces the v0.1 deserialize_tool_args passthrough at run.rs:417
   with validate_tool_args. On validation failure (LLM hallucinated
   the args), the call site short-circuits BEFORE Tool::invoke:
     - best-effort teardown of the just-opened session;
     - writes MessagePayload::ToolError to the conversation with the
       MANDATORY-rule template;
     - `continue`s the inner for tool_use loop.

   The existing Err(invoke_err) arm at run.rs:436-447 stays unchanged
   — real plugin invocation crashes still terminate the run, which is
   the right outcome for that failure mode.

   Removes the now-unused deserialize_tool_args function + tests.

   Refs: docs/superpowers/specs/2026-04-30-tool-args-schema-design.md §4.4, §4.5
   ```

8. Push.

---

## Task 5: e2e integration test

**Hybrid format.**

**Files:**
- Create: `/Users/titouanlebocq/code/tau/crates/tau-runtime/tests/tool_args_validation_e2e.rs` — gated `#![cfg(unix)]` per existing convention from priority 3's `tool_plugin_e2e.rs`.

**Spec sections:** §8.

**Per-task summary:**

Mirror the harness from `crates/tau-runtime/tests/tool_plugin_e2e.rs`:
- Import the in-process `DynTool` adapter pattern (or copy `InProcessFsRead` if needed; verify whether priority 3's `tool_plugin_e2e.rs` exposes it cross-test).
- Use a `ScriptedFsReadLlm`-style two-turn LLM that emits a malformed call on turn 1 and (in scenario 3) a corrected call on turn 2.
- Build a `Runtime` with `FsReadPlugin` registered (it has a real Draft 7 schema requiring `path: string`).

Three test scenarios:

1. **`bad_args_missing_required_field_surfaces_in_conversation`** — LLM emits `{}` (no `path`). Expected: `RunOutcome::Failed` with `kind: OutOfResources` (max_turns reached because the LLM keeps emitting bad calls and we don't have a self-correcting fixture). The `all_messages` field must contain a `MessagePayload::ToolError { kind: "tool_args_validation", message, .. }` whose `message` includes the MANDATORY substrings ("You sent:", "Expected (input_schema):", "Specific issue").

2. **`bad_args_type_mismatch_surfaces_in_conversation`** — LLM emits `{"path": 42}` (path should be string). Same expected shape: ToolError in the conversation; MANDATORY template substrings present.

3. **`scripted_llm_self_corrects_after_validation_error`** — Turn 1: LLM emits bad args. Turn 2: LLM emits corrected args. Expected: `RunOutcome::Completed` (the run loop survived the validation error and the second turn succeeded). `all_messages` contains BOTH the validation `ToolError` AND a successful `ToolResult` from the corrected call.

```rust
#![cfg(unix)]

mod common;

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use fs_read_plugin_lib::plugin::{FsReadPlugin, FsReadSession};
use tau_domain::{MessagePayload, Value};
use tau_plugin_sdk::Configure;
use tau_ports::{
    fixtures::{make_completion_response, make_token_usage, make_tool_use},
    CompletionRequest, CompletionResponse, CompletionStream, LlmBackend, LlmError, SessionContext,
    StopReason, Tool, ToolError, ToolResult, ToolSpec,
};
use tau_runtime::{builder::DynTool, RunOptions, RunOutcome, Runtime};

// ... (copy InProcessFsRead struct + impl + ScriptedFsReadLlm verbatim
//      from crates/tau-runtime/tests/tool_plugin_e2e.rs lines ~50-211).

// 3 tests as described above. Each test builds a Runtime, runs an
// agent loop, and asserts the per-scenario expectations.
```

Implementer should copy the `InProcessFsRead` adapter struct + impl + LLM helpers from `tool_plugin_e2e.rs` rather than try to share them via a common module — the existing precedent (priority 4's e2e test, priority 5's e2e test) is to duplicate the harness for test isolation.

For scenario 3 specifically, the scripted LLM emits TWO completion responses:
- Turn 1: `tool_use { name: "fs-read", input: { "path": 42 } }` (type mismatch — int instead of string).
- Turn 2: `tool_use { name: "fs-read", input: { "path": "/tmp/something-that-fails-with-IO-error" } }` (validates fine; the actual file read may fail, that's OK — the test asserts on the `ToolResult`/`ToolError` shape, not the read content).
- Turn 3: empty response with `stop_reason: EndTurn`.

The third turn is needed because turn 2's tool call may succeed-but-error (file not found); the run loop continues, and the third turn's `EndTurn` lets the run complete cleanly.

**Verification:**
```bash
cargo build --workspace
cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --doc
```

Expected: ALL PASS. The 3 new e2e tests pass; existing tool_plugin_e2e.rs tests unaffected (different file).

**Commit message:**
```
test(runtime): tool-args schema validation e2e coverage

Three scenarios via the in-process FsReadPlugin adapter (mirrors the
existing tool_plugin_e2e.rs harness):

- bad_args_missing_required_field_surfaces_in_conversation: LLM emits
  {} (path missing) → MessagePayload::ToolError in conversation with
  the MANDATORY-rule template substrings.
- bad_args_type_mismatch_surfaces_in_conversation: LLM emits
  {"path": 42} (int instead of string) → same shape.
- scripted_llm_self_corrects_after_validation_error: turn 1 bad args,
  turn 2 good args. RunOutcome::Completed; conversation contains BOTH
  the validation ToolError AND a follow-up tool result.

Gated #![cfg(unix)] (Windows tempfile paths break TOML embedding —
matches priority 3 + priority 4 e2e test convention).

Refs: docs/superpowers/specs/2026-04-30-tool-args-schema-design.md §8
```

Push.

---

## Task 6: Final verification + open PR

**User-driven gate. PAUSE before this task.**

### Steps

- [ ] **Step 6.1: Full local verification**

```bash
cargo build --workspace
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

All must pass. If anything fails, fix it before opening the PR.

- [ ] **Step 6.2: Open the PR (or mark draft → ready)**

```bash
gh pr list --head feat/tool-args-schema-spec --json number,state,isDraft
```

If empty, create:

```bash
gh pr create --title "feat: tool-args schema validation (Tier 2 priority 6)" \
  --body "$(cat <<'EOF'
## Summary

Validates every tool-call's arguments against the tool's declared `ToolSpec.input_schema` before invoking `Tool::invoke`. Realizes ADR-0006 §3 deferral closure.

- New `tau-runtime::tool_args` module with `ToolArgsValidator` (Draft 7 via `jsonschema` crate).
- Schemas pre-compiled at `RuntimeBuilder::build()` — malformed schemas fail loudly via new `BuildError::ToolSchemaInvalid` variant before any LLM round-trip.
- Validation failures at invoke time surface as `ToolError::BadArgs` carrying the original args, the full declared schema, and the specific issue (the MANDATORY rule from ADR-0010 §4). The agent loop CONTINUES so the LLM self-corrects on the next turn.
- New typed `BuildError::ToolSchemaInvalid` variant. `RuntimeError::PluginContractViolation` stays reserved for a future runtime path (out-of-process plugin lying in handshake).
- New ADR-0010 lands in Task 7.

## Spec / Plan

- Spec: `docs/superpowers/specs/2026-04-30-tool-args-schema-design.md`
- Plan: `docs/superpowers/plans/2026-04-30-tool-args-schema.md`

## Test plan

- [x] `cargo build --workspace` green
- [x] `cargo test --workspace --all-targets` green
- [x] `cargo test --workspace --doc` green
- [x] `cargo clippy --workspace --all-targets -- -D warnings` green
- [x] `cargo fmt --all -- --check` green
- [ ] CI matrix (23 required checks) green — verifying on push

## Out of scope (deferred)

- Runtime trigger path for `RuntimeError::PluginContractViolation` (out-of-process plugin lying about its schema in handshake response). Stays reserved; future sub-project.
- `additionalProperties: false` strict-mode tightening beyond what the plugin author declares.
- Schema dialect upgrade Draft 7 → Draft 2020-12.
- Subschema-only error slicing (currently the full declared schema is included in the error).

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

If a draft already exists, mark ready: `gh pr ready <number>`.

- [ ] **Step 6.3: Capture PR URL**

```bash
gh pr view --json number,url --jq '{number, url}'
```

- [ ] **Step 6.4: PAUSE — wait for CI green before Task 7**

Use the same Bash + run_in_background poller pattern from priority 5's Task 9.

---

## Task 7: ADR-0010 + ROADMAP + squash merge

**User-driven gate. PAUSE before this task.**

**Files:**
- Create: `/Users/titouanlebocq/code/tau/docs/decisions/0010-tool-args-schema-validation.md` — the new ADR (full body, not a stub).
- Modify: `/Users/titouanlebocq/code/tau/ROADMAP.md` — mark Tier 2 priority 6 ✅ + add row to top shipped table.

### Steps

- [ ] **Step 7.1: Write ADR-0010**

Create `/Users/titouanlebocq/code/tau/docs/decisions/0010-tool-args-schema-validation.md`. Use the template at `/Users/titouanlebocq/code/tau/docs/decisions/template.md` if it exists; otherwise mirror the structure of an existing ADR (e.g., ADR-0009).

ADR sections (each as a numbered subsection):
1. **Dialect: JSON Schema Draft 7.** Plugin authors write Draft 7 schemas; the `jsonschema` crate is configured `Draft::Draft7`. Trigger to revisit: ecosystem-wide migration to Draft 2020-12.
2. **Validation location: tau-runtime kernel** via `tool_args::validate_tool_args` (replaces v0.1 `deserialize_tool_args` passthrough). Trigger to revisit: SDK-side enforcement when an out-of-process plugin lies in its handshake response.
3. **Compilation timing: `RuntimeBuilder::build()`.** One-time per registered tool; failure → `BuildError::ToolSchemaInvalid`. Trigger to revisit: dynamic tool registration mid-run (currently impossible).
4. **Error semantics:**
   - Schema malformed at build → `BuildError::ToolSchemaInvalid` (terminates build).
   - Args mismatch at invoke → `ToolError::BadArgs` with **MANDATORY template** (recoverable; LLM sees error in conversation; loop continues; LLM self-corrects on next turn).
   - **MANDATORY template (locked at validator boundary):** every BadArgs reason MUST contain `"You sent:"`, `"Expected (input_schema):"`, and `"Specific issue"` substrings.
5. **`RuntimeError::PluginContractViolation` stays reserved** for a future runtime trigger path (out-of-process plugin lying in handshake response). v0.1 of this sub-project does NOT light it up.

Status: Accepted, 2026-04-30.

Cross-references: ADR-0006 §3 (the deferral this closes), ADR-0009 (typed-error policy this follows).

- [ ] **Step 7.2: Update ROADMAP**

Find the Tier 2 priority 6 entry (around line 110-112 of ROADMAP.md):

```markdown
6. **Schema validation for tool args** (activates
   `RuntimeError::PluginContractViolation`).
```

Replace with:

```markdown
6. **Schema validation for tool args** ✅ Shipped 2026-04-30 — see
   [spec](docs/superpowers/specs/2026-04-30-tool-args-schema-design.md)
   and [ADR-0010](docs/decisions/0010-tool-args-schema-validation.md).
   Validates every tool-call's arguments against the tool's declared
   `ToolSpec.input_schema` (JSON Schema Draft 7) at the kernel
   boundary. Schemas pre-compile at `RuntimeBuilder::build()` —
   malformed schemas fail loudly via new `BuildError::ToolSchemaInvalid`
   before any LLM round-trip. Runtime validation failures surface as
   `ToolError::BadArgs` with the MANDATORY template (original args +
   full schema + specific issue) so the LLM self-corrects on the next
   turn. `RuntimeError::PluginContractViolation` stays reserved for a
   future out-of-process plugin handshake-lying trigger path. No new
   CI jobs (23 required checks unchanged).
```

Add to the top-of-file shipped table (mirror priority 5's row format):

```markdown
| 6 | Schema validation for tool args ✅ | Tier 2 priority 6 — realizes ADR-0006 §3 deferral closure. New `tau-runtime::tool_args` module with `ToolArgsValidator` (Draft 7 via `jsonschema` crate). Schemas pre-compile at `RuntimeBuilder::build()`; malformed → `BuildError::ToolSchemaInvalid`. Runtime arg-validation failures surface as `ToolError::BadArgs` with MANDATORY template (original args + full schema + specific issue) so the LLM self-corrects via the conversation. Loop survives validation errors; only real plugin invocation crashes still terminate. New ADR-0010. No new CI jobs (23 required checks unchanged). | 2026-04-30 |
```

Update the front-matter narrative paragraph (around line 20-25) to reflect that priorities 4, 5, AND 6 are closed.

- [ ] **Step 7.3: Commit + push**

```bash
git add docs/decisions/0010-tool-args-schema-validation.md ROADMAP.md
git commit -m "$(cat <<'EOF'
docs: ADR-0010 + ROADMAP Tier 2 priority 6 done

Locks the 5 design decisions for tool-args schema validation:
1. Dialect: JSON Schema Draft 7
2. Validation location: tau-runtime kernel
3. Compilation timing: RuntimeBuilder::build()
4. Error semantics: BuildError::ToolSchemaInvalid (build) +
   ToolError::BadArgs (invoke) with MANDATORY template
5. RuntimeError::PluginContractViolation stays reserved

Updates ROADMAP:
- Top-of-file shipped table gains a row for Tier 2 priority 6.
- Tier 2 priority 6 entry marked ✅ Shipped 2026-04-30 with key
  artifacts called out.
- Front-matter narrative updates to reflect that priorities 4, 5,
  and 6 are all closed.

No new CI jobs in this sub-project; branch protection stays at 23
required checks.

Refs: docs/superpowers/specs/2026-04-30-tool-args-schema-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push
```

- [ ] **Step 7.4: Wait for CI green on the PR**

Same poller pattern as priority 5. 23 required checks must all pass.

- [ ] **Step 7.5: Squash merge**

```bash
gh pr merge --squash --delete-branch
```

- [ ] **Step 7.6: Verify branch protection unchanged**

```bash
gh api repos/LEBOCQTitouan/tau/branches/main/protection/required_status_checks/contexts | jq 'length'
```
Expected: `23`.

- [ ] **Step 7.7: Sync local main + report squash SHA**

```bash
git checkout main && git pull && git log --oneline -3
```

Report back to the user with the squash SHA.

---

## Verification standard (per task)

Each task ends with:

```bash
cargo build --workspace
cargo test -p tau-runtime --all-targets
cargo test -p tau-runtime --doc
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

For tasks touching multiple crates (none in this plan — everything is in tau-runtime), use `cargo test --workspace --all-targets` instead.

CI continues on push; no new jobs added; branch protection stays at 23.
