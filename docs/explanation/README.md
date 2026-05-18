# Explanation

Understanding-oriented documentation: discursive material on
architecture, trade-offs, and the rationale behind decisions that
shape how tau feels to use.

Explanation pages are the place to argue, compare, and reflect. They
sit between the dense legal-style prose of ADRs and the
goal-directed brevity of tutorials and how-tos. Read them when you
want the *why* — and the why's edges, alternatives, and consequences.

Where this section ends and ADRs begin: explanation pages are
allowed to be opinionated and to evolve; ADRs in
[`../decisions/`](../decisions/README.md) are immutable records of a
specific decision at a specific date.

## Pages

- [Packages](packages.md) — the unit of extension. What a package is,
  the seven kinds, sources, scopes (global / project), and the
  install → lock → verify → run lifecycle. Read this first if you're
  new to the book.
- [Capabilities and consent](capabilities-and-consent.md) — what a
  capability is, declared-vs-granted, the consent prompt at install
  time, project-side narrowing, and where the kernel enforces the
  set.
- [Sandboxing](sandboxing.md) — the tier model
  (`none` / `light` / `strict`), the adapter set per platform, the
  four-layer enforcement model, and what sandboxing is *not*
  designed to do.
- [Multi-agent orchestration](multi-agent-orchestration.md) — six
  entities, three channels, six invariants, and the five
  tree-topology patterns the kernel composes (linear, worker-pool,
  plan-revise, supervisor/critic, hierarchical). What this rules
  out and why.
- [Workflows](workflows.md) — the external-script-driven
  counterpart to in-run orchestration. Deterministic linear
  pipelines authored in TOML, persisted as JSONL, resumable. When
  to reach for a workflow vs in-run orchestration.
- [Serve mode](serve-mode.md) — tau as a long-lived subprocess
  speaking JSON-RPC 2.0 over NDJSON-framed stdio. One of tau's
  two public surfaces (G6). When to embed tau in a parent app
  vs. driving it through the CLI.

### For contributors

- [Architecture overview](architecture-overview.md) — a single
  `tau chat` traced end-to-end through the codebase. Phase ⨯
  owner-crate map. Where to look when X is broken.
- [Crate map](crate-map.md) — every workspace crate with a
  one-line purpose, the hexagonal-architecture dependency rules,
  and "where new code lands" by feature kind.
- [Testing strategy](testing-strategy.md) — the standard Rust
  layers (QG5) plus the four sandbox-test layers (L1 manifest →
  L4 kernel enforcement). Where each kind of test lives and what
  helpers exist.
- [Tau as language](tau-as-language.md) — the framing that "tau is a
  language for installing and running agents," what that buys, and
  what it costs.
- [Escape hatches](escape-hatches.md) — the principled set of opt-outs
  (`--no-sandbox`, `--allow`, capability overrides) and what each one
  actually disables.
- [Two-layer skills](two-layer-skills.md) — why a tau skill is
  `SKILL.md` *plus* `tau.toml` rather than either one alone; the
  Option-D reframing, the roundtrip claim, and how this differs from
  pure-Anthropic skills.

## See also

- [`CONSTITUTION.md`](../../CONSTITUTION.md) — the guidelines that
  explanation pages tend to elaborate on.
- [Architecture decisions](../decisions/README.md) — the historical
  record of specific choices.
