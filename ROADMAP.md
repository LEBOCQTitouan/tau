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

**Status:** Phase 1 sub-project 1 (plugin loading) shipped
2026-04-28; first real LLM-backend plugin (priority 2) is the
natural next sub-project.

| # | Sub-project | Produces | Merged |
|---|---|---|---|
| 1 | Plugin loading mechanism ✅ | Out-of-process IPC over MessagePack-RPC; tau-plugin-protocol + tau-plugin-sdk crates; plugin_host module in tau-runtime; tau-pkg build-on-install; debug-tier subcommands; echo-llm + echo-tool toy plugins | 2026-04-28 |
| 2a | Anthropic LLM-backend plugin ✅ | First real LLM-backend plugin: Anthropic Claude Messages API client at `crates/tau-plugins/anthropic/`; day-1 streaming + tool-use; cassette-replay test harness + env-gated live smoke; in-plugin retry honoring Retry-After; ConfigError::InvalidEnvVar SDK amendment | 2026-04-29 |

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
2. **First real LLM-backend plugin.** Anthropic shipped 2026-04-29 as
   priority 2a (see [ADR-0008](docs/decisions/0008-plugin-loading.md)
   first real consumer). Ollama (priority 2b) and OpenAI (priority 2c)
   follow as their own sub-projects. 16 required CI checks gating
   `main` (was 15).
3. **First real Tool plugin.** `fs-read` + `shell` initial set;
   exercises capability checks at runtime.

### Tier 2 — completes Phase 0 deferrals

4. **Capability override implementation** (project tau.toml
   `[agents.<id>.capabilities]` with intersect-only semantics, per
   ADR-0007 §4 reservation).
5. **Transitive dependency resolution** (`requires.tools` auto-install,
   per ADR-0004 §10 deferral).
6. **Schema validation for tool args** (activates
   `RuntimeError::PluginContractViolation`).
7. **`tau update` / `tau verify` / `tau uninstall` subcommands.**
8. **Streaming LLM responses** (`Runtime::run_streaming` additive).

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
