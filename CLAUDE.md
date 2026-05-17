# CARGO RULES — read before running any cargo command

This workspace has 8 crates sharing one `target/.cargo-lock`. Concurrent
cargo invocations queue on this lock and waste 2–4 minutes per build.
Every cargo command MUST follow these rules. No exceptions.

## Rule 1: Always set CARGO_TARGET_DIR

NEVER run bare `cargo`. ALWAYS prefix with `CARGO_TARGET_DIR=<path>`.

| Caller | CARGO_TARGET_DIR value |
|---|---|
| Main agent (top-level Bash tool) | `target/main` |
| Any subagent spawned via Agent tool | `target/agent-<role>` where `<role>` is the subagent's purpose (e.g. `spec-review`, `solution-review`, `impl`, `adversary`) |
| One-off diagnostic from main agent (cargo --version, cargo metadata, etc.) | `target/main` |
| `lefthook` pre-commit hooks (host-side) | `target/lefthook/fmt`, `target/lefthook/clippy`, `target/lefthook/test`, `target/lefthook/check-linux` (one per command) |
| `lefthook` pre-push hook (Podman container) | `target/lefthook-podman` (mounted as a named Podman volume `target-cache` so it persists across runs) |

If you cannot determine your role, use `target/agent-misc`. Never omit the variable.

The `target/lefthook/*` and `target/lefthook-podman` paths are reserved
for the pre-commit and pre-push git hooks defined in `lefthook.yml`.
Contributors install them with `lefthook install` after `brew install
lefthook podman`. See `docs/dev-environment.md` for full setup.

## Rule 2: Always scope to a single crate

Use `-p <crate>`. Never invoke cargo from the workspace root without `-p`.

✅ `CARGO_TARGET_DIR=target/main cargo test -p tau-domain`
❌ `cargo test`
❌ `cargo test --workspace`
❌ `CARGO_TARGET_DIR=target/main cargo test`  (no -p)

## Rule 3: Always wrap with timeout

| Command | Timeout |
|---|---|
| `cargo test` | 300s |
| `cargo build` / `cargo check` | 180s |
| `cargo clippy` | 240s |
| `cargo fmt --check` | 30s |

Format: `timeout 300 env CARGO_TARGET_DIR=target/main cargo test -p tau-domain`

## Rule 4: Always set CARGO_INCREMENTAL=0

Cargo's incremental compilation defaults to `1` (on) for the dev
profile. sccache cannot deduplicate incremental-compilation outputs
because they embed compilation-state metadata, so leaving incremental
on means **0% Rust cache hit rate** through sccache (verified —
3,907 hits / 2,854 misses without `CARGO_INCREMENTAL=0`, all 2 of the
hits were Rust). Disabling incremental restores normal sccache
caching.

Per-agent target dirs (Rule 1) plus sccache (with incremental
disabled) gives the best of both worlds: each agent has an isolated
target dir that doesn't collide with the main agent's, but the
underlying rustc cache is shared via sccache.

Combine with Rule 1:

    timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-<role> cargo test -p <crate>

## Rule 5: Before invoking cargo, check for active builds

If another cargo process is running on a shared target dir, your build
will queue on the lock. Quick check:

    pgrep -af cargo | grep -v grep

If you see another cargo invocation using the same CARGO_TARGET_DIR you
were about to use, EITHER wait for it OR pick a different target dir
(e.g. `target/agent-<role>-2`). Do not just launch and hope.

## Rule 6: Prefer `cargo nextest` for tests

CI runs `cargo nextest run` everywhere except doctests. Using nextest
locally matches CI behavior more closely (per-test isolation, parallel
binary execution). Install once: `cargo install cargo-nextest --locked`.

For doctests, still use `cargo test --doc` — nextest doctest support is
incomplete.

`.config/nextest.toml` configures `retries = 2` to handle timing-sensitive
flakes that nextest's parallelism can expose vs cargo test's serial
execution.

## Why these rules exist

Past sessions accumulated 24 lock-contended builds totaling ~36 minutes
of pure waiting. `sccache` (`RUSTC_WRAPPER=sccache`, set in user env)
ensures distinct target dirs share the rustc compile cache, so the disk
and CPU cost of multiple target dirs is negligible. The rule eliminates
contention without sacrificing speed.

## Reference command shape

Copy-paste template, fill in `<role>`, `<crate>`, and the actual cargo args:

    timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-<role> cargo test -p <crate>

# AGENT PUSH RULES — read before running `git push`

When invoking `git push` from an agent runtime (Claude Code's Bash tool,
similar), `git push` is silently terminated mid-hook if the lefthook
pre-push hook spawns a long-running container (the deep gate runs all
10 Linux CI jobs in Podman, ~3-4 min warm / ~15-20 min cold). The
container survives orphaned because Podman owns it; the push itself
never completes. Diagnosed 2026-05-09.

Empirical:

- Plain `run_in_background` bash + sleep loops survive 60s+
- Plain `run_in_background` podman containers survive 60s+
- `git push` triggering the deep gate dies mid-hook every time

The kill is specific to the git-push-invokes-long-running-hook path,
not background commands generally. Likely cause: signal propagation
when the hook's stdout/stderr are wired through git push's pipe.

## Rule: never `git push` directly from agent runtime when the gate is on

Three options, ordered by preference:

1. **`scripts/agent-push.sh`** — runs `lefthook run pre-push` as a
   standalone command (which does NOT die), then `git push --no-verify`
   (fast network-only step). Forwards args. Use this by default.

2. **`git push --no-verify`** — bypass the gate entirely. Acceptable
   for docs-only / yaml-only changes where the gate adds nothing.
   Document the bypass in the commit message; CI is the safety net.

3. **`lefthook run pre-push && git push --no-verify`** — inline form
   of option 1 if the script isn't available. Same effect.

NEVER:

- Run `git push` (no flags) and expect it to complete with the gate
  active. It will silently die.
- Bypass with `--no-verify` for Rust code changes when the agent
  runtime is the only Linux validation surface, unless you've run
  the gate separately first.

If you observe a `git push` that produced 0 bytes of output and no
error message, the silent-kill happened. Recover by:

    podman ps   # zombie gate container probably still running
    podman rm -f <container-id>   # clean it up
    scripts/agent-push.sh         # try again the right way

## Keeping PRs up-to-date with main

Branch protection on `main` is `strict: true` — PRs must be up-to-date
with `main` to merge. When other sessions land commits while your PR
is open:

    gh pr update-branch <PR#>

adds a merge-commit from main into the PR branch via GitHub's "Update
branch" button. No local rebase, no force-push, triggers one fresh CI
run. Squash merge collapses the merge commit at merge time so history
stays clean.

Do NOT use `gh pr merge --auto` (auto-merge is disabled at repo
level) or `gh pr merge --admin` (`enforce_admins: true` blocks admin
bypass). The only sanctioned mergeability fix is `update-branch` (or a
local rebase + `scripts/agent-push.sh`).

## Lefthook tests can corrupt git identity

The lefthook integration test suite writes `Test User
<test@example.com>` to the worktree-local `[user]` config and does
not always restore it. A subsequent commit then picks up that
identity. Safe pattern for every agent-driven commit:

    git -c user.name="<real>" -c user.email="<real>" \
      commit --no-verify -m "..."

`-c` overrides at the command level without persisting. Combined
with `--no-verify` (acceptable for docs-only changes per the rules
above), this also avoids re-triggering the corrupting test run.

# DOCS RULES — read before editing anything under `docs/`

The published book is `mdbook build` + `mdbook-linkcheck` over the
`docs/` tree, deployed to GitHub Pages by `.github/workflows/docs-deploy.yml`.
`book.toml` sets `warning-policy = "error"` for linkcheck, so a single
broken link fails the deploy job.

## Rule: build the book locally before opening a docs PR

Both binaries live at `~/.cargo/bin/{mdbook,mdbook-linkcheck}` but
that directory is not on the agent runtime's PATH. Build with PATH
prepended for the duration of the call, from the `docs/` directory:

    cd docs && PATH="$HOME/.cargo/bin:$PATH" mdbook build

A clean build produces only `[INFO]` lines and leaves a `docs/book/`
tree (`book/html/` for the site, `book/linkcheck/` for the link
report). Remove `docs/book/` before committing — it is gitignored, but
worth `rm -rf docs/book` after verifying.

If either binary is missing, install once (the user must invoke this,
not the agent — `cargo install` of agent-chosen packages is denied):

    cargo install mdbook --locked --version ^0.4
    cargo install mdbook-linkcheck --locked --version ^0.7

## Rule: every doc page must be in `SUMMARY.md`

mdBook silently skips pages not listed in `docs/SUMMARY.md`. New
ADRs, tutorials, how-tos, reference pages, and explanation pages all
need a corresponding line. Linkcheck only verifies links between
pages that *are* in SUMMARY, so a forgotten entry hides both the page
and any broken outbound links it contains.

## Rule: docs-only PRs may bypass the pre-push gate

The lefthook deep gate is Rust-CI mirroring; it adds nothing to a
docs-only change and the gate has its own silent-kill failure mode
under `git push` (see AGENT PUSH RULES above). For pure
`docs/**` + `.md` changes:

    git push --no-verify

is sanctioned. CI's `docs-deploy` job is the real gate. If the PR
also touches Rust, follow AGENT PUSH RULES and run the full gate.

## Rule: the live URL is `lebocqtitouan.github.io/tau/`

The repository is `LEBOCQTitouan/tau` (capitalized). GitHub Pages
lowercases the owner, so the deployed site is at
`https://lebocqtitouan.github.io/tau/latest/`. `titouanlebocq.github.io`
returns 404 — do not confuse the two when smoke-testing a deploy.
