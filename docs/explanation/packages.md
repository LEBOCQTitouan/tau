# Packages: the unit of extension in tau

> "Tau installs and runs agents in the terminal. Everything else —
> models, tools, skills, pipelines — is a package."
>
> — *Constitution* (compressed thesis)

A *package* is the only way to add functionality to tau. There is no
plugin discovery directory, no env-var "load this DLL," no bundled
official content. A new LLM backend, a new tool, a new skill, a new
pipeline — they all arrive through one verb: **`tau install`**.

This page explains the model: what a package is, what kinds exist,
where they live, and what the install → lock → verify → run lifecycle
buys you. Pages elsewhere in the book — [Build your first
skill](../tutorials/build-your-first-skill.md), [Install a
skill](../how-to/install-a-skill.md), [the manifest
schema](../reference/skill-manifest-schema.md) — assume you have read
this one.

## Why packages are the only extension surface

Core does four things (G1): install packages, run agents, pass
messages, observe. Anything domain-specific — *what model do I talk
to, what tool can the agent run, what pipeline orchestrates several
agents* — must come from outside core. G2 makes the rule concrete:
**everything domain-specific is a package**. G7 closes the loophole:
**the package manager is the only way to add extensions**.

Two consequences:

- **Core ships empty** (G11). The first time you use tau, the next
  step is `tau install <something>`. Nothing is preinstalled. There
  is no privileged "official" content.
- **One audit surface.** Capabilities, dependencies, sandbox
  requirements, content hashes — they're all declared in one place
  (the manifest) and enforced through one code path (the runtime).
  Env vars and magic directories would fracture this.

If you find yourself wanting to add functionality without `tau
install`, the answer is *don't* — package it instead.

## What a package is

A package is a manifest plus the artifacts the manifest describes:

- **`tau.toml`** — the manifest. Declares name, version, kind, source,
  dependencies, capabilities, sandbox requirements, and any
  kind-specific blocks. ADR-0002 pins the field set and serialization.
- **Artifacts** — the actual content. For an LLM-backend or tool, that
  is a compiled plugin binary (a dynamic library exposing the runtime
  ABI; see ADR-0008). For a skill, that is a `SKILL.md` plus bundled
  files (see [Two-layer skills](two-layer-skills.md)).

The manifest is the contract. The runtime never trusts a package
beyond what the manifest declares — capabilities not listed are not
granted, sandbox tiers not declared are not relaxed.

## The seven kinds

tau-domain reserves seven `kind` strings (`crates/tau-domain` →
`package::kinds`). Each one names a role the package fulfils in the
runtime:

| Kind | What it provides | Examples (planned / shipped) |
|---|---|---|
| `llm-backend` | An implementation of the LLM trait — sends messages, receives streams. Required for any agent to run (G4 + ADR-0006). | `@tau/anthropic`, `@tau/openai`, `@tau/ollama` |
| `tool` | Callable functionality an agent can invoke (fs.read, shell, http). Tools live behind capability gates. | `@tau/fs`, `@tau/shell` |
| `skill` | A reusable behaviour (an instruction document + optional bundled files + optional sub-tool refs). See [Two-layer skills](two-layer-skills.md). | `critic`, `pr-reviewer`, your own |
| `pipeline` | Coordinates multiple agents through a methodology. *Not* a general-purpose workflow engine (NG5). | `stature` (downstream) |
| `mcp-server` | A Model Context Protocol server exposed to agents as a tool surface (G10). | any MCP server, wrapped |
| `storage` | A persistence backend for state plugins need beyond a single invocation (memory plugins, log sinks). | future |
| `sandbox` | An adapter implementing the sandbox trait (landlock, sandbox-exec, AppContainer, container). | `tau-sandbox-native`, `-darwin`, `-windows` |

The kind is not just metadata — the runtime resolves what trait
the package must implement from it. A `kind = "llm-backend"` package
is loaded as an `LlmBackend`; a `kind = "tool"` package is loaded as
a `Tool`. ADR-0008 walks through the loading mechanism.

There is an escape hatch: `PackageKind::Custom { kind: String }`. Use
it when prototyping a new role that core does not yet recognise.
Custom kinds do not get a trait implementation — the runtime won't
*do* anything with them on its own. The escape-hatch registry tracks
every such variant ([escape-hatches](escape-hatches.md)).

## Where packages come from: sources

The manifest's `source` field declares where the package lives.
`PackageSource` has one variant today — `Git { location, rev }` — but
that variant covers every install mode you've seen in the how-to
recipes:

```toml
# Hosted git
source = "https://github.com/example/skill-praise-poet.git"

# Pinned to a revision (branch, tag, or SHA)
source = { git = "https://github.com/example/skill-praise-poet.git", rev = "v0.3.1" }

# scp-style git address
source = "git@github.com:example/skill-praise-poet.git"

# Local checkout via file:// (the "local install" case)
source = "file:///Users/you/work/my-skill"
```

A "local install" is not a separate source kind — it's a `file://`
URL that points at a working tree on disk. tau-pkg clones from it the
same way it clones from `https://`. This is deliberate: one source
type, one install path, one verification surface.

The dependency graph is resolved through the same field. `tau install
<git-url>` reads the manifest, walks `dependencies[]`, fetches each
transitively, and writes the resolved set into a lockfile.

## Scopes: global vs project

Where a package is *installed* is independent of where it comes
from. tau supports two scopes (G8):

- **Global** (`Scope::Global`) — defaults to `~/.tau` (or
  `$TAU_HOME` / `$XDG_DATA_HOME/tau`). Installs here are
  user-wide; available in any shell.
- **Project** (`Scope::Project`) — a `.tau/` directory in the
  project's source tree. Detected by walking up from cwd.

Project scope **overrides** global when both apply: if the same
package is installed in both, the project version wins for invocations
inside that project. This makes "Claude-like personal use" (everything
global) and "ECC-like project use" (everything pinned in
`.tau/`-as-a-lockfile) both first-class. You don't pick one model;
you mix them per package.

Each scope owns its own:

- `lockfile.toml` — the resolved dependency graph + content hashes.
- `config.toml` — including the `[sandbox]` block: minimum tier
  required for any plugin spawned in this scope.
- `packages/<name>/` — the actual installed artifacts.

## The install lifecycle: install → lock → verify → run

A successful `tau install <source>` performs roughly the following:

1. **Clone** the source into a temp location (tau-pkg).
2. **Parse + validate** `tau.toml` against the v0.1 schema (tau-domain).
3. **Cross-check** against the scope's sandbox config — the package's
   declared `[sandbox] required_tier` must be installable here
   (ADR-0016).
4. **Resolve dependencies** recursively, fetching each. Cycles are an
   error; conflicts surface a chosen pin per ADR-0005.
5. **Hash content**, write into `packages/<name>/`, record the entry
   in the lockfile (ADR-0026 for skills, equivalent for other kinds).
6. **Prompt consent** for the declared capabilities (G14). The user
   sees what the package claims to do *before* it lands.

After install, `tau verify` re-hashes the installed content against
the lockfile to catch drift (a third party editing
`~/.tau/packages/.../SKILL.md`, or a corrupted git fetch). `tau
update` re-resolves with newer compatible versions. `tau uninstall`
removes the package and its lockfile entries.

At runtime, the loader resolves a package reference (e.g. an
agent's `llm_backend = "@tau/anthropic"`) against the lockfile,
spawns the plugin process under the sandbox tier the package
declared, and routes messages through the runtime. The plugin
never sees credentials directly (G13) and cannot exceed its declared
capability set (G12 + G14).

## What this rules out

The package model is the constraint that gives tau its shape. Several
common designs are *excluded* by it:

- **No "officially blessed" packages.** Tau does not curate, rank,
  feature, or moderate (NG4). Reach for `@tau/anthropic` because you
  trust it, not because tau marks it as canonical.
- **No bundled subsystems.** "Tau ships with an LLM client" would be
  G11 violation. The runtime knows the *trait*; the wire is the
  package's job.
- **No "drop-in" loading.** Putting a `.so` in a magic directory does
  not extend tau (G7). The lockfile is the only source of truth for
  what's installed.
- **No silent capability escalation.** A skill that wants to spawn a
  shell *declares* `process.spawn` and the user *consents* at install
  time. Adding the capability later requires reinstalling, which
  re-prompts (ADR-0014 + ADR-0015).

## When to write a package vs. a skill vs. a tool

Three rules of thumb:

- **Need to call something the model can't do?** Write a `tool` (or
  use an MCP server through `mcp-server`).
- **Need to give the model a reusable instruction with optional
  files?** Write a `skill` — far cheaper than a tool, no Rust
  required.
- **Need to coordinate multiple agents through a methodology?**
  That's a `pipeline`. *Not* a workflow engine — coordination of
  agents specifically (NG5).

If your need is "I want a button in a GUI" — tau is not the layer for
that (G3). If it's "I want long-term agent memory" — that's not in
core (NG6); a `storage` package or an LLM-backend feature is the
answer.

## See also

- [`CONSTITUTION.md`](../../CONSTITUTION.md) — G1–G17 define the
  identity, NG1–NG12 the non-goals.
- [ADR-0002](../decisions/0002-manifest-format.md) — the manifest
  field set and the canonicalization rules.
- [ADR-0004](../decisions/0004-tau-pkg.md) — package manager design.
- [ADR-0008](../decisions/0008-plugin-loading.md) — how a package
  becomes a running plugin.
- [ADR-0014](../decisions/0014-sandboxing.md) /
  [ADR-0015](../decisions/0015-sandbox-activation.md) — the sandbox
  tier model and how install-time declaration becomes runtime
  enforcement.
- [Two-layer skills](two-layer-skills.md) — the skill-specific shape
  on top of the package model.
- [Escape hatches](escape-hatches.md) — the registry of every
  `Custom` and `InternalError` variant in core.
