# CI Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate tau's CI from 29 required checks / ~33-min PR critical path to 18 required checks / ≤25-min path via 5 phased PRs, each independently revertable, while upgrading Windows from advisory to hard gate.

**Architecture:** Each phase is one PR. Phase A introduces tooling (shared-cache, nextest, mold, sccache) without renaming any check. Phases B–E reshape the workflow shape (matrix split, build-fixtures + e2e refactor, plugin-release fold, feature-flag-matrix) with explicit branch-protection deltas at each USER GATE. The composite action `.github/actions/setup-rust` absorbs all duplication; no reusable workflow files.

**Tech Stack:** GitHub Actions YAML, `Swatinem/rust-cache@v2`, `mozilla-actions/sccache-action`, `taiki-e/install-action@v2` (nextest), `rui314/setup-mold@v1`, `actions/upload-artifact@v4`, `dtolnay/rust-toolchain@master`.

---

## Plan-erratum block (apply preemptively)

These are carryovers from sub-projects A, B, D — apply them throughout execution.

- **BASE_SHA = `c64c489`** for "pre-existing failure" verification. Sub-project A had 4-of-5 false alarms; B + D had 0 (improved discipline). Maintain that.
- **Per-task focused gate.** This is pure CI / workflow / composite-action work; verification is "did the workflow YAML parse + the change land green on a draft PR?" — NOT full workspace test.
- **Cargo.lock is NOT touched.** No Rust code changes (except possibly `cfg(not(target_os = "windows"))` skips for Phase B's Windows hard-gate prep). If a task tries to add Rust deps, that's a red flag — re-read the spec.
- **`CARGO_INCREMENTAL: 0`** is already set workflow-level (sub-project D); plan does NOT re-introduce.
- **Branch-protection updates require manual GitHub-settings change** by the user during each USER GATE. Embed the exact "Add" / "Remove" check-name lists in PR descriptions.
- **The test-stable job needs both `cargo nextest run` AND `cargo test --workspace --doc`** because nextest doctest support is incomplete.
- **The matrix Linux entries currently run `-- --ignored` integration tests** (lines 66–71 of current ci.yml). These should be REMOVED in Phase B's B1 — the dedicated e2e jobs cover them properly with `--features integration-tests`.
- **Two existing job names mislead.** YAML keys `no-default-features-protocol` and `no-default-features-sdk` produce user-visible check names `build (tau-plugin-protocol)` and `build (tau-plugin-sdk)` but actually run `cargo build/test --no-default-features`. Phase E absorbs them into `feature-flag-matrix`, NOT some "build the protocol/sdk crate" job.
- **`build-tau-plugin-test-support` and `build-tau-plugin-conformance`** are real default-features build/test jobs. They STAY as separate checks in the final 18-check list.
- **The `test (conformance)` job currently uses `cargo test`, NOT cargo nextest.** Phase A's A3 converts it; Phase C's C5 refactors to download artifacts.
- **Phase B requires a Windows test-history audit BEFORE the Phase B PR opens.** This is task B3 — flaky tests must be fixed or `cfg(not(target_os = "windows"))`-skipped before flipping `continue-on-error: false`.
- **Cargo freshness check.** Plan must `touch` prebuilt binaries to bump mtime above source files; verify with `--verbose` that cargo doesn't recompile despite the prebuilt binary present.
- **`Swatinem/rust-cache` save policy stays `save-if: github.ref == 'refs/heads/main'`.** Only main writes the cache; PRs read-only. No write contention between parallel jobs sharing a `shared-key`.
- **DRY-RUN before verifying via PR.** `act -j fmt --dry-run` (if `act` is available) or `gh workflow lint` (if available) catches YAML syntax errors locally; otherwise rely on push-to-branch CI feedback.

---

## File structure

This plan modifies only:

- `.github/actions/setup-rust/action.yml` — enhanced composite action with new inputs (Phase A only).
- `.github/workflows/ci.yml` — refactored across all phases.
- `docs/decisions/0018-ci-optimization.md` — NEW ADR (Phase F).
- `CLAUDE.md` — minor edit if local-dev story changes (Phase F).
- `ROADMAP.md` — mark sub-project shipped (Phase F).

No Rust source files touched (except possibly skip-cfgs in Phase B).

---

# Phase A — Tier 1 wins (foundational tooling)

Phase A introduces cross-job cache sharing, nextest, sccache, and mold without changing any check name. Branch protection unchanged. PR's own CI is the canary.

**Phase A goal:** ~30–40% wall-clock reduction without any control-surface change.

---

### Task A1: Enhance `setup-rust` composite action

**Files:**
- Modify: `.github/actions/setup-rust/action.yml` (full rewrite)

**Context:** Current action has 3 inputs (`toolchain`, `components`, `cache-key`) and uses `Swatinem/rust-cache@v2` with `key:` (per-job key by default). Phase A migrates to `shared-key` (cross-job sharing) and adds 3 optional tools: nextest, sccache, mold. Backward-compat: keep the deprecated `cache-key` input as an alias for `shared-key` for one migration cycle so callers don't all need to flip atomically — but Task A2 immediately migrates every caller.

- [ ] **Step 1: Rewrite composite action**

Replace the entirety of `.github/actions/setup-rust/action.yml` with:

```yaml
name: Setup Rust
description: |
  Install a Rust toolchain (with optional components) and configure
  caching, optional cargo-nextest, sccache, and mold linker. Saves cache
  only on main-branch pushes; PRs read-only.

inputs:
  toolchain:
    description: Rust toolchain version (stable, 1.91, master, etc.)
    required: false
    default: stable
  components:
    description: Comma-separated list of components (rustfmt, clippy)
    required: false
    default: ""
  shared-key:
    description: |
      rust-cache shared-key. Pinned per (os, toolchain). All jobs with
      the same shared-key share one cache entry. Recommended pattern:
      "<os>-<toolchain>", e.g. "linux-stable", "windows-1.91".
    required: false
    default: ""
  cache-key:
    description: |
      DEPRECATED: legacy per-job cache key. Use `shared-key` instead.
      If both are set, `shared-key` wins. Kept for transition; remove
      after all callers migrate.
    required: false
    default: ""
  with-nextest:
    description: Install cargo-nextest (true / false)
    required: false
    default: "false"
  with-sccache:
    description: Install sccache and set RUSTC_WRAPPER (true / false)
    required: false
    default: "false"
  with-mold:
    description: |
      Install the mold linker (Linux only) and set RUSTFLAGS to use it.
      Ignored on non-Linux runners.
    required: false
    default: "false"

runs:
  using: composite
  steps:
    - name: Install Rust toolchain
      uses: dtolnay/rust-toolchain@master
      with:
        toolchain: ${{ inputs.toolchain }}
        components: ${{ inputs.components }}

    - name: Install mold linker (Linux only)
      if: inputs.with-mold == 'true' && runner.os == 'Linux'
      uses: rui314/setup-mold@v1

    - name: Set RUSTFLAGS for mold (Linux only)
      if: inputs.with-mold == 'true' && runner.os == 'Linux'
      shell: bash
      run: echo "RUSTFLAGS=-C link-arg=-fuse-ld=mold" >> $GITHUB_ENV

    - name: Install sccache
      if: inputs.with-sccache == 'true'
      uses: mozilla-actions/sccache-action@v0.0.6

    - name: Configure sccache env
      if: inputs.with-sccache == 'true'
      shell: bash
      run: |
        echo "SCCACHE_GHA_ENABLED=true" >> $GITHUB_ENV
        echo "RUSTC_WRAPPER=sccache" >> $GITHUB_ENV

    - name: Install cargo-nextest
      if: inputs.with-nextest == 'true'
      uses: taiki-e/install-action@v2
      with:
        tool: nextest

    - name: Cache cargo registry, target, and sccache
      uses: Swatinem/rust-cache@v2
      with:
        # `shared-key` enables cross-job cache sharing (per (os, toolchain)).
        # Falls back to deprecated `cache-key` for callers mid-migration;
        # if neither is set, rust-cache uses its automatic per-job key.
        shared-key: ${{ inputs.shared-key }}
        key: ${{ inputs.cache-key }}
        # Save the cache only when running on main (push events to main).
        # PRs restore but don't write — keeps cache stable + avoids
        # PR-specific bloat AND prevents write contention between
        # parallel jobs sharing a key.
        save-if: ${{ github.ref == 'refs/heads/main' }}
```

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/actions/setup-rust/action.yml'))"`
Expected: no output (success).

- [ ] **Step 3: Commit**

```bash
git add .github/actions/setup-rust/action.yml
git commit -m "ci(setup-rust): add shared-key, with-nextest, with-sccache, with-mold inputs

Sub-project E (CI optimization) Phase A Task 1. Enhances the composite
action to support cross-job cache sharing via Swatinem/rust-cache's
shared-key parameter, plus optional nextest / sccache / mold installs.
The deprecated cache-key input remains as a one-cycle alias for
caller migration (Task A2).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task A2: Migrate all 22 job invocations to `shared-key`

**Files:**
- Modify: `.github/workflows/ci.yml` (every `setup-rust` invocation)

**Context:** Current ci.yml has 22 job keys; one is the matrix `test:` job that uses `cache-key: test-${{ matrix.os }}-${{ matrix.toolchain }}`. All other jobs omit `cache-key` (default per-job key applies). After A2: all jobs pass an explicit `shared-key`. Pattern:

| Job category | Shared-key value |
|---|---|
| `fmt` | `linux-stable` |
| `clippy` | `linux-stable` |
| matrix `test` | `${{ matrix.os }}-${{ matrix.toolchain }}` (will be replaced in Phase B) |
| All Linux-only default-toolchain jobs | `linux-stable` |

Cross-job sharing is the goal: every Linux-stable job shares the SAME cache entry, dramatically reducing cold-cache time per job.

- [ ] **Step 1: Update fmt job**

Replace lines 27–35 (the `fmt:` job's `setup-rust` invocation) with:

```yaml
  fmt:
    name: rustfmt
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          components: rustfmt
          shared-key: linux-stable
      - run: cargo fmt --all -- --check
```

- [ ] **Step 2: Update clippy job**

Replace lines 37–45:

```yaml
  clippy:
    name: clippy
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          components: clippy
          shared-key: linux-stable
      - run: cargo clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 3: Update matrix test job**

Replace lines 47–71 with the same job but `shared-key` instead of `cache-key`:

```yaml
  test:
    name: test (${{ matrix.os }} / ${{ matrix.toolchain }})
    runs-on: ${{ matrix.os }}
    continue-on-error: ${{ matrix.os == 'windows-latest' }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        toolchain: [stable, "1.91"]
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          toolchain: ${{ matrix.toolchain }}
          shared-key: ${{ matrix.os }}-${{ matrix.toolchain }}
      - run: cargo test --workspace --all-targets
      - run: cargo test --workspace --doc
      - if: matrix.os == 'ubuntu-latest'
        run: cargo test -p tau-sandbox-native --features integration-tests --tests -- --ignored
      - if: matrix.os == 'ubuntu-latest'
        run: cargo test -p tau-sandbox-container --tests -- --ignored
      - if: matrix.os == 'ubuntu-latest'
        run: cargo test -p tau-runtime --features integration-tests --tests -- --ignored
```

(NOTE: the `cargo test` invocations remain — A3 converts them to `cargo nextest run`. The `-- --ignored` block remains — Phase B's B1 removes it.)

- [ ] **Step 4: Update all 19 remaining Linux-only default-toolchain jobs**

For each of these jobs, add `with: { shared-key: linux-stable }` to the `setup-rust` invocation:

`no-default-features`, `no-default-features-ports`, `test-fixtures-ports`, `no-default-features-pkg`, `no-default-features-runtime`, `no-default-features-cli`, `no-default-features-protocol`, `no-default-features-sdk`, `build-tau-plugins`, `build-anthropic-plugin`, `build-ollama-plugin`, `build-openai-plugin`, `build-fs-read-plugin`, `build-shell-plugin`, `build-tau-plugin-test-support`, `build-tau-plugin-conformance`, `test-conformance`, `build-tau-plugin-compat`, `test-tau-plugin-compat`, `test-tau-sandbox-native-e2e`, `test-tau-runtime-e2e`.

Pattern:

```yaml
  <job-name>:
    name: <name>
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: linux-stable
      - ...
```

- [ ] **Step 5: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`
Expected: no output (success).

- [ ] **Step 6: Verify every setup-rust call has shared-key**

Run: `grep -c "shared-key:" .github/workflows/ci.yml`
Expected: ≥ 22 (one per job invocation).

Run: `grep -c "cache-key:" .github/workflows/ci.yml`
Expected: 0 (all callers migrated; the deprecated alias in setup-rust action is no longer used).

- [ ] **Step 7: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: migrate all jobs to shared-key for cross-job cache sharing

Sub-project E Phase A Task 2. Every setup-rust invocation now passes
shared-key per (os, toolchain). All Linux-stable jobs share one cache
entry; matrix entries share per-(os, toolchain) entries. Eliminates
the per-job key fragmentation that prevented cache reuse across jobs.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task A3: Adopt `cargo nextest` for all `cargo test` invocations except doctests

**Files:**
- Modify: `.github/workflows/ci.yml` (every `cargo test` step)

**Context:** Up to 3× faster on workspaces with many test binaries. Doctests stay on `cargo test --doc` because nextest doctest support is incomplete. Every job that runs `cargo test` needs `with-nextest: true` added to its setup-rust invocation AND its `cargo test` swapped for `cargo nextest run`.

Jobs that run `cargo test`:
- `test` (matrix) — 2 plain `cargo test` + 3 conditional `cargo test ... -- --ignored` + 1 `cargo test --doc`
- `no-default-features` (tau-domain), `no-default-features-ports`, `no-default-features-pkg`, `no-default-features-runtime`, `no-default-features-cli`, `no-default-features-protocol`, `no-default-features-sdk` — each runs `cargo test --no-default-features --lib`
- `test-fixtures-ports` — runs `cargo test -p tau-ports --features test-fixtures`
- `build-tau-plugin-test-support` — runs `cargo test -p tau-plugin-test-support --all-targets`
- `test-conformance` — runs 3 × `cargo test -p X --test conformance -- --nocapture`
- `test-tau-plugin-compat` — runs `cargo test -p tau-plugin-compat --features integration-tests --tests`
- `test-tau-sandbox-native-e2e` — runs `cargo test -p tau-sandbox-native --features integration-tests --tests`
- `test-tau-runtime-e2e` — runs `cargo test -p tau-runtime --features integration-tests --tests`

Jobs that don't need nextest (only `cargo build` or `cargo fmt`/`clippy`): `fmt`, `clippy`, `build-tau-plugins`, `build-{anthropic,ollama,openai,fs-read,shell}-plugin`, `build-tau-plugin-conformance`, `build-tau-plugin-compat`.

- [ ] **Step 1: Add `with-nextest: true` to all 13 test-running jobs**

Add `with-nextest: true` to the `with:` block of each `setup-rust` invocation in the 13 jobs above.

Example for the matrix `test:` job:

```yaml
      - uses: ./.github/actions/setup-rust
        with:
          toolchain: ${{ matrix.toolchain }}
          shared-key: ${{ matrix.os }}-${{ matrix.toolchain }}
          with-nextest: true
```

- [ ] **Step 2: Convert `cargo test` to `cargo nextest run` (matrix test job)**

In the matrix `test:` job, replace:

```yaml
      - run: cargo test --workspace --all-targets
      - run: cargo test --workspace --doc
      - if: matrix.os == 'ubuntu-latest'
        run: cargo test -p tau-sandbox-native --features integration-tests --tests -- --ignored
      - if: matrix.os == 'ubuntu-latest'
        run: cargo test -p tau-sandbox-container --tests -- --ignored
      - if: matrix.os == 'ubuntu-latest'
        run: cargo test -p tau-runtime --features integration-tests --tests -- --ignored
```

With:

```yaml
      - run: cargo nextest run --workspace --all-targets
      - run: cargo test --workspace --doc
      - if: matrix.os == 'ubuntu-latest'
        run: cargo nextest run -p tau-sandbox-native --features integration-tests --tests --run-ignored only
      - if: matrix.os == 'ubuntu-latest'
        run: cargo nextest run -p tau-sandbox-container --tests --run-ignored only
      - if: matrix.os == 'ubuntu-latest'
        run: cargo nextest run -p tau-runtime --features integration-tests --tests --run-ignored only
```

Note: `cargo test ... -- --ignored` becomes `cargo nextest run ... --run-ignored only` (nextest's equivalent flag). The doctest line stays on `cargo test --doc`.

- [ ] **Step 3: Convert `cargo test` to `cargo nextest run` in 7 no-default-features jobs**

For each of `no-default-features`, `no-default-features-ports`, `no-default-features-pkg`, `no-default-features-runtime`, `no-default-features-cli`, `no-default-features-protocol`, `no-default-features-sdk`, replace the test step:

```yaml
      - name: Test <crate> (no default features)
        run: cargo test -p <crate> --no-default-features --lib
```

With:

```yaml
      - name: Test <crate> (no default features)
        run: cargo nextest run -p <crate> --no-default-features --lib
```

- [ ] **Step 4: Convert `cargo test` in test-fixtures-ports**

Replace:
```yaml
      - name: Test tau-ports (test-fixtures feature only)
        run: cargo test -p tau-ports --features test-fixtures
```
With:
```yaml
      - name: Test tau-ports (test-fixtures feature only)
        run: cargo nextest run -p tau-ports --features test-fixtures
```

- [ ] **Step 5: Convert `cargo test` in build-tau-plugin-test-support**

Replace:
```yaml
      - name: Test tau-plugin-test-support
        run: cargo test -p tau-plugin-test-support --all-targets
```
With:
```yaml
      - name: Test tau-plugin-test-support
        run: cargo nextest run -p tau-plugin-test-support --all-targets
```

- [ ] **Step 6: Convert `cargo test` in test-conformance**

Replace:
```yaml
      - name: Run conformance suite against all 3 plugins
        run: |
          cargo test -p anthropic --test conformance -- --nocapture
          cargo test -p ollama    --test conformance -- --nocapture
          cargo test -p openai    --test conformance -- --nocapture
```
With:
```yaml
      - name: Run conformance suite against all 3 plugins
        run: |
          cargo nextest run -p anthropic --test conformance --no-capture
          cargo nextest run -p ollama    --test conformance --no-capture
          cargo nextest run -p openai    --test conformance --no-capture
```

(Note: `cargo test -- --nocapture` becomes `cargo nextest run --no-capture` — different flag spelling.)

- [ ] **Step 7: Convert `cargo test` in 3 e2e jobs**

In `test-tau-plugin-compat`, `test-tau-sandbox-native-e2e`, `test-tau-runtime-e2e`, replace each `cargo test ...` line:

```yaml
        run: cargo test -p <crate> --features integration-tests --tests
```
With:
```yaml
        run: cargo nextest run -p <crate> --features integration-tests --tests
```

- [ ] **Step 8: Verify all conversions done**

Run: `grep -nE "^\s+(- )?run: cargo test " .github/workflows/ci.yml`
Expected: ONLY one match — the `cargo test --workspace --doc` line in the matrix test job.

- [ ] **Step 9: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`
Expected: no output.

- [ ] **Step 10: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: adopt cargo nextest for all test jobs (doctests stay on cargo test)

Sub-project E Phase A Task 3. Replaces cargo test with cargo nextest run
in 13 jobs. Doctests stay on cargo test --workspace --doc because
nextest doctest support is incomplete. Translates -- --ignored to
--run-ignored only and -- --nocapture to --no-capture per nextest's
flag spelling.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task A4: Enable sccache on test/check jobs (NOT release builds)

**Files:**
- Modify: `.github/workflows/ci.yml`

**Context:** Sccache caches individual rustc invocations. Per the spec's research finding, sccache *speeds* test/check builds 11–14% but can *slow* release builds up to 50% — apply only to test/check jobs.

Jobs that get `with-sccache: true`:
- All 13 jobs that already got `with-nextest: true` in Task A3
- `clippy` (runs `cargo clippy` which is a check operation)
- `fmt` does NOT need sccache (no compilation)

Jobs that do NOT get `with-sccache: true` (release builds):
- `build-tau-plugins`, `build-anthropic-plugin`, `build-ollama-plugin`, `build-openai-plugin`, `build-fs-read-plugin`, `build-shell-plugin`
- `build-tau-plugin-conformance`, `build-tau-plugin-compat` (these run `cargo build` in dev mode, not release; treat as test-like and ENABLE sccache)
- `test-tau-plugin-compat` runs `cargo build -p tau-cli` (dev) and 5 `cargo build --release -p X-plugin` — mixed; the release builds dominate. Set `with-sccache: false` here. The downstream e2e jobs in Phase C download artifacts, so this concern goes away.

Decision matrix:

| Job | with-sccache |
|---|---|
| `fmt` | false |
| `clippy` | true |
| matrix `test` | true |
| 7 × no-default-features | true |
| `test-fixtures-ports` | true |
| `build-tau-plugin-test-support` | true (cargo test runs) |
| `build-tau-plugin-conformance` | true (cargo build dev) |
| `test-conformance` | true |
| `build-tau-plugin-compat` | true (cargo build dev) |
| `test-tau-plugin-compat` | false (mixed; phase C refactors) |
| `test-tau-sandbox-native-e2e` | false (release build of controlled-env; phase C refactors) |
| `test-tau-runtime-e2e` | false (same as above) |
| `build-tau-plugins`, `build-{anthropic,ollama,openai,fs-read,shell}-plugin` | false (release builds) |

- [ ] **Step 1: Add `with-sccache: true` to qualifying jobs**

For each of: `clippy`, `test` (matrix), 7 × `no-default-features-*` jobs, `test-fixtures-ports`, `build-tau-plugin-test-support`, `build-tau-plugin-conformance`, `test-conformance`, `build-tau-plugin-compat` — add `with-sccache: true` to the `setup-rust` invocation's `with:` block.

Example for clippy:

```yaml
      - uses: ./.github/actions/setup-rust
        with:
          components: clippy
          shared-key: linux-stable
          with-sccache: true
```

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: enable sccache on test/check jobs (not release builds)

Sub-project E Phase A Task 4. Adds with-sccache: true to 13 jobs that
benefit from sccache's per-rustc-call cache. Release-build jobs and
the 3 e2e jobs (which currently do release builds; refactored in
Phase C) keep sccache disabled per the spec's tooling decision matrix.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task A5: Enable mold linker on all Linux jobs

**Files:**
- Modify: `.github/workflows/ci.yml`

**Context:** Mold offers ~10× linker speed on Linux. Adds `with-mold: true` to every Linux job. The composite action conditionally installs only on Linux runners; on macOS/Windows it's a no-op.

Linux jobs (every job in current ci.yml is `runs-on: ubuntu-latest` EXCEPT the matrix `test:` which has 3-OS matrix; the matrix entry's mold install is conditional inside the composite action):
- All 22 non-matrix jobs are `ubuntu-latest`.
- Matrix `test:` adds `with-mold: true`; composite action's `if: runner.os == 'Linux'` skips install on macOS/Windows.

- [ ] **Step 1: Add `with-mold: true` to ALL 22 setup-rust invocations**

For every job (including the matrix `test:`), add `with-mold: true` to the `with:` block.

Example for fmt:
```yaml
      - uses: ./.github/actions/setup-rust
        with:
          components: rustfmt
          shared-key: linux-stable
          with-mold: true
```

Note: `fmt` doesn't compile, so mold has no effect — but adding it is harmless and keeps job invocations uniform.

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: enable mold linker on Linux jobs

Sub-project E Phase A Task 5. Adds with-mold: true to all 22 setup-rust
invocations. Composite action's runner.os == 'Linux' guard skips
install on macOS/Windows runners (matrix test entries). RUSTFLAGS
gets -C link-arg=-fuse-ld=mold injected on Linux only.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task A6 (USER GATE): Open Phase A PR

**Files:** none (PR description only)

- [ ] **Step 1: Push branch and open draft PR**

```bash
git push -u origin feat/ci-optimization-spec
gh pr create --draft --title "ci(opt-A): tier 1 wins — shared cache + nextest + sccache + mold" --body "$(cat <<'EOF'
## Phase A — Tier 1 wins

Sub-project E (CI optimization) Phase A. Adopts cross-job cache sharing,
cargo-nextest, sccache, and mold linker without renaming any check.
Branch protection unchanged.

## Summary

- `setup-rust` composite action gains `shared-key`, `with-nextest`,
  `with-sccache`, `with-mold` inputs (Task A1).
- All 22 jobs migrated to `shared-key` per (os, toolchain) (Task A2).
- All `cargo test` invocations replaced with `cargo nextest run` except
  doctests (Task A3).
- sccache enabled on test/check jobs (NOT release builds) (Task A4).
- mold linker enabled on Linux jobs (Task A5).

## Branch protection

**No update needed.** All 29 existing check names are emitted
unchanged; PRs just get faster.

## Test plan

- [ ] All 29 checks green on this PR
- [ ] Wall-clock < previous PR's CI time (compare in PR Actions tab)
- [ ] Phase A is its own canary; if green, the tools work

## Spec

`docs/superpowers/specs/2026-05-06-ci-optimization-design.md`
EOF
)"
```

- [ ] **Step 2: Wait for CI green, then mark ready for review**

Monitor CI run. When all 29 checks pass:
```bash
gh pr ready
```

- [ ] **Step 3: PAUSE — user reviews + merges Phase A**

User reviews the PR. User merges (no branch-protection update needed). Implementation halts here until user explicitly approves continuing to Phase B.

---

# Phase B — Test matrix split + Windows hard gate

Renames 6 matrix entries. Removes `-- --ignored` block. Upgrades Windows from advisory (`continue-on-error: true`) to hard gate.

**Phase B prerequisites:** Phase A merged on main. Branch `feat/ci-opt-B` cut from latest main.

---

### Task B1: Split matrix `test:` into `test-stable:` + `msrv-check:`

**Files:**
- Modify: `.github/workflows/ci.yml` (lines 47–71 in pre-Phase-B state)

**Summary:** Replace the single matrix `test:` job with two new jobs:
- `test-stable:` matrix on `os` only, runs full nextest + doctest, no `-- --ignored` block.
- `msrv-check:` matrix on `os` only, toolchain pinned to `1.91`, runs `cargo check --workspace --all-targets --locked`.

LOC: ~50 lines (removes ~25; adds ~50).

- [ ] **Step 1: Replace `test:` job block**

Locate the `test:` job (was lines 47–71 in pre-A state; line numbers may differ after Phase A's edits). Replace the entire block with:

```yaml
  test-stable:
    name: test-stable / ${{ matrix.os == 'ubuntu-latest' && 'linux' || matrix.os == 'macos-latest' && 'macos' || 'windows' }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          toolchain: stable
          shared-key: ${{ matrix.os }}-stable
          with-nextest: true
          with-sccache: true
          with-mold: true
      - run: cargo nextest run --workspace --all-targets
      - run: cargo test --workspace --doc

  msrv-check:
    name: msrv-check / ${{ matrix.os == 'ubuntu-latest' && 'linux' || matrix.os == 'macos-latest' && 'macos' || 'windows' }}
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          toolchain: "1.91"
          shared-key: ${{ matrix.os }}-1.91
          with-sccache: true
          with-mold: true
      - run: cargo check --workspace --all-targets --locked
```

Notes:
- The `-- --ignored` integration test block is REMOVED. The dedicated `test-tau-sandbox-native-e2e`, `test-tau-runtime-e2e`, `test-tau-plugin-compat` jobs cover that scope properly with `--features integration-tests`.
- **`continue-on-error: true` on Windows is gone** — neither the new `test-stable:` nor `msrv-check:` job specifies `continue-on-error:`, so it defaults to false. This is the W2 strictness upgrade. Do NOT add `continue-on-error:` lines back; Windows is now a hard gate.
- The check name template uses Bash-style ternary in GHA expression syntax to translate `ubuntu-latest` → `linux`, etc.

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Verify check names will resolve**

Search for the new check names that branch protection will need:
```
test-stable / linux
test-stable / macos
test-stable / windows
msrv-check / linux
msrv-check / macos
msrv-check / windows
```

Run: `grep -nE 'name: (test-stable|msrv-check)' .github/workflows/ci.yml`
Expected: 2 matches (the `name:` template lines).

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: split test matrix into test-stable + msrv-check, drop --ignored block

Sub-project E Phase B Task 1. Replaces the single matrix test: job with
test-stable: (full nextest + doctest, stable toolchain) and msrv-check:
(cargo check only, 1.91 toolchain). Removes the -- --ignored integration
test block; the dedicated e2e jobs cover that scope properly. Drops
continue-on-error so Windows is no longer auto-passing.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task B2: Pre-merge Windows test-history audit

**Files:**
- Possibly modify: Rust source files (`#[cfg(not(target_os = "windows"))]` skips for known-flaky tests)

**Summary:** BEFORE Phase B's PR is merged, run 10 consecutive CI runs on the Phase B draft PR. Document Windows test pass/fail history. Any consistently-failing Windows tests must be either fixed or skipped via `cfg(not(target_os = "windows"))` BEFORE flipping `continue-on-error: false`.

LOC: 0 if Windows is already clean; up to 50 LOC of Rust skip-cfgs if not.

- [ ] **Step 1: Push the B1 commit to a draft PR and run CI 10×**

```bash
git push origin feat/ci-opt-B
gh pr create --draft --title "ci(opt-B): test matrix split + Windows hard gate (audit phase)" --body "AUDIT: rerun CI 10x to check Windows stability before merging."
```

For each of 10 runs (use empty commits or `gh workflow run`):

```bash
for i in {1..10}; do
  git commit --allow-empty -m "ci: audit run $i"
  git push
  sleep 60  # let CI start
done
```

- [ ] **Step 2: Monitor and tabulate Windows results**

Run:
```bash
gh run list --branch feat/ci-opt-B --workflow=ci.yml --limit 10 --json databaseId,conclusion,headSha
```

For each run, check the Windows entries:
```bash
gh run view <run-id> --json jobs | jq '.jobs[] | select(.name | contains("windows")) | {name, conclusion}'
```

Tabulate: how many of 10 Windows runs passed? List any tests that failed.

- [ ] **Step 3: Decision branch**

- **All 10 Windows runs pass:** proceed to Task B3.
- **1–2 spurious failures (different tests each time, no pattern):** flake — proceed but note in PR description; consider re-running.
- **Consistent failure on a specific test:** that test must be fixed or skipped. Add `#[cfg(not(target_os = "windows"))]` in a separate commit, re-run audit.

For skip-cfg pattern, in the relevant test file:

```rust
#[cfg(not(target_os = "windows"))]
#[test]
fn flaky_on_windows() {
    // ...
}
```

Or for a whole module:
```rust
#[cfg(not(target_os = "windows"))]
mod windows_flaky_tests {
    // ...
}
```

- [ ] **Step 4: Commit any Windows skips**

```bash
git add <relevant test files>
git commit -m "test: skip <test-name> on Windows pending sub-project follow-up

Sub-project E Phase B Task 2. <test-name> consistently fails on
Windows (audit: <X>/10 runs failed). Skipped via
cfg(not(target_os = 'windows')) before promoting Windows to a hard
CI gate. Tracked for follow-up at <issue/note>.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task B3: Open Phase B PR (audit complete)

**Files:** PR description.

- [ ] **Step 1: Update PR title and body**

```bash
gh pr edit <PR#> --title "ci(opt-B): test matrix split + Windows hard gate" --body "$(cat <<'EOF'
## Phase B — Matrix split + Windows hard gate

Sub-project E (CI optimization) Phase B.

## Summary

- Matrix `test:` job split into `test-stable:` (full nextest + doctest)
  and `msrv-check:` (cargo check only).
- `-- --ignored` integration test block removed from matrix entries —
  dedicated e2e jobs cover that scope properly.
- Windows promoted from advisory (`continue-on-error: true`) to hard
  gate. (W2 strictness upgrade per spec.)

## Windows audit

10 consecutive CI runs against this branch:
- `test-stable / windows`: <X>/10 passed
- `msrv-check / windows`: <X>/10 passed

[Document any flakes resolved as `cfg(not(target_os = "windows"))` skips.]

## Branch protection update required

**Remove from required checks:**
- test (ubuntu-latest / stable)
- test (macos-latest / stable)
- test (windows-latest / stable)
- test (ubuntu-latest / 1.91)
- test (macos-latest / 1.91)
- test (windows-latest / 1.91)

**Add to required checks:**
- test-stable / linux
- test-stable / macos
- test-stable / windows
- msrv-check / linux
- msrv-check / macos
- msrv-check / windows

Net: 6 in, 6 out. Still 29 checks total.

## Test plan

- [ ] All 6 new check names present and green
- [ ] Old `test (X / Y)` names absent
- [ ] Open PRs after merge will need rebase (note in PR description)

## Spec

`docs/superpowers/specs/2026-05-06-ci-optimization-design.md`
EOF
)"
```

- [ ] **Step 2: Mark PR ready for review**

```bash
gh pr ready
```

- [ ] **Step 3: PAUSE — user reviews + audits + merges**

User reviews PR, audits Windows results, **manually updates branch protection** (the 6-in/6-out swap above), then merges.

---

# Phase C — build-fixtures + e2e refactor

Adds `build-fixtures-linux:` job. Refactors 4 e2e jobs to download artifacts instead of self-building.

**Phase C prerequisites:** Phase B merged on main. Branch `feat/ci-opt-C` cut from latest main.

---

### Task C1: Add `build-fixtures-linux:` job

**Files:**
- Modify: `.github/workflows/ci.yml` (add new job, ~40 lines)

**Summary:** New job builds 5 real plugins + 2 toy plugins + tau-cli + controlled-env binary in release mode. Uploads as `linux-fixture-binaries` artifact.

LOC: +40.

- [ ] **Step 1: Add the build-fixtures-linux job**

Append this job to ci.yml (after the existing `clippy:` job is a sensible spot; placement is cosmetic since GHA builds the DAG from `needs:`):

```yaml
  build-fixtures-linux:
    name: build-fixtures / linux
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: linux-stable
          with-mold: true
      - name: Build all release-mode binaries
        run: |
          cargo build --release \
            -p anthropic -p ollama -p openai \
            -p fs-read -p shell \
            -p echo-llm -p echo-tool \
            -p tau-cli
      - name: Build controlled-env binary
        run: |
          cargo build --release \
            --manifest-path crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml
      - name: Upload fixture binaries
        uses: actions/upload-artifact@v4
        with:
          name: linux-fixture-binaries
          retention-days: 1
          path: |
            target/release/anthropic
            target/release/ollama
            target/release/openai
            target/release/fs-read
            target/release/shell
            target/release/echo-llm
            target/release/echo-tool
            target/release/tau
            crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env
```

Note: no `with-sccache: true` because release builds; rust-cache restores `target/release` from main's most recent run.

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add build-fixtures-linux job that prebuilds plugins + tau-cli + controlled-env

Sub-project E Phase C Task 1. New job builds all 9 binaries needed
by the 4 Linux e2e jobs in one cargo invocation, uploads them as the
linux-fixture-binaries artifact. Downstream jobs (Phase C tasks 2-5)
download instead of self-building, eliminating ~9 min of redundant
compilation per PR.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task C2: Refactor `test-tau-plugin-compat:` to download artifact

**Files:**
- Modify: `.github/workflows/ci.yml` (the `test-tau-plugin-compat:` job)

**Summary:** Add `needs: build-fixtures-linux`. Replace cargo-build steps with download-artifact + mv into expected paths. Keep the cargo-build path commented out as fallback (removed in Task C7).

LOC: ~30 net change.

- [ ] **Step 1: Replace the job body**

Find the `test-tau-plugin-compat:` job. Replace its body with:

```yaml
  test-tau-plugin-compat:
    name: test (tau-plugin-compat / linux)
    needs: build-fixtures-linux
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: linux-stable
          with-nextest: true
          with-mold: true
      - name: Download fixture binaries
        uses: actions/download-artifact@v4
        with:
          name: linux-fixture-binaries
          path: ./prebuilt
      - name: Place binaries at cargo-expected paths
        run: |
          mkdir -p target/release
          mv ./prebuilt/anthropic ./prebuilt/ollama ./prebuilt/openai \
             ./prebuilt/fs-read ./prebuilt/shell \
             ./prebuilt/echo-llm ./prebuilt/echo-tool \
             ./prebuilt/tau \
             target/release/
          mkdir -p crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release
          mv ./prebuilt/tau-controlled-env \
             crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/
          # Touch to bump mtime above source files so cargo's freshness
          # check doesn't decide to rebuild.
          touch target/release/* \
                crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env
      # FALLBACK (removed in Task C7 once green):
      # - name: Build tau binary first
      #   run: cargo build -p tau-cli --bin tau
      # - name: Build all real plugin binaries (Layer 4 tests)
      #   run: |
      #     cargo build -p anthropic --release
      #     cargo build -p ollama --release
      #     cargo build -p openai --release
      #     cargo build -p fs-read --release
      #     cargo build -p shell --release
      - name: Test tau-plugin-compat (integration-tests feature)
        run: cargo nextest run -p tau-plugin-compat --features integration-tests --tests --verbose
```

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: refactor test-tau-plugin-compat to download fixture artifacts

Sub-project E Phase C Task 2. Replaces in-job cargo build with
actions/download-artifact + mv into target/release/. Touches binaries
to bump mtime above source files so cargo's freshness check doesn't
recompile. Fallback build path commented out; removed in Task C7
once Phase C is green.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task C3: Refactor `test-tau-sandbox-native-e2e:` to download artifact

**Files:**
- Modify: `.github/workflows/ci.yml` (the `test-tau-sandbox-native-e2e:` job)

**Summary:** Same pattern as C2 but only needs the controlled-env binary.

LOC: ~25 net change.

- [ ] **Step 1: Replace the job body**

```yaml
  test-tau-sandbox-native-e2e:
    name: test (tau-sandbox-native e2e / linux)
    needs: build-fixtures-linux
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: linux-stable
          with-nextest: true
          with-mold: true
      - name: Download fixture binaries
        uses: actions/download-artifact@v4
        with:
          name: linux-fixture-binaries
          path: ./prebuilt
      - name: Place controlled-env binary
        run: |
          mkdir -p crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release
          mv ./prebuilt/tau-controlled-env \
             crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/
          touch crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env
      # FALLBACK (removed in Task C7):
      # - name: Build controlled-env binary
      #   run: cargo build --manifest-path crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml --release
      - name: Test tau-sandbox-native e2e
        run: cargo nextest run -p tau-sandbox-native --features integration-tests --tests --verbose
```

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: refactor test-tau-sandbox-native-e2e to download fixture artifacts

Sub-project E Phase C Task 3. Same artifact-download pattern as Task C2
but only needs the controlled-env binary.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task C4: Refactor `test-tau-runtime-e2e:` to download artifact

**Files:**
- Modify: `.github/workflows/ci.yml` (the `test-tau-runtime-e2e:` job)

**Summary:** Identical to C3.

LOC: ~25 net change.

- [ ] **Step 1: Replace the job body**

```yaml
  test-tau-runtime-e2e:
    name: test (tau-runtime e2e / linux)
    needs: build-fixtures-linux
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: linux-stable
          with-nextest: true
          with-mold: true
      - name: Download fixture binaries
        uses: actions/download-artifact@v4
        with:
          name: linux-fixture-binaries
          path: ./prebuilt
      - name: Place controlled-env binary
        run: |
          mkdir -p crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release
          mv ./prebuilt/tau-controlled-env \
             crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/
          touch crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env
      # FALLBACK (removed in Task C7):
      # - name: Build controlled-env binary
      #   run: cargo build --manifest-path crates/tau-plugin-compat/fixtures/controlled-env-binary/Cargo.toml --release
      - name: Test tau-runtime e2e
        run: cargo nextest run -p tau-runtime --features integration-tests --tests --verbose
```

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: refactor test-tau-runtime-e2e to download fixture artifacts

Sub-project E Phase C Task 4. Same artifact-download pattern as Task C3.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task C5: Refactor `test-conformance:` to download artifact

**Files:**
- Modify: `.github/workflows/ci.yml` (the `test-conformance:` job)

**Summary:** Same artifact-download pattern. The job runs nextest against the 3 HTTP plugin crates' conformance tests; pre-built plugin binaries skip the rebuild.

LOC: ~30 net change.

- [ ] **Step 1: Replace the job body**

```yaml
  test-conformance:
    name: test (conformance)
    needs: build-fixtures-linux
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: linux-stable
          with-nextest: true
          with-sccache: true
          with-mold: true
      - name: Download fixture binaries
        uses: actions/download-artifact@v4
        with:
          name: linux-fixture-binaries
          path: ./prebuilt
      - name: Place plugin binaries
        run: |
          mkdir -p target/release
          mv ./prebuilt/anthropic ./prebuilt/ollama ./prebuilt/openai \
             target/release/
          touch target/release/anthropic target/release/ollama target/release/openai
      - name: Run conformance suite against all 3 plugins
        run: |
          cargo nextest run -p anthropic --test conformance --no-capture
          cargo nextest run -p ollama    --test conformance --no-capture
          cargo nextest run -p openai    --test conformance --no-capture
```

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: refactor test-conformance to download fixture artifacts

Sub-project E Phase C Task 5. Conformance tests for the 3 HTTP plugins
now use prebuilt plugin binaries from build-fixtures-linux instead of
recompiling.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task C6 (USER GATE): Open Phase C PR

**Files:** PR description only.

- [ ] **Step 1: Push branch and open PR**

```bash
git push origin feat/ci-opt-C
gh pr create --title "ci(opt-C): build-fixtures + e2e refactor" --body "$(cat <<'EOF'
## Phase C — build-fixtures + e2e refactor

Sub-project E (CI optimization) Phase C.

## Summary

- New job `build-fixtures-linux` builds 9 binaries (5 plugins + 2 toy
  plugins + tau-cli + controlled-env) once, uploads as artifact.
- 4 e2e/conformance jobs refactored to download the artifact instead
  of self-building. Eliminates ~9 min of redundant compilation per PR.

## Branch protection update required

**Add to required checks:**
- build-fixtures / linux

(No removals; the 4 e2e check names stay the same.)

Net: 29 → 30 transient. Phase D drops 6 plugin builds; Phase E
consolidates the no-default-features jobs to land at 18.

## Fallback path

The cargo-build fallback paths in 4 e2e jobs are commented out, not
removed, so artifact-download bugs can be diagnosed without breaking
CI. Task C7 removes them once Phase C is green on main.

## Test plan

- [ ] `build-fixtures / linux` uploads `linux-fixture-binaries` with all 9 binaries
- [ ] All 4 downstream jobs successfully download + place binaries
- [ ] Cargo `--verbose` shows no recompilation in downstream jobs

## Spec

`docs/superpowers/specs/2026-05-06-ci-optimization-design.md`
EOF
)"
```

- [ ] **Step 2: PAUSE — user reviews + updates branch protection + merges**

User adds `build-fixtures / linux` to branch-protection required-checks list, then merges.

---

### Task C7: Remove fallback paths

**Files:**
- Modify: `.github/workflows/ci.yml`

**Summary:** Once Phase C is merged and at least 2 PRs have run successfully on main, remove the commented-out fallback paths in the 4 refactored jobs.

LOC: -20.

- [ ] **Step 1: Cut a follow-up branch**

```bash
git checkout main && git pull
git checkout -b feat/ci-opt-C-cleanup
```

- [ ] **Step 2: Remove the FALLBACK comment blocks**

In each of `test-tau-plugin-compat`, `test-tau-sandbox-native-e2e`, `test-tau-runtime-e2e`, `test-conformance` jobs, delete the `# FALLBACK (removed in Task C7):` comment block.

- [ ] **Step 3: Commit and open PR**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: remove Phase C fallback build paths now that artifact path is proven

Sub-project E Phase C Task 7. The artifact-download pattern is stable;
remove the commented-out cargo-build fallback paths. Pure cleanup.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
git push -u origin feat/ci-opt-C-cleanup
gh pr create --title "ci(opt-C-cleanup): remove fallback build paths from refactored e2e jobs"
```

User reviews + merges.

---

# Phase D — Plugin release fold

Drops 6 redundant plugin build jobs (5 real plugins + 1 toy plugins). Coverage absorbed by `build-fixtures-linux`.

**Phase D prerequisites:** Phase C cleanup merged on main. Branch `feat/ci-opt-D` cut from latest main.

---

### Task D1: Delete 6 plugin build jobs

**Files:**
- Modify: `.github/workflows/ci.yml`

**Summary:** Delete `build-tau-plugins`, `build-anthropic-plugin`, `build-ollama-plugin`, `build-openai-plugin`, `build-fs-read-plugin`, `build-shell-plugin` job blocks. Each is ~9 lines.

LOC: -54.

- [ ] **Step 1: Delete the 6 job blocks**

Locate each of the 6 job keys. Delete the entire block including the leading whitespace through the final blank line before the next job.

The 6 job keys to delete:
- `build-tau-plugins:`
- `build-anthropic-plugin:`
- `build-ollama-plugin:`
- `build-openai-plugin:`
- `build-fs-read-plugin:`
- `build-shell-plugin:`

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Verify the 6 jobs are gone**

Run: `grep -E "(build-tau-plugins|build-anthropic-plugin|build-ollama-plugin|build-openai-plugin|build-fs-read-plugin|build-shell-plugin):" .github/workflows/ci.yml`
Expected: no matches.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: drop 6 redundant plugin release-build jobs (absorbed by build-fixtures)

Sub-project E Phase D Task 1. The release-mode build coverage of all
5 real plugins (anthropic, ollama, openai, fs-read, shell) and the 2
toy plugins (echo-llm, echo-tool) is performed by build-fixtures-linux,
which is itself a required check. The 6 standalone build jobs were
duplicating that work.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task D2 (USER GATE): Open Phase D PR

- [ ] **Step 1: Push and open PR**

```bash
git push -u origin feat/ci-opt-D
gh pr create --title "ci(opt-D): drop 6 plugin release-build jobs absorbed by build-fixtures" --body "$(cat <<'EOF'
## Phase D — Plugin release fold

Sub-project E Phase D. Drops 6 redundant plugin release-build jobs.

## Branch protection update required

**Remove from required checks:**
- build (anthropic-plugin)
- build (ollama-plugin)
- build (openai-plugin)
- build (fs-read-plugin)
- build (shell-plugin)
- build (tau-plugins)

Net: -6 (30 → 24).

## Risk

Zero — `build-fixtures / linux` does the exact same work and is a
required check.

## Spec

`docs/superpowers/specs/2026-05-06-ci-optimization-design.md`
EOF
)"
```

- [ ] **Step 2: PAUSE — user reviews + updates branch protection + merges**

User removes the 6 check names, then merges.

---

# Phase E — Feature-flag-matrix + test-fixtures-ports rename

Adds `feature-flag-matrix:` job. Renames `test-fixtures-ports` check name. Drops 7 `--no-default-features` jobs (5 explicit + 2 misnamed).

**Phase E prerequisites:** Phase D merged on main. Branch `feat/ci-opt-E` cut from latest main.

---

### Task E1: Add `feature-flag-matrix:` job

**Files:**
- Modify: `.github/workflows/ci.yml`

**Summary:** New job loops `cargo check -p X --no-default-features` over 7 crates with `::group::<crate>` markers for log readability.

LOC: +25.

- [ ] **Step 1: Add the feature-flag-matrix job**

Append to ci.yml (placement cosmetic):

```yaml
  feature-flag-matrix:
    name: feature-flag-matrix / linux
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup-rust
        with:
          shared-key: linux-stable
          with-sccache: true
          with-mold: true
      - name: Check each crate with --no-default-features
        run: |
          set -e
          for crate in tau-domain tau-ports tau-pkg tau-runtime tau-cli tau-plugin-protocol tau-plugin-sdk; do
            echo "::group::$crate"
            cargo check -p "$crate" --no-default-features
            echo "::endgroup::"
          done
```

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add feature-flag-matrix job consolidating 7 no-default-features checks

Sub-project E Phase E Task 1. Single job loops cargo check
--no-default-features over 7 crates with ::group::<crate> log markers
preserving per-crate failure attribution.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task E2: Rename `test-fixtures-ports` check name

**Files:**
- Modify: `.github/workflows/ci.yml`

**Summary:** Change the user-visible name from `test (tau-ports test-fixtures only)` to `test-fixtures-ports / linux` for naming consistency with the new check-name conventions. The job key `test-fixtures-ports:` already matches; only the `name:` line changes.

LOC: 1 line.

- [ ] **Step 1: Update the name field**

Find the `test-fixtures-ports:` job. Change:

```yaml
  test-fixtures-ports:
    name: test (tau-ports test-fixtures only)
```

To:

```yaml
  test-fixtures-ports:
    name: test-fixtures-ports / linux
```

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: rename 'test (tau-ports test-fixtures only)' to 'test-fixtures-ports / linux'

Sub-project E Phase E Task 2. Aligns the check name with the new
naming convention (kebab-case + / linux suffix).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task E3: Delete 7 `--no-default-features` jobs

**Files:**
- Modify: `.github/workflows/ci.yml`

**Summary:** Delete 7 job blocks (5 explicit + 2 misnamed). Coverage absorbed by `feature-flag-matrix`.

LOC: -70 (each job is ~10 lines).

- [ ] **Step 1: Delete 7 job blocks**

Delete the entire block (key + name + steps + trailing blank line) for each of:

- `no-default-features:` (user-visible: `build (no-default-features)`; tests tau-domain)
- `no-default-features-ports:` (user-visible: `build (tau-ports no-default-features)`)
- `no-default-features-pkg:` (user-visible: `build (tau-pkg no-default-features)`)
- `no-default-features-runtime:` (user-visible: `build (tau-runtime no-default-features)`)
- `no-default-features-cli:` (user-visible: `build (tau-cli no-default-features)`)
- `no-default-features-protocol:` (user-visible: `build (tau-plugin-protocol)` — MISNAMED)
- `no-default-features-sdk:` (user-visible: `build (tau-plugin-sdk)` — MISNAMED)

- [ ] **Step 2: Verify YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`

- [ ] **Step 3: Verify all 7 are gone**

Run: `grep -E "no-default-features(-(ports|pkg|runtime|cli|protocol|sdk))?:" .github/workflows/ci.yml`
Expected: no matches.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: drop 7 --no-default-features jobs (absorbed by feature-flag-matrix)

Sub-project E Phase E Task 3. Removes the 5 explicit no-default-features
jobs plus the 2 misnamed ones (build (tau-plugin-protocol) and build
(tau-plugin-sdk) were YAML keys no-default-features-protocol/sdk in
disguise). Coverage moves to feature-flag-matrix.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task E4 (USER GATE): Open Phase E PR

- [ ] **Step 1: Push and open PR**

```bash
git push -u origin feat/ci-opt-E
gh pr create --title "ci(opt-E): feature-flag-matrix + test-fixtures-ports rename + drop 7 ndf jobs" --body "$(cat <<'EOF'
## Phase E — Feature-flag-matrix consolidation

Sub-project E Phase E. Final structural change.

## Branch protection update required

**Remove from required checks:**
- build (no-default-features)              ← actually tau-domain
- build (tau-ports no-default-features)
- build (tau-pkg no-default-features)
- build (tau-runtime no-default-features)
- build (tau-cli no-default-features)
- build (tau-plugin-protocol)               ← misnamed; was --no-default-features
- build (tau-plugin-sdk)                    ← misnamed; was --no-default-features
- test (tau-ports test-fixtures only)

**Add to required checks:**
- feature-flag-matrix / linux
- test-fixtures-ports / linux

Net: 8 removed, 2 added → -6. 24 → 18 final.

## Spec

`docs/superpowers/specs/2026-05-06-ci-optimization-design.md`
EOF
)"
```

- [ ] **Step 2: PAUSE — user reviews + updates branch protection + merges**

User makes the 8-out / 2-in branch-protection swap, then merges.

---

# Phase F — Documentation deliverables

Final task: capture the architecture decisions and update local-dev guidance.

---

### Task F1: Write ADR-0018 + update CLAUDE.md + update ROADMAP

**Files:**
- Create: `docs/decisions/0018-ci-optimization.md`
- Modify: `CLAUDE.md` (add `cargo nextest` recommendation if appropriate)
- Modify: `ROADMAP.md` (mark sub-project E shipped)

**Summary:** ADR captures the 10 decisions from the spec. CLAUDE.md gets a note about local nextest usage. ROADMAP gets a row.

LOC: ADR ~150, CLAUDE.md ~10, ROADMAP ~5.

- [ ] **Step 1: Write ADR-0018**

Create `docs/decisions/0018-ci-optimization.md`:

```markdown
# ADR-0018: CI optimization architecture

**Date:** 2026-05-06
**Status:** Accepted

## Context

After sub-project D shipped (commit 6c8be31), tau's CI gated main with
29 required checks running PRs in ~33 min wall-clock and ~85 min CI
compute. Sub-project D added Swatinem/rust-cache@v2 via the setup-rust
composite action plus CARGO_INCREMENTAL=0 workflow-level. Caching was
in place but jobs didn't share the cache (default per-job key) and
substantial duplicate work remained: 5 plugin release-build jobs + 1
toy-plugin release-build job duplicated work the matrix test already
did; 7 no-default-features jobs (5 explicit + 2 misnamed) checked
identical-shape feature-flag breakage 7 times; 4 Linux e2e jobs each
rebuilt plugins + tau-cli + the controlled-env fixture.

## Decision

Phased migration to 18 required checks via 5 PRs (Phases A–E), each
independently revertable. Conservative within-constraint optimization:
full e2e on every (OS × toolchain) combination preserved; no smoke-test
tier; no OS dropped; no PR-time check deferred to post-merge.

## Decisions recorded

1. **Conservative within-constraint optimization path.** Full e2e on
   every (OS × toolchain) combination preserved.
2. **W2: Windows upgrades from advisory to hard gate** as part of Phase B.
3. **MSRV is a compile-time contract**, verified by `cargo check`, not
   `cargo test`. Industry-standard pattern (tokio, serde, clap).
4. **Cargo `target/` is internal**; we upload only specific compiled
   binaries via `actions/upload-artifact`. Per-job rust-cache plus
   cross-job sccache covers the rest.
5. **No reusable workflow files** at this CI scale; the composite
   action absorbs duplication. Revisit at ~500-line ci.yml.
6. **Phased migration over big-bang**; each phase independently
   revertable.
7. **Phase A and Phase B are valid resting points** if later phases
   reveal problems.
8. **Required check count drops 29 → 18** by absorbing 6 plugin-release
   jobs into build-fixtures (Phase C/D) and 7 no-default-features
   jobs (5 explicit + 2 misnamed) into feature-flag-matrix (Phase E).
9. **Doctests stay on `cargo test --doc`** in test-stable / linux;
   nextest doctest support is incomplete.
10. **The matrix Linux `-- --ignored` integration test block was
    removed** in Phase B; the dedicated e2e jobs cover that scope
    properly with `--features integration-tests`.

## Consequences

**Positive:**
- PR wall-clock 33 → ≤25 min (~25% reduction targeted).
- CI compute 85 → ≤50 min (~40% reduction targeted).
- Required check count 29 → 18 (simpler branch-protection settings).
- Windows is now a real gate, not advisory.
- Cross-job cache sharing (rust-cache shared-key) plus sccache plus
  mold linker compounded for substantial speed wins.

**Negative:**
- feature-flag-matrix loses per-crate failure attribution at the
  GitHub Actions check-name level. Mitigated by `::group::<crate>`
  markers in the shell loop.
- Phase B's Windows hard-gate may surface previously-tolerated flakes;
  one-time audit + skip-cfgs may be required.

**Neutral:**
- No reusable workflow files introduced; the composite action absorbs
  the duplication.

## Alternatives considered

- **Self-hosted runners** — out of scope per cost.
- **Matrix slimming** (drop Windows or macOS) — rejected; user wanted
  real e2e on every platform.
- **Smoke-test tier on macOS/Windows** — rejected; user wanted real
  e2e on every platform.
- **Artifact-passing of `target/` directories** — rejected; cargo's
  target layout is internal and subject to change. Specific binary
  artifacts only.
- **`cargo-make`-style workflow consolidation** — rejected; cargo
  invocations stay readable in YAML.
- **`cargo-hakari` workspace-hack** — rejected; not worth the
  complexity at tau's workspace size.

## References

- Spec: `docs/superpowers/specs/2026-05-06-ci-optimization-design.md`
- Plan: `docs/superpowers/plans/2026-05-06-ci-optimization.md`
```

- [ ] **Step 2: Update CLAUDE.md to recommend nextest locally**

Add to `CLAUDE.md` (location: in the Rule 1 / Rule 2 section, before "Reference command shape"):

```markdown
## Rule 6: Prefer `cargo nextest` for tests

CI runs `cargo nextest run` everywhere except doctests. Using nextest
locally matches CI behavior more closely (per-test isolation, parallel
binary execution). Install once: `cargo install cargo-nextest --locked`.

For doctests, still use `cargo test --doc` — nextest doctest support is
incomplete.
```

- [ ] **Step 3: Update ROADMAP.md**

Add a row for sub-project E:

```markdown
| 12-E | CI optimization | Shipped 2026-05-06 |
```

(Adjust to match the existing ROADMAP table format; check `ROADMAP.md` for the actual schema.)

- [ ] **Step 4: Commit**

```bash
git add docs/decisions/0018-ci-optimization.md CLAUDE.md ROADMAP.md
git commit -m "docs: ADR-0018 (CI optimization) + CLAUDE.md nextest rule + ROADMAP

Sub-project E Phase F Task 1. ADR captures the 10 architectural
decisions from the spec. CLAUDE.md adds Rule 6 recommending nextest
locally. ROADMAP marks sub-project E shipped.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

### Task F2 (USER GATE): Final docs PR + squash-merge approval

- [ ] **Step 1: Push and open PR**

```bash
git push -u origin feat/ci-opt-F
gh pr create --title "docs(ci-opt): ADR-0018 + CLAUDE.md + ROADMAP" --body "Final docs deliverables for sub-project E (CI optimization).

ADR-0018 captures architecture decisions. CLAUDE.md adds nextest rule.
ROADMAP marks sub-project shipped."
```

- [ ] **Step 2: PAUSE — user reviews + squash-merges**

After CI green, user squash-merges. Sub-project E ships.

---

# Verification (end-to-end after Phase E ships)

After Phase E lands on main, verify:

| Metric | Baseline | Target | Verification |
|---|---|---|---|
| Wall-clock per PR | ~33 min | ≤ 25 min | Mean of 5 PRs from PR Actions tab |
| CI compute per PR | ~85 min | ≤ 50 min | Sum of "Billable time" across jobs |
| Required check count | 29 | 18 | Branch protection settings page |
| rust-cache hit rate | unmeasured | > 80% on PRs | rust-cache action logs |
| test-stable wall-clock | ~7 min | ~3-4 min | Per-job duration |
| Windows status | advisory | hard gate | Branch protection check history |

If any metric misses target, open a follow-up issue before considering sub-project E done.
