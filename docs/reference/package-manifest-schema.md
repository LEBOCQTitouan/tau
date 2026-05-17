# Package manifest schema

Reference for `tau.toml`, the manifest every tau package ships. Read
[Packages](../explanation/packages.md) first for the model — this page
is the precise field-by-field schema. The skill-specific
[`[skill]` block reference](skill-manifest-schema.md) covers that
sub-section separately.

The authoritative source is `crates/tau-domain/src/package/`. This
page tracks v0.1 of the schema (ADR-0002). Adding fields is
non-breaking (the structs are `#[non_exhaustive]`); removing or
renaming is a pre-1.0 minor breaking change per QG11.

## Overview

```toml
# Required top-level fields
name        = "fs-tools"
version     = "0.3.0"
description = "Filesystem read/write tools."
source      = "https://github.com/example/fs-tools.git"
kind        = "tool"

# Optional top-level
authors      = ["Example Org <hi@example.org>"]
license      = "Apache-2.0 OR MIT"
dependencies = []

# Optional tables
[plugin]
provides = "tool"
kind     = "rust-cargo"
bin      = "fs-tools-plugin"

[sandbox]
required_tier   = "strict"
required_shapes = []

[[capabilities]]
kind  = "fs.read"
paths = ["${PROJECT}/**"]

[[capabilities]]
kind = "net.http"
hosts   = ["api.example.com"]
methods = ["GET", "POST"]
```

## Top-level fields

| Field | Type | Required | Notes |
|---|---|---|---|
| `name` | `PackageName` (string) | yes | Validated; same shape across tau (kebab-case identifier). |
| `version` | SemVer string | yes | E.g. `"0.3.0"`. |
| `description` | string | yes | Must be non-empty (validation). |
| `authors` | array of strings | no | Free-form, e.g. `"Acme <hi@acme.dev>"`. Default: empty. |
| `license` | string | no | SPDX expression as opaque text. `None` = unlicensed. |
| `source` | string \| table | yes | See [Source](#source). |
| `kind` | string | yes | One of the [seven kinds](#kind). |
| `dependencies` | array of tables | no | See [Dependencies](#dependencies). |
| `[plugin]` | table | iff package ships a runnable plugin | See [`[plugin]`](#plugin-block). |
| `[sandbox]` | table | no | See [`[sandbox]`](#sandbox-block). Default: no tier floor. |
| `[[capabilities]]` | array of tables | no | See [`[[capabilities]]`](#capabilities). |
| `[skill]` | table | iff `kind = "skill"` | See [Skill manifest schema](skill-manifest-schema.md). |

### `kind`

| Value | Provides | `[plugin]` block? | Trait loaded by runtime |
|---|---|---|---|
| `"llm-backend"` | LLM completion + streaming | yes | `LlmBackend` |
| `"tool"` | Callable tool methods | yes | `Tool` |
| `"skill"` | Reusable agent behaviour | no (rejected by validation) | n/a — invoked via `skill.<name>.spawn` |
| `"pipeline"` | Multi-agent coordination | yes | `Pipeline` |
| `"mcp-server"` | MCP-server-as-tool | yes | wrapped as `Tool` |
| `"storage"` | Persistence backend | yes | `Storage` |
| `"sandbox"` | Sandbox adapter | yes | `Sandbox` |

A `kind` value not in this table parses as
`PackageKind::Custom { kind: <string> }` — the [escape
hatch](../explanation/escape-hatches.md#packagekind-custom). Custom
kinds load no trait; the runtime can store and list them but does
not *do* anything with them.

### `source`

Both string-form and table-form are accepted. The string form is sugar
for the table form.

```toml
# String form: https / http / ssh / git / file URL
source = "https://github.com/example/repo.git"
source = "file:///Users/me/work/local-pkg"
source = "git@github.com:example/repo.git"   # scp-style address

# Table form (use to pin a revision)
[source]
git = "https://github.com/example/repo.git"
rev = "v0.3.1"        # branch, tag, or commit SHA
```

`PackageSource` has one variant — `Git { location, rev }` — with
`GitLocation::{Url, Scp}` for ordinary URLs vs scp-style addresses.
"Local install" is just a `file://` URL.

### `dependencies`

```toml
[[dependencies]]
name        = "fs-tools"
version_req = "^0.3"

[[dependencies]]
name        = "shell-tools"
version_req = ">=0.2, <0.4"
```

Each entry is a `PackageDep { name, version_req }`. Cycles are
rejected at install time; conflicts resolve per ADR-0005.

## `[plugin]` block

Required when the package ships an executable binary the runtime
will load. Omit for data-only packages (skills, manifest-only
extensions).

```toml
[plugin]
provides = "tool"          # required: PortKind
kind     = "rust-cargo"    # required: PluginKind
bin      = "fs-tools-plugin"  # required: Cargo bin target name
```

| Field | Type | Values |
|---|---|---|
| `provides` | `PortKind` | `"llm_backend"`, `"tool"`, `"storage"`, `"sandbox"` |
| `kind` | `PluginKind` | `"rust-cargo"` (only variant today) |
| `bin` | string | The `[[bin]]` target name in the package's `Cargo.toml`. |

`tau-pkg` uses the `[plugin]` block to gate the build step during
install (see ADR-0008). `provides` must match the trait the host
wants to load.

## `[sandbox]` block

Plugin-side sandbox floor. Both fields default to "no floor" — a
package with no `[sandbox]` block accepts any adapter the host can
deliver.

```toml
[sandbox]
required_tier   = "strict"   # "none" | "light" | "strict"
required_shapes = []         # see CapabilityShape; defaults to auto-derived
```

| Field | Type | Default |
|---|---|---|
| `required_tier` | `"none"` \| `"light"` \| `"strict"` | none (no floor) |
| `required_shapes` | array of `CapabilityShape` strings | empty (auto-derived from `[[capabilities]]`) |

Project-side sandbox configuration lives separately in
`<scope>/config.toml`. The resolver intersects scope `required_tier`
× plugin `required_tier` to pick an adapter. See
[Sandboxing](../explanation/sandboxing.md) for the model and the
[platform support reference](sandbox-platform-support.md) for which
adapters deliver which tiers on which OS.

`CapabilityShape` values are PascalCase strings (current schema, kebab
alignment is a deferred follow-up):

| Shape | Source capability |
|---|---|
| `"FilesystemRead"` | `kind = "fs.read"` |
| `"FilesystemWrite"` | `kind = "fs.write"` |
| `"ProcessExec"` | `kind = "fs.exec"` / `"process.spawn"` |
| `"NetworkHttp"` | `kind = "net.http"` |
| `"AgentSpawn"` | `kind = "agent.spawn"` |
| `"SkillSpawn"` | `kind = "skill.spawn"` |
| `"Custom"` | `Capability::Custom` |

## `[[capabilities]]`

Every capability is an item of the `[[capabilities]]` array of
tables. The serialization form is flat: `kind = "<dot.namespaced>"`
plus the per-variant fields as siblings (ADR-0002 §3). The
deserializer maps recognised `kind` strings onto typed variants;
unknown `kind` falls through to `Capability::Custom`.

### Filesystem

```toml
[[capabilities]]
kind  = "fs.read"
paths = ["${PROJECT}/**", "${HOME}/data/*.json"]
```

```toml
[[capabilities]]
kind      = "fs.write"
paths     = ["${PROJECT}/build/**"]
max_bytes = 10_485_760    # optional: per-file write cap
```

```toml
[[capabilities]]
kind  = "fs.exec"
paths = ["/usr/bin/git", "/usr/local/bin/cargo"]
```

`paths` are glob patterns. `${PROJECT}` and `${HOME}` are substituted
at install / spawn time. `fs.exec` is per-command exec gating —
landlock V1 `AccessFs::Execute` on Linux (ADR-0017 §3).

### Network

```toml
[[capabilities]]
kind    = "net.http"
hosts   = ["api.anthropic.com", "*.openai.com"]
methods = ["GET", "POST"]
```

`hosts` are exact or glob; `methods` are uppercase by convention.
Strict tier validates the SNI against `hosts` (no IPs, no wildcards
inside literal IPs) per ADR-0020.

### Process

```toml
[[capabilities]]
kind     = "process.spawn"
commands = ["git", "cargo"]
```

Allows spawning the named subprocesses. Combined with `fs.exec` for
the actual landlock grant.

### Agent

```toml
[[capabilities]]
kind          = "agent.spawn"
allowed_kinds = ["worker"]
```

Permits the parent agent to spawn sub-agents whose package `kind`
matches `allowed_kinds`. Multi-agent orchestration (ADR-0024).

### Skill

```toml
[[capabilities]]
kind           = "skill.spawn"
allowed_skills = ["critic", "fact-checker"]
```

Authorises the parent agent to invoke installed skills as child
agents via `skill.<name>.spawn` (ADR-0028). The names match
`LockedPackage.name` for `kind = "skill"` entries in the lockfile.

### Task-list / plan (virtual)

```toml
[[capabilities]]
kind = "task-list"
mode = "read"      # "read" | "write" | "manage"

[[capabilities]]
kind = "plan"
mode = "write"     # "read" | "write"
```

These are runtime-only — not OS-sandbox-enforced — gated at the
virtual-tool dispatch layer in `tau-runtime`.

### Custom

Anything not in the typed table above:

```toml
[[capabilities]]
kind = "mcp.tool.use"
params = { server = "weather", tool = "current" }
```

Falls through to `Capability::Custom { name, params }`. `params` is a
free-form table; `name` must be non-empty (validation). The dot
convention is recommended but not mandated (ADR-0002 §4). Custom
capabilities are tracked in the [escape-hatch
registry](../explanation/escape-hatches.md).

## Validation

`UncheckedManifest::validate()` checks invariants the field types
can't enforce alone (`crates/tau-domain/src/package/manifest.rs`):

| Rule | Error |
|---|---|
| `description` is non-empty | `EmptyDescription` |
| Every `Custom` capability has a non-empty `name` | `CapabilityEmptyName { index }` |
| `kind = "skill"` packages MUST NOT carry a `[plugin]` block | `SkillCannotHavePluginBlock` |

Per-dependency invariants (duplicate names, version-range
cross-checks) are reserved hooks at v0.1 — no rules fire today, but
the iteration is in place for future additions.

## Reserved names

ADR-0002 §3 reserves typed param names so they round-trip through
`Capability::Custom` and re-promote into typed variants in a future
v0.X:

- `paths` (used by `fs.*`)
- `max_bytes` (`fs.write`)
- `hosts`, `methods` (`net.http`)
- `commands` (`process.spawn`)
- `allowed_kinds` (`agent.spawn`)
- `allowed_skills` (`skill.spawn`)

Don't repurpose them inside `Capability::Custom.params` for unrelated
meanings.

## Complete examples

### A tool plugin (`fs-tools`)

```toml
name        = "fs-tools"
version     = "0.3.0"
description = "Filesystem read tools for tau agents."
authors     = ["Example <hi@example.org>"]
license     = "Apache-2.0 OR MIT"
source      = "https://github.com/example/fs-tools.git"
kind        = "tool"

[plugin]
provides = "tool"
kind     = "rust-cargo"
bin      = "fs-tools-plugin"

[sandbox]
required_tier = "strict"

[[capabilities]]
kind  = "fs.read"
paths = ["${PROJECT}/**"]
```

### An LLM-backend plugin (`@tau/anthropic`)

```toml
name        = "tau-anthropic"
version     = "0.1.4"
description = "Anthropic Claude LLM backend."
source      = { git = "https://github.com/example/tau-anthropic.git", rev = "v0.1.4" }
kind        = "llm-backend"

[plugin]
provides = "llm_backend"
kind     = "rust-cargo"
bin      = "tau-anthropic"

[sandbox]
required_tier = "strict"

[[capabilities]]
kind    = "net.http"
hosts   = ["api.anthropic.com"]
methods = ["POST"]
```

### A skill package (`praise-poet`)

```toml
name        = "praise-poet"
version     = "0.1.0"
description = "Writes effusive praise for the topic at hand."
source      = "file:///Users/me/work/praise-poet"
kind        = "skill"

[skill]
content        = "SKILL.md"
requires_tools = []
requires_skills = []
```

No `[plugin]` block (validation rejects it for `kind = "skill"`);
the `[skill]` block is specified separately — see the
[skill manifest schema](skill-manifest-schema.md).

## See also

- [Packages](../explanation/packages.md) — the conceptual model.
- [Skill manifest schema](skill-manifest-schema.md) — the
  `[skill]` block and `SKILL.md` frontmatter.
- [Sandboxing](../explanation/sandboxing.md) — tier model and
  resolver.
- [Sandbox platform support](sandbox-platform-support.md) — which
  adapter delivers which tier on which OS.
- [Escape hatches](../explanation/escape-hatches.md) — registry of
  every `Custom` variant in the schema.
- [ADR-0002](../decisions/0002-manifest-format.md) — manifest field
  set, capability canonicalisation, escape-hatch policy.
- [ADR-0005](../decisions/0005-package-source-and-kind-serde.md) —
  string-form `source` / `kind` serde shape.
- [ADR-0008](../decisions/0008-plugin-loading.md) — how `[plugin]`
  becomes a running process.
- [ADR-0016](../decisions/0016-plugin-compat-verification.md) —
  install-time Layer 2 cross-check between manifest and binary.
