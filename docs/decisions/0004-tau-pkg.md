# ADR-0004: tau-pkg package manager — public API, storage layout, lockfile

**Status:** Proposed
**Date:** 2026-04-27
**Supersedes:** —

## Context

tau-pkg (sub-project 3) is the package manager that implements `tau install`
from git URLs, as specified by ROADMAP row 3 and Constitution G7, G8, G14.
It is the first sub-project that performs real I/O: git clone, file-system
mutation, TOML parse-and-write. The earlier sub-projects (tau-domain,
tau-ports) were pure data structures and trait boundaries.

Per QG18, public API additions require an ADR. This ADR records the decisions
made during the sub-project 3 implementation and locks them at v0.1.

Relevant Constitution constraints: G7 (package manager is the only way to add
extensions), G8 (global + project scopes), G14 (capabilities declared at
install), NG9 (no credential management), NG11 (developer tool, not an
end-user consumer product), NG12 (runtime not framework).

Note that ADR-0005 (custom serde for `PackageSource` and `PackageKind`) was
filed mid-implementation as a focused fix. ADR-0004 records the broader
tau-pkg decisions and cross-references ADR-0005 where relevant.

## Decision

### 1. Sync public API

The `tau_pkg` public surface (`install`, `uninstall`, `list`, `get`,
`read_manifest`, `Scope::resolve`) is fully synchronous — no `async fn`, no
`tokio` runtime dependency. The only v0.1 caller is `tau-cli`, a CLI tool
where blocking on git clone is acceptable and concurrency adds no value.

Trigger to revisit: a concrete async-server use case where multiple packages
must be installed concurrently, or a long-running daemon that must remain
responsive during installs.

### 2. Shell out to the `git` binary

All git operations (clone, rev-parse) invoke the system `git` binary via
`std::process::Command`. tau is a developer tool (NG11); users have git on
PATH. This eliminates a C dependency (`libgit2` via `git2-rs`) and reduces
binary size.

Trigger to revisit: shipping pre-built binaries to non-developer environments
where `git` is not guaranteed on PATH (NG11 guard).

### 3. Storage layout

Packages are stored at `<scope>/.tau/packages/<name>/<version>/`, where
`<scope>` is either the project root or the global home directory. This
layout supports multi-version cohabitation: two packages at different versions
live under different paths and do not conflict. The `.tau/` directory is
considered local state and is not committed (see decision 6).

### 4. Scope resolution

tau-pkg walks up the directory tree looking for a `.tau/` directory (or the
project root marker). If no project scope is found, it falls back to the
global scope at `~/.tau`. The global directory respects `$TAU_HOME` if set,
and `$XDG_DATA_HOME/tau` if `$TAU_HOME` is absent and `$XDG_DATA_HOME` is
set.

This walk-up matches developer intuitions from tools like `git` and `npm`.
The two-level fallback (env vars → XDG → `~/.tau`) follows the XDG Base
Directory specification.

### 5. Lockfile location

Project lockfile: `<project-root>/tau-lock.toml` — at the project root,
alongside `tau.toml` and `Cargo.toml`. It is intended to be committed to
version control (reproducible installs).

Global lockfile: `~/.tau/tau-lock.toml` — inside the global tau home
directory, treated as local state and not committed anywhere.

The project lockfile lives outside `.tau/` deliberately: it must be committed
while `.tau/` must not. Placing them in the same directory would require
selective `.gitignore` entries that are fragile and easy to mis-configure.

### 6. `.tau/` is gitignored local state

The `.tau/` directory holds installed package trees and the advisory lock
file. It is local, machine-specific state and must not be committed.
`tau init` (Phase 1, in `tau-cli`) will print a hint recommending that the
user add `.tau/` to `.gitignore`, but does NOT modify `.gitignore`
automatically. The hint is pure information; the user owns their gitignore.

### 7. Lockfile schema

The lockfile is versioned (`schema_version = 1`). Each locked entry
(`[[packages]]`) records `name`, `version`, `source`, `kind`, and `sha256`.
At v0.1 the `sha256` slot is written as an empty string. Phase 1+ `tau verify`
will populate it by hashing the installed package tree. The schema version
field allows future readers to detect and reject or migrate incompatible
formats.

### 8. Public API verbs

The v0.1 public API exports six operations: `install`, `uninstall`, `list`,
`get`, `read_manifest`, and `Scope::resolve`. `update` and `verify` are
deferred to Phase 1+; no stubs are included in the v0.1 crate surface to
avoid committing a signature before the implementation exists.

### 9. Manifest location convention

`tau-pkg` expects the package manifest at `tau.toml` in the root of the
cloned repository. This is a convention, not enforced by a config field.
ADR-0002 established the manifest schema; ADR-0005 established its serde
representation. tau-pkg reads the manifest using `read_manifest`, which
returns a typed `UncheckedManifest`.

### 10. No transitive dependency resolution at v0.1

The `dependencies` field in `tau.toml` is parsed and stored but is
informational only. `tau install A` does not automatically install A's
declared dependencies. Users must `tau install B` separately. This keeps the
v0.1 resolver trivially correct (no resolution graph, no version conflict
analysis) and avoids committing a resolution algorithm before real use cases
are known.

Trigger to revisit: the first concrete multi-package workflow where manual
chaining becomes a user pain point.

### 11. Concurrent install protection

tau-pkg acquires an advisory exclusive file lock (`<scope>/.tau/tau-pkg.lock`)
at the start of every mutating operation (`install`, `uninstall`) via
`fs4::FileExt::lock_exclusive`. If `block_on_lock` is `false`, it uses
`try_lock_exclusive` and returns an error immediately instead of blocking.
The lock prevents two concurrent `tau install` invocations from corrupting
the package tree (a real risk in CI pipelines that run multiple install
scripts in parallel).

The lock is advisory: it does not protect against callers that skip tau-pkg
and write to `.tau/packages/` directly. This is acceptable; such callers
are outside the defined API.

### 12. Error taxonomy

Each public operation has its own typed error enum (`InstallError`,
`UninstallError`, `ListError`, `GetError`). There is no top-level `PkgError`
umbrella. Variant composition uses `#[from]` so `?` propagation works without
manual mapping. This mirrors the tau-domain and tau-ports error policy
(see ADR-0003 §3).

### 13. `file://` scheme support in `PackageSource`

`PackageSource::Git` accepts `file://` URLs in addition to `https`, `http`,
`ssh`, and `git` schemes (an extension to tau-domain made during the
tau-pkg integration test phase). This enables hermetic local test fixtures
(no network required in CI) and supports air-gapped installs from a local
mirror.

The `file://` scheme is a first-class extension point: `GitLocation::from_str`
in tau-domain accepts `file` alongside the other schemes, and the serde
round-trip (ADR-0005) preserves it as the canonical string form.

### 14. `protocol.file.allow=always` override in `Git::clone`

git 2.38+ sets `protocol.file.allow=user` by default as a mitigation for
[CVE-2022-39253](https://nvd.nist.gov/vuln/detail/CVE-2022-39253). With that
default, cloning a `file://` URL on a stock git 2.38+ installation "succeeds"
with exit code 0 but writes nothing — the protocol layer silently blocks the
transfer.

tau-pkg passes `-c protocol.file.allow=always` when invoking `git clone` with
a `file://` source. The CVE-2022-39253 attack vector requires
`--recurse-submodules`; tau-pkg does not use submodules. The override is
therefore safe for the current implementation. This analysis must be
re-checked if tau-pkg ever adds submodule support.

## Consequences

### Positive

- v0.1 ships a working `install` / `uninstall` / `list` / `get` path against
  real git repositories, with no async runtime dependency for callers.
- 73 unit tests + 24 integration tests cover the public API surface.
- Per-operation typed errors enable `?`-propagation throughout the codebase
  and give callers precise match arms.
- `file://` support lets integration tests run hermetically in CI with no
  network access.

### Negative

- The sync API blocks the calling thread during git clone. This is fine for a
  CLI, but embeds in an async server would require `spawn_blocking`. Trigger
  to revisit: a concrete async-server use case.
- `file://` support requires the `protocol.file.allow=always` env override.
  The security analysis (CVE-2022-39253 not applicable because we do not use
  `--recurse-submodules`) is documented here, but must be re-checked if
  submodule support is ever added.
- No transitive dependency resolution: `tau install A` does not install A's
  declared dependencies. Users must install each dependency manually.
- v0.1 lockfile has an empty `sha256` field — Phase 1+ `tau verify` will
  populate it, but until then `tau-lock.toml` cannot be used to detect
  tampered installs.

### Neutral / new obligations

- Future PRs that change tau-pkg's public API surface require their own ADRs
  (QG18).
- ADR-0005 governs the manifest TOML serde format for `PackageSource` and
  `PackageKind`; this ADR governs the broader tau-pkg API and storage
  decisions.
- The walk-up scope resolution algorithm is now the v0.1 standard; changing
  it (e.g., to never fall back to global scope) requires a new ADR.
- Adding submodule support in the future requires revisiting the
  `protocol.file.allow=always` security analysis in decision 14.

## Alternatives considered

### A. Async-first public API

Rejected for v0.1. The only caller is `tau-cli` (a CLI, not a server), there
is no concurrency value at the call site, and adding a `tokio` dependency
would impose a runtime on every future crate that links tau-pkg. Cost vs.
benefit is poor; see decision 1.

### B. Pure-Rust git client (gitoxide or git2-rs)

Rejected for v0.1. tau is a developer tool (NG11); developers have git on
PATH. Shelling out avoids a complex C dependency (`libgit2` via `git2-rs`) and
keeps binary size small. `gitoxide` is promising but not yet stable across all
needed operations. Reconsider when shipping pre-built binaries to environments
where git may not be present.

### C. Centralized package registry

Rejected permanently (NG4). Git URLs are the canonical source of truth for
tau packages. A centralized registry introduces availability, trust, and
governance dependencies that tau explicitly avoids.

### D. Top-level `PkgError` umbrella with all variants

Rejected. Per-operation error enums give callers more precision and allow
`?`-propagation through `#[from]` without pattern-matching over a large
umbrella that mixes unrelated variants. Mirrors the error policy established
in tau-domain and tau-ports (ADR-0003 §3).

### E. Project lockfile under `.tau/tau-lock.toml`

Rejected. Project lockfiles must be committed to enable reproducible installs;
`.tau/` must not be committed (it is machine-local state). Placing the
lockfile at the project root alongside `tau.toml` makes the commit vs.
non-commit distinction visible and does not require complex `.gitignore`
exceptions.

### F. `tau init` modifies `.gitignore` automatically

Rejected. Automatically mutating a user-owned configuration file is too
magical and surprising. `tau-cli`'s `tau init` will print a hint; the user
decides whether and how to update their `.gitignore`. Pure information, no
mutation.

### G. No advisory file lock — rely on users not running concurrent installs

Rejected. CI pipelines routinely run multiple install scripts in parallel; a
TUI tool may run an install concurrently with a background script. The `fs4`
file lock costs nothing in the non-contended path and prevents silent
corruption. Relying on user discipline to avoid concurrency is a footgun.

### H. `protocol.file.allow=user` (git 2.38+ default) for `file://` clones

Initially considered. Rejected because CI runners with stock git 2.38+ config
would produce silently empty clones from `file://` test fixtures — `git clone`
exits 0 but writes nothing, since the protocol layer blocks the transfer.
The override to `always` is safe because tau-pkg does not use
`--recurse-submodules` (the CVE-2022-39253 attack vector). See decision 14.
