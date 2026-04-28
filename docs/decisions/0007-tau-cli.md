# ADR-0007: tau-cli + tau-runtime amendments (capability filter, run_with_history)

**Status:** Proposed
**Date:** 2026-04-28
**Supersedes:** —
**Amends:** ADR-0006 §7 and §11 (refines typed-capability story by
pre-filtering tools per-call; adds `Runtime::run_with_history` and the
`history_len` span field as additive vocabulary).
**Refines:** ADR-0002 (project `tau.toml` shares filename with package
`tau.toml`; the two are distinct schemas keyed by their root tables).

## Context

tau-cli (sub-project 5, ROADMAP row 5) is the CLI binary surface for tau
Phase 0 — the `tau` executable. It is the first sub-project producing a
binary rather than a library, and it is the consumer that wires
tau-runtime, tau-pkg, tau-domain, and the tau-ports plugin traits into a
user-facing tool. Per ROADMAP row 5 the v0.1 scope is the solo path;
multi-agent orchestration (G10) is Phase 1+.

Per QG18, public API additions (the CLI subcommand surface, the project
`tau.toml` schema) and additive plugin / runtime amendments require
ADRs. This ADR bundles 18 tau-cli decisions plus 2 additive tau-runtime
amendments per the ADR-0006 precedent (kernel + `Tool::capabilities()`
amendment recorded together because the amendment was solely motivated
by the kernel). Both amendments here are solely motivated by tau-cli —
specifically by the capability-aware tool dispatch path and the REPL's
need to thread conversation history across user turns. Bundling avoids
splitting one motivated change across multiple ADRs; future tau-runtime
amendments motivated by their own sub-projects will get their own ADRs
(no promiscuous bundling).

Relevant Constitution constraints: G6 (tau is a runtime, not a
framework — the CLI is a thin shell), G9 (observable by default), G10
(solo path; orchestration Phase 1+), NG6 (no persistent agent memory
in core), NG9 (tau does not redact for the caller and does not manage
credentials), NG11 (developer tool, not end-user product), NG12 (runtime
not framework), QG2 (`thiserror` everywhere), QG18 (ADRs for public API
additions). This is the last sub-project before the formal Phase 0
retrospective (per PG4); decisions made here close out the Phase 0
public surface.

## Decision

### 1. 5-subcommand surface at v0.1

`install`, `list`, `run`, `init`, `chat`. ROADMAP row 5 enumerates
`install`, `list`, `run`. `init` is added per ADR-0004 §6 (the
project-scaffolding verb that creates a starter `tau.toml` plus a stub
agent). `chat` is added per the brainstorm's option-A REPL choice (see
decision 6). Deferred to Phase 1+: `uninstall`, `update`, `verify`,
`workflow` and any orchestration verbs. The deferred list is
documented; v0.1 hard-rejects unknown subcommands rather than silently
succeeding.

Trigger to revisit: a real user workflow demanding any deferred verb.

### 2. `#[tokio::main]` async at the entry

The CLI binary's `main` is `#[tokio::main(flavor = "current_thread")]`.
`Runtime::run` and `Runtime::run_with_history` are async (per ADR-0006
§2); sync tau-pkg calls run inline. tokio is in dev-deps for tau-runtime
tests already; the CLI as the consumer picks it. `Runtime::run` itself
remains async-runtime-agnostic per ADR-0006 §2 — only the binary
commits. The current-thread flavor is sufficient because v0.1 has no
internal concurrency in the CLI (no `tokio::spawn`, no parallel
subcommand execution); a future multi-agent orchestration step in Phase
1+ may upgrade to the multi-thread flavor.

Trigger to revisit: a use case where async forces unwanted complexity
on a small embedding (the runtime stance is unchanged; this decision
only binds the binary).

### 3. Project `tau.toml` named-table schema

A project's `tau.toml` has `[project]` and `[agents.<id>]` named tables
(Cargo's `[dependencies.foo]` style) with optional sub-tables
`requires`, `capabilities` (reserved — see decision 4), `config`,
`prompt`. The project file is distinct from the package `tau.toml`
defined by ADR-0002: the two share a filename modeled on Cargo's
`[package]` (in a crate's `Cargo.toml`) vs `[workspace]` (in a
workspace root) precedent — the root table tells the parser which
schema applies. tau-cli's `config::read_project` rejects files that
match neither schema with a typed `ProjectConfigError::AmbiguousFile`.

Named tables (`[agents.foo]`) are chosen over array-of-tables
(`[[agents]]` with an `id` field) because the named-table form makes
the agent id visually obvious in the section header and matches
Cargo's map-by-name idiom for the same role.

Trigger to revisit: confusion between project and package files in
real user reports — at which point the file extension or name should
be split.

### 4. Capability override is a Phase 1+ requirement

The `[agents.<id>.capabilities]` table is reserved at v0.1: the schema
slot is parsed; presence triggers
`ProjectConfigError::CapabilityOverrideUnsupported` pointing at the
Phase 1+ roadmap. Intersect-only semantics are committed now: a Phase 1
override can narrow but never expand the capabilities granted by the
package manifest. Documenting the semantic at v0.1 locks Phase 1 into
the safe direction — without this, Phase 1 could quietly choose
expand-and-narrow (which would let a project widen what an installed
package was authorized to do, breaking the install-time security
contract).

Trigger to revisit: Phase 1+ when a real user case demands per-project
capability narrowing — at which point the implementation lands behind
the same schema slot already reserved.

### 5. Per-agent `requires.tools` advisory check

Each `[agents.<id>.requires.tools]` is a list of tool package names (or
`name@semver`) the agent expects to be installed. `tau run` and
`tau chat` check each entry against the local registry; any missing
entry yields a clear error and exit code 2 (kernel/CLI broke — see
decision 7). At v0.1 this is advisory only: tau-cli does NOT auto-
install missing dependencies. Phase 1+ activates auto-install via
tau-pkg's transitive-resolution work.

Trigger to revisit: Phase 1's transitive-resolution work, at which
point this hook becomes the auto-install entry point.

### 6. One-shot `tau run` + separate `tau chat` REPL

Two distinct verbs, two mental models. `tau run <id> "<prompt>"` is
strict 1:1 with `Runtime::run` — single prompt in, single outcome out,
scriptable, predictable, no detection logic. `tau chat <id>` opens a
REPL using `rustyline` (line editor) and `termimad` (Markdown rendering
to ANSI), with in-memory-only history and a small slash-command surface
(see decision 11). Overloading one verb (e.g., "if no prompt and stdin
is a TTY, enter REPL") was rejected because CI scripts that forget a
prompt argument would accidentally hang in a REPL; explicit verbs
prevent the surprise.

Trigger to revisit: a workflow that genuinely needs both modes from
the same verb.

### 7. Three-bucket exit codes (0 / 1 / 2)

The Outcome / Error dichotomy from ADR-0006 §9 maps cleanly to three
exit codes:

- `0` — success: `tau install/list/init` succeeded, or `tau run`
  produced `RunOutcome::Completed`.
- `1` — `tau run` graceful failure: `RunOutcome::Failed` (any
  `FailureKind`). Reserved exclusively for `tau run`.
- `2` — kernel/CLI broke: `Err(RuntimeError)`, parse errors, missing
  dependencies, unknown subcommand, IO errors.

Three buckets give CI scripts the most common branching predicate
(agent ran but failed vs. tau itself broke) without coupling exit codes
to `FailureKind` variants. Finer-grained machine-readable detail comes
through `--json` (see decision 8).

Trigger to revisit: CI usage that needs finer granularity than `--json`
provides.

### 8. Strict stdout/stderr split + `--json` mode

Output discipline lives in the `Output` abstraction per spec §3.6:

- `stdout` carries scriptable content only — agent text, list rows,
  JSON. Nothing else. Pipes (`tau run … | jq`, `tau list | wc -l`)
  must work.
- `stderr` carries everything else — status messages, tracing output,
  errors, warnings. Tracing always goes to stderr regardless of mode.

`--json` switches stdout from human-formatted output to a
schema-versioned JSON envelope per subcommand. `tau chat` rejects
`--json` at argument parse time (REPL is interactive by definition).
Insta JSON snapshots assert the envelopes don't drift.

Trigger to revisit: a subcommand whose output convention legitimately
breaks the split (none anticipated at v0.1).

### 9. Verbosity model

Standard env_logger / tracing-subscriber conventions:

- Default: `INFO` scoped to `tau=*` (tau crates only — third-party
  noise stays at `WARN`).
- `-v` → `DEBUG` (still scoped to `tau=*`).
- `-vv` → `TRACE`.
- `-q` → `WARN`.
- `--debug` → `DEBUG` AND switches the top-level error printer from
  `Display` to `Debug` (see decision 12) — the single "everything"
  knob.
- `RUST_LOG` env var, when present, overrides verbatim and ignores
  `-v/-vv/-q`.

Trigger to revisit: production deployments demanding JSON-formatted
tracing output (additive — a `--log-format json` flag at the binary
level).

### 10. Color via `is-terminal` + `--color` + `NO_COLOR`

Standard CLI conventions, implemented exactly once in `Output`:

- Auto-detect: color enabled when stdout is a terminal (`is-terminal`).
- `NO_COLOR` env var (any non-empty value): force-disable. Universal
  opt-out per the no-color.org convention.
- `--color={auto,always,never}`: explicit override beats auto-detect.

Trigger to revisit: terminals that lie about `IS_TERMINAL` and need a
workaround switch.

### 11. REPL: rustyline + termimad, in-memory-only history, 4 slash commands

`tau chat` uses:

- `rustyline` for line editing (arrow-key recall, emacs/vi mode,
  Ctrl-C / Ctrl-D handling).
- `termimad` for rendering agent Markdown responses to ANSI.
- An in-memory `Vec<Message>` for conversation history, threaded into
  each `Runtime::run_with_history` call (see decision 15). The history
  is dropped on exit. This aligns with NG6 (no persistent agent memory
  in core); REPL history is a CLI-side concern, not a runtime
  responsibility.
- Four slash commands: `/exit` (quit), `/help` (show commands),
  `/clear` (drop in-memory history), `/history` (print messages so
  far). `/save`, `/load`, `/system`, `/model` etc. are deferred to
  Phase 1+.

Trigger to revisit: a Phase 1+ feature demanding history persistence
or a richer slash surface.

### 12. Top-level error message by default; `--debug` expands to source chain

tau-cli uses `anyhow` only at the binary boundary (each subcommand
returns `Result<(), anyhow::Error>` to a single top-level handler).
The handler prints `eprintln!("error: {err}")` by default — anyhow's
`Display` impl, which shows only the top-level message. With
`--debug`, the handler switches to `eprintln!("error: {err:?}")` —
anyhow's `Debug` impl, which prints the full source chain. This
matches Cargo / rustc convention. Internal subcommand code uses
`thiserror` (per QG2); `anyhow` lives only at the boundary.

Trigger to revisit: rare cases where the chain is needed by default
(none anticipated).

### 13. `--dry-run` on install/run/init/chat (not list)

`tau list` is read-only — `--dry-run` is meaningless and is rejected
at argument parse time with a clear error. The other four have
meaningful "would do X" semantics:

- `tau install --dry-run`: clone + parse + validate, no registry
  write.
- `tau run --dry-run`: build the `AgentDefinition`, validate
  `requires.tools`, but do NOT call the LLM.
- `tau init --dry-run`: print the files that would be written, do
  NOT touch disk.
- `tau chat --dry-run`: same validation as `tau run --dry-run`, then
  exit (do not enter the REPL).

Stderr lines from dry-run mode are prefixed with `[dry-run]` for grep-
ability.

Trigger to revisit: a future read-only subcommand whose `--dry-run`
could surface validation issues (at which point the rejection becomes
selective).

### 14. tau-runtime amendment 1: capability-filtered tools

This refines ADR-0006 §7's typed-capability story. Today the kernel
checks tool invocations against the agent's manifest and denies
mismatches with `FailureKind::PolicyDenied`. The amendment adds a
pre-filter step before `LlmBackend::complete`: the kernel computes
which registered tools are satisfied by the agent's package manifest
and only those tools are sent in `CompletionRequest.tools`. Effect:

- Smaller LLM prompts (no point telling the model about a tool it
  can't call).
- No spurious denials when the model picks a denied tool just because
  the runtime mentioned it.

Implementation: two new tracing events join the §3.9 vocabulary as
additive (non-breaking per ADR-0006 §17):

- `runtime.tool_filtered` at WARN, one per filtered tool, with fields
  `tool_name`, `agent_id`, `denial_kind`, `denial_detail`.
- `runtime.tools_filtered` at DEBUG, one per turn, with summary
  counts (`requested`, `granted`, `filtered`).

The defense-in-depth invoke-time check from ADR-0006 §7 is unchanged —
if a tool somehow bypasses the filter (e.g., a future direct-dispatch
path), the runtime still denies it. No new error variants per decision
17 below.

Trigger to revisit: future dynamic-capability tools where per-call
grants might change between filter time and invoke time.

### 15. tau-runtime amendment 2: `Runtime::run_with_history`

`tau chat` needs to thread conversation history across user turns. The
kernel exposes:

```rust
pub async fn run_with_history(
    &self,
    agent_def: &AgentDefinition,
    manifest: &PackageManifest,
    history: Vec<Message>,
    initial_message: Message,
    options: RunOptions,
) -> Result<RunOutcome, RuntimeError>
```

`Runtime::run` becomes a thin wrapper that calls `run_with_history`
with `history = Vec::new()`. Existing callers (sub-project 4's tests,
tau-cli's `cmd::run`) are unchanged — the wrapper preserves the v0.1
signature.

The `runtime.agent_run` span gains a `history_len: usize` field —
additive per ADR-0006 §17.

This aligns with NG6: history is a CLI-side concern threaded into each
kernel call. The kernel doesn't persist anything between
`run_with_history` invocations; it's the REPL's job to carry the
returned `all_messages` back into the next call.

Trigger to revisit: history-aware features at the kernel level
(summarization, sliding windows, automatic compaction) — at which
point a richer `RunOptions` field or a dedicated session API lands.

### 16. `AgentDefinition` construction lives in tau-cli

`AgentDefinition` is a tau-domain data type, but the logic to
construct one from a project `tau.toml` (resolve a package by
`name@semver`, resolve the `llm_backend` reference, read the prompt
from `[agents.<id>.prompt.path]` or `[agents.<id>.prompt.inline]`,
validate `requires.tools`) is tau-cli's responsibility. tau-cli's
`config::build_agent_definition` is the canonical builder for the
project-`tau.toml` case. Other consumers — Phase 1+ workflow runners,
embedded SDKs reading from a different config — write their own
builders matching their config formats.

This keeps tau-domain free of CLI / config concerns and tau-runtime
free of file-format concerns; the dependency direction stays crisp.

Trigger to revisit: a second consumer (Phase 1+ workflow runner, IDE
plugin, library SDK) that wants the same logic — at which point the
builder extracts to a shared crate.

### 17. NO new error variants without triggering codepaths

Per the phase-0-mid memo. Both tau-runtime amendments (§14, §15) and
tau-cli's own error taxonomies (`ProjectConfigError`,
`AgentResolutionError`, `CliError`) add only variants with concrete
reachable failure paths backed by ≥1 test. The capability-filter
amendment (§14) is silent-but-traced: no new error variants — a
filtered tool simply doesn't appear in `CompletionRequest.tools`.
`run_with_history` (§15) reuses the existing `RuntimeError` variants.

This codifies the discipline that bit ADR-0006 (the
`PluginContractViolation` and `Sandbox(_)` variants are wired but
trigger-pathless at v0.1). Going forward, any new variant ships with
its trigger path or doesn't ship.

Trigger to revisit: never — this is a process discipline that should
always hold.

### 18. Plugin loading deferred to Phase 1+

tau-pkg installs packages as source trees on disk, but tau-cli at v0.1
has no mechanism to compile and dynamically load them as plugins. Real
loading (in-process dlopen via `abi_stable`, out-of-process IPC,
WASM/WASI) is a major design decision that benefits from Phase 1+
requirements before being committed. v0.1 ships compiled-in mock
plugins gated by `cfg(feature = "test-mock")` for integration testing
end-to-end. `tau install` registers source trees in the registry, but
those trees do not execute against `tau run` / `tau chat` until Phase
1's loader lands.

This is a material v0.1 limitation and is surfaced in `tau install`'s
success message (a hint pointing at the Phase 1+ loader ADR slot).

Trigger to revisit: Phase 1+ ADR for the plugin-loading mechanism.

## Consequences

### Positive

- v0.1 ships a 5-subcommand CLI with 88 unit + 64 integration + 12
  snapshot tests covering every public surface and the agent run loop
  end-to-end (against the `test-mock` backend).
- Three-bucket exit codes give CI scripts a clean branch on the
  Outcome / Error dichotomy without leaking `FailureKind` into the
  exit-code surface.
- `--json` mode plus insta JSON snapshots make schema drift impossible
  to ship accidentally — every JSON envelope has a snapshot test.
- Project `tau.toml` schema lays the foundation for Phase 1+ workflow
  / pipeline definitions; the `[project]` and `[agents.<id>]` tables
  give those features a natural home.
- tau-runtime amendments are additive — every existing caller
  (sub-project 4's tests, the tau-cli `cmd::run` path) is unchanged.
- The REPL provides the conversation-threading UX that one-shot
  `tau run` can't, without compromising scriptability — `tau run`
  stays strict 1:1 with `Runtime::run`.
- `AgentDefinition` construction in tau-cli keeps tau-domain config-
  free and lets future consumers (workflow runners, SDKs) build
  agents from their own config formats without inheriting tau-cli's.

### Negative

- Plugin loading deferred means v0.1 can install package source trees
  but cannot run user-supplied plugins. The compiled-in `test-mock`
  backend keeps integration tests viable but is not a production
  solution. Decision 18 makes the limitation explicit; users see a
  hint at install time.
- Project `tau.toml` shares a filename with package `tau.toml`. This
  is modeled on Cargo's `[package]` vs `[workspace]` precedent but is
  still a source of confusion for newcomers. Documented in decision 3
  and surfaced via `ProjectConfigError::AmbiguousFile`.
- Capability override schema slot reserved but rejected at v0.1 —
  Phase 1 must implement intersect semantics or accept a deprecation
  cycle if it picks differently.
- REPL UX is minimal at v0.1: terminal-mode features (arrow-key
  recall verification, color rendering correctness, terminal resize
  handling) are not covered by automated tests; they rely on
  rustyline + termimad's defaults and manual smoke testing.
- `#[tokio::main]` couples the CLI binary to tokio. Documented in
  decision 2; replaceable but only tau-cli is affected — the runtime
  stays async-runtime-agnostic.
- The `tau install` source-tree-only behavior means a successful
  install does not imply a runnable agent at v0.1; the hint at install
  time mitigates but does not eliminate the surprise.

### Neutral / new obligations

- Future tau-cli public API additions (new subcommands, new project
  `tau.toml` schema fields, new `--flags`) require their own ADRs
  (QG18). The CLI surface is now public.
- The bundled tau-runtime amendments mean future tau-runtime changes
  motivated by their own sub-project (or by a library consumer that
  is not tau-cli) get their own ADRs (don't bundle promiscuously).
- Mid-implementation additive items in this sub-project (the `Scope`
  / `PackageSource` / `Capability` API findings recorded during Task
  9 + Task 11) are documented in the relevant commit bodies, not in
  this ADR — they are tau-pkg internal API surfaces that don't change
  Phase-0 semantics.
- The `runtime.tool_filtered` and `runtime.tools_filtered` tracing
  events join the ADR-0006 §3.9 (tracing) vocabulary as additive
  (non-breaking per ADR-0006 §17). Future renames or removals require
  an ADR.
- The `history_len` field on the `runtime.agent_run` span is
  similarly additive vocabulary.
- tau-cli's `lib.rs` exposes internal modules for integration testing.
  This is NOT considered a public API in the QG18 sense — tau-cli is
  a binary, not a library consumed by external code. The crate name
  is published only so the test binary can link against the same
  module tree as the binary.

## Alternatives considered

### A. Sync `tau-cli` with `block_on` at every async boundary

Rejected. Forcing sync entry would require
`tokio::runtime::Handle::current().block_on(...)` at each call site,
which fails outside a running runtime. tokio is in the dependency
graph regardless once real LLM plugins land (the canonical
`reqwest`-based clients are tokio-native). See decision 2.

### B. Overload `tau run` with prompt-vs-no-prompt detection for REPL

Rejected. Entering a REPL on missing prompt + isatty detection is
surprising — CI scripts that forget to pass a prompt accidentally
enter a REPL and hang. A separate `tau chat` verb is cleaner and
gives the REPL room to grow its own surface (slash commands, future
history persistence) without polluting `tau run`. See decision 6.

### C. Two-bucket exit codes (Ok vs not-Ok)

Rejected. CI scripts need to distinguish "agent ran but failed" from
"kernel broke" — the most common branching predicate in agent
pipelines. Three buckets give that distinction without coupling exit
codes to `FailureKind`. See decision 7.

### D. `[[agents]]` array-of-tables instead of `[agents.<id>]` named tables

Rejected. Array-of-tables is visually flat and requires a separate
`id` field; named tables make the scope visually obvious (the section
header IS the id) and match Cargo's `[dependencies.foo]` for the same
map-by-name role. See decision 3.

### E. `tau init` interactively prompts for name + first agent details

Rejected. Adds prompt-handling complexity for marginal UX value at
v0.1. The stub agent gives the user a concrete starting point to edit
and is friendlier to scripted use (`tau init <name>` doesn't hang
waiting for input). A future `tau init --interactive` is a natural
Phase 1+ addition.

### F. Real plugin loading at v0.1 (dlopen / abi_stable)

Rejected. Plugin-loading mechanism choice (in-process dynamic linking
via `abi_stable`, out-of-process IPC, WASI) is a major design decision
that benefits from real Phase 1+ requirements before being committed.
v0.1's compiled-in `test-mock` backend keeps integration tests viable
without locking in a loading approach. See decision 18.

### G. Persisted REPL session history at v0.1

Rejected. Persistence raises "where does the file live", "garbage
collection", "schema versioning", "encryption-at-rest" questions that
don't have answers at v0.1 — and persistence interacts with NG6 (no
persistent agent memory in core) in ways that need their own design
pass. In-memory-only is the simplest correct option; persistence is
additive when a real use case appears.

### H. New `Runtime::run` signature instead of `run_with_history` + wrapper

Rejected. Breaking the existing `Runtime::run` signature would force
every existing caller (sub-project 4's tests + tau-cli's `cmd::run`)
to update for a feature only `tau chat` uses today. Additive
`run_with_history` plus a thin `Runtime::run` wrapper preserves
source compatibility for every existing caller and isolates the
history-threading complexity to the one consumer that needs it. See
decision 15.

### I. `--json` mode on `tau chat`

Rejected. `tau chat` is interactive — JSON output during a REPL would
confuse both the user (they want rendered Markdown) and any
hypothetical script (a script wanting JSON should use `tau run --json`
in a loop). Hard-rejecting `--json` on `tau chat` at argument parse
time is clearer than silently downgrading to plain text. See decision
8.

### J. Per-`FailureKind` exit-code sub-codes (e.g., 10 = PolicyDenied, 11 = OutOfResources)

Rejected. Coupling exit codes to `FailureKind` variants forces every
new `FailureKind` to allocate an exit code and forces CI scripts to
maintain a code-to-kind mapping. The three-bucket scheme keeps exit
codes stable across `FailureKind` additions; precision-needing
consumers use `--json`. See decision 7.
