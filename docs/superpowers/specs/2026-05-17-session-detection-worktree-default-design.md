# Session-overlap detection + worktree-default — design

**Date:** 2026-05-17
**Status:** Draft (pending user review)
**Scope:** `tau/.claude/` (project-local, does not affect global `~/.claude/`)

## Problem

`tau/` is frequently worked from multiple concurrent Claude Code sessions and `~/code/tau-worktrees/` already holds 22 active worktrees. The workflow is real but informal: nothing detects concurrent sessions, nothing automatically routes the second-arriving session to a worktree, and worktree path conventions drift (19 under `~/code/tau-worktrees/`, 3 under `tau/.claude/worktrees/`).

Two specific gaps:

1. **No session-overlap detection.** Two interactive sessions in the same checkout can step on each other's edits and git state. There is no built-in warning.
2. **Foreground sessions do not auto-isolate.** Per `code.claude.com/docs/en/worktrees`, only *background* sessions are automatically moved into an isolated worktree on first write. Foreground sessions stay in the shared checkout unless the user passes `--worktree` or invokes `EnterWorktree`.

## Non-goals

- Replacing Claude Code's native worktree machinery. The spec uses official extension points (`WorktreeCreate` hook, `EnterWorktree` tool, `.worktreeinclude`) — it does not build parallel infrastructure.
- Cross-project enforcement. Lives entirely under `tau/.claude/`. Global `~/.claude/` is untouched.
- Auto-routing when no overlap is detected. A single session in `tau/` works in the main checkout as it always has.
- Worktree cleanup. The native `cleanupPeriodDays` setting and existing `commit-commands:clean_gone` already handle this.

## What's native to Claude Code (do NOT reimplement)

Confirmed via official docs (`code.claude.com/docs/en/worktrees`, May 2026):

| Capability | Mechanism |
|---|---|
| Worktree creation | `claude --worktree <name>` flag, or `EnterWorktree` tool mid-session |
| Custom worktree path / strategy | `WorktreeCreate` hook (replaces default `git worktree` logic entirely) |
| Base branch selection | `worktree.baseRef` setting: `"fresh"` (default — `origin/HEAD`) or `"head"` |
| Copy gitignored files | `.worktreeinclude` at repo root, gitignore syntax |
| Subagent isolation | `isolation: worktree` in agent frontmatter; `Agent` tool's `isolation: "worktree"` parameter |
| Auto-cleanup | `cleanupPeriodDays` setting (sweep on session start) |
| Background-session write-isolation | Automatic — first write moves the session into a fresh worktree |

The `superpowers:using-git-worktrees` skill already wraps Step 0 (detect existing isolation) and Step 1 (prefer native tool). Once the policy below tells Claude to invoke that skill, the skill does the right thing.

## Genuine gaps the spec closes

1. **Same-cwd session detection.** Nothing native counts other Claude sessions pointed at this repo. A `SessionStart` hook fills the gap.
2. **Path redirect.** Native default is `.claude/worktrees/<name>/` — tau's dominant convention is `~/code/tau-worktrees/<name>`. A `WorktreeCreate` hook redirects.
3. **Auto-routing policy.** Tying the two together so the second session moves to a worktree without manual prompt.

## Architecture

```
tau/
├── .claude/
│   ├── settings.json                       (NEW)
│   ├── settings.local.json                 (unchanged)
│   ├── SESSION-RULES.md                    (NEW)
│   └── hooks/
│       ├── detect-session-overlap.sh       (NEW)
│       └── worktree-create.sh              (NEW)
├── .worktreeinclude                        (NEW)
└── docs/superpowers/specs/
    └── 2026-05-17-session-detection-worktree-default-design.md   (this file)
```

Flow:

```
session starts
   │
   ▼
SessionStart hook → detect-session-overlap.sh
   │
   ├── 0 other sessions in this cwd → emit "no overlap" context, done
   │
   └── ≥1 other session in this cwd → emit "OVERLAP DETECTED" context
                                      ↓
                                Claude reads SESSION-RULES.md
                                      ↓
                                Claude invokes EnterWorktree tool
                                      ↓
                                Claude Code calls WorktreeCreate hook
                                      ↓
                                worktree-create.sh: git worktree add ~/code/tau-worktrees/<slug>
                                      ↓
                                Session continues in the new worktree
```

## Component details

### 1. `detect-session-overlap.sh` (SessionStart hook)

Run once at session start. Reads no stdin. Emits a plain-text block on stdout that lands in Claude's session context.

**Responsibilities:**

- Determine my own session id (from `$CLAUDE_SESSION_ID` env if exposed, else from `$$` + cwd).
- Resolve cwd's canonical path (`realpath`).
- Resolve git toplevel — if not in a git repo, exit 0 with no output.
- Detect "am I already in a linked worktree?" via `[ "$(git rev-parse --git-dir)" != "$(git rev-parse --git-common-dir)" ]` plus the submodule guard from `using-git-worktrees`. If yes → emit a one-liner ("already isolated") and exit. No further action.
- Count other live Claude sessions pointed at the same cwd:
  - Iterate `~/.claude/sessions/*.json` (or whatever the runtime stores). Filter by `cwd == mine && session_id != mine && mtime within last 4h`.
  - Fall back to `pgrep -af claude` filtered by `--cwd`-style arg if session-file format changes.
- Count active builds via `pgrep -af 'cargo (build|test|check|clippy)' | grep -v grep | wc -l`.
- Check `.git/index.lock` and any `.git/worktrees/*/index.lock`.

**Output schema (stable, machine-readable so policy can grep it):**

```
=== SESSION OVERLAP DETECTION ===
cwd: /Users/titouanlebocq/code/tau
git_toplevel: /Users/titouanlebocq/code/tau
in_linked_worktree: no
branch: main
other_sessions_same_cwd: 2
cargo_processes_active: 0
git_index_locked: no
overlap_action: route_to_worktree | none
suggested_slug: auto-<short-random>
=================================
```

The `suggested_slug` is a fallback only — the detector cannot know the user's task. Format: `auto-` + 6 random alphanumeric chars (e.g. `auto-x7k2qp`). Claude SHOULD override this with a task-derived kebab-case slug when invoking `EnterWorktree`; the fallback exists so a worktree can be created even if the model has no task hint yet.


**Hard requirements:**

- Exit 0 on all paths (a failed detector must never block the session).
- Total runtime < 200 ms on a warm cache.
- No network calls.
- POSIX `bash` + `git` + `pgrep` only. No deps beyond what `lefthook.yml` already assumes.

**Non-requirements:**

- Detecting sessions from other VCS systems. tau is git-only; revisit if that changes.
- Cross-machine detection. Sessions on remotes are out of scope.

### 2. `worktree-create.sh` (WorktreeCreate hook)

Replaces Claude Code's default worktree-creation logic. Per the docs:

> Configure a WorktreeCreate hook, which replaces the default git worktree logic entirely.

**Input:** JSON on stdin with at least `{ "name": "<value>" }` (the `--worktree <value>` argument or the slug derived by `EnterWorktree`).

**Behavior:**

- Read `name` via `jq -r .name`.
- Compute target: `$HOME/code/tau-worktrees/${name}`.
- If the target already exists and is a valid worktree, print the path and exit 0 (idempotent).
- Otherwise `git -C "$(git rev-parse --show-toplevel)" worktree add -b "worktree-${name}" "$target" "$(base_ref)"` where `base_ref` honors the `worktree.baseRef` setting (`origin/HEAD` for `fresh`, `HEAD` for `head`).
- Print the absolute target path on the final stdout line (Claude Code reads this as the session's new cwd).

**Failure handling:**

- If `git worktree add` fails (branch collision, permission, etc.), print error to stderr and exit non-zero. Claude Code surfaces the error to the model.

**Pairing:** No matching `WorktreeRemove` hook is needed; tau wants worktrees to survive session exit by default (matching existing usage of `~/code/tau-worktrees/`). Cleanup is manual via `commit-commands:clean_gone` or `git worktree remove`.

### 3. `SESSION-RULES.md`

Short policy file (target ≤ 60 lines). Sections:

```markdown
# SESSION RULES — read at every session start

The SessionStart hook (`.claude/hooks/detect-session-overlap.sh`) emits a
"SESSION OVERLAP DETECTION" block. Read it. The rules below apply.

## Rule 1: If `overlap_action: route_to_worktree`, move to a worktree

Invoke the `superpowers:using-git-worktrees` skill, which will use the
native `EnterWorktree` tool. The `WorktreeCreate` hook redirects the
target to `~/code/tau-worktrees/<name>` automatically — do not pass an
explicit path.

If the detection block includes a `suggested_slug`, pass it as the
worktree name. Otherwise derive a short kebab-case slug from the user's
current task.

## Rule 2: When already in a linked worktree, do nothing

If `in_linked_worktree: yes`, you are already isolated. Continue normally.

## Rule 3: When `cargo_processes_active > 0`, queue cargo commands

Another session is mid-build on a shared target dir. Either wait or pick
a different `CARGO_TARGET_DIR` per CLAUDE.md Rule 1.

## Rule 4: If `git_index_locked: yes`, abort writes until resolved

Another process is mid-commit. Do not run `git add` / `git commit` /
`git rebase` until the lock clears.
```

The file is intentionally short so it can be reread cheaply. All deeper worktree mechanics are deferred to the `superpowers:using-git-worktrees` skill.

### 4. `.claude/settings.json`

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "*",
        "hooks": [
          { "type": "command", "command": ".claude/hooks/detect-session-overlap.sh" }
        ]
      }
    ],
    "WorktreeCreate": [
      {
        "hooks": [
          { "type": "command", "command": ".claude/hooks/worktree-create.sh" }
        ]
      }
    ]
  },
  "worktree": {
    "baseRef": "fresh"
  }
}
```

Note: this is `.claude/settings.json` (project, checked in), not `.claude/settings.local.json` (per-user, gitignored). The hooks should apply to every contributor.

### 5. `.worktreeinclude`

Copies env-style files into each new worktree so the dev loop works immediately:

```
.env
.env.local
.cargo/config.toml.local
target/main/.rustc_info.json
```

(Final list determined during implementation — pick whatever tau actually depends on locally.)

## Testing strategy

| Scenario | Verification |
|---|---|
| Single session, no overlap | Open one Claude session in `tau/`. Hook emits `other_sessions_same_cwd: 0`, `overlap_action: none`. No worktree created. |
| Two foreground sessions | Open session A in `tau/`, then session B in `tau/`. B's hook reports `other_sessions_same_cwd: 1`, `overlap_action: route_to_worktree`. B invokes `EnterWorktree`. Verify session B's cwd is `~/code/tau-worktrees/<slug>`. |
| Session inside existing worktree | Open session in `~/code/tau-worktrees/feat-existing`. Hook reports `in_linked_worktree: yes`. No new worktree. |
| `WorktreeCreate` idempotency | Call hook with a name that already maps to an existing worktree. Should print the existing path and exit 0. |
| Hook failure path | `chmod -x detect-session-overlap.sh`. Session should still start (hook failure must not block). |
| `worktree.baseRef: "head"` | Set baseRef to `head` in a local override. Create a worktree from a feature branch with unpushed commits. Verify the worktree carries those commits. |
| `.worktreeinclude` propagation | Place a `.env` in main checkout matching the include. Create new worktree. Verify `.env` is present. |

Manual at first. Could be promoted to a shell-based integration test under `crates/landlock-exec-repro/` style if it becomes a regression target.

## Risks & tradeoffs

- **Detection accuracy.** Reading `~/.claude/sessions/*.json` ties us to an undocumented file format. If Anthropic changes it, the detector emits 0 sessions and falls back to "no overlap" — silently degrading rather than producing false positives. Acceptable for now; revisit if the false-negative rate becomes noticeable.
- **First-arriving session keeps the main checkout.** The detector only fires for sessions that arrive *after* a session is already established. If two sessions launch within the same second, both may report 0 overlap. Race window is bounded by the time between session-file write and the next session's hook read — empirically sub-second. The cost of the race is "two sessions in main for a few seconds until one decides to route" — survivable.
- **WorktreeCreate hook becomes load-bearing.** A bug here breaks `claude --worktree`, `EnterWorktree`, and subagent isolation simultaneously. Mitigated by: keeping the script small, exiting 0 + printing the default `.claude/worktrees/<name>` path as a fallback if the redirect fails.
- **Drift from `~/code/tau-worktrees/` convention.** If a contributor's home dir layout differs (e.g., they keep code under `~/dev/`), the hardcoded `~/code/tau-worktrees/` breaks. Mitigation: the hook reads `TAU_WORKTREE_DIR` env override before falling back to `~/code/tau-worktrees/`. Documented in `SESSION-RULES.md`.

## Open implementation questions

- Exact path of Claude Code's session-state files in v2.1.50+. The detector script should be developed against a probe (`ls ~/.claude/sessions/ ~/.claude/projects/ -la`) and the format documented inline.
- Whether the `worktree-create.sh` hook should call `cargo build` once to warm the per-worktree target dir, or leave that to the `using-git-worktrees` skill's Step 3. Default: leave it to the skill — keeps hook fast and avoids surprising contributors who don't want a full build on every worktree creation.
- Whether `SESSION-RULES.md` should be loaded automatically (via CLAUDE.md import) or rely on the `SessionStart` hook output mentioning it by name. Probably the latter: import keeps it in context every turn, hook mention pays the cost only on overlap.

## Out of scope (deferred)

- A status-line indicator showing "N other sessions active." Could be a follow-up using `statusline-setup`.
- A periodic re-check during a session (in case another session starts after this one). `SessionStart` is one-shot. Native background-session write-isolation covers the most common version of this.
- Cross-project generalization. If this proves out, lift to `~/.claude/` as a global default in a follow-up spec.
