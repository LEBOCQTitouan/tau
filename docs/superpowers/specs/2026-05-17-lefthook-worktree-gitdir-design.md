# Lefthook deep-gate worktree gitdir bind-mount — design

**Status:** Draft
**Date:** 2026-05-17

## Context

PR #119 made foreground Claude Code sessions auto-route to linked
worktrees (`~/code/tau-worktrees/<slug>/`). The deep-gate currently
breaks in that setup because:

- Container does `-v "$PWD":/workspace` — sees the worktree's working
  copy.
- The worktree's `.git` is a *file* (not a directory) containing:
  `gitdir: /Users/titouanlebocq/code/tau/.git/worktrees/<slug>`
- That darwin host path doesn't exist inside the container, so any
  `git` invocation inside fails with `fatal: not a git repository`.

Symptom: `tau-cli::cmd_update::*` tests in test-stable fail because
`tau update` shells out to `git ls-remote`. Other tests that exec git
likely fail too; cmd_update is just the first hit. CI does not see
this because `actions/checkout` produces a normal clone, never a
linked worktree.

Documented in memory entry `project_deep_gate_worktree_gitdir`.

## Goals

1. Make `lefthook run pre-push` work correctly when invoked from a
   linked worktree.
2. No-op behavior when invoked from a normal checkout or the main repo.
3. Surface the configuration via standard git plumbing
   (`git rev-parse --git-common-dir`) so this works for any
   worktree layout (bare-repo + linked worktrees as in tau, or a
   regular checkout with `git worktree add`).
4. Independent of PRs #121 and #137 — different region of `lefthook.yml`.

## Non-Goals

- Fixing the "lefthook tests can corrupt git identity" footgun
  documented in CLAUDE.md (tests writing `Test User <test@example.com>`
  to the worktree's git config). That's a test-suite issue, not a
  gate-config issue, and is orthogonal.
- Sandboxing the gitdir read-only. RW matches what a normal-checkout
  contributor gets and avoids regressing any test that writes config.

## Design

### Detection

```bash
GIT_COMMON_DIR_ABS="$(cd "$(git rev-parse --git-common-dir)" && pwd -P)"
```

`git rev-parse --git-common-dir` returns:
- normal repo: `.git` (relative; resolves to `$PWD/.git`)
- linked worktree of a normal repo: `<main>/.git` (absolute)
- linked worktree of a bare repo (tau today): `<bare-repo>/.git` (absolute)

In all cases `pwd -P` produces a canonical host-absolute path.

### Conditional bind mount

```bash
EXTRA_MOUNTS=()
if [ "$GIT_COMMON_DIR_ABS" != "$PWD/.git" ]; then
  # Linked worktree: the worktree's .git file references the
  # git-common-dir. Bind-mount the common dir at the same absolute
  # path inside the container so the reference resolves.
  EXTRA_MOUNTS+=("-v" "$GIT_COMMON_DIR_ABS:$GIT_COMMON_DIR_ABS")
fi
```

The bind mount uses the **same absolute path inside the container** as
on the host, so the gitdir pointer (which is a host-absolute path)
resolves without any rewriting. This is the only place that path
appears inside the container; nothing else cares.

### Wire into podman run

`"${EXTRA_MOUNTS[@]}"` expands to either zero or two args. Pass it to
`podman run` like any other flag.

### Why not read-only

Bind mounting `ro` would catch the "tests corrupt git identity"
footgun by surfacing it as a permission error, but it would also
break legitimate write paths (e.g. tests that legitimately stage
content in the workspace, or hypothetical tests that `git config`
into a temp dir but then end up writing through the real gitdir
via misconfiguration). RW preserves whatever today's tests expect
from a normal-checkout run, which is the same thing CI sees.

## Test plan

1. **Worktree run smoke.** From the new worktree (this branch), run
   `lefthook run pre-push --force`. Expect:
   - Stage 0/1/2/3/4 complete (or whatever subset is in scope on
     current `main`; with #121 and #137 still un-merged this branch
     uses main's lefthook.yml as base, modulo this one delta).
   - `tau-cli::cmd_update::*` tests pass at test-stable (previously
     they failed with `not a git repository`).
   - Gate eventually exits 0, OR exits non-zero only on unrelated
     causes (e.g. macOS-style flakes that don't apply here since we
     run on Linux).

2. **Non-worktree no-op.** From a normal-checkout repo, verify the
   `EXTRA_MOUNTS` array stays empty. (Inspectable via `bash -x`.)

3. **Bind-mount path inspection.** During the worktree run, inspect
   `podman ps` from another shell to confirm the extra `-v` flag
   appears in the container's args.

4. **CI parity.** Push, confirm CI green. CI is unaffected — it does
   not use linked worktrees — but the change should not break parsing
   or shape of the yaml.

## Risks + mitigations

1. **Path with spaces / unusual chars in `GIT_COMMON_DIR_ABS`.**
   `pwd -P` doesn't escape spaces. The bind-mount arg is properly
   quoted via array expansion, so spaces survive into podman. Tested
   conceptually only — there are no spaces in the canonical tau setup.

2. **SELinux contexts (Linux hosts).** macOS Podman runs in a VM with
   `--security-opt label=disable` already set; the bind mount won't
   be relabeled. On native Linux hosts (CI never does this, but a
   user might), SELinux could refuse the mount; we already set
   `label=disable` so this is unaffected.

3. **Same-path collision.** If `GIT_COMMON_DIR_ABS` overlaps with an
   existing bind mount (e.g. `/workspace`), the inner path would be
   shadowed. In practice the common-dir lives under
   `/Users/.../tau/.git`, never under `/workspace` or `/usr/local/...`,
   so no collision.

## Follow-ups (not in this spec)

- The "tests corrupt git identity" footgun in CLAUDE.md — separately
  worth fixing by making the offending lefthook integration tests
  always shell out into a temp dir.
- Once PRs #121 and #137 land, this fix composes with them
  unconditionally; no further work.
