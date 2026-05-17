# Test Suite Upgrades — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Raise test-suite quality across the tau workspace by closing concrete coverage gaps, removing flake sources, and adopting better assertion idioms — one shippable task at a time.

**Architecture:** Each task is an independent, single-PR-sized unit. Tasks are ordered roughly by ROI (high-impact / low-risk first). Tasks do NOT depend on each other unless explicitly noted — pick any unblocked task and ship it. Each produces a green CI run with a stand-alone commit.

**Tech Stack:** Rust workspace, `cargo nextest` (with `retries = 2`, `failure-output = "immediate-final"` already configured in `.config/nextest.toml`), `insta` snapshots, hand-rolled mocks in `tau-ports::fixtures` and `tau-runtime/tests/common/mock_llm.rs`, `proptest` in selected crates.

**Branch policy:** Per memory `feedback_branch_protection_workflow`, every task ships on a `feat/tests-*` branch via PR to `main`. Never push to `main` directly. Use `scripts/agent-push.sh` (see CLAUDE.md AGENT PUSH RULES).

**Cargo discipline:** Every cargo command MUST follow CLAUDE.md rules — `CARGO_TARGET_DIR=target/agent-tests` (or `target/main`), `-p <crate>`, `timeout`, `CARGO_INCREMENTAL=0`, prefer `cargo nextest run`.

---

## Refresh history

This plan supersedes `docs/superpowers/plans/2026-05-13-test-suite-upgrades.md`
(an uncommitted draft from before Skills-4/5/6 shipped). Differences from the
2026-05-13 draft, verified against current code on 2026-05-17:

| 2026-05-13 task                                       | Status today          | Note                                                                                       |
|-------------------------------------------------------|-----------------------|--------------------------------------------------------------------------------------------|
| T1: panic!("expected …") → assert_matches!            | KEEP — count grew     | Was "10+ in run_streaming_e2e"; verified 66 sites across workspace.                        |
| T2: skill list/show negative-path coverage            | KEEP                  | Skills-3 shipped (PR #66); happy paths covered, errors thin.                               |
| T3: eprintln!("SKIP") + return → #[ignore]            | KEEP, smaller         | Now only 2 files (was 10+). Layer4 SKIPs migrated to `#[ignore]` during Skills work.       |
| T4: strict_proxy.rs sleep(100ms) deflake              | **DONE**              | strict_proxy.rs uses polling helper with 20ms tick + deadline. No blind sleep remains.     |
| T5: #[ignore] inventory                               | KEEP, refresh counts  | Now 34 annotations (was 31). Three buckets identified below.                               |
| T6: orchestration patterns A–E                        | **DONE**              | Skills-4 T9 (PR #83) shipped MockLlmBackend + removed #[ignore] from all five A–E tests.   |
| T7: tempfile::TempDir::new() scratch_dir helper       | KEEP, smaller         | 15 sites left (was reported as ~60). Still worth the helper for diagnostic quality.        |
| T8: MockLlmBackend invocation verification            | KEEP                  | Skills-4 fixture lives at `tau-runtime/tests/common/mock_llm.rs` (copied in cmd_orch).     |
| T9: doctest activation on tau-ports public traits     | KEEP                  | Examples still `ignore`'d.                                                                 |
| T10: rstest parametrization pilot                     | KEEP                  | Skills-3 added more cmd_*.rs tests; pilot has more material to work with.                  |

Three NEW tasks (T11–T13) cover surfaces that did not exist on 2026-05-13.

---

## Pre-flight (do once before starting)

- [ ] **Confirm clean baseline**

```bash
git status
git fetch origin
git log --oneline origin/main..HEAD
```

Expected: working tree clean OR uncommitted state matches your understanding.

- [ ] **Confirm baseline test status**

```bash
timeout 600 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run --workspace --no-fail-fast 2>&1 | tail -30
```

Expected: known-passing baseline. Record the count of passing/ignored tests. Each task in this roadmap should keep passing count monotonically non-decreasing, and ideally move tests from `ignored` → `passing` or add new `passing`.

---

## Task 1 — Replace `panic!("expected X")` with `assert_matches!` in runtime tests

**Why:** Verified 66 occurrences of `panic!("expected …")` across the workspace via `grep -rln 'panic!("expected' crates --include="*.rs" | wc -l`. The pattern works but truncates structured info and isn't grep-friendly. `assert_matches!` (stable in std since Rust 1.82, or via the `assert_matches` crate) gives diff-style output and is the project's idiomatic choice in newer code.

**Files (broader than 2026-05-13 draft — now includes all crates):**
- Modify: `crates/tau-runtime/tests/run_streaming_e2e.rs` (most sites)
- Modify: `crates/tau-runtime/tests/run_kernel_errors.rs`
- Modify: `crates/tau-runtime/tests/run_with_tool_calls.rs`
- Modify: any other test file matching `grep -rln 'panic!("expected' crates --include="*.rs"`

**Scope discipline:** Even though 66 sites exist workspace-wide, prefer one crate per PR. Start with `tau-runtime` (largest concentration), then sweep one crate at a time.

- [ ] **Step 1: Inventory the panics**

```bash
grep -rn 'panic!("expected' crates --include="*.rs" | tee /tmp/panic-inventory.txt
wc -l /tmp/panic-inventory.txt
```

- [ ] **Step 2: Decide on macro source**

Check whether `std::assert_matches::assert_matches` is stable in the project's MSRV. Read `rust-toolchain.toml` (or `Cargo.toml` workspace `rust-version`). If MSRV < 1.82, add the `assert_matches = "1"` crate to `[workspace.dependencies]` and to consuming crates' `[dev-dependencies]`.

- [ ] **Step 3: Convert one site first to validate the pattern**

Example transformation (BEFORE):
```rust
let StreamEvent::ToolCallCompleted { call_id, result, .. } = ev else {
    panic!("expected ToolCallCompleted");
};
```

AFTER:
```rust
use assert_matches::assert_matches;

assert_matches!(
    &ev,
    StreamEvent::ToolCallCompleted { call_id, result, .. } => {
        // assertions on call_id / result here
    },
    "expected ToolCallCompleted, got {ev:?}"
);
```

For `_ => panic!(...)` arms inside `match`, prefer:
```rust
assert_matches!(ev, StreamEvent::ToolCallCompleted { .. });
```

- [ ] **Step 4: Run the converted test in isolation**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p tau-runtime --test run_streaming_e2e -- <specific_test_name>
```

Sanity check: temporarily mutate the expected variant; verify `assert_matches!` produces a helpful diff.

- [ ] **Step 5: Convert remaining sites in the chosen crate**

Apply to all sites in the crate. Keep diff focused — do NOT refactor adjacent code.

- [ ] **Step 6: Run the full crate**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p tau-runtime
```

- [ ] **Step 7: Commit**

```bash
git checkout -b feat/tests-assert-matches-runtime
git add crates/tau-runtime/ Cargo.toml Cargo.lock
git commit -m "test(runtime): replace panic!() with assert_matches! in event-shape assertions"
```

PR title: `test(runtime): use assert_matches! for event-shape assertions`

Repeat per crate (`tau-cli`, `tau-pkg`, etc.) as separate PRs.

---

## Task 2 — Negative-path coverage for `tau skill list` / `tau skill show`

**Why:** Skills-3 shipped (PR #66, ADR-0027). `crates/tau-cli/tests/cmd_skill_list.rs` and `cmd_skill_show.rs` exist with happy paths and 1–2 trivial errors. Skills-5/6 (PRs #102/#115) added more user-facing surface; error paths users will actually hit deserve test coverage.

**Files:**
- Modify: `crates/tau-cli/tests/cmd_skill_list.rs`
- Modify: `crates/tau-cli/tests/cmd_skill_show.rs`
- Reuse: `crates/tau-cli/tests/common/mod.rs` helpers (read first)

- [ ] **Step 1: Read the existing tests and helpers**

Read these files in full first (do not skim):
- `crates/tau-cli/tests/cmd_skill_list.rs`
- `crates/tau-cli/tests/cmd_skill_show.rs`
- `crates/tau-cli/tests/common/mod.rs`
- `crates/tau-cli/src/error_render.rs` (to know what error shapes user sees)

- [ ] **Step 2: Enumerate the realistic negative paths**

For `tau skill list`:
1. Run from a directory with no `.tau/` scope marker → expect helpful error (suggest `tau init`)
2. Run inside a scope whose lockfile schema version is unknown (e.g. write `schema_version = 99` to lockfile) → expect schema-mismatch error, not panic
3. Lockfile present but malformed TOML → expect parse error with file path in message

For `tau skill show`:
1. Skill name that fuzzy-matches multiple packages → expect disambiguation (or pin current behavior)
2. `--json` and `--body` flags combined → expect either explicit reject or sensible JSON-with-body. Pin whichever the current implementation does; if the JSON-with-body shape is unintentional, file an issue rather than fixing in this task.
3. Skill present in manifest but install dir was deleted out-of-band → expect specific error, not "file not found" leak

- [ ] **Step 3: Write one failing test per scenario**

Each test follows the pattern already used in `cmd_skill_list.rs`. Example for "missing scope marker":

```rust
#[test]
fn list_errors_outside_tau_scope() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let mut cmd = Command::cargo_bin("tau").expect("tau bin");
    cmd.current_dir(temp.path()).args(["skill", "list"]);

    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("no .tau"))
        .stderr(predicates::str::contains("tau init"));
}
```

Adjust expected stderr substrings to whatever `error_render.rs` actually produces — read it, do not guess.

- [ ] **Step 4: Run each new test individually first**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p tau-cli --test cmd_skill_list -- <test_name>
```

If a test fails because production code panics instead of erroring cleanly, DO NOT silently change production code — stop and surface the finding to the user.

- [ ] **Step 5: Run both files**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p tau-cli --test cmd_skill_list --test cmd_skill_show
```

- [ ] **Step 6: Commit**

```bash
git checkout -b feat/tests-skill-negative-paths
git add crates/tau-cli/tests/cmd_skill_list.rs crates/tau-cli/tests/cmd_skill_show.rs
git commit -m "test(cli/skill): negative-path coverage for list/show"
```

---

## Task 3 — Replace remaining `eprintln!("SKIP: ...") + return` with `#[ignore]`

**Why:** Most silent SKIPs migrated to `#[ignore]` during Skills work. Verified two sites remain: `crates/tau-sandbox-native/tests/strict_bridge.rs` and `crates/tau-sandbox-native/src/strict.rs`. The src/ one is inside non-test code — leave it unless investigation shows it's only reachable from a test.

**Files:**
- `crates/tau-sandbox-native/tests/strict_bridge.rs`
- Investigate (do not modify unless test-only): `crates/tau-sandbox-native/src/strict.rs`

- [ ] **Step 1: Read the test file SKIP site and classify**

Answer: (a) what causes the skip? (b) does any CI job actually exercise the non-skipped path? (c) is the skip masking a real coverage hole?

- [ ] **Step 2: Convert to `#[ignore = "..."]` with explicit reason**

```rust
#[tokio::test]
#[ignore = "requires <specific condition>; see CI job <name>"]
async fn strict_bridge_smoke() {
    let adapter = NativeAdapter::probe().await.expect("native adapter");
    // ... rest of test
}
```

- [ ] **Step 3: Update CI to run the now-ignored test where possible**

Check `.github/workflows/`. If a job exists that satisfies the probe (e.g. Linux native landlock), add a step:

```yaml
- name: Run ignored landlock tests
  run: cargo nextest run --run-ignored only -p tau-sandbox-native
```

If no such job exists, document in the PR description that the test is currently dark on CI.

- [ ] **Step 4: Run locally**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p tau-sandbox-native
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p tau-sandbox-native --run-ignored only
```

- [ ] **Step 5: Commit**

```bash
git checkout -b feat/tests-explicit-skip-strict-bridge
git add crates/tau-sandbox-native/tests/strict_bridge.rs .github/workflows/
git commit -m "test(sandbox-native): replace silent SKIP return with #[ignore]"
```

---

## Task 4 — DONE (2026-05-13 draft Task 4)

`strict_proxy.rs` no longer uses a blind `sleep(100ms)`. Current code at
`crates/tau-sandbox-native/tests/strict_proxy.rs:105` uses a polling helper
with a 20ms tick and explicit deadline (`Err(format!("after {:?}, still present in ..."))`).
No action required.

---

## Task 5 — Audit and triage all `#[ignore]` annotations

**Why:** Verified 34 `#[ignore]` annotations across the workspace (close to the 2026-05-13 count of 31). No tracking issue per ignore.

**Buckets identified by 2026-05-17 inventory:**

1. **Live-API tests (6 sites):** `tau-plugins/{anthropic,ollama,openai}/tests/live.rs` — gated behind `TAU_*_LIVE_TESTS=1` + API keys. Legitimate; document in CI README rather than promote.
2. **Layer4 sandbox tests (10 sites):** `tau-plugin-compat/tests/layer4_{container,native}.rs` — require Docker/Podman daemon + prebuilt plugin binaries. **Largest dark-spot in CI today**; see Task 11.
3. **Sub-project D e2e (2 sites):** `tau-cli/tests/cmd_resolve_check_sandbox.rs` — require host with no strict-capable adapter. Promote when sub-project D e2e CI lands.
4. **Other deferred (6 sites):** `tau-pkg/tests/install_cross_check.rs` (release-build cost), `tau-cli/tests/cmd_workflow.rs` (echo-llm plugin fixture pending), `tau-runtime/tests/sandbox_container.rs` (Linux+container daemon).

**Files:**
- Read-only audit, then create `docs/test-ignores-inventory.md` with the table.
- This task produces ONE artifact: a tracked inventory. Subsequent tasks pick items off it.

- [ ] **Step 1: Generate the raw list**

```bash
grep -rn '#\[ignore' crates --include="*.rs" > /tmp/ignores-raw.txt
wc -l /tmp/ignores-raw.txt
```

- [ ] **Step 2: For each ignore, gather context**

For every line in the raw list, open the file and capture:
- Test function name
- Ignore reason (from `#[ignore = "..."]`) — if no reason, the ignore is suspect
- Date / commit that introduced the ignore (`git blame`)
- Bucket (1–4 above)

- [ ] **Step 3: Write the inventory file**

Create `docs/test-ignores-inventory.md`:

```markdown
# Test Ignore Inventory — 2026-05-17

| File:line | Test | Reason | Bucket | Status |
|-----------|------|--------|--------|--------|
| crates/tau-plugin-compat/tests/layer4_container.rs:278 | shell_plugin_container | requires Docker/Podman + tau-plugin-shell-plugin:dev | 2 | DARK in CI — see Task 11 |
| ... | ... | ... | ... | ... |
```

Status values: `LIVE-DOCUMENTED` (bucket 1), `DARK` (bucket 2/3, needs new CI job), `DEFERRED` (bucket 4, waiting on dependency).

- [ ] **Step 4: Commit**

```bash
git checkout -b feat/tests-ignore-inventory
git add docs/test-ignores-inventory.md
git commit -m "docs(tests): inventory of #[ignore]'d tests with triage buckets"
```

---

## Task 6 — DONE (2026-05-13 draft Task 6)

Orchestration patterns A–E are no longer `#[ignore]`'d. Skills-4 T9 (PR #83
at `1f6f331`) built `MockLlmBackend` at `crates/tau-runtime/tests/common/mock_llm.rs`,
copied into `crates/tau-cli/tests/common/`, and removed the `#[ignore]`
annotations from all five pattern tests in `crates/tau-cli/tests/cmd_orchestration.rs`.
No action required.

---

## Task 7 — `scratch_dir` helper for labeled tempdir creation

**Why:** Verified 15 sites using `tempfile::TempDir::new().unwrap()` raw (the 2026-05-13 draft reported ~60; reality is much lower today). Still worth the helper: cleanup races give terrible diagnostics, and `.unwrap()` on tempfile failure points at tempfile internals not the test.

**Files:**
- Modify: `crates/tau-ports/src/fixtures.rs` (add helper under existing `test-fixtures` feature)
- Migrate: opt-in, one crate at a time. Start with the crate that has the most sites.

- [ ] **Step 1: Inventory**

```bash
grep -rn "tempfile::TempDir::new()" crates --include="*.rs" | tee /tmp/tempdir-sites.txt
wc -l /tmp/tempdir-sites.txt
# Group by crate to pick the largest:
sort -u /tmp/tempdir-sites.txt | awk -F/ '{print $2}' | sort | uniq -c | sort -rn
```

- [ ] **Step 2: Add helper to `tau-ports::fixtures`**

```rust
/// Test scratch directory with descriptive failure messages.
///
/// Prefer this over `tempfile::TempDir::new().unwrap()` so test failures
/// point at the call site rather than a bare unwrap inside tempfile.
pub fn scratch_dir(label: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("tau-test-{label}-"))
        .tempdir()
        .unwrap_or_else(|e| panic!("failed to create scratch dir for '{label}': {e}"))
}
```

- [ ] **Step 3: Migrate ONE crate**

Find/replace `tempfile::TempDir::new().unwrap()` → `tau_ports::fixtures::scratch_dir("some-label")` where the label briefly describes the test scenario.

- [ ] **Step 4: Run that crate's tests**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p <crate>
```

- [ ] **Step 5: Commit**

```bash
git checkout -b feat/tests-scratch-dir-helper
git add crates/tau-ports/src/fixtures.rs crates/<crate>/
git commit -m "test(<crate>): adopt scratch_dir helper for labeled tempdir creation"
```

Repeat once per crate. Do NOT do a workspace-wide search/replace in one PR.

---

## Task 8 — `MockLlmBackend` invocation verification

**Why:** Two copies of `MockLlmBackend` exist today: `crates/tau-runtime/tests/common/mock_llm.rs` (Skills-4 T7) and `crates/tau-cli/tests/common/` (copy, per file header rationale). Both record scripted responses but don't validate that the expected calls actually happened. Tests can pass while the mock was never exercised.

**Files:**
- Modify: `crates/tau-runtime/tests/common/mock_llm.rs` (primary)
- Mirror change to: `crates/tau-cli/tests/common/` copy (or fix the duplication — see Step 4)

- [ ] **Step 1: Read `mock_llm.rs` in full**

Catalog what the mock records (turn index, message, tool args) and what it does NOT record.

- [ ] **Step 2: Add explicit verifier**

```rust
impl MockLlmBackend {
    /// Assert the mock was invoked exactly `n` times. Call before the test ends.
    #[track_caller]
    pub fn verify_invocation_count(&self, expected: usize) {
        let actual = self.invocations.lock().unwrap().len();
        assert_eq!(
            actual, expected,
            "MockLlmBackend invocation count mismatch: expected {expected}, got {actual}"
        );
    }

    /// Assert the mock script was fully consumed (no leftover scripted turns).
    #[track_caller]
    pub fn verify_fully_consumed(&self) {
        let remaining = self.script_remaining();
        assert_eq!(
            remaining, 0,
            "MockLlmBackend had {remaining} scripted turns left unconsumed — \
             test exited early or script is over-provisioned"
        );
    }
}
```

- [ ] **Step 3: Adopt in one test as proof**

Find a test in `tau-runtime/tests/` that uses MockLlmBackend, add `.verify_fully_consumed()` before the test ends. Expected: PASS if the script is accurate; FAIL with clear message if not.

- [ ] **Step 4: Decide on the duplication**

The mock is duplicated `tau-runtime/tests/common/` ↔ `tau-cli/tests/common/`. Options:
- (A) Accept duplication; mirror this change to both files. Simplest; matches current state.
- (B) Lift to `tau-ports::fixtures` under a feature gate. Cleaner; requires coordinating with Skills-4 author intent (file header explicitly chose duplication).

Recommend (A) for this task. File (B) as a follow-up issue.

- [ ] **Step 5: Run**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p tau-runtime
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p tau-cli
```

- [ ] **Step 6: Commit**

```bash
git checkout -b feat/tests-mock-verification
git add crates/tau-runtime/tests/common/ crates/tau-cli/tests/common/
git commit -m "test(fixtures): verify MockLlmBackend script consumption"
```

---

## Task 9 — Doctest activation for `tau-ports` public traits

**Why:** Most public-API examples are marked ```` ```ignore ````. Verified by `grep -rn '\`\`\`ignore' crates/tau-ports/src/`. Engineers reading the docs can't trust the examples.

**Files:**
- Modify: `crates/tau-ports/src/tool.rs`, `llm.rs`, and other public surface files
- Possibly: `.github/workflows/*.yml` (add `cargo test --doc` step if missing)

- [ ] **Step 1: List existing `ignore` doctests**

```bash
grep -rn '```ignore' crates/tau-ports/src/ | tee /tmp/ignored-doctests.txt
```

- [ ] **Step 2: Pick the easiest example, convert to runnable**

For each `ignore` block, choose:
- (a) Add required `use` lines and show a complete minimal snippet → ` ``` `
- (b) Demote to `no_run` if the example is conceptually correct but requires runtime setup
- (c) Demote to `text` if it's not really a code example

NEVER leave a `ignore` block without a justifying comment.

- [ ] **Step 3: Run doctests for the crate**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo test --doc -p tau-ports
```

Note: nextest does NOT run doctests — must use plain `cargo test --doc`.

- [ ] **Step 4: Check CI runs doctests**

Look at `.github/workflows/*.yml` for `cargo test --doc` or equivalent. If absent, add a step.

- [ ] **Step 5: Commit**

```bash
git checkout -b feat/tests-doctests-ports
git add crates/tau-ports/src/ .github/workflows/
git commit -m "test(ports): activate doctests on public trait examples"
```

---

## Task 10 — Parametrize CLI output-format duplication with `rstest`

**Why:** Every `crates/tau-cli/tests/cmd_*.rs` has duplicated human-snapshot + JSON tests, hand-written. Adding `--raw` or `--debug` requires editing N files. `rstest` parametrization collapses this and enforces format parity.

**Files:**
- Add: `rstest = "0.18"` (or current) to `tau-cli`'s `[dev-dependencies]`
- Modify: ONE `cmd_*.rs` file as the pilot — suggest `cmd_skill_list.rs` (small, dual-format)

- [ ] **Step 1: Add dependency**

Edit `crates/tau-cli/Cargo.toml` manually under `[dev-dependencies]`.

- [ ] **Step 2: Convert one test pair to a single parametrized test**

BEFORE:
```rust
#[test]
fn list_human_three_skills() { /* ... */ }

#[test]
fn list_json_three_skills() { /* ... */ }
```

AFTER:
```rust
#[rstest]
#[case::human("human", "list_human_three_skills")]
#[case::json("json", "list_json_three_skills")]
fn list_three_skills(#[case] format: &str, #[case] snapshot_name: &str) {
    let project = build_three_skill_fixture();
    let output = run_skill_list(project.path(), format);
    insta::assert_snapshot!(snapshot_name, output);
}
```

- [ ] **Step 3: Verify snapshots still match**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p tau-cli --test cmd_skill_list
```

If any snapshot diff appears (`.snap.new` files), STOP and reconcile. The snapshot name should not have changed.

- [ ] **Step 4: Commit**

```bash
git checkout -b feat/tests-rstest-pilot
git add crates/tau-cli/Cargo.toml Cargo.lock crates/tau-cli/tests/cmd_skill_list.rs
git commit -m "test(cli/skill): parametrize list tests with rstest as format-parity pilot"
```

Discuss with the team before expanding to all `cmd_*.rs` — `rstest` adds a dep and a learning curve; one pilot validates the choice.

---

## Task 11 — Layer4 sandbox CI dark-spot ("ignored-only" matrix job)

**Why:** 10 `#[ignore]`'d tests in `crates/tau-plugin-compat/tests/layer4_{container,native}.rs` are the largest hidden coverage gap in the workspace today. They cover the strict-tier sandbox boundary for shell, fs-read, anthropic, ollama, and openai plugins under both native (Linux landlock/seccomp) and container (Docker/Podman) execution. Each is gated by `#[ignore = "requires <daemon> + <prebuilt plugin>"]` precisely because they can't run in the default CI matrix.

**This is the single highest-coverage-impact task in the plan.**

**Files:**
- Modify: `.github/workflows/<existing-linux-job>.yml` OR add new `.github/workflows/layer4-ignored.yml`
- Reference: `crates/tau-plugin-compat/tests/layer4_native.rs` (5 ignores) and `layer4_container.rs` (5 ignores)
- Reference: existing pre-push gate at `lefthook.yml` — already runs Linux jobs in Podman per memory `project_dev_environment_findings_2026_05_07`. The setup that makes lefthook's container job work is reusable for CI.

- [ ] **Step 1: Read the existing Linux CI jobs and the lefthook setup**

```bash
ls .github/workflows/
cat lefthook.yml 2>/dev/null | head -80
```

Find which CI jobs run on Linux runners with Docker available (most GitHub-hosted ubuntu-latest do). Note the existing job names and their cache configuration.

- [ ] **Step 2: Pre-flight: build the required plugin binaries in CI**

The ignored tests need pre-built plugin binaries (e.g. `tau-plugins-shell --release`). Decide:
- (A) Build them as part of the new job — slower per-run but simple.
- (B) Build once in an upstream job and pass via `actions/upload-artifact` + `download-artifact`.

Recommend (A) for v1; cache via existing `Swatinem/rust-cache` (which the repo already uses).

- [ ] **Step 3: Add a new workflow job (matrix: native + container)**

```yaml
# .github/workflows/layer4-ignored.yml (or extend existing)
jobs:
  layer4-ignored:
    name: layer4-ignored-${{ matrix.flavor }}
    runs-on: ubuntu-latest
    strategy:
      matrix:
        flavor: [native, container]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Install nextest
        uses: taiki-e/install-action@nextest
      - name: Build required plugin binaries
        run: |
          cargo build --release -p tau-plugins-shell -p tau-plugins-fs-read \
            -p tau-plugins-anthropic -p tau-plugins-ollama -p tau-plugins-openai
      - name: Run ignored layer4 tests
        env:
          CARGO_INCREMENTAL: 0
          CARGO_TARGET_DIR: target/ci-layer4
        run: |
          cargo nextest run --run-ignored only -p tau-plugin-compat \
            --test layer4_${{ matrix.flavor }}
```

Match plug-in build commands to the actual `#[ignore]` reason strings — they document exactly which binary each test needs.

- [ ] **Step 4: Confirm tests can find their prebuilt binaries**

Read the test fixture helpers in `crates/tau-plugin-compat/tests/layer4_*.rs` (each has a `tests/helpers/` style block per file header). They typically locate binaries under `target/release/` or via an env var. Make sure the CI job's build target dir lines up with where the tests look.

- [ ] **Step 5: Add the job to required checks**

Once green twice in a row, mark the job as a required check under branch protection (or document why not). At minimum, do NOT auto-merge PRs that touch `tau-plugin-compat/` or any of the 5 plugin crates without this job green.

- [ ] **Step 6: Run locally to confirm test contracts**

Linux box or VM, with Docker/Podman:

```bash
cargo build --release -p tau-plugins-shell ...
timeout 600 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests \
  cargo nextest run --run-ignored only -p tau-plugin-compat --test layer4_native
```

If you don't have Linux locally, this can validate solely on CI after pushing — note that in the PR description.

- [ ] **Step 7: Commit**

```bash
git checkout -b feat/tests-layer4-ignored-ci
git add .github/workflows/
git commit -m "ci(layer4): run ignored layer4 tests in dedicated matrix job"
```

Update `docs/test-ignores-inventory.md` (from Task 5) to reflect the now-lit tests.

---

## Task 12 — Lockfile migration + Anthropic-strict drift coverage

**Why:** Skills-2 → Skills-5 walked the lockfile through v4 → v5 → v6 (PRs #64, #102). Each migration is a one-way door for users. Today no test loads an old lockfile snapshot and verifies clean migration. Skills-5 also introduced `tau verify --anthropic-strict` (a new failure surface; ADR-0029) whose drift shapes deserve snapshot coverage.

**Files:**
- Read: `crates/tau-domain/src/package/lock.rs` (or wherever lockfile types live; `grep -rn "schema_version" crates/tau-domain/src/`)
- Read: `crates/tau-pkg/src/skill_check.rs` (Skills-2 module)
- Read: `crates/tau-cli/src/error_render.rs` (for `--anthropic-strict` rendering)
- Create: `crates/tau-pkg/tests/fixtures/lockfiles/v4-minimal.toml`, `v5-minimal.toml`, etc.
- Create: `crates/tau-pkg/tests/lockfile_migration.rs`
- Create: `crates/tau-cli/tests/cmd_verify_anthropic_strict.rs`

- [ ] **Step 1: Identify all lockfile schema versions and their migration paths**

```bash
grep -rn "schema_version" crates --include="*.rs"
git log --oneline --all -- crates/tau-domain/src/package/lock.rs | head
```

Document the chain: v1 → v2 → ... → v6. Note which migrations are lossless and which are not (Skills-5 `synthesized_from` per memory).

- [ ] **Step 2: Build minimal fixture lockfiles for each historical version**

One file per supported version. Keep them tiny — 2 packages + 1 skill is enough.

- [ ] **Step 3: Write migration round-trip tests**

```rust
#[test]
fn lockfile_v4_loads_and_migrates_to_v6() {
    let v4 = include_str!("fixtures/lockfiles/v4-minimal.toml");
    let parsed = Lockfile::from_toml_str(v4).expect("v4 must parse");
    assert_eq!(parsed.schema_version, 6); // auto-migrated
    // Spot-check semantics survive migration:
    assert!(!parsed.packages.is_empty());
    // ... etc
}
```

- [ ] **Step 4: Snapshot the `--anthropic-strict` failure shapes**

For each known drift variant (`SkillContentDrift`, etc.) build a minimal fixture and snapshot stderr. Use `insta::assert_snapshot!`.

- [ ] **Step 5: Run**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p tau-pkg --test lockfile_migration
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-tests cargo nextest run -p tau-cli --test cmd_verify_anthropic_strict
```

- [ ] **Step 6: Commit (two commits, one per concern)**

```bash
git checkout -b feat/tests-lockfile-migration
git add crates/tau-pkg/tests/fixtures/ crates/tau-pkg/tests/lockfile_migration.rs
git commit -m "test(pkg): lockfile v4→v5→v6 migration round-trip fixtures"

git add crates/tau-cli/tests/cmd_verify_anthropic_strict.rs
git commit -m "test(cli): snapshot --anthropic-strict drift error shapes"
```

---

## Task 13 — `cargo-llvm-cov` baseline + CI report

**Why:** No coverage measurement today. Adding it once gives every future task a measurable target ("did this PR move the needle?") and surfaces zero-coverage modules. Use `cargo-llvm-cov` because it works with nextest (`cargo llvm-cov nextest`), supports merging across crates, and produces lcov for GitHub PR comments.

**Out of scope:** Setting a hard coverage threshold gate. Coverage as a gate is a known anti-pattern (incentivizes test-for-coverage-not-correctness). This task installs measurement; threshold discussion is a follow-up.

**Files:**
- Add: `.github/workflows/coverage.yml`
- Possibly: `cargo-llvm-cov` config in `.config/nextest.toml` or new `.cargo/config.toml`
- Document: `docs/dev-environment.md` (one-line link to local invocation)

- [ ] **Step 1: Install and run locally to confirm the toolchain works**

```bash
cargo install cargo-llvm-cov --locked
rustup component add llvm-tools-preview
timeout 600 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-coverage \
  cargo llvm-cov nextest --workspace --no-fail-fast --html
```

Open `target/agent-coverage/llvm-cov/html/index.html`. Record baseline percent per crate.

- [ ] **Step 2: Add CI workflow**

```yaml
# .github/workflows/coverage.yml
name: coverage
on:
  pull_request:
  push:
    branches: [main]
jobs:
  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: llvm-tools-preview
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@nextest
      - uses: taiki-e/install-action@cargo-llvm-cov
      - name: Generate coverage
        run: cargo llvm-cov nextest --workspace --no-fail-fast --lcov --output-path lcov.info
      - name: Upload to Codecov
        uses: codecov/codecov-action@v4
        with:
          files: lcov.info
          fail_ci_if_error: false
```

(Codecov is one choice; alternative is to publish lcov as an artifact and post a comment via a GitHub action. Pick based on team preference — Codecov gives prettier PR comments at the cost of an external dep.)

- [ ] **Step 3: Document local usage**

Add a one-line section in `docs/dev-environment.md`:

```markdown
### Coverage

Local: `cargo llvm-cov nextest --workspace --no-fail-fast --html` then open the html report.
CI: posted to PR by Codecov. **Coverage is a signal, not a gate** — do not write tests to hit a number.
```

- [ ] **Step 4: Run end-to-end on the PR**

The PR should itself trigger the new workflow. Expected: lcov uploaded, baseline established.

- [ ] **Step 5: Commit**

```bash
git checkout -b feat/tests-coverage-baseline
git add .github/workflows/coverage.yml docs/dev-environment.md
git commit -m "ci(coverage): cargo-llvm-cov baseline (no gate)"
```

---

## Cross-cutting reminders for every task

- **Run pre-push gate before pushing**: `scripts/agent-push.sh` (per CLAUDE.md AGENT PUSH RULES). Plain `git push` will silently die mid-hook.
- **Verify before claiming done**: do not write "tests pass" in a commit message without showing the green nextest output in your scratch notes. (Per `superpowers:verification-before-completion`.)
- **One PR per task** (or per sub-step for T1, T6 split). Squash-merge.
- **Update `docs/test-ignores-inventory.md`** (created in Task 5) whenever a task removes or promotes an `#[ignore]`.
- **Worktree note**: per memory `project_deep_gate_worktree_gitdir`, running lefthook pre-push from a linked worktree may fail `cmd_update` tests inside the container. CI is the authoritative gate; `--no-verify` is safe for docs/YAML-only changes.

---

## Out of scope (intentional)

- **Property-based testing expansion** — `proptest` already exists where it makes sense; expanding it should be a separate spec, not bundled here.
- **Mutation testing** (`cargo-mutants`) — interesting but a tooling investment, not a test-quality task.
- **Performance/bench tests** — different discipline; tracked elsewhere.
- **Workspace-wide migration to `tau-test-fixtures` crate** — extraction discussed in the audit; creates cross-crate dependencies. Defer until a second consumer of `tau-ports::fixtures` outside dev-deps emerges.
- **Coverage threshold gate** — see Task 13. Install measurement first; threshold is a separate decision.
