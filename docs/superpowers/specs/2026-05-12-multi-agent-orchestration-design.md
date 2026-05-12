# Multi-agent orchestration — primitive set design

**Date:** 2026-05-12
**Status:** Approved for implementation
**Branch:** `feat/multi-agent-orchestration`
**Roadmap reference:** Tier 3 §9 ("Multi-agent orchestration — G10's deferred half").
**Predecessors:** tau-workflow v1 (PR #58, c9bf67d) ships linear pipelines via tau-cli–side composition. This spec defines the runtime-level primitives that multi-agent workflows compose from.

## Goal

Define the irreducible primitives — entities, operations, channels, invariants — that enable agents to spawn, coordinate with, and observe other agents. Not a workflow runner; a *language* for writing one (and any other multi-agent pattern: supervisor, worker pool, plan-revise loop, hierarchical team).

## Non-goals

This spec defines a primitive set. It is deliberately silent on:

- A specific orchestration pattern. The same primitives compose into linear / hierarchical / supervisor / worker-pool / plan-revise patterns; the runtime endorses none of them.
- A user-facing TUI. CLI output is npm/cargo-style line-feed; no cursor magic.
- Background / monitor / watch tools. Tracked in `ROADMAP.md` as a future Tier 3 sub-project; would extend the primitive set with a fourth channel and a new tool kind. Not in v1.
- Inter-agent message bus or inbox stacks. v1 uses **shared state** (TaskList + Plan) for coordination, not message-passing. See "Considered and rejected" below.
- Output schemas / typed tool returns. Future extension on `Tool.result_schema`.
- Plan DAG with task dependencies. Future extension on `Task.depends_on`.
- Cross-run memory. Future extension above `Run`.

## Considered and rejected for v1

Each entry is a valid pattern that production agentic systems ship; each is a clean extension of the v1 primitive set when a real use case lands. Rejected here for one reason: no concrete v1 use case justifies the design surface.

| Rejected | Why considered | Why rejected for v1 | Future extension shape |
|---|---|---|---|
| Message bus | enables broadcast / cross-child coord | tau is single-machine; tree topology, not many-to-many | new Channel + AgentInbox entity |
| Push messages to LLM context | unsolicited interrupts | breaks LLM coherence (no widely-accepted solution in 2026) | same as above |
| Inbox stack (deferred delivery at turn boundary) | safe variant of push | TaskList already serves "deferred coordination" via pull | same as above |
| Pull-status tool | parent checks on slow child | watchdog timeouts (host-side) serve same use case | virtual tool addition |
| Handoffs (OpenAI Swarm) | router pattern | expressible as agent-as-tool today | optional virtual tool |
| Background tools / monitors | event-driven tools | different primitive class (Channel D) | new entity + channel |
| Plan DAG (CrewAI tasks-with-deps) | parallel scheduling | linear hierarchy is enough for v1 | `Task.depends_on` field |
| Output schemas | typed chaining | tau has no schema vocabulary yet | refine `Tool.result_schema` |
| Cross-run memory | learning across sessions | session persistence is already enough | new persistent entity |
| Group chat / mediator | many-to-many | TaskList serves the cases that need this | new Channel + Mediator entity |

The principle: v1's primitives must support all five workflow patterns enumerated below. Each rejected feature is an addition, not a re-architecture.

## Entities (the nouns)

### Identity

```
Id = String                      // ULID; sortable, unique
```

Every entity has one. Immutable. Issued on creation.

### Capability

```
Capability =
  | Filesystem { mode: Read|Write|Exec, paths: [Glob] }
  | Network { kind: Http, hosts: [HostPattern] }
  | Process { Spawn { commands: [String] } }
  | Agent { Spawn { allowed_kinds: [String] } }           // existing in tau-domain
  | TaskList { mode: Read | Write | Manage }              // NEW
  | Plan { mode: Read | Write }                           // NEW
  | Custom { name, params }
```

`TaskList::Read` permits `task.list` and `task.get`. `TaskList::Write` adds `task.create`, `task.claim`, `task.update`, `task.complete`, `task.fail`, `task.release`, `task.heartbeat`, `task.discard`. `TaskList::Manage` adds administrative operations (delete tasks irrespective of ownership).

`Plan::Read` permits `run.plan` (read scratchpad); `Plan::Write` adds `run.note`.

### Agent

```
Agent {
  id: Id
  kind: String                   // "researcher" | "writer" | "orchestrator" | ...
  spawned_by: Option<Id>         // parent agent; None for run-root
  grant: [Capability]            // ⊆ parent's grant (no auto-propagation)
  llm_backend: PluginRef
  system_prompt: String
  history: [Message]             // private; never enters other agents' contexts
  status: spawning | running | completed | failed | aborted
  spawned_at, completed_at: Timestamp
}
```

An actor. Has private state (history) + observable lifecycle.

### Task

```
Task {
  id: Id                         // hierarchical: "01" → "01.2" → "01.2.1"
  description: String
  parent_task_id: Option<Id>
  created_by: Id (Agent)
  owner: Option<Id>              // lock holder; None = unclaimed
  lease_expires_at: Option<Timestamp>
  status: pending | claimed | in_progress | done | failed | discarded
  result_summary: Option<String>
  error: Option<String>
  events: [TaskEvent]            // audit trail of mutations
}
```

A unit of intended work. Mutable. Locked by owner via lease (TTL); heartbeat renews.

### TraceEvent

```
TraceEvent {
  id: Id
  ts: Timestamp
  run_id: Id
  agent_id: Option<Id>           // who emitted (None for host events)
  kind:
    | spawn { child_id, kind, grant }
    | turn { agent_id, turn_index, duration_ms }
    | tool_call { tool_name, duration_ms, status }
    | task_event { task_id, kind: created|claimed|updated|completed|failed|... }
    | plan_note { agent_id, text_snippet }
    | budget_warn / budget_exceeded
    | completion { agent_id, status }
    | abort { reason }
  payload: Json
}
```

An observable side-effect. Append-only. Lives in the trace stream.

### Run

```
Run {
  id: Id
  root_agent_id: Id
  task_list: [Task]
  plan: String                   // free-form scratchpad; appendable
  budget: {
    max_total_tokens: Option<u64>
    max_total_duration: Option<Duration>
    max_total_agents: Option<u32>
  }
  trace: [TraceEvent]
  status: running | completed | failed | aborted
  started_at, ended_at: Timestamp
}
```

Top-level invocation. Owns the task list, plan, trace, budget. One JSONL per run.

### Tool

```
Tool {
  name: String                   // "fs.read", "agent.researcher", "task.claim", ...
  args_schema: JsonSchema
  result_schema: JsonSchema
  required_capability: Capability
  backed_by: BackingKind
    | Plugin (real subprocess, existing tau-runtime plugin host)
    | Virtual (host-resolved: task.*, run.note, agent.<kind>.spawn)
}
```

An effect agents can invoke. Some are backed by plugins; some are synthetic. Virtual tools are how agents address shared state and spawning.

## Operations (the verbs)

Every action is one of these. Atomic.

### Agent-emitted (LLM decides)

```
think()                  // run one LLM turn; produces assistant message
call(tool_name, args)    // invoke tool; capability check; return tool_result
complete(final_text)     // commit final answer; lifecycle → completed
```

### Virtual tools (host-implemented; LLM-callable like any tool)

```
task.create(description, owner_id?, parent_task_id?)   → task_id
task.claim(task_id)                                    → ok | locked_by(agent, since)
task.heartbeat(task_id)                                → ok | not_owner
task.release(task_id)                                  → ok
task.update(task_id, status?, notes?)                  → ok
task.complete(task_id, result_summary)                 → ok
task.fail(task_id, error)                              → ok
task.discard(task_id, reason)                          → ok    // accept orphaned
task.list(filter)                                      → [task]
task.get(task_id)                                      → task

run.note(text)                                         → ok    // append to plan
run.plan()                                             → string

agent.spawn(kind, grant, message)                      → tool_result  // sync; child's final answer
```

### Host-emitted (runtime kernel; not LLM-callable)

```
trace.emit(event)              // any side-effect → trace stream
budget.check()                 // tick per turn; may trigger abort
lease.expire(task_id)          // claim TTL expired; lock released
persist(run)                   // periodic JSONL flush
```

## Channels (data-flow rules)

Three distinct channels with strict routing.

**Channel A — synchronous tool return**
- `agent.call(tool, args) → result`
- Result lands in caller's LLM context AS A TOOL_RESULT MESSAGE
- Exactly one per call
- LLM coherence: clean

**Channel B — shared mutable state**
- TaskList + Plan/notes
- Mutated via virtual tools (which traverse Channel A)
- Read via virtual tools (which traverse Channel A)
- LLM coherence: clean (pull-only)

**Channel C — trace stream**
- TraceEvent append-only sequence
- Destination: host (CLI, --record-protocol, watchdog, JSONL persister)
- NEVER enters any LLM's context
- LLM coherence: irrelevant

**Critical invariant: only Channel A inputs anything into an LLM's context.** Channels B and C are observable but not perturbing.

## Invariants (the laws)

The runtime enforces these. Together they make the primitives safe to compose.

1. **Capability subset law.** `child.grant ⊆ parent.grant`. No auto-propagation; every cap must be explicitly carried over at `agent.spawn`.
2. **Task lock exclusivity.** For any task with `owner = A` and lease not expired: no operation by `B ≠ A` mutates that task. Reads are always permitted (subject to `TaskList::Read`).
3. **LLM-context immutability outside Channel A.** For any message in `agent.history`: it arrived via Channel A (own assistant turn, own tool_result, or initial user input). No other channel can prepend, append, or inject.
4. **Trace event monotonicity.** The trace stream is append-only with monotone timestamps per agent.
5. **Run termination rule.** `run.status = completed` iff `root.status = completed` AND for every task: `status ∈ {done, failed, discarded}`. Orphaned tasks (pending / claimed / in_progress) at root completion ⇒ `run.status = failed` with `orphaned_tasks_at_termination` trace event, unless the orchestrator explicitly `task.discard`s them.
6. **Budget enforcement.** For every set `run.budget.max_*`: when exceeded, host emits `budget_exceeded` trace event + sends abort signal to every running agent in the spawn tree. Run terminates as `aborted`.

## Pattern examples (same primitives, different shapes)

These illustrate that the primitive set is sufficient. None requires a new framework concept.

### Pattern A — Linear pipeline (already shipped via tau-workflow v1)

```
ROOT (kind=orchestrator)
  └ task.create("01: research") owner=researcher
  └ task.create("02: summarize") owner=summarizer
  └ agent.spawn(researcher, grant=..., msg="...")     ← Channel A blocks
       researcher: task.claim("01") → task.complete("01", result) → returns final
  └ root sees tool_result
  └ agent.spawn(summarizer, grant=..., msg="...")
  └ root.complete(final_text)
```

### Pattern B — Worker pool

```
ROOT (kind=planner)
  └ for i in 0..10: task.create("subtask_" + i)
  └ root.note("plan: dispatch via 3 workers")
  └ for _ in 0..3:
       agent.spawn(worker, grant=[..., TaskList::Write], msg="pick a task")
          worker: task.list(status=pending, owner=None)
                  → task.claim(an_id)               ← exclusivity prevents collisions
                  → task.heartbeat (per turn)
                  → task.complete(claimed_id, result)
                  → returns final
   (root iterates sync; each worker spawn picks a different task via claim CAS)
```

### Pattern C — Supervisor / critic

```
ROOT (kind=supervisor)
  └ task.create("01: research") owner=researcher
  └ agent.spawn(researcher, grant=[..., TaskList::Read], msg="...")
  └ root sees tool_result
  └ task.get("01") → reads researcher's result + audit trail
  └ if quality.bad:
        agent.spawn(critic, grant=[TaskList::Read], msg="critique result")
             critic reads task list, returns analysis
        decide: accept | reject | re-spawn researcher with feedback
```

### Pattern D — Hierarchical with sub-orchestrator

```
ROOT (kind=program_manager)
  └ agent.spawn(team_lead, grant=[..., Agent::Spawn { kinds=["coder","tester"] }])
       team_lead: task.create("subtask_a") task.create("subtask_b")
                  agent.spawn(coder, msg="implement A")
                  agent.spawn(tester, msg="test A")
                  task.complete("subtask_a", "passes")
                  agent.spawn(coder, msg="implement B")
                  agent.spawn(tester, msg="test B")
                  team_lead.complete("all green")
  └ root receives team_lead's final
```

### Pattern E — Plan-revise loop

```
ROOT (kind=orchestrator)
  while not done:
    └ root.note("considering: ...")
    └ task.list(status=pending) → if empty: think; spawn agent to add tasks
    └ task.list(status=pending) → spawn worker for next
    └ task.list(status=failed) → if any: revise plan, retry
    └ check: all tasks ∈ {done, failed, discarded} ⇒ root.complete(...)
```

## CLI output (npm/cargo-style line-feed)

```
$ tau run orchestrator --input "research RAG and summarize"

  ❯ tau run orchestrator · run 01HKZ8FX...

  ◆ orchestrator                                              spawned
    └ task created: [01] Research RAG
        owner: orchestrator
    ◆ researcher                                              spawned
      └ task created: [01.1] Find seminal papers
      └ task claimed: [01.1]                        🔒 researcher
        Turn 1 (tool: web_search)                              1.4s
        Turn 2 (tool: web_search)                              2.1s
        Turn 3                                                 0.8s
      └ task done:    [01.1] → "found 3 seminal papers"
    ✓ researcher                                              5.6s · 18,400 tok
    ◆ writer                                                  spawned
      └ task done:    [02] → "summary drafted"
    ✓ writer                                                  1.8s ·  4,100 tok
  ✓ orchestrator                                              9.5s ·  3,200 tok

  ─────────────────────────────────────────────────────────────────────
  Summary                                          33,800 tok · 9.5s

      agent           turns    duration    tokens
      orchestrator        2        0.9s     3,200
      researcher          5        5.6s    18,400
      writer              1        1.8s     4,100

  ─────────────────────────────────────────────────────────────────────

  run_id: 01HKZ8FX0...

  RAG (retrieval-augmented generation) combines a retrieval step over an
  external knowledge corpus with a generation step in an LLM ...
```

Visual elements (all line-feed compatible): `❯` for command echo (cyan); `◆` for spawn (blue); `✓` for completion (green); `✗` for failure (red); `🔒` next to claimed tasks; tree-style `└` indentation; right-aligned duration + tokens column. When piped (`| tee log.txt`), ANSI escapes fall back to plain text; alignment via space-padding still works.

Interactive TTY may overlay a braille spinner on the most recent "in_progress" line, replaced via `\r\033[K` ONLY as long as it remains the most recent line. Once a new line follows, the spinner line freezes. This is the cargo / pnpm pattern; degrades cleanly on non-TTY.

No TUI. No cursor jumps beyond the spinner overlay. Pipe-friendly by construction.

## Persistence

One JSONL file per run at `<scope>/.tau/runs/<run-id>.jsonl`.

Schema: each line is one of:
- `{"kind": "trace_event", "event": <TraceEvent>}`
- `{"kind": "task_mutation", "task_id": ..., "before": ..., "after": ...}`
- `{"kind": "plan_append", "agent_id": ..., "text": ...}`
- `{"kind": "run_completion", "status": ..., "summary": ...}`

Append-only with `fsync` after each write. Replay tolerates trailing partial lines (crash safety). Mirrors the tau-workflow + REPL-session pattern.

## Implementation surface

This spec describes runtime + protocol semantics. The implementation lives across:

- **`tau-domain`** — add `Capability::TaskList { mode }` + `Capability::Plan { mode }` variants. Pure types.
- **`tau-ports`** — define a `MultiAgentRun` port that the runtime exposes; CLI / tau-workflow consume it. Minimal — mostly hosting the entity types.
- **`tau-runtime`** — implement the operations:
  - `Runtime::spawn_root_agent(...)` (replaces the single-agent `run` for multi-agent runs; single-agent `Runtime::run` stays for backwards compat)
  - Virtual tools (`task.*`, `run.note`, `agent.spawn`) resolved by the host before plugin dispatch
  - TaskList state inside the Runtime
  - TraceStream emission + persistence
  - Lock + lease + heartbeat enforcement
  - Capability subset law + budget enforcement at every `agent.spawn`
- **`tau-cli`** — new `tau run` flow that wraps `spawn_root_agent`; npm/cargo-style printer subscribed to the trace stream; summary table at end.
- **`docs/decisions/0023-multi-agent-orchestration.md`** — the ADR.

Whether a separate `tau-orchestration` crate is warranted (vs. extending `tau-runtime`) is an implementation-plan decision. The case for a separate crate: keeps the kernel small; lets workflow / serve-mode reuse the orchestration layer. The case against: most operations are kernel-adjacent (capability checks, plugin dispatch).

**Estimate per the plan:** single PR for the runtime + CLI surface; ~2,000 LOC including tests. The primitive set is intentionally small.

## Testing strategy

- **Unit tests** in each module (Capability variants, Task state transitions, lock acquire/release/expire, TraceEvent serialization, Run lifecycle).
- **Integration tests** exercising each of the 5 pattern examples end-to-end against the `MockLlmBackend` fixture.
- **Property tests** for the invariants:
  - Capability subset law: random spawn trees; assert child.grant ⊆ parent.grant always.
  - Lock exclusivity: concurrent claims on the same task; assert at most one succeeds.
  - LLM context immutability: replay a run; assert no agent's history changed except via Channel A operations.
  - Trace monotonicity: random event interleavings; assert per-agent timestamps monotone.
- **CLI snapshot tests** (insta) for the npm/cargo-style output across the 5 patterns.

## Verification (end-to-end at PR time)

1. `cargo fmt --all -- --check` clean.
2. `cargo clippy --workspace --all-targets -- -D warnings` clean.
3. `cargo nextest run --workspace --all-targets` green.
4. `cargo deny check` green (no new deps; if any added, allow-list extended).
5. Five pattern examples have passing integration tests.
6. Property-test suite green (100k iterations per invariant).
7. CI green on all 19 required checks.

## Roadmap update

This spec adds an entry to `ROADMAP.md` under Tier 3:

- §9 Multi-agent orchestration: **In progress** (this spec → implementation plan → PR).
- New deferred sub-project: **Background tools / monitors** ("watch tools" — a fourth channel + new tool kind; would enable claude-code-style Monitor patterns; out of v1 scope, tracked here for future).
- New deferred sub-projects (cross-referenced in this spec's "Considered and rejected" table): inter-agent message bus, inbox stacks, output schemas, plan DAG, cross-run memory, group chat / mediator.

## Out of scope (recap)

- Specific orchestration patterns (no opinion on linear vs hierarchical vs supervisor; the primitives support all).
- TUI / cursor-jumping CLI display.
- Background / monitor / watch tools.
- Message bus / inbox / async push to LLM.
- Output schemas.
- Plan DAG with task dependencies.
- Cross-run memory.
- A separate "orchestration DSL" or workflow language. tau-workflow's `workflows/*.toml` continues to serve linear pipelines; ad-hoc orchestration is expressed in agent system prompts.
