# Tau roadmap

This document tracks current phase, near-term priorities, and
explicit out-of-scope items. Updated at phase transitions per PG1 and
PG4.

For per-issue tracking, see [GitHub
Issues](https://github.com/LEBOCQTitouan/tau/issues).

## Current phase: 1 — runnable runtime

**Goal:** make the Phase 0 stack actually runnable end-to-end. Plugin
loading mechanism, first real LLM-backend + tool plugins, capability
override, transitive dependency resolution. The first sub-project of
Phase 1 unblocks everything else.

**Status:** Phase 1 priority 3 (first real Tool plugins: fs-read +
shell) shipped 2026-04-30. Tier 1 fully complete: plugin loading
mechanism (priority 1), three real LLM-backend plugins (priority 2),
and two real Tool plugins (priority 3) with end-to-end capability
enforcement. **Tier 2 fully complete** as of 2026-05-01: priorities
4 (capability override), 5 (transitive dependency resolution), 6
(tool-args schema validation), 7 (tau update/verify/uninstall), and
8 (streaming LLM responses) all shipped, closing the ADR-0007 §4,
§5, §1, ADR-0006 §3, and ADR-0006 §5 reservations. Tier 3 (multi-
agent orchestration, workflow runner, REPL persistence, sandboxing)
is the natural next phase of work.

| # | Sub-project | Produces | Merged |
|---|---|---|---|
| 1 | Plugin loading mechanism ✅ | Out-of-process IPC over MessagePack-RPC; tau-plugin-protocol + tau-plugin-sdk crates; plugin_host module in tau-runtime; tau-pkg build-on-install; debug-tier subcommands; echo-llm + echo-tool toy plugins | 2026-04-28 |
| 2a | Anthropic LLM-backend plugin ✅ | First real LLM-backend plugin: Anthropic Claude Messages API client at `crates/tau-plugins/anthropic/`; day-1 streaming + tool-use; cassette-replay test harness + env-gated live smoke; in-plugin retry honoring Retry-After; ConfigError::InvalidEnvVar SDK amendment | 2026-04-29 |
| 2b | Ollama LLM-backend plugin ✅ | Second real LLM-backend plugin: Ollama (local LLM runner) at `crates/tau-plugins/ollama/`; native `/api/chat` over NDJSON streaming (~50 LOC hand-rolled, no eventsource-stream); optional bearer-token auth; cassette-replay test harness duplicated from Anthropic; in-plugin retry honoring 503-on-model-load case; 404 errors include `ollama pull` remediation hint | 2026-04-29 |
| 2c | OpenAI plugin + supporting infrastructure ✅ | Third real LLM-backend plugin: OpenAI Chat Completions client at `crates/tau-plugins/openai/`; SSE streaming, real `tool_call_id` round-trip, full `tool_choice` round-trip. Plus `crates/tau-plugin-test-support/` (rule-of-three refactor of cassette replayer) and `crates/tau-plugin-conformance/` (parameterized behavioral test suite, deferred from ADR-0008 §17). All 3 plugins migrated to typed `LlmError` variants. ADR-0009 Accepted. | 2026-04-29 |
| 3 | First real Tool plugins (fs-read + shell) ✅ | Two minimal Tool plugins demonstrating the kernel's capability check end-to-end. `fs-read` enforces `FsCapability::Read.paths` globs; `shell` enforces `ProcessCapability::Spawn.commands` allow-list (wall-clock timeout, 1 MiB output cap, kill+drain on timeout, no env inheritance, no stdin). Closed two infrastructure gaps in the same sub-project: `tool.describe_capabilities` wire method (Gap 1: plugin-declared capabilities now surface to the kernel for IPC tools); `SessionContext.granted_capabilities` (Gap 2: agent grants flow to plugin processes for finer-grained scope checks). Trust model: unsandboxed v0.1; sandboxing deferred to Tier 3 priority 12. | 2026-04-30 |
| 4 | Capability override implementation ✅ | Tier 2 priority 4 — realizes ADR-0007 §4 reservation. Project tau.toml `[[agents.<id>.capabilities]]` narrows but never expands package manifest grants. `tau-runtime::capability_override` module (semantic glob-subset analyzer + `compute_effective`); `RunOptions.project_override` flows from tau-cli through `Runtime::run`; `SessionContext.deny_entries` channel; `DenyEntry` type; fs-read + shell plugins honor deny-after-allow (deny wins per spec §9). Validation at parse time AND every runtime load (fail-closed both places). New `tau list agents --capabilities` audit surface. New typed errors `ProjectConfigError::CapabilityOverrideExpands` and `RuntimeError::CapabilityOverrideExpands`. Telemetry event `runtime.capability_override_rejected`. No new CI jobs (23 required checks unchanged). | 2026-04-30 |
| 5 | Transitive dependency resolution ✅ | Tier 2 priority 5 — realizes ADR-0007 §5 reservation. New `tau-pkg::source_list` (git ls-remote tag enumeration + rev-pinned shallow read) and `tau-pkg::resolve` (three-phase resolver: group / conflict / pick highest-compatible). New `tau resolve` subcommand. Schema upgrade: `[[agents.<id>.requires.tools]]` typed entries with `name + source + version`; bare strings rejected at parse. Lazy resolve at `tau run`/`tau chat` with `--no-install` opt-out emitting copy-pasteable install hints. npm-style progress output (one line per phase, JSON event stream). New typed `ResolveError`, `SourceListError`, `RequiresToolsBareStringRejected`. Tests use `file://` git fixtures — no real network in CI. No new CI jobs (23 required checks unchanged). | 2026-04-30 |
| 6 | Tool-args schema validation ✅ | Tier 2 priority 6 — realizes ADR-0006 §3 deferral closure. New `tau-runtime::tool_args` module with `ToolArgsValidator` (Draft 7 via `jsonschema` crate). Schemas pre-compile at `RuntimeBuilder::build()`; malformed → `BuildError::ToolSchemaInvalid` (terminates build before any LLM round-trip). Runtime arg-validation failures surface as `ToolError::BadArgs` with MANDATORY template (original args + full schema + specific issue) so the LLM self-corrects via the conversation. Loop survives validation errors; only real plugin invocation crashes still terminate. New ADR-0010. No new CI jobs (23 required checks unchanged). | 2026-04-30 |
| 7 | tau update / verify / uninstall ✅ | Tier 2 priority 7 — closes ADR-0007 §1 deferral. New tau_pkg::tree_hash module (walkdir + sha2; excludes .git/, target/, *.tau-tmp/; symlinks contribute target bytes). New tau_pkg::verify module returning structured VerifyReport (Ok / TreeDrift / BinaryDrift / Missing / Unverified). New tau_pkg::update_package library function composing existing source_list + resolver + install + uninstall. Three CLI subcommands: tau update (default latest tag, --version pin, --prune), tau verify (exit 0/2, --json), tau uninstall (permissive + remediation hint). Lockfile schema v2 → v3 additive (LockedPlugin.binary_sha256 field; v2-leftover entries flagged unverified, not drift). Existing tau_pkg::uninstall library function reused unchanged. New ADR-0012. No new CI jobs (23 required checks unchanged). | 2026-05-01 |
| 8 | Streaming LLM responses ✅ | Tier 2 priority 8 — realizes ADR-0006 §5 deferral closure. New `tau-runtime::stream` module with `RunEvent` enum + `run_streaming_inner` async generator (via `async-stream` crate). Kernel pump translates `CompletionChunk` into higher-level `RunEvent`s (`TextDelta`, `ToolCallStarted`, `ToolCallCompleted`, `TurnCompleted`, `RunCompleted`, `FatalError`). `Runtime::run_streaming` + `run_streaming_with_history` public entry points return `Result<impl Stream + 'static, RuntimeError>`. `Runtime::run`/`run_with_history` REFACTOR as thin stream-drainers (zero behavior change; 100+ existing tests pass unchanged). New `RunEvent::FatalError` variant (with `tool_error_variant` tag) preserves byte-identical batch-API error reconstruction for typed `RuntimeError::*` variants (plan-erratum revision documented in ADR-0011 decision 2). `tau chat` streams by default (`--no-stream` opt-out, two-pass termimad render); `tau run --stream` opt-in flag (human + JSON modes; canonical 5-event JSON shape per spec §4.6). New ADR-0011. No new CI jobs (23 required checks unchanged). | 2026-04-30 |

## Phase 0 (complete) — bootstrap + foundational sub-projects

**Goal:** empty repo with green CI, full governance files, and the
hexagonal workspace skeleton in place; then five foundational
sub-projects (tau-domain, tau-ports, tau-pkg, tau-runtime, tau-cli)
producing working, testable software on its own per the
brainstorm→spec→plan→implementation cycle.

**Outcome:** all sub-projects shipped on schedule (2026-04-24 →
2026-04-28). 6 ADRs Accepted. 464 workspace tests passing. 12 required
CI status checks gating `main`. Hexagonal architecture realized across
the 5-crate runtime surface (`tau-domain`, `tau-ports`, `tau-pkg`,
`tau-runtime`, `tau-cli`); 3 stub crates (`tau-app`, `tau-infra`,
`tau-observe`) reserved for Phase 1+ work.

**Material v0.1 limitation:** plugin loading is deferred to Phase 1+
per ADR-0007 §18. `tau install` records source trees; the loader lands
in Phase 1.

| # | Sub-project | Produces | Merged |
|---|---|---|---|
| 0 | Repo bootstrap | Empty workspace + governance + CI | 2026-04-24 |
| 1 | `tau-domain` Message + Agent + Package types ✅ | Pure-types crate with `thiserror` errors, doc tests, proptest for parsers | 2026-04-25 |
| 2 | `tau-ports` plugin traits ✅ | Trait definitions for LLM backend, tool, storage, sandbox | 2026-04-26 |
| 3 | `tau-pkg` package manager ✅ | `tau install` from git URLs, capability declarations parsed (G14), scope resolution (G8) | 2026-04-27 |
| 4 | `tau-runtime` agent lifecycle + message passing ✅ | Spawn an agent, deliver messages, observe via structured logs (solo path only) | 2026-04-28 |
| 5 | `tau-cli` real subcommands ✅ | `tau install`, `tau run`, `tau ls`, `tau init`, `tau chat` | 2026-04-28 |

Phase 0 retrospective: [`docs/retrospectives/phase-0.md`](docs/retrospectives/phase-0.md).

## Phase 1 priorities

Detailed motivation per priority is in
[`docs/retrospectives/phase-0.md` §7](docs/retrospectives/phase-0.md).
Tier ordering reflects criticality, not strict implementation order
(some Tier 2/3 items can run in parallel with later Tier 1 items).

### Tier 1 — unblocks Phase 1 itself

1. **Plugin loading mechanism.** ✅ Shipped 2026-04-28 — see
   [ADR-0008](docs/decisions/0008-plugin-loading.md). Out-of-process
   IPC over MessagePack-RPC + tau-pkg/tau-runtime/tau-domain
   amendments. 15 required CI checks gating `main` (was 12 in Phase
   0).
2. **First real LLM-backend plugin.** ✅ Tier 1 priority 2 fully
   complete: Anthropic shipped 2026-04-29 as priority 2a; Ollama
   shipped 2026-04-29 as priority 2b; OpenAI shipped 2026-04-29 as
   priority 2c — closing out Tier 1 priority 2 with the rule-of-three
   refactor (`tau-plugin-test-support`) and the deferred conformance
   suite (`tau-plugin-conformance`). All three plugins migrated to
   typed `LlmError` variants. ADR-0009 (typed-error migration policy
   + conformance suite charter) Accepted. 21 required CI checks
   gating `main` (was 17).
3. **First real Tool plugins.** ✅ `fs-read` + `shell` shipped
   2026-04-30 as priority 3 — exercises capability checks at runtime
   end-to-end. Closed two IPC infrastructure gaps in the same sub-
   project: kernel-side capability enforcement for IPC tools (Gap 1
   via new `tool.describe_capabilities` wire method) and agent-grant
   flow to plugin processes (Gap 2 via additive
   `SessionContext.granted_capabilities`). 23 required CI checks
   gating `main` (was 21).

### Tier 2 — completes Phase 0 deferrals

4. **Capability override implementation** ✅ Shipped 2026-04-30 — see
   [spec](docs/superpowers/specs/2026-04-30-capability-override-design.md).
   Realizes ADR-0007 §4 reservation. Project tau.toml
   `[[agents.<id>.capabilities]]` narrows package grants via
   semantic glob-subset on `allow_*` plus `deny_*` carve-outs (deny
   wins). Validation at parse time + every runtime load (fail-closed
   both places). Audit surface: `tau list agents --capabilities`.
5. **Transitive dependency resolution** ✅ Shipped 2026-04-30 — see
   [spec](docs/superpowers/specs/2026-04-30-transitive-deps-design.md).
   Realizes ADR-0007 §5 reservation. Project tau.toml
   `[[agents.<id>.requires.tools]]` declares typed dependencies
   (`name + source + optional version constraint`); `tau run`/`tau chat`
   auto-install missing entries via lazy resolve; new `tau resolve`
   subcommand serves project-wide install. Cargo-style semver
   intersection across declarations of the same tool. One level deep:
   recursive package-level `dependencies` (ADR-0004 §10) stays
   deferred. No new CI jobs (23 required checks unchanged).
6. **Schema validation for tool args** ✅ Shipped 2026-04-30 — see
   [spec](docs/superpowers/specs/2026-04-30-tool-args-schema-design.md)
   and [ADR-0010](docs/decisions/0010-tool-args-schema-validation.md).
   New `tau-runtime::tool_args` module validates every tool-call's
   args against the tool's declared `ToolSpec.input_schema` (Draft 7
   via `jsonschema` crate). Schemas pre-compile at
   `RuntimeBuilder::build()`; malformed → `BuildError::ToolSchemaInvalid`
   before any LLM round-trip. Runtime arg-validation failures surface
   as `ToolError::BadArgs` with MANDATORY template (original args +
   full schema + specific issue) so the LLM self-corrects via the
   conversation. `RuntimeError::PluginContractViolation` stays
   reserved for a future out-of-process plugin handshake-lying
   trigger path. No new CI jobs (23 required checks unchanged).
7. **`tau update` / `tau verify` / `tau uninstall` subcommands** ✅ Shipped 2026-05-01 — see
   [spec](docs/superpowers/specs/2026-05-01-tau-lifecycle-design.md)
   and [ADR-0012](docs/decisions/0012-tau-lifecycle-commands.md).
   New `tau_pkg::tree_hash`, `verify`, `update_package` modules.
   Whole-tree SHA-256 verify is source-agnostic (anticipates future
   `PackageSource` variants). `tau update <pkg>` defaults to latest
   tag; `--version` to pin; `--prune` opt-in. `tau uninstall` is
   permissive with a remediation hint pointing to project tau.toml's
   `[[requires.tools]]` entries. Lockfile schema v2 → v3 (additive:
   `LockedPlugin.binary_sha256`). Existing `tau_pkg::uninstall`
   library function reused unchanged. No new CI jobs (23 required
   checks unchanged).
8. **Streaming LLM responses** ✅ Shipped 2026-04-30 — see
   [spec](docs/superpowers/specs/2026-04-30-streaming-design.md)
   and [ADR-0011](docs/decisions/0011-streaming-llm-responses.md).
   New `Runtime::run_streaming` and `run_streaming_with_history`
   yield a `Stream<Item = RunEvent> + 'static` as the agent loop
   progresses. Existing `run`/`run_with_history` REFACTOR as thin
   stream-drainers (zero behavior change for batch callers; one
   source of truth for the agent loop). New `RunEvent::FatalError`
   variant preserves byte-identical batch-API error semantics
   (LLM, Tool::*, ToolNotRegistered errors round-trip via
   `tool_error_variant` tagging — see ADR-0011 decision 2). `tau
   chat` streams by default with two-pass termimad rendering
   (`--no-stream` opt-out); `tau run --stream` opt-in flag (human
   + JSON modes; canonical 5-event JSON shape per spec §4.6). No
   new CI jobs (23 required checks unchanged).

### Tier 3 — extends the runtime

9. **Multi-agent orchestration** (G10's deferred half).
10. **Workflow / pipeline runner** (deterministic step-by-step
    pipelines; possibly new `tau-workflow` crate).
11. **REPL persistence** (`tau chat --resume <id>`).
12. **Sandboxing implementation** (Constitution G12).

### Tier 4 — operational quality

13. **Performance budgets enforced in CI** (Constitution QG14, G16).
14. **`cargo audit` + `cargo-deny` in CI** (Constitution QG16).
15. **Serve mode** (JSON-RPC over stdio; Constitution G6, QG12). Lives
    in `tau-app`.

## Out of scope (forever)

These are tau's explicit non-goals from
[`CONSTITUTION.md` §2](CONSTITUTION.md). They will not be added to
core regardless of demand:

- **NG1.** Tau is not an LLM or an agent.
- **NG2.** Tau is not a coding-specific tool.
- **NG3.** Tau is not a hosted service.
- **NG4.** Tau is not a package marketplace.
- **NG5.** Tau is not a general-purpose workflow engine.
- **NG6.** Tau does not provide persistent agent memory in core.
- **NG7.** Tau does not evaluate agent quality.
- **NG8.** Tau is not an AI safety harness.
- **NG9.** Tau does not manage identity, authentication, or
  credentials.
- **NG10.** Tau does not collect telemetry or training data.
- **NG11.** Tau is a developer tool, not an end-user tool.
- **NG12.** Tau is a runtime, not a framework.

Adjacent ideas may belong in plugins or downstream projects (such as
`stature`, the opinionated coding pipeline planned as a separate
project).
