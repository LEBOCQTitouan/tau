# Propose an ADR

You're about to make a change the constitution (QG18) requires an
ADR for, or you've discovered a decision worth recording. This
recipe walks through the mechanics.

## When an ADR is required

QG18 enumerates the change classes:

- Changes to project guidelines (G* / NG* / QG* / PG*).
- Additions or breaking changes to public APIs (the `tau-runtime`
  crate's exported items, the serve-mode IPC protocol).
- Changes to the serve-mode protocol.
- Changes to the package manifest format.
- Changes to plugin trait boundaries (`LlmBackend`, `Tool`,
  `Storage`, `Sandbox`).

Other changes — bug fixes, refactors confined to a single crate,
docs-only updates, additive non-breaking enum variants — do **not**
require an ADR. Don't write one defensively; the bar is real cost
plus durable decision.

## Step 1 — write a spec first (if the work is non-trivial)

For multi-day work, the convention is to write a spec under
`docs/superpowers/specs/YYYY-MM-DD-<slug>-design.md` *before*
landing the ADR. The spec is mutable and exploratory; it explores
the option space, names what was rejected and why, and converges
on a recommended option. The ADR is the *result* — a stable
historical record of the decision.

Skip the spec for one-PR-sized work; go straight to step 2.

## Step 2 — copy the template

```bash
cp docs/decisions/template.md docs/decisions/NNNN-<slug>.md
```

Where:

- `NNNN` is the next sequential number. Check `ls docs/decisions/`
  for the highest existing.
- `<slug>` is kebab-case, brief, descriptive. Examples:
  `0024-multi-agent-orchestration`, `0020-sandbox-proxy`.

The template has six sections:

```
# ADR-NNNN — Title

**Status:** Accepted YYYY-MM-DD.
**Branch / PR:** feat/foo (PR pending).
**Spec:** docs/superpowers/specs/YYYY-MM-DD-foo-design.md (if applicable).

## Context
## Decision
## Alternatives considered
## Consequences
## v1 limitations (if applicable)
## References
```

## Step 3 — write each section

| Section | What to cover |
|---|---|
| **Context** | Why this decision is happening now. Cite the constitution guidelines, ROADMAP entries, sub-projects, prior ADRs you're amending. Two short paragraphs is usually enough. |
| **Decision** | What you're doing, in declarative present tense. Multiple decisions split as `## Decision 1`, `## Decision 2`. State scope and scope-not boundaries explicitly. |
| **Alternatives considered** | Numbered list of options you rejected, with one-line reasons. This is the section future-you will reach for. Don't skip. |
| **Consequences** | What changes for callers / contributors / users. Public-API surface impact, migration path, performance impact, what tests now exist. |
| **v1 limitations** | If you're shipping less than the full vision, name it. "Live trace rendering is summary-only" type honesty. |
| **References** | Related ADRs, the spec, RFCs, prior art (LangGraph, AutoGen, CrewAI for orchestration; landlock RFCs for sandboxing; etc.). |

## Step 4 — cross-link

If the new ADR *amends*, *refines*, *closes*, or *supersedes* an
existing ADR, the template's frontmatter has fields for each:

```
**Amends:** [ADR-0014](0014-sandboxing.md) §1 — `select_adapter` removed.
**Refines:** [ADR-0008](0008-plugin-loading.md) — `PluginHostOptions` gains fields.
**Closes:** sub-project A from the sandboxing followups doc.
**Supersedes:** [ADR-0019](0019-per-host-network-filter.md) (veth + nft machinery).
```

Then update the superseded ADR's own status to "Superseded by
ADR-NNNN" so future readers don't accidentally rely on it.

## Step 5 — index it in `SUMMARY.md`

Add a line to `docs/SUMMARY.md` under the "Architecture decisions"
section so the ADR is reachable from the published book:

```markdown
- [ADR-NNNN — Short title](decisions/NNNN-slug.md)
```

The ADR-index page (`docs/decisions/README.md`) lists ADRs by
status; update it if your ADR introduces a new category or
supersedes an existing entry.

## Step 6 — wait at least 24 hours before merge

Per QG22 ("Code review is required, even for solo maintainer"),
guideline-touching ADRs wait at least 24 hours between draft and
merge. Typo or formatting fixes are exempt; substantive changes
are not.

For non-guideline ADRs, no formal cooldown exists, but the
overnight-fresh-eyes review is good practice anyway.

## Step 7 — PR mechanics

ADR PRs follow the same conventional-commits + branch protection
rules as code PRs:

- Commit type: `docs(adr):` (or `docs(decisions):` historically).
- Subject: the ADR title verbatim, capped at the configured
  conventional-commits length.
- Body: paste the ADR's Context + Decision summary for the PR
  description. CI gates run via `docs-check.yml`; merge via the
  standard squash workflow.

## Numbering collisions

When two contributors take the same ADR number in parallel PRs,
the rule is **first-merge wins**. The later PR rebases and bumps
to the next number. This has happened (e.g. ADR-0028 used twice
in the docs-deployment and skills-runtime PRs because they merged
within hours of each other; the deploy worked but the duplication
is fixed by a follow-up rename — both are valid distinct ADRs).

Avoid the collision by claiming a number at PR-open time in your
PR title.

## What an ADR is NOT

Three explicit non-purposes:

- **Not a feature spec.** Specs are mutable, exploratory, in
  `docs/superpowers/specs/`. ADRs are immutable historical
  decisions in `docs/decisions/`.
- **Not a roadmap entry.** Phase / priority / sub-project
  tracking lives in `ROADMAP.md`.
- **Not a tutorial.** If you're describing how to *use* the
  feature an ADR records, the user-facing doc goes in
  `docs/tutorials/` or `docs/how-to/`, not in the ADR.

## See also

- [`docs/decisions/template.md`](../decisions/template.md) — the
  template to copy.
- [`docs/decisions/README.md`](../decisions/README.md) — the ADR
  index page.
- [`CONSTITUTION.md`](../../CONSTITUTION.md) §4 "Amendment
  process" + QG18 (when ADRs are required) + QG22 (overnight
  delay).
- ADR-0024, ADR-0020, ADR-0014 — three well-shaped recent ADRs
  worth reading as exemplars.
