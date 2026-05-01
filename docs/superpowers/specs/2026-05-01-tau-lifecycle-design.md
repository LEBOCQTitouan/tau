# Spec: tau update / tau verify / tau uninstall

**Sub-project:** Tier 2 priority 7 of the tau project.

**Date:** 2026-05-01

**Closes:** [ADR-0007](../../decisions/0007-tau-cli.md) §1 reservation
— `uninstall`, `update`, `verify` were explicitly deferred to Phase 1+
when the v0.1 5-subcommand surface (`install`, `list`, `run`, `init`,
`chat`) was locked.

**Future ADR:** ADR-0012 will lock the 4 design decisions documented
here (verify primitive, update defaults, uninstall semantics, exit
code policy).

## Goals

Three lifecycle subcommands that complete the package-management
story started in Phase 0 priority 3 (`tau-pkg` + `tau install`):

- **`tau update <package>`** — re-resolve a package against its
  source's latest tag (or `--version <vN>`); update the lockfile;
  install the new version alongside the old; optionally prune.
- **`tau verify [<package>]`** — validate that an installed package's
  on-disk content still matches the lockfile's recorded SHA / source
  / version. Detect tampering or drift.
- **`tau uninstall <package>`** — remove a package's install tree
  and its lockfile entry. (Library function already exists at
  `tau_pkg::uninstall(name, version, scope)` since Phase 0; this
  sub-project wires the CLI subcommand.)

## Non-goals

- **`tau update` for all packages at once.** No `tau update` (no
  args) at v0.1. Updating every plugin in one command without
  per-package review is a footgun. Defer to a future ADR.
- **`tau uninstall` cascade.** No automatic detection of "package A
  declares B as a `requires.tools` dep, so removing B should
  refuse." Tau's installed packages live in scopes; project-level
  dep declarations are scattered across user `tau.toml`s outside
  the uninstall surface's view. Self-healing via lazy resolve
  (priority 5) is the model.
- **Cryptographic signing of packages.** Tree-hashing is integrity,
  not authenticity. Signed packages (registry signing keys, GPG, or
  Sigstore) belong to a future Phase 2+ ADR.
- **Per-file diff in verify drift reports.** v0.1 reports
  "tree hash mismatch" without identifying which file changed.
  Per-file diff is a future enhancement; cheap to add later.
- **`PackageSource` extension.** This sub-project anticipates non-git
  source variants but does not implement them. Today only
  `PackageSource::Git` exists; the verify primitive is chosen to
  scale to future variants without redesign.

## Architecture

Three new CLI subcommands and three corresponding library helpers in
`tau-pkg`:

| CLI command | CLI file | Library function | Notes |
|---|---|---|---|
| `tau update` | `crates/tau-cli/src/cmd/update.rs` | `tau_pkg::update_package` | New library function; composes existing `source_list` + resolver + `install_with_options` + optional `uninstall`. |
| `tau verify` | `crates/tau-cli/src/cmd/verify.rs` | `tau_pkg::verify` (new module) | Source-agnostic whole-tree SHA-256 recompute + compare. |
| `tau uninstall` | `crates/tau-cli/src/cmd/uninstall.rs` | `tau_pkg::uninstall` (existing) | Thin CLI wrapper; library does the heavy lifting. |

Plus shared infrastructure:

- `tau_pkg::tree_hash` (new module) — source-agnostic hash helper.
  Used at install time (populate `LockedVersion.sha256`) and at
  verify time (recompute + compare).
- `LockedPlugin.binary_sha256` (new field) — separate hash for built
  plugin binaries. Plugin binaries are the runtime trust boundary;
  source-tree integrity (manifest, src/) is a separate concern from
  binary integrity (target/release/<bin>).
- Lockfile schema v2 → v3 (additive). v2 lockfiles auto-upgrade on
  load; v2-leftover entries (empty `sha256`) are flagged
  `unverified` by `tau verify`, not `drift`.

**No new workspace member. No new CI jobs.** Branch protection stays
at 23.

## 1. Verify primitive: whole-tree SHA-256 (source-agnostic)

**Decision:** At install time, after the source materializes the
package files, compute a whole-tree SHA-256 over the install dir
(excluding `.git/` and `target/`). Store in `LockedVersion.sha256`.
On verify, recompute and compare. Plugin binaries get a separate
`LockedPlugin.binary_sha256` field.

**Rationale:**

- `PackageSource` is `#[non_exhaustive]` with only `Git` today, but
  the marker explicitly anticipates future variants (registry,
  tarball, local path). Tying verify to git primitives (`git
  rev-parse HEAD`, `git status`) would mean redoing verify each
  time a new source variant lands.
- Tree-hashing works for any source: clone → hash, unpack → hash,
  resolve path → hash, fetch from registry → hash. The recompute
  step is identical regardless of how the install got there.
- Sub-100ms for typical packages (~100 files, <1MB). Cheap enough
  to run on every `tau verify`.

### 1.1. Hash algorithm

```text
1. Walk install dir with walkdir.
2. Skip `.git/`, `target/`, and any `*.tau-tmp/` directories.
3. Sort entries lexicographically by relative path (deterministic
   across OSes / filesystems).
4. For each file:
     hash.update(rel_path.as_bytes());
     hash.update(&[0]);
     hash.update(file_contents);
     hash.update(&[0]);
5. Return format!("{:x}", hash.finalize()) — 64-char lowercase hex.
```

The `\0` separators prevent the
"path-content-pathcontent" ambiguity (where `a/bcontent` and
`a/b\0content` would collide without separators).

### 1.2. Excluded paths

- **`.git/`** — git-specific noise. Changes on `git gc`, ref updates,
  shallow-clone metadata. Not part of what the runtime depends on.
- **`target/`** — build artifacts. Recomputed on each `cargo build`,
  would always show drift. Plugin binaries are tracked separately
  via `LockedPlugin.binary_sha256`.
- **`*.tau-tmp/`** — any stale temp dirs from interrupted operations.
  Defensive.

### 1.3. Plugin binary hashing

`LockedPlugin` (lockfile schema v2) gains a `binary_sha256: String`
field. Populated at install time, after `cargo build` succeeds, by
`sha256_of_file(LockedPlugin.binary_path)`. Verified separately from
the source-tree hash because:

- Source tree (manifest, src/) drift = "did someone tamper with the
  package code on disk."
- Binary drift = "did someone replace the runtime artifact." Different
  threat models; different remediations (rebuild vs reinstall).

Older `LockedPlugin` entries (lockfile v2-leftover, no
`binary_sha256`) are flagged `unverified` for the binary, not
`drift`.

### 1.4. Trigger to revisit

- Per-file drift diagnosis (which file changed) — would require
  additional manifest storage. Cheap to add later.
- Cryptographic signing — separate ADR.
- Hash algorithm migration (BLAKE3, SHA-256 truncated, etc.) —
  hash field is opaque string; algorithm prefix (`sha256:...`)
  could be added later.

## 2. `tau update` defaults: latest tag, `--version` to pin

**Decision:** `tau update <pkg>` re-resolves the package against the
source's latest published tag (using priority 5's `source_list`).
`tau update <pkg> --version 1.2.3` jumps to that exact version.
Re-resolves transitive deps. Old versions stay installed by default;
opt-in `--prune` to remove the old active version.

**Rationale:**

- Dominant idiom (npm, cargo, gem). `tau update foo` "give me the new
  version" is what users expect.
- `--version` provides explicit pinning when needed (rollback, CI
  determinism).
- Multi-version cohabitation already exists in the lockfile
  (`LockedPackage.installed_versions: Vec<LockedVersion>`); rollback
  is `tau update <pkg> --version <old>` away.
- Re-resolving transitive deps means new `[[requires.tools]]`
  declarations in the new version automatically install. Priority
  5's "highest-compatible" picker handles existing deps that are
  still satisfied without re-installing.

### 2.1. Behavior matrix

| Command | Behavior |
|---|---|
| `tau update <pkg>` | Find latest tag matching package source's rev constraint. If newer than active, install + promote. Old versions kept. |
| `tau update <pkg> --version <v>` | Validate `<v>` is reachable from source. Install + promote. Error if version not found. |
| `tau update <pkg> --prune` | Same as `tau update <pkg>` plus `tau_pkg::uninstall(name, Some(old_active_version), scope)` after promotion. Atomic in spirit; old version removed only after new one is verified. |
| `tau update <pkg> --json` | Streaming JSON event output (mirrors priority 5 + 8 conventions). |

### 2.2. Output (human mode)

Mirrors priority 5's npm-style progress vocabulary:

```text
update <pkg>
  current: 1.0.0
  resolving: <source>
  found: 1.1.0 (latest)
  installing 1.1.0...
  ✓ installed at <scope>/packages/<pkg>/1.1.0
  promoted active: 1.0.0 → 1.1.0
```

With `--prune`:

```text
  pruning: 1.0.0
  ✓ removed <scope>/packages/<pkg>/1.0.0
```

### 2.3. Output (JSON mode)

Per spec §4.6 streaming convention, one event per stdout line:

```json
{"event":"update_started","name":"<pkg>","current":"1.0.0"}
{"event":"resolving","source":"..."}
{"event":"resolved","version":"1.1.0","tag":"v1.1.0"}
{"event":"installing","version":"1.1.0"}
{"event":"installed","version":"1.1.0","path":"..."}
{"event":"promoted","from":"1.0.0","to":"1.1.0"}
{"event":"pruned","version":"1.0.0"}     // only with --prune
{"event":"update_completed","new_active":"1.1.0"}
```

### 2.4. Error cases

- **Package not installed** → `UpdateError::PackageNotInstalled` →
  exit 2.
- **Source unreachable / `git ls-remote` fails** →
  `UpdateError::SourceList { source: SourceListError }` → exit 2.
- **`--version <v>` not reachable** → `UpdateError::Resolve { source:
  ResolveError }` → exit 2.
- **Install fails** (network, build, capability rejection) →
  `UpdateError::Install { source: InstallError }` → exit 2. The
  partially-installed dir is removed (best-effort cleanup) before
  returning.
- **`--prune` uninstall fails** after successful install →
  `UpdateError::Uninstall { source: UninstallError }` → exit 2. The
  new version is installed and active; the old version remained.
  The user can manually retry the uninstall.

### 2.5. Trigger to revisit

- `tau update` (no args) for batch update — needs an ADR amendment
  to lock the trust posture.
- `tau update --dry-run` — print what would happen without
  modifying anything.
- Atomic install-then-prune (rollback on failure) — current design
  is install-then-prune-best-effort.

## 3. `tau verify` failure mode: exit codes + JSON

**Decision:** `tau verify [<pkg>] [--version <v>] [--json]`. Default
verifies all installed packages. Exit 0 on all-verified, exit 2 on
any drift. JSON mode (`--json`) emits one event per stdout line.

**Rationale:**

- Maps to ADR-0007 §7 three-bucket exit code policy: 0 success, 1
  agent failure, 2 kernel error. Tampered packages are kernel
  trust, not agent failure → exit 2.
- Per-line JSON event shape mirrors streaming sub-project (ADR-0011)
  for consistency. CI scripts can pipe `tau verify --json | jq` and
  react to `status: "drift"` events.

### 3.1. Behavior matrix

| Command | Behavior |
|---|---|
| `tau verify` | Walk lockfile + scope's `packages/` dir. Verify every entry. Detect orphaned install dirs (no lockfile entry). |
| `tau verify <pkg>` | Verify all versions of `<pkg>`. |
| `tau verify <pkg> --version <v>` | Verify single (pkg, version). |
| `tau verify --json` | Per-line JSON event output. |

### 3.2. Drift kinds

| Kind | Trigger | Lockfile sha256 | Disk |
|---|---|---|---|
| `tree` | Source files modified (manifest, src/, etc.) | populated | exists, hash differs |
| `binary` | Plugin binary modified | populated (`binary_sha256`) | exists, hash differs |
| `missing` | Install dir deleted | populated | absent |
| `orphaned` | Lockfile entry gone, dir present | n/a | exists, no entry |
| `unverified` | v2-leftover, hash empty | empty | exists, no comparison possible |

`unverified` is **informational, not drift** — exit 0 unless other
drifts exist. Printed once with hint to "re-install <pkg> to
populate hash."

### 3.3. Output (human mode)

```text
verify <pkg>@1.0.0... ok
verify <other-pkg>@2.1.0... ✗ drift (tree)
  expected: abc123...
  actual:   xyz789...
verify <plugin>@1.2.0... ✗ drift (binary)
  path: <scope>/packages/<plugin>/1.2.0/target/release/<bin>
  expected: def456...
  actual:   ghi789...

3 packages verified, 2 drifted.
```

### 3.4. Output (JSON mode)

```json
{"event":"verify_started","total":3}
{"event":"verify_package","name":"<pkg>","version":"1.0.0","status":"ok"}
{"event":"verify_package","name":"<other-pkg>","version":"2.1.0","status":"drift","kind":"tree","expected":"abc...","actual":"xyz..."}
{"event":"verify_package","name":"<plugin>","version":"1.2.0","status":"drift","kind":"binary","path":"...","expected":"def...","actual":"ghi..."}
{"event":"verify_completed","total":3,"ok":1,"drift":2,"unverified":0}
```

### 3.5. Trigger to revisit

- Per-file drift diagnosis (which file in the tree changed).
- `tau verify --fix` to auto-reinstall drifted packages.
- Parallel verify for many packages.

## 4. `tau uninstall` semantics: permissive + remediation hint

**Decision:** `tau uninstall <pkg>` operates on the lockfile and
install tree only — no cross-project dep tracking. Print a
remediation hint pointing to project `tau.toml`s and `tau resolve`
for self-healing.

**Rationale:**

- Tau's installed packages aren't a single shared system — they live
  in scopes. Other projects' `tau.toml`s outside the current scope
  are invisible at uninstall time.
- The `[[requires.tools]]` graph lives in **project** files, not the
  lockfile. `tau uninstall` operates on the lockfile + install tree;
  it has no project-level context.
- Self-healing: if a user uninstalls a package they still need, the
  next `tau run` / `tau chat` re-resolves and re-installs (priority
  5's lazy resolve). Surprise minimization: the system "just works"
  again on next use.
- Cascade-detection (option A in the brainstorm) would require
  scanning the entire user's home for `tau.toml`s — privacy-invasive
  and unreliable.

### 4.1. Behavior matrix

| Command | Behavior |
|---|---|
| `tau uninstall <pkg>` | Remove all versions; remove lockfile entry. |
| `tau uninstall <pkg> --version <v>` | Remove single version; promote highest remaining as active; remove lockfile entry only if no versions remain. |

The library function `tau_pkg::uninstall(name, version, scope)`
already exists at `crates/tau-pkg/src/install.rs:612` and handles
all these cases (including active-version promotion). The CLI is
a thin wrapper.

### 4.2. Output (human mode)

```text
✓ Uninstalled <pkg>@<v>.
  Removed: <scope>/packages/<pkg>/<v>
  Lockfile: <scope>/lock.toml

If any project still depends on <pkg>:
  • Remove or update the [[agents.<id>.requires.tools]] entry
    for <pkg> in the project's tau.toml.
  • Or re-install on next run: cd <project> && tau resolve
```

The remediation hint always prints — even when no projects depend
on the package. Reasoning: cheap to print, and users can't always
remember which projects depend on which packages. The hint is
informational; actionable for those who need it, ignorable for
those who don't.

### 4.3. Output (JSON mode)

```json
{"event":"uninstall_started","name":"<pkg>","version":"<v>"}
{"event":"removed_dir","path":"<scope>/packages/<pkg>/<v>"}
{"event":"lockfile_updated","entries_remaining":N}
{"event":"uninstall_completed","name":"<pkg>","version":"<v>"}
```

### 4.4. Error cases

- **Package not installed** → `UninstallError::NotInstalled` → exit 2.
- **Version not installed** → `UninstallError::VersionNotInstalled`
  → exit 2.
- **Filesystem permission** → `UninstallError::Io` → exit 2.

### 4.5. Trigger to revisit

- `tau uninstall --dry-run`.
- `tau uninstall --force` to override a future cascade-detection
  feature.
- Cross-scope uninstall (today: scope is determined by CWD's
  project).

## 5. Lockfile schema migration: v2 → v3

**Additive changes only.** v2 lockfiles auto-upgrade on next save.

### 5.1. New / populated fields

- `LockedVersion.sha256` — already exists (lockfile v2), currently
  empty at v0.1. Populated at install time by `tau_pkg::tree_hash`.
- `LockedPlugin.binary_sha256` — new field, lockfile v3. Populated
  at install time after successful `cargo build`. Skipped (left
  empty) for non-plugin packages.

### 5.2. Backwards compatibility

- v2 lockfiles load successfully into v3 in-memory representation
  (`#[serde(default)]` on the new field; empty for v2-leftover
  entries).
- `tau verify` flags v2-leftover entries with status `unverified`
  rather than `drift` (exit 0 unless other drifts exist).
- `tau update <pkg>` populates the hash on the new version's entry
  even if the rest of the lockfile is v2-leftover.
- A future `tau install --rehash` (out of scope here) could
  retroactively populate v2 entries; v0.1 just lets new
  installs/updates fill in over time.

### 5.3. Schema version field

`LockFile.schema_version: u32` already tracks this. The save path
bumps to `3` when any `LockedPlugin` carries a non-empty
`binary_sha256`. This is a soft signal — neither `tau verify` nor
`tau update` rejects v2 lockfiles. The bump just helps users
diagnose "why does my old lockfile still report unverified."

## 6. Error handling (per ADR-0009 typed-error policy)

### 6.1. New typed enums

```rust
// crates/tau-pkg/src/error.rs (or a new dedicated file)

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("loading lockfile: {source}")]
    LockfileLoad { source: LockfileError },

    #[error("package {name} not installed")]
    PackageNotInstalled { name: String },

    #[error("package {name}: version {version} not installed")]
    VersionNotInstalled { name: String, version: String },

    #[error("io error at {path}: {message}")]
    Io { path: PathBuf, message: String },
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("loading lockfile: {source}")]
    LockfileLoad { source: LockfileError },

    #[error("package {name} not installed; run `tau install <source>` first")]
    PackageNotInstalled { name: String },

    #[error("source listing failed: {source}")]
    SourceList { source: SourceListError },

    #[error("resolving {name}: {source}")]
    Resolve { name: String, source: ResolveError },

    #[error("installing {name}@{version}: {source}")]
    Install { name: String, version: String, source: InstallError },

    #[error("pruning old version of {name}: {source}")]
    Uninstall { name: String, source: UninstallError },
}
```

### 6.2. Existing error chain unchanged

- `tau_pkg::UninstallError` — unchanged.
- `tau_pkg::InstallError` — unchanged.
- `tau_pkg::ResolveError` — unchanged.
- `tau_pkg::SourceListError` — unchanged.
- `tau_pkg::LockfileError` — unchanged.

### 6.3. tau-cli conversions

`tau_cli::CliError` (existing) gains:

```rust
impl From<tau_pkg::VerifyError> for CliError { ... }
impl From<tau_pkg::UpdateError> for CliError { ... }
// From<UninstallError> likely already exists from earlier work; verify.
```

The conversions map kernel-tier errors to exit code 2 via the
existing CliError → exit-code dispatch.

## 7. Testing tier

### 7.1. Unit tests in `tau-pkg`

- `tree_hash` module: empty dir; single file; nested dirs;
  `.git/` exclusion; `target/` exclusion; deterministic ordering
  (call hash twice → same result); unicode filenames; binary file;
  symlinks (skipped — `walkdir::WalkDir::follow_links(false)`. Source
  trees that depend on symlink-resolved content are out of scope; the
  hash records "this is a symlink to X" via the symlink's target
  path bytes, not the resolved file's content. Locked here to prevent
  symlink loop pitfalls during walk.).
- `verify` module: ok path; tree drift; binary drift; missing dir;
  orphaned dir; v2-leftover unverified path.
- `update_package` function: resolves new version; installs alongside;
  promotes active; optionally prunes old; handles "no newer version
  available" cleanly.

### 7.2. E2E tests via `tau-cli/tests/`

Mirror the `cmd_install_*` test pattern: file:// git fixtures, no
real network in CI.

- `cmd_update.rs`:
  - Update v1.0 → v1.1 (latest tag).
  - Update v1.0 → v1.2 (explicit `--version`).
  - Update with `--prune` (old version removed).
  - Update fails on unreachable version (exit 2).
- `cmd_verify.rs`:
  - All-verified install (exit 0).
  - Tamper one file → exit 2 + tree drift event.
  - Tamper binary → exit 2 + binary drift event.
  - Remove install dir → exit 2 + missing event.
  - `tau verify --json` parses line-by-line.
  - v2-leftover lockfile → unverified event, exit 0 unless other
    drifts.
- `cmd_uninstall.rs`:
  - Uninstall whole package.
  - Uninstall single version (active promotion).
  - Not-installed error (exit 2).
  - Remediation hint appears in stdout.

### 7.3. Help snapshot updates

- `tau --help` (new top-level subcommands).
- `tau update --help`, `tau verify --help`, `tau uninstall --help`.
- Insta snapshots accepted.

## 8. ADR-0012 outline

Locks four design decisions:

1. **Whole-tree SHA-256 verify** (source-agnostic; anticipates non-git
   `PackageSource` variants).
2. **`tau update` defaults to latest tag**, `--version` to pin,
   `--prune` opt-in.
3. **`tau uninstall` is permissive** (no cascade), prints
   remediation hint guiding the user to project `tau.toml`s and
   `tau resolve`.
4. **Verify exit codes** reuse the 3-bucket policy from ADR-0007 §7
   (0 / 1 / 2).

Plus invariants:
- Hash excludes `.git/`, `target/`, `*.tau-tmp/`.
- Binary hashes stored separately from source-tree hashes.
- Lockfile schema v2 → v3 additive.

## 9. Task outline (~10-12 tasks for the implementation plan)

The implementation plan (next step) will derive ~10-12 tasks. Likely
structure:

1. `tau_pkg::tree_hash` module + unit tests.
2. Populate `LockedVersion.sha256` in `install_with_options` + add
   `LockedPlugin.binary_sha256` field (lockfile v3).
3. `tau_pkg::verify` module + unit tests.
4. `tau_pkg::update_package` library function + unit tests.
5. `cmd/uninstall.rs` CLI subcommand + e2e tests.
6. `cmd/verify.rs` CLI subcommand + e2e tests.
7. `cmd/update.rs` CLI subcommand + e2e tests.
8. Help snapshot updates (insta accept).
9. PAUSE — final verification + open PR.
10. PAUSE — ADR-0012 + ROADMAP Tier 2 priority 7 done + squash merge.

(Plus possibly a separate task for error type definitions if they
don't fit cleanly into Tasks 3-7.)

## 10. References

- ADR-0007 §1 — the deferral this sub-project closes.
- ADR-0007 §7 — three-bucket exit code policy (reused for verify).
- ADR-0009 — typed-error policy.
- ADR-0011 — JSON event-per-line streaming convention (reused for
  update / verify JSON modes).
- Phase 0 priority 3 — `tau-pkg` + `tau install` foundation.
- Tier 2 priority 5 — `source_list` + resolver (reused for `tau
  update`'s version discovery).
- `crates/tau-pkg/src/install.rs:612` — existing
  `tau_pkg::uninstall`.
- `crates/tau-pkg/src/lockfile.rs:178` — `LockedVersion.sha256`
  reserved field.
