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

**Reference lint is hard-failing** (revised from initial warn-only
draft): if `SKILL.md` body contains `${SKILL_DIR}/<rel-path>`
references and no `[[capabilities]] kind = "fs.read"` glob covers
the path, install rejects with `InstallError::SkillReferenceWithoutCapability`.
Catches the misconfiguration at install time rather than at runtime
when the agent gets a confusing capability-denied error mid-task.
The fix is trivial (skill author adds one capability line or removes
the reference), so false-positive cost is low.

**Lockfile schema migration** (revised from initial no-migration
draft): a new lockfile schema version adds two fields per skill
entry — `content_sha256: [u8; 32]` (SHA-256 of the `SKILL.md` file
bytes) and `frontmatter: SkillFrontmatterSnapshot { name, description }`.
This lets `tau skill list` enumerate installed skills from the
lockfile alone (no disk seeks per skill — fast for scopes with many
skills) and `tau verify` detects SKILL.md drift the same way it
detects plugin-binary drift today.

`${SKILL_DIR}` remains symbolic in both lockfile and manifest; runtime
substitution is Skills-4's responsibility.

This option was reached after weighing it against two alternatives:
(a) inline the skill-specific logic in `install_with_options` instead
of a parallel module, and (b) keep the lockfile schema unchanged
(re-read `SKILL.md` on demand from `tau skill list`). Both rejected —
see "Considered and rejected" below.

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

### Step 4: Reference lint (hard-fail)

Scan the body for substrings matching `${SKILL_DIR}/<rel-path>`. For
each match, check whether ANY `[[capabilities]] kind = "fs.read"`
entry has a `paths` glob covering `${SKILL_DIR}/<rel-path>`.

- No covering glob → `InstallError::SkillReferenceWithoutCapability {
  reference: String, declared_paths: Vec<String> }`.

The error carries both the offending reference (so the user sees
exactly what's missing) and the set of `fs.read` paths actually
declared (so the user sees what would need to extend).

Hard-fail rationale: the false-positive cost (skill author adds one
`[[capabilities]]` line or removes a stale reference) is trivial.
The true-positive cost — caught at runtime when the agent gets a
capability-denied error mid-task — is severe (confusing failure mode
for the agent's LLM, no clear remediation path from the runtime error
alone). Install time is the right gate.

The glob check reuses `globset::Glob` (already a tau-pkg dep via
`compute_effective`) for consistency with how `fs.read` paths are
matched everywhere else in tau.

Reference extraction is conservative: matches the literal string
`${SKILL_DIR}/` followed by a path-like sequence (`[A-Za-z0-9_\-./]+`).
Markdown link syntax (`[name](${SKILL_DIR}/foo.md)`), inline code
(`` `${SKILL_DIR}/foo.md` ``), and prose mention all match equivalently.
Skill authors who genuinely want a human-only reference (not for the
agent) can use an explicit prose form without the `${SKILL_DIR}/`
prefix, e.g. "see the style guide in references/".

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

**Schema migration: v4 → v5** (additive — old v4 entries auto-upgrade
on read with a `tracing::warn!` once per process, matching the v2→v3
migration precedent from sub-project 12).

Two new fields per skill entry:

```rust
/// SHA-256 of the SKILL.md file bytes at install time. Lets
/// `tau verify` detect drift the same way binary_sha256 does for
/// plugin binaries. None for non-skill packages.
content_sha256: Option<[u8; 32]>,

/// Snapshot of SKILL.md frontmatter at install time. Lets
/// `tau skill list` and `tau skill show` enumerate without disk
/// seeks. None for non-skill packages.
frontmatter: Option<SkillFrontmatterSnapshot>,
```

Where:

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillFrontmatterSnapshot {
    pub name: String,
    pub description: String,
}
```

**Snapshot scope:** only `name` + `description` are cached. The body
is NOT cached — it can be arbitrarily large and Skills-4 will load
it lazily at spawn time. The cached fields are exactly what
`tau skill list` needs for its summary view.

**Drift detection:** `tau verify` (priority 7) checks `content_sha256`
against the on-disk file. Mismatch surfaces as
`VerifyReport::SkillContentDrift { name, expected, got }` — a new
variant parallel to the existing `BinaryDrift` for plugin binaries.
The user remediates by re-running `tau install` (which will recompute
the hash + refresh the cached frontmatter).

**Existing fields unchanged:** `binary_sha256` is `None` for skill
packages (no binary); verify logic already tolerates `None`. Other
fields (`name`, `version`, `source`, `kind`) carry through unchanged.

**Why cache:** scopes with ~30-50 installed skills would otherwise
incur that many file opens per `tau skill list`. Cached frontmatter
(~200 bytes per skill) is negligible storage; the perf payoff
compounds across other CLI surfaces (autocomplete, `tau skill show`,
future indexing). Drift is solved by the SHA-256 check the same way
plugin binaries are protected today.

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

SkillReferenceWithoutCapability {
    /// The offending `${SKILL_DIR}/<path>` reference found in body.
    reference: String,
    /// `fs.read` glob entries declared in the manifest at install time
    /// (so the error message can suggest "extend one of these").
    declared_paths: Vec<String>,
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

~6 tests:
- `skill_check_succeeds_on_valid_critic_fixture`
- `skill_check_returns_content_missing_when_skill_md_absent`
- `skill_check_returns_frontmatter_invalid_on_malformed_yaml`
- `skill_check_returns_name_mismatch_when_diverged`
- `skill_check_returns_reference_without_capability_when_body_refs_uncovered_path`
- `skill_check_accepts_reference_covered_by_fs_read_glob`

### Integration tests (tau-pkg `--test install_skill_cross_check`)

~4 tests using a minimal critic fixture package at
`crates/tau-pkg/tests/fixtures/skills/critic/`:
- Happy path: skill with valid SKILL.md installs cleanly, lockfile
  records `kind = "skill"` + caches `content_sha256` +
  `frontmatter { name, description }`
- `SkillContentMissing` triggers + propagates through
  `install_with_options`
- `SkillNameMismatch` triggers + propagates
- `SkillReferenceWithoutCapability` triggers when SKILL.md body
  references `${SKILL_DIR}/refs/foo.md` but manifest's `fs.read`
  paths don't cover it

### Verify-time test (tau-pkg `--test verify_skill_drift`)

~2 tests:
- Happy path: `tau verify` returns Ok for a skill whose SKILL.md
  matches the cached `content_sha256`
- Drift: mutating SKILL.md after install produces
  `VerifyReport::SkillContentDrift { name, expected, got }`

### Snapshot tests (tau-cli `--test cmd_install_skill_render`)

~4 insta snapshots covering each of the 4 new error variants
(`SkillContentMissing`, `SkillNameMismatch`,
`SkillFrontmatterInvalid`, `SkillReferenceWithoutCapability`).

### Lockfile migration test (tau-pkg `--lib lockfile`)

~1 test: load a v4 lockfile (no `content_sha256` / `frontmatter`
fields on skill entries), verify auto-upgrade emits the once-per-
process warning and treats the missing fields as needing refresh
on next `tau install`.

## Manifest parse-time test (tau-domain)

1 unit test in tau-domain: `skill_kind_rejects_plugin_block` —
constructs a manifest with both `kind = "skill"` and a `[plugin]`
block, asserts `PackageManifestError::SkillCannotHavePluginBlock`.

## Estimated effort

4-5 days (revised up from 3-4 after strengthening #2 and #3).
Components:
- `crates/tau-pkg/src/skill_check.rs` (~150 LOC + 6 unit tests —
  hard-fail reference lint adds the most logic)
- 4 new `InstallError` variants in `crates/tau-pkg/src/error.rs`
- `install_with_options` integration in
  `crates/tau-pkg/src/install.rs` (~30 LOC including content_sha256
  computation + frontmatter snapshotting)
- Lockfile schema v4 → v5 migration (~50 LOC + 1 test)
- `tau verify` extension for `SkillContentDrift` (~30 LOC + 2 tests)
- 4 integration tests + critic fixture
- `crates/tau-cli/src/cmd/error_render.rs` extension (~40 LOC)
- 4 insta snapshots in `crates/tau-cli/tests/`
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

### Warn-only reference lint (initial draft)

The first draft of this spec emitted `tracing::warn!` when SKILL.md
referenced `${SKILL_DIR}/<path>` without a covering `fs.read`
capability. Rejected on review:
- The true-positive cost (caught at runtime when the agent gets a
  capability-denied error mid-task) is severe — confusing failure
  mode for the agent's LLM, no clear remediation path from the
  runtime error alone.
- The false-positive cost (skill author adds one `[[capabilities]]`
  line or removes a stale reference) is trivial.
- Hard-fail at install time matches how the rest of tau handles
  capability declarations: explicit > implicit. Warn-only would be
  the outlier.

### Keep lockfile schema unchanged (initial draft)

The first draft had no lockfile migration — `tau skill list` would
re-read every `SKILL.md` from disk on demand. Rejected on review:
- A scope with 30-50 installed skills incurs 30-50 file opens per
  `tau skill list`. Noticeable latency that compounds across CLI
  surfaces (autocomplete, `tau skill show`, indexing).
- Drift between snapshot and source IS a concern, but it's solved
  by the same SHA-256 mechanism that protects plugin binaries today
  (`tau verify` checks `content_sha256`).
- Cached frontmatter is ~200 bytes per skill — negligible lockfile
  growth.
- Better to migrate the schema once in Skills-2 than to ship an
  optimization PR later when list time gets slow.

## ADR

ADR-0026 will document this decision once Skills-2 ships. Open items
for the ADR:
- The inline-vs-module call for `skill_check`
- The hard-fail reference lint (and the rejected warn-only draft)
- The lockfile v4 → v5 migration adding `content_sha256` + cached
  frontmatter
- The skill-cannot-have-plugin-block validation
- Skills-2's relation to Layer 2 sandbox cross-check (skipped for
  skills)
- `SkillContentDrift` as a new `VerifyReport` variant parallel to
  `BinaryDrift`
