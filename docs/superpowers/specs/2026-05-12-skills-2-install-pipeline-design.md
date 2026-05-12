# Skills-2: Install pipeline — design

## Context

Second of 6 sub-projects decomposed from ROADMAP §16 (Skills as
first-class packages, Constitution G10). See
[`2026-05-12-post-multi-agent-priority-queue.md`](2026-05-12-post-multi-agent-priority-queue.md)
for the full decomposition and
[`2026-05-12-skills-1-manifest-design.md`](2026-05-12-skills-1-manifest-design.md)
for the Skills-1 (manifest extension) design that this builds on.

Skills-1 added the typed `[skill]` manifest block, `SkillManifest`
type in tau-domain, `SkillContent`/`SkillFrontmatter` parsing, and the
`${SKILL_DIR}` interpolation variable. Skills-2 wires `tau install
<skill-pkg>` end-to-end through `tau-pkg` so a skill package
resolves, installs, validates, and is recorded in the lockfile —
making it ready for Skills-3 (discovery) and Skills-4 (runtime
invocation).

## Goal

`tau install <skill-pkg-source>` works for `kind = "skill"` packages
with the same UX as `tau install <plugin-pkg-source>` does today:
fetch, validate manifest, resolve transitive deps, write to scope,
record in lockfile, surface install errors with remediation hints.

Skills-2 ships the install-time validation that makes "the skill
actually got installed correctly" provable. Runtime invocation
(Skills-4) trusts that anything in the lockfile passed Skills-2's
validation.

## Decision (locked during brainstorm)

A new module **`tau-pkg::skill_check`**, parallel to the existing
`tau-pkg::sandbox_check`. Single entry point
`cross_check_skill_package(install_dir, manifest) ->
Result<(), InstallError>` called from `install_with_options` between
manifest validation and lockfile write. Layer 2 cross-check
(`cross_check_plugin_capabilities`) is skipped for `kind = "skill"`
packages because skills have no plugin process. Manifest parse-time
validation in tau-domain rejects packages that combine `kind =
"skill"` with a `[plugin]` block.

Lockfile schema is unchanged; `${SKILL_DIR}` is stored symbolically
and resolved at runtime by Skills-4. `tau skill list` (Skills-3)
re-reads `SKILL.md` from disk on demand rather than caching
frontmatter in the lockfile.

This option was reached after weighing it against two alternatives:
(a) inline the skill-specific logic in `install_with_options` instead
of a parallel module, and (b) cache parsed `SKILL.md` content in the
lockfile to speed up Skills-3. Both rejected — see "Considered and
rejected" below.

## What `skill_check` does

Module: `crates/tau-pkg/src/skill_check.rs`

Public entry:

```rust
pub fn cross_check_skill_package(
    install_dir: &Path,
    manifest: &PackageManifest,
) -> Result<(), InstallError>;
```

Steps, in order:

### Step 1: Read `SKILL.md`

Content path comes from `manifest.skill().unwrap().content` (default
`"SKILL.md"`, set by Skills-1's `default_skill_content` serde hook).
Combined with `install_dir` to form an absolute path.

- Missing file → `InstallError::SkillContentMissing { expected_path }`
- Read I/O error other than NotFound → `InstallError::Io(e)`

### Step 2: Parse YAML frontmatter

Frontmatter format: lines between two `---` markers at the top of the
file. Body is everything after the closing `---`. The full
`SkillContent { frontmatter, body }` is returned by Skills-1's
`tau_domain::package::skill::parse_skill_md` (must exist by then).

- Missing frontmatter delimiter, malformed YAML, or absent `name` /
  `description` field → `InstallError::SkillFrontmatterInvalid { detail }`.

### Step 3: Validate name match

`frontmatter.name == manifest.name().as_str()` must hold.
- Mismatch → `InstallError::SkillNameMismatch { tau_toml, skill_md }`
  carrying both strings for the error message.

### Step 4: Reference lint (warn-only)

Scan the body for substrings matching `${SKILL_DIR}/<rel-path>`. For
each match, check whether ANY `[[capabilities]] kind = "fs.read"`
entry has a `paths` glob covering `${SKILL_DIR}/<rel-path>`.

- No covering glob → `tracing::warn!("skill {name}: SKILL.md
  references ${{SKILL_DIR}}/{path} but no fs.read capability covers
  it. The agent may be unable to read this file at runtime.")`

Warn-only because false positives are common (a reference may be for
human readers, the skill author may intend the parent agent to read
on behalf of the child, etc.). Hard-fail at install time is too
strict for content the runtime never actually needs.

The glob check reuses `globset::Glob` (already a tau-pkg dep via
`compute_effective`) for consistency with how `fs.read` paths are
matched everywhere else in tau.

## Wiring into `install_with_options`

`install_with_options` already runs through:
1. Source fetch (git clone / tarball)
2. Manifest parse
3. Manifest validation
4. Transitive dep resolution
5. Tree-hash + binary-hash
6. Sandbox cross-check (Layer 2; skipped for non-plugin kinds)
7. Lockfile write

Skills-2 adds **step 6.5**: skill cross-check. Placed between sandbox
cross-check and lockfile write because both are content-validation
steps; lockfile is the final commit.

Logic:
```rust
if manifest.kind() == &PackageKind::Custom { kind: "skill".into() } {
    skill_check::cross_check_skill_package(&install_dir, &manifest)?;
}
```

The existing sandbox cross-check at step 6 is similarly kind-gated
(only runs for plugin packages); the dispatch is by `manifest.kind()`
in both places.

## Skipping Layer 2 sandbox cross-check for skills

`sandbox_check::cross_check_plugin_capabilities` spawns the plugin
binary and diffs declared vs runtime capabilities. Skills have no
plugin process — there's nothing to spawn. Concretely: when
`manifest.kind()` is `"skill"`, the existing Layer 2 step is skipped.
Add a brief comment in `install.rs` documenting this; no new logic
needed.

## Manifest parse-time validation (tau-domain change)

Currently `PackageManifest`'s `validate()` permits any combination of
`kind` + `[plugin]` + `[skill]` blocks. Skills-2 adds a check: if
`kind == "skill"` and `plugin.is_some()`, return
`PackageManifestError::SkillCannotHavePluginBlock`.

Symmetrically (but Skills-2 doesn't add this — left to Skills-5): if
`kind == "plugin"` and `skill.is_some()`, reject. Skills-2 is
conservative and only enforces the skill→no-plugin direction.

## Dependency resolution

`tau-pkg::resolve::resolve` is polymorphic over the input set of
`PackageDep`s. Today the input is the agent's `[[requires.tools]]`.
Skills-2 extends the input collection for skill packages: when
installing a skill, the resolver receives the union of the skill's
`[[skill.requires_tools]]` + `[[skill.requires_skills]]`.

No changes to `resolve.rs` — it doesn't know or care what kind of
package a dep is; it just resolves the source/version graph. The
extension is at the call site in `install.rs`.

Transitive: if a skill depends on another skill, the second skill is
installed via the same pipeline (recursive entry to
`install_with_options`). Cycle detection is the existing resolver's
responsibility (already handled per ADR-0007 §5).

## Lockfile

Unchanged. The lockfile already records `LockedPlugin { name,
version, source, kind, binary_sha256, ... }` keyed by name; `kind =
"skill"` is a valid value today (it's in `kinds::SKILL`). The
existing lockfile readers don't distinguish kinds.

`binary_sha256` is `None` for skill packages (no binary). Existing
verify-time logic already tolerates `None` (per priority 7's `tau
verify`).

No fields cached from `SKILL.md` (description, body) — Skills-3
re-reads on demand.

## `${SKILL_DIR}` at install time

Stored symbolically in both `tau.toml` and `SKILL.md`. Skills-2 does
NOT do any path substitution. Skills-4 (runtime invocation)
substitutes the symbolic form to an absolute install path when the
runtime reads the manifest at spawn time.

This means the lockfile entry for a skill stores `paths =
["${SKILL_DIR}/references/**"]` verbatim. Verify (`tau verify`)
treats `${SKILL_DIR}` as a recognized symbolic variable and skips it
for path-equivalence checks; existing verify logic already does this
for `${SCOPE}` and `${PROJECT}`.

## New `InstallError` variants

```rust
// crates/tau-pkg/src/error.rs (additive)

SkillContentMissing {
    /// Absolute path the install pipeline tried to read.
    expected_path: PathBuf,
},

SkillNameMismatch {
    /// `name` field from tau.toml.
    tau_toml: String,
    /// `name` field from SKILL.md frontmatter.
    skill_md: String,
},

SkillFrontmatterInvalid {
    /// Human-readable reason (e.g. "missing required field 'name'",
    /// "YAML parse error at line 3: ...").
    detail: String,
},
```

All `#[non_exhaustive]`-friendly additions to the existing
`#[non_exhaustive] InstallError` enum.

## CLI error rendering

`tau-cli/src/cmd/error_render.rs` gains one new render branch
(handling all 3 new variants). Mirrors the existing
`render_cross_check_error` precedent from sub-project B
(plugin-compat). Output shape (illustrative):

```
error: skill "critic" failed install validation

  SKILL.md frontmatter declares name = "kritic" but tau.toml says "critic".
  Both must match. Fix the name field in one of:

    crates/tau-skills/critic/SKILL.md       (frontmatter)
    crates/tau-skills/critic/tau.toml        (top-level `name`)
```

Snapshot tests via `insta` (mirrors `cmd_install_cross_check_render`).

## Tests

### Unit tests (tau-pkg `--lib skill_check`)

~5 tests:
- `skill_check_succeeds_on_valid_critic_fixture`
- `skill_check_returns_content_missing_when_skill_md_absent`
- `skill_check_returns_frontmatter_invalid_on_malformed_yaml`
- `skill_check_returns_name_mismatch_when_diverged`
- `skill_check_warns_on_uncovered_skill_dir_reference` (uses
  `tracing_test` or captures warnings via a test subscriber)

### Integration tests (tau-pkg `--test install_skill_cross_check`)

~3 tests using a minimal critic fixture package at
`crates/tau-pkg/tests/fixtures/skills/critic/`:
- Happy path: skill with valid SKILL.md installs cleanly, lockfile
  records `kind = "skill"`
- `SkillContentMissing` triggers + propagates through
  `install_with_options`
- `SkillNameMismatch` triggers + propagates

### Snapshot tests (tau-cli `--test cmd_install_skill_render`)

~3 insta snapshots covering each of the 3 new error variants.

## Manifest parse-time test (tau-domain)

1 unit test in tau-domain: `skill_kind_rejects_plugin_block` —
constructs a manifest with both `kind = "skill"` and a `[plugin]`
block, asserts `PackageManifestError::SkillCannotHavePluginBlock`.

## Estimated effort

3-4 days. Components:
- `crates/tau-pkg/src/skill_check.rs` (~120 LOC + 5 unit tests)
- 3 new `InstallError` variants in `crates/tau-pkg/src/error.rs`
- `install_with_options` integration in
  `crates/tau-pkg/src/install.rs` (~20 LOC)
- 3 integration tests + critic fixture
- `crates/tau-cli/src/cmd/error_render.rs` extension (~30 LOC)
- 3 insta snapshots in `crates/tau-cli/tests/`
- 1 tau-domain test for skill+plugin rejection

## Out of scope (deferred)

- **`tau skill list` / `tau skill show`** — Skills-3
- **Runtime invocation** (resolving `agent.<skill>.spawn` to installed
  manifest, reading SKILL.md as system_prompt, runtime substitution
  of `${SKILL_DIR}`) — Skills-4
- **Agent Skills spec export / import** — Skills-5
- **Plugin-package symmetry** (rejecting `[skill]` block on
  `kind = "plugin"` packages) — Skills-5 if needed; not load-bearing
  for v1
- **Caching parsed SKILL.md in lockfile** — explicitly rejected; see
  below

## Considered and rejected

### Inline `skill_check` logic in `install_with_options`

Tempting because Skills-2 is small. Rejected because:
- `sandbox_check` is its own module for the same reasons (size of
  `install.rs`, future shareability, testability in isolation).
- Skills-3 (`tau skill list`) and Skills-4 (runtime invocation) need
  to parse SKILL.md too. Putting the parser in a shared
  `skill_check` module lets all three reuse it.
- Symmetry with the existing pattern matters more than line-count
  savings.

### Cache parsed `SKILL.md` content in the lockfile

Would speed up Skills-3 (`tau skill list` reads from lockfile, no
disk seek per skill). Rejected because:
- Drift: lockfile snapshot vs on-disk source can diverge if a skill
  author edits SKILL.md without re-running install. The lockfile
  should be the install contract, not a denormalized content store.
- Skills are small in count (humans install ~5-50 per scope). Disk
  reads at list time are negligible.
- Skills-3 can always optimize later if list time becomes a problem
  — the lockfile schema is additive.

## ADR

ADR-0026 will document this decision once Skills-2 ships. Open items
for the ADR: the inline-vs-module call, the no-cache lockfile choice,
the skill-cannot-have-plugin-block validation, and Skills-2's relation
to Layer 2 sandbox cross-check (skipped for skills).
