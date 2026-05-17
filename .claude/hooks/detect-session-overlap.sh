#!/usr/bin/env bash
# Claude Code SessionStart hook for tau.
# Detects other live Claude sessions sharing this cwd and other build
# contention signals, then emits a SESSION OVERLAP DETECTION block
# into session context.
#
# MUST exit 0 on every path. A failed detector must never block the
# session.
set -uo pipefail

# Bail out cleanly if we are not inside a git repo (no shared-checkout risk).
if ! git_top="$(git rev-parse --show-toplevel 2>/dev/null)"; then
  exit 0
fi

cwd="$(pwd -P)"
# Filter our own session by Claude Code's sessionId env var. The hook script
# runs in a subshell whose $$ differs from the Claude Code process pid, so
# pid-based "skip myself" filtering is unreliable. CLAUDE_CODE_SESSION_ID
# is exported by Claude Code into every hook subprocess.
self_session_id="${CLAUDE_CODE_SESSION_ID:-}"

# Detect "am I already in a linked worktree?" with submodule guard.
git_dir="$(cd "$(git rev-parse --git-dir)" 2>/dev/null && pwd -P || echo "")"
git_common="$(cd "$(git rev-parse --git-common-dir)" 2>/dev/null && pwd -P || echo "")"
in_submodule="$(git rev-parse --show-superproject-working-tree 2>/dev/null || echo "")"
if [ -n "$git_dir" ] && [ -n "$git_common" ] && [ "$git_dir" != "$git_common" ] && [ -z "$in_submodule" ]; then
  in_linked_worktree="yes"
else
  in_linked_worktree="no"
fi

# Count other live Claude sessions whose cwd matches ours.
# Live = pid is alive AND it's not us.
sessions_dir="$HOME/.claude/sessions"
other_sessions=0
if [ -d "$sessions_dir" ] && command -v jq >/dev/null 2>&1; then
  shopt -s nullglob
  for f in "$sessions_dir"/*.json; do
    pid="$(jq -r '.pid // empty' "$f" 2>/dev/null)"
    scwd="$(jq -r '.cwd // empty' "$f" 2>/dev/null)"
    sid="$(jq -r '.sessionId // empty' "$f" 2>/dev/null)"
    [ -z "$pid" ] && continue
    [ "$scwd" != "$cwd" ] && continue
    # Skip ourselves — match by sessionId, not pid (hook runs in a subshell).
    [ -n "$self_session_id" ] && [ "$sid" = "$self_session_id" ] && continue
    # Is this pid still alive?
    if kill -0 "$pid" 2>/dev/null; then
      other_sessions=$((other_sessions + 1))
    fi
  done
fi

# Count active cargo processes.
cargo_active="$(pgrep -af 'cargo (build|test|check|clippy|nextest)' 2>/dev/null | grep -v grep | wc -l | tr -d ' ')"
cargo_active="${cargo_active:-0}"

# Git index lock.
if [ -f "$git_top/.git/index.lock" ]; then
  git_locked="yes"
else
  git_locked="no"
fi

branch="$(git -C "$git_top" branch --show-current 2>/dev/null || echo "")"

# Decide action.
if [ "$in_linked_worktree" = "yes" ]; then
  action="none (already isolated)"
elif [ "$other_sessions" -gt 0 ]; then
  action="route_to_worktree"
else
  action="none"
fi

# Suggested slug — random fallback. Claude SHOULD override with a task-derived slug.
suggested_slug="auto-$(LC_ALL=C tr -dc 'a-z0-9' </dev/urandom | head -c 6)"

cat <<EOF
=== SESSION OVERLAP DETECTION ===
cwd: $cwd
git_toplevel: $git_top
branch: $branch
in_linked_worktree: $in_linked_worktree
other_sessions_same_cwd: $other_sessions
cargo_processes_active: $cargo_active
git_index_locked: $git_locked
overlap_action: $action
suggested_slug: $suggested_slug
=================================
EOF

# Append a one-line directive when action is route_to_worktree, so Claude
# reads the directive immediately even before reaching SESSION-RULES.md.
if [ "$action" = "route_to_worktree" ]; then
  cat <<'EOF'

>>> DIRECTIVE: Another Claude session shares this cwd. Per .claude/SESSION-RULES.md
>>> Rule 1, invoke the superpowers:using-git-worktrees skill (which will use the
>>> native EnterWorktree tool). Use a task-derived kebab-case slug, or fall back
>>> to the suggested_slug above.
EOF
fi

exit 0
