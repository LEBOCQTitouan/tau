# CI/CD audit pass — Implementation Plan

**Goal:** Apply the audit findings from 2026-05-14 to the CI/CD pipeline: security hardening, action upgrades, workflow restructure, test config improvements, and small cleanups. One PR, ten numbered changes, each its own commit.

**Architecture:** Three tiers of changes, ordered low-risk → high-risk so a failure mid-batch surfaces in the safest tier first:

1. **Hardening** (changes 1–5): permissions blocks, action version pins. Zero behavior change at the matrix level.
2. **Restructure** (changes 6–8): `paths-ignore` + summary job, build job merge, composite action extraction. Behavior unchanged; branch-protection list changes.
3. **Optimization** (changes 9–10): split doctests, split nextest profile. Behavior changes (where tests run, how flakes surface).

**Scope NOT included** (audit findings the user explicitly opted out of, or that need separate work):
- Shrink macOS/Windows `test-stable` coverage (audit O6) — user kept full coverage per ADR-0018 D1.
- Shrink pre-push deep gate (audit R5) — user kept full mirror.
- SHA-pin third-party actions (audit U15) — defer to a separate Dependabot wiring PR.

**Branch:** `feat/ci-audit-pass`, stacked on `feat/docs-link-audit` (PR #68), which is stacked on `feat/docs-publish` (PR #67). When the stack merges in order, this PR's base auto-retargets to `main`.

**Working directory:** `/Users/titouanlebocq/code/tau-worktrees/feat-ci-audit-pass`.

**Audit source:** the conversation that produced this plan. The full audit findings are in the chat transcript, not in the repo (no separate spec file — this plan IS the spec).

---

## Pre-flight

- [ ] Confirm worktree is on `feat/ci-audit-pass`, branched off `feat/docs-link-audit` (HEAD = `ce68220`), clean.
- [ ] All commits use `--no-verify` (lefthook gates would only test Rust, and this PR is CI/YAML-only).
- [ ] All commits authored as `titouanlebocq <lebocq.titouan@gmail.com>` via explicit `git -c user.email=... -c user.name=...` to bypass any lefthook test-fixture identity corruption.

---

## Tier 1 — Hardening (commits 1–5)

### Change 1 — Add workflow-scoped `permissions: contents: read` to `ci.yml`

**Why:** `ci.yml` has no `permissions:` block, so it inherits the repo default. Standard GHA default is `permissions: write-all`. The workflow needs only `contents: read`. Least-privilege.

**File:** `.github/workflows/ci.yml`

**Change:** After the `env:` block (line 24), before `jobs:`, add:
```yaml
permissions:
  contents: read
```

**Commit message:** `ci: scope ci.yml permissions to contents:read`

---

### Change 2 — Add workflow-scoped `permissions: contents: read` to `docs-check.yml`

**Why:** Same rationale. `docs-check.yml` already has `permissions: contents: read` at the job scope but not at workflow scope. Workflow-scoped is the recommended pattern.

**File:** `.github/workflows/docs-check.yml`

**Change:** Move the `permissions: contents: read` block from the `build` job up to workflow scope (after the `concurrency:` block).

**Commit message:** `ci(docs): move docs-check permissions to workflow scope`

---

### Change 3 — Scope `docs-deploy.yml` permissions to per-job

**Why:** Workflow-scoped `contents: write, pull-requests: write` leaks write permission to `decide` (logic only) and `build` (read-only repo + artifact upload). Only `deploy-overlay` and `cleanup-preview` need write.

**File:** `.github/workflows/docs-deploy.yml`

**Change:**
- Remove the workflow-scoped `permissions:` block.
- Add `permissions: contents: read` to `decide` and `build`.
- Add `permissions: { contents: write, pull-requests: write }` to `deploy-overlay` and `cleanup-preview`.

**Commit message:** `ci(docs): scope docs-deploy permissions per job`

---

### Change 4 — Pin `dtolnay/rust-toolchain@master` → `@stable`

**Why:** `@master` is a moving target. Per session memory, this has caused 2 transient macOS rustup flakes recently. `@stable` is a published named ref (auto-bumps with Rust releases, but pinned to released artifacts). The composite action's `toolchain:` input still accepts `stable`, `1.91`, `master`, etc., independently.

**File:** `.github/actions/setup-rust/action.yml`

**Change:** Line 50: `uses: dtolnay/rust-toolchain@master` → `uses: dtolnay/rust-toolchain@stable`.

The action itself doesn't pin the toolchain to "stable" by version; the `toolchain:` input controls that. The `@stable` ref is just the action version (one of the named refs the action publishes).

**Commit message:** `ci(setup-rust): pin dtolnay/rust-toolchain @master → @stable`

---

### Change 5 — Bump `peaceiris/actions-gh-pages@v3` → `@v4`

**Why:** v3 runs on Node 16 (already deprecated; Node 20 deprecation warnings show in `docs-check.yml` runs). v4 runs on Node 20+. Same input/output shape — drop-in.

**File:** `.github/workflows/docs-deploy.yml`

**Change:** The `Deploy via peaceiris/actions-gh-pages` step: `uses: peaceiris/actions-gh-pages@v3` → `uses: peaceiris/actions-gh-pages@v4`.

**Verification:** Confirm v4's input names are unchanged for the params we use (`github_token`, `publish_dir`, `publish_branch`, `keep_files`, `commit_message`, `user_name`, `user_email`). Spot-check the v4 README.

**Commit message:** `ci(docs): bump peaceiris/actions-gh-pages v3 → v4`

---

## Tier 2 — Restructure (commits 6–8)

### Change 6 — `paths-ignore` on `ci.yml` + `ci-summary` job for branch protection

**Why:** Pure-docs PRs currently trigger all 14 Rust jobs (~25 min) for zero useful coverage. `paths-ignore` skips the workflow, but branch protection requires the named status checks to report. Solution: a tiny always-running `ci-summary` job that branch protection can require instead of (or in addition to) the per-job checks.

**File:** `.github/workflows/ci.yml`

**Change:**

1. Update the `on.pull_request:` trigger to add `paths-ignore`:
   ```yaml
   on:
     push:
       branches: [main]
     pull_request:
       paths-ignore:
         - 'docs/**'
         - '*.md'
         - '.github/workflows/docs-*.yml'
   ```
   Push to `main` keeps the full matrix (no skip on protected-branch pushes).

2. Add a new `ci-summary` job at the end that depends on every other job and reports green when all pass OR skipped:

   ```yaml
   ci-summary:
     name: ci-summary
     # Always run, even if dependencies skipped (paths-ignore case).
     if: always()
     needs:
       - fmt
       - clippy
       - cargo-deny
       - test-stable
       - msrv-check
       - test-fixtures-ports
       - feature-flag-matrix
       - build-fixtures-linux
       - build-checks-linux       # consolidated from change 7
       - doc-tests                # new in change 9
       - test-conformance
       - test-tau-plugin-compat
       - test-tau-sandbox-native-e2e
       - test-tau-runtime-e2e
     runs-on: ubuntu-latest
     steps:
       - name: Verify all required jobs passed or skipped
         run: |
           # GHA exposes each dep's result in toJSON(needs). Any "failure"
           # or "cancelled" means the gate fails. "success" and "skipped"
           # are both green.
           results='${{ toJSON(needs) }}'
           echo "$results"
           if echo "$results" | jq -e 'to_entries | map(select(.value.result != "success" and .value.result != "skipped")) | length > 0' >/dev/null; then
             echo "::error::One or more required jobs failed or were cancelled"
             exit 1
           fi
           echo "All required jobs passed or were skipped (docs-only PR)."
   ```

**Post-merge action (user):** in repo Settings → Branches → main protection rule → Required status checks: REPLACE the per-job entries (`rustfmt`, `clippy`, etc.) with just `ci-summary`. This way docs-only PRs pass instantly via the summary job's skipped-deps path; Rust PRs still gate on every individual job because `ci-summary` only succeeds when all deps succeed.

**Commit message:** `ci: skip Rust matrix on docs-only PRs via paths-ignore + summary gate`

---

### Change 7 — Merge 3 `build-tau-plugin-*` jobs into one `build-checks-linux`

**Why:** `build-tau-plugin-test-support`, `build-tau-plugin-conformance`, `build-tau-plugin-compat` are three single-purpose jobs each paying ~1min cache-restore + cargo overhead. Merging shares the cache restore once. ~3min wall-clock + 3 runner-minute savings.

**File:** `.github/workflows/ci.yml`

**Change:** Replace the three jobs with one:

```yaml
build-checks-linux:
  name: build-checks / linux
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: ./.github/actions/setup-rust
      with:
        shared-key: linux-stable
        with-nextest: true
        with-sccache: true
        with-mold: true
    - name: Build tau-plugin-test-support
      run: cargo build -p tau-plugin-test-support
    - name: Test tau-plugin-test-support
      run: cargo nextest run -p tau-plugin-test-support --all-targets
    - name: Build tau-plugin-conformance
      run: cargo build -p tau-plugin-conformance
    - name: Build tau-plugin-compat
      run: cargo build -p tau-plugin-compat
    - name: Build tau-plugin-compat (integration-tests feature)
      run: cargo build -p tau-plugin-compat --features integration-tests --tests
```

**Trade-off:** less granular failure reporting in the GitHub Checks UI. A `build-checks` failure forces the developer to read the job log to identify the failing step. Acceptable; step names are descriptive.

**Branch protection change:** replace three required checks (`build (tau-plugin-test-support)`, `build (tau-plugin-conformance)`, `build (tau-plugin-compat)`) with one (`build-checks / linux`). The `ci-summary` job from change 6 also needs its `needs:` list updated (already shown above).

**Commit message:** `ci: merge 3 build-tau-plugin-* jobs into one build-checks-linux`

---

### Change 8 — Extract `place-fixture-binaries` composite action

**Why:** Four jobs (`test-conformance`, `test-tau-plugin-compat`, `test-tau-sandbox-native-e2e`, `test-tau-runtime-e2e`) each duplicate ~20–30 lines of bash to download the fixture-binaries artifact, mkdir `target/release/`, mv binaries, chmod +x, touch (mtime bump). DRY.

**Files:**
- Create: `.github/actions/place-fixture-binaries/action.yml`
- Modify: `.github/workflows/ci.yml` — 4 jobs replace their inline steps with a composite call

**New composite action content:**

```yaml
name: Place fixture binaries
description: |
  Download the `linux-fixture-binaries` artifact produced by
  build-fixtures-linux and place each binary at the path cargo
  expects. Restore executable bits (upload-artifact strips them)
  and bump mtime above source files so cargo doesn't decide to
  rebuild.

inputs:
  binaries:
    description: |
      Which binaries to place. Either "all" or a space-separated
      subset of names from the artifact (e.g. "tau-controlled-env"
      or "anthropic-plugin ollama-plugin openai-plugin").
    required: true

runs:
  using: composite
  steps:
    - name: Download fixture binaries
      uses: actions/download-artifact@v4
      with:
        name: linux-fixture-binaries
        path: ./prebuilt

    - name: Place binaries
      shell: bash
      env:
        BINARIES: ${{ inputs.binaries }}
      run: |
        set -euo pipefail
        mkdir -p target/release
        mkdir -p crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release

        place() {
          local name="$1"
          local src="./prebuilt/$name"
          [[ -f "$src" ]] || { echo "::error::expected binary not in artifact: $name"; exit 1; }
          if [[ "$name" == "tau-controlled-env" ]]; then
            mv "$src" crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/
          else
            mv "$src" target/release/
          fi
        }

        if [[ "$BINARIES" == "all" ]]; then
          # All names that build-fixtures-linux stages into _artifacts/.
          for name in anthropic-plugin ollama-plugin openai-plugin \
                      fs-read-plugin shell-plugin echo-llm echo-tool tau \
                      tau-controlled-env; do
            place "$name"
          done
        else
          for name in $BINARIES; do
            place "$name"
          done
        fi

        # Restore executable bits (upload-artifact strips them).
        chmod -R +x target/release/
        chmod +x crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release/tau-controlled-env 2>/dev/null || true

        # Bump mtime above source files so cargo's freshness check doesn't rebuild.
        find target/release -maxdepth 1 -type f -exec touch {} +
        find crates/tau-plugin-compat/fixtures/controlled-env-binary/target/release -maxdepth 1 -type f -exec touch {} + 2>/dev/null || true
```

**Workflow edits (4 jobs):**

- `test-conformance`: replace lines 218–230 with:
  ```yaml
  - uses: ./.github/actions/place-fixture-binaries
    with:
      binaries: "anthropic-plugin ollama-plugin openai-plugin"
  ```
- `test-tau-plugin-compat`: replace lines 269–291 with:
  ```yaml
  - uses: ./.github/actions/place-fixture-binaries
    with:
      binaries: "all"
  ```
- `test-tau-sandbox-native-e2e`: replace lines 326–338 with:
  ```yaml
  - uses: ./.github/actions/place-fixture-binaries
    with:
      binaries: "tau-controlled-env"
  ```
- `test-tau-runtime-e2e`: replace lines 356–368 with:
  ```yaml
  - uses: ./.github/actions/place-fixture-binaries
    with:
      binaries: "tau-controlled-env"
  ```

**Validation:** All 4 jobs must still produce the same `target/release/...` and `crates/.../target/release/...` files. The composite preserves both target trees.

**Commit message:** `ci: extract place-fixture-binaries composite action`

---

## Tier 3 — Optimization (commits 9–10)

### Change 9 — Split `cargo test --doc` into its own Linux-only job

**Why:** `test-stable` runs nextest then `cargo test --workspace --doc` sequentially on all 3 OSes. Doctests are serial + slow. Cross-OS doctest behavior is virtually identical (doctests don't exercise OS-specific code paths in this workspace). Splitting saves ~2–3 min on the macOS/Windows critical path.

**File:** `.github/workflows/ci.yml`

**Change:**

1. Remove line 80 (`- run: cargo test --workspace --doc`) from `test-stable`.

2. Add a new `doc-tests` job after `test-stable`:
   ```yaml
   doc-tests:
     name: doc-tests / linux
     runs-on: ubuntu-latest
     steps:
       - uses: actions/checkout@v4
       - uses: ./.github/actions/setup-rust
         with:
           toolchain: stable
           shared-key: linux-stable
           with-sccache: true
           with-mold: true
       - run: cargo test --workspace --doc
   ```

3. Add `doc-tests` to the `ci-summary` job's `needs:` list (already shown in change 6).

**Trade-off:** if a doctest broke specifically on macOS or Windows (rare; would require platform-conditional code in a doc example), CI would no longer catch it. Acceptable per audit assessment.

**Branch protection change:** required checks list: add `doc-tests / linux`.

**Commit message:** `ci: split doctests into a Linux-only doc-tests job`

---

### Change 10 — Split nextest into `[profile.default]` + `[profile.ci]` with retries=0

**Why:** Current `.config/nextest.toml` sets `retries = 2` for all callers. CI silently retries flakes; the second-attempt success hides the flake. Splitting profiles lets local dev keep retries (developer ergonomics) while CI fails fast and surfaces flakes as real signal.

**File:** `.config/nextest.toml`

**Change:**

```toml
# Nextest configuration. See https://nexte.st/docs/configuration/

[profile.default]
# Local dev: retry flaky tests up to twice. nextest's parallel test
# execution exposes timing-sensitive flakes that cargo test
# (single-threaded per binary) masked. Two retries keep the inner
# dev loop snappy without re-running cargo manually.
retries = 2
failure-output = "immediate-final"
success-output = "never"

[profile.ci]
# CI: NO retries. A flake is a real signal — surface it. If a test
# is genuinely timing-sensitive in a way that retries can mask,
# fix the test, don't hide the symptom.
#
# Invoked via `cargo nextest run --profile ci ...` in
# .github/workflows/ci.yml.
retries = 0
failure-output = "immediate-final"
success-output = "never"
```

**File:** `.github/workflows/ci.yml`

**Change:** Every `cargo nextest run ...` invocation gets `--profile ci`:
- Line 79: `cargo nextest run --workspace --all-targets` → `cargo nextest run --profile ci --workspace --all-targets`
- Line 111: `cargo nextest run -p tau-ports --features test-fixtures` → `cargo nextest run --profile ci -p tau-ports --features test-fixtures`
- (Repeat for all `nextest run` invocations: `test-conformance`, `test-tau-plugin-compat`, `test-tau-sandbox-native-e2e`, `test-tau-runtime-e2e`, and any from the new `build-checks-linux` if it contains a nextest call.)

**Trade-off:** the first CI run after this change may surface flakes that were previously masked. Expect 1–2 retries of individual jobs after merge, then steady-state. Each surfaced flake is a separate small fix (out of scope for this PR — link them as follow-up issues).

**Commit message:** `ci: split nextest profile.default vs profile.ci (retries=0)`

---

## Validation between commits

After each commit:
1. `python3 -c "import yaml; yaml.safe_load(open('<path>.yml'))" && echo OK` — syntactic check.
2. Push and wait for `docs-check` if any docs-* workflow changed (none in this plan touch docs-check.yml mechanics, but commit 2 + 3 do edit YAML structure).
3. After all 10 commits: push, open PR, let full CI matrix run. Iterate on any failures.

If a Rust CI job fails after this PR pushes (commits 6–10 are the risk surface), the issue is most likely:
- Commit 7 (build merge): a step name mismatch with branch protection — will surface as a green CI run but a red branch-protection panel. Fix in branch-protection settings.
- Commit 8 (composite action): a binary not present in the artifact for a job that asks for "all" but doesn't need all. Fix by listing the actual subset.
- Commit 9 (doctest split): the new `doc-tests / linux` job missing from `ci-summary`'s `needs:`. Fix in same PR.
- Commit 10 (nextest --profile ci): a flake surfaces. Re-run; if it surfaces twice, file as a follow-up issue and revert to `--profile default` for that specific job as a temporary measure.

---

## Post-merge action items (for the user)

After PR merges to `main`, update branch protection rules at GitHub repo Settings → Branches → main:

1. **Required status checks** — REPLACE the current per-job list with just `ci-summary`. The summary job depends on all the others, so requiring only `ci-summary` is equivalent to requiring all of them — AND it correctly handles the skip-on-docs-only case.

   Optional alternative: keep the per-job names as required AND add `ci-summary`. This makes the protection more restrictive (any single job failure also fails protection) but breaks docs-only PRs because skipped checks aren't "success" to branch protection's per-job view. Recommend the REPLACE option.

2. The new required check is `ci-summary`. Old required-check names that will disappear (and should be unchecked in protection):
   - `build (tau-plugin-test-support)` → folded into `build-checks / linux`
   - `build (tau-plugin-conformance)` → folded into `build-checks / linux`
   - `build (tau-plugin-compat)` → folded into `build-checks / linux`
   - All `test-stable / *` doctest coverage → moved to `doc-tests / linux`

3. (Optional follow-up) wire Dependabot for GitHub Actions (`.github/dependabot.yml`):
   ```yaml
   version: 2
   updates:
     - package-ecosystem: github-actions
       directory: /
       schedule:
         interval: weekly
   ```
   This auto-opens PRs to bump action versions, making SHA pinning feasible without a maintenance tax. Not in this PR — separate concern.
