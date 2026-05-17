# Session-Overlap Detection + Worktree-Default Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `tau/.claude/` automatically detect concurrent Claude sessions sharing this cwd and route the second-arriving session into an isolated worktree under `~/code/tau-worktrees/<name>`, using Claude Code's native `EnterWorktree` + `WorktreeCreate` hook.

**Architecture:** Two shell hooks (`SessionStart` + `WorktreeCreate`) wired through `.claude/settings.json`, plus a short `SESSION-RULES.md` policy file Claude reads when the SessionStart hook reports overlap. The `WorktreeCreate` hook replaces Claude Code's default `git worktree` logic to redirect output to `~/code/tau-worktrees/`. No custom worktree-creation script is needed — `EnterWorktree` is the API. Spec: `docs/superpowers/specs/2026-05-17-session-detection-worktree-default-design.md`.

**Tech Stack:** bash 3.2+ (macOS default), `jq`, `git`, `pgrep`. No new deps. No Rust changes.

---

## File Structure

| Path | Status | Responsibility |
|---|---|---|
| `.claude/hooks/detect-session-overlap.sh` | NEW | SessionStart hook. Scans `~/.claude/sessions/*.json` for other sessions with matching cwd; emits a fenced context block. |
| `.claude/hooks/worktree-create.sh` | NEW | WorktreeCreate hook. Reads `{name}` JSON on stdin, runs `git worktree add` at `~/code/tau-worktrees/<name>`, prints absolute path on final stdout line. |
| `.claude/SESSION-RULES.md` | NEW | Short policy file Claude reads when SessionStart reports overlap. |
| `.claude/settings.json` | NEW | Wires SessionStart + WorktreeCreate hooks; sets `worktree.baseRef`. |
| `.worktreeinclude` | NEW | Lists gitignored files to copy into new worktrees (`.env`, etc). |
| `.gitignore` | MODIFY | Ensure `.claude/worktrees/` is ignored (defensive — Claude Code's docs recommend it even though we redirect away from there). |

Each hook is < 80 lines and lives in its own file so it can be tested independently. SESSION-RULES.md is < 70 lines so re-reading is cheap.

---

## Task 1: Probe and document session-file format

This task locks down our assumption about Claude Code's session-state shape so later tasks can rely on it.

**Files:**
- Create: `.claude/hooks/README.md`

- [ ] **Step 1: Inspect the live session-file format**

Run:
```bash
ls ~/.claude/sessions/*.json | head -3
cat "$(ls -t ~/.claude/sessions/*.json | head -1)"
```

Expected: each file contains a single JSON object with at least these keys: `pid`, `sessionId`, `cwd`, `startedAt`, `kind`, `entrypoint`, `status`, `updatedAt`, `version`.

- [ ] **Step 2: Write `.claude/hooks/README.md` documenting the assumption**

```markdown
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
```

- [ ] **Step 3: Commit**

```bash
git add .claude/hooks/README.md
git commit -m "docs(hooks): document Claude Code session-state file format"
```

---

## Task 2: WorktreeCreate hook — redirect worktree path

Start with this hook because it's smaller and easier to test in isolation, and Task 3 will depend on the path it produces.

**Files:**
- Create: `.claude/hooks/worktree-create.sh`

- [ ] **Step 1: Write the failing probe**

Open a new terminal in `tau/`. Run this probe to demonstrate the file does not exist yet:

```bash
test -x .claude/hooks/worktree-create.sh && echo "EXISTS" || echo "MISSING"
```

Expected: `MISSING`.

- [ ] **Step 2: Write the hook**

Create `.claude/hooks/worktree-create.sh`:

```bash
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
```

Make it executable:

```bash
chmod +x .claude/hooks/worktree-create.sh
```

- [ ] **Step 3: Test the happy path**

Run:
```bash
mkdir -p /tmp/tau-wt-test-input
echo '{"name": "smoke-test-001"}' | .claude/hooks/worktree-create.sh
```

Expected: final stdout line is `/Users/<user>/code/tau-worktrees/smoke-test-001`. Verify:
```bash
git worktree list | grep smoke-test-001
ls -la ~/code/tau-worktrees/smoke-test-001/
```

- [ ] **Step 4: Test idempotency**

Re-run the same command:
```bash
echo '{"name": "smoke-test-001"}' | .claude/hooks/worktree-create.sh
```

Expected: same path printed, no error, no duplicate worktree.

- [ ] **Step 5: Test the override path**

```bash
TAU_WORKTREE_DIR=/tmp/tau-wt-test echo '{"name": "smoke-test-002"}' | TAU_WORKTREE_DIR=/tmp/tau-wt-test .claude/hooks/worktree-create.sh
ls /tmp/tau-wt-test/smoke-test-002/
```

Expected: worktree created under `/tmp/tau-wt-test/`.

- [ ] **Step 6: Test invalid-name rejection**

```bash
echo '{"name": "../escape"}' | .claude/hooks/worktree-create.sh; echo "exit=$?"
echo '{"name": ""}' | .claude/hooks/worktree-create.sh; echo "exit=$?"
echo '{}' | .claude/hooks/worktree-create.sh; echo "exit=$?"
```

Expected: each exits 64 with a message on stderr.

- [ ] **Step 7: Clean up test worktrees**

```bash
git worktree remove ~/code/tau-worktrees/smoke-test-001
git worktree remove /tmp/tau-wt-test/smoke-test-002
rm -rf /tmp/tau-wt-test /tmp/tau-wt-test-input
git branch -D worktree-smoke-test-001 worktree-smoke-test-002 2>/dev/null || true
```

- [ ] **Step 8: Commit**

```bash
git add .claude/hooks/worktree-create.sh
git commit -m "feat(hooks): WorktreeCreate hook redirects to ~/code/tau-worktrees/"
```

---

## Task 3: SessionStart hook — detect same-cwd sessions

**Files:**
- Create: `.claude/hooks/detect-session-overlap.sh`

- [ ] **Step 1: Write the failing probe**

```bash
test -x .claude/hooks/detect-session-overlap.sh && echo "EXISTS" || echo "MISSING"
```

Expected: `MISSING`.

- [ ] **Step 2: Write the hook**

Create `.claude/hooks/detect-session-overlap.sh`:

```bash
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
self_pid="$$"

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
    [ -z "$pid" ] && continue
    [ "$pid" = "$self_pid" ] && continue
    [ "$scwd" != "$cwd" ] && continue
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
```

Make it executable:
```bash
chmod +x .claude/hooks/detect-session-overlap.sh
```

- [ ] **Step 3: Test single-session (no overlap)**

```bash
.claude/hooks/detect-session-overlap.sh
```

Expected output includes:
```
other_sessions_same_cwd: 0
overlap_action: none
```

- [ ] **Step 4: Test overlap detection with a fake live session**

Pick a long-running process pid (e.g. your shell):
```bash
fake_pid=$$
fake_session=~/.claude/sessions/test-overlap-$fake_pid.json
cat > "$fake_session" <<EOF
{"pid":$fake_pid,"sessionId":"test-overlap","cwd":"$(pwd -P)","startedAt":1,"version":"test","kind":"interactive","entrypoint":"cli","status":"busy","updatedAt":1}
EOF
# Run the detector in a subshell with a DIFFERENT $$ so it doesn't filter our own pid.
bash -c '.claude/hooks/detect-session-overlap.sh' | grep -E '(other_sessions_same_cwd|overlap_action|DIRECTIVE)'
```

Expected:
```
other_sessions_same_cwd: 1
overlap_action: route_to_worktree
>>> DIRECTIVE: Another Claude session shares this cwd. ...
```

Cleanup:
```bash
rm "$fake_session"
```

- [ ] **Step 5: Test in a linked worktree (no action)**

```bash
git worktree add /tmp/tau-wt-probe -b worktree-probe-detector HEAD
cd /tmp/tau-wt-probe
"$OLDPWD/.claude/hooks/detect-session-overlap.sh" | grep -E '(in_linked_worktree|overlap_action)'
cd -
git worktree remove /tmp/tau-wt-probe
git branch -D worktree-probe-detector
```

Expected:
```
in_linked_worktree: yes
overlap_action: none (already isolated)
```

- [ ] **Step 6: Test failure resilience**

```bash
chmod -x .claude/hooks/detect-session-overlap.sh
.claude/hooks/detect-session-overlap.sh 2>&1; echo "exit=$?"
chmod +x .claude/hooks/detect-session-overlap.sh
```

Expected: shell refuses to execute (exit 126). This confirms Claude Code sees a non-zero exit on a broken hook; the hook protocol allows the session to proceed regardless. We will verify the end-to-end "session still starts" in Task 7.

- [ ] **Step 7: Commit**

```bash
git add .claude/hooks/detect-session-overlap.sh
git commit -m "feat(hooks): SessionStart detector for concurrent same-cwd sessions"
```

---

## Task 4: SESSION-RULES.md — the policy

**Files:**
- Create: `.claude/SESSION-RULES.md`

- [ ] **Step 1: Write the policy file**

Create `.claude/SESSION-RULES.md`:

```markdown
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
```

- [ ] **Step 2: Sanity-check the line count**

```bash
wc -l .claude/SESSION-RULES.md
```

Expected: < 70 lines (target ≤ 60).

- [ ] **Step 3: Commit**

```bash
git add .claude/SESSION-RULES.md
git commit -m "docs: SESSION-RULES.md policy for overlap auto-routing"
```

---

## Task 5: .worktreeinclude — copy gitignored files into new worktrees

**Files:**
- Create: `.worktreeinclude`
- Modify: `.gitignore`

- [ ] **Step 1: Inspect what tau actually needs in a worktree**

Check what's gitignored at the repo root that the dev loop depends on:
```bash
cd "$(git rev-parse --show-toplevel)"
git check-ignore -v $(ls -A | grep -vE '^(\.git|target)$') 2>/dev/null | head -20
ls -la | grep -E '^\.env|\.cargo'
```

Verify: `.env` files (if any), local `.cargo/config.toml`, etc. Do NOT include `target/` — each worktree gets its own.

- [ ] **Step 2: Write `.worktreeinclude`**

Create `.worktreeinclude` at the repo root. Conservative initial contents:

```text
# Files to copy into new worktrees (gitignore syntax).
# Only gitignored files matching these patterns are copied — tracked
# files are never duplicated.
.env
.env.local
.envrc
.cargo/config.toml.local
```

- [ ] **Step 3: Confirm .gitignore covers `.claude/worktrees/`**

Per Claude Code docs the default worktree path is `.claude/worktrees/<name>` and they advise gitignoring it. We redirect away from there with the WorktreeCreate hook, but a contributor running raw `claude --worktree` without our hook (in a `git stash` situation, for example) could still create one. Defensive entry:

```bash
grep -q '^\.claude/worktrees/' .gitignore || echo '.claude/worktrees/' >> .gitignore
git diff .gitignore
```

If `.gitignore` changed, stage it.

- [ ] **Step 4: Commit**

```bash
git add .worktreeinclude .gitignore
git commit -m "feat: .worktreeinclude + gitignore .claude/worktrees/"
```

---

## Task 6: settings.json — wire the hooks

This is the task that activates everything. Do it last so all referenced files exist.

**Files:**
- Create: `.claude/settings.json`

- [ ] **Step 1: Verify all referenced files exist**

```bash
test -x .claude/hooks/detect-session-overlap.sh && echo "ok: detect"
test -x .claude/hooks/worktree-create.sh && echo "ok: create"
test -f .claude/SESSION-RULES.md && echo "ok: rules"
test -f .worktreeinclude && echo "ok: include"
```

Expected: all four print "ok: …". If any are missing, return to the corresponding task.

- [ ] **Step 2: Write `.claude/settings.json`**

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/detect-session-overlap.sh"
          }
        ]
      }
    ],
    "WorktreeCreate": [
      {
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/worktree-create.sh"
          }
        ]
      }
    ]
  },
  "worktree": {
    "baseRef": "fresh"
  }
}
```

- [ ] **Step 3: Verify it's valid JSON**

```bash
jq . .claude/settings.json > /dev/null && echo "valid JSON"
```

Expected: `valid JSON`.

- [ ] **Step 4: Commit**

```bash
git add .claude/settings.json
git commit -m "feat: wire SessionStart + WorktreeCreate hooks via .claude/settings.json"
```

---

## Task 7: End-to-end smoke test

Now verify the whole thing works with real Claude Code sessions. This is manual and runs outside any automated test suite.

**Files:** none modified.

- [ ] **Step 1: Smoke test — single session, no overlap**

Open a fresh terminal. Run:
```bash
cd ~/code/tau
claude
```

In the Claude session, ask: "Read your SessionStart hook output and report what you see."

Expected: Claude reports `other_sessions_same_cwd: 0`, `overlap_action: none`. No worktree is created. Exit the session with `/exit`.

- [ ] **Step 2: Smoke test — two foreground sessions**

Open terminal A, run `claude` from `~/code/tau`. Ask it to "stay alive but do nothing — wait for instructions."

Open terminal B, run `claude` from `~/code/tau`. Watch the session-start output.

Expected: terminal B's hook reports `other_sessions_same_cwd: 1`, `overlap_action: route_to_worktree`, and includes the `>>> DIRECTIVE:` line. Claude in terminal B then invokes the `superpowers:using-git-worktrees` skill, which calls `EnterWorktree`, which calls our `WorktreeCreate` hook, which creates `~/code/tau-worktrees/<slug>/`. Terminal B's cwd should now be that path. Claude prints "Detected concurrent session in main checkout. Moved to <path>." per Rule 1.

Verify externally:
```bash
git worktree list | grep tau-worktrees
```

- [ ] **Step 3: Smoke test — session inside an existing worktree (no double-route)**

```bash
cd ~/code/tau-worktrees/<the-one-from-step-2>
claude
```

Ask: "Read your SessionStart hook output and report what you see."

Expected: `in_linked_worktree: yes`, `overlap_action: none (already isolated)`. No new worktree.

- [ ] **Step 4: Smoke test — hook failure resilience**

Temporarily break the detector:
```bash
chmod -x .claude/hooks/detect-session-overlap.sh
claude
```

Expected: Claude session still starts. The broken hook is reported but does not block. Exit, restore:
```bash
chmod +x .claude/hooks/detect-session-overlap.sh
```

- [ ] **Step 5: Cleanup**

Remove the smoke-test worktree(s):
```bash
git worktree remove ~/code/tau-worktrees/<slug>
git branch -D worktree-<slug>
```

- [ ] **Step 6: No commit needed for this task** (it's verification only).

---

## Task 8: Open PR

- [ ] **Step 1: Push the branch**

Per `CLAUDE.md` AGENT PUSH RULES, use `scripts/agent-push.sh`:

```bash
scripts/agent-push.sh -u origin feat/session-overlap-default
```

- [ ] **Step 2: Open PR with summary**

```bash
gh pr create --title "feat: session-overlap detection + worktree default" --body "$(cat <<'EOF'
## Summary

- Adds `SessionStart` hook (`.claude/hooks/detect-session-overlap.sh`) that detects other live Claude sessions sharing this cwd and emits a directive when overlap is found.
- Adds `WorktreeCreate` hook (`.claude/hooks/worktree-create.sh`) that redirects native `EnterWorktree` output to `~/code/tau-worktrees/<name>` (overridable via `TAU_WORKTREE_DIR`).
- Adds `.claude/SESSION-RULES.md` policy: when overlap is detected, invoke `superpowers:using-git-worktrees` → `EnterWorktree`.
- Adds `.worktreeinclude` so new worktrees inherit `.env*` files.
- Wires both hooks via `.claude/settings.json` with `worktree.baseRef: "fresh"`.

Uses only native Claude Code extension points. No custom worktree-creation script — `EnterWorktree` is the API.

Spec: `docs/superpowers/specs/2026-05-17-session-detection-worktree-default-design.md`.

## Test plan

- [ ] Single session in `tau/` → `overlap_action: none`
- [ ] Two foreground sessions in `tau/` → second routes to `~/code/tau-worktrees/<slug>/`
- [ ] Session inside an existing worktree → `in_linked_worktree: yes`, no double-route
- [ ] Detector with `chmod -x` → session still starts (failure non-blocking)
- [ ] `TAU_WORKTREE_DIR=/tmp/x` honored by WorktreeCreate hook
- [ ] `.worktreeinclude` copies `.env*` into a new worktree

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 3: Wait for CI to pass, request review, merge.**

---

## Self-review

**Spec coverage check:**

| Spec requirement | Task |
|---|---|
| `detect-session-overlap.sh` SessionStart hook | Task 3 |
| `worktree-create.sh` WorktreeCreate hook | Task 2 |
| `SESSION-RULES.md` policy | Task 4 |
| `.claude/settings.json` wiring | Task 6 |
| `.worktreeinclude` | Task 5 |
| Output schema (cwd, branch, in_linked_worktree, other_sessions_same_cwd, cargo_processes_active, git_index_locked, overlap_action, suggested_slug) | Task 3 Step 2 |
| Hook exits 0 on every path | Task 3 Step 2 (`set -uo pipefail` — note: deliberately NOT `-e`) |
| Hook runtime < 200 ms | Implicit; if test reveals it's slower, optimize (no explicit task, deferred) |
| WorktreeCreate idempotency | Task 2 Step 4 |
| `TAU_WORKTREE_DIR` override | Task 2 Step 5 / Rule 5 in SESSION-RULES.md |
| `worktree.baseRef` honored by hook | Task 2 Step 2 (reads `git config worktree.baseRef`) |
| Submodule guard | Task 3 Step 2 |
| Already-isolated short-circuit | Task 3 Step 2, tested Task 3 Step 5 |
| Subagent isolation via `isolation: worktree` | Native — no work needed |
| Auto-cleanup via `cleanupPeriodDays` | Native — no work needed |
| Smoke tests (single / two / nested-worktree / broken hook) | Task 7 |
| PR / merge workflow | Task 8 |

All spec sections have at least one task. The "hook runtime < 200ms" requirement has no explicit measurement task — if Task 7 reveals lag, add a `time` invocation. Acceptable risk: warm-cache execution of pure-bash + jq + git is well under that budget on macOS.

**Placeholder scan:** Searched for "TBD", "TODO", "implement later", "fill in", "handle edge cases" — none present in plan steps. The `.worktreeinclude` contents include "(if any)" qualifiers but Step 1 of Task 5 verifies what to include before writing the file; this is a deliberate probe-then-write pattern, not a placeholder.

**Type consistency:** `name` field in WorktreeCreate stdin matches `name` in jq filter in Task 2 Step 2. `cwd`, `pid`, `sessionId` field names match the documented session-state schema in Task 1. The detector's output field names (`other_sessions_same_cwd`, `overlap_action`, etc.) match the SESSION-RULES.md references in Task 4.

No issues found.
