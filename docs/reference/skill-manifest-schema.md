# Skill manifest schema

This page is the complete reference for tau skill packages: the
`tau.toml` `[skill]` block, the `SKILL.md` frontmatter requirements,
the capabilities accepted on `kind = "skill"` packages, the
`${SKILL_DIR}` substitution rules, and the lockfile entries.

For background and design rationale, see
[Explanation: two-layer skills](../explanation/two-layer-skills.md)
and ADR-0025 through ADR-0030.

## Package layout

A tau skill package is a directory containing:

| File / path | Required | Purpose |
|---|---|---|
| `tau.toml` | Yes (tau-native) | Package manifest |
| `SKILL.md` | Yes | System prompt (Anthropic format) |
| `<other files>` | No | Bundled payload, accessible at `${SKILL_DIR}/...` |

If the directory has only `SKILL.md` (no `tau.toml`), `tau install`
auto-detects the Anthropic format and synthesizes a `tau.toml` in
memory + on disk. See [How-to: install a skill](../how-to/install-a-skill.md)
for the user surface.

## `tau.toml` top-level fields (`kind = "skill"`)

| Field | Type | Required | Notes |
|---|---|---|---|
| `name` | string | Yes | Must match `SKILL.md` frontmatter `name`. ASCII lowercase, digits, `-`. |
| `version` | string (semver) | Yes | E.g., `"0.1.0"`. |
| `description` | string | Yes | Non-empty. Should match `SKILL.md` frontmatter `description`. |
| `authors` | list of strings | Yes | May be empty. |
| `source` | URL or `local://...` | Yes | Origin. `https://`, `git@`, `file://`, or `local://<name>` for in-tree. |
| `kind` | string | Yes | Must be `"skill"`. |
| `dependencies` | list of `PackageDep` | Yes | May be empty. |
| `capabilities` | list of capability tables | Yes | May be empty. See below. |
| `[skill]` | table | Yes | Skill-specific block (see below). |

## `[skill]` block

| Field | Type | Default | Purpose |
|---|---|---|---|
| `content` | string | `"SKILL.md"` | Path to the SKILL.md content file, relative to the package root. |
| `requires_tools` | list of `PackageDep` | `[]` | Tool dependencies (not yet enforced at runtime; advisory). |
| `requires_skills` | list of `PackageDep` | `[]` | Sub-skill dependencies (not yet enforced at runtime; advisory). |

Example:

    [skill]
    content = "SKILL.md"

    [[skill.requires_skills]]
    name = "fact-checker"
    version_req = "^0.1"

## `SKILL.md` frontmatter

YAML frontmatter delimited by `---` lines. Body is everything after
the closing `---`.

| Field | Required | Notes |
|---|---|---|
| `name` | Yes | Must match `tau.toml` `name`. |
| `description` | Yes | Non-empty. |

Other fields are tolerated and discarded by tau v1.

Example:

    ---
    name: critic
    description: Reviews drafts for clarity, completeness, and rhetorical quality.
    ---

    You are a writing critic. ...

## Capability shapes for skill packages

| `kind` | Fields | Purpose |
|---|---|---|
| `fs.read` | `paths: [...]`, optionally `max_bytes` | Read files matching the path globs. |
| `fs.write` | `paths: [...]`, optionally `max_bytes` | Write files matching the path globs. |
| `fs.exec` | `paths: [...]` | Execute binaries matching the path globs. |
| `net.http` | `hosts: [...]`, `methods: [...]` | HTTP requests to allowed hosts + methods. |
| `process.spawn` | `commands: [...]` | Spawn the named processes (resolved via `PATH`). |
| `agent.spawn` | `allowed_kinds: [...]` | Spawn child agents of the named kinds. |
| `skill.spawn` | `allowed_skills: [...]` | Spawn child agents from installed skills. |
| `task_list` | `mode: "read" \| "write" \| "manage"` | TaskList virtual-tool access. |
| `plan` | `mode: "read" \| "write"` | Plan virtual-tool access. |
| `Custom` | `name: ...`, `params: { ... }` | Plugin-defined capability. |

All capability blocks are TOML array-of-tables:

    [[capabilities]]
    kind = "fs.read"
    paths = ["${SKILL_DIR}/references/**"]

    [[capabilities]]
    kind = "process.spawn"
    commands = ["git", "rg"]

## `${SKILL_DIR}` substitution

The literal string `${SKILL_DIR}` in any `paths` field is substituted
at spawn time with the absolute path to the skill's install
directory (e.g., `<scope>/.tau/packages/<name>/<version>/`).

Substitution applies to:

- `fs.read paths`
- `fs.write paths`
- `fs.exec paths`

It does NOT apply to:

- `net.http hosts`
- `process.spawn commands` (which are resolved via PATH, not absolute paths)
- `Custom params` (plugin-defined; opt in if the plugin supports it)

## Lockfile entries

After installation, the package appears in the project's
`tau-lock.toml` as a `[[package]]` entry with a `[package.skill]` block.

    [[package]]
    name = "critic"
    active_version = "0.1.0"
    source = "https://github.com/..."

    [package.skill]
    content_sha256 = "<64-char hex>"

    [package.skill.frontmatter]
    name = "critic"
    description = "Reviews drafts ..."

    [[package.versions]]
    version = "0.1.0"
    resolved_commit = "..."
    sha256 = "..."
    installed_at = "..."

For skills synthesized from Anthropic-format sources, the
`synthesized_from` field appears at the package level:

    [[package]]
    name = "imported-skill"
    active_version = "0.1.0"
    source = "https://github.com/anthropic-author/imported-skill.git"
    synthesized_from = "anthropic"

    ...

`synthesized_from` is `Some("anthropic")` when `tau install`
auto-detected Anthropic format (no tau.toml in source). It is `None`
for tau-native packages (tau.toml present in source).

## Lockfile schema versioning

| Version | Introduced | Skills change |
|---|---|---|
| v4 | Pre-Skills | (no skill data) |
| v5 | Skills-2 | `[package.skill]` block (content_sha256 + frontmatter snapshot) |
| v6 | Skills-5 | `synthesized_from: Option<SynthesizedSource>` provenance |

The current schema version is **v6** (Skills-5).
`MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION` in `crates/tau-pkg/src/lockfile.rs`.

## Cross-references

- ADR-0025: foundation + two-layer architecture
- ADR-0026: install pipeline + lockfile v5
- ADR-0027: discovery (`tau skill list/show`)
- ADR-0028: runtime invocation (`skill.<name>.spawn`)
- ADR-0029: Anthropic interop + lockfile v6
- ADR-0030: reference packages + docs (this sub-project)
