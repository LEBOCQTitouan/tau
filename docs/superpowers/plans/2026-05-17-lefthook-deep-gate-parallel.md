# Lefthook deep-gate parallelization — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parallelize setup, conformance, and e2e stages of `lefthook.yml`'s `pre-push:deep-gate` to reduce warm wall-clock by ≥60s while preserving CI parity.

**Architecture:** Single Podman container, single `target/lefthook-podman` cargo dir (unchanged). Bash `&`/`wait` for parallelism, with a `_par`/`_wait_all` helper pair that captures each parallel stage's output to a per-stage logfile and replays them grouped in stage order. Stages that share cargo artifacts (msrv-check, test-fixtures-ports, feature-flag-matrix, build-fixtures, test-stable, xtask-plugin-images) stay sequential.

**Tech Stack:** lefthook, bash, podman, cargo, cargo-nextest, rustup.

**Spec:** `docs/superpowers/specs/2026-05-17-lefthook-deep-gate-parallel-design.md`.

---

## Files

- Modify: `lefthook.yml` — the only file changed. All edits live inside the
  `pre-push.commands.deep-gate.run` heredoc.
- Modify: `CLAUDE.md` (small note in AGENT PUSH RULES if behavior of
  warm/cold timing changes substantially). Optional, end of plan.

There is no unit-test surface for this YAML/bash change. Verification is
running `lefthook run pre-push` against the worktree and confirming green +
faster + clean log ordering. Treat the gate run itself as the "test".

---

## Task 1: Add `_par` / `_wait_all` helpers

**Files:**
- Modify: `lefthook.yml` (inside the `pre-push.commands.deep-gate.run`
  bash heredoc, immediately after `unset CARGO_TARGET_DIR` and `TARGET=...`)

- [ ] **Step 1: Edit lefthook.yml**

Insert these two functions immediately after the existing
`TARGET=target/lefthook-podman` line and before the `# ─── 1. msrv-check`
group marker.

```bash
            # _par <label> <cmd...>  → background <cmd>, capture combined
            # output to /tmp/par-<label>.log, print "<pid>:<label>:<log>"
            # so the caller can collect tokens for _wait_all.
            _par() {
              local label=$1; shift
              local log=/tmp/par-$label.log
              ( "$@" ) >"$log" 2>&1 &
              echo "$!:$label:$log"
            }

            # _wait_all <token...>  → wait for every "pid:label:log" token
            # in order, replay each log inside ::group::<label>...::endgroup::
            # blocks, return 1 if any stage exited non-zero.
            _wait_all() {
              local fail=0
              for tok in "$@"; do
                local pid=${tok%%:*}; local rest=${tok#*:}
                local label=${rest%%:*}; local log=${rest#*:}
                if ! wait "$pid"; then fail=1; fi
                echo "::group::$label"
                cat "$log"
                echo "::endgroup::"
              done
              return $fail
            }
```

- [ ] **Step 2: Sanity check the heredoc is still well-formed**

Run: `lefthook run pre-push --dry-run 2>&1 | head -20`
Expected: no parse error, the command summary mentions `deep-gate`. If
lefthook lacks `--dry-run`, run instead:

```bash
git diff lefthook.yml | head -40
```
and visually confirm the heredoc quoting (the existing
`'"'"'-c controlled-env-binary/...'"'"'` shell-quote in the binary loop
must remain intact — don't touch that line).

- [ ] **Step 3: Commit**

```bash
git add lefthook.yml
git -c user.name="titouanlebocq" -c user.email="lebocq.tit@gmail.com" \
  commit --no-verify -m "build(lefthook): add _par/_wait_all helpers to deep-gate

No behavior change. Helpers enable subsequent parallel stages."
```

---

## Task 2: Parallelize container setup (Stage 0)

**Files:**
- Modify: `lefthook.yml` — replace the existing serial setup block
  (`apt-get update && apt-get install` + the `cargo-nextest` install
  `if`-block) with a parallel `_par` group, and remove the
  `rustup toolchain install 1.91` line from the existing Stage 1
  (msrv-check) group (it now runs in Stage 0).

- [ ] **Step 1: Replace the current setup block**

Current block (delete):

```bash
            apt-get update -qq
            apt-get install -y -qq iproute2 nftables podman

            # Install cargo-nextest (not bundled with the rust image).
            if ! command -v cargo-nextest >/dev/null; then
              ARCH=$(uname -m)
              case "$ARCH" in
                aarch64) NEXTEST_URL="https://get.nexte.st/latest/linux-arm" ;;
                *)       NEXTEST_URL="https://get.nexte.st/latest/linux" ;;
              esac
              curl -LsSf "$NEXTEST_URL" | tar zxf - -C /usr/local/cargo/bin
            fi
```

Replacement (insert):

```bash
            # ─── 0. parallel setup ────────────────────────────────
            # apt-get, cargo-nextest, and rustup MSRV install are independent
            # of each other and of cargo. Run them concurrently.
            echo "::group::setup-launch"
            T_APT=$(_par apt-deps bash -c "apt-get update -qq && apt-get install -y -qq iproute2 nftables podman")
            T_NEXTEST=$(_par nextest-install bash -c '
              if ! command -v cargo-nextest >/dev/null; then
                ARCH=$(uname -m)
                case "$ARCH" in
                  aarch64) NEXTEST_URL="https://get.nexte.st/latest/linux-arm" ;;
                  *)       NEXTEST_URL="https://get.nexte.st/latest/linux" ;;
                esac
                curl -LsSf "$NEXTEST_URL" | tar zxf - -C /usr/local/cargo/bin
              fi
            ')
            T_RUSTUP=$(_par rustup-1.91 rustup toolchain install 1.91 --profile minimal --no-self-update)
            echo "::endgroup::"
            _wait_all "$T_APT" "$T_NEXTEST" "$T_RUSTUP"
```

- [ ] **Step 2: Remove `rustup toolchain install 1.91` from Stage 1**

Find this block:

```bash
            # ─── 1. msrv-check / linux ────────────────────────────
            # CI MSRV pin is 1.91 (see ci.yml). Install + use rustup
            # to invoke it inline so we do not pollute the default
            # toolchain in the persistent volume.
            echo "::group::msrv-check"
            rustup toolchain install 1.91 --profile minimal --no-self-update >/dev/null
            cargo +1.91 check --workspace --all-targets --locked --target-dir $TARGET
            echo "::endgroup::"
```

Replace with (rustup line removed, comment updated):

```bash
            # ─── 1. msrv-check / linux ────────────────────────────
            # CI MSRV pin is 1.91 (see ci.yml). The 1.91 toolchain was
            # installed in Stage 0 in parallel with apt-deps.
            echo "::group::msrv-check"
            cargo +1.91 check --workspace --all-targets --locked --target-dir $TARGET
            echo "::endgroup::"
```

- [ ] **Step 3: Commit**

```bash
git add lefthook.yml
git -c user.name="titouanlebocq" -c user.email="lebocq.tit@gmail.com" \
  commit --no-verify -m "build(lefthook): parallel container setup (Stage 0)

apt-get, cargo-nextest, and rustup MSRV install now run concurrently
via _par/_wait_all. Saves max(setup tasks) - sum(setup tasks) per run
(~10-20s warm)."
```

---

## Task 3: Parallelize conformance (replaces serial `for plugin in ...` loop)

**Files:**
- Modify: `lefthook.yml` — the Stage 6 conformance block.

- [ ] **Step 1: Replace the serial loop**

Current block:

```bash
            # ─── 6. test (conformance) ────────────────────────────
            # CI runs the conformance suite against each HTTP plugin
            # individually via its `--test conformance` integration test.
            echo "::group::test-conformance"
            for plugin in anthropic ollama openai; do
              cargo nextest run -p "$plugin" --test conformance --target-dir $TARGET
            done
            echo "::endgroup::"
```

Replace with:

```bash
            # ─── 6. test (conformance) — parallel ─────────────────
            # Three independent test binaries (--test conformance per
            # plugin crate). Build phases queue briefly on the cargo
            # lock; test executions overlap.
            echo "::group::conformance-launch"
            T_CONF_A=$(_par conformance-anthropic cargo nextest run -p anthropic --test conformance --target-dir $TARGET)
            T_CONF_O=$(_par conformance-ollama    cargo nextest run -p ollama    --test conformance --target-dir $TARGET)
            T_CONF_P=$(_par conformance-openai    cargo nextest run -p openai    --test conformance --target-dir $TARGET)
            echo "::endgroup::"
            _wait_all "$T_CONF_A" "$T_CONF_O" "$T_CONF_P"
```

- [ ] **Step 2: Commit**

```bash
git add lefthook.yml
git -c user.name="titouanlebocq" -c user.email="lebocq.tit@gmail.com" \
  commit --no-verify -m "build(lefthook): parallel conformance tests (Stage 6)

3 plugins x cargo nextest run --test conformance now run concurrently.
Build phases queue on cargo lock; executions overlap. ~15-30s warm savings."
```

---

## Task 4: Parallelize e2e + relocate xtask-plugin-images

**Files:**
- Modify: `lefthook.yml` — Stages 7, 8, 9, 10 are restructured. xtask runs
  immediately after Stage 6 conformance and immediately before the parallel
  e2e block, because tau-plugin-compat consumes the docker images xtask
  builds.

- [ ] **Step 1: Delete the current Stages 7, 8, 9, 10 blocks**

Locate (in lefthook.yml, currently) and delete:

```bash
            # ─── 7. test (tau-runtime e2e) ────────────────────────
            echo "::group::test-tau-runtime-e2e"
            cargo nextest run -p tau-runtime --features integration-tests --tests --target-dir $TARGET
            echo "::endgroup::"

            # ─── 8. test (tau-sandbox-native e2e) ─────────────────
            echo "::group::test-tau-sandbox-native-e2e"
            cargo nextest run -p tau-sandbox-native --features integration-tests --tests --target-dir $TARGET
            echo "::endgroup::"

            # ─── 9. xtask-plugin-images (DooD) ────────────────────
            # The container has /var/run/podman.sock bind-mounted; xtask
            # uses TAU_CONTAINER_RUNTIME=podman + the bind-mounted socket
            # for nested container spawn. `--target-dir` is a cargo
            # option, must precede `--`.
            echo "::group::xtask-plugin-images"
            cargo run --target-dir $TARGET -p xtask -- build-plugin-images
            echo "::endgroup::"

            # ─── 10. test (tau-plugin-compat / linux) ─────────────
            echo "::group::test-plugin-compat"
            cargo nextest run -p tau-plugin-compat --features integration-tests --tests --target-dir $TARGET
            echo "::endgroup::"
```

- [ ] **Step 2: Replace with reordered xtask + parallel e2e block**

Insert in the same location:

```bash
            # ─── 7. xtask-plugin-images (DooD) ────────────────────
            # MUST run before the parallel e2e block because
            # test-plugin-compat consumes images built by xtask.
            # The container has /var/run/podman.sock bind-mounted; xtask
            # uses TAU_CONTAINER_RUNTIME=podman + the bind-mounted socket
            # for nested container spawn. `--target-dir` is a cargo
            # option, must precede `--`.
            echo "::group::xtask-plugin-images"
            cargo run --target-dir $TARGET -p xtask -- build-plugin-images
            echo "::endgroup::"

            # ─── 8. e2e tests — parallel ──────────────────────────
            # Three independent crate test runs with --features
            # integration-tests. Build phases queue briefly on the
            # cargo lock; test executions overlap.
            echo "::group::e2e-launch"
            T_E2E_RT=$(_par e2e-tau-runtime         cargo nextest run -p tau-runtime         --features integration-tests --tests --target-dir $TARGET)
            T_E2E_SB=$(_par e2e-tau-sandbox-native  cargo nextest run -p tau-sandbox-native  --features integration-tests --tests --target-dir $TARGET)
            T_E2E_PC=$(_par e2e-tau-plugin-compat   cargo nextest run -p tau-plugin-compat   --features integration-tests --tests --target-dir $TARGET)
            echo "::endgroup::"
            _wait_all "$T_E2E_RT" "$T_E2E_SB" "$T_E2E_PC"
```

- [ ] **Step 3: Update the top-of-file coverage comment**

The doc comment at the top of the `deep-gate` command lists 10 numbered
jobs. Update it so the listed ordering matches the new flow. Find:

```yaml
      # Coverage — every Linux CI job is reproduced inside this single
      # container so local pre-push gives the same answer as remote CI:
      #   1. msrv-check / linux        — cargo check at MSRV (rust 1.91)
      #   2. test-fixtures-ports/linux — cargo nextest -p tau-ports --features test-fixtures
      #   3. feature-flag-matrix/linux — cargo check per-crate --no-default-features
      #   4. build-fixtures / linux    — release binaries (consumed by jobs 6, 7, 8, 10)
      #   5. test-stable / linux       — cargo nextest --workspace --all-targets + doctests
      #   6. test (conformance)        — plugin conformance suite
      #   7. test (tau-runtime e2e)    — runtime e2e tests
      #   8. test (sandbox-native e2e) — sandbox-native e2e tests
      #   9. xtask-plugin-images       — DooD-built per-plugin Docker images
      #  10. test (tau-plugin-compat)  — integration-tests feature
```

Replace with:

```yaml
      # Coverage — every Linux CI job is reproduced inside this single
      # container so local pre-push gives the same answer as remote CI.
      # Stages marked PARALLEL run concurrently via _par/_wait_all.
      #   0. setup                     — PARALLEL: apt-deps | nextest | rustup-1.91
      #   1. msrv-check / linux        — cargo check at MSRV (rust 1.91)
      #   2. test-fixtures-ports/linux — cargo nextest -p tau-ports --features test-fixtures
      #   3. feature-flag-matrix/linux — cargo check per-crate --no-default-features
      #   4. build-fixtures / linux    — release binaries (consumed by stages 6, 7, 8)
      #   5. test-stable / linux       — cargo nextest --workspace --all-targets + doctests
      #   6. test (conformance)        — PARALLEL: anthropic | ollama | openai
      #   7. xtask-plugin-images       — DooD-built per-plugin Docker images
      #   8. e2e                       — PARALLEL: tau-runtime | tau-sandbox-native | tau-plugin-compat
```

- [ ] **Step 4: Commit**

```bash
git add lefthook.yml
git -c user.name="titouanlebocq" -c user.email="lebocq.tit@gmail.com" \
  commit --no-verify -m "build(lefthook): parallel e2e + reorder xtask before plugin-compat

The 3 e2e test invocations (tau-runtime, tau-sandbox-native, tau-plugin-compat)
now run concurrently. xtask-plugin-images moves immediately before the parallel
e2e block since plugin-compat consumes images xtask builds. ~30-60s warm savings."
```

---

## Task 5: Local verification

**Files:**
- None modified.

- [ ] **Step 1: Confirm a Podman base or warm cache exists**

```bash
podman volume ls | grep -E 'cargo-cache|target-cache' || echo "cold start: volumes will be created"
podman ps -a | grep tau-lefthook  || true   # leftover containers
```

If a zombie container is around from a prior failed gate, remove it:
`podman rm -f <id>`.

- [ ] **Step 2: Run the full gate against the worktree HEAD**

```bash
time lefthook run pre-push
```

Expected:
- Exit code 0.
- Output contains `::group::setup-launch`, `::group::apt-deps`,
  `::group::nextest-install`, `::group::rustup-1.91` (the parallel-block
  group markers).
- Output contains `::group::conformance-anthropic`, `::group::conformance-ollama`,
  `::group::conformance-openai`.
- Output contains `::group::e2e-tau-runtime`, `::group::e2e-tau-sandbox-native`,
  `::group::e2e-tau-plugin-compat`.
- Each parallel group's content is contiguous (no interleaved lines from
  sibling groups).
- Wall-clock total faster than the pre-change baseline. Record the time.

- [ ] **Step 3: Force-fail one parallel stage to verify error path**

```bash
# Inject a deliberate failure into a conformance test, e.g.:
git stash -u
# Temporarily edit a conformance test to panic, OR use a non-existent
# plugin name in the cargo nextest call inside the heredoc (faster).
# Easiest: open lefthook.yml, change `cargo nextest run -p anthropic ...`
# to `cargo nextest run -p definitely-not-a-crate ...`, save, do NOT commit.

lefthook run pre-push
```

Expected:
- Exit code non-zero.
- `::group::conformance-anthropic` log shows the cargo error.
- Both other conformance groups (`-ollama`, `-openai`) still ran to
  completion and their logs are printed.
- Subsequent stages do NOT run (set -e aborts after `_wait_all` returns 1).

Restore: `git checkout lefthook.yml`.

- [ ] **Step 4: Commit nothing in this task**

Verification only. If a problem surfaces, fix it in the earlier task's
file and amend that commit with `git commit --fixup` or a follow-up
commit. Do NOT amend a parent commit with `--amend` (per CLAUDE.md).

---

## Task 6: Open PR

**Files:**
- Modify: nothing further.

- [ ] **Step 1: Push the branch**

```bash
scripts/agent-push.sh -u origin HEAD
```

Per CLAUDE.md AGENT PUSH RULES, never `git push` bare. `agent-push.sh`
runs `lefthook run pre-push` standalone (already validated in Task 5)
then `git push --no-verify`.

- [ ] **Step 2: Open the PR**

```bash
gh pr create --title "build(lefthook): parallelize deep-gate setup/conformance/e2e" \
  --body "$(cat <<'EOF'
## Summary
- Parallelize Stage 0 setup: apt-deps, cargo-nextest install, rustup MSRV install all run concurrently
- Parallelize conformance: anthropic/ollama/openai run as 3 concurrent `cargo nextest run --test conformance`
- Parallelize e2e: tau-runtime / tau-sandbox-native / tau-plugin-compat run concurrently
- Move `xtask-plugin-images` before the parallel e2e block (plugin-compat depends on it)
- Adds tiny `_par` / `_wait_all` bash helpers; logs replay grouped in stage order

## Test plan
- [x] `lefthook run pre-push` green on this branch (warm)
- [x] Forced-failure test: parallel-stage failure aborts gate, peer logs still printed
- [ ] CI green

Spec: `docs/superpowers/specs/2026-05-17-lefthook-deep-gate-parallel-design.md`
Plan: `docs/superpowers/plans/2026-05-17-lefthook-deep-gate-parallel.md`

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Self-review notes

- Every spec section is covered: helpers (Task 1), Stage 0 setup (Task 2),
  conformance parallel (Task 3), e2e parallel + xtask reorder (Task 4),
  test plan (Task 5), follow-up PR (Task 6).
- No placeholders. Every step has exact code or exact command.
- Identifier consistency: `_par`, `_wait_all`, `T_APT`, `T_NEXTEST`,
  `T_RUSTUP`, `T_CONF_*`, `T_E2E_*` — labels and var names match between
  task definitions.
- Type/signature consistency: `_par` always emits `pid:label:log`;
  `_wait_all` always consumes that format. Match.
- xtask-plugin-images dependency contract on plugin-compat is preserved
  (xtask runs in new Stage 7, plugin-compat in parallel Stage 8 alongside
  the other e2e tests).
