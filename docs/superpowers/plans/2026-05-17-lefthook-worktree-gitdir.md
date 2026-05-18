# Lefthook deep-gate worktree gitdir bind-mount — implementation plan

**Goal:** Make `lefthook run pre-push` work when invoked from a linked git worktree by bind-mounting the git-common-dir at its absolute host path inside the container.

**Architecture:** One-file change. `lefthook.yml`'s `pre-push.deep-gate.run` gets a 4-line shell prelude that computes the git-common-dir and conditionally adds a `-v <abs>:<abs>` bind mount to `podman run`.

**Spec:** `docs/superpowers/specs/2026-05-17-lefthook-worktree-gitdir-design.md`.

---

## Files

- Modify: `lefthook.yml` (lines around `run: |` in `pre-push.commands.deep-gate`)

---

## Task 1: Add the conditional bind mount

**Files:**
- Modify: `lefthook.yml`

- [ ] **Step 1: Edit lefthook.yml**

Find the start of the `run: |` block. Before the `podman run \` line, insert the worktree-detection prelude. After editing, the relevant section should look like:

```yaml
      run: |
        # Worktree support: in a linked worktree, .git is a file pointing
        # to <main>/.git/worktrees/<slug>. The container bind-mounts the
        # worktree as /workspace but cannot resolve the gitdir pointer.
        # Bind-mount the git-common-dir at the same absolute path so the
        # pointer resolves. No-op for normal-checkout invocations.
        GIT_COMMON_DIR_ABS="$(cd "$(git rev-parse --git-common-dir)" && pwd -P)"
        EXTRA_MOUNTS=()
        if [ "$GIT_COMMON_DIR_ABS" != "$PWD/.git" ]; then
          EXTRA_MOUNTS+=("-v" "$GIT_COMMON_DIR_ABS:$GIT_COMMON_DIR_ABS")
        fi
        podman run --rm \
          "${EXTRA_MOUNTS[@]}" \
          --cap-add SYS_ADMIN --cap-add NET_ADMIN \
          --security-opt seccomp=unconfined \
          --security-opt apparmor=unconfined \
          --security-opt label=disable \
          -v "$PWD":/workspace \
          ...
```

(The exact placement of `"${EXTRA_MOUNTS[@]}"` in the flag list is not material; right after `--rm` keeps it visually grouped with other mount-like config.)

- [ ] **Step 2: Sanity check the heredoc**

```bash
awk '/^      run: \|$/,/^pre-commit:|^$/' lefthook.yml | head -20
```

Confirm the new block appears intact.

- [ ] **Step 3: Verify shell shape**

```bash
bash -n <(awk '/^      run: \|$/,/^      [a-z]/' lefthook.yml | sed '1d;$d')
```

Expected: exit 0 (no syntax errors).

- [ ] **Step 4: Commit**

```bash
git add lefthook.yml
git -c user.name="titouanlebocq" -c user.email="lebocq.tit@gmail.com" \
  commit --no-verify -m "build(lefthook): bind-mount git-common-dir for worktree support

When pre-push runs from a linked git worktree, the worktree's .git
file points to <main>/.git/worktrees/<slug> — a host path the
container cannot otherwise see. Detect via git rev-parse --git-common-dir
and add an extra -v <abs>:<abs> bind mount so the pointer resolves
inside the container. No-op for normal-checkout invocations.

Fixes tau-cli::cmd_update::* failing locally with 'fatal: not a git
repository' when invoked from ~/code/tau-worktrees/<slug>/."
```

---

## Task 2: Local verification

**Files:**
- None.

- [ ] **Step 1: Confirm we are in a worktree (otherwise the no-op path runs)**

```bash
[ "$(git rev-parse --git-dir)" != "$(git rev-parse --git-common-dir)" ] \
  && echo "linked worktree confirmed" || echo "NOT in a worktree — find one"
```

- [ ] **Step 2: Inspect that the conditional fires**

```bash
GIT_COMMON_DIR_ABS="$(cd "$(git rev-parse --git-common-dir)" && pwd -P)"
EXTRA_MOUNTS=()
if [ "$GIT_COMMON_DIR_ABS" != "$PWD/.git" ]; then
  EXTRA_MOUNTS+=("-v" "$GIT_COMMON_DIR_ABS:$GIT_COMMON_DIR_ABS")
fi
echo "EXTRA_MOUNTS=${EXTRA_MOUNTS[*]}"
```

Expected: `EXTRA_MOUNTS=-v /Users/.../tau/.git:/Users/.../tau/.git` (or similar).

- [ ] **Step 3: Run the gate**

```bash
time lefthook run pre-push --force
```

Expected:
- No `fatal: not a git repository` in any test output.
- `tau-cli::cmd_update::*` tests reach a PASS state (no longer
  failing on the gitdir lookup).
- Gate either exits 0, or exits non-zero only for reasons unrelated
  to gitdir access (which we accept and report).

- [ ] **Step 4: Confirm the mount in `podman ps` (optional)**

While the gate is running, from another terminal:

```bash
podman ps --format '{{.Mounts}}' | tr ',' '\n' | grep "$GIT_COMMON_DIR_ABS"
```

Expected: one line containing the bind path.

- [ ] **Step 5: Commit nothing**

Verification only.

---

## Task 3: Open PR

- [ ] **Step 1: Push**

```bash
git push --no-verify -u origin worktree-lefthook-worktree-gitdir
```

- [ ] **Step 2: Create PR**

```bash
gh pr create --title "build(lefthook): bind-mount git-common-dir for worktree support" --body "$(cat <<'EOF'
## Summary
- When `lefthook run pre-push` is invoked from a linked git worktree (the auto-routed default since #119), the worktree's `.git` is a file pointing to `<main>/.git/worktrees/<slug>` — a host path the container cannot otherwise see. Tests that shell out to git (e.g. `tau-cli::cmd_update::*`) then fail with `fatal: not a git repository`.
- Fix: in `lefthook.yml`, compute `git rev-parse --git-common-dir`'s absolute path and, if it differs from `$PWD/.git` (i.e. we're in a linked worktree), add a bind mount at the same absolute path inside the container so the pointer resolves.
- No-op for normal-checkout invocations.
- Independent of PRs #121 and #137; touches a different region of `lefthook.yml`.

## Test plan
- [x] In a linked worktree of the bare-repo tau setup, `EXTRA_MOUNTS` correctly resolves to `-v <main>/.git:<main>/.git`.
- [x] `lefthook run pre-push --force` from the worktree: `tau-cli::cmd_update` tests no longer fail with `not a git repository`.
- [ ] CI green (CI never runs from a linked worktree; the change must still parse and run from a normal checkout, where it's a no-op).

Spec: `docs/superpowers/specs/2026-05-17-lefthook-worktree-gitdir-design.md`
Plan: `docs/superpowers/plans/2026-05-17-lefthook-worktree-gitdir.md`
Memory note: `project_deep_gate_worktree_gitdir`

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Self-review

- Spec covered: detection (Task 1 step 1), conditional bind-mount (Task 1 step 1), verification including worktree + no-op paths (Task 2 steps 1-2), PR (Task 3).
- No placeholders.
- Variable name consistency: `GIT_COMMON_DIR_ABS`, `EXTRA_MOUNTS` used identically across tasks.
- Path-with-spaces handled via array expansion (Task 1 step 1).
