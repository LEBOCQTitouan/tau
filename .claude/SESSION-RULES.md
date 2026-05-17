# SESSION RULES — read at every session start

The SessionStart hook (`.claude/hooks/detect-session-overlap.sh`) emits a
"SESSION OVERLAP DETECTION" block into the session context. Read it.
The rules below apply.

## Rule 1: If `overlap_action: route_to_worktree`, move to a worktree

Invoke the `superpowers:using-git-worktrees` skill. It will use the
native `EnterWorktree` tool. The `WorktreeCreate` hook redirects the
target to `~/code/tau-worktrees/<name>` automatically — do NOT pass an
explicit absolute path.

Slug selection (in priority order):
1. If the user has stated a task ("fix flaky test X", "add feature Y"),
   derive a short kebab-case slug from it (e.g. `flaky-test-x`,
   `feature-y`). Max 32 chars.
2. Otherwise fall back to the `suggested_slug` field from the
   detection block.

After `EnterWorktree` succeeds, tell the user one short line:
"Detected concurrent session in main checkout. Moved to <path>."

## Rule 2: If `in_linked_worktree: yes`, do nothing

You are already isolated. Proceed normally.

## Rule 3: If `cargo_processes_active > 0`, coordinate cargo

Another process is mid-build on a shared target dir. Either wait until
it completes (poll via `pgrep -af cargo | grep -v grep`) or pick a
different `CARGO_TARGET_DIR` per `CLAUDE.md` Rule 1. Never assume
contention is harmless — it adds 2–4 minutes per build.

## Rule 4: If `git_index_locked: yes`, abort writes until resolved

Another process is mid-commit. Do NOT run `git add` / `git commit` /
`git rebase` until the lock clears.

## Rule 5: Override the worktree path with `TAU_WORKTREE_DIR`

If a contributor's home layout differs from `~/code/tau-worktrees/`,
they set `TAU_WORKTREE_DIR` in their shell rc. The hook reads it
automatically. Do not hardcode the path in Bash commands you generate.
