# Tool-Args Schema Validation — Design Spec

**Date:** 2026-04-30
**Status:** Approved (pending user review of this written spec)
**Sub-project:** Tier 2 priority 6 (per ROADMAP `Tier 2 — completes Phase 0 deferrals`).
**Closes deferral:** ADR-0006 §3 / Consequences — `RuntimeError::PluginContractViolation` "wired but not yet trigger-pathed"; `deserialize_tool_args` is currently a v0.1 passthrough.

---

## 1. Summary

Validate every tool-call's arguments against the tool's declared
`ToolSpec.input_schema` before invoking `Tool::invoke`. Validation runs
at the kernel boundary (`tau-runtime`), uses the `jsonschema` crate
configured for JSON Schema Draft 7, and pre-compiles each tool's schema
once at `RuntimeBuilder::build()` so per-invoke cost stays in the tens
of microseconds.

Two distinct failure paths:

- **Schema malformed at registration** → `BuildError::ToolSchemaInvalid`.
  The build fails before any LLM is called. Catches plugin-author bugs
  fast, costs zero tokens.
- **Args mismatch at invoke** → `ToolError::BadArgs` carrying a
  self-instructive error message that includes the full declared
  `input_schema`. The agent loop **continues**; the LLM sees the error
  in the conversation and self-corrects on the next turn. This is the
  dominant case (LLM hallucinated a tool call) and recovery is
  recoverable, not fatal.

A new ADR-0010 lands as part of this sub-project to lock the dialect
(Draft 7), the validation location (kernel), and the BadArgs-includes-schema
mandatory rule.

This sub-project is wholly in-tree: changes to `tau-runtime`, a new
ADR, and a new workspace dep on `jsonschema`. No new workspace member.

---

## 2. Background and motivation

ADR-0006 §3 + Consequences (lines 376-380) document the deferral:

> `RuntimeError::PluginContractViolation` is wired (Task 10) but the
> v0.1 implementation does not have a trigger path:
> `deserialize_tool_args` is a passthrough today. Phase-1+ schema
> validation will populate this variant; until then the variant is
> dead code on every observed run.

The hook (`deserialize_tool_args` at `crates/tau-runtime/src/run.rs:564-570`)
is already in place — a v0.1 passthrough returning `Ok(value)`. This
sub-project replaces the body with real validation.

Motivation: tools today guard their own arg shape via ad-hoc
deserialization (e.g., fs-read's `parse_path_arg` returning
`BadArgs` if `path` is missing). This is fragile (every plugin
reimplements the same checks), inconsistent (different error message
formats across plugins), and silently permissive (a typo in the args
key fails at the wrong layer). Hoisting validation to the kernel:

1. Plugins can trust their args without duplicate guards.
2. Error messages are uniform across all tools.
3. The LLM gets the schema back in the error reason — making
   self-correction reliable.

Aside from the user-facing experience, this also closes the formal
ADR-0006 deferral and lights up the existing
`RuntimeError::PluginContractViolation` (or its sibling
`BuildError::ToolSchemaInvalid` — see §3 Q4) trigger paths.

---

## 3. Decisions table

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| Q1 | Validation location | **A** — tau-runtime kernel via the existing `deserialize_tool_args` hook | Hook is already there; one place to maintain; failures route directly to the typed `ToolError::BadArgs` variant |
| Q2 | Schema engine | **A** — `jsonschema` crate (~1MB compiled) | Most popular, full Draft 7/2019-09/2020-12 support, regex caching. Heavy crates already in tau-runtime (reqwest, tokio); 1MB is negligible |
| Q3 | Schema dialect | **A** — JSON Schema Draft 7 | Matches MCP spec + Anthropic tool-use API + every shipped plugin schema; jsonschema crate's default; least disruptive |
| Q4 | Error semantics | **C** — split: malformed-at-build → `BuildError::ToolSchemaInvalid` (terminates build); args-mismatch-at-invoke → `ToolError::BadArgs` (recoverable, LLM sees error and retries) | Two failure modes have genuinely different recoverability profiles. Plugin-author bugs catch early; LLM hallucinations stay recoverable |
| Q4 add. | MANDATORY rule for BadArgs reason | Every BadArgs from validation MUST include the tool's full declared `input_schema` plus the original args plus the specific issue. Locked at the validator boundary | Self-instructive recovery. Without this, the LLM may re-emit the same bad call because it doesn't know the right shape |
| Q5 | Validation timing | **B** — pre-compile at `RuntimeBuilder::build()` | Standard build-once-validate-many pattern. Compilation cost amortized; per-invoke is microseconds; doubles as registration-time well-formedness check |
| Q6 | Distribution | In-tree changes to tau-runtime; new ADR-0010 | YAGNI — only one consumer (the kernel). No new workspace member |

---

## 4. Architecture

### 4.1 Module layout

A new module `crates/tau-runtime/src/tool_args.rs` owns the validator
type and the validation entry point. The dispatch path in `run.rs`
calls into it.

```rust
// tau-runtime::tool_args

#[non_exhaustive]
pub(crate) struct ToolArgsValidator {
    /// Compiled jsonschema instance. None = tool opted out (empty
    /// schema or `Value::Null`); validation is a no-op.
    compiled: Option<jsonschema::JSONSchema>,
    /// The original input_schema as declared, kept for inclusion in
    /// BadArgs error messages (the MANDATORY rule).
    declared_schema_json: String,
}

impl ToolArgsValidator {
    pub(crate) fn compile(input_schema: &tau_domain::Value) -> Result<Self, SchemaCompileError>;
    pub(crate) fn validate(&self, args: &tau_domain::Value, tool_name: &str) -> Result<(), String>;
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub(crate) struct SchemaCompileError {
    pub kind: String,             // jsonschema's diagnostic
    pub schema_excerpt: String,   // first 200 chars of the schema for context
}
```

Empty/null schemas opt out cleanly: `compile` returns
`Ok(Self { compiled: None, declared_schema_json: "{}".into() })` when
`input_schema` is `Value::Null` or `Value::Object(empty)`. Validation
is a no-op for these tools.

### 4.2 Storage on RegisteredTool

The existing tools registry in `RuntimeBuilder` carries an
`Arc<dyn DynTool>` per tool. Each tool gains a paired validator:

```rust
pub(crate) struct RegisteredTool {
    pub tool: Arc<dyn DynTool>,
    pub validator: ToolArgsValidator,
}
```

`RuntimeBuilder.tools` becomes `HashMap<ToolName, RegisteredTool>`.
Public `Runtime::tools()` continues to return `Arc<dyn DynTool>` views;
the validator field is internal to the kernel's dispatch path.

### 4.3 Build-time pre-compilation

`RuntimeBuilder::build()` compiles each registered tool's schema once.
Failure surfaces as the new `BuildError::ToolSchemaInvalid` variant:

```rust
impl RuntimeBuilder {
    pub fn build(self) -> Result<Runtime, BuildError> {
        // ... existing checks ...
        let mut registered: HashMap<ToolName, RegisteredTool> = HashMap::new();
        for (name, tool) in self.tools {
            let spec = tool.schema();
            let validator = ToolArgsValidator::compile(&spec.input_schema).map_err(|e| {
                BuildError::ToolSchemaInvalid {
                    tool_name: name.as_str().to_string(),
                    detail: format!("{}: {}", e.kind, e.schema_excerpt),
                }
            })?;
            registered.insert(name, RegisteredTool { tool, validator });
        }
        // ... rest of build ...
    }
}
```

### 4.4 Invoke-time validation

The current passthrough at `run.rs:564-570`:

```rust
pub(crate) fn deserialize_tool_args<'a>(
    value: &'a Value,
    _tool_name: &str,
    _llm_backend_name: &str,
) -> Result<&'a Value, RuntimeError> {
    Ok(value)
}
```

is replaced with:

```rust
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
```

Note the return type changes from `Result<_, RuntimeError>` to
`Result<_, ToolError>`. This is the **C-path** decision surfacing in
the type signature: args mismatch is recoverable, NOT a kernel-level
run-terminator. The existing `?` propagation in `run.rs` already
converts `ToolError → RuntimeError::Tool(_)` via `#[from]`, but the
call site (§4.5) explicitly catches the `Err(ToolError)` and routes
it through the existing tool-error handling that writes
`MessagePayload::ToolError` to the conversation and continues the
loop. No run termination on validation failure.

### 4.5 Call-site integration

At `run.rs:415-421` (the existing call to the passthrough). NOTE: the
current `Err(invoke_err)` arm at `run.rs:436-447` returns `Err(RuntimeError::from(err))`
— it TERMINATES the run on real plugin invocation failures. That behavior
is correct for "the plugin process crashed mid-call" and stays unchanged.

Validation failures are a different failure mode: the LLM produced a
malformed call before the plugin was even reached. We don't want a
hallucinated arg to terminate the run. So validation failure follows
its own short-circuit path: skip `Tool::invoke` entirely, write a
`MessagePayload::ToolError` to the conversation, and `continue` the
inner `for tool_use` loop so the next tool_use (or the next turn)
sees the validation error in the conversation context.

```rust
// Replaces the existing line 417 call to deserialize_tool_args.
match validate_tool_args(&tool_use.input, &tool_use.name, &registered.validator) {
    Ok(_) => { /* proceed to Tool::invoke as before */ }
    Err(tool_err) => {
        warn!(name = "tool.args_validation_failed", tool_name = %tool_use.name);
        // Best-effort teardown of the session we just opened, mirroring
        // the existing Err(invoke_err) arm at line 444-445 — but we
        // continue rather than return, because validation failure is
        // recoverable.
        let _ = tool.teardown(()).await;
        // Write the validation error into the conversation as a
        // MessagePayload::ToolError so the LLM sees the structured
        // error next turn and self-corrects.
        let validation_msg = match &tool_err {
            ToolError::BadArgs { reason } => reason.clone(),
            other => format!("{other}"), // defensive — validate_tool_args only emits BadArgs
        };
        messages.push(Message::new(
            tool_addr.clone(),
            agent_addr.clone(),
            MessagePayload::ToolError {
                kind: "tool_args_validation".into(),
                message: validation_msg,
                details: None,
            },
        ));
        trace!(name = "message.added", kind = "tool_error");
        continue;  // advances to the next tool_use in the inner for loop
    }
}
```

This path doesn't reuse the `Err(invoke_err)` arm at line 436-447
(which `return`s a `RuntimeError`). Validation failures are
short-circuited before `Tool::invoke` is even called and continue the
loop so the LLM gets to retry on the next turn.

The existing `Err(invoke_err)` arm continues to `return` on real
plugin invocation crashes — that's a kernel-level failure (the plugin
process likely died or violated the wire contract) and run-termination
is the right outcome there. Validation is a non-overlapping new path.

### 4.6 The MANDATORY error template

Every `BadArgs.reason` produced by `ToolArgsValidator::validate` MUST
contain three blocks: what-the-LLM-sent, what-the-schema-is, and the
specific issue. Locked at the validator boundary so no caller can
short-circuit it:

```rust
impl ToolArgsValidator {
    pub(crate) fn validate(&self, args: &Value, tool_name: &str) -> Result<(), String> {
        let Some(compiled) = &self.compiled else { return Ok(()); };
        let args_json = serde_json::to_value(args).map_err(|e| {
            format!("internal: args serialization failed: {e}")
        })?;
        if let Err(errors) = compiled.validate(&args_json) {
            let issues: Vec<String> = errors.map(|e| {
                format!("{}: {}", e.instance_path, e)
            }).collect();
            return Err(format!(
                "{tool_name}: args validation failed.\n\n\
                 You sent: {args_repr}\n\n\
                 Expected (input_schema):\n{schema}\n\n\
                 Specific issue(s):\n{issues}",
                args_repr = serde_json::to_string(&args_json)
                    .unwrap_or_else(|_| "<unprintable>".into()),
                schema = self.declared_schema_json,
                issues = issues.join("\n"),
            ));
        }
        Ok(())
    }
}
```

The 3-tuple `(what-you-sent, what-was-expected, why-it-failed)` is
exactly what the LLM needs to retry correctly. Tests assert this
template explicitly.

---

## 5. Type changes

### 5.1 New types in `tau-runtime`

Already shown in §4.1. Recap:

- `tool_args::ToolArgsValidator` — `pub(crate)` (kernel-internal).
- `tool_args::SchemaCompileError` — `pub(crate)`.
- `tool_args::validate_tool_args` — `pub(crate)`.
- `builder::RegisteredTool` — `pub(crate)`.

### 5.2 Modified types

`BuildError` is `#[non_exhaustive]`. New variant (additive,
non-breaking):

```rust
#[error("tool {tool_name:?}: input_schema is not a valid Draft 7 schema: {detail}")]
ToolSchemaInvalid {
    /// The tool name whose schema failed to compile.
    tool_name: String,
    /// Diagnostic from the schema compiler.
    detail: String,
},
```

`RuntimeError::PluginContractViolation` (already-reserved variant)
stays reserved for the runtime path (out-of-process plugin lying
about its schema in the handshake response). v0.1 of THIS sub-project
does not light it up; the build path covers all in-tree uses.

`ToolError::BadArgs { reason }` is reused (no schema change). The new
`reason` content is constrained by §4.6's MANDATORY template.

### 5.3 No `Tool` trait changes

The `Tool` trait stays unchanged. Plugins continue to declare
`input_schema` via `Tool::schema().input_schema` exactly as today.
The kernel handles validation; plugins are oblivious.

---

## 6. CLI surface

No new CLI surface. Existing user experience changes:

- `tau run` / `tau chat` will surface validation failures in the
  conversation as `MessagePayload::ToolError` (just like any other
  tool error today). The LLM sees the structured error and
  self-corrects.
- `RuntimeBuilder::build()` may now fail with
  `BuildError::ToolSchemaInvalid` when a registered tool's schema
  is malformed. The CLI's existing `BuildError`-displaying path
  (which prints the typed error to stderr and exits 2) handles this
  uniformly.

---

## 7. ADR-0010 — Tool-args schema validation policy

A new ADR lands alongside the implementation. Pins:

1. **Dialect: JSON Schema Draft 7.** Plugin authors write Draft-7-flavored
   schemas; the `jsonschema` crate validates against Draft 7.
2. **Validation location: tau-runtime kernel** via the
   `validate_tool_args` hook (replacing v0.1's `deserialize_tool_args`
   passthrough).
3. **Compilation timing: at `RuntimeBuilder::build()`.** One-time
   cost; pre-compile catches malformed schemas before any LLM
   round-trip.
4. **Error semantics:**
   - Schema malformed at build → `BuildError::ToolSchemaInvalid`
     (terminates build; the CLI exits 2 with the typed error).
   - Args mismatch at invoke → `ToolError::BadArgs` (recoverable;
     LLM sees the formatted error and retries).
5. **MANDATORY rule (locked at validator boundary):** every BadArgs
   reason from validation MUST include the full declared
   `input_schema` plus the original args plus the specific issue.

Trigger to revisit: a future where out-of-process plugins lie about
their schema in the handshake response — at which point
`RuntimeError::PluginContractViolation` gains a runtime trigger path
that mirrors the build-time check for IPC tools.

---

## 8. Testing

| Tier | Coverage | Where |
|------|----------|-------|
| Unit | `ToolArgsValidator::compile` — empty/null schema → opt-out (None); valid Draft 7 → Some(compiled); malformed schema → `SchemaCompileError` | `crates/tau-runtime/src/tool_args.rs::tests` (~6 tests) |
| Unit | `ToolArgsValidator::validate` — well-formed args pass; missing required → `BadArgs` with schema in reason; type mismatch → `BadArgs` with schema in reason; out-of-range integer (`minimum`/`maximum`) → `BadArgs` with schema in reason | `tool_args.rs::tests` (~5 tests) |
| Unit | **MANDATORY-rule assertion** — every error reason produced by `validate` contains the literal substrings `"You sent:"`, `"Expected (input_schema):"`, and `"Specific issue"` | `tool_args.rs::tests` (~1 dedicated test) |
| Unit | `RuntimeBuilder::build()` rejects a tool whose schema doesn't compile via `BuildError::ToolSchemaInvalid` | `crates/tau-runtime/src/builder.rs::tests` (~2 tests) |
| Integration | End-to-end via the existing `tool_plugin_e2e.rs` harness pattern: agent emits a bad tool call → `BadArgs` is surfaced in `MessagePayload::ToolError` → the LLM-loop continues → the error message contains the schema | `crates/tau-runtime/tests/tool_args_validation_e2e.rs` (~3 tests, gated `#![cfg(unix)]` per the existing convention) |

Test fixtures: reuse the existing `FsReadPlugin` and `EchoToolPlugin`
from priority 3 + the in-process `DynTool` adapter pattern from
`tool_plugin_e2e.rs`. No new fixture infrastructure needed.

---

## 9. Out of scope (deferred)

- **Runtime trigger path for `PluginContractViolation`** when an
  out-of-process plugin lies about its schema in the handshake
  response. Stays reserved; future sub-project. v0.1 of this
  sub-project covers in-tree (in-process + IPC-via-SDK) tools whose
  schemas are visible at `RuntimeBuilder::build()`.
- **`additionalProperties: false` strict-mode tightening** beyond
  what the plugin author declares. v0.1 honors whatever the
  plugin's schema says; we don't add an opinionated overlay.
- **Streaming validation.** Args arrive as a complete `Value`;
  partial validation isn't a thing.
- **Schema migration / Draft 2020-12 upgrade.** ADR-0010 commits to
  Draft 7. A future ADR can supersede when the ecosystem moves.
- **Subschema-only error sliced** (vs. the full schema). For brevity
  in error messages, future work could include only the failing
  field's sub-schema. v0.1 includes the full declared schema —
  simpler, matches what was visible at registration time.
- **Custom `format` validators** (e.g., `format: "uri"`,
  `format: "email"`). The `jsonschema` crate honors the standard
  Draft 7 formats; we don't add tau-specific formats.

---

## 10. Implementation plan outline (~6–7 tasks)

The plan derived from this spec will have these tasks. Final wording
lives in the implementation plan.

1. Add `jsonschema` to root `Cargo.toml` `[workspace.dependencies]`;
   wire into `crates/tau-runtime/Cargo.toml`.
2. Create `crates/tau-runtime/src/tool_args.rs` — `ToolArgsValidator`,
   `SchemaCompileError`, `validate_tool_args`, full unit tests
   covering the MANDATORY-rule template.
3. Modify `crates/tau-runtime/src/builder.rs` — introduce
   `RegisteredTool { tool, validator }`; update `build()` to compile
   schemas; add `BuildError::ToolSchemaInvalid` variant.
4. Modify `crates/tau-runtime/src/run.rs` — replace the
   `deserialize_tool_args` call site with `validate_tool_args`;
   route validation failure through the existing `Err(ToolError)`
   handling that writes `MessagePayload::ToolError` and continues the
   loop.
5. Add e2e integration test at
   `crates/tau-runtime/tests/tool_args_validation_e2e.rs`.
6. Write ADR-0010
   `docs/decisions/0010-tool-args-schema-validation.md`.
7. (Gate) Final verification + open PR; (gate) ADR-0010 + ROADMAP
   update + squash merge.

Each task is a single Conventional Commits commit. Per-task
verification: `cargo build/test/doc/fmt/clippy` workspace-level. CI:
no new jobs (no new workspace member; no new external service).
Branch protection stays at 23.

---

## 11. Cross-references

- ADR-0006 (tau-runtime) §3 + Consequences (lines 376-380) — the
  reservation this spec closes.
- `crates/tau-runtime/src/run.rs:564-570` — current
  `deserialize_tool_args` passthrough; replaced by
  `validate_tool_args`.
- `crates/tau-runtime/src/error.rs:168-175` —
  `RuntimeError::PluginContractViolation`; stays reserved per §5.2.
- `crates/tau-runtime/src/error.rs` (BuildError) — gains
  `ToolSchemaInvalid` variant per §5.2.
- `crates/tau-plugins/{fs-read,shell,echo-tool}/...` — shipped
  schemas that exercise Draft 7 keywords (`type`, `properties`,
  `required`, `items`, `minimum`, `maximum`).
- `crates/tau-runtime/tests/tool_plugin_e2e.rs` — reference pattern
  for the new e2e test in §8.
- ADR-0009 (typed errors + conformance) — new variants follow this
  policy.
- ADR-0010 (this sub-project's deliverable) — locks the design
  decisions in §3.
