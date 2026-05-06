# Dev Environment + Pre-Push Test Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add automatic two-tier git-hook gate (pre-commit fast checks on host + pre-push deep gate in privileged Podman container) that catches cross-platform build/runtime issues locally before pushing to CI.

**Architecture:** lefthook drives both hooks. Pre-commit runs `cargo fmt`/`clippy`/`nextest`/`check --target x86_64-unknown-linux-gnu` on the macOS host. Pre-push spins up an ephemeral Podman container from `rust:1.82-bookworm` with selective Linux caps (`SYS_ADMIN + NET_ADMIN + seccomp/apparmor unconfined`) and runs the full Rust workspace test suite. Persistent named Podman volumes (`cargo-cache`, `target-cache`) make warm runs ~20s vs cold ~3 min. CI's `test-net-filter / linux` job is updated to use the same selective caps for parity.

**Tech Stack:** lefthook (FOSS, MIT, Go binary), Podman (FOSS, Apache 2.0), Apple Virtualization.framework (built into macOS), `rust:1.82-bookworm` Docker image (matches existing CI).

**Branch:** `feat/dev-environment` (already cut from main; spec committed at `2dac2e5`)

**Spec reference:** `docs/superpowers/specs/2026-05-06-dev-environment-design.md` — six locked decisions (Linux-only scope, automatic two-tier hooks, selective caps, CI parity, persistent volumes, inline pre-push command).

**Plan-erratum carryovers (apply preemptively):**
- VERIFY against BASE_SHA = `2dac2e5` before claiming "pre-existing failure"
- Cargo.lock NOT touched — zero new Rust deps in this PR
- CI parity (T4) is the riskiest commit — committed separately so it can be reverted independently if test-net-filter / linux fails on PR CI
- Verify lefthook YAML schema at https://lefthook.dev/configuration/Lefthook.html before locking config
- The new `target/lefthook/*` and `target/lefthook-podman` dirs are added to CLAUDE.md Rule 1 in T3

**Implementer prerequisites (one-time setup, NOT a commit):**

Before T1 can be tested, the implementer must have:
```bash
brew install lefthook podman                      # Both FOSS
rustup target add x86_64-unknown-linux-gnu        # For pre-commit cross-check
podman machine init --cpus 4 --memory 8192 --rootful
podman machine start
```

If `podman machine` is already running with different settings, leave it alone — Podman is forgiving about reuse. The `--rootful` flag is required for privileged-cap containers; if the existing machine is non-rootful, recreate it (`podman machine rm && podman machine init --rootful ...`).

---

## File structure

| File | Action | Responsibility |
|---|---|---|
| `lefthook.yml` | CREATE (root) | Defines pre-commit + pre-push hooks. Pre-push command is inlined. |
| `docs/dev-environment.md` | CREATE | One-time setup, day-to-day usage, interactive-debug pattern, architecture-mismatch caveat, troubleshooting. |
| `CLAUDE.md` | MODIFY | Register `target/lefthook/{fmt,clippy,test,check-linux}` and `target/lefthook-podman` in Rule 1's target-dir table. |
| `.github/workflows/ci.yml` | MODIFY | Replace `--privileged` in test-net-filter / linux job (line ~327) with selective caps. |

---

## Task 1: Add `lefthook.yml`

**Files:**
- Create: `lefthook.yml` (repo root)

**What this delivers:** The two automatic git hooks. After this task, every `git commit` runs the fast pre-commit checks; every `git push` runs the deep Linux gate in a Podman container.

- [ ] **Step 1: Create `lefthook.yml` at the repo root**

```yaml
# lefthook.yml — automatic pre-commit + pre-push gate for tau
#
# Goal: catch cross-platform build/runtime issues locally before
# pushing, so CI is confirmation rather than discovery.
#
# Two tiers, both automatic, no manual scripts:
#   - pre-commit: fast host checks (~30-60s, parallel)
#   - pre-push:   deep Linux gate (~3-5min cold, ~20s warm via cache)
#
# Install: `lefthook install` (after `brew install lefthook`)
# Bypass for emergencies: `git commit --no-verify` / `git push --no-verify`
#
# Cargo target dirs are isolated per command per CLAUDE.md Rule 1:
#   target/lefthook/fmt
#   target/lefthook/clippy
#   target/lefthook/test
#   target/lefthook/check-linux
#   target/lefthook-podman   (inside container; mounted as named volume)
#
# These do NOT collide with target/main (main agent) or
# target/agent-* (sub-agents).

pre-commit:
  parallel: true
  commands:
    fmt:
      glob: "*.rs"
      run: env CARGO_TARGET_DIR=target/lefthook/fmt cargo fmt --all -- --check
    clippy:
      glob: "*.{rs,toml}"
      run: env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/lefthook/clippy cargo clippy --workspace --all-targets -- -D warnings
    test-native:
      glob: "*.{rs,toml}"
      run: env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/lefthook/test cargo nextest run --workspace --all-targets
    check-linux-x86:
      glob: "*.{rs,toml}"
      run: env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/lefthook/check-linux cargo check --workspace --all-targets --target x86_64-unknown-linux-gnu

pre-push:
  commands:
    deep-gate:
      # Privileged Linux container running the full Rust workspace test
      # suite. Selective caps mirror what tau's strict-tier sandbox
      # documents (see docs/decisions/0019-per-host-network-filter.md):
      #   --cap-add SYS_ADMIN          (uid_map writes after CLONE_NEWUSER)
      #   --cap-add NET_ADMIN          (veth + nftables for net filter)
      #   --security-opt seccomp=unconfined  (let tau install its own)
      #   --security-opt apparmor=unconfined (avoid distro AppArmor blocks)
      #
      # Persistent named volumes:
      #   cargo-cache:/usr/local/cargo/registry — registry index + crates
      #   target-cache:/workspace/target/lefthook-podman — incremental
      run: |
        podman run --rm \
          --cap-add SYS_ADMIN --cap-add NET_ADMIN \
          --security-opt seccomp=unconfined \
          --security-opt apparmor=unconfined \
          -v "$PWD":/workspace:Z \
          -v cargo-cache:/usr/local/cargo/registry \
          -v target-cache:/workspace/target/lefthook-podman \
          -w /workspace \
          docker.io/library/rust:1.82-bookworm \
          bash -c '
            set -e
            apt-get update -qq
            apt-get install -y -qq iproute2 nftables
            env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/lefthook-podman \
              cargo nextest run --workspace --all-targets
          '
```

- [ ] **Step 2: Wire git hooks**

```bash
lefthook install
```

Expected output: lefthook reports the hooks installed in `.git/hooks/`.

- [ ] **Step 3: Verify pre-commit runs cleanly on the current tree**

```bash
lefthook run pre-commit --all-files
```

Expected: all four commands (fmt, clippy, test-native, check-linux-x86) exit 0. Wall-clock ~30-60s.

If this fails, the failure is unrelated to this commit (the working tree is clean spec-only changes); investigate separately. Do NOT proceed to Step 4 with a failing pre-commit.

- [ ] **Step 4: Verify pre-push deep gate runs cleanly**

```bash
lefthook run pre-push --all-files
```

Expected: lefthook invokes `podman run ...` which spins up the privileged container, installs iproute2/nftables, and runs `cargo nextest run --workspace --all-targets`. All tests pass. Wall-clock ~3-5 min cold.

**If pre-push fails because of insufficient caps**: do NOT widen to `--privileged`. Identify the missing cap from the error message (e.g., `EPERM` on a specific syscall). Add the cap to the `--cap-add` list in `lefthook.yml`, document why in a comment referencing the source of the requirement (a kernel man page or an existing test), re-run `lefthook run pre-push --all-files`, and only commit when the gate passes with the expanded list. The cap list IS the documented contract; widening must be deliberate and justified.

**If pre-push fails because of a real test regression**: stop. The spec assumes the current main is green. Investigate the regression separately before continuing this task.

- [ ] **Step 5: Commit**

```bash
git add lefthook.yml
git commit -m "ci: lefthook pre-commit + pre-push gate (Linux dev env)

Automatic two-tier git-hook gate. Pre-commit runs fast host checks
(fmt + clippy + native macOS nextest + cross-compile-check Linux
x86_64) ~30-60s. Pre-push runs the privileged Linux deep gate in a
Podman container with selective caps (SYS_ADMIN + NET_ADMIN +
seccomp/apparmor unconfined) ~3-5min cold, ~20s warm via persistent
named volumes (cargo-cache + target-cache).

Selective caps mirror what tau's strict-tier sandbox documents in
ADR-0019 — tighter than --privileged so the gate catches privilege
drift if a future test silently grows a new cap dependency.

Bypass with --no-verify for emergencies; not for routine use.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 2: Add `docs/dev-environment.md`

**Files:**
- Create: `docs/dev-environment.md`

**What this delivers:** Contributor-facing setup + usage doc for the new gate. Covers one-time install, day-to-day, interactive-debug pattern, architecture-mismatch caveat, troubleshooting.

- [ ] **Step 1: Create `docs/dev-environment.md`**

```markdown
# tau dev environment — Linux pre-commit + pre-push gate

Goal: catch cross-platform build/runtime issues locally before pushing, so CI is confirmation rather than discovery. Also unblocks local debugging of F task 6.5 follow-ups (strict_net_filter integration test hang, Container-adapter network filtering).

This iteration covers **Linux only** (running on Apple Silicon Mac). Windows + macOS legs are deferred to follow-up PRs.

## TL;DR — one-time setup

```bash
# Install tools (FOSS only)
brew install lefthook podman
rustup target add x86_64-unknown-linux-gnu

# Initialize Podman's hidden Linux VM (--rootful required for caps)
podman machine init --cpus 4 --memory 8192 --rootful
podman machine start

# Wire git hooks
lefthook install

# Verify
lefthook run pre-commit --all-files     # ~30-60s, must exit 0
lefthook run pre-push --all-files       # ~3-5min cold, must exit 0
```

After this, every `git commit` runs the fast checks; every `git push` runs the deep Linux gate.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│ Apple Silicon Mac (host)                                         │
│                                                                  │
│  git commit ──── lefthook pre-commit ─── ~30-60s (host, no VM)   │
│                  • cargo fmt --all -- --check                    │
│                  • cargo clippy --workspace --all-targets        │
│                  • cargo nextest run --workspace --all-targets   │
│                  • cargo check --target x86_64-unknown-linux-gnu │
│                                                                  │
│  git push ─────── lefthook pre-push ────  ~3-5min (Linux VM)     │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │ Podman machine (Linux VM, persistent)                      │  │
│  │                                                            │  │
│  │  ephemeral container per pre-push run:                     │  │
│  │  podman run --rm                                           │  │
│  │    --cap-add SYS_ADMIN --cap-add NET_ADMIN                 │  │
│  │    --security-opt seccomp=unconfined                       │  │
│  │    --security-opt apparmor=unconfined                      │  │
│  │    -v $WORKSPACE:/workspace                                │  │
│  │    -v cargo-cache:/usr/local/cargo/registry                │  │
│  │    -v target-cache:/workspace/target/lefthook-podman       │  │
│  │    rust:1.82-bookworm                                      │  │
│  │    bash -c 'cargo nextest run --workspace --all-targets'   │  │
│  └────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────┘
```

## Interactive Linux debugging

When you need to investigate a test failure or reproduce a bug interactively (e.g., the strict_net_filter integration test hang from F task 6.5 follow-ups), use the same image and caps as the gate, just with `bash` instead of `cargo nextest`:

```bash
podman run -it --rm \
  --cap-add SYS_ADMIN --cap-add NET_ADMIN \
  --security-opt seccomp=unconfined \
  --security-opt apparmor=unconfined \
  -v "$PWD":/workspace:Z \
  -v cargo-cache:/usr/local/cargo/registry \
  -v target-cache:/workspace/target/lefthook-podman \
  -w /workspace \
  docker.io/library/rust:1.82-bookworm \
  bash
```

Inside the container, `apt-get install -y iproute2 nftables` then run cargo commands directly. Same kernel, same caps, same crate cache as the automated gate — what fails interactively will fail in the gate, and vice versa.

## Bypassing the gate (emergencies only)

```bash
git commit --no-verify        # skip pre-commit
git push --no-verify          # skip pre-push
```

Don't do this routinely. The gate exists so CI doesn't have to find the bugs.

## Architecture mismatch (known gaps)

Apple Silicon is arm64; CI Linux runners are x86_64. The local gate will catch:

- ✅ All cfg-gating bugs (`cfg(unix)`, `cfg(target_os)`)
- ✅ All build-time API mismatches (the `std::os::fd` class)
- ✅ Most Linux runtime regressions (sandbox tests, IPC, syscall behavior on arm64 Linux)
- ✅ Privilege drift (selective-cap regressions)

The local gate will NOT catch:

- ❌ Arch-specific runtime bugs that only manifest on x86_64 (struct alignment, atomic ordering, syscall numbers in raw asm — landlock syscall numbers differ between x86_64 and arm64 Linux)
- ❌ glibc-version differences (Podman ships current Debian; CI ships Ubuntu)
- ❌ Windows-specific issues (deferred to follow-up PR)
- ❌ macOS-specific issues that require a fresh macOS VM (deferred indefinitely; Mac coverage stays via the host)

These remaining gaps are caught by CI. The pre-push gate covers ~95% of the cross-platform pain points; CI handles the rest.

## Troubleshooting

**`lefthook: command not found`**: open a new shell after `brew install lefthook` to pick up `$PATH` updates.

**`podman machine` is non-rootful**: privileged-style containers won't work. Recreate:
```bash
podman machine stop
podman machine rm
podman machine init --cpus 4 --memory 8192 --rootful
podman machine start
```

**Pre-push hangs at apt-get**: the container is a fresh Debian — `apt-get update` reaches Debian mirrors. If you're behind a corporate proxy, configure Podman to use it (`podman machine ssh` then add proxy env vars to `/etc/profile.d/`).

**Pre-push fails with permission denied on a syscall**: a test is using a cap outside the documented list. Either expand the cap list in `lefthook.yml` (with a justifying comment) or fix the test to not need that cap. Never silently widen to `--privileged` — that defeats the gate.

**Pre-commit hook didn't run**: check `.git/hooks/pre-commit` exists. If not, re-run `lefthook install`.

**Cargo target dir collision**: `lefthook` uses `target/lefthook/{fmt,clippy,test,check-linux}` and `target/lefthook-podman`. These don't collide with `target/main` (main agent) or `target/agent-*` (sub-agents) per [CLAUDE.md Rule 1](../CLAUDE.md). If you see lock contention, ensure your bare cargo invocations are using the right `CARGO_TARGET_DIR`.

**Container-VM disk space fills up**: `podman machine` defaults are conservative. If you hit "no space left on device", increase the machine disk size:
```bash
podman machine stop
podman machine set --disk-size 100
podman machine start
```

## What this enables

After this PR ships:
1. F task 6.5 follow-up #2 (strict_net_filter integration test hang) becomes debuggable locally — `podman run -it ...` reproduces the privileged-Linux environment for interactive `gdb`/`strace` work.
2. F task 6.5 follow-up #1 (Container-adapter network filtering) becomes debuggable locally via the same mechanism.
3. The cfg(unix) / cfg(target_os) class of bug is caught at commit time, not CI time.
4. Privilege drift is caught at commit/push time, not silently masked by CI's previous `--privileged`.

## Out of scope (follow-up PRs)

- **Windows VM (UTM + Windows 11 ARM)** — largest setup; tracked as PR2 candidate.
- **macOS VM (Tart)** — Tart is Fair Source, not OSI FOSS; not pursued.
- **x86_64 Linux runtime via QEMU** — too slow; CI provides x86_64 ground truth.
- **Pre-commit fix mode** — the gate is verify-only (`cargo fmt -- --check`), not auto-rewrite (`cargo fmt`).
```

- [ ] **Step 2: Verify the doc renders sanely**

```bash
# Markdown link check (best-effort; skip if mdformat unavailable)
test -f docs/dev-environment.md && head -20 docs/dev-environment.md
```

Expected: file exists, first lines render the title + TL;DR.

- [ ] **Step 3: Commit**

```bash
git add docs/dev-environment.md
git commit -m "docs(dev-environment): setup + usage for Linux pre-push gate

Contributor-facing doc covering one-time setup (brew install lefthook
podman, rustup target add, podman machine init --rootful, lefthook
install), day-to-day, interactive Linux debugging via the same Podman
container, the architecture-mismatch caveat (arm64-local vs x86_64-CI),
bypass instructions, and troubleshooting.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 3: Update `CLAUDE.md` to register new target dirs

**Files:**
- Modify: `CLAUDE.md` (Rule 1 target-dir table around line 11-15)

**What this delivers:** future Claude sessions (main agents and subagents) see the new target dirs reserved by lefthook and don't accidentally collide with them.

- [ ] **Step 1: Read the current Rule 1 section**

```bash
sed -n '1,35p' CLAUDE.md
```

The table at lines 11-15 today reads:

```markdown
| Caller | CARGO_TARGET_DIR value |
|---|---|
| Main agent (top-level Bash tool) | `target/main` |
| Any subagent spawned via Agent tool | `target/agent-<role>` where `<role>` is the subagent's purpose (e.g. `spec-review`, `solution-review`, `impl`, `adversary`) |
| One-off diagnostic from main agent (cargo --version, cargo metadata, etc.) | `target/main` |
```

- [ ] **Step 2: Add lefthook + Podman entries to the table**

Use the Edit tool with this old/new pair (preserves exact spacing):

old:
```
| Caller | CARGO_TARGET_DIR value |
|---|---|
| Main agent (top-level Bash tool) | `target/main` |
| Any subagent spawned via Agent tool | `target/agent-<role>` where `<role>` is the subagent's purpose (e.g. `spec-review`, `solution-review`, `impl`, `adversary`) |
| One-off diagnostic from main agent (cargo --version, cargo metadata, etc.) | `target/main` |
```

new:
```
| Caller | CARGO_TARGET_DIR value |
|---|---|
| Main agent (top-level Bash tool) | `target/main` |
| Any subagent spawned via Agent tool | `target/agent-<role>` where `<role>` is the subagent's purpose (e.g. `spec-review`, `solution-review`, `impl`, `adversary`) |
| One-off diagnostic from main agent (cargo --version, cargo metadata, etc.) | `target/main` |
| `lefthook` pre-commit hooks (host-side) | `target/lefthook/fmt`, `target/lefthook/clippy`, `target/lefthook/test`, `target/lefthook/check-linux` (one per command) |
| `lefthook` pre-push hook (Podman container) | `target/lefthook-podman` (mounted as a named Podman volume `target-cache` so it persists across runs) |
```

- [ ] **Step 3: Append a setup note to the rule's prose**

Find the existing line (~line 17):

old:
```
If you cannot determine your role, use `target/agent-misc`. Never omit the variable.
```

new:
```
If you cannot determine your role, use `target/agent-misc`. Never omit the variable.

The `target/lefthook/*` and `target/lefthook-podman` paths are reserved
for the pre-commit and pre-push git hooks defined in `lefthook.yml`.
Contributors install them with `lefthook install` after `brew install
lefthook podman`. See `docs/dev-environment.md` for full setup.
```

- [ ] **Step 4: Verify the change**

```bash
sed -n '1,25p' CLAUDE.md
```

Expected: the table now has 5 rows (was 3), and the prose mentions lefthook + dev-environment.md.

- [ ] **Step 5: Commit**

```bash
git add CLAUDE.md
git commit -m "docs(claude): register lefthook target dirs in Rule 1

The pre-commit hooks use target/lefthook/{fmt,clippy,test,check-linux}
on the macOS host; the pre-push deep gate uses target/lefthook-podman
inside the Podman container (mounted as a named volume target-cache).
Neither collides with target/main (main agent) or target/agent-*
(subagents). Setup pointer added to docs/dev-environment.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 4: Update `.github/workflows/ci.yml` for CI parity

**Files:**
- Modify: `.github/workflows/ci.yml` (test-net-filter / linux job, around line 327)

**What this delivers:** CI's `test-net-filter / linux` job uses the same selective caps as the local lefthook deep gate. This is the "drift detection" guarantee — Decision 4 in the spec. Committed separately so it can be reverted if it surfaces an unexpected cap dependency.

- [ ] **Step 1: Locate the current `--privileged` line**

```bash
grep -n "docker run --rm --privileged" .github/workflows/ci.yml
```

Expected: one match around line 327.

Read the surrounding 8 lines for context:

```bash
sed -n '325,340p' .github/workflows/ci.yml
```

- [ ] **Step 2: Replace `--privileged` with selective caps**

Use the Edit tool with this old/new pair (the surrounding lines are preserved exactly; ONLY the flags on the `docker run` line change):

old:
```
          docker run --rm --privileged \
            -v "$PWD":/workspace \
            -w /workspace \
            rust:1.82-bookworm \
```

new:
```
          docker run --rm \
            --cap-add SYS_ADMIN --cap-add NET_ADMIN \
            --security-opt seccomp=unconfined \
            --security-opt apparmor=unconfined \
            -v "$PWD":/workspace \
            -w /workspace \
            rust:1.82-bookworm \
```

- [ ] **Step 3: Verify the YAML is still valid**

```bash
# Quick smoke test: parse the workflow with python's yaml module (built-in)
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))" && echo "yaml-ok"
```

Expected: prints `yaml-ok`. If it errors, the indentation or quoting got mangled — revert the edit and try again.

- [ ] **Step 4: Verify the test-net-filter / linux command still makes sense**

```bash
sed -n '315,345p' .github/workflows/ci.yml
```

Expected: the job's `docker run` invocation now lists 4 explicit flags (`--cap-add SYS_ADMIN`, `--cap-add NET_ADMIN`, `--security-opt seccomp=unconfined`, `--security-opt apparmor=unconfined`) instead of `--privileged`. The body of the bash script (`apt-get install ... cargo test ...`) is unchanged.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: test-net-filter / linux uses selective caps (parity with local gate)

Replaces --privileged with the same cap set the local lefthook
pre-push gate uses (--cap-add SYS_ADMIN --cap-add NET_ADMIN
--security-opt seccomp=unconfined --security-opt apparmor=unconfined).

Catches privilege drift at both ends: a test that silently grows a
new cap dependency now fails BOTH locally and in CI, instead of
passing under the permissive --privileged and only failing locally.

If this commit breaks test-net-filter / linux on CI: a test relies
on a cap outside the documented set in ADR-0019. Either expand the
cap list (with justification) in BOTH lefthook.yml and this file, or
revert this commit only.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 5: USER GATE — Open PR + monitor CI

**Files:** none (git ops only)

**What this delivers:** the PR is open, CI runs, the user reviews and merges.

- [ ] **Step 1: Push the branch**

```bash
git push -u origin feat/dev-environment
```

Expected: branch pushes; `gh` reports a "create a PR" URL.

- [ ] **Step 2: Open the PR**

```bash
gh pr create --base main --head feat/dev-environment --title "feat(dev-env): lefthook pre-commit + pre-push gate (Linux iteration)" --body "$(cat <<'EOF'
## Summary

Adds an automatic two-tier git-hook gate so cross-platform issues are caught locally before reaching CI:

- **pre-commit** (~30-60s, host): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace --all-targets`, `cargo check --workspace --all-targets --target x86_64-unknown-linux-gnu`.
- **pre-push** (~3-5min cold, ~20s warm): privileged Podman container (`rust:1.82-bookworm`) with selective caps (`SYS_ADMIN + NET_ADMIN + seccomp/apparmor unconfined`) running the full `cargo nextest run --workspace --all-targets`. Persistent named volumes (`cargo-cache` + `target-cache`) make warm runs fast.

Both hooks are wired automatically by `lefthook install`. No manual scripts.

CI's `test-net-filter / linux` job is updated to use the same selective caps for parity (catches privilege drift in CI too, not just locally).

## Locked decisions (from spec)

1. Linux-only this iteration; Windows + macOS as follow-up PRs
2. Two automatic git hooks (no manual user-invoked scripts)
3. Selective caps (`SYS_ADMIN + NET_ADMIN + seccomp/apparmor unconfined`) instead of `--privileged` — catches privilege drift
4. CI parity update (test-net-filter / linux)
5. Persistent named Podman volumes for warm-run speed
6. Pre-push command inlined in `lefthook.yml` (no sidecar shell)

## What this unblocks

- F task 6.5 follow-up #2 (`strict_net_filter` integration test hang) — now debuggable locally via `podman run -it ...`
- F task 6.5 follow-up #1 (Container-adapter network filtering) — same
- The `cfg(unix)` class of bug (caught the day of, not at CI time)
- Privilege drift in CI as well as locally

## Test plan

- [x] `lefthook run pre-commit --all-files` exits 0 locally
- [x] `lefthook run pre-push --all-files` exits 0 locally (Podman container with selective caps successfully runs the full workspace test suite)
- [ ] CI: rustfmt + clippy + test-stable / linux + test-stable / windows + test-stable / macos pass
- [ ] CI: **test-net-filter / linux** passes under selective caps (the parity migration; if this fails, expand caps with justification or revert just commit T4)
- [ ] User reviews `docs/dev-environment.md` for completeness

## Spec + plan

- Spec: `docs/superpowers/specs/2026-05-06-dev-environment-design.md`
- Plan: `docs/superpowers/plans/2026-05-06-dev-environment.md`

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Expected: prints PR URL.

- [ ] **Step 3: Monitor CI**

Use a Monitor command that emits each transitioning check:

```bash
prev=""; while true; do s=$(gh pr checks <PR#> --json name,bucket 2>/dev/null || echo "[]"); cur=$(jq -r '.[] | select(.bucket!="pending") | "\(.name): \(.bucket)"' <<<"$s" | sort); comm -13 <(echo "$prev") <(echo "$cur"); prev=$cur; jq -e 'length>0 and all(.bucket!="pending")' <<<"$s" >/dev/null 2>&1 && break; sleep 30; done; echo "ALL_DONE"
```

Replace `<PR#>` with the actual PR number.

**Special attention to `test-net-filter / linux`** — this is the parity-migration leg from Task 4. If it fails:
- Read the failure log (`gh api /repos/<owner>/<repo>/actions/jobs/<id>/logs`)
- Identify the missing cap (likely from an `EPERM` on a specific syscall)
- Expand the cap list in BOTH `lefthook.yml` and `.github/workflows/ci.yml`, document why in a code comment, push a fix commit
- OR if the cap is something we don't want in production (e.g., `CAP_SYS_PTRACE`), revert just the Task 4 commit and document the divergence in the spec as a follow-up

If everything passes: green. Surface the PR URL to the user and wait for them to merge.

- [ ] **Step 4: PAUSE for user merge**

User reviews the PR + diffs (especially `docs/dev-environment.md` for completeness, and the `ci.yml` change for safety). User merges via `gh pr merge <PR#> --squash --delete-branch`.

Do NOT proceed to Task 6 until the user confirms the merge.

---

## Task 6: USER GATE — Final squash-merge + cleanup

**Files:** none (git ops only)

**What this delivers:** branch closed, main updated, work tracked.

- [ ] **Step 1: Verify the merge landed on main**

```bash
git fetch origin main
git log origin/main --oneline -5
```

Expected: the most recent commit is the squashed PR (title starts with `feat(dev-env):`), at the head of main.

- [ ] **Step 2: Switch back to main locally and clean up**

```bash
git checkout main
git pull origin main
git branch -d feat/dev-environment 2>&1 || true
```

The local feat/dev-environment branch may already be auto-deleted (if the user used `gh pr merge --delete-branch`); the `|| true` swallows the "already gone" case.

- [ ] **Step 3: Update memory pointer**

Add a brief entry to the auto-memory:

```bash
# In ~/.claude/projects/-Users-titouanlebocq-code-tau/memory/MEMORY.md,
# add an indexed pointer to the new dev environment work:

# - [tau dev environment 2026-05-06](project_dev_environment_2026_05_06.md) — lefthook pre-commit + pre-push gate (Linux only); unblocks F task 6.5 follow-ups
```

Then write the referenced memory file with details about how the gate works, what's in scope, and what's deferred (Windows + macOS follow-ups). The existing `project_containerization_research_2026_05_06.md` memory file remains relevant for the deferred legs.

This memory entry is the closing-out signal for this iteration. Future sessions starting with "let's improve the dev env" should pick up from this snapshot.

---

## Self-review checklist (run before declaring the plan done)

**Spec coverage:**
- [x] Decision 1 (Linux-only scope) → covered by entire plan; no Windows/Mac tasks
- [x] Decision 2 (two-tier automatic hooks) → Task 1
- [x] Decision 3 (selective caps) → Task 1 (lefthook.yml), Task 4 (ci.yml)
- [x] Decision 4 (CI parity) → Task 4
- [x] Decision 5 (persistent volumes) → Task 1 (volume mounts in YAML)
- [x] Decision 6 (inline pre-push command) → Task 1 (no sidecar shell)
- [x] Files added (lefthook.yml, dev-environment.md) → Tasks 1, 2
- [x] Files updated (CLAUDE.md, ci.yml) → Tasks 3, 4

**Placeholder scan:** no TBD/TODO/"add appropriate"/"similar to Task N" patterns in this plan. All commands are exact, all code blocks are complete.

**Type consistency:**
- `lefthook.yml` filename — same in plan, spec, dev-environment.md, CLAUDE.md
- `target/lefthook/{fmt,clippy,test,check-linux}` — consistent across plan and CLAUDE.md update
- `target/lefthook-podman` — consistent across plan and CLAUDE.md update
- Cap list (`SYS_ADMIN + NET_ADMIN + seccomp=unconfined + apparmor=unconfined`) — consistent across lefthook.yml, ci.yml, and dev-environment.md
- Image (`docker.io/library/rust:1.82-bookworm` in lefthook.yml; `rust:1.82-bookworm` in ci.yml) — both refer to the same image; the registry prefix is optional
- Branch name `feat/dev-environment` — consistent throughout

No issues found.
