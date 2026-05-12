# Skills-1: Manifest extension — design

## Context

ROADMAP §16 ("Skills as first-class packages", Constitution G10)
decomposed into 6 sub-projects in
[`2026-05-12-post-multi-agent-priority-queue.md`](2026-05-12-post-multi-agent-priority-queue.md).
This is the first: **manifest extension**. Foundation for the rest.

The runtime precondition shipped in v1.2 (commit `e400688`,
multi-agent v1.2 PR): the spawn arg `system_prompt: Option<String>`
lets a caller override a child agent's prompt at spawn time. Skills-1
extends the *package* layer with a typed declaration of "this package
ships a skill," giving the runtime a way to look up that prompt by
skill name instead of requiring the caller to supply it inline.

## Decision (locked during brainstorm)

A tau skill package is **a directory containing both an Anthropic-
format `SKILL.md` and a tau-format `tau.toml`**. The two layers cleanly
divide responsibility:

- **`SKILL.md`** — content. Pure Anthropic skill format (YAML
  frontmatter + Markdown body). Bit-identical to what
  claude-code / claude.ai / any compliant runtime expects. Zero
  tau-specific extensions in this file.
- **`tau.toml`** — packaging. Capability declaration, tool / skill
  dependencies, version, source. The same shape as a plugin or any
  other tau package, with a new `[skill]` block.

A tau skill IS an Anthropic skill (strip `tau.toml`, ship to
claude-code). An Anthropic skill becomes a tau skill by adding
`tau.toml` (declare capabilities + version + dependencies). No
translation; no two-spec divergence.

This option was reached after rejecting three alternatives during the
brainstorm — see the "Considered and rejected" section below.

## Manifest shape

### `tau.toml`

A skill package's `tau.toml` looks exactly like a plugin or tool
package's `tau.toml`, with three differences:

1. `kind = "skill"` (the existing `kinds::SKILL` constant in
   tau-domain).
2. A new optional `[skill]` block declaring skill-specific fields.
3. A new optional `[[skill.requires_skills]]` array for sub-skill
   composition (resolved at install via the existing `requires_tools`
   pipeline).

```toml
name = "critic"
version = "0.1.0"
description = "Reviews drafts for unsourced claims."
kind = "skill"

# Capabilities the skill needs at runtime.
# Subject to the v1.1 capability subset law: parent's grant ⊆ skill's
# declared capabilities is the precondition at spawn time.
[[capabilities]]
kind = "fs.read"
paths = ["${SKILL_DIR}/references/**", "${SKILL_DIR}/templates/**"]

[[capabilities]]
kind = "task_list"
mode = "write"

[skill]
# Path within the package to the SKILL.md file.
# Defaults to "SKILL.md" if omitted.
content = "SKILL.md"

# Optional: tools the skill expects at invocation time.
# Same shape as the existing top-level [[requires.tools]] entries.
[[skill.requires_tools]]
name = "fs-read"
source = "git+https://github.com/example/fs-read"
version = "^0.1"

# Optional: sub-skills the skill may invoke via agent.<kind>.spawn.
# Resolved at install; Skills-4 (runtime invocation) wires the
# agent-side dispatch.
[[skill.requires_skills]]
name = "fact-checker"
source = "git+https://github.com/example/fact-checker"
version = "^0.1"
```

### `SKILL.md`

Pure Anthropic format. YAML frontmatter MUST contain `name` and
`description`; `name` MUST equal the `name` field in `tau.toml`
(validated at install).

```markdown
---
name: critic
description: Reviews drafts for unsourced claims. Use after a draft section is complete; invoke with the draft path. Returns terse Markdown bullets.
---

# Critic

You are a strict editor. Flag every claim that lacks a source.

## How you work
1. Read the draft at the path provided in the user message
2. For each paragraph: identify any factual claim
3. For each claim: is there a citation in the same paragraph, or a
   "see ..." pointer?
4. Emit one Markdown bullet per missing source
5. Be terse — bullets only, no preamble

## Style guide
For examples of what counts as a "claim," read
${SKILL_DIR}/references/style-guide.md before starting.
```

The body is what becomes the spawned child's `system_prompt`. The
frontmatter is metadata used by `tau skill list` / `tau skill show`
(Skills-3) and for validation (name must match).

## The `${SKILL_DIR}` interpolation variable

A new tau interpolation variable, parallel to the existing
`${SCOPE}` and `${PROJECT}` variables. Expands at install time to the
absolute path of the installed skill directory:
`<scope>/.tau/skills/<name>/`.

Used in:
- `[[capabilities]]` `paths` entries (so the skill can declare
  fs.read access to its own references / templates folder).
- The `SKILL.md` body (so the agent's instructions can reference
  files by absolute path that survives install relocation).

Resolution rule: `${SKILL_DIR}` resolves to the directory containing
the skill's `tau.toml` after installation. Validated at install time
(the directory must exist); rejected at parse time if used in a
context where the variable isn't defined (e.g. used in a non-skill
package's `tau.toml`).

## Validation pipeline

At install time (`tau install <skill-pkg>`), the pipeline runs three
checks beyond standard package validation:

1. **`SKILL.md` exists.** Path comes from `[skill] content`
   (default `"SKILL.md"`). Missing → `InstallError::SkillContentMissing`.
2. **Frontmatter parses + name matches.** YAML frontmatter MUST parse;
   `name` field MUST equal `tau.toml`'s `name`. Mismatch →
   `InstallError::SkillNameMismatch { tau_toml: ..., skill_md: ... }`.
3. **Reference lint.** Optional but warn-emitting: if `SKILL.md` body
   contains `${SKILL_DIR}/<rel-path>` references AND the relative
   path is inside the package but `[[capabilities]] kind = "fs.read"`
   doesn't include a glob covering it, emit
   `tracing::warn!("skill {name}: SKILL.md references ${{SKILL_DIR}}/{path} but no fs.read capability grants access to it")`.
   Not a hard error (false positives are likely — the agent may not
   actually need fs.read if the reference is for human readers).

## Types added to tau-domain

```rust
// crates/tau-domain/src/package/skill.rs (new file)

/// Skill-specific manifest block (parsed from `[skill]` in tau.toml).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SkillManifest {
    /// Path to the SKILL.md content file, relative to the package
    /// root. Defaults to "SKILL.md".
    #[cfg_attr(feature = "serde", serde(default = "default_skill_content"))]
    pub content: String,

    /// Tool dependencies (same shape as the top-level
    /// `[[requires.tools]]`).
    #[cfg_attr(feature = "serde", serde(default, rename = "requires_tools"))]
    pub requires_tools: Vec<PackageDep>,

    /// Sub-skill dependencies.
    #[cfg_attr(feature = "serde", serde(default, rename = "requires_skills"))]
    pub requires_skills: Vec<PackageDep>,
}

fn default_skill_content() -> String {
    "SKILL.md".to_string()
}

/// Parsed YAML frontmatter from a SKILL.md file.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
}

/// Parsed SKILL.md content: frontmatter + body.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillContent {
    pub frontmatter: SkillFrontmatter,
    /// The Markdown body (becomes the spawned child's system_prompt).
    pub body: String,
}
```

`PackageManifest` gains an optional `skill: Option<SkillManifest>`
field (parallel to the existing `plugin: Option<PluginManifest>`).
Accessor: `manifest.skill() -> Option<&SkillManifest>`.

## tau-pkg integration

The install pipeline (`tau_pkg::install_with_options`) gains a step
that runs only for `kind = "skill"` packages, between manifest
validation (step 4-ish) and lockfile write:

1. Read `SKILL.md` from `<install_dir>/<content_path>`.
2. Parse frontmatter (likely via the existing `yaml` workspace dep
   if there is one; otherwise add a small `gray_matter`-style
   parser).
3. Validate name match.
4. Lint references vs declared fs.read capabilities (warn-only).
5. Store `SkillContent` in the lockfile? **No** — lockfile schema
   stays minimal. The runtime re-reads `SKILL.md` at spawn time
   (Skills-4). This avoids drift between install-time snapshot and
   on-disk source, and keeps lockfile small.

## Lockfile

No schema migration needed for Skills-1. The lockfile already
records installed packages with their kind; `kind = "skill"` is just
another value. Skills-2 (install pipeline integration) may surface
additional fields, but Skills-1 itself doesn't add any.

## Testing

- **tau-domain `SkillManifest` round-trip tests** — serde parse +
  serialize through `tau.toml` snippets. ~4 tests covering: minimal
  manifest, full manifest with requires_tools + requires_skills,
  default `content = "SKILL.md"`, malformed → ParseError.
- **tau-domain `SkillContent` parsing** — YAML frontmatter + body
  extraction. ~4 tests: valid frontmatter, missing frontmatter,
  malformed YAML, frontmatter without required `name` field.
- **tau-pkg install integration** — fixture-based: a minimal critic
  skill package with `tau.toml` + `SKILL.md` installs cleanly;
  `SkillContentMissing` triggers when `SKILL.md` is absent;
  `SkillNameMismatch` triggers when frontmatter name diverges from
  tau.toml. ~3 tests.
- **`${SKILL_DIR}` interpolation** — install a skill, verify capability
  paths resolve to absolute paths. ~2 tests.

## What this does NOT cover (subsequent sub-projects)

- **Skills-2: Install pipeline.** Wiring `tau install <skill-pkg>`
  end-to-end through tau-pkg + lockfile. Skills-1 adds the parser +
  validators; Skills-2 wires the install machinery to use them.
- **Skills-3: Discovery.** `tau skill list` + `tau skill show`.
- **Skills-4: Runtime invocation.** Resolving `agent.<skill-name>.spawn`
  to the installed skill manifest. Runtime reads `SKILL.md` body →
  becomes child's `system_prompt`. Uses capabilities from `tau.toml`
  as the child's default grant (caller may further narrow per the
  capability subset law).
- **Skills-5: Agent Skills spec compliance.** Conformance testing
  against the canonical 2026-ecosystem spec; possibly an export
  command emitting tau skills in the canonical wire format.
- **Skills-6: Reference skill packages + docs.** Two or three
  exemplary skill packages shipped as test fixtures.

## Estimated effort

3-4 days as a focused sub-project. Components:

- New `tau-domain::package::skill` module + 8 unit tests
- `PackageManifest::skill()` accessor + serde plumbing in
  `manifest.rs`
- New `${SKILL_DIR}` interpolation hook in
  `tau-domain::package::interpolation` (or wherever existing
  variables live)
- tau-pkg install-step extension + 3 integration tests
- ADR-0025 documenting the two-layer design + Anthropic format
  alignment

## Considered and rejected

During the brainstorm, three alternative shapes were considered
before landing on Option D (two-layer, Anthropic-content + tau-
packaging):

- **Option A — Typed `[skill]` block embedding system_prompt
  inline.** Rejected: TOML triple-quoted strings are awkward for
  long prompts; the format diverges from the Anthropic ecosystem;
  no portability.
- **Option B — Metadata-only `[skill]` + separate prompt file in a
  tau-specific layout.** Rejected: invents a layout convention that
  diverges from Anthropic skills for no benefit. Option D is
  Option B except the layout convention IS the Anthropic format.
- **Option C — Adopt the Agent Skills spec format verbatim
  (skill.json or similar).** Rejected as the *only* manifest: would
  require either replacing tau.toml or splitting capability
  declarations across two manifest files. Option D adopts the
  Anthropic format for the content (`SKILL.md`) while keeping tau's
  packaging machinery (capabilities, tool deps, lockfile) in
  tau.toml — best of both.

The reframe that produced Option D: "Anthropic's skill format ALREADY
solved the canonical-content question. Tau's job is to be the
package manager around it, not to redesign the content layer."

## ADR

ADR-0025 will document this decision once Skills-1 ships. Pending
items for the ADR: the rejected alternatives + their dates, the
multi-file directory layout convention, and the `${SKILL_DIR}`
variable as a public package-system primitive.
