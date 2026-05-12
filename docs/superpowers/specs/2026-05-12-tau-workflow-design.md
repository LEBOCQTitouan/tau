# `tau-workflow` — linear pipeline runner v1

**Date:** 2026-05-12
**Status:** Approved for implementation
**Branch:** `feat/tau-workflow`
**Roadmap reference:** Tier 3 §10 ("Workflow / pipeline runner — deterministic step-by-step pipelines; possibly new `tau-workflow` crate").

## Goal

Ship a v1 linear-pipeline runner that composes existing `tau-runtime` primitives — `agent.run` + `tool.call` — into multi-step workflows defined in TOML files under `workflows/`. Persist each step's output as append-only JSONL so workflows are resumable across crashes.

## Non-goals

- DAG / parallel branches / fan-out / fan-in. Deferred to a follow-up sub-project ("workflow-DAG") once usage patterns reveal whether linear is sufficient.
- Conditionals, loops, variable assignment. YAGNI for v1.
- Per-step capability overrides; tools inherit the workflow's default agent grants.
- A public library API outside `tau-cli`. `tau-workflow` is internal until another caller (serve-mode, `tau-app`) needs it.

## Locked decisions (from brainstorm)

1. **Shape**: linear v1, plan DAG follow-up.
2. **Where workflows live**: `workflows/*.toml` under the project root. Multiple workflows per project; one file per workflow.
3. **Step vocabulary**: `agent.run` + `tool.call` only.
4. **Persistence**: JSONL append-only log under `<scope>/.tau/workflow-runs/<workflow-name>-<run-id>.jsonl`. `--resume` replays the log, skips completed steps. Mirrors REPL session persistence.
5. **Crate layout**: new `tau-workflow` workspace crate. `tau-cli` consumes it via a new `workflow` subcommand group.

## Architecture

```
                    ┌─────────────────────────────┐
                    │  workflows/<name>.toml      │
                    │   [workflow]                │
                    │   description = "..."       │
                    │   [[steps]]                 │
                    │   id = "research"           │
                    │   kind = "agent.run"        │
                    │   agent = "researcher"      │
                    │   input = "${input}"        │
                    │   [[steps]]                 │
                    │   id = "summarize"          │
                    │   kind = "agent.run"        │
                    │   agent = "summarizer"      │
                    │   input = "${steps.research.output}"
                    └─────────────┬───────────────┘
                                  │ parse
                                  ▼
                    ┌─────────────────────────────┐
                    │  tau_workflow::Workflow     │
                    │  (typed, validated)         │
                    └─────────────┬───────────────┘
                                  │ run(input, opts)
                                  ▼
        ┌─────────────────────────────────────────────────┐
        │  tau_workflow::Runner                           │
        │                                                 │
        │  for each step:                                 │
        │    1. resolve templates ${input}, ${steps.X.out}│
        │    2. dispatch by kind:                         │
        │       • agent.run  → tau_runtime::load_agent +  │
        │                      single-turn run            │
        │       • tool.call  → tau_runtime::load_tool +   │
        │                      invoke                     │
        │    3. append JSONL line                         │
        │    4. carry output forward                      │
        └─────────────┬───────────────────────────────────┘
                      │ on each step completion
                      ▼
                ┌─────────────────────────────────────────┐
                │ .tau/workflow-runs/<name>-<run-id>.jsonl│
                │ { step:"research",  out:"...", ... }    │
                │ { step:"summarize", out:"...", ... }    │
                │ ...                                     │
                └─────────────────────────────────────────┘
```

The runner is `pub async fn Runner::run(workflow: &Workflow, input: String, opts: RunOpts) -> Result<RunOutcome, WorkflowError>`. It depends on `tau-runtime` for agent + tool dispatch and on `tau-domain` for plan/capability types. No new sandbox concerns — every step runs through the existing sandboxed plugin host.

## Components

| Path | Status | Responsibility |
|---|---|---|
| `crates/tau-workflow/Cargo.toml` | Create | New workspace member. Deps: `tau-domain`, `tau-runtime`, `tau-ports`, `serde`, `serde_json`, `toml`, `thiserror`, `tracing`, `uuid`, `chrono`, `tokio` (with `fs`, `io-util`, `time`). |
| `crates/tau-workflow/src/lib.rs` | Create | Re-exports + crate-level docs. |
| `crates/tau-workflow/src/model.rs` | Create | `Workflow`, `Step`, `StepKind` enum (`AgentRun { agent, input }` / `ToolCall { tool, args }`), `Template` (the `${...}` resolver). Pure types + parse from TOML via serde. |
| `crates/tau-workflow/src/runner.rs` | Create | `Runner::new(runtime) -> Self`, `Runner::run(workflow, input, opts) -> Result<RunOutcome, WorkflowError>`. Dispatch per-step via `tau_runtime::plugin_host::{load_llm_backend, load_tool}`. Templates resolved before each step. |
| `crates/tau-workflow/src/persistence.rs` | Create | JSONL append-only log: `RunLog::open_for_write(path)`, `RunLog::append(StepRecord)`, `RunLog::replay(path) -> Vec<StepRecord>`. Mirrors `tau-cli/src/cmd/session/persistence.rs` patterns. |
| `crates/tau-workflow/src/error.rs` | Create | `WorkflowError` enum with variants `ParseFailed`, `StepNotFound`, `AgentNotFound`, `ToolNotFound`, `TemplateError`, `StepFailed { step_id, source }`, `PersistenceError`. All `#[non_exhaustive]`. |
| `crates/tau-workflow/src/template.rs` | Create | Templating engine. Recognizes `${input}` and `${steps.<id>.output}`. Returns `Result<String, TemplateError>`. ~50 LOC, pure. |
| `crates/tau-workflow/tests/integration.rs` | Create | End-to-end workflow run against the `echo-llm` + `echo-tool` fixtures. Verifies JSONL output, resume behavior, template resolution. Behind `integration-tests` feature. |
| `crates/tau-cli/src/cmd/workflow/mod.rs` | Create | `tau workflow {list, run, log, resume}` subcommand dispatch. |
| `crates/tau-cli/src/cmd/workflow/{list,run,log,resume}.rs` | Create | One per subcommand. ~30–80 LOC each. |
| `crates/tau-cli/src/cli.rs` | Modify | Add `Workflow(WorkflowArgs)` enum variant + sub-args. |
| `Cargo.toml` (workspace root) | Modify | Add `crates/tau-workflow` to `members`. Add `tau-workflow = { path = "crates/tau-workflow", version = "0.0.0" }` to `[workspace.dependencies]`. |
| `docs/decisions/0021-tau-workflow.md` | Create | ADR for the workflow crate + format + format-stability commitments. |

## Workflow file format

TOML, one workflow per file under `workflows/`. Example:

```toml
# workflows/research-pipeline.toml
[workflow]
description = "Two-step research pipeline: gather then summarize."

[[steps]]
id = "gather"
kind = "agent.run"
agent = "researcher"      # references [agents.researcher] in tau.toml
input = "${input}"        # user-supplied input from `tau workflow run`

[[steps]]
id = "summarize"
kind = "agent.run"
agent = "summarizer"
input = "${steps.gather.output}"
```

Tool-call example:

```toml
[[steps]]
id = "fetch-readme"
kind = "tool.call"
tool = "fs-read"          # references [plugins.fs-read] in tau.toml
args = { path = "${input}/README.md" }
```

The `args` table is passed verbatim as the tool's invoke args (JSON). Tool capability grants are inherited from the workflow's `[workflow].default-agent`, which is REQUIRED when any `tool.call` step is present. Validation at parse time. Per-step capability overrides are out of scope for v1.

```toml
[workflow]
description = "..."
default-agent = "researcher"   # required if any [[steps]] has kind = "tool.call"
```

## JSONL line schema

One line per step completion. Written + `fsync`'d before the next step starts so a crash mid-write loses at most the trailing line.

```json
{
  "ts": "2026-05-12T14:23:01.123Z",
  "run_id": "01HKZ...",
  "step_id": "gather",
  "step_index": 0,
  "kind": "agent.run",
  "input": "what is RAG?",
  "output": "Retrieval-augmented generation is...",
  "started_at": "2026-05-12T14:22:55.001Z",
  "ended_at":   "2026-05-12T14:23:01.123Z",
  "duration_ms": 6122,
  "status": "ok"
}
```

On failure, `"status": "failed"` and additional fields `"error"` (typed string for matching) + `"detail"` (human-readable). Run aborts; subsequent steps are NOT executed.

`Runner::replay` tolerates a truncated trailing line by skipping it.

## Run flows

### Fresh run

```
tau workflow run research-pipeline --input "what is RAG?"
  │
  ├─ parse workflows/research-pipeline.toml → Workflow
  ├─ resolve scope, build tau_runtime PluginHost (sandboxed per agent grants)
  ├─ allocate run_id (ULID)
  ├─ open <scope>/.tau/workflow-runs/research-pipeline-<run_id>.jsonl (append, fsync)
  ├─ for step in workflow.steps:
  │     resolve templates against {input, steps_so_far}
  │     dispatch by kind:
  │       agent.run  → spawn agent, run one turn, capture final text
  │       tool.call  → call tool.invoke with templated args, capture result
  │     append JSONL line, fsync
  │     if status == "failed": break
  └─ print last step's output to stdout (and the run_id to stderr)
```

### Resume

```
tau workflow resume <run_id>
  │
  ├─ locate <scope>/.tau/workflow-runs/*-<run_id>.jsonl by glob
  ├─ replay: parse all complete lines into Vec<StepRecord>
  ├─ drop trailing partial line (if any)
  ├─ verify the JSONL's workflow-name still resolves to a workflow file
  ├─ load that workflow file; verify step ids match the replayed records
  │    (strict drift check: if step ids don't match, refuse to resume)
  ├─ skip steps already in the log with status == "ok"
  ├─ if the last logged step was "failed": re-run it (don't continue past failure blindly)
  └─ proceed with remaining steps; append to the same JSONL
```

**Drift handling**: if the workflow file changed between original run and resume, refuse and surface the diff. `--force` flag overrides with a `tracing::warn!`. Mirrors REPL persistence's `--force` semantics.

### `tau workflow log <run_id>`

```
research-pipeline / run 01HKZ...                     ✓ completed
  [0] gather      agent.run  6.1s   ok
      input:  "what is RAG?"
      output: "Retrieval-augmented generation is..."
  [1] summarize   agent.run  3.2s   ok
      input:  "Retrieval-augmented generation is..."
      output: "RAG combines retrieval + LLMs to..."
```

`--json` emits the raw JSONL lines instead of the pretty form.

## Error handling

- All `WorkflowError` variants are `#[non_exhaustive]`.
- Step failures wrap the upstream error (`PluginError`, `LlmError`, `ToolError`) preserving the source for `Debug` output.
- Template-resolution errors include the unresolved `${ref}` and the step id where it appeared.
- Persistence errors (disk full, permission denied) abort the run with a clear message. The partial JSONL is NOT cleaned up — the user can inspect it.
- CLI exit codes: 0 = success, 1 = workflow ran but a step failed (analogous to `tau run`'s `AgentFailed`), 2 = CLI/config error.

## Testing

- **Unit tests in `tau-workflow`:**
  - `model::tests` — TOML parsing happy path + every error variant (missing `id`, unknown `kind`, malformed `args` JSON, duplicate step ids).
  - `template::tests` — `${input}`, `${steps.gather.output}`, unresolved-reference error, escaping `$${...}`, forward-reference rejection at parse time.
  - `persistence::tests` — round-trip a `StepRecord` through JSONL; `replay` on a file with a truncated trailing line; `replay` on an empty file; `append` then `replay` returns what was appended.
  - `runner::tests` — using a thin trait abstraction over `tau-runtime`'s plugin host (or its existing mock fixtures): a 2-step workflow runs both steps in order; templates resolve correctly; a failed step aborts the run and logs `status=failed`.

- **Integration test (`tests/integration.rs`, feature `integration-tests`):**
  - End-to-end run against the `echo-llm` + `echo-tool` plugins. Verifies the runner + persistence + templates work together. Uses `tempfile::TempDir` for scope.

- **CLI tests in `tau-cli/tests/cmd_workflow.rs`:**
  - `tau workflow list` on a tempdir with two workflow files → asserts both names printed.
  - `tau workflow run echo-pipeline` → asserts exit code 0 + JSONL file created with 2 lines.
  - `tau workflow log <run-id>` → snapshot test (insta) for the pretty output.
  - `tau workflow resume <run-id>` after a simulated mid-run crash (truncate JSONL to 1 line, then resume) → asserts step 2 executes, JSONL grows to 2 lines.
  - Drift test: workflow file changes between run and resume → without `--force`, exits with code 2; with `--force`, proceeds + emits warning.

- **Regression guards** — load-bearing assertions:
  - `runner_aborts_on_step_failure` — a failed step must NOT be followed by subsequent steps.
  - `jsonl_is_atomic_per_line` — partial trailing line is dropped on replay (write 1 full line + 50 random bytes, replay must yield 1 record).
  - `resume_with_drift_refuses_without_force` — modifying the workflow file between runs invalidates the resume.

## Scope

**In scope:**

- Linear sequential workflows, one TOML file per workflow under `workflows/`.
- Step kinds: `agent.run`, `tool.call`.
- Templates: `${input}`, `${steps.<id>.output}`. String substitution only — no expressions, no conditionals.
- JSONL persistence + `--resume` with strict drift checking + `--force`.
- CLI: `tau workflow {list, run, log, resume}`.
- ADR-0021 documenting the format + crate + decisions.
- Test coverage: unit + integration + CLI snapshots.

**Out of scope (deferred to follow-ups):**

- DAG / parallel branches / fan-out / fan-in. Earmarked: sub-project "workflow-DAG".
- Conditional steps (`if`, `when`, `branch`).
- Loop / map / each-over-list.
- Per-step capability overrides; tools inherit the workflow's default agent grants.
- Inter-step variables beyond `${steps.<id>.output}` (no `assign`, no `set`).
- `tau workflow runs gc` cleanup command.
- Cron-style scheduled workflows.
- Workflow-level pre/post hooks.
- Network / cluster orchestration (out forever per ROADMAP).
- `tau-workflow` as a public library API (only consumed by `tau-cli` for v1).

## Risks

- **Template-resolution edge cases.** Users will write `${steps.foo.output}` referring to a later step. Detect + reject at workflow-parse time (forward reference). Tested.
- **Workflow drift.** Users edit a workflow mid-run, then `--resume`. Strict mode + `--force` mitigates; documented in resume-flow above.
- **Long-running steps.** Single LLM call ≥ 30s is normal. Runner does not impose a step timeout; persistence is between steps, not within. (Per-step timeout is a follow-up if users ask.)
- **Capability gaps for tool.call.** If a step does `tool.call` for a tool whose default-agent capabilities don't include the right grant, the runtime refuses at invoke time. Surface that as a clear error (`tool_call_capability_missing`) rather than a confusing pkg-side denial. Tested.

## Verification (end-to-end at PR time)

1. `cargo fmt --all -- --check` clean.
2. `cargo clippy --workspace --all-targets -- -D warnings` clean.
3. `cargo nextest run -p tau-workflow --lib` — unit suite green.
4. `cargo nextest run -p tau-workflow --features integration-tests --tests` — integration suite green.
5. `cargo nextest run -p tau-cli --test cmd_workflow` — CLI suite green.
6. Full workspace `cargo nextest run --workspace --all-targets` — no regressions in adjacent crates.
7. `cargo deny check` green (new crate's deps within existing allow-list).
8. CI green on all 19 required checks.
