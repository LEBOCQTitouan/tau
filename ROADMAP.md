# Tau roadmap

This document tracks current phase, near-term priorities, and
explicit out-of-scope items. Updated at phase transitions per PG1 and
PG4.

For per-issue tracking, see [GitHub
Issues](https://github.com/LEBOCQTitouan/tau/issues).

## Current phase: 0 — bootstrap

**Goal:** empty repo with green CI, full governance files, and the
hexagonal workspace skeleton in place. No domain logic.

**Status:** in progress (this commit is part of the bootstrap).

**Done when:** the bootstrap implementation plan
(`docs/superpowers/plans/2026-04-24-repo-bootstrap.md`) is complete and
CI is green on `main` for Linux, macOS, and Windows.

## Near term — Phase 0 sub-projects

Each sub-project below produces working, testable software on its own
and ships in its own brainstorm → spec → plan → implementation cycle.

| # | Sub-project | Produces |
|---|---|---|
| 0 | Repo bootstrap *(this one)* | Empty workspace + governance + CI |
| 1 | `tau-domain` Message + Agent + Package types ✅ | Pure-types crate with `thiserror` errors, doc tests, proptest for parsers *(complete — 2026-04-25)* |
| 2 | `tau-ports` plugin traits ✅ | Trait definitions for LLM backend, tool, storage, sandbox *(complete — 2026-04-26)* |
| 3 | `tau-pkg` package manager ✅ | `tau install` from git URLs, capability declarations parsed (G14), scope resolution (G8) *(complete — 2026-04-27)* |
| 4 | `tau-runtime` agent lifecycle + message passing | Spawn an agent, deliver messages, observe via structured logs (solo path only) |
| 5 | `tau-cli` real subcommands | `tau install`, `tau run`, `tau ls` |

Once 1–5 land, Phase 0 is complete. A retrospective per PG4 closes the
phase and updates this file with Phase 1 priorities.

## Phase 1 (preview)

Subject to retrospective:

- Serve mode (JSON-RPC over stdio) — second public surface (G6, QG12)
- Sandboxing implementation — fulfils G12 (mechanism TBD via ADR)
- Performance budgets enforced in CI (QG14)
- `cargo audit` and `cargo-deny` (QG16)

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
