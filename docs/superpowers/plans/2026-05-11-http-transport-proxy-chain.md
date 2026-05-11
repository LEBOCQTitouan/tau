# HTTP Transport ↔ Proxy Chain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the 3 HTTP layer4 tests (anthropic/ollama/openai) by adding `HTTP_PROXY` env alongside the existing `HTTPS_PROXY` in `tau-sandbox-native::strict::wrap_spawn`. reqwest scheme-gates these env vars; the cassette server is plain HTTP, so HTTPS_PROXY-only configuration causes reqwest to bypass the bridge.

**Architecture:** Single PR (`feat/http-transport-proxy-chain`). T0a verifies the hypothesis by editing locally + re-running the 3 HTTP tests in the lefthook Podman gate. If confirmed (high-confidence hypothesis), T0b applies the one-line fix + un-`#[ignore]`'s the 3 tests. T0c/T0d are USER GATEs (push, monitor CI, merge).

**Tech Stack:** Rust 2021, `tau-sandbox-native::strict::wrap_spawn`, lefthook + Podman gate for verification, nextest.

**Spec:** `docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md` (committed at `ba71bc5`).

---

## Pre-flight checks (apply to every task)

- BASE_SHA = `ba71bc5`. Verify against this if claiming "pre-existing failure".
- All cargo invocations: `timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p <crate>` per CLAUDE.md.
- `RUSTC_WRAPPER=` to clear sccache if EPERM.
- Investigation tasks (T0a) emit findings to the spec's "Investigation findings" template. NO code commit on T0a.
- T0c push uses `scripts/agent-push.sh` (helper from PR #49) — NOT plain `git push`.
- For Podman repro inside T0a / T0b, the lefthook gate config:
  ```
  docker.io/library/rust:1.82-bookworm
  --cap-add SYS_ADMIN --cap-add NET_ADMIN
  --security-opt seccomp=unconfined --security-opt apparmor=unconfined
  --security-opt label=disable
  -v "$PWD:/workspace"
  -v cargo-cache:/usr/local/cargo/registry
  -v target-cache:/workspace/target/lefthook-podman
  -e CARGO_INCREMENTAL=0
  -e CARGO_TARGET_DIR=/workspace/target/lefthook-podman
  -w /workspace
  ```
- For nextest install inside Podman, **detect arch** (lefthook.yml pattern):
  ```bash
  ARCH=$(uname -m)
  case "$ARCH" in
    aarch64) NEXTEST_URL="https://get.nexte.st/latest/linux-arm" ;;
    *)       NEXTEST_URL="https://get.nexte.st/latest/linux" ;;
  esac
  ```

---

## File structure

| File | Responsibility | Task |
|---|---|---|
| `crates/tau-sandbox-native/src/strict.rs:453` (`cmd.env("HTTPS_PROXY", ...)`) | The exact site where the strict-tier `wrap_spawn` sets the child's proxy env. T0b adds one line setting `HTTP_PROXY` to the same destination. | T0a (local edit + revert), T0b (real commit) |
| `crates/tau-plugin-compat/tests/layer4_native.rs:538, 642, 739` | The 3 `#[ignore]` attributes on the HTTP layer4 tests. T0b removes them. | T0b (modify) |
| `docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md` (Investigation findings template at the bottom) | Spec amendment populated by T0a. | T0a (populate + commit) |

---

## Task 0a: Hypothesis verification — HTTP_PROXY env addition unblocks 3 tests

**HARD GATE.** Spec edit only on this task. NO code commit. Main agent reviews findings before T0b.

**Files:**
- Modify: `docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md` (populate "Investigation findings" template)

- [ ] **Step 1: Apply the candidate fix LOCALLY (do NOT commit)**

Edit `crates/tau-sandbox-native/src/strict.rs` line 453. Current line:

```rust
        cmd.env("HTTPS_PROXY", "http://127.0.0.1:8443");
```

Replace with:

```rust
        cmd.env("HTTPS_PROXY", "http://127.0.0.1:8443");
        // T0a (2026-05-11): reqwest scheme-gates HTTPS_PROXY (HTTPS-only)
        // vs HTTP_PROXY (HTTP-only). Cassette tests use plain-HTTP URLs.
        // Both env vars route to the same bridge inside the netns.
        cmd.env("HTTP_PROXY", "http://127.0.0.1:8443");
```

- [ ] **Step 2: Run the 3 HTTP layer4 tests in Podman gate**

Single Podman invocation that builds + runs. Cold cache will take ~10-15 min; warm (from earlier today) is ~2-3 min.

```bash
podman run --rm \
  --cap-add SYS_ADMIN --cap-add NET_ADMIN \
  --security-opt seccomp=unconfined \
  --security-opt apparmor=unconfined \
  --security-opt label=disable \
  -v "$PWD:/workspace" \
  -v cargo-cache:/usr/local/cargo/registry \
  -v target-cache:/workspace/target/lefthook-podman \
  -w /workspace \
  -e CARGO_INCREMENTAL=0 \
  -e CARGO_TARGET_DIR=/workspace/target/lefthook-podman \
  docker.io/library/rust:1.82-bookworm \
  bash -c '
set -ex
apt-get update -qq && apt-get install -y -qq iproute2 nftables
ARCH=$(uname -m)
case "$ARCH" in
  aarch64) NEXTEST_URL="https://get.nexte.st/latest/linux-arm" ;;
  *)       NEXTEST_URL="https://get.nexte.st/latest/linux" ;;
esac
if ! command -v cargo-nextest >/dev/null; then
  curl -LsSf "$NEXTEST_URL" | tar zxf - -C /usr/local/cargo/bin
fi

cargo build --release -p anthropic -p ollama -p openai -p tau-sandbox-native --bin tau-net-bridge
cargo build -p tau-cli --bin tau

mkdir -p target/release
for bin in anthropic-plugin ollama-plugin openai-plugin tau tau-net-bridge; do
  cp -f target/lefthook-podman/release/$bin target/release/$bin 2>/dev/null || true
done

timeout 180 cargo nextest run -p tau-plugin-compat --test layer4_native \
  anthropic_layer4_native_completes_via_cassette \
  ollama_layer4_native_completes_via_cassette \
  openai_layer4_native_completes_via_cassette \
  --features integration-tests \
  --no-fail-fast \
  -- --include-ignored 2>&1 | tail -40
'
```

Capture the FULL output. Look for:
- 3 PASS → hypothesis CONFIRMED. Proceed to Step 3.
- 1-3 FAIL → hypothesis falsified. Skip to Step 6 (escalation).

- [ ] **Step 3 (if confirmed): Revert local edit**

```bash
cd /Users/titouanlebocq/code/tau
git checkout -- crates/tau-sandbox-native/src/strict.rs
git status  # confirm only docs/ changes remain
```

T0b will reintroduce the same edit as the real commit.

- [ ] **Step 4 (if confirmed): Populate spec template**

Open `docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md`. Find the "## Investigation findings" section near the end (template starts `### T0a — HTTP_PROXY hypothesis verification (DATE)`). Replace `[bracketed placeholders]` with concrete data:

- **Date:** today (2026-05-11)
- **Investigator:** subagent or human
- **Environment:** lefthook Podman gate (`docker.io/library/rust:1.82-bookworm`) on darwin-arm64 host
- **Hypothesis tested:** Setting `HTTP_PROXY=http://127.0.0.1:8443` alongside `HTTPS_PROXY` in wrap_spawn's child env will unblock the 3 HTTP layer4 tests
- **Local edit applied:** the exact two-line addition (`cmd.env("HTTP_PROXY", ...)` + comment) at `strict.rs:453`
- **Test command:** the Podman command from Step 2 (paste verbatim)
- **Outcome:** "3 passed, 0 failed, 2 skipped" (or the exact verbatim summary from nextest)
- **Confidence assessment:** hypothesis CONFIRMED — 3 HTTP tests pass with the single-line addition
- **Decision:** proceed to T0b with the proposed fix

- [ ] **Step 5 (if confirmed): Commit spec edit**

```bash
git status  # confirm ONLY the spec file is staged
git add docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md
git commit --no-verify -m "docs(spec): T0a investigation findings — HTTP_PROXY hypothesis confirmed

Per spec's Investigation findings template. T0a verified locally
inside the lefthook Podman gate: adding HTTP_PROXY=http://127.0.0.1:8443
alongside the existing HTTPS_PROXY in wrap_spawn unblocks all 3
HTTP layer4 tests. reqwest scheme-gates the env vars (HTTPS_PROXY
HTTPS-only, HTTP_PROXY HTTP-only); cassette server is plain HTTP.

Hypothesis confirmed. T0b applies the one-line fix.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

- [ ] **Step 6 (if hypothesis falsified): Revert + escalate**

If 1 or more of the 3 HTTP tests still fails after the local edit:

```bash
git checkout -- crates/tau-sandbox-native/src/strict.rs
git status  # clean
```

Populate the spec template with the FAIL outcome + exact failure shape. Set "Decision: escalate to user — hypothesis falsified". Commit the spec edit. Report DONE_WITH_CONCERNS to main agent with the failure shape so main can decide next steps (likely: strace investigation of the proxy chain).

DO NOT proceed to T0b.

---

## Task 0b: Apply env fix + un-`#[ignore]` 3 HTTP tests

**Prerequisite:** T0a confirmed the hypothesis.

**Files:**
- Modify: `crates/tau-sandbox-native/src/strict.rs:453` (add HTTP_PROXY env)
- Modify: `crates/tau-plugin-compat/tests/layer4_native.rs:538, 642, 739` (remove `#[ignore]` from 3 HTTP tests)

- [ ] **Step 1: Apply the env fix in wrap_spawn**

Edit `crates/tau-sandbox-native/src/strict.rs`. Find the existing line at ~453:

```rust
        cmd.env("HTTPS_PROXY", "http://127.0.0.1:8443");
```

Replace with:

```rust
        cmd.env("HTTPS_PROXY", "http://127.0.0.1:8443");
        // reqwest scheme-gates env: HTTPS_PROXY for HTTPS-scheme URLs only,
        // HTTP_PROXY for plain-HTTP URLs only. Plugin cassette tests use
        // plain HTTP; without HTTP_PROXY, reqwest bypasses the bridge for
        // those requests and tries direct TCP inside the empty netns
        // (where nothing is reachable). Both env vars alias the same
        // bridge destination, so this doesn't broaden the security
        // envelope. T0a 2026-05-11.
        cmd.env("HTTP_PROXY", "http://127.0.0.1:8443");
```

- [ ] **Step 2: Remove `#[ignore]` from 3 HTTP layer4 tests**

Edit `crates/tau-plugin-compat/tests/layer4_native.rs`. Find lines 538, 642, 739 — each is an `#[ignore = "Plugin now spawns, handshakes, and dispatches via the strict-tier sandbox cleanly..."]` attribute. Delete ALL THREE LINES (just the `#[ignore]` attribute lines; do NOT touch the `#[tokio::test]` or `async fn` lines below them or the test bodies).

After deletion, only line 246 (shell test, sub-project E territory) retains `#[ignore]` in `layer4_native.rs`.

- [ ] **Step 3: Run unit tests + clippy + fmt**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-sandbox-native --lib 2>&1 | tail -15
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo clippy -p tau-sandbox-native -p tau-plugin-compat --all-targets -- -D warnings 2>&1 | tail -10
timeout 30 cargo fmt --all -- --check 2>&1 | tail -5
```

All clean. If fmt fails, run `cargo fmt --all` (no `--check`).

If sccache fails with EPERM, prefix with `RUSTC_WRAPPER=`.

- [ ] **Step 4: Verify in Podman gate**

Single Podman invocation. Run ALL 4 un-`#[ignore]`'d layer4 native tests (fs-read regression check + 3 HTTP closure) PLUS the existing strict_bridge + strict_proxy + strict_seccomp e2e tests:

```bash
podman run --rm \
  --cap-add SYS_ADMIN --cap-add NET_ADMIN \
  --security-opt seccomp=unconfined \
  --security-opt apparmor=unconfined \
  --security-opt label=disable \
  -v "$PWD:/workspace" \
  -v cargo-cache:/usr/local/cargo/registry \
  -v target-cache:/workspace/target/lefthook-podman \
  -w /workspace \
  -e CARGO_INCREMENTAL=0 \
  -e CARGO_TARGET_DIR=/workspace/target/lefthook-podman \
  docker.io/library/rust:1.82-bookworm \
  bash -c '
set -ex
apt-get update -qq && apt-get install -y -qq iproute2 nftables
ARCH=$(uname -m)
case "$ARCH" in
  aarch64) NEXTEST_URL="https://get.nexte.st/latest/linux-arm" ;;
  *)       NEXTEST_URL="https://get.nexte.st/latest/linux" ;;
esac
if ! command -v cargo-nextest >/dev/null; then
  curl -LsSf "$NEXTEST_URL" | tar zxf - -C /usr/local/cargo/bin
fi

cargo build --release -p anthropic -p ollama -p openai -p fs-read -p tau-sandbox-native --bin tau-net-bridge
cargo build -p tau-cli --bin tau

mkdir -p target/release
for bin in anthropic-plugin ollama-plugin openai-plugin fs-read-plugin tau tau-net-bridge; do
  cp -f target/lefthook-podman/release/$bin target/release/$bin 2>/dev/null || true
done

# Without --include-ignored: 4 should pass (fs-read + 3 HTTP), shell stays ignored.
timeout 180 cargo nextest run -p tau-plugin-compat --test layer4_native \
  --features integration-tests \
  --no-fail-fast 2>&1 | tail -25

# Existing tau-sandbox-native integration tests must continue passing.
timeout 180 cargo nextest run -p tau-sandbox-native \
  --features integration-tests \
  --tests 2>&1 | tail -15
'
```

Expected:
- `tau-plugin-compat::layer4_native`: 4 passed, 1 skipped (shell stays `#[ignore]`'d), 0 failed
- `tau-sandbox-native` integration tests: all pass (strict_bridge, strict_proxy, strict_seccomp, etc.)

If any test fails: hard-stop, investigate, do NOT commit until clean.

- [ ] **Step 5: Commit**

```bash
git status  # confirm 2 files modified: strict.rs + layer4_native.rs
git add crates/tau-sandbox-native/src/strict.rs crates/tau-plugin-compat/tests/layer4_native.rs
git commit --no-verify -m "fix(sandbox-native): HTTP_PROXY env in wrap_spawn — closes 3 HTTP layer4 tests

Per T0a investigation (committed in this branch's prior commit):
reqwest scheme-gates the proxy env vars — HTTPS_PROXY is consulted
only for HTTPS-scheme URLs, HTTP_PROXY only for plain-HTTP URLs.
Plugin cassette tests use plain HTTP (random port, no TLS). Without
HTTP_PROXY set, reqwest bypassed the bridge for cassette requests
and tried direct TCP inside the empty netns — nothing reachable
there, so requests failed with 'error sending request for url'.

Fix: set HTTP_PROXY=http://127.0.0.1:8443 alongside the existing
HTTPS_PROXY in wrap_spawn. Both env vars alias the same bridge
destination, so this doesn't broaden the security envelope.

Verified inside the lefthook Podman gate:
- anthropic_layer4_native_completes_via_cassette: PASS
- ollama_layer4_native_completes_via_cassette: PASS
- openai_layer4_native_completes_via_cassette: PASS
- fs_read_layer4_native_reads_data_file: PASS (no regression)
- shell_layer4_native_runs_echo_hello: still #[ignore]'d
  (sub-project E territory; out of scope)
- tau-sandbox-native integration tests all pass (no regression
  in strict_bridge / strict_proxy / strict_seccomp)

Closes the original PR 2 work end-to-end. Today's progression:
- Pre-Phase-0: spawn ENOENT
- Phase 0 (#49): spawn + stdio fixed → handshake EOF
- Bridge integration (#51): seccomp bind/listen → reqwest transport error
- This PR: HTTP_PROXY env → all 3 HTTP layer4 tests pass

Spec: docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 0c: USER GATE — push via agent-push.sh, open PR, monitor CI

**Main agent only — no subagent.**

- [ ] **Step 1: Verify branch state**

```bash
git status  # clean working tree
git log --oneline main..HEAD
```

Expected: 3 commits ahead of main (spec, T0a findings, T0b fix).

- [ ] **Step 2: Push via the helper**

```bash
scripts/agent-push.sh -u origin feat/http-transport-proxy-chain
```

If the lefthook gate hangs at xtask-plugin-images for >20 min, the Podman VM may be in disk-full deadlock. Recovery (per CLAUDE.md AGENT PUSH RULES):
```bash
podman machine stop && podman machine start
git push --no-verify -u origin feat/http-transport-proxy-chain
```

- [ ] **Step 3: Open PR**

```bash
gh pr create --title "fix(sandbox-native): HTTP_PROXY env in wrap_spawn — closes 3 HTTP layer4 tests" --body "$(cat <<'EOF'
## Summary

Closes the original PR 2 work for the Layer 4 plugin-compat sub-project. After Phase 0 (PR #49 — spawn + stdio) and bridge integration (PR #51 — seccomp bind/listen + bridge survival), the 3 HTTP layer4 tests still failed with reqwest "error sending request for url". T0a's hypothesis-first investigation confirmed the root cause: `wrap_spawn` set HTTPS_PROXY but not HTTP_PROXY. reqwest scheme-gates these env vars (HTTPS_PROXY for HTTPS-scheme URLs only; HTTP_PROXY for plain-HTTP URLs only). Cassette servers are plain HTTP, so reqwest bypassed the bridge entirely.

**Fix:** one line in `tau-sandbox-native::strict::wrap_spawn` adding `HTTP_PROXY=http://127.0.0.1:8443` alongside the existing HTTPS_PROXY. Both env vars alias the same bridge destination — no broadening of the security envelope.

**Verified inside lefthook Podman gate:**
- `anthropic_layer4_native_completes_via_cassette`: PASS
- `ollama_layer4_native_completes_via_cassette`: PASS
- `openai_layer4_native_completes_via_cassette`: PASS
- `fs_read_layer4_native_reads_data_file`: PASS (no regression)
- `tau-sandbox-native` integration tests: all pass (no regression in strict_bridge / strict_proxy / strict_seccomp)

The shell layer4 test stays `#[ignore]`'d (sub-project E territory — per-command exec gating).

## Today's full progression on these 3 HTTP tests

| Stage | Status | PR |
|---|---|---|
| Pre-Phase-0: spawn ENOENT (test infra missed bridge binary path) | ✓ fixed | #49 |
| Handshake EOF (bridge ran but stdio dropped by Command rebuild) | ✓ fixed | #49 |
| Bridge SIGSYS on bind/listen | ✓ fixed | #51 |
| reqwest "error sending request" (HTTPS_PROXY-only) | ✓ fixed | THIS PR |

## Test plan

- [ ] CI green on the 14 required checks (especially `test (tau-plugin-compat / linux)`)
- [ ] Diff review: 2-line addition in strict.rs + 3 `#[ignore]` line deletions
- [ ] T0a investigation findings in spec are concrete (hypothesis explicitly confirmed)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Monitor CI**

```bash
prev=""
while true; do
  s=$(gh pr checks <PR#> --json name,state 2>/dev/null) || { echo "gh-error"; sleep 30; continue; }
  cur=$(jq -r '.[] | select(.state!="PENDING" and .state!="QUEUED" and .state!="IN_PROGRESS") | "\(.name): \(.state)"' <<<"$s" | sort)
  comm -13 <(echo "$prev") <(echo "$cur")
  prev=$cur
  jq -e 'length>0 and (all(.state!="PENDING" and .state!="QUEUED" and .state!="IN_PROGRESS"))' <<<"$s" >/dev/null && { echo "ALL CHECKS COMPLETE"; break; }
  sleep 30
done
```

Pause for user approval before T0d.

---

## Task 0d: USER GATE — squash-merge

**Main agent only — no subagent.**

- [ ] **Step 1: Verify all 14 required checks green**

```bash
gh pr checks <PR#> --json name,state | jq '[.[] | select(.state != "SUCCESS")] | length'
```

Expected: `0`.

- [ ] **Step 2: Squash-merge**

```bash
gh pr merge <PR#> --squash --delete-branch
```

- [ ] **Step 3: Sync main**

```bash
git checkout main
git pull --ff-only
git log --oneline -3
```

Expected: top commit is the squash-merged PR.

- [ ] **Step 4: Optional memory update**

This PR closes a 12+ hour multi-PR thread (#49, #51, #52, this one). Consider writing a session snapshot to `~/.claude/projects/-Users-titouanlebocq-code-tau/memory/` capturing the 4-step progression for future-self context. Optional; the commit history is the canonical record.

---

## Self-review checklist

After all tasks complete, verify:

- [ ] `git log --oneline main..HEAD` shows 3 commits (spec, T0a findings, T0b fix)
- [ ] All 14 required CI checks green on the PR
- [ ] 3 HTTP layer4 tests un-`#[ignore]`'d and passing in CI
- [ ] fs-read continues passing (no regression)
- [ ] tau-sandbox-native integration tests continue passing (strict_bridge, strict_proxy)
- [ ] Spec's "Investigation findings" section is filled with concrete data
- [ ] No new public API (extension in existing `wrap_spawn` only)
