# Post-multi-agent priority queue (2026-05-12)

## Context

As of branch tip `e400688` (multi-agent v1.2 — per-spawn `system_prompt`
+ ROADMAP §16 Skills entry), the immediate runtime work on multi-agent
orchestration has reached a natural pause. This doc captures the queue
ordering for what comes next, locked in after a brainstorming session.

The ordering is opinionated: it reflects current strategic priorities
(close Constitution commitments, ship usable end-to-end multi-agent,
make progress on the compiled-language vision) and a bias toward
small/medium PRs over large strategic surfaces.

## Priority queue

### Position 1 — Push v1.2 PR + merge

**Branch:** `feat/multi-agent-v1.2-spawn-system-prompt-plus-skills-roadmap`
**Estimate:** 30 minutes (push + CI + squash-merge per the established
PR #53/#55/#56/#57/#58/#59/#60 cadence).

Closes the loop on per-spawn `system_prompt` work that's already
committed but unpushed. Trivial — just needs the operational push +
CI + merge cycle.

### Position 2 — v1.2 follow-up: Live trace rendering

**Estimate:** 1-2 days.

Today the CLI runs an orchestrated agent end-to-end and prints a
summary AFTER the run completes. Trace events (`Spawn`, `Turn`,
`ToolCall`, `TaskMutation`, `PlanNote`, budget events) are written to
the JSONL log but invisible in real time.

**Scope of change:** extend `Runtime::spawn_root_agent` to accept a
caller-supplied `Vec<TraceSubscriber>` (or a `TraceSubscriber`
parameter). CLI builds a subscriber wired to `run_printer` from
`crates/tau-cli/src/cmd/output_orchestration.rs` (printer already
exists; just wasn't subscribed). Both subscribers (printer + JSONL
writer) coexist on the same `TraceStream`.

**Why this position:** small, well-bounded, makes the multi-agent
runtime feel real to end users. The npm/cargo-style printer is
shipped but unreachable today.

### Position 3 — §16 Skills (decomposed)

**Estimate:** 3-4 weeks total, decomposed into ~6 sub-projects of
~3-5 days each.

The biggest single strategic deliverable on the queue. Closes
Constitution G10 ("Skills and MCP are first-class concepts in core").
v1.2's per-spawn `system_prompt` work shipped the runtime
precondition; now the package/install/discovery layer.

**Decomposition (each shipped as its own PR):**

1. **Skills-1: Manifest extension.** New `[skill]` table in
   `tau.toml`; new `SkillManifest` type in `tau-domain`; serde
   round-trip + validation tests. Foundation for everything else.
   ~3-4 days.
2. **Skills-2: Install pipeline.** `tau install <skill-pkg>` resolves
   + installs to scope (reuses `tau-pkg`). Lockfile schema migration
   if needed. ~3-4 days.
3. **Skills-3: Discovery.** `tau skill list` + `tau skill show <name>`
   subcommands. Parallel to `tau list agents`. ~2-3 days.
4. **Skills-4: Runtime invocation.** Spawned `agent.<skill-name>.spawn`
   resolves to the installed skill manifest, pulling `system_prompt`
   + `grant` defaults from the package. Caller can still override per
   spawn. Wired through `validate_agent_spawn` + the kernel intercept.
   ~3-5 days.
5. **Skills-5: Agent Skills spec compliance.** Interop with the
   broader 2026 ecosystem (Anthropic Agent Skills, MCP-adjacent). May
   surface manifest revisions back into Skills-1. ~3-5 days.
6. **Skills-6: Reference skill packages + docs.** Two or three
   exemplary skill packages shipped as test fixtures + user docs.
   ~3-4 days.

Skills-1 is foundational; Skills-2/3/4 depend on it; Skills-5 may loop
back into Skills-1; Skills-6 depends on Skills-1 + Skills-4. Order:
1 → (2, 3) → 4 → 5 → 6.

Each sub-project gets its own design spec + plan + PR cycle. Skills-1
brainstorm follows immediately after this priority doc lands.

### Position 4 — Phase 2 §A: `tau check` standalone

**Estimate:** ~3 weeks (per ROADMAP estimate).

First piece of the "tau as a compiled language" vision. Layer 3
validation as a first-class CLI verb — invocable from CI gates, IDE
extensions, pre-commit hooks. Reuses the validation logic from Tier 3
priority 12 (sandboxing).

**Why this position:** independent of Skills (could parallelize across
sessions). Strategic step toward Phase 2; modest scope; well-bounded.
Picks up after Skills-1 lands if a parallel session is available.

### Position 5 — Pattern integration tests via MockLlmBackend

**Estimate:** 3-5 days.

Un-ignore the 5 `#[ignore]`'d skeletons in
`crates/tau-cli/tests/cmd_orchestration.rs`. Lift MockLlmBackend
multi-turn fixtures from `tau-runtime/tests/`. End-to-end validation
for each of the 5 spec patterns (linear, worker pool, supervisor,
hierarchical, plan-revise).

**Why this position:** lower urgency — the property tests already
cover the invariants and v1.1 closed the last gap (recursive spawn).
But unblocks confidence about end-to-end behavior, especially after
Skills lands and the supervisor/critic pattern becomes
canonical-with-different-prompts.

## Deferred (slot after position 5 when ready)

These remain tracked but don't fit the immediate critical path:

- **Tier 4 §15: Serve mode** — JSON-RPC over stdio; Constitution
  G6/QG12; second stable public surface. Big strategic but no
  immediate driver (~4-6 weeks). Wait for a concrete embed use case.
- **Tier 4 §13: Perf budgets in CI** — Constitution QG14/G16. Important
  hygiene but not blocking new features (~1-2 weeks). Slot when CI
  latency or kernel perf becomes a complaint.
- **Per-spawn `DenyEntry` threading (v1.2 follow-up)** — small, only
  when a use case actually surfaces (e.g. "child should see narrower
  `fs.read` paths than parent").
- **8 cross-tier deferred sub-projects from ROADMAP** — background
  tools, message bus, pull-status, output schemas, plan DAG,
  cross-run memory, workflow-DAG, group chat. Each gated on a
  concrete use case justifying its design surface.

## Cadence note

Recent 7 PRs (#53 through #60) shipped over 3 days each with single-
or low-commit branches and `--no-verify` pushes; CI is authoritative.
The priority queue above assumes similar cadence: ~1 PR per day on
small items, 3-5 days per medium sub-project, with brainstorm-→-spec-→-plan-→-execute
discipline for each.

## Revisit trigger

Re-prioritize when:
- A constitutional commitment becomes load-bearing (e.g. MCP becomes
  necessary for an external integration).
- A use case lands for one of the deferred items (someone needs
  background tools, or serves the runtime over JSON-RPC).
- Strategic direction shifts away from compiled-language vision toward
  pure runtime (or vice versa).
