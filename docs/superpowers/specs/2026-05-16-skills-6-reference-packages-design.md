# Skills-6 — Reference skill packages + user docs design

**Status:** Brainstormed 2026-05-16 (auto mode).
**Branch:** `feat/skills-6-reference-packages`.
**Predecessors:** Skills-1 (`1d71032`), Skills-2 (`93dbe95`), Skills-3 (`7bec3ab`), Skills-4 (`1f6f331`), Skills-5 (`419fd2c`).
**Depends on:** ADR-0025 (Skills foundation), ADR-0026 (install pipeline), ADR-0027 (discovery), ADR-0028 (runtime invocation), ADR-0029 (Anthropic interop).

## Goal

Close ROADMAP §16 by shipping the proof-of-concept the Skills infrastructure has been building toward. Skills-1 through Skills-5 built the **machinery**; Skills-6 ships the **content**:

1. **Three exemplary skill packages** in a new top-level `skills/` directory — each exercises a different capability axis so the value proposition is concrete.
2. **Full Diátaxis documentation** in the existing mdBook — tutorial + how-to + reference + explanation pages — so users have a single coherent on-ramp.
3. **End-to-end integration tests** that install + invoke + export the in-tree skills so the user story is verified continuously in CI.

After Skills-6, a new contributor can `git clone tau` → `cargo build` → `./tau install ./skills/critic` → `./tau skill show critic` → `./tau skill export critic` and have the entire Skills track make sense.

## Anti-goals

- **Refactoring existing test fixtures** to point at `skills/<name>/`. Skills-1–5 inline-synthesize tempdir fixtures in ~10+ test files; lifting them into shared in-tree references would be a high-churn refactor with merge-conflict risk against parallel Claude sessions. Add-only is the policy (D2).
- **Separate `tau-skills` git repo** for external distribution. In-tree is fine for the proof-of-concept; an external repo becomes worth it only when versioning needs to decouple from tau.
- **`tau skill new <name>` scaffolding command.** Useful but separate sub-project; not load-bearing for the user story Skills-6 ships.
- **Additional reference skills** (`summarizer`, `editor`, etc.) — additive; can land in follow-up PRs.
- **mdBook live tutorial with executable code samples.** Current mdBook setup doesn't have execution; not worth introducing here.

## Locked design decisions

### D1 — Three reference skills covering three capability axes

Picked from a 1/2/3-skill scope question. Three skills give one example per major capability axis:

- **`critic`** — `capabilities = []`. Anthropic-roundtrip proof. `tau skill export critic` produces byte-identical SKILL.md to the in-tree source. Demonstrates the two-layer architecture's "tau skill IS an Anthropic skill" claim.
- **`fact-checker`** — `[[capabilities]] kind = "fs.read"` with `paths = ["${SKILL_DIR}/references/**"]`. Bundled `references/style-guide.md` + `references/common-claims.md`. Demonstrates Skills-1's `${SKILL_DIR}` substitution, Skills-4's runtime invocation reading the bundled files, and the multi-file payload story.
- **`pr-reviewer`** — `[[capabilities]] kind = "process.spawn"` with `commands = ["git", "rg"]`. Demonstrates the third capability axis (process spawning) and that `process.spawn` is sandbox-tier-compatible per Skills-2's `sandbox_check`.

### D2 — Add-only: no refactor of existing test fixtures

Skills-1–5 inline-synthesize critic fixtures in tempdirs across the test suite. Skills-6 ADDs new integration tests that exercise the in-tree skills. Existing test fixtures stay as inline tempdir synthesis. Rationale: refactoring 10+ test files for marginal benefit risks subtle regressions in stable code + merge conflicts with the 4-5 concurrent Claude sessions.

### D3 — Full Diátaxis documentation

mdBook (already set up; auto-deploys via PR #67's workflow) gains pages in all four quadrants:

- **Tutorial:** `docs/tutorials/build-your-first-skill.md` — narrative walkthrough using `critic` as the running example. Reader writes their own skill from scratch.
- **How-to:** `docs/how-to/install-a-skill.md`, `author-a-skill.md`, `export-a-skill.md` — recipe-style, problem-oriented.
- **Reference:** `docs/reference/skill-manifest-schema.md` — complete schema of `tau.toml`'s `[skill]` block + `[[capabilities]]` shapes + `SKILL.md` frontmatter requirements.
- **Explanation:** `docs/explanation/two-layer-skills.md` — design reasoning: why Skills-1 picked Option D, what the trade-offs are, how this differs from pure-Anthropic.

`docs/SUMMARY.md` is updated to index the new pages.

## Architecture

```
skills/                                      <- new top-level directory
├── README.md                                <- index of reference skills
├── critic/
│   ├── tau.toml                             <- capabilities = []
│   └── SKILL.md                             <- pure prompt
├── fact-checker/
│   ├── tau.toml                             <- fs.read on ${SKILL_DIR}/refs/**
│   ├── SKILL.md
│   └── references/
│       ├── style-guide.md
│       └── common-claims.md
└── pr-reviewer/
    ├── tau.toml                             <- process.spawn for git + rg
    └── SKILL.md

docs/                                         (modified)
├── SUMMARY.md                                <- index new pages
├── tutorials/build-your-first-skill.md      <- NEW
├── how-to/install-a-skill.md                 <- NEW
├── how-to/author-a-skill.md                  <- NEW
├── how-to/export-a-skill.md                  <- NEW
├── reference/skill-manifest-schema.md        <- NEW
├── explanation/two-layer-skills.md           <- NEW
└── decisions/0030-skills-reference-packages.md <- NEW (ADR)

crates/                                       (additive only)
├── tau-pkg/tests/install_reference_skills.rs  <- NEW (4 tests)
└── tau-cli/tests/reference_skills_e2e.rs      <- NEW (5 tests)
```

## Per-skill detail

### `skills/critic/`

`tau.toml`:
```toml
name = "critic"
version = "0.1.0"
description = "Reviews drafts for clarity, completeness, and rhetorical quality."
authors = ["tau contributors <dev@tau>"]
source = "https://github.com/LEBOCQTitouan/tau.git"
kind = "skill"
dependencies = []
capabilities = []

[skill]
```

`SKILL.md`:
```markdown
---
name: critic
description: Reviews drafts for clarity, completeness, and rhetorical quality.
---

You are a writing critic. Read the user's draft and respond with:

1. **What works.** Two or three concrete strengths.
2. **What's unclear.** Specific passages that lose the reader, with brief
   suggestions for sharpening.
3. **What's missing.** Any audience-facing assumption the draft doesn't earn.

Be specific, not generic. Quote the draft when calling something out.
```

Verified by `tau skill export critic` producing a directory whose SKILL.md byte-matches the in-tree source. No capabilities to drop; export warning quiet.

### `skills/fact-checker/`

`tau.toml`:
```toml
name = "fact-checker"
version = "0.1.0"
description = "Validates factual claims against bundled reference materials."
authors = ["tau contributors <dev@tau>"]
source = "https://github.com/LEBOCQTitouan/tau.git"
kind = "skill"
dependencies = []

[[capabilities]]
kind = "fs.read"
paths = ["${SKILL_DIR}/references/**"]

[skill]
```

`SKILL.md` body references the bundled files:
```markdown
---
name: fact-checker
description: Validates factual claims against bundled reference materials.
---

You are a fact-checker. Use the bundled references at `references/` to
validate claims in the user's input:

- `references/style-guide.md` — house style conventions
- `references/common-claims.md` — vetted statements + supporting evidence

When a claim is uncertain, say so. Cite the reference file when you do.
```

Capability `fs.read paths = ["${SKILL_DIR}/references/**"]` is substituted at spawn time by Skills-1's `${SKILL_DIR}` substitution + Skills-4's resolution. The bundled files exist at the install path; read at runtime via the agent's tool layer.

### `skills/pr-reviewer/`

`tau.toml`:
```toml
name = "pr-reviewer"
version = "0.1.0"
description = "Reviews git diffs against the project's coding style + finds nearby callers."
authors = ["tau contributors <dev@tau>"]
source = "https://github.com/LEBOCQTitouan/tau.git"
kind = "skill"
dependencies = []

[[capabilities]]
kind = "process.spawn"
commands = ["git", "rg"]
```

`SKILL.md` body documents the expected workflow:
```markdown
---
name: pr-reviewer
description: Reviews git diffs against the project's coding style + finds nearby callers.
---

You are a code reviewer for a Rust project. Workflow:

1. Run `git diff <base>...HEAD` to gather the proposed changes.
2. For each non-trivial change, use `rg <symbol>` to find nearby callers
   or related code the change might affect.
3. Render a review: what's well-considered, what's risky, what needs tests.

Be direct. Cite filenames + line numbers. Flag missing tests explicitly.
```

**Sandbox-tier compatibility:** `process.spawn` for `git` + `rg` works across all three tau sandbox tiers (passthrough / strict / container) per Skills-2's `sandbox_check`. The reference skill's CI smoke test runs in `passthrough` (simplest). Strict-tier validation is covered by tau-sandbox-native e2e tests separately.

## Documentation pages

### Tutorial: `docs/tutorials/build-your-first-skill.md`

Narrative walkthrough following a contributor as they:
1. Decide what their skill does
2. Write the SKILL.md frontmatter + body
3. Add `tau.toml` with kind="skill"
4. Add a fs.read capability for bundled context
5. `tau install ./my-skill` + `tau skill show my-skill`
6. (Optional) `tau skill export my-skill` for sharing with Anthropic ecosystem

Uses `critic` as the running example. Cross-links to how-to recipes for deeper dives.

### How-to: `docs/how-to/install-a-skill.md`

Recipe-style: covers `tau install <git-url>` (auto-detect Anthropic + tau formats), `tau skill import <src> --output <dir>` for inspect-before-install, and `tau install ./local-dir`.

### How-to: `docs/how-to/author-a-skill.md`

Recipe-style: covers minimal SKILL.md, adding capabilities, multi-file `${SKILL_DIR}` references, declaring `requires_skills` for sub-skill composition.

### How-to: `docs/how-to/export-a-skill.md`

Recipe-style: covers `tau skill export` semantics (drops capabilities + requires_skills), `--strict` flag, when to use export vs publishing the source repo directly.

### Reference: `docs/reference/skill-manifest-schema.md`

Complete schema for:
- `tau.toml` top-level fields when `kind = "skill"`
- `[skill]` block fields: `content`, `requires_tools`, `requires_skills`
- `SKILL.md` frontmatter requirements (`name`, `description` required; both validated)
- Capability shapes that work with skills (`fs.read`, `fs.write`, `fs.exec`, `net.http`, `process.spawn`, `agent.spawn`, `skill.spawn`, `task_list`, `plan`, `Custom`)
- `${SKILL_DIR}` substitution rules
- Lockfile entries (v6 schema; `LockedSkill` + `synthesized_from`)

Cross-links to ADRs 0025–0029 for design rationale.

### Explanation: `docs/explanation/two-layer-skills.md`

Architecture reasoning:
- Why two layers (SKILL.md + tau.toml)?
- Why not embed system_prompt in tau.toml directly?
- What does tau add over plain Anthropic skills?
- How does the export/import roundtrip preserve compatibility?

Anchors the design decisions across ADRs 0025–0029 in one narrative document for readers who want to understand the system holistically.

## Test plan

### `crates/tau-pkg/tests/install_reference_skills.rs` (4 tests)

| Test | Asserts |
|---|---|
| `install_critic_from_in_tree_path` | Install succeeds; lockfile has `critic@0.1.0`; install_path contains SKILL.md; capabilities empty |
| `install_fact_checker_preserves_references_dir` | `references/style-guide.md` + `references/common-claims.md` present in install_path; fs.read capability includes `${SKILL_DIR}/references/**` |
| `install_pr_reviewer_records_process_spawn_cap` | capabilities contain `process.spawn` with `commands = ["git", "rg"]` |
| `install_all_three_yields_three_lockfile_entries` | One lockfile contains all three; v6 schema; ordering stable |

### `crates/tau-cli/tests/reference_skills_e2e.rs` (5 tests)

| Test | Asserts |
|---|---|
| `tau_skill_list_shows_three_installed_references` | After installing all three, `tau skill list` lines up |
| `tau_skill_show_critic_renders_anthropic_compatible` | `tau skill show critic` displays expected fields, JSON output is round-trippable |
| `tau_skill_export_critic_is_byte_identical` | `export` → diff `skills/critic/SKILL.md` vs `./out/SKILL.md` is empty |
| `tau_skill_export_fact_checker_drops_capabilities_warns` | stderr warns about dropped `fs.read`; exit 0 (warning, not error) |
| `tau_skill_export_fact_checker_preserves_references` | Exported dir has `references/` subdir intact with both files |

**Total: 9 new tests.** Existing tests untouched.

**No CI changes.** New test binaries run under the existing `test-stable` matrix on Linux/macOS/Windows. mdBook auto-deploy already runs on PR via PR #67's workflow; new pages will appear after squash-merge.

## File structure

**New files:**

| Path | Status | LOC estimate |
|---|---|---|
| `skills/README.md` | Create | ~60 |
| `skills/critic/tau.toml` | Create | ~12 |
| `skills/critic/SKILL.md` | Create | ~25 |
| `skills/fact-checker/tau.toml` | Create | ~18 |
| `skills/fact-checker/SKILL.md` | Create | ~30 |
| `skills/fact-checker/references/style-guide.md` | Create | ~40 |
| `skills/fact-checker/references/common-claims.md` | Create | ~50 |
| `skills/pr-reviewer/tau.toml` | Create | ~16 |
| `skills/pr-reviewer/SKILL.md` | Create | ~30 |
| `docs/tutorials/build-your-first-skill.md` | Create | ~200 |
| `docs/how-to/install-a-skill.md` | Create | ~80 |
| `docs/how-to/author-a-skill.md` | Create | ~120 |
| `docs/how-to/export-a-skill.md` | Create | ~80 |
| `docs/reference/skill-manifest-schema.md` | Create | ~250 |
| `docs/explanation/two-layer-skills.md` | Create | ~200 |
| `crates/tau-pkg/tests/install_reference_skills.rs` | Create | ~180 |
| `crates/tau-cli/tests/reference_skills_e2e.rs` | Create | ~250 |
| `docs/decisions/0030-skills-reference-packages.md` | Create | ~90 |

**Modified files:**

| Path | Change |
|---|---|
| `docs/SUMMARY.md` | Insert links to the 6 new mdBook pages under their respective sections |

## Estimated effort

| Task | Subagent | Effort |
|---|---|---|
| T1: `skills/critic/` package | haiku | 0.25d |
| T2: `skills/fact-checker/` package (+ 2 reference files) | haiku | 0.5d |
| T3: `skills/pr-reviewer/` package | haiku | 0.25d |
| T4: `tau-pkg/tests/install_reference_skills.rs` (4 tests) | sonnet | 0.5d |
| T5: `tau-cli/tests/reference_skills_e2e.rs` (5 tests) | sonnet | 0.75d |
| T6: `docs/tutorials/build-your-first-skill.md` | sonnet | 0.75d |
| T7: `docs/how-to/{install,author,export}-a-skill.md` (3 recipes) | sonnet | 0.75d |
| T8: `docs/reference/skill-manifest-schema.md` | sonnet | 0.5d |
| T9: `docs/explanation/two-layer-skills.md` + `docs/SUMMARY.md` updates | sonnet | 0.5d |
| T10: `skills/README.md` index | haiku | 0.25d |
| T11: ADR-0030 | haiku | 0.25d |
| T12: USER GATE — push + open PR + monitor CI | main | — |

**Total: ~5 days, 12 tasks.** Slightly above the 3-4d priority queue estimate due to D1 (3 skills vs 2) + D3 (full Diátaxis vs partial).

## Considered and rejected

- **1-skill scope (just critic).** Rejected: too minimal; doesn't prove tau's capability story.
- **Refactor existing test fixtures to use `skills/<name>/`.** Rejected: 10+ files touched, refactor risk, merge-conflict risk with parallel Claude sessions, marginal benefit.
- **Separate `tau-skills` git repo.** Rejected: in-tree is fine for proof-of-concept; external repo addressable when versioning needs to decouple from tau.
- **In-repo README only (no mdBook authoring).** Rejected: under-invests in the user-facing surface; mdBook is already wired and auto-deploys, so the marginal cost of the four Diátaxis pages is modest.
- **Tutorial-only docs (no reference/explanation).** Rejected: reference is load-bearing for users authoring their own skills; explanation anchors the design choices for readers who want depth.
- **`tau skill new <name>` scaffolding command.** Rejected: useful but separate sub-project; not blocking the user story Skills-6 ships.

## Out of scope (post Skills track)

- **`tau-skills` external repo** for community-contributed skills — address when external versioning needs emerge.
- **Additional reference skills** (`summarizer`, `editor`, `commit-message-writer`, etc.) — additive; future PRs.
- **`tau skill new <name>` bootstrap helper** — separate small sub-project.
- **Skill marketplace / registry** — premature; not in ROADMAP §16.
- **mdBook live execution / playground** — would require mdBook plugins + sandboxed exec; out of scope.

## References

- Spec: this document
- Implementation plan: `docs/superpowers/plans/2026-05-16-skills-6-reference-packages.md` (to be written next)
- ADR (pending): `docs/decisions/0030-skills-reference-packages.md`
- Predecessor specs:
  - Skills-1: `docs/superpowers/specs/2026-05-12-skills-1-manifest-design.md`
  - Skills-2: `docs/superpowers/specs/2026-05-12-skills-2-install-pipeline-design.md`
  - Skills-3: `docs/superpowers/specs/2026-05-13-skills-3-discovery-design.md`
  - Skills-4: `docs/superpowers/specs/2026-05-14-skills-4-runtime-invocation-design.md`
  - Skills-5: `docs/superpowers/specs/2026-05-15-skills-5-anthropic-interop-design.md`
- Predecessor ADRs: 0025 (foundation), 0026 (install pipeline), 0027 (discovery), 0028 (runtime invocation), 0029 (Anthropic interop)
- ROADMAP §16
- Priority queue: `docs/superpowers/specs/2026-05-12-post-multi-agent-priority-queue.md`
- mdBook deploy workflow: PR #67 (commit `c620794`)
