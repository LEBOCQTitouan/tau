# Layer 4 LLM-Backend Spawn Fix (Phase 0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unblock PR 2 of the Layer 4 plugin-compat startup-IO sub-project by fixing the LLM-backend spawn path. Currently all 3 HTTP plugins (anthropic, ollama, openai) fail at `spawn_llm_under_sandbox` with `Os { code: 2 }` (ENOENT) within 4 ms — before reaching the startup-IO surface T7 was scoped to investigate. Phase 0 ships the spawn fix + agent-push helper as one PR; T7' (renumbered T7) becomes unblocked after merge.

**Architecture:** Single PR `feat/layer4-llm-spawn-fix` cut from main at `f9c2822`. Phase 0 is investigation-then-fix-then-infra: read `load_tool` vs `load_llm_backend` side-by-side, find the divergence, ship the minimal correctness fix to `tau-runtime::plugin_host`, fold in the agent-push.sh helper + CLAUDE.md AGENT PUSH RULES drafted in stash during the silent-kill diagnostic that enabled this Phase 0.

**Tech Stack:** Rust 2021, `tau-runtime::plugin_host` (out-of-process plugin loader), `tau-plugin-compat` (Layer 4 driver + tests), nextest for test execution, lefthook + Podman for verification, bash + git for the agent-push helper.

**Spec:** `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md` Phase 0 section (committed at `129f05b` on this branch).

---

## Pre-flight checks (apply to every task)

- BASE_SHA = `129f05b`. If a test is failing, verify it failed at this SHA before claiming "pre-existing failure".
- All cargo invocations use `timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p <crate>` (subagent) or `target/main` (main agent). Per CLAUDE.md.
- If sccache fails with EPERM, prefix with `RUSTC_WRAPPER=` to clear it.
- Investigation tasks (T0a) emit findings to the spec's "Investigation findings (Phase 0)" template subsection. NO code commit — spec edit only.
- For T0d push, use `scripts/agent-push.sh` (which lands in T0c) — NOT plain `git push`. Avoids the silent-kill issue documented in CLAUDE.md AGENT PUSH RULES.
- For Podman-based verification inside T0a / T0b, use the same image + cap config the lefthook gate uses:
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
  Bug from 2026-05-09: forgetting this fetched x86_64 binary on arm64 container, rosetta-translated, dyld error: `failed to open elf at /lib64/ld-linux-x86-64.so.2`. Use the arch-aware variant.

---

## File structure

| File | Responsibility | Task |
|---|---|---|
| `crates/tau-runtime/src/plugin_host/mod.rs` (line 343 `load_llm_backend`, line 419 `load_tool`, line 515 `load_storage`) | Three port-specific load functions sharing `process::PluginProcess::spawn_and_handshake`. Diff target. | T0a (read), T0b (likely fix) |
| `crates/tau-runtime/src/plugin_host/process.rs` (842 LOC) | Hosts `PluginProcess::spawn_and_handshake`. Tokio Command + sandbox wrap_spawn integration. Likely fix site if the divergence is in spawn-mechanics. | T0a (read), T0b (likely fix) |
| `crates/tau-plugin-compat/tests/layer4_native.rs` lines 98-121 (`make_locked_plugin` + `make_llm_locked_plugin`) | Test helpers constructing `LockedPlugin`. Differ in `PortKind` only. | T0a (read), T0b (no change unless T0a finds the divergence here) |
| `crates/tau-plugin-compat/tests/layer4_native.rs` lines 459, 549, 639 | The 3 HTTP test `#[ignore]` strings. Updated in T0b to reflect post-fix failure shape. | T0b (modify) |
| `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md` lines 269-end ("Phase 0 …" + "Investigation findings (Phase 0)" template at the bottom) | Spec amendment with template for T0a output. | T0a (populate template) |
| `scripts/agent-push.sh` (NEW from stash) | `lefthook run pre-push` standalone, then `git push --no-verify`. Bypasses agent-runtime silent-kill. | T0c |
| `CLAUDE.md` lines 99-end (NEW section "AGENT PUSH RULES") | Documents the silent-kill issue + the helper script. | T0c |

---

## Task 0a: Investigation — diff `load_tool` vs `load_llm_backend`, identify ENOENT root cause

**HARD GATE.** Spec edit only. NO code commit on this task. Main agent reviews the populated findings before T0b dispatches.

**Files:**
- Modify: `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md` (populate "Investigation findings (Phase 0)" template at the bottom)

- [ ] **Step 1: Read `load_tool` and `load_llm_backend` side-by-side**

```bash
sed -n '343,396p' crates/tau-runtime/src/plugin_host/mod.rs > /tmp/load_llm_backend.snippet
sed -n '419,472p' crates/tau-runtime/src/plugin_host/mod.rs > /tmp/load_tool.snippet
diff -u /tmp/load_tool.snippet /tmp/load_llm_backend.snippet | head -100
```

Expected: a diff that shows the two functions have similar structure but differ in (a) the `PortKind` argument passed to `handshake::drive_handshake`, (b) the required wire methods array (`["llm.complete"]` vs `["tool.call"]`), and (c) the post-spawn extra step in `load_tool` that issues a `tool.describe` RPC. Note any other divergences.

- [ ] **Step 2: Read `PluginProcess::spawn_and_handshake` in `process.rs` for any PortKind-dependent code path**

```bash
grep -n "PortKind\|spawn_and_handshake\|fn spawn\|binary_path\|exec\|execve" \
  crates/tau-runtime/src/plugin_host/process.rs | head -30
```

Look for: code paths that branch on the manifest's `PortKind`, OR pre-spawn validation that checks the binary path. The function is 842 LOC; focus on `spawn_and_handshake` itself.

- [ ] **Step 3: Read the test helpers `make_locked_plugin` vs `make_llm_locked_plugin`**

```bash
sed -n '95,125p' crates/tau-plugin-compat/tests/layer4_native.rs
```

Expected output (from a 2026-05-09 read):

```rust
fn make_locked_plugin(bin_name: &str, binary_path: PathBuf) -> LockedPlugin {
    let manifest = PluginManifest::new(PortKind::Tool, PluginKind::RustCargo, bin_name.to_string());
    LockedPlugin::new(manifest, binary_path, std::time::SystemTime::UNIX_EPOCH, String::new())
}

fn make_llm_locked_plugin(bin_name: &str, binary_path: PathBuf) -> LockedPlugin {
    let manifest = PluginManifest::new(PortKind::LlmBackend, PluginKind::RustCargo, bin_name.to_string());
    LockedPlugin::new(manifest, binary_path, std::time::SystemTime::UNIX_EPOCH, String::new())
}
```

Difference: the `PortKind` arg only. Same `binary_path` is passed positionally. So if `binary_path` resolves correctly for tool tests, it should resolve correctly for LLM tests.

- [ ] **Step 4: Reproduce the ENOENT inside the lefthook Podman gate**

Build all 3 HTTP plugins + tau-cli + run all 3 ignored tests in one Podman invocation. Use the env config from "Pre-flight checks" above:

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
apt-get update -qq && apt-get install -y -qq iproute2 nftables strace
ARCH=$(uname -m)
case "$ARCH" in
  aarch64) NEXTEST_URL="https://get.nexte.st/latest/linux-arm" ;;
  *)       NEXTEST_URL="https://get.nexte.st/latest/linux" ;;
esac
rm -f /usr/local/cargo/bin/cargo-nextest
curl -LsSf "$NEXTEST_URL" | tar zxf - -C /usr/local/cargo/bin

cargo build --release -p anthropic -p ollama -p openai -p fs-read
cargo build -p tau-cli --bin tau

mkdir -p target/release
for bin in anthropic-plugin ollama-plugin openai-plugin fs-read-plugin tau; do
  cp -f target/lefthook-podman/release/$bin target/release/$bin 2>/dev/null || true
done

# Run anthropic test with --include-ignored to trigger the ENOENT.
timeout 60 cargo nextest run -p tau-plugin-compat --test layer4_native \
  anthropic_layer4_native_completes_via_cassette \
  --features integration-tests \
  -- --include-ignored 2>&1 | tail -40
'
```

Expected: `spawn anthropic-plugin under native adapter failed: LoadFailed("PluginSpawnFailed { plugin: \"anthropic-plugin\", source: Os { code: 2, kind: NotFound, ... } }")`. Capture the full stderr/stdout for the findings.

- [ ] **Step 5: Add diagnostic logging to identify exact ENOENT source**

Edit `crates/tau-runtime/src/plugin_host/process.rs` LOCALLY (do NOT commit yet) and add `tracing::error!` lines around the spawn point. Find the spawn site:

```bash
grep -n "Command::new\|tokio::process::Command\|cmd.spawn\|\.spawn()" \
  crates/tau-runtime/src/plugin_host/process.rs
```

For each `Command::new` and `.spawn()` site, add `tracing::error!("PHASE0_DEBUG: about to {action}: binary_path={:?}, exists={}, cwd={:?}", binary_path, binary_path.exists(), std::env::current_dir());` to localize the failure.

Re-run the same Podman command from Step 4 with `RUST_LOG=tau_runtime=error` env added to the bash -c block:
```bash
-e RUST_LOG=tau_runtime::plugin_host=error
```

Then re-run with `--nocapture`:
```bash
timeout 60 cargo nextest run -p tau-plugin-compat --test layer4_native \
  anthropic_layer4_native_completes_via_cassette \
  --features integration-tests \
  --nocapture \
  -- --include-ignored 2>&1 | grep "PHASE0_DEBUG\|Os {" | head -20
```

The `PHASE0_DEBUG` lines reveal exactly which path is being spawned and what its `exists()` returns.

- [ ] **Step 6: Revert the diagnostic logging**

```bash
git checkout -- crates/tau-runtime/src/plugin_host/process.rs
```

T0b will reintroduce real changes (not these diagnostic prints) based on findings.

- [ ] **Step 7: Append findings to spec**

Open `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md`. Find the "Investigation findings (Phase 0)" section near the end with the template that starts with `### T0a — load_tool vs load_llm_backend diff (DATE)`. Replace the template's bracketed placeholders with the actual data from Steps 1–5:

Required fields to populate:
- **Date** + **Investigator**
- **Environment** (lefthook Podman gate, host arch)
- **Reproduction**: the exact command from Step 4
- **Diff observed**: the side-by-side bullets from Steps 1+3 (PortKind difference, required-methods difference, post-spawn `tool.describe` extra in load_tool, anything found in process.rs)
- **Root cause**: the specific code path producing ENOENT, identified via the `PHASE0_DEBUG` traces from Step 5
- **Fix scope**: which file(s), what change is the minimal correctness fix (e.g. "add foo missing here" or "binary_path needs canonicalization in load_llm_backend's path because Y")
- **Outcome**: with the fix applied locally, the 3 HTTP tests fail at handshake-EOF (not spawn-ENOENT); fs-read still passes; existing `tau-runtime` lib tests still pass

If the root cause turns out to require a `tau-runtime` API change beyond a small local edit, **STOP** and escalate to the user via DONE_WITH_CONCERNS — do not commit the spec edit.

- [ ] **Step 8: Commit the spec edit**

```bash
git status  # confirm only the spec file is staged
git add docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md
git commit -m "docs(spec): T0a Phase 0 investigation findings — LLM-backend spawn diff

Per spec's Phase 0 Investigation findings template. Documents the
exact divergence between load_tool and load_llm_backend that produces
spawn-ENOENT for HTTP plugins, the root cause identified through
diagnostic tracing inside the lefthook Podman gate, and the fix
scope T0b will implement.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

**HARD GATE:** Main agent reviews findings before T0b. If the findings reveal that the fix is invasive (requires `tau-runtime` API change, breaks plugin_host callers), escalate to user.

---

## Task 0b: Apply spawn fix; verify 3 HTTP tests reach handshake-EOF stage

**Files:**
- Modify: based on T0a findings — most likely `crates/tau-runtime/src/plugin_host/{mod.rs,process.rs}` (one or both)
- Modify: `crates/tau-plugin-compat/tests/layer4_native.rs` lines 459, 549, 639 (update `#[ignore]` messages)

- [ ] **Step 1: Apply the fix per T0a's "Fix scope"**

The exact code change depends on T0a's findings. Three likely shapes (T0a will identify which):

Shape A: Missing field/branch in `load_llm_backend`. Apply the symmetric branch to mirror what `load_tool` does. Example:

```rust
// In load_llm_backend, mirror what load_tool does for the missing piece
// (T0a will identify what specifically — e.g. an absolute-path resolve,
// a missing arg to spawn_and_handshake, or a config validation step).
```

Shape B: PortKind-dependent code path in `spawn_and_handshake` (process.rs). Apply the fix to handle LlmBackend symmetrically.

Shape C: Test helper bug in `make_llm_locked_plugin` (less likely since helper is symmetric to tool variant, but possible). Apply fix in tests/layer4_native.rs.

Whatever shape T0a identifies, the change must be **minimal**. If the fix grows beyond ~30 LOC across 1-2 files, escalate via DONE_WITH_CONCERNS — that signals a deeper architectural issue.

- [ ] **Step 2: Run the existing tau-runtime unit tests**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-runtime --lib 2>&1 | tail -30
```

Expected: all existing `tau-runtime` lib tests pass. No regression.

If sccache fails with EPERM:
```bash
timeout 300 env CARGO_INCREMENTAL=0 RUSTC_WRAPPER= CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-runtime --lib 2>&1 | tail -30
```

- [ ] **Step 3: Run clippy + fmt**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo clippy -p tau-runtime --all-targets -- -D warnings 2>&1 | tail -10
timeout 30 cargo fmt -p tau-runtime -- --check 2>&1 | tail -5
```

Both clean. If fmt fails, run `cargo fmt -p tau-runtime` (no `--check`) to fix.

- [ ] **Step 4: Verify the fix inside the lefthook Podman gate**

Use the same Podman config as T0a Step 4. Run all 3 HTTP tests + fs-read with `--include-ignored`:

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

cargo build --release -p anthropic -p ollama -p openai -p fs-read
cargo build -p tau-cli --bin tau

mkdir -p target/release
for bin in anthropic-plugin ollama-plugin openai-plugin fs-read-plugin tau; do
  cp -f target/lefthook-podman/release/$bin target/release/$bin 2>/dev/null || true
done

# fs-read: must still PASS (PR 1 win not regressed).
timeout 120 cargo nextest run -p tau-plugin-compat --test layer4_native \
  fs_read_layer4_native_reads_data_file \
  --features integration-tests 2>&1 | tail -15

# 3 HTTP: must now fail at handshake-EOF (not spawn-ENOENT).
timeout 180 cargo nextest run -p tau-plugin-compat --test layer4_native \
  anthropic_layer4_native_completes_via_cassette \
  ollama_layer4_native_completes_via_cassette \
  openai_layer4_native_completes_via_cassette \
  --features integration-tests \
  --no-fail-fast \
  -- --include-ignored 2>&1 | tail -40
'
```

Expected:
- `fs_read_layer4_native_reads_data_file`: 1 passed; 0 failed.
- 3 HTTP tests: 0 passed; 3 failed. Each failure is `PluginHandshakeFailed: EOF before handshake response` or similar — **NOT** `Os { code: 2 }` (ENOENT).

If a HTTP test still fails with ENOENT: T0a's diagnosis was incomplete. Re-investigate.

If `fs_read_layer4_native_reads_data_file` regresses (fails): the fix broke the tool path. Revert and re-investigate.

- [ ] **Step 5: Update the 3 HTTP `#[ignore]` messages**

Edit `crates/tau-plugin-compat/tests/layer4_native.rs`. Find the three `#[ignore]` lines at line 459 (anthropic), 549 (ollama), 639 (openai). Replace each with the post-fix message.

For anthropic at line 459, replace:
```rust
#[ignore = "Plugin EOFs before handshake under strict tier — anthropic-plugin's HTTP client init touches state outside plan's read paths. Defer to a sub-project D follow-up that builds plugin-specific plans, or sub-project F."]
```

With:
```rust
#[ignore = "Spawn fixed in Phase 0 (PR feat/layer4-llm-spawn-fix); plugin now reaches handshake but EOFs there because reqwest TLS init touches paths beyond BASELINE_SYSTEM_READ_PATHS. Awaits T7' (renumbered T7) HTTP startup-IO investigation per docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md."]
```

For ollama at line 549, replace the "Plugin EOFs before handshake under strict tier — ollama-plugin's HTTP client init touches state outside plan's read paths. Defer to a sub-project D follow-up that builds plugin-specific plans, or sub-project F." with the equivalent (substituting `ollama-plugin`):

```rust
#[ignore = "Spawn fixed in Phase 0 (PR feat/layer4-llm-spawn-fix); plugin now reaches handshake but EOFs there because reqwest TLS init touches paths beyond BASELINE_SYSTEM_READ_PATHS. Awaits T7' (renumbered T7) HTTP startup-IO investigation per docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md."]
```

For openai at line 639, same shape:

```rust
#[ignore = "Spawn fixed in Phase 0 (PR feat/layer4-llm-spawn-fix); plugin now reaches handshake but EOFs there because reqwest TLS init touches paths beyond BASELINE_SYSTEM_READ_PATHS. Awaits T7' (renumbered T7) HTTP startup-IO investigation per docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md."]
```

Tests stay `#[ignore]`'d. Test bodies unchanged.

- [ ] **Step 6: Compile-check the test changes**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo build --tests -p tau-plugin-compat 2>&1 | tail -10
```

Expected: clean compile.

- [ ] **Step 7: Commit**

```bash
git status  # confirm only the relevant files (mod.rs/process.rs and layer4_native.rs)
git add crates/tau-runtime/src/plugin_host/ crates/tau-plugin-compat/tests/layer4_native.rs
git commit -m "fix(runtime): T0b LLM-backend spawn path — 3 HTTP tests reach handshake

Per T0a findings (committed in this branch's prior commit):
[paraphrase the root cause from the spec's investigation findings —
e.g. 'load_llm_backend was missing X that load_tool does, producing
ENOENT before exec'.]

Verified inside the lefthook Podman gate:
- fs-read test continues to pass (PR 1 win not regressed).
- 3 HTTP tests now fail at handshake-EOF (not spawn-ENOENT).
- All existing tau-runtime lib tests still pass.

The 3 HTTP tests stay #[ignore]'d. Their messages are updated to
reflect the post-fix failure shape and point to T7' (renumbered T7)
which will close them after the startup-IO investigation lands.

Phase 0 of the Layer 4 plugin-compat startup-IO sub-project (spec at
docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

If lefthook pre-commit fails for environmental reasons (Homebrew rust shadowing rustup), use `git commit --no-verify`. Documented escape hatch in user's memory.

---

## Task 0c: Agent infra — pop stash, verify, commit

**Files:**
- Create: `scripts/agent-push.sh` (from `stash@{0}`)
- Modify: `CLAUDE.md` (append "AGENT PUSH RULES" section from `stash@{0}`)

- [ ] **Step 1: Pop the stash**

```bash
git stash list
# Expected: stash@{0}: On feat/layer4-startup-io-baseline: agent-push fix (2026-05-09)

git stash pop stash@{0}
```

If pop produces conflicts: the stash was created on `feat/layer4-startup-io-baseline` (now merged via PR #48). It should pop cleanly onto this branch since neither `scripts/agent-push.sh` (untracked file) nor the CLAUDE.md trailing section conflict with anything on `feat/layer4-llm-spawn-fix`. If a conflict surfaces, resolve by keeping the working-tree (stashed) version: `git checkout --theirs <file>` then `git add <file>`.

- [ ] **Step 2: Verify the staged content**

```bash
git status
# Expected:
#   modified:   CLAUDE.md
# Untracked files:
#   scripts/agent-push.sh

ls -la scripts/agent-push.sh
# Expected: -rwxr-xr-x ... scripts/agent-push.sh
# (executable bit must be set; if not, run: chmod +x scripts/agent-push.sh)

head -5 scripts/agent-push.sh
# Expected first line: #!/bin/bash

tail -50 CLAUDE.md
# Expected: section "# AGENT PUSH RULES — read before running `git push`"
# with subsection "## Rule: never `git push` directly from agent runtime when the gate is on"
# and three numbered options.
```

- [ ] **Step 3: Smoke-test the script**

The helper invokes `lefthook run pre-push` then `git push --no-verify`. To smoke-test without actually pushing, replace `git push` temporarily with `echo` and verify the lefthook step runs as standalone:

```bash
# Read the script — confirm the structure matches the documented design.
cat scripts/agent-push.sh

# Quick syntax check.
bash -n scripts/agent-push.sh
echo "Exit: $?  (must be 0)"
```

Expected: bash -n returns 0 (no syntax errors).

- [ ] **Step 4: Commit**

```bash
git add scripts/agent-push.sh CLAUDE.md
git commit -m "ci(agent-push): scripts/agent-push.sh helper + CLAUDE.md AGENT PUSH RULES

Documents and works around the silent-kill issue diagnosed during the
Phase 0 debug session (2026-05-09): when an agent runtime invokes
git push and the lefthook pre-push hook spawns a long-running
container (the deep gate runs all 10 Linux CI jobs in Podman,
~3-4 min warm), the parent git push process is silently terminated
mid-hook. The orphaned container survives because Podman owns it,
but the actual push never completes.

Empirical (diagnostic in this session):
- Plain run_in_background bash + sleep loops survive 60s+
- Plain run_in_background podman containers survive 60s+
- git push triggering the deep gate dies mid-hook every time
- The kill is specific to the git-push-invokes-long-running-hook path

scripts/agent-push.sh runs lefthook run pre-push as a standalone
command (which does NOT die — the kill is git-push-specific), then
git push --no-verify. This decouples the gate from the network
operation. Forwards args.

CLAUDE.md AGENT PUSH RULES section documents the issue + helper +
recovery procedure for orphaned gate containers.

Folded into Phase 0 PR per the spec amendment (decision 10):
silent-kill diagnostic enabled the Phase 0 investigation; future
agents need the helper to repeat that path safely.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

If lefthook pre-commit fails for environmental reasons, `git commit --no-verify`.

---

## Task 0d: USER GATE — push, open PR, monitor CI

**Main agent only — no subagent.**

- [ ] **Step 1: Verify branch state**

```bash
git status
# Expected: clean working tree
git log --oneline main..HEAD
# Expected: 3 commits (T0a spec, T0b fix, T0c agent-push), plus the prior 129f05b spec amendment.
# So total: 4 commits ahead of main.
```

- [ ] **Step 2: Push via the new helper**

```bash
scripts/agent-push.sh -u origin feat/layer4-llm-spawn-fix
```

This runs `lefthook run pre-push` first (the deep gate; ~3-4 min warm, ~15-20 min cold). If green, runs `git push --no-verify -u origin feat/layer4-llm-spawn-fix`. If the gate fails for legitimate code reasons: fix forward, commit, retry. If for environmental reasons (Podman VM disk full): see CLAUDE.md AGENT PUSH RULES recovery procedure.

If the helper itself misbehaves (it just landed in T0c and is unproven), fall back to `lefthook run pre-push && git push --no-verify -u origin feat/layer4-llm-spawn-fix`.

- [ ] **Step 3: Open PR**

```bash
gh pr create --title "fix(runtime): LLM-backend spawn path (Phase 0 of layer4-startup-io)" --body "$(cat <<'EOF'
## Summary

Phase 0 of the Layer 4 plugin-compat startup-IO sub-project. Spec amendment at `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md` (Phase 0 section) explains the unexpected blocker that surfaced during T7 investigation and locks this PR's scope.

**The bug:** all 3 LlmBackend HTTP plugins (anthropic, ollama, openai) failed at SPAWN with `Os { code: 2 }` (ENOENT) within 4 ms — before reaching the startup-IO surface T7 was scoped to investigate. Tool plugins (shell, fs-read) succeeded with the same baseline + same Podman environment + same plugin binaries on disk. Bug was in `tau-runtime::plugin_host::load_llm_backend`'s code path specifically.

**The fix:** [paraphrase root cause + fix from T0a/T0b commits].

**Verified in lefthook Podman gate:**
- `fs_read_layer4_native_reads_data_file` still PASSES (PR 1 win not regressed)
- 3 HTTP tests now fail at handshake-EOF (the original PR 2 startup-IO blocker), not spawn-ENOENT
- All existing `tau-runtime` lib tests pass

**Folded scope:** `scripts/agent-push.sh` + CLAUDE.md AGENT PUSH RULES section, drafted in stash during the silent-kill diagnostic that enabled this Phase 0 (per spec decision 10).

PR 2 of the layer4-startup-io sub-project resumes after this merges. The 3 HTTP `#[ignore]` messages now point at T7' for tracking.

## Test plan

- [ ] CI green on the 14 required checks (especially `test (tau-plugin-compat / linux)` and `test (tau-runtime e2e / linux)`)
- [ ] Lefthook pre-push gate green (run via `scripts/agent-push.sh`)
- [ ] Diff review: spawn fix is minimal (≤30 LOC across 1-2 files)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Monitor CI**

Use the Monitor tool with a poll loop on `gh pr checks <PR#> --json name,state` emitting a line per check transition out of pending:

```bash
prev=""
while true; do
  s=$(gh pr checks <PR#> --json name,state 2>/dev/null) || { echo "gh-error: failed to fetch checks"; sleep 30; continue; }
  cur=$(jq -r '.[] | select(.state!="PENDING" and .state!="QUEUED" and .state!="IN_PROGRESS") | "\(.name): \(.state)"' <<<"$s" | sort)
  comm -13 <(echo "$prev") <(echo "$cur")
  prev=$cur
  jq -e 'length>0 and (all(.state!="PENDING" and .state!="QUEUED" and .state!="IN_PROGRESS"))' <<<"$s" >/dev/null && { echo "ALL CHECKS COMPLETE"; break; }
  sleep 30
done
```

Pause for user approval before T0e.

---

## Task 0e: USER GATE — squash-merge

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

Expected: top commit is the squash-merged Phase 0.

- [ ] **Step 4: Update the original layer4-startup-io plan**

Open `docs/superpowers/plans/2026-05-09-layer4-startup-io.md` — find the existing T7 task header (currently "Task 7: Investigation (HARD GATE) — identify HTTP plugin startup-IO"). Above it, add a brief note:

```markdown
> **Phase 0 prerequisite shipped 2026-05-09 (PR #<NUMBER>).** Original T7 was blocked by a `tau-runtime::plugin_host::load_llm_backend` ENOENT bug; Phase 0 (`feat/layer4-llm-spawn-fix`) shipped the spawn fix. T7 below is unblocked and can run as originally scoped.
```

Then commit on a fresh branch (NOT main):

```bash
git checkout -b chore/layer4-plan-renumber
git add docs/superpowers/plans/2026-05-09-layer4-startup-io.md
git commit -m "docs(plan): annotate T7 unblocked after Phase 0 merge"
scripts/agent-push.sh -u origin chore/layer4-plan-renumber
gh pr create --title "docs(plan): annotate T7 unblocked after Phase 0" --body "Tiny doc-only PR. Adds a note above T7 in the layer4-startup-io plan that Phase 0 (PR #<PHASE0_PR>) shipped the spawn fix; T7 can now run as originally scoped."
```

(Optional: this can also be folded into the next PR that does T7' work, instead of a standalone PR. Either is fine.)

---

## Self-review checklist

After all tasks complete, verify:

- [ ] `git log --oneline main..HEAD` shows the 3 Phase 0 commits + the prior spec amendment
- [ ] All 14 required CI checks green on PR #<PHASE0_PR>
- [ ] `fs_read_layer4_native_reads_data_file` still passes (no regression)
- [ ] The 3 HTTP tests' `#[ignore]` messages reference T7' and the spec
- [ ] `scripts/agent-push.sh` is executable and CLAUDE.md AGENT PUSH RULES is in place
- [ ] Spec's "Investigation findings (Phase 0)" section is filled with concrete data, not template placeholders
