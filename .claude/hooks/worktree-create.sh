#!/usr/bin/env bash
# Claude Code WorktreeCreate hook.
# Reads {"name": "<value>"} JSON on stdin.
# Creates a worktree at $TAU_WORKTREE_DIR/<name> (default
# $HOME/code/tau-worktrees/<name>) on branch worktree-<name>.
# Prints the absolute target path on the final stdout line.
# Exits non-zero on failure so Claude Code surfaces the error.
set -euo pipefail

input="$(cat)"
name="$(printf '%s' "$input" | jq -r '.name // empty')"
if [ -z "$name" ]; then
  echo "worktree-create.sh: missing .name in stdin JSON" >&2
  exit 64
fi

# Validate name: kebab-case-ish, no path separators, no leading dot.
case "$name" in
  ""|.*|*/*|*..*)
    echo "worktree-create.sh: invalid worktree name: $name" >&2
    exit 64
    ;;
esac

worktree_root="${TAU_WORKTREE_DIR:-$HOME/code/tau-worktrees}"
target="$worktree_root/$name"
branch="worktree-$name"

repo_root="$(git rev-parse --show-toplevel)"

# Honor worktree.baseRef from settings: "fresh" -> origin/HEAD, "head" -> HEAD.
base_ref_setting="$(git -C "$repo_root" config --get worktree.baseRef 2>/dev/null || echo fresh)"
case "$base_ref_setting" in
  head) base_ref="HEAD" ;;
  *)
    # Fall back to local HEAD if no remote is configured or fetch is undesirable here.
    if git -C "$repo_root" rev-parse --verify --quiet origin/HEAD >/dev/null; then
      base_ref="origin/HEAD"
    else
      base_ref="HEAD"
    fi
    ;;
esac

mkdir -p "$worktree_root"

# Idempotent: if the worktree already exists at $target, just print its path.
if git -C "$repo_root" worktree list --porcelain | awk '/^worktree /{print $2}' | grep -Fxq "$target"; then
  echo "$target"
  exit 0
fi

# Create the worktree. If the branch already exists, attach to it instead of -b.
if git -C "$repo_root" show-ref --verify --quiet "refs/heads/$branch"; then
  git -C "$repo_root" worktree add "$target" "$branch" >&2
else
  git -C "$repo_root" worktree add -b "$branch" "$target" "$base_ref" >&2
fi

# Final stdout line MUST be the absolute target path (per Claude Code hook contract).
echo "$target"
