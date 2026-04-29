# ADR-0009: Typed `LlmError` migration policy + conformance suite charter

**Status:** Proposed
**Date:** 2026-04-29
**Deciders:** Titouan Lebocq
**Supersedes:** —
**Closes:** [ADR-0008](0008-plugin-loading.md) §17 (conformance test
suite deferral — "until ≥2 real LLM-backend implementations exist").
**Amends:** —
**Refines:** [ADR-0007](0007-tau-cli.md) (escape-hatch registry rule
applies to the `LlmError::Internal` callsite-scope reduction in this
sub-project).

## Context

Phase 1 sub-project 2c shipped the OpenAI plugin alongside two
infrastructure deliverables:

1. A shared `tau-plugin-test-support` crate (cassette replayer lifted
   from anthropic; rule-of-three refactor at N=3 plugins).
2. A `tau-plugin-conformance` parameterized behavioral test crate.

Both pre-existing plugins (anthropic, ollama) had been mapping every
non-2xx HTTP response to `LlmError::Internal { message }` — a
deliberate v0.1 simplification (deferred per spec §1 of both prior
sub-projects). At N=3 plugins, the cost of this is concrete:

- `tau_runtime` and tool callers cannot branch on retry-eligibility.
  `LlmError::is_retryable()` returns `true` for `RateLimited /
  Transport / Stream / Provider` and `false` for `Internal` — meaning
  every 429 / 5xx that should retry was being mis-classified as
  non-retryable, silently losing the signal.
- Callers that want to wait `Retry-After` seconds before the next
  call must string-grep `Internal.message`; the typed
  `RateLimited { retry_after_seconds: Option<u32> }` variant carries
  the integer structurally.
- Error envelope shapes differ across providers (Anthropic typed,
  OpenAI typed, Ollama bare-string), but consumers expect a uniform
  surface.

`tau_ports::LlmError` is `#[non_exhaustive]` and **already** had the
typed variants needed (`InvalidRequest`, `RateLimited`, `Auth`,
`Transport`, `Stream`, `Provider`). The work was a migration, not a
vocabulary expansion.

The conformance suite was deferred in ADR-0008 §17 explicitly until
two-or-more real plugins existed. Three now do; the parameterized
harness becomes high-leverage.

## Decision

### A. Typed-error mapping policy

All `tau` LLM-backend plugins MUST emit typed `LlmError` variants for
HTTP-mapped failures:

| Status | Typed variant |
|---|---|
| 400 | `LlmError::InvalidRequest { reason }` |
| 401, 403 | `LlmError::Auth { message }` |
| 404 | `LlmError::InvalidRequest { reason }` (with remediation embedded in `reason` when applicable) |
| 429 | `LlmError::RateLimited { retry_after_seconds: Option<u32> }` (parsed from `Retry-After` HTTP header) |
| 5xx (except 501) | `LlmError::Provider { message }` (retryable per `is_retryable()`) |
| Network / TLS / DNS | `LlmError::Transport { message }` |
| Mid-stream | `LlmError::Stream { message }` (only emitted from stream items) |

`map_response_error` MUST take the signature
`(status, headers, body)`. The `headers` parameter is required so 429
responses can populate `RateLimited.retry_after_seconds` from the
`Retry-After` HTTP header. Plugin `complete()` and `stream()` methods
MUST extract `headers` BEFORE consuming the response via
`Response::text()` (which moves the response).

`LlmError::Internal { message }` is RESERVED for plugin-internal
translation errors only (e.g., wrapping a `BuildError` from
`request.rs` when the typed-tau-domain → JSON conversion fails). It
MUST NOT be used for HTTP-mapped paths.

The `llmerror-internal` entry in
[`docs/explanation/escape-hatches.md`](../explanation/escape-hatches.md)
remains active. Its scope narrows post-migration — `Internal`
callsites now exist only at the BuildError/ParseError translation
boundary, not at the HTTP-mapping boundary. The mechanical CI test
at `crates/tau-domain/tests/escape_hatch_registry.rs` continues to
gate against accidental NEW `Internal` / `Custom` variants in
`tau-ports`. **No new such variants ship in this sub-project.**

`LlmError::ModelNotFound` (a typed variant we don't yet have) is OUT
of scope for v0.1: `InvalidRequest { reason }` with the remediation
hint embedded in `reason` is sufficient. A future sub-project may
promote it when a caller needs to branch on the case structurally.

### B. Conformance suite charter

A new crate `tau-plugin-conformance` provides
`ConformanceSuite::default().run(build_plugin, cassettes_dir).await`,
where `build_plugin: Fn(String) -> impl LlmBackend` returns a freshly
configured plugin pointed at the per-test cassette server's URL.

The catalog at v0.1 is FIXED at 6 baseline tests:

1. `batch_happy_path` — `complete()` returns Ok; non-empty `text`;
   valid `stop_reason`.
2. `batch_with_tools` — one `tool_use` with non-empty id+name and
   Object-typed input.
3. `streaming_text` — at least one `Text` chunk; exactly one terminal
   `Finish`.
4. `streaming_tool_use` — at least one `ToolUse` chunk before
   `Finish`; `tu.input` is Object.
5. `error_rate_limited` — 429-exhausting cassette → typed
   `LlmError::RateLimited`.
6. `error_auth` — 401 cassette → typed `LlmError::Auth`.

The catalog is conservative by design. The suite tests **mechanical
correctness**: request shape, response shape, stream chunk ordering,
error typing. The suite does NOT test:

- **Response quality** (does the model follow the instruction? is the
  answer right?). [Constitution NG7](../../CONSTITUTION.md) explicitly
  forbids tau evaluating quality.
- Specific response text or `stop_reason` values beyond "valid
  variant". Plugins differ in how they map provider-specific
  finish-reasons to `tau_ports::StopReason`; mandating exact strings
  would break that flexibility.
- Specific tool_use ids. Some plugins synthesize ids (Ollama); others
  preserve provider ids (Anthropic, OpenAI). The suite asserts only
  "id is non-empty".
- Specific token counts. Different providers tokenize differently;
  asserting numeric equality would fail spuriously.

Catalog extension requires a follow-up ADR-amendment. Adding tests is
non-trivial because each new test must hold for ALL plugins
simultaneously — a stricter contract than per-plugin tests.

Per-plugin cassettes live at
`crates/tau-plugins/<plugin>/tests/conformance-cassettes/<test_name>.yaml`,
with file names matching the test catalog. Each plugin's
`tests/conformance.rs` is a 5-15-line shim that wires its
`Configure::from_config` into the closure-based suite API.

The suite panics on the first assertion failure with a descriptive
message including the test name. The caller's `#[tokio::test]`
surface fails accordingly.

## Consequences

### Positive

- `tau_runtime`'s retry helper can use `LlmError::is_retryable()`
  honestly. Today every variant including `Internal` is
  non-retryable; post-migration, `RateLimited`, `Provider`,
  `Transport`, and `Stream` correctly mark transient failures as
  retryable.
- Future LLM-backend plugin authors get a behavioral test suite for
  free. Adding sub-project 2d (next provider) is a smaller surface
  because the conformance shim catches mechanical correctness errors
  before any plugin-specific tests run.
- The cassette replayer's centralization (~323 LOC) eliminates
  drift across plugins. Past sub-projects had to maintain three
  copies; future plugins import the shared crate.
- 17 → 21 required CI checks gating `main` (`build (openai-plugin)`,
  `build (tau-plugin-test-support)`,
  `build (tau-plugin-conformance)`, `test (conformance)`).

### Negative

- The PR shipping this ADR is large (~3000 LOC churn across 5+
  crates). Reviewers must trust the per-task subagent-driven
  workflow + per-plugin test runs.
- Conformance cassettes are duplicated across three plugins (each
  plugin needs its own cassette directory because wire formats
  differ). 18 cassette YAMLs total. A future cassette record-mode
  replayer could automate maintenance.
- `map_response_error`'s 3-arg signature `(status, headers, body)`
  is a MINOR breaking change for the in-tree plugins; external
  plugin authors who copied the 2-arg signature from earlier sub-
  projects will need to update. Acceptable: no external plugins
  exist yet at this phase.

### Neutral

- The `LlmError::Internal` variant remains in `tau-ports` (no
  vocabulary deprecation). Callsites narrow but the variant stays —
  there are still legitimate plugin-internal translation errors that
  warrant the `Internal` escape hatch.
- The conformance suite is `dev-dependency`-only; it adds no runtime
  surface to the runtime or any plugin binary.

### Obligations

- Future LLM-backend plugin authors MUST integrate with the
  conformance suite by adding `tests/conformance.rs` + a
  `tests/conformance-cassettes/` directory.
- Future plugin authors MUST NOT use `LlmError::Internal` for
  HTTP-mapped paths.

## Alternatives considered

### Alt A: Defer typed-error migration to a separate sub-project

Ship OpenAI with `Internal`-only mapping like 2a/2b; let a follow-up
sub-project handle the typed migration across all three plugins.

**Rejected:** The migration is mechanical (1-for-1 cassette assertion
updates) and bundling it with sub-project 2c amortizes the testing
overhead — the existing anthropic + ollama integration tests run
against the typed mappers as a regression check. Splitting would
require running the same tests twice (once with `Internal`, once
typed) for no architectural benefit.

### Alt B: Add `LlmError::ModelNotFound` typed variant

Promote 404 errors to a typed `ModelNotFound { model: String }`
variant.

**Rejected for v0.1:** Three providers each represent
"model not found" differently (OpenAI: `code: "model_not_found"`;
Ollama: `error.error: "model 'X' not found"`; Anthropic: `error.type:
"not_found_error"`). A typed variant would encode one of these
shapes; structurally embedding the model name is non-trivial because
not all providers return it consistently. `InvalidRequest { reason }`
with the model name embedded in `reason` is adequate at v0.1; revisit
when a real caller needs to branch on the case.

### Alt C: Conformance suite as a trait, not a closure

`pub trait PluginBuilder { type Plugin: LlmBackend; fn build(&self, base_url: String) -> Self::Plugin; }`
plus `ConformanceSuite::run<P: PluginBuilder>(&self, builder: &P, ...)`.

**Rejected:** Yet another trait to define and implement. The closure
signature `Fn(String) -> impl LlmBackend` is idiomatic Rust, requires
no boilerplate, and supports the same use cases. Per-plugin shims
become 5-15 lines instead of 30+.

### Alt D: Conformance catalog extends to response-quality tests

Add a "model-follows-instructions" test that asserts the response
text contains the requested keywords.

**Rejected (NG7):** [Constitution NG7](../../CONSTITUTION.md) forbids
tau evaluating model quality. Quality tests belong in downstream
projects (e.g., `stature`, the opinionated coding pipeline). The
conformance suite is mechanical correctness only; this boundary is
explicit in the catalog charter to prevent scope creep.
