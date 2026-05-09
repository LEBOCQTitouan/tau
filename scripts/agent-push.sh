#!/bin/bash
# scripts/agent-push.sh — push a branch with the lefthook pre-push gate
# running as a separate top-level command, sidestepping the silent-kill
# behavior that occurs when an agent runtime invokes `git push` with a
# long-running pre-push hook attached.
#
# # Why this exists
#
# When an agent's Bash runtime invokes `git push` and the pre-push hook
# spawns a long-running container (the deep gate runs all 10 Linux CI
# jobs in Podman, ~3-4 min warm / ~15-20 min cold), `git push` is
# silently terminated mid-hook by signal propagation from the runtime's
# command-management layer. The orphaned container survives because the
# Podman daemon owns it, but the actual push never completes.
#
# Empirical diagnostics (2026-05-09):
#  - Plain `run_in_background` bash + sleep loops: complete normally at
#    60s (no kill).
#  - Plain `run_in_background` podman containers: complete normally
#    at 60s (no kill).
#  - `git push` triggering the lefthook deep-gate: dies mid-hook every
#    time, leaving the gate container orphaned.
#
# This script avoids the issue by running the gate as a top-level
# standalone command (which does NOT get killed), then runs
# `git push --no-verify` as a fast network-only step (no hook).
#
# # Usage
#
#   scripts/agent-push.sh                       # pushes current branch
#   scripts/agent-push.sh -u origin <branch>    # passes through to git push
#
# Any args you pass are forwarded to the final `git push --no-verify`
# invocation. The lefthook gate always runs first; if it fails, the
# push is aborted.
#
# # When NOT to use
#
# Interactive shells running `git push` directly work fine — this
# script is only needed when the runtime is killing your `git push`
# mid-hook. Humans running git push at a terminal don't have this
# problem.

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# 1. Run the lefthook pre-push gate as a top-level command. Output is
#    streamed to stdout so the agent sees progress; exit code is
#    captured. The gate spawns the deep-gate Podman container; that
#    container's lifetime is independent of this script's shell, so
#    even if a runtime kills *this* script mid-gate, the container
#    keeps running and we see its result on the next invocation.
echo "==> Running lefthook pre-push gate as standalone command..."
if ! lefthook run pre-push; then
  echo "==> Gate FAILED. Push aborted." >&2
  exit 1
fi

# 2. Gate passed — push the bytes. --no-verify because the gate already
#    ran above; we don't want git push to re-trigger it (and re-trigger
#    the silent-kill).
echo "==> Gate passed. Pushing..."
git push --no-verify "$@"
