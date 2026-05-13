# Skills-3: Discovery — design

## Context

Third of 6 sub-projects decomposed from ROADMAP §16 (Skills as
first-class packages, Constitution G10). See
[`2026-05-12-post-multi-agent-priority-queue.md`](2026-05-12-post-multi-agent-priority-queue.md)
for the full decomposition. Skills-1 (PR #63, `1d71032`) shipped the
manifest types + parser; Skills-2 (PR #64, `93dbe95`) wired the
install pipeline + lockfile schema v4→v5 + `LockedSkill { content_sha256,
frontmatter }` cache.

Skills-3 surfaces installed skills to the user via the CLI:
`tau skill list` enumerates; `tau skill show <name>` inspects.
Reads the frontmatter Skills-2 cached in the lockfile (no per-skill
disk seeks for `list`); reads `tau.toml` once for `show`; reads
`SKILL.md` once for `show --body`.

## Goal

A user who runs `tau install <skill-pkg>` can immediately:
- See it appears in `tau skill list` with name, version, description.
- Run `tau skill show <name>` to inspect capabilities, dependencies,
  source, install path.
- Run `tau skill show <name> --body` to read the SKILL.md content —
  rendered for terminal display by default, `--raw` for pipe-friendly
  output.

Skills-3 ships only the inspection commands; runtime invocation
(spawning skills as agents) lands in Skills-4.

## Decision (locked during brainstorm)

Two subcommands: `tau skill list` and `tau skill show <name>`. No
`tau skill uninstall` (generic `tau uninstall <name>` already handles
skill packages — adding a kind-specific alias is unnecessary surface).
No `tau skill where` (path is in `show`'s output). Both subcommands
support `--json` for canonical machine-readable output; both default
to human-formatted terminal output.

`list` reads only the lockfile (Skills-2's `LockedSkill.frontmatter`
provides everything needed). `show` reads the lockfile + the
package's `tau.toml` (one disk seek for the requested skill) to fill
in fields not cached (capabilities, requires_tools, requires_skills).
`show --body` adds one more read of `SKILL.md`.

`show --body` renders the markdown by default (termimad, matching
`tau chat`'s precedent). `--raw` opts out for pipe / grep / diff
workflows.

Two minor calls also locked: no `--sort` flag in v1 (alphabetical by
name, ascending); unknown-name handling on `show` includes a
Levenshtein "Did you mean…?" suggestion before the exit-2 error.

## CLI surface

### `tau skill list`

Enumerate all installed skills in the current scope.

```
USAGE:
    tau skill list [OPTIONS]

OPTIONS:
        --json    Emit canonical JSON instead of the default
                  human-formatted table
    -h, --help    Print help
```

**Human output (default):**

```
NAME       VERSION  DESCRIPTION
critic     0.1.0    Reviews drafts for unsourced claims.
fact-check 0.2.0    Validates citations against the source corpus.
proofread  0.1.1    Catches typos, agreement errors, and tense slips.
```

Description truncated to 60 chars with `…` if longer. Column widths
are auto-fit per `terminal_size` (no hard wraps). Output goes to
stdout.

Empty state:

```
no skills installed.
hint: install one with `tau install <git-url>`
```

Exit 0.

**JSON output (`--json`):**

```json
{
  "skills": [
    {
      "name": "critic",
      "version": "0.1.0",
      "description": "Reviews drafts for unsourced claims.",
      "source": "https://example.com/critic.git",
      "install_path": "/scope/.tau/packages/critic/0.1.0"
    }
  ]
}
```

Empty state: `{"skills": []}` (no hint, exit 0).

### `tau skill show <name>`

Detailed view of one installed skill.

```
USAGE:
    tau skill show <NAME> [OPTIONS]

ARGS:
    <NAME>  Skill package name (exact match)

OPTIONS:
        --json    Emit canonical JSON instead of the default
                  human-formatted summary
        --body    Include the SKILL.md body (rendered by default;
                  use --raw for verbatim markdown)
        --raw     With --body: emit verbatim file bytes instead of
                  rendered output. Implies --body. Useful for piping
                  to grep, diff, less.
    -h, --help    Print help
```

**Human output (default, no `--body`):**

```
critic 0.1.0
─────────────────────────────────────────────────
description    Reviews drafts for unsourced claims.
source         https://example.com/critic.git
install path   /scope/.tau/packages/critic/0.1.0

capabilities
  fs.read   ${SKILL_DIR}/references/**
  task_list write

requires tools
  fs-read    https://example.com/fs-read.git  ^0.1

requires skills
  (none)
```

Sections are omitted if empty (no `capabilities` / `requires_tools` /
`requires_skills` block if the manifest has none). Source URL gets
truncated visually (no truncation in `--json`).

**Human output with `--body` (rendered):**

Append a rule + the rendered SKILL.md content under a `body` heading:

```
critic 0.1.0
─────────────────────────────────────────────────
description    Reviews drafts for unsourced claims.
source         https://example.com/critic.git
install path   /scope/.tau/packages/critic/0.1.0

capabilities
  fs.read   ${SKILL_DIR}/references/**
  task_list write

requires tools
  fs-read    https://example.com/fs-read.git  ^0.1

body
─────────────────────────────────────────────────

╔══════════════════════════════════════════╗
║  CRITIC                                  ║
╚══════════════════════════════════════════╝

You are a strict editor. Flag every claim that lacks a source.

  How you work
  ────────────
   1.  Read the draft at the path provided
   2.  For each paragraph: identify any factual claim
   ...
```

(Actual rendering uses termimad's ANSI escapes — bold headings,
inverse-video code spans, hanging-indent lists. The box-drawing
above is illustrative.)

**Human output with `--body --raw`:**

Append the verbatim SKILL.md text after the metadata block — no
rendering, no escape codes. Frontmatter is stripped (already
projected into the metadata section above); only the markdown body
goes through.

```
body (raw)
─────────────────────────────────────────────────

# Critic

You are a strict editor. Flag every claim that lacks a source.

## How you work

1. Read the draft at the path provided
...
```

**JSON output (`--json`):**

```json
{
  "name": "critic",
  "version": "0.1.0",
  "description": "Reviews drafts for unsourced claims.",
  "source": "https://example.com/critic.git",
  "install_path": "/scope/.tau/packages/critic/0.1.0",
  "capabilities": [
    {"kind": "fs.read", "paths": ["${SKILL_DIR}/references/**"]},
    {"kind": "task_list", "mode": "write"}
  ],
  "requires_tools": [
    {"name": "fs-read", "source": "https://example.com/fs-read.git", "version_req": "^0.1"}
  ],
  "requires_skills": []
}
```

With `--body --json`: same shape plus a `body: "..."` field (always
raw text — JSON doesn't render markdown). With `--body --raw --json`:
identical to `--body --json` (the `--raw` flag is a no-op for JSON).

**Unknown name handling:**

```
$ tau skill show kritic
error: skill not found: kritic
  did you mean: critic?

  installed skills:
    critic
    fact-check
    proofread
```

Levenshtein distance ≤ 2 surfaces a suggestion; otherwise list all
installed. Exit code 2 (matches existing `tau install` not-found
convention).

JSON form (`--json`):

```json
{"error": "skill not found: kritic", "suggestion": "critic", "installed": ["critic", "fact-check", "proofread"]}
```

To stderr; exit 2.

## Data flow

### `tau skill list`

```
scope discovery       (existing tau-pkg::Scope::resolve)
       ↓
LockFile::load        (existing — already auto-upgrades v4→v5)
       ↓
filter packages       packages.iter().filter(|p| p.skill.is_some())
       ↓
project rows          name = pkg.name, version = pkg.active_version,
                      description = pkg.skill.frontmatter.description,
                      source = pkg.source, install_path = computed
       ↓
sort by name (asc)
       ↓
emit
```

Zero disk reads per skill. Everything comes from the in-memory lockfile.

### `tau skill show <name>`

```
scope discovery + LockFile::load    (same as list)
       ↓
locate package by name              packages.iter().find(|p| p.name == name)
                                    if None → unknown-name handler (Levenshtein)
       ↓
verify it's a skill                 .skill.is_some()
                                    if None → "package found but is not a skill"
       ↓
read tau.toml from install_path     std::fs::read_to_string(install_path/tau.toml)
       ↓
parse + extract                     capabilities, requires_tools, requires_skills
       ↓
if --body:
    read SKILL.md from install_path/<skill.content>
    if --raw: passthrough raw bytes
    else: termimad render
       ↓
emit
```

One extra disk read for `show`; one more for `show --body`.

## Source file resolution

The `install_path` written to the lockfile is the absolute path of
the installed package dir (e.g.
`<scope>/.tau/packages/<name>/<version>/`). `tau.toml` lives at
`install_path/tau.toml`. `SKILL.md` lives at
`install_path/<manifest.skill.content>` (default `"SKILL.md"`).

If `install_path` doesn't exist (manual deletion, scope corruption),
`show` errors with:

```
error: skill "critic" lockfile entry points at /missing/path
  the skill may have been manually removed — re-run `tau install` to restore
```

Exit 2.

## Files added / modified

| Path | Status | Responsibility |
|---|---|---|
| `crates/tau-cli/src/cmd/skill/mod.rs` | Create | Subcommand dispatch. Parallel to existing `cmd/workflow/mod.rs`, `cmd/session/mod.rs`. |
| `crates/tau-cli/src/cmd/skill/list.rs` | Create | `tau skill list` implementation: scope→lockfile→filter→project→sort→emit. ~80 LOC. |
| `crates/tau-cli/src/cmd/skill/show.rs` | Create | `tau skill show` implementation: lookup + tau.toml read + optional SKILL.md render. ~150 LOC. |
| `crates/tau-cli/src/cmd/skill/render.rs` | Create | Termimad rendering helper for `--body` (gated by termimad availability — already a tau-cli dep via `tau chat`). |
| `crates/tau-cli/src/cmd/skill/levenshtein.rs` | Create | Levenshtein distance for "did you mean…?" suggestions. ~30 LOC + 3 unit tests. |
| `crates/tau-cli/src/cli.rs` | Modify | Add `Skill { #[command(subcommand)] cmd: SkillCommand }` variant + `SkillCommand` enum (List, Show). |
| `crates/tau-cli/src/lib.rs` | Modify | `pub mod skill;` if new sibling module; or update `cmd::dispatch` to route the new subcommand. |
| `crates/tau-cli/tests/cmd_skill_list.rs` | Create | 3 integration tests: list with multiple skills, empty state, --json. |
| `crates/tau-cli/tests/cmd_skill_show.rs` | Create | 5 integration tests: show happy path, --json, --body --raw, unknown-name with suggestion, install_path missing. |
| `crates/tau-cli/tests/snapshots/` | Create | 4-5 insta snapshots for human-formatted outputs. |
| `docs/decisions/0027-skills-discovery.md` | Create | ADR. |

**Test fixtures:** integration tests build a synthetic lockfile + minimal install_path layouts in `tempdir`s — no real `tau install` needed. The existing `tau-pkg::LockFile` API lets us synthesize lockfile entries directly.

## Snapshot tests

Skills-3 ships 4-5 insta snapshots covering the human-formatted output
shape. Two reasons to snapshot rather than assert-by-substring:

1. Column alignment + width depends on terminal-size probes; snapshot
   captures the layout reproducibly with a fixed terminal width
   environment variable (`COLUMNS=80`).
2. Termimad's rendered output for `show --body` is a complex ANSI
   string — snapshots are the easiest way to ensure renders stay
   stable.

Snapshot list:
- `list_human_three_skills`
- `list_human_empty_state`
- `show_human_no_body`
- `show_human_with_body` (rendered)
- `show_human_with_body_raw`

## Termimad rendering

Helper function in `cmd/skill/render.rs`:

```rust
/// Render markdown text to ANSI-styled terminal output using termimad.
/// Returns the rendered string. Caller writes to stdout.
///
/// Width auto-detected from the terminal (falls back to 80 if no
/// tty). Termimad's default `MadSkin` is used; future Skills-5 may
/// add user-customizable skins.
pub fn render_markdown(body: &str) -> String {
    let width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80);
    let skin = termimad::MadSkin::default();
    skin.text(body, Some(width)).to_string()
}
```

Existing `tau chat` already uses termimad — the dep is available. The
function is a single call wrapping termimad's `text()` method.

## Levenshtein "Did you mean…?"

Helper function in `cmd/skill/levenshtein.rs`:

```rust
/// Find the closest match to `query` among `candidates` with edit
/// distance ≤ max_dist. Returns the closest match or None.
///
/// Used by `tau skill show <name>` when `<name>` doesn't match any
/// installed skill. Distance ≤ 2 is the suggestion threshold.
pub fn closest_match<'a>(
    query: &str,
    candidates: &'a [String],
    max_dist: usize,
) -> Option<&'a str> { ... }
```

Implementation: standard Wagner-Fischer dynamic programming, O(n*m)
where n, m are string lengths. ~30 LOC. 3 unit tests:
- Exact match returns query (well, the candidate equal to query)
- Single char typo returns the closest
- Distance > max returns None

## CLI parser integration

In `crates/tau-cli/src/cli.rs`, add the `Skill` variant to the
top-level `Commands` enum:

```rust
#[derive(Debug, Subcommand)]
pub enum Commands {
    // ... existing variants ...

    /// Inspect installed skill packages.
    Skill {
        #[command(subcommand)]
        cmd: SkillCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    /// List all installed skills.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show detail for one installed skill.
    Show {
        /// Skill package name.
        name: String,
        #[arg(long)]
        json: bool,
        /// Include the SKILL.md body in the output.
        #[arg(long)]
        body: bool,
        /// With --body: emit verbatim file bytes instead of rendered
        /// output. Implies --body.
        #[arg(long, requires = "body")]
        raw: bool,
    },
}
```

`cmd::dispatch` (or wherever the existing subcommand routing lives)
routes `Commands::Skill { cmd }` to `cmd::skill::dispatch(cmd)`.

## Exit codes

Match the existing tau CLI convention:

- **0** on success (including empty `list`).
- **2** on user-facing errors (skill not found, install_path missing,
  `--body` requested but SKILL.md unreadable).
- **1** never used for these commands (no agent-failure semantics).

## Estimated effort

2-3 days. Components:
- `cmd/skill/{mod, list, show, render, levenshtein}.rs` (~280 LOC + 3 unit tests for Levenshtein)
- 8 integration tests (3 for list, 5 for show)
- 5 insta snapshots
- CLI parser additions (~50 LOC)
- ADR-0027

## Out of scope (deferred)

- **Runtime invocation** — `agent.<skill-name>.spawn` resolution →
  Skills-4. Discovery surfaces installed skills; invocation is a
  separate concern.
- **`tau skill uninstall <name>`** — `tau uninstall` already works for
  skill packages. Adding a kind-specific alias is pure surface bloat.
- **`tau skill where <name>`** — install path is already in `show`'s
  output. Adding `where` is redundant.
- **`--sort=installed-at|version|name`** — v1 sorts alphabetically by
  name. Easy additive enhancement if a user asks.
- **Filtering** (`--with-caps`, `--with-deps`) — `tau skill show
  <name>` already displays this. List filtering is unnecessary;
  `grep`-able output covers most use cases.
- **Recursive sub-skill display** — `tau skill show A` shows A's
  `requires_skills` as bare entries (name + version). Doesn't recurse
  into B's manifest. Recursion is a future ergonomics enhancement.
- **Editable/symlinked dev skills** — installed lockfile entry is the
  source of truth in v1. Skills-2 doesn't yet support `tau install
  --link` for in-place skill development; Skills-3 inherits that gap.
- **User-customizable termimad skins** — default skin only. Skills-5
  or a separate ROADMAP item if users want themes.

## Considered and rejected

### Default to raw markdown for `tau skill show --body`

Considered: would be pipe-friendly out of the box, no rendering hop.
Rejected:
- Tau's existing precedent (`tau chat` streams rendered markdown) is
  render-by-default. Skills should match.
- The interactive case (a user running `tau skill show` to refresh
  their memory) is the most common one. Render makes it scannable.
- `--raw` is one flag away for pipe / grep / diff workflows. The
  trade-off favors the interactive default.

### Add `tau skill uninstall` as an alias

Considered: discoverability. A user who runs `tau skill list` might
look for a `tau skill uninstall` next. Rejected:
- Pure alias for `tau uninstall <name>` — no skill-specific behavior.
- API surface bloat; one more name to maintain compatibility for.
- `tau uninstall` is generic; users learn it once.
- Add later if telemetry shows users hunting for the verb.

### Cache `capabilities` / `requires_tools` / `requires_skills` in lockfile

Considered: would make `show` zero-disk-read like `list`. Rejected:
- Skills-2 just shipped the v5 schema. Bumping to v6 for a marginal
  perf gain is unjustified churn.
- `tau.toml` reads are fast (one file, small payload).
- Drift would need another `sha256` cache to detect — Skills-2 already
  has one for SKILL.md content; adding another for tau.toml is more
  surface.

### Per-row formatter with `--format <template>` flag

Considered: power-user feature for scripting. Rejected:
- `--json | jq` already covers this use case better.
- Template language is its own design problem.
- YAGNI for v1.

## ADR

ADR-0027 will document the design once Skills-3 ships. Open items
for the ADR:
- Render-by-default for `--body` (the locked decision)
- `tau skill uninstall` rejection rationale
- Lockfile read-only strategy (no schema bump in Skills-3)
