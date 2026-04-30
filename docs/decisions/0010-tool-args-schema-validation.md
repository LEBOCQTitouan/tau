# ADR-0010: Tool-args schema validation policy

**Status:** Accepted
**Date:** 2026-04-30
**Deciders:** Titouan Lebocq
**Supersedes:** —
**Closes:** [ADR-0006](0006-tau-runtime.md) §3 + Consequences (lines
376-380) — `RuntimeError::PluginContractViolation` was "wired but not
yet trigger-pathed"; `deserialize_tool_args` was a v0.1 passthrough.
**Amends:** —
**Refines:** [ADR-0006](0006-tau-runtime.md) §9 (outcome/error
dichotomy — this ADR explicitly classifies validation failure modes
across the build-time / invoke-time boundary).

## Context

Tools today declare an `input_schema: tau_domain::Value` (a JSON
Schema object) via `Tool::schema().input_schema`, but the runtime
never validates the LLM's tool-call args against it before invoking
`Tool::invoke`. Each plugin guards its own arg shape via ad-hoc
deserialization (e.g. fs-read's `parse_path_arg` returning `BadArgs`
if `path` is missing). The cost of this:

1. Plugins reimplement the same checks. Drift between schemas and
   guards is inevitable; the schema becomes documentation, not
   enforcement.
2. Error message format varies per plugin. When the LLM produces a
   bad tool call, the recovery message it sees is whatever the
   plugin author wrote — sometimes precise, sometimes "missing
   field".
3. The schema isn't validated at registration time. A typo in
   `"type": "objectt"` silently ships in `tau install`, and only
   blows up when the LLM happens to emit a call that exercises the
   broken constraint — *after* tokens have been spent.

ADR-0006 §3 / Consequences (lines 376-380) reserved this work
explicitly:

> `RuntimeError::PluginContractViolation` is wired (Task 10) but the
> v0.1 implementation does not have a trigger path:
> `deserialize_tool_args` is a passthrough today. Phase-1+ schema
> validation will populate this variant; until then the variant is
> dead code on every observed run.

This ADR closes that reservation by hoisting validation to the
kernel.

## Decision

Five inter-locking commitments:

### 1. Dialect: JSON Schema Draft 7

Plugin authors write Draft-7-flavored schemas via
`serde_json::json!{...}` literals (or equivalent). The runtime uses
the [`jsonschema`](https://crates.io/crates/jsonschema) crate
configured `Draft::Draft7`.

Rationale: Draft 7 is what every shipped tau plugin's
`Tool::schema()` already uses — `type`, `properties`, `required`,
`items`, `minimum`, `maximum`, `description`, `default`. It matches
the [MCP](https://modelcontextprotocol.io/) spec's tool-input format
and Anthropic's tool-use API. Migration to Draft 2020-12 is deferred
to a future ADR if the ecosystem moves.

Trigger to revisit: ecosystem-wide adoption of Draft 2020-12 in
LLM tool-use APIs.

### 2. Validation location: tau-runtime kernel

Validation runs at `tau-runtime::tool_args::validate_tool_args`,
called from `run.rs` immediately before `Tool::invoke`. This
replaces the v0.1 `deserialize_tool_args` passthrough.

Rationale: the hook was already in place at `run.rs:417` with the
right signature (post-capability-check, pre-invoke). One enforcement
point eliminates plugin/runtime drift. Routing failures through
typed error variants (`BuildError::ToolSchemaInvalid` at build,
`ToolError::BadArgs` at invoke) integrates naturally with the
existing `RuntimeBuilder::build()` / `Runtime::run` flows.

Alternative considered: SDK-side enforcement
(`tau-plugin-sdk::run_tool_with_io`). Rejected at v0.1 because
in-process tools (the in-tree path used by every test) wouldn't
exercise SDK-side validation, and out-of-process plugins still pay
the IPC round-trip before validation fires.

Trigger to revisit: an out-of-process plugin lying about its schema
in the handshake response (responding with one schema then
`Tool::invoke`-ing under a different one). At that point the
runtime gains a parallel SDK-side enforcement path — see decision 5.

### 3. Compilation timing: at `RuntimeBuilder::build()`

Each registered tool's `input_schema` is compiled **once** via
`ToolArgsValidator::compile(&input_schema)`. The compiled validator
is stored in `Runtime.tool_validators: HashMap<String, ToolArgsValidator>`,
a parallel field next to `Runtime.tools`. Per-invoke validation
just runs the compiled matcher (~10–50µs).

Failure to compile (malformed schema) returns
`SchemaCompileError`, which the builder maps to
`BuildError::ToolSchemaInvalid { tool_name, detail }` — the build
fails before any `Runtime::run` is called and before any LLM
round-trip. This catches plugin-author bugs at the earliest
possible point in the lifecycle.

Empty schemas (`Value::Null` or `Value::Object(empty)`) opt out
cleanly — `compile` returns a validator with `compiled: None` and
`validate` is a no-op. Tools that genuinely accept arbitrary input
declare an empty schema.

Rationale: build-once-validate-many is the standard jsonschema
pattern. The cost of compilation is non-trivial (regex precompile,
type-coercion graph); paying it per-invoke is wasteful for an
agent loop that may issue dozens of tool calls.

Trigger to revisit: dynamic tool registration mid-run (currently
impossible — `Runtime` is immutable after `build()`).

### 4. Error semantics: split by failure mode

| Failure mode | Trigger path | Outcome | LLM sees |
|---|---|---|---|
| Schema malformed | Build time, in `compile()` | `BuildError::ToolSchemaInvalid` (build aborts) | Nothing — build fails before LLM is called |
| Args mismatch | Invoke time, in `validate()` | `ToolError::BadArgs` (recoverable) | Structured error in conversation; loop continues; LLM self-corrects on next turn |

The two failure modes have **genuinely different recoverability
profiles**. A malformed schema is a plugin-author bug; the user
needs to fix it, and continuing the run wastes tokens. An args
mismatch is most likely an LLM hallucination; terminating the run
on every hallucinated arg would be hostile to the dominant case.

**MANDATORY rule (locked at validator boundary):** every
`ToolError::BadArgs.reason` produced by `ToolArgsValidator::validate`
MUST contain three blocks:
1. `"You sent: {args_json}"` — the args the LLM actually emitted.
2. `"Expected (input_schema):\n{schema_json}"` — the full declared
   schema the args are failing against.
3. `"Specific issue(s):\n{validation_errors}"` — the per-field
   diagnostic from jsonschema's `iter_errors`.

The rule is enforced at `ToolArgsValidator::validate`'s only return
path, so callers cannot short-circuit it. Tests assert the literal
substrings `"You sent:"`, `"Expected (input_schema):"`, and
`"Specific issue"` are present in every failure-case error.

Rationale: the LLM that emitted the bad call is also the one that
needs to recover. Without the schema in the error message, the LLM
may re-emit the same bad call because it doesn't know what shape is
expected. With it, the error is **self-instructive**: the model
sees `(what-I-sent, what-was-expected, why-it-failed)` in one
round-trip and can correct.

Trigger to revisit: real-world data showing the LLM doesn't
recover even with the schema in the error — at which point error
formatting becomes a tuning problem rather than a structural one.

### 5. `RuntimeError::PluginContractViolation` stays reserved

The existing `RuntimeError::PluginContractViolation { plugin, detail }`
variant remains documented and wired but **does not gain a trigger
path in this sub-project**. ADR-0006 documented it as
"wired but not yet trigger-pathed"; v0.1 of this work narrows the
trigger paths to **build-time `ToolSchemaInvalid`** and
**invoke-time `BadArgs`**. The reserved variant becomes the home
for a future runtime check: out-of-process plugin lies about its
schema in the handshake response, then later violates the declared
contract during invoke.

Rationale: opening that path requires SDK-side enforcement
infrastructure (decision 2's "alternative considered"). Building
it speculatively before a real motivating case lands would over-
engineer the v0.1 surface.

Trigger to revisit: a third-party tool plugin shipped via tau's
plugin protocol whose handshake-declared schema diverges from its
actual `Tool::invoke` behavior — at which point a runtime
validation path lights up the variant.

## Consequences

### Negative / new cost

- `tau-runtime` gains a transitive dep on `jsonschema = "0.46"`
  (~1MB compiled, brings `regex_syntax`, `fancy-regex`, `email_address`,
  `ahash` into the dep tree). Negligible vs. the existing reqwest /
  tokio / semver footprint.
- `RuntimeBuilder::build()` is now O(tools × schema_size) at
  compile time. For typical agent configs (<10 tools, schemas in
  the hundreds of bytes) this is well under 1ms. Plugins with
  pathologically large schemas — e.g., a `enum` with 10000 entries
  — could push this; future perf work can address.
- `Runtime` gains a parallel `tool_validators` field, increasing the
  struct size by a HashMap. Memory cost for typical configs is in
  the noise.

### Positive

- Plugin authors stop reimplementing argument shape checks. The
  `parse_*_arg` patterns in shipped plugins (e.g., fs-read,
  shell) become redundant once Phase-2 cleanup lands; for v0.1
  they coexist defensively.
- Plugin-author bugs (schema typos) catch at `tau install` /
  `tau run` startup before any LLM is called. Zero tokens
  wasted.
- LLMs get **uniform** structured error feedback across all tools,
  with the schema embedded for self-correction.

### Neutral / new obligations

- Future tau-runtime API additions involving validation require
  their own ADRs (QG18). The MANDATORY-rule template is locked at
  this ADR; any softening (e.g., subschema-only error slicing)
  needs a new ADR.
- The `jsonschema` crate version is pinned to the major (`"0.46"`)
  in `[workspace.dependencies]`; major upgrades (e.g., 0.47) need
  to verify the public API surface — past versions of jsonschema
  have renamed types between minors.

## Alternatives considered

### A. Hand-roll a minimal Draft 7 subset

Rejected. Validate only the keywords actually used by shipped plugins
(`type`, `properties`, `required`, `items`, `minimum`, `maximum`).
~150 LOC + tests. Zero deps.

Cons: any plugin author using `enum`, `pattern`, `format`,
`oneOf` would silently have those keywords ignored — no compile-time
warning, no runtime error, just silently permissive validation.
For a system that will accept third-party plugins, this is a footgun.

### B. Schema engine: `valico` instead of `jsonschema`

Rejected. Lighter (~200KB) but Draft-4-only and "lightly maintained"
since 2023. Bus-factor risk if we hit a bug, and Draft 4's
constraint vocabulary differs from what shipped plugins use
(e.g., `required` was an object property in Draft 4, became a
top-level array in Draft 6+).

### C. Validation location: SDK runner only (in-plugin enforcement)

Rejected. See decision 2's "alternative considered". The runtime
boundary is the right enforcement point at v0.1; SDK-side adds
coverage gaps for out-of-process plugins that bypass the SDK.

### D. Per-invoke compilation (no caching)

Rejected. See decision 3. Order-of-magnitude slower for busy agent
loops; loses the registration-time well-formedness check that's
load-bearing for decision 4's "schema malformed" build-fatal path.

### E. Treat all validation failures as kernel-fatal
(via `RuntimeError::PluginContractViolation`)

Rejected. See decision 4. Args mismatch is dominantly an LLM
hallucination (recoverable); terminating the run on every
hallucinated arg is hostile to the dominant case. The
build-fatal/invoke-recoverable split matches the actual
recoverability profile.

## References

- Spec: `docs/superpowers/specs/2026-04-30-tool-args-schema-design.md`
- Plan: `docs/superpowers/plans/2026-04-30-tool-args-schema.md`
- ADR-0006 §3 + Consequences — the deferral this ADR closes.
- ADR-0009 (typed-error policy) — new variants follow this policy.
- `crates/tau-runtime/src/tool_args.rs` — the validator module.
- `crates/tau-runtime/src/run.rs:417`-area — the call-site integration.
- `crates/tau-runtime/src/builder.rs::collect_tools_by_name` — the
  build-time compilation step.
