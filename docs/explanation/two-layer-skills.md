# Why two-layer skills

A tau skill is two files in a directory: `SKILL.md` (the prompt)
and `tau.toml` (the manifest). This isn't an obvious choice — most
agent frameworks embed both in a single file. This page explains
why tau picked the two-layer split, what trade-offs it locked in,
and how it interacts with the broader Anthropic Agent Skills
ecosystem.

For the surface details, see
[Reference: skill manifest schema](../reference/skill-manifest-schema.md).
For the design history, see ADR-0025.

## The reframing that produced Option D

When Skills-1 was designed, four shapes were on the table:

- **A:** Typed `[skill]` block in `tau.toml`, with the system prompt
  embedded as a TOML triple-quoted string.
- **B:** External `SKILL.md` referenced by relative path from
  `tau.toml`, with a tau-specific filename convention.
- **C:** Adopt the Agent Skills spec format verbatim (just SKILL.md,
  no tau.toml — drop tau-specific packaging).
- **D:** Two-layer: Anthropic-compatible SKILL.md (frontmatter + body)
  PLUS tau.toml for packaging metadata + capabilities.

Option D won. The reframing: **Anthropic's skill format already
defines the prompt layout we want. We don't need to compete with it
on prompt encoding; we need to extend it with packaging.**

The result is a skill directory that's simultaneously:

- A valid Anthropic Agent Skill (just remove `tau.toml` to ship to
  claude-code or another Anthropic consumer).
- A tau package with capabilities, semver, dependencies, lockfile
  participation.

## What each layer owns

**SKILL.md owns: the prompt content.**

- The YAML frontmatter declares `name` and `description` (both
  required, both validated).
- The Markdown body is the system prompt. It becomes the spawned
  child agent's `system_prompt` at runtime (per ADR-0028).
- Format is fixed by the Anthropic Agent Skills spec; tau doesn't
  extend it.

**tau.toml owns: everything else.**

- Package identity: `name`, `version`, `source`, `authors`.
- Capability declarations (`[[capabilities]]` blocks).
- Sub-skill dependencies (`[skill.requires_skills]`).
- Package kind (`kind = "skill"` — disambiguates from `kind = "tool"`
  / `kind = "llm-backend"`).

The two layers must agree on `name` (validated on install).
Everything else is independent: tau can add `[skill.requires_tools]`,
`requires_skills`, future fields, without touching SKILL.md.

## The roundtrip claim

For any tau skill with `capabilities = []` and no `requires_skills`:

    tau install <source> → tau skill export <name> → re-install

produces an identical SKILL.md byte-for-byte (and the same `name` +
`description`). Skills-5 ships this as `tests/skill_format_roundtrip.rs`.

For capability-bearing skills, the export is one-way (Anthropic
format doesn't preserve capabilities). `tau skill export --strict`
makes the metadata-drop a hard error if round-trippability matters.

The roundtrip claim is what makes the two-layer architecture
worth it: tau can extend the Anthropic format without forking it.

## What this rules out

Two consequences worth being explicit about:

**1. We can't bake tau-specific behavior into SKILL.md.** No tau-
specific YAML frontmatter extensions (e.g., `x-tau-capabilities`),
no tau-flavored Markdown syntax. Why: any such extension would break
the byte-identical roundtrip claim for pure-prompt skills, and the
Anthropic ecosystem ignores unknown YAML keys anyway so there's no
benefit.

Skills-5 explicitly rejected an `x-tau-capabilities` YAML extension
for this reason.

**2. We can't combine tau.toml and SKILL.md into a single file.** A
skill is fundamentally a directory of files. The `${SKILL_DIR}/...`
substitution + multi-file payloads (e.g., `references/` subdirs)
depend on the directory being the unit of distribution. Embedding
everything in tau.toml would lose that.

## Comparison with neighboring systems

| System | Prompt encoding | Packaging |
|---|---|---|
| Anthropic Agent Skills (vanilla) | SKILL.md (YAML + Markdown) | none (just the directory) |
| tau | SKILL.md (same) | tau.toml (additive) |
| Single-file agents (e.g., `.prompt` files) | one TOML / YAML / JSON file | none |
| Plugin-style (Python decorators, MCP servers) | code-embedded | language-native imports |

tau differs from each:

- vs Anthropic vanilla: gains semver, capabilities, lockfile.
- vs single-file: gains multi-file payloads, structured packaging.
- vs plugin-style: gains LLM-readable prompts (skills aren't code).

## Sub-skill composition (currently advisory)

The `[skill.requires_skills]` block lets a skill declare that it's
meant to be used with another skill:

    [[skill.requires_skills]]
    name = "fact-checker"
    version_req = "^0.1"

In tau v1, this is **advisory**. The runtime doesn't:

- Auto-install dependencies on `tau install`.
- Auto-spawn sub-skills when the parent is invoked.
- Enforce that the dependency is present at spawn time.

It documents the relationship for users authoring agents (so they
can install the dependencies + grant the parent skill `skill.spawn`
authorization for them).

A future Skills sub-project may tighten this if a concrete use case
emerges.

## When NOT to write a skill

A skill is the right shape when:

- The work is purely prompt-driven (LLM reads input → emits output).
- The skill has clear boundaries (one purpose, one entry point).
- The capability set is small (one or two fs / process / net access).

A skill is the WRONG shape when:

- The work requires real code (compute, parsing, custom logic). Use
  a tool plugin instead (`kind = "tool"`).
- The work requires many capabilities or long chains of tool calls.
  Compose smaller skills + tools instead.
- The work doesn't generalize beyond a single project. Just inline
  the prompt in your agent definition.

The skill abstraction earns its keep when it's installable, named,
versioned, and reusable across agents.

## Further reading

- ADR-0025: foundation
- ADR-0028: runtime invocation
- ADR-0029: Anthropic interop
- Tutorial: [Build your first skill](../tutorials/build-your-first-skill.md)
- Reference: [Skill manifest schema](../reference/skill-manifest-schema.md)
