# .claude/hooks/

Hook scripts wired via `.claude/settings.json`. Each script must:
- Exit 0 on every code path (hook failure must NOT block the session).
- Run in < 200 ms on a warm cache.
- Use only `bash`, `jq`, `git`, `pgrep` (no extra deps).

## SessionStart: `detect-session-overlap.sh`

Scans Claude Code's session-state directory for other sessions whose
`cwd` matches ours, then emits a "SESSION OVERLAP DETECTION" block
into session context.

Session-state directory (verified 2026-05-17, Claude Code v2.1.143):

    ~/.claude/sessions/<pid>.json

Each file is a single JSON object:

    {
      "pid": 4949,
      "sessionId": "20a13636-f20b-4f69-b925-61de7ac88b4a",
      "cwd": "/Users/titouanlebocq/code/tau",
      "startedAt": 1779016458686,
      "procStart": "Sun May 17 11:14:15 2026",
      "version": "2.1.143",
      "peerProtocol": 1,
      "kind": "interactive",
      "entrypoint": "cli",
      "status": "busy",
      "updatedAt": 1779017220640
    }

A session is "live" when its `pid` is still alive (`kill -0 $pid`). If
the format changes in a future Claude Code release the detector must
degrade silently to "no overlap detected" rather than producing false
positives.

## WorktreeCreate: `worktree-create.sh`

Replaces Claude Code's default `git worktree` logic. Reads
`{"name": "<value>"}` from stdin. Creates the worktree at
`$HOME/code/tau-worktrees/<name>` (override with `TAU_WORKTREE_DIR`)
on branch `worktree-<name>`. Prints the absolute path on the final
stdout line so Claude Code adopts it as the session's cwd.
