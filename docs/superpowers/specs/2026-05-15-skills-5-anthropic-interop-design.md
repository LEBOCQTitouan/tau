# Skills-5 — Anthropic interop (export + import + conformance) design

**Status:** Brainstormed 2026-05-15 (auto mode).
**Branch:** `feat/skills-5-anthropic-interop-design`.
**Predecessors:** Skills-1 (`1d71032`), Skills-2 (`93dbe95`), Skills-3 (`7bec3ab`), Skills-4 (`1f6f331`).
**Depends on:** ADR-0025 (Skills foundation), ADR-0026 (Skills install pipeline), ADR-0027 (Skills discovery), ADR-0028 (Skills runtime invocation).

## Goal

Make tau skill packages **bidirectionally exchangeable** with the broader Agent Skills ecosystem (specifically Anthropic's spec). Today a tau skill is *almost* an Anthropic skill — Skills-1's Option D explicitly built tau on top of the Anthropic format, but no tooling exists for the conversion in either direction. Skills-5 ships:

1. **Export:** `tau skill export <name>` produces a directory the Anthropic ecosystem (e.g. claude-code) consumes as-is.
2. **Import:** `tau install <git-url>` auto-detects vanilla Anthropic skill repos (no `tau.toml`) and installs them with a synthesized manifest. `tau skill import` provides an explicit two-step variant for the customize-before-install workflow.
3. **Conformance:** `tau verify --anthropic-strict` flags installed skills whose SKILL.md frontmatter is non-conformant.

## Anti-goals

- **Custom export targets.** Only Anthropic format. No `--format <foo>` flag plumbing for hypothetical future spec targets.
- **Lossless round-trip for capability-bearing skills.** Capabilities are dropped on export with an stderr warning. `--strict` exists to fail the export if metadata loss would occur (useful for "round-trippable export" guarantees).
- **Sub-skill `requires_skills` cross-format mapping.** Dropped silently on export. Anthropic spec has no equivalent.
- **Bulk operations** (`tau skill export --all`). YAGNI.
- **Conformance against future spec revisions.** Re-evaluate when the next major Agent Skills spec lands.
- **Modifying SKILL.md format.** Skills-1 already established the format; Skills-5 only routes content between formats.

## Locked design decisions

### D1 — Scope: bidirectional export + import + conformance

Picked from a 4-option scope question (export-only / bidirectional / conformance-only / full interop with `x-tau-capabilities` extension). Bidirectional gives ergonomic UX for the common "import this Anthropic skill from GitHub" flow; conformance closes the validation loop. The `x-tau-capabilities` YAML extension was rejected as premature — its only benefit is round-trippable export for capability-bearing skills, which has no real use case yet.

### D2 — Import flow: BOTH auto-detect AND explicit `tau skill import`

`tau install <src>` detects Anthropic format and synthesizes the manifest transparently (ergonomic happy path). `tau skill import <src> --output <dir>` produces an editable directory for users who want to inspect or customize the synthesized `tau.toml` before installing.

```
# Quick install — synthesized invisibly
$ tau install https://github.com/anthropic/critic-skill
> Installed critic@0.1.0 (synthesized: yes)

# Customize first
$ tau skill import https://github.com/anthropic/critic-skill --output ./my-critic
> Wrote ./my-critic/tau.toml (capabilities=[])
$ # edit ./my-critic/tau.toml: add fs.read capability
$ tau install ./my-critic
```

The synthesized manifest lives in-memory during `tau install`; the on-disk source is left untouched (the install pipeline copies the source into the scope's `.tau/packages/<name>/<version>/` location with the synthesized `tau.toml` written there).

### D3 — Capability translation: drop on export, default-empty on import

Synthesized manifests start with `capabilities = []`. Skill authors who want capabilities must add them by hand (via `tau skill import` + edit, or by writing tau.toml from scratch).

For non-SKILL.md content (refs/, assets/, etc.), Skills-5 ships a simple **copy-everything-except-tau.toml** export strategy. `tau-domain::package::SkillManifest` does not currently carry a `files` glob — the install pipeline already copies the entire source tree into `<scope>/.tau/packages/<name>/<version>/`, so on export we copy the whole installed directory verbatim minus `tau.toml`. This is byte-identical for Anthropic-sourced skills (round-trip clean) and includes any extra files for tau-native skills (acceptable: those files were authored to live alongside SKILL.md).

On export, any capabilities in the installed skill's `tau.toml` are silently dropped from the output (Anthropic format has no equivalent). A single-line stderr note records what was dropped: `note: "critic" had 2 capabilities dropped on Anthropic export (fs.read, net.http); Anthropic format does not preserve capability declarations`.

`tau skill export --strict` makes the warning a hard error — the export refuses to proceed if anything tau-specific would be lost. Useful for users who want lossless round-trippability guarantees and would rather know about it than silently lose metadata.

### D4 — Provenance: lockfile schema v5 → v6

`LockedPackage.synthesized_from: Option<SynthesizedSource>` records the manifest origin. Enum variants for now: `SynthesizedSource::Anthropic`. Field defaults to `None` for v5 entries on read; lockfile auto-upgrades to v6 on next write. `tau skill show <name>` displays `Source: synthesized (Anthropic format)` when the field is `Some`.

### D5 — Conformance: `tau verify --anthropic-strict` flag

Extends the existing `tau verify` pipeline from Skills-2. Without the flag, behavior unchanged. With the flag, additionally checks each installed skill's SKILL.md against Anthropic conformance:

- `frontmatter.description` is non-empty
- SKILL.md body (after stripping frontmatter) is non-empty
- Frontmatter is well-formed YAML

Emits new `VerifyStatus::AnthropicConformance` variants on failure.

## Architecture

Three layers receive changes; two new CLI commands land.

```
                          ┌──────────────────────────┐
                          │  tau-cli (new commands)  │
                          │                          │
                          │  cmd/skill/import.rs ←──┐│
                          │  cmd/skill/export.rs    ││
                          │  cmd/verify.rs (flag)   ││
                          └──────────┬───────────────┘
                                     │
                          ┌──────────▼──────────┐
                          │       tau-pkg       │
                          │                     │
                          │  install.rs (detect)│ ──┐
                          │  synthesize.rs (new)│   │
                          │  lockfile.rs (v6)   │   │
                          │  verify.rs (strict) │   │
                          └──────────┬──────────┘   │
                                     │              │
                          ┌──────────▼──────────┐   │
                          │     tau-domain      │   │
                          │                     │   │
                          │  package/skill_format.rs (new)
                          │  package/skill.rs   │   │
                          └─────────────────────┘   │
                                                    │
   Anthropic git source ──> clone ──> detect ──────┘
   (SKILL.md, no tau.toml)
```

**Component responsibilities:**

| Component | Responsibility |
|---|---|
| `tau-domain::package::skill_format` | Pure detection + synthesis. `SkillFormat::{Tau, Anthropic, Invalid}` + `detect_format(dir)` + `synthesize_manifest_from_skill_md(parsed) -> PackageManifest`. No I/O beyond reading the source directory. |
| `tau-pkg::synthesize` | Bridge module: orchestrates `detect_format` + SKILL.md parse + domain's synthesizer to return a `PackageManifest` ready for install. |
| `tau-pkg::install` | Extend `install_with_options()` to call `synthesize` for Anthropic-format sources; pipe `SynthesizedSource::Anthropic` through to the lockfile. |
| `tau-pkg::lockfile` | v5 → v6 bump: `LockedPackage.synthesized_from` field with serde default. Backward-compatible reader (v5 reads as `None`). |
| `tau-pkg::verify` | New `--anthropic-strict` mode + `VerifyStatus::AnthropicConformance` variants. |
| `tau-cli::cmd::skill::import` | Clone + write synthesized `tau.toml` + print next-step hint. No install. |
| `tau-cli::cmd::skill::export` | Read installed skill from lockfile; copy SKILL.md + `[skill].files` glob members to `--output`; drop `tau.toml`. Emit dropped-capabilities warning by default; `--strict` makes warnings errors. |
| `tau-cli::cmd::verify` | Wire `--anthropic-strict` CLI flag through to `tau-pkg::verify`. |
| `tau-cli::cmd::skill::show` | Display `Source: synthesized (Anthropic format)` line when `synthesized_from.is_some()`. |

## Data flow

### Install path (auto-detect)

```
tau install <git-url-or-path>
  → tau-pkg::source::clone_to_workspace()            [existing]
  → SkillFormat::detect(cloned_dir)                  [new]
      │
      ├─ Tau:       proceed normally (existing path; no change)
      │
      ├─ Anthropic: parse_skill_md(SKILL.md)
      │             → synthesize_manifest_from_skill_md(parsed)
      │                 → PackageManifest {
      │                     name: parsed.frontmatter.name,
      │                     version: "0.1.0",
      │                     kind: "skill",
      │                     description: parsed.frontmatter.description,
      │                     source: <git-url> | "local://<path>",
      │                     authors: [],
      │                     capabilities: [],
      │                     skill: SkillManifest {
      │                       content: "SKILL.md".into(),
      │                       requires_tools: vec![],
      │                       requires_skills: vec![],
      │                     },
      │                   }
      │             → install with synthesized PackageManifest
      │             → LockedPackage.synthesized_from = Some(Anthropic)
      │
      └─ Invalid:   InstallError::NotASkillPackage { path, detail }
```

### `tau skill import` path

```
tau skill import <src> --output ./my-skill [--force]
  → tau-pkg::source::clone_to_path(./my-skill)
  → SkillFormat::detect(./my-skill)
      │
      ├─ Tau:       ImportError::SourceAlreadyTauSkill { path }
      │             → suggest: `tau install ./my-skill`
      │
      ├─ Anthropic: synthesize_manifest_from_skill_md(parsed)
      │             → serialize PackageManifest as TOML
      │             → write ./my-skill/tau.toml
      │             → print: "Wrote ./my-skill/tau.toml.
      │                       Run `tau install ./my-skill` to install."
      │
      └─ Invalid:   ImportError::NotASkillPackage
```

If `--output` exists and `--force` is not set: `ImportError::OutputDirectoryExists`.

### `tau skill export` path

```
tau skill export <name> --output ./out [--strict] [--force]
  → find_installed_skill(scope, name)                [reuse Skills-4 helper]
  → check installed.capabilities is empty OR --strict not set:
      OK path:
        → walk <install_path> recursively
        → for each file: copy to ./out/<relative_path>
                          UNLESS file_name == "tau.toml"
        → DO NOT copy tau.toml
        → DO NOT copy any scope metadata
        → if installed.capabilities.len() > 0:
            stderr: "note: {N} capabilities dropped on Anthropic export ({kinds})"
        → print: "Exported {name} to ./out/"
      --strict + capabilities present:
        → ExportError::WouldDropMetadata { name, dropped: vec!["fs.read", "net.http"] }
```

If `--output` exists and `--force` is not set: `ExportError::OutputDirectoryExists`.

### `tau verify --anthropic-strict` path

```
tau verify --anthropic-strict
  → existing tau verify pipeline                     [Skills-2]
  → for each installed skill:
      → read install_path/SKILL.md
      → parse_skill_md()
      → if frontmatter.description.is_empty():
          VerifyStatus::AnthropicConformance { name, MissingDescription }
      → if body.trim().is_empty():
          VerifyStatus::AnthropicConformance { name, EmptyBody }
      → if parse failed:
          VerifyStatus::AnthropicConformance { name, MalformedFrontmatter }
```

Non-skill packages skipped (no Anthropic conformance check for tools / llm-backends).

## Error handling

**New `tau-pkg::install::InstallError` variants:**

```rust
#[non_exhaustive]
pub enum InstallError {
    // ... existing variants ...
    /// Cloned source has neither tau.toml nor SKILL.md.
    NotASkillPackage { path: PathBuf, detail: String },
    /// SKILL.md parse failed during Anthropic-format synthesis.
    SynthesizeFailed { detail: String },
}
```

**New `tau-cli::cmd::skill::import::ImportError` enum** (private to cmd/skill/import.rs):

```rust
pub(crate) enum ImportError {
    SourceAlreadyTauSkill { path: PathBuf },
    OutputDirectoryExists { path: PathBuf },
    Install(#[from] tau_pkg::install::InstallError),
    Io(#[from] std::io::Error),
}
```

**New `tau-cli::cmd::skill::export::ExportError` enum:**

```rust
pub(crate) enum ExportError {
    SkillNotInstalled { name: String, suggestion: Option<String> },
    WouldDropMetadata { name: String, dropped: Vec<String> },
    OutputDirectoryExists { path: PathBuf },
    Io(#[from] std::io::Error),
    FindSkill(#[from] tau_pkg::FindSkillError),
}
```

**New `tau-pkg::verify::VerifyStatus::AnthropicConformance` variant:**

```rust
#[non_exhaustive]
pub enum VerifyStatus {
    // ... existing variants ...
    AnthropicConformance {
        skill_name: String,
        issue: AnthropicConformanceIssue,
    },
}

#[non_exhaustive]
pub enum AnthropicConformanceIssue {
    MissingDescription,
    EmptyBody,
    MalformedFrontmatter { detail: String },
}
```

All new errors render via `tau-cli::cmd::error_render` with actionable remediation hints (e.g. `SourceAlreadyTauSkill` suggests `tau install` instead).

## Testing

**~25 new tests across the layers:**

| Layer | Test file | Tests |
|---|---|---|
| `tau-domain::skill_format` | `tau-domain/src/package/skill_format.rs` `#[cfg(test)]` | 5 unit: detect Tau / Anthropic / Invalid; synthesize roundtrip; synthesize from missing description fails |
| `tau-pkg::install` Anthropic | `tau-pkg/tests/install_anthropic_format.rs` (new) | 4 integration: install from Anthropic git URL; lockfile records `synthesized_from = Anthropic`; existing tau-install path unaffected; lockfile v5→v6 migration reads cleanly |
| `tau-cli::cmd::skill::import` | `tau-cli/tests/cmd_skill_import.rs` (new) | 4 integration: import to fresh dir; refuse existing dir without `--force`; refuse tau-format source; assert synthesized tau.toml content matches expected TOML |
| `tau-cli::cmd::skill::export` | `tau-cli/tests/cmd_skill_export.rs` (new) | 5 integration: capability-less skill roundtrips; capability-bearing skill drops with stderr warning; `--strict` fails when capabilities present; `--output` collision refused without `--force`; multi-file skill (with `[skill].files = ["refs/**"]`) copies all referenced files |
| `tau-cli::cmd::verify --anthropic-strict` | extend `tau-cli/tests/cmd_verify.rs` (existing) | 3 integration: passes for conformant skill; fails for missing-description skill; existing `tau verify` behavior unaffected |
| Roundtrip e2e | `tau-cli/tests/skill_format_roundtrip.rs` (new) | 2 integration: tau export → tau install reproduces the original lockfile for capability-less skills; tau import → tau install matches a direct tau install of an Anthropic-format source |

**Lockfile migration backward compatibility test:** synthesize a v5 lockfile by hand; load via `LockFile::load`; assert `synthesized_from = None` for all entries; mutate + save; assert new file is v6.

## File structure

**New files:**

| Path | Status | LOC estimate |
|---|---|---|
| `crates/tau-domain/src/package/skill_format.rs` | Create | ~120 |
| `crates/tau-pkg/src/synthesize.rs` | Create | ~80 |
| `crates/tau-cli/src/cmd/skill/import.rs` | Create | ~150 |
| `crates/tau-cli/src/cmd/skill/export.rs` | Create | ~200 |
| `crates/tau-pkg/tests/install_anthropic_format.rs` | Create | ~150 |
| `crates/tau-cli/tests/cmd_skill_import.rs` | Create | ~120 |
| `crates/tau-cli/tests/cmd_skill_export.rs` | Create | ~180 |
| `crates/tau-cli/tests/skill_format_roundtrip.rs` | Create | ~100 |
| `docs/decisions/0029-skills-anthropic-interop.md` | Create | ADR |

**Modified files:**

| Path | Change |
|---|---|
| `crates/tau-domain/src/package/mod.rs` | Add `pub mod skill_format;` + re-export `SkillFormat`, `SynthesizedSource` |
| `crates/tau-pkg/src/install.rs` | Auto-detect format; synthesize Anthropic manifests in-memory; pass `synthesized_from` through to lockfile write |
| `crates/tau-pkg/src/lockfile.rs` | Bump `SCHEMA_VERSION` constant 5 → 6; add `synthesized_from: Option<SynthesizedSource>` field with `#[serde(default)]`; ensure backward-compatible reading |
| `crates/tau-pkg/src/lib.rs` | Re-export `synthesize` module + `SynthesizedSource` |
| `crates/tau-pkg/src/verify.rs` | `--anthropic-strict` mode + `VerifyStatus::AnthropicConformance` + `AnthropicConformanceIssue` |
| `crates/tau-cli/src/cmd/skill/mod.rs` | Wire `Import` + `Export` subcommands |
| `crates/tau-cli/src/cmd/skill/show.rs` | Display `synthesized_from` field when present |
| `crates/tau-cli/src/cli.rs` | Add `SkillSubcommand::{Import, Export}` + their args; add `VerifyArgs::anthropic_strict: bool` |
| `crates/tau-cli/src/cmd/verify.rs` | Wire `--anthropic-strict` flag through to `tau-pkg::verify` |
| `crates/tau-cli/src/cmd/error_render.rs` | Render new ImportError / ExportError variants |

## Estimated effort

| Task | Subagent | Effort |
|---|---|---|
| T1: `SkillFormat` + `detect_format` + `synthesize_manifest_from_skill_md` (tau-domain) | sonnet | 0.5d |
| T2: Lockfile v5→v6 + `synthesized_from` field + migration tests | sonnet | 0.5d |
| T3: `tau-pkg::install` auto-detect + `synthesize.rs` bridge module + integration tests | sonnet | 0.75d |
| T4: `tau-pkg::verify --anthropic-strict` mode + `AnthropicConformance` variants | sonnet | 0.5d |
| T5: `tau skill import` subcommand + 4 integration tests | sonnet | 0.5d |
| T6: `tau skill export` subcommand + 5 integration tests (multi-file copy + strict + warning) | sonnet | 0.75d |
| T7: Roundtrip e2e tests + `tau skill show` `synthesized_from` display | sonnet | 0.5d |
| T8: ADR-0029 | haiku | 0.25d |
| T9: USER GATE — push + open PR | main | — |

**Total: ~4 days, 9 tasks.** Within the 3-5d priority queue estimate.

## Considered and rejected

- **`x-tau-capabilities` YAML extension key** for round-trippable export of capability-bearing skills. Rejected: no real use case yet; YAGNI. Capabilities are dropped on export with a warning; users can re-add on the receiving end if needed.
- **Conformance-only Skills-5** (Option C in brainstorm). Rejected: the import flow is the highest-leverage ergonomic improvement of the whole Skills track. Without it, users can't consume the existing Anthropic skill ecosystem.
- **Export-only Skills-5** (Option A in brainstorm). Rejected for the same reason: import is the highest-value direction.
- **Auto-detect ONLY** (no `tau skill import` command). Rejected: power users want to inspect/edit the synthesized tau.toml before install. Auto-detect alone forces edits to happen post-install (which is awkward because tau.toml lives inside the scope's `.tau/packages/` location).
- **`tau skill import` ONLY** (no auto-detect in `tau install`). Rejected: forces every user to know about the format mismatch and run an extra step. Most installs of Anthropic skills should Just Work.
- **`--format <foo>` flag for plural spec targets.** Rejected: only Anthropic format is in scope; adding it now is premature plumbing.
- **Bulk export (`tau skill export --all`).** Rejected: YAGNI; trivially scriptable via `tau skill list --json | jq -r '.skills[].name' | xargs -I{} tau skill export {}` once needed.

## Out of scope (Skills-6+)

- **Reference skill packages themselves** — Skills-6
- **Sub-skill `requires_skills` cross-format mapping** — advisory only; drop on export silently
- **Conformance against future Anthropic spec revisions** — re-evaluate when the next major spec lands
- **`tau skill convert` as an alias for `import` or `export`** — Skills-5 v2 if requested
- **MCP-adjacent interop** (e.g. consuming MCP server descriptors as tau skills) — separate sub-project; out of ROADMAP §16 scope

## References

- Spec: this document
- Implementation plan: `docs/superpowers/plans/2026-05-15-skills-5-anthropic-interop.md` (to be written next)
- ADR (pending): `docs/decisions/0029-skills-anthropic-interop.md`
- Predecessor specs:
  - `docs/superpowers/specs/2026-05-12-skills-1-manifest-design.md` (foundation; established Option D two-layer architecture)
  - `docs/superpowers/specs/2026-05-12-skills-2-install-pipeline-design.md` (lockfile schema; install machinery)
  - `docs/superpowers/specs/2026-05-13-skills-3-discovery-design.md` (`tau skill show` UI surface)
  - `docs/superpowers/specs/2026-05-14-skills-4-runtime-invocation-design.md` (`find_installed_skill` helper reused for export)
- Predecessor ADRs: 0025 (foundation), 0026 (install pipeline), 0027 (discovery), 0028 (runtime invocation)
- ROADMAP §16
- Priority queue: `docs/superpowers/specs/2026-05-12-post-multi-agent-priority-queue.md`
