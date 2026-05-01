# ADR-0012: tau update / verify / uninstall lifecycle commands

**Status:** Accepted
**Date:** 2026-05-01
**Deciders:** Titouan Lebocq
**Supersedes:** —
**Closes:** [ADR-0007](0007-tau-cli.md) §1 — `uninstall`, `update`,
`verify` were explicitly deferred to Phase 1+ when the v0.1
5-subcommand surface (`install`, `list`, `run`, `init`, `chat`) was
locked.
**Amends:** —
**Refines:** [ADR-0007](0007-tau-cli.md) §7 (three-bucket exit code
policy — this ADR explicitly maps verify-drift and lifecycle errors
into the 0/1/2 buckets).

## Context

Phase 0 priority 3 shipped `tau-pkg` and `tau install`. The
`LockedVersion.sha256` field was reserved for "Phase-1 `tau verify`
content hashing"; the `tau_pkg::uninstall(name, version, scope)`
library function was already implemented at install.rs:612.
ADR-0007 §1 deferred the three lifecycle subcommands explicitly:

> Deferred to Phase 1+: `uninstall`, `update`, `verify`, `workflow`
> and any orchestration verbs.

After Tier 2 priorities 4 (capability override), 5 (transitive
dependency resolution), 6 (tool-args schema validation), and 8
(streaming LLM responses) shipped, only this priority and one Tier 2
slot remained open. This ADR closes priority 7's scope.

The constraints driving the four design decisions below:

1. **`PackageSource` is `#[non_exhaustive]`**. Today only the `Git`
   variant exists, but the marker explicitly anticipates future
   variants (registry, tarball, local path). Verify primitives that
   tie to git operations (`git rev-parse HEAD`, `git status
   --porcelain`) would need redesign for each new source — wrong
   coupling.
2. **The library function `tau_pkg::uninstall` already exists** and
   handles whole-package, per-version, active-version-promotion, and
   lockfile mutation. The CLI is a thin wrapper.
3. **Multi-version cohabitation already works** in the lockfile via
   `LockedPackage.installed_versions: Vec<LockedVersion>`. Update
   semantics build on top.
4. **The exit-code policy from ADR-0007 §7** (0 success, 1 agent
   failure, 2 kernel error) maps cleanly to verify drift (kernel
   trust → 2) and lifecycle errors (kernel error → 2).

## Decision

Four inter-locking commitments:

### 1. Whole-tree SHA-256 verify, source-agnostic

**Decision:** At install time, after the source materializes the
package files, compute a whole-tree SHA-256 over the install dir
(excluding `.git/`, `target/`, and `*.tau-tmp/`). Store in
`LockedVersion.sha256`. On verify, recompute and compare. Plugin
binaries get a separate `LockedPlugin.binary_sha256` field
(introduced in lockfile schema v3).

**Rationale:**

`PackageSource` is `#[non_exhaustive]` — future variants are
inevitable. A verify primitive that ties to git operations would
need redesign for each new source. Tree-hashing works for any
source: clone → hash, unpack tarball → hash, resolve local path →
hash, fetch from registry → hash. The recompute step is identical
regardless of how the install got there.

Sub-100ms for typical packages (~100 files, <1MB). Cheap enough to
run on every `tau verify`, including in CI.

#### 1.1. Excluded paths

- **`.git/`** — git-specific noise. Changes on `git gc`, ref
  updates, shallow-clone metadata. Not part of what the runtime
  depends on.
- **`target/`** — build artifacts. Recomputed on each `cargo
  build`, would always show drift. Plugin binaries are tracked
  separately via `LockedPlugin.binary_sha256`.
- **`*.tau-tmp/`** — any stale temp dirs from interrupted operations.
  Defensive.

#### 1.2. Hash algorithm

```text
1. Walk install dir with walkdir, follow_links(false).
2. Skip `.git/`, `target/`, `*.tau-tmp/` directories.
3. Sort entries lexicographically by relative path.
4. For each file:
     hasher.update(rel_path.as_bytes());
     hasher.update(&[0]);
     hasher.update(file_contents);
     hasher.update(&[0]);
5. Return format!("{:x}", hash.finalize()) — 64-char lowercase hex.
```

The `\0` separators prevent path-content ambiguity (e.g. `a/bcontent`
vs `a/b\0content`). Symlinks are NOT followed; a symlink contributes
its target path bytes (not the resolved file content) to the hash —
preventing symlink-loop pitfalls during walk.

#### 1.3. Plugin binary hashing

`LockedPlugin` (lockfile schema v2) gains a `binary_sha256: String`
field at v3. Populated at install time after `cargo build` succeeds,
via `sha256_of_file(LockedPlugin.binary_path)`. Verified separately
from the source-tree hash because:

- Source tree drift = "did someone tamper with the package code on
  disk."
- Binary drift = "did someone replace the runtime artifact." These
  are different threat models with different remediations (rebuild
  vs reinstall).

Older `LockedPlugin` entries from lockfile v2-leftover (no
`binary_sha256`) are flagged `unverified` for the binary, NOT
`drift`.

#### 1.4. Trigger to revisit

- Per-file drift diagnosis ("file X changed at line Y") — would
  require additional manifest storage. Cheap to add later as a
  `--diff` flag on `tau verify`.
- Cryptographic signing (registry signing keys, GPG, Sigstore) —
  separate Phase 2+ ADR.
- Hash algorithm migration (BLAKE3, etc.) — hash field is opaque
  string; algorithm prefix (`sha256:...`) could be added later.

### 2. `tau update` defaults to latest tag

**Decision:** `tau update <pkg>` re-resolves the package against
the source's latest published tag (using priority 5's
`source_list::list_versions_at_source`). `tau update <pkg>
--version 1.2.3` jumps to that exact version. Re-resolves
transitive deps. Old versions stay installed by default; opt-in
`--prune` removes the previous active version after the new
install succeeds.

**Rationale:**

Dominant idiom (npm, cargo, gem). Users expect "give me the new
version of X" by default. Multi-version cohabitation already exists
in the lockfile (`LockedPackage.installed_versions: Vec<LockedVersion>`);
rollback is `tau update <pkg> --version <old>` away.

Re-resolving transitive deps means new `[[requires.tools]]`
declarations in the new version automatically install. Priority 5's
"highest-compatible" picker handles existing deps that are still
satisfied without re-installing.

#### 2.1. Behavior matrix

| Command | Behavior |
|---|---|
| `tau update <pkg>` | Find latest tag matching package source's rev constraint. If newer than active, install + promote. Old versions kept. |
| `tau update <pkg> --version <v>` | Validate `<v>` is reachable from source. Install + promote. Error if version not found. |
| `tau update <pkg> --prune` | Same as `tau update <pkg>` plus `tau_pkg::uninstall(name, Some(old_active_version), scope)` after promotion. |
| `tau update <pkg> --json` | Streaming-style JSON event output (post-call summary; library is synchronous). |

#### 2.2. JSON event shape

The library function returns synchronously after all work completes,
so per-step progress events (resolving / installing / etc.) would
require library refactoring. v0.1 emits a two-event pair:

```json
{"event":"update_started","name":"...","current":"1.0.0"}
{"event":"update_completed","name":"...","from":"1.0.0","to":"1.1.0","pruned":true|false,"transitive_deps_changed":[...]}
```

On failure: `update_started` is emitted, then no `update_completed`
follows. JSON consumers detect failure via exit code 2 + stderr.

#### 2.3. No `tau update` (no args) at v0.1

Updating every plugin in one command without per-package review is a
footgun. The `Default<Risky>` posture from QG14 says "don't ship
defaults that surprise the user with broad effects." `tau update`
with no args is omitted; a future ADR can revisit if there's
demonstrated demand.

#### 2.4. Trigger to revisit

- Batch update (`tau update` no args) — needs an ADR amendment to
  lock the trust posture.
- `tau update --dry-run` — print what would happen without modifying
  anything.
- Atomic install-then-prune (rollback on failure) — current design
  is install-then-prune-best-effort.
- Per-step progress JSON events — needs library refactoring to
  expose hooks.

### 3. `tau uninstall` is permissive (no cascade)

**Decision:** `tau uninstall <pkg>` operates on the lockfile and
install tree only — no cross-project dep tracking. Print a
remediation hint pointing to project `tau.toml`s and `tau resolve`
for self-healing.

**Rationale:**

Tau's installed packages aren't a single shared system — they live
in scopes (per-user, per-project). The `[[requires.tools]]` graph
lives in **project** files, not the lockfile. `tau uninstall` operates
on the lockfile + install tree; it has no project-level context.

Cascade-detection would require scanning the entire user's home for
`tau.toml`s — privacy-invasive and unreliable.

Self-healing is the alternative: if a user uninstalls a package they
still need, the next `tau run` / `tau chat` re-resolves and
re-installs (priority 5's lazy resolve). The system "just works" again
on next use.

#### 3.1. Remediation hint

Always printed (informational; ignorable for those who don't need
it):

```text
✓ Uninstalled <pkg>@<v>.
  Removed: <scope>/packages/<pkg>/<v>
  Lockfile: <scope>/lock.toml

If any project still depends on <pkg>:
  • Remove or update the [[agents.<id>.requires.tools]] entry
    for <pkg> in the project's tau.toml.
  • Or re-install on next run: cd <project> && tau resolve
```

The hint always prints because users can't always remember which
projects depend on which packages. Cheap to print, actionable for
those who need it.

#### 3.2. Behavior matrix

| Command | Behavior |
|---|---|
| `tau uninstall <pkg>` | Remove all versions; remove lockfile entry. |
| `tau uninstall <pkg> --version <v>` | Remove single version; promote highest remaining as active; remove lockfile entry only if no versions remain. |

The library function `tau_pkg::uninstall(name, version, scope)`
already exists at `crates/tau-pkg/src/install.rs:612` (Phase 0) and
handles all these cases — including active-version promotion. The
CLI is a thin wrapper.

#### 3.3. Trigger to revisit

- `tau uninstall --force` to override a future cascade-detection
  feature.
- `tau uninstall --dry-run` — preview what would be removed.
- Cross-scope uninstall (today: scope is determined by CWD's
  project).

### 4. Verify exit codes reuse the 3-bucket policy

**Decision:** `tau verify` exits 0 if all packages verified or
unverified (v2-leftover); exits 2 if any drift detected (tree,
binary, missing). Drift kinds are reported via JSON status field
(`drift` with `kind: tree|binary|missing`) or human stderr lines.

**Rationale:**

Tampered packages are a kernel-trust issue, not an agent issue, so
exit 2 (kernel error) is correct per ADR-0007 §7. The 3-bucket
policy is:

- **0** — success: lifecycle command succeeded.
- **1** — agent failure: not applicable to lifecycle commands
  (reserved for `tau run`'s `RunOutcome::Failed`).
- **2** — kernel error: lifecycle error OR verify drift.

#### 4.1. Drift kinds

| Kind | Trigger | Lockfile sha256 | Disk |
|---|---|---|---|
| `tree` | Source files modified (manifest, src/, etc.) | populated | exists, hash differs |
| `binary` | Plugin binary modified | populated (`binary_sha256`) | exists, hash differs |
| `missing` | Install dir deleted | populated | absent |
| `unverified` | v2-leftover, hash empty | empty | exists, no comparison possible |

`unverified` is **informational, not drift** — exit 0 unless other
drifts exist. Printed with hint to "re-install <pkg> to populate
hash" (deferred polish; current v0.1 just labels the status).

#### 4.2. JSON event shape

Mirrors ADR-0011's per-line streaming convention:

```json
{"event":"verify_started","total":N}
{"event":"verify_package","name":"...","version":"...","status":"ok"}
{"event":"verify_package","name":"...","version":"...","status":"drift","kind":"tree","expected":"...","actual":"..."}
{"event":"verify_package","name":"...","version":"...","status":"unverified"}
{"event":"verify_completed","total":N,"ok":N,"drift":M,"unverified":K}
```

#### 4.3. Trigger to revisit

- Per-file drift diagnosis (which file changed) — `--diff` flag.
- `tau verify --fix` to auto-reinstall drifted packages.
- Parallel verify for many packages (current implementation is
  sequential).
- Orphaned install-dir detection (lockfile entry gone but install
  dir present) — currently scoped out of v0.1; can be added at the
  CLI layer without changing the library.

## Consequences

### Negative / new cost

- `tau-pkg` gains transitive deps on `sha2 = "0.10"` and `walkdir =
  "2"` (~150KB compiled total). Negligible vs. existing
  `git2`/`tokio`/etc. footprint.
- `tau-pkg` gains 3 new modules (`tree_hash`, `verify`, `update`)
  totaling ~700 LOC + tests. Code organization is clean: one module
  per logical responsibility.
- `LockedPlugin` gains a `binary_sha256: String` field. Lockfile
  schema bumps v2 → v3. Additive: v2 lockfiles auto-upgrade on next
  save. v2-leftover entries (empty `binary_sha256` + empty
  `LockedVersion.sha256`) are flagged `unverified` by `tau verify`,
  NOT `drift`.
- `tau install` is now O(file-count + file-bytes) at install time
  for the source-tree hash. Sub-100ms for typical packages; not a
  measurable regression.
- `Runtime::run_with_history`-equivalent JSON-event-per-line
  convention now applies to `tau update --json` and `tau verify
  --json`. Future commands joining the convention should mirror.

### Positive

- `tau verify` provides the integrity-check primitive ADR-0006
  reserved at v0.1. Source-agnostic by design — future
  `PackageSource` variants reuse the verify primitive without
  redesign.
- `tau update <pkg>` matches user expectations (npm/cargo/gem
  defaults). `--version` to pin, `--prune` to clean up.
- `tau uninstall <pkg>` finally exists as a CLI surface. The
  library function existed since Phase 0; this just wires it up.
- Self-healing via lazy resolve (priority 5's lazy install)
  means `tau uninstall <pkg>` is non-destructive in practice — the
  next `tau run` / `tau chat` re-installs if needed. The remediation
  hint guides users to the right place.
- Lockfile schema v2 → v3 is forward-compatible with future drift
  diagnostics, signing, and per-file hashing.

### Neutral / new obligations

- Future `tau verify` extensions (per-file diff, `--fix`, parallel
  verify) require their own ADRs (QG18). The hash format is locked
  at this ADR; any change requires a new ADR.
- The `sha2 = "0.10"` and `walkdir = "2"` workspace dep versions
  are pinned to the major. Major upgrades verify the public API
  surface.

## Alternatives considered

### A. Git-revision verify (`git rev-parse HEAD` + `git status --porcelain`)

Rejected. Reuses git's content addressing for free; cheap to
implement (~30 LOC). However: ties verify to the `Git` variant of
`PackageSource`. Future variants (registry, tarball, local path)
would need their own verify primitive. Wrong coupling.

The early brainstorm leaned toward this option until the user
flagged the non-git futures. Pivoting to whole-tree SHA-256
makes verify source-agnostic from day one.

### B. Per-file SHA-256 manifest (`.tau/manifest.json`)

Rejected. Granular ("file X changed at line Y" reachable). Disk
overhead per install (~50KB). Implementation complexity (manifest
write at install, walk + diff at verify, manifest schema
versioning).

The whole-tree single-SHA approach is leaner for v0.1: same
correctness primitive, smaller disk + code footprint, easier to
reason about. Per-file granularity can land later as a
`tau verify --diff` flag if real-world drift reports demand it.

### C. `tau update` (no args) batch update

Rejected. Updating every plugin in one command without per-package
review is a footgun. ADR-0007's QG14 (`Default<Risky>`) argues
against shipping defaults that surprise users with broad effects.
v0.1: explicit per-package update only. Defer the batch case to a
future ADR if there's demonstrated demand.

### D. `tau uninstall` with cascade detection

Rejected. Cross-project dep tracking would require scanning the
user's entire home directory for `tau.toml`s — privacy-invasive,
unreliable, and slow. Self-healing via priority 5's lazy resolve
is the safer default. The remediation hint guides users to the right
place if they need it.

### E. Cryptographic signing instead of (or alongside) hashing

Rejected for v0.1. Signing answers "did the publisher actually
release this content?" — different question than "did it change
since I installed it?" The hashing approach is the integrity
primitive; signing is the authenticity primitive. They compose; ship
hashing first. Signing belongs to a future Phase 2+ ADR (registry
infrastructure, key management, revocation are all major scope).

### F. Track `LockedPlugin.binary_sha256` inline in `LockedVersion.sha256`

Rejected. Conflating source-tree drift with binary drift muddies
the threat model. Source = "package authors edited code"; binary =
"build artifact replaced." Different remediations (rebuild from
source vs reinstall from source). Separating the fields keeps
`tau verify`'s output legible: `kind=tree` vs `kind=binary` is
informational.

## References

- Spec: `docs/superpowers/specs/2026-05-01-tau-lifecycle-design.md`
- Plan: `docs/superpowers/plans/2026-05-01-tau-lifecycle.md`
- ADR-0007 §1 — the deferral this ADR closes.
- ADR-0007 §7 — three-bucket exit code policy reused for verify.
- ADR-0009 — typed-error policy; new error variants follow this
  (`VerifyError`, `UpdateError`).
- ADR-0011 — JSON event-per-line streaming convention reused for
  `tau update --json` and `tau verify --json`.
- `crates/tau-pkg/src/tree_hash.rs` — the source-agnostic walker.
- `crates/tau-pkg/src/verify.rs` — drift detection.
- `crates/tau-pkg/src/update.rs` — composition over existing APIs.
- `crates/tau-pkg/src/install.rs:612` — pre-existing
  `tau_pkg::uninstall` from Phase 0.
- `crates/tau-pkg/src/lockfile.rs` — `LockedPlugin.binary_sha256`
  field (lockfile schema v3).
- `crates/tau-cli/src/cmd/{update,verify,uninstall}.rs` — CLI
  wrappers.
- `crates/tau-cli/tests/cmd_{update,verify,uninstall}.rs` — e2e
  tests via `file://` git fixtures.
