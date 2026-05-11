# HTTP Transport ↔ Proxy Chain Implementation Plan (Amended post-T0a)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Amended 2026-05-11 (post-T0a):** The original plan tested a falsified hypothesis (HTTPS_PROXY-vs-HTTP_PROXY scheme gating). T0a's findings revealed that **reqwest bypasses proxy env vars for loopback targets by default** — adding HTTP_PROXY is necessary but not sufficient. This amendment switches T0a → T0a' (renewed test: option C `NO_PROXY=""` env addition), with explicit HARD GATE escalation to option D (cassette infra change) if C falsifies. Spec amended at commit `d711d38`.

**Goal:** Close the 3 HTTP layer4 tests (anthropic/ollama/openai) by overriding reqwest's default loopback bypass so the cassette server's `127.0.0.1:<random-port>` URL routes through the bridge → proxy → host chain.

**Architecture:** Single PR (`feat/http-transport-proxy-chain`). T0a' tests option C (NO_PROXY="" env) in Podman. If C passes (renewed hypothesis confirmed) → T0b applies the env fix + un-`#[ignore]`'s 3 tests. If C falsifies → HARD GATE escalates to user (option D investigation tracked separately).

**Tech Stack:** Rust 2021, `tau-sandbox-native::strict::wrap_spawn`, reqwest's NO_PROXY env-var behavior, lefthook + Podman gate for verification, nextest.

**Spec:** `docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md` (amended at `d711d38`).

---

## Pre-flight checks (apply to every task)

- BASE_SHA = `d711d38`. Verify against this if claiming "pre-existing failure".
- All cargo invocations: `timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p <crate>` per CLAUDE.md.
- `RUSTC_WRAPPER=` to clear sccache if EPERM.
- Investigation tasks (T0a') emit findings to the spec's "T0a' — Renewed test" template (currently below the historical T0a findings). NO code commit on T0a'.
- T0c push uses `scripts/agent-push.sh` — NOT plain `git push`.
- Podman gate config (standard):
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
- For nextest install inside Podman, **detect arch**:
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
| `crates/tau-sandbox-native/src/strict.rs:453` (existing `cmd.env("HTTPS_PROXY", ...)`) | The exact site where wrap_spawn sets the child's proxy env. T0a' adds 2 lines locally for test; T0b commits the same 2 lines. | T0a' (local edit + revert), T0b (real commit) |
| `crates/tau-plugin-compat/tests/layer4_native.rs:538, 642, 739` | The 3 `#[ignore]` attributes on the HTTP layer4 tests. T0b removes them. | T0b (modify) |
| `docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md` (T0a' template at bottom) | Spec amendment populated by T0a' with hypothesis-C outcome. | T0a' (populate + commit) |

---

## Task 0a': Renewed hypothesis test — option C (NO_PROXY="")

**HARD GATE.** Spec edit only. NO code commit. Main agent reviews findings before T0b.

**Files:**
- Modify: `docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md` (populate "T0a' — Renewed test" template at the bottom)

- [ ] **Step 1: Apply candidate fix LOCALLY**

Edit `crates/tau-sandbox-native/src/strict.rs` around line 453. Current state:

```rust
        cmd.env("HTTPS_PROXY", "http://127.0.0.1:8443");
```

Replace with (2 additional lines, neither committed):

```rust
        cmd.env("HTTPS_PROXY", "http://127.0.0.1:8443");
        // T0a (2026-05-11): reqwest scheme-gates HTTPS_PROXY (HTTPS-only)
        // vs HTTP_PROXY (HTTP-only). Cassette tests use plain-HTTP URLs.
        cmd.env("HTTP_PROXY", "http://127.0.0.1:8443");
        // T0a' (2026-05-11): explicit-empty NO_PROXY disables reqwest's
        // default loopback bypass. Without this, reqwest sees the
        // cassette's 127.0.0.1:<port> URL and short-circuits the proxy.
        cmd.env("NO_PROXY", "");
```

Both HTTP_PROXY (from T0a) and NO_PROXY (T0a' new) are added together — T0a confirmed HTTP_PROXY is necessary; T0a' tests whether HTTP_PROXY + NO_PROXY="" together are sufficient.

- [ ] **Step 2: Run the 3 HTTP layer4 tests in Podman gate**

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

Capture verbatim nextest summary.

- [ ] **Step 3: Revert local edit**

```bash
cd /Users/titouanlebocq/code/tau
git checkout -- crates/tau-sandbox-native/src/strict.rs
git status  # clean working tree
```

- [ ] **Step 4 (if 3 tests PASS): Populate T0a' template + commit spec edit**

Open `docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md`. Find the "### T0a' — Renewed test (option C: NO_PROXY="") — TEMPLATE" section near the bottom. Replace `[bracketed placeholders]`:

- **Investigator:** subagent (T0a' implementer)
- **Outcome:** verbatim "3 tests run: 3 passed, 0 failed" + the 3 individual PASS lines
- **Confidence assessment:** Hypothesis CONFIRMED — option C works.
- **Decision:** Proceed to T0b. Apply HTTP_PROXY + NO_PROXY="" env additions + un-`#[ignore]` 3 tests.

Commit:

```bash
git status  # confirm only the spec file is staged
git add docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md
git commit --no-verify -m "docs(spec): T0a' renewed test — option C (NO_PROXY=\"\") confirmed

Per spec's T0a' template. T0a' verified locally inside the lefthook
Podman gate: adding NO_PROXY=\"\" (explicit empty) alongside the
T0a-tested HTTPS_PROXY + HTTP_PROXY env vars in wrap_spawn unblocks
all 3 HTTP layer4 tests. Confirms the post-T0a loopback-bypass
diagnosis: reqwest's default loopback exemption is NO_PROXY-driven,
and an explicit-empty NO_PROXY overrides it.

Option C confirmed; T0b applies the 2-line fix + un-#[ignore]
3 tests.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

- [ ] **Step 5 (if 1+ tests FAIL): Populate T0a' template with falsification + escalate**

Replace the template's bracketed placeholders with the falsification outcome. Set "Decision: ESCALATE to user — option C falsified; option D investigation needed."

```bash
git add docs/superpowers/specs/2026-05-11-http-transport-proxy-chain-design.md
git commit --no-verify -m "docs(spec): T0a' renewed test — option C falsified

NO_PROXY=\"\" alongside HTTPS_PROXY + HTTP_PROXY did not unblock the
3 HTTP layer4 tests. reqwest's loopback bypass may be hardcoded
beyond NO_PROXY env override. Option D (cassette-side non-loopback
base_url) needs investigation. Escalated to user.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

Report DONE_WITH_CONCERNS to main agent. DO NOT proceed to T0b.

---

## Task 0b: Apply env fix + un-`#[ignore]` 3 HTTP tests

**Prerequisite:** T0a' confirmed option C in Step 4.

**Files:**
- Modify: `crates/tau-sandbox-native/src/strict.rs:453` area (add HTTP_PROXY + NO_PROXY env lines)
- Modify: `crates/tau-plugin-compat/tests/layer4_native.rs:538, 642, 739` (remove `#[ignore]`)

- [ ] **Step 1: Apply the env additions in wrap_spawn**

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
        // those requests.
        cmd.env("HTTP_PROXY", "http://127.0.0.1:8443");
        // reqwest's default loopback bypass would short-circuit the proxy
        // for any 127.0.0.1 URL (which is what cassette test servers use).
        // Explicit-empty NO_PROXY overrides the bypass so the proxy applies
        // to loopback too. Both env vars alias the same bridge destination,
        // so this doesn't broaden the security envelope.
        cmd.env("NO_PROXY", "");
```

- [ ] **Step 2: Remove `#[ignore]` from 3 HTTP layer4 tests**

Edit `crates/tau-plugin-compat/tests/layer4_native.rs`. Find lines 538, 642, 739 — each is an `#[ignore = "..."]` attribute. Delete those 3 lines entirely. Do NOT touch the `#[tokio::test]` or `async fn` lines below them; do NOT touch test bodies. The shell test at line 246 retains its `#[ignore]`.

After deletion, only line 246 (shell, sub-project E) retains `#[ignore]` in layer4_native.rs.

- [ ] **Step 3: Run unit tests + clippy + fmt**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-sandbox-native --lib 2>&1 | tail -15
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo clippy -p tau-sandbox-native -p tau-plugin-compat --all-targets -- -D warnings 2>&1 | tail -10
timeout 30 cargo fmt --all -- --check 2>&1 | tail -5
```

All clean. If fmt fails, run `cargo fmt --all` (no `--check`). If sccache fails with EPERM, prefix with `RUSTC_WRAPPER=`.

- [ ] **Step 4: Verify in Podman gate**

Run all un-`#[ignore]`'d layer4_native tests PLUS the existing tau-sandbox-native integration tests:

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

# Without --include-ignored: 4 layer4_native tests should pass (fs-read + 3 HTTP), shell stays ignored.
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
- `tau-plugin-compat::layer4_native`: 4 passed, 1 skipped (shell), 0 failed
- `tau-sandbox-native` integration tests: all pass (strict_bridge, strict_proxy, strict_seccomp, etc.)

If any test fails: hard-stop. Do NOT commit.

- [ ] **Step 5: Commit**

```bash
git status  # confirm 2 files modified: strict.rs + layer4_native.rs
git add crates/tau-sandbox-native/src/strict.rs crates/tau-plugin-compat/tests/layer4_native.rs
git commit --no-verify -m "fix(sandbox-native): HTTP_PROXY + NO_PROXY env in wrap_spawn — closes 3 HTTP layer4 tests

Per T0a + T0a' investigations (committed earlier on this branch):

T0a confirmed: reqwest scheme-gates the proxy env vars — HTTPS_PROXY
is consulted only for HTTPS-scheme URLs, HTTP_PROXY only for plain-
HTTP URLs. Without HTTP_PROXY, reqwest bypasses the bridge for plain-
HTTP cassette URLs. But HTTP_PROXY alone is not sufficient.

T0a' confirmed: reqwest's default loopback bypass short-circuits any
configured proxy for 127.0.0.1 URLs. The cassette server runs on the
host's loopback at a random port and the test fixtures pass that URL
to the plugin's base_url config. Setting NO_PROXY=\"\" (explicit empty)
overrides the bypass so the proxy applies to loopback too.

Fix: set both HTTP_PROXY=http://127.0.0.1:8443 AND NO_PROXY=\"\" in
wrap_spawn alongside the existing HTTPS_PROXY. Both env vars alias
the same bridge destination, so this doesn't broaden the security
envelope (the strict-tier seccomp + landlock baseline still gates
all network destinations).

Verified inside the lefthook Podman gate:
- anthropic_layer4_native_completes_via_cassette: PASS
- ollama_layer4_native_completes_via_cassette: PASS
- openai_layer4_native_completes_via_cassette: PASS
- fs_read_layer4_native_reads_data_file: PASS (no regression)
- shell_layer4_native_runs_echo_hello: still #[ignore]'d
  (sub-project E territory; out of scope)
- tau-sandbox-native integration tests all pass (no regression in
  strict_bridge / strict_proxy / strict_seccomp)

Closes the original PR 2 work end-to-end. Today's full progression
across the 3 HTTP layer4 tests:

- Pre-Phase-0: spawn ENOENT (test infra missed bridge binary path)
- Phase 0 (#49): spawn fixed; failure → handshake EOF
- Bridge integration (#51): bridge SIGSYS on bind/listen — fixed
- This PR (T0a): HTTPS_PROXY-vs-HTTP_PROXY scheme gating — fixed
  but exposed loopback bypass
- This PR (T0a'): reqwest loopback bypass via NO_PROXY=\"\" — fixed

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

Expected: 6 commits ahead of main (original spec, plan, T0a falsification, spec amendment, T0a' findings, T0b fix).

- [ ] **Step 2: Push via agent-push.sh**

```bash
scripts/agent-push.sh -u origin feat/http-transport-proxy-chain
```

If the lefthook gate hangs at xtask-plugin-images for >20 min (Podman VM disk-full deadlock), recover per CLAUDE.md AGENT PUSH RULES:
```bash
podman machine stop && podman machine start
git push --no-verify -u origin feat/http-transport-proxy-chain
```

- [ ] **Step 3: Open PR**

```bash
gh pr create --title "fix(sandbox-native): HTTP_PROXY + NO_PROXY in wrap_spawn — closes 3 HTTP layer4 tests" --body "$(cat <<'EOF'
## Summary

Closes the original PR 2 work for the Layer 4 plugin-compat sub-project. After Phase 0 (PR #49) and bridge integration (PR #51), the 3 HTTP layer4 tests still failed at HTTP transport. T0a + T0a' investigations confirmed two compounding root causes:

1. **HTTPS_PROXY-vs-HTTP_PROXY scheme gate** (T0a): reqwest uses HTTPS_PROXY only for HTTPS URLs; HTTP_PROXY only for plain HTTP. Cassette tests use plain HTTP. wrap_spawn was setting HTTPS_PROXY only.
2. **reqwest default loopback bypass** (T0a'): reqwest auto-exempts 127.0.0.1 URLs from any configured proxy. Cassette tests use 127.0.0.1:<random-port>. Explicit-empty NO_PROXY overrides this bypass.

**Fix:** 3 env lines in wrap_spawn (HTTPS_PROXY existing + HTTP_PROXY new + NO_PROXY new). All three alias the same bridge destination inside the netns; security envelope unchanged.

**Verified inside lefthook Podman gate:**
- `anthropic_layer4_native_completes_via_cassette`: PASS
- `ollama_layer4_native_completes_via_cassette`: PASS
- `openai_layer4_native_completes_via_cassette`: PASS
- `fs_read_layer4_native_reads_data_file`: PASS (no regression)
- tau-sandbox-native integration tests: all pass (no regression in strict_bridge / strict_proxy / strict_seccomp)

Shell layer4 test stays `#[ignore]`'d (sub-project E territory — per-command exec gating).

## Today's full progression on these 3 HTTP tests

| Stage | Status | PR |
|---|---|---|
| Pre-Phase-0: spawn ENOENT | ✓ fixed | #49 |
| Phase 0: handshake EOF | ✓ fixed | #49 |
| Bridge SIGSYS on bind/listen | ✓ fixed | #51 |
| reqwest HTTPS-only proxy env | ✓ fixed | THIS PR (T0a) |
| reqwest loopback bypass | ✓ fixed | THIS PR (T0a') |

## Test plan

- [ ] CI green on 14 required checks
- [ ] Diff review: 4-line addition in strict.rs (2 env lines + comments) + 3 #[ignore] deletions

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

- [ ] **Step 4: Optional memory snapshot**

This PR closes a multi-PR thread on the layer4 HTTP tests (#49, #51, #52, this one). Consider writing a session snapshot to `~/.claude/projects/-Users-titouanlebocq-code-tau/memory/` documenting the 5-step progression. Optional; commit history is the canonical record.

---

## Self-review checklist

- [ ] `git log --oneline main..HEAD` shows the expected commits (spec, plan, T0a falsification, spec amendment, T0a' findings, T0b fix)
- [ ] All 14 required CI checks green on PR
- [ ] 3 HTTP layer4 tests un-`#[ignore]`'d and passing in CI
- [ ] fs-read continues passing
- [ ] tau-sandbox-native integration tests continue passing
- [ ] Spec's T0a' findings section filled with concrete data
- [ ] No new public API; extension in existing wrap_spawn only

---

## If T0a' falsifies (option D escalation)

If renewed T0a' falsifies option C, the implementer reports DONE_WITH_CONCERNS and main agent escalates to user. Option D requires its own investigation:

1. **Decide D's mechanism** (cassette returns `.test` synthetic hostname, host's primary IP, `0.0.0.0`, etc.) — needs verification that the chosen mechanism actually routes through the proxy.
2. **Possibly touch** `crates/tau-plugin-test-support/src/cassette.rs:124`, `crates/tau-sandbox-proxy/`, or `crates/tau-sandbox-native/src/bin/tau-net-bridge.rs` depending on mechanism.
3. **Either** extend this PR's scope or close this PR and open a separate one.

That investigation is OUT OF SCOPE for this plan unless C falsifies. If escalated, user decides path forward.
