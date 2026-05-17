# CI upgrades — round 1 implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship three low-risk CI improvements as one PR — preserve the rust-cache write on `main`, drop non-Linux MSRV legs, align the `download-artifact` action version.

**Architecture:** All edits live in two files: `.github/workflows/ci.yml` and `.github/actions/place-fixture-binaries/action.yml`. No Rust code is touched. Each task is a single targeted edit followed by YAML well-formedness verification and a commit. The final task pushes the branch and opens a PR.

**Tech Stack:** GitHub Actions workflow YAML, PyYAML (already present on macOS via system Python or `pip install pyyaml`), `gh` CLI.

**Spec:** `docs/superpowers/specs/2026-05-17-ci-upgrades-round-1-design.md`

**Branch:** `feat/ci-upgrades` (already checked out in worktree at `~/code/tau-worktrees/ci-upgrades`)

---

## Pre-flight

Before starting Task 1, confirm the worktree state:

- [ ] **Step 0a: Confirm clean working tree on the right branch**

Run:

    git -C /Users/titouanlebocq/code/tau-worktrees/ci-upgrades branch --show-current
    git -C /Users/titouanlebocq/code/tau-worktrees/ci-upgrades status --short

Expected: branch is `feat/ci-upgrades`, status is empty (the spec commit `dabe384` is already in).

- [ ] **Step 0b: Confirm PyYAML is importable for the verification steps**

Run:

    python3 -c "import yaml; print(yaml.__version__)"

If this fails with `ModuleNotFoundError`, install it once:

    python3 -m pip install --user pyyaml

(All YAML parse checks below use `python3 -c 'import yaml; yaml.safe_load(open(...))'`.)

---

## Task 1: Item A — preserve `main`-branch cache

**Files:**
- Modify: `.github/workflows/ci.yml` (the `concurrency:` block at lines 17–20)

**Why this task is first:** Item A is the highest-impact change. Shipping it before Items B + C keeps the cache-write window from being interrupted by any subsequent merge during the rest of the PR's life.

- [ ] **Step 1: Apply the edit**

Replace exactly this block in `.github/workflows/ci.yml`:

    # Cancel superseded runs on the same ref.
    concurrency:
      group: ci-${{ github.workflow }}-${{ github.ref }}
      cancel-in-progress: true

with:

    # Cancel superseded runs on the same ref EXCEPT on main, where the
    # Swatinem rust-cache save-step only runs (see
    # .github/actions/setup-rust/action.yml `save-if`). Cancelling a
    # main run mid-cache-write means the cache write never completes
    # and subsequent PRs restore from a stale cache.
    concurrency:
      group: ci-${{ github.workflow }}-${{ github.ref }}
      cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}

The comment is updated in the same edit so a future reader sees the rationale on the line they would otherwise wonder about.

- [ ] **Step 2: Verify the YAML still parses**

Run:

    python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))" && echo PARSE_OK

Expected output: `PARSE_OK`.

- [ ] **Step 3: Verify the expression landed verbatim**

Run:

    grep -n "cancel-in-progress: " .github/workflows/ci.yml

Expected output:

    20:  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}

(Line number may shift by one if the comment grew or shrank — only the content matters. There should be exactly one match in this file.)

- [ ] **Step 4: Inspect the diff one more time**

Run:

    git -C /Users/titouanlebocq/code/tau-worktrees/ci-upgrades diff -- .github/workflows/ci.yml

Confirm visually: only the `concurrency:` block and its preceding comment changed; everything else is untouched.

- [ ] **Step 5: Commit**

Run:

    git -C /Users/titouanlebocq/code/tau-worktrees/ci-upgrades \
      -c user.name="Titouan Lebocq" \
      -c user.email="lebocq.tit@gmail.com" \
      commit --no-verify -am "ci: preserve main-branch rust-cache writes on fast-follow merges

cancel-in-progress was unconditionally true, which cancels in-flight
main runs mid-cache-write. setup-rust/action.yml only writes
Swatinem's rust-cache on refs/heads/main, so the cancellation leaves
the cache unwritten and subsequent PRs restore stale state. Make the
cancel conditional on the ref so main runs always complete.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

`--no-verify` is acceptable here per CLAUDE.md's "AGENT PUSH RULES" — this is a yaml-only change, and the deep gate adds nothing for workflow YAML.

---

## Task 2: Item B — drop macOS/Windows MSRV legs

**Files:**
- Modify: `.github/workflows/ci.yml` (the `msrv-check:` job at lines 108–123)

- [ ] **Step 1: Apply the edit**

Replace exactly this block in `.github/workflows/ci.yml`:

      msrv-check:
        name: msrv-check / ${{ matrix.os == 'ubuntu-latest' && 'linux' || matrix.os == 'macos-latest' && 'macos' || 'windows' }}
        runs-on: ${{ matrix.os }}
        strategy:
          fail-fast: false
          matrix:
            os: [ubuntu-latest, macos-latest, windows-latest]
        steps:
          - uses: actions/checkout@v6
          - uses: ./.github/actions/setup-rust
            with:
              toolchain: "1.91"
              shared-key: ${{ matrix.os }}-1.91
              with-sccache: true
              with-mold: true
          - run: cargo check --workspace --all-targets --locked

with:

      msrv-check:
        # MSRV is a rustc-version property, not an OS property. The only
        # cfg(target_os)-gated code in this workspace lives in
        # tau-sandbox-windows (scaffold) and tau-sandbox-darwin (real),
        # both already covered by `test-stable` on their native OS.
        # Cross-OS MSRV signal is not worth the ~14 Linux-equivalent
        # runner minutes per CI run.
        name: msrv-check / linux
        runs-on: ubuntu-latest
        steps:
          - uses: actions/checkout@v6
          - uses: ./.github/actions/setup-rust
            with:
              toolchain: "1.91"
              shared-key: linux-1.91
              with-sccache: true
              with-mold: true
          - run: cargo check --workspace --all-targets --locked

Note the four substantive deletions: the dynamic `name:` expression, the entire `strategy:` block, the dynamic `runs-on:`, and the dynamic `shared-key:`. Indentation matches the surrounding job definitions (4-space indent inside `jobs:`, 6-space inside the job, 8-space inside `with:`).

- [ ] **Step 2: Verify the YAML still parses**

Run:

    python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))" && echo PARSE_OK

Expected output: `PARSE_OK`.

- [ ] **Step 3: Verify the matrix is gone and the job is single-OS**

Run:

    python3 -c "
    import yaml
    d = yaml.safe_load(open('.github/workflows/ci.yml'))
    j = d['jobs']['msrv-check']
    assert 'strategy' not in j, 'strategy still present'
    assert j['runs-on'] == 'ubuntu-latest', j['runs-on']
    assert j['name'] == 'msrv-check / linux', j['name']
    # setup-rust step's shared-key:
    setup = next(s for s in j['steps'] if isinstance(s, dict) and s.get('uses', '').endswith('setup-rust'))
    assert setup['with']['shared-key'] == 'linux-1.91', setup['with']['shared-key']
    print('CHECKS_OK')
    "

Expected output: `CHECKS_OK`. If any assertion fires, fix the YAML and re-run Step 2.

- [ ] **Step 4: Inspect the diff**

Run:

    git -C /Users/titouanlebocq/code/tau-worktrees/ci-upgrades diff -- .github/workflows/ci.yml

Confirm visually: the `msrv-check:` job lost its matrix, the surrounding `clippy:` (job above) and `test-stable:` (job below) are untouched.

- [ ] **Step 5: Commit**

Run:

    git -C /Users/titouanlebocq/code/tau-worktrees/ci-upgrades \
      -c user.name="Titouan Lebocq" \
      -c user.email="lebocq.tit@gmail.com" \
      commit --no-verify -am "ci: drop macOS + Windows MSRV legs

MSRV is a rustc-version property, not an OS property. The only
cfg(target_os)-gated code in this workspace is in tau-sandbox-windows
(scaffold) and tau-sandbox-darwin (real macOS adapter), both already
covered by test-stable on their native OS at stable toolchain.

Saves ~14 Linux-equivalent runner minutes per CI run (60s macOS at
10x multiplier + 2m3s Windows at 2x).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

---

## Task 3: Item C — bump `download-artifact` to `@v8`

**Files:**
- Modify: `.github/actions/place-fixture-binaries/action.yml` (the `Download fixture binaries` step at lines 20–24)

- [ ] **Step 1: Apply the edit**

Replace exactly this snippet in `.github/actions/place-fixture-binaries/action.yml`:

        - name: Download fixture binaries
          uses: actions/download-artifact@v4
          with:
            name: linux-fixture-binaries
            path: ./prebuilt

with:

        - name: Download fixture binaries
          uses: actions/download-artifact@v8
          with:
            name: linux-fixture-binaries
            path: ./prebuilt

Only the version pin changes (`@v4` → `@v8`). No other lines move.

- [ ] **Step 2: Verify the YAML still parses**

Run:

    python3 -c "import yaml; yaml.safe_load(open('.github/actions/place-fixture-binaries/action.yml'))" && echo PARSE_OK

Expected output: `PARSE_OK`.

- [ ] **Step 3: Verify the new version landed**

Run:

    grep -n "download-artifact@" .github/actions/place-fixture-binaries/action.yml

Expected output:

    21:      uses: actions/download-artifact@v8

(Single match.)

- [ ] **Step 4: Verify cross-repo consistency (download-artifact)**

Run:

    grep -rn "download-artifact@" .github/

Expected: every reference is now `@v8`. Specifically:

    .github/actions/place-fixture-binaries/action.yml:21:      uses: actions/download-artifact@v8
    .github/workflows/docs-deploy.yml:250:        uses: actions/download-artifact@v8

If any other `@vN` appears for `download-artifact`, fix it before committing — drift was the whole reason for this task.

- [ ] **Step 5: Inspect the diff**

Run:

    git -C /Users/titouanlebocq/code/tau-worktrees/ci-upgrades diff -- .github/actions/place-fixture-binaries/action.yml

One-line change: `download-artifact@v4` → `download-artifact@v8`.

- [ ] **Step 6: Commit**

Run:

    git -C /Users/titouanlebocq/code/tau-worktrees/ci-upgrades \
      -c user.name="Titouan Lebocq" \
      -c user.email="lebocq.tit@gmail.com" \
      commit --no-verify -am "ci: align download-artifact to @v8 across .github/

place-fixture-binaries was pinned at @v4 while docs-deploy already
uses @v8. Bring them in line so future Dependabot bumps land as one
PR and reviewers see one version family. @v8 download is
forward-compatible with the existing @v7 uploads.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

---

## Task 4: Push and open the PR

**Files:** none (git/gh operations only)

- [ ] **Step 1: Inspect the full commit set before pushing**

Run:

    git -C /Users/titouanlebocq/code/tau-worktrees/ci-upgrades log --oneline main..HEAD

Expected four commits, newest first:

    <sha>  ci: align download-artifact to @v8 across .github/
    <sha>  ci: drop macOS + Windows MSRV legs
    <sha>  ci: preserve main-branch rust-cache writes on fast-follow merges
    dabe384 docs(specs): CI upgrades round 1 design

If the order is different or commits are missing, stop and reconcile.

- [ ] **Step 2: Push the branch**

Per CLAUDE.md's "AGENT PUSH RULES", use `--no-verify` (this PR is yaml-only, the deep podman gate adds nothing). Direct `git push --no-verify` is safe because no long-running pre-push hook fires.

    git -C /Users/titouanlebocq/code/tau-worktrees/ci-upgrades push -u origin feat/ci-upgrades --no-verify

- [ ] **Step 3: Open the PR**

Run, from the worktree:

    cd /Users/titouanlebocq/code/tau-worktrees/ci-upgrades
    gh pr create --title "ci: round 1 upgrades (main-branch cache, MSRV legs, artifact version)" --body "$(cat <<'EOF'
    ## Summary

    Three independent low-risk CI improvements, bundled as one PR.

    1. **Preserve main-branch rust-cache writes on fast-follow merges.**
       `cancel-in-progress` was unconditionally true on `ci.yml`'s
       concurrency group. `setup-rust/action.yml` only writes the
       Swatinem rust-cache on `refs/heads/main`, so cancelling a main
       run mid-cache-write means the cache write never completes.
       Make the cancel conditional on the ref.

    2. **Drop macOS + Windows MSRV legs.** MSRV is a rustc-version
       property, not an OS property. Saves ~14 Linux-equivalent
       runner minutes per CI run.

    3. **Align `download-artifact` to `@v8` across `.github/`.**
       `place-fixture-binaries` was pinned at `@v4` while
       `docs-deploy.yml` already uses `@v8`. Bring them in line.

    Spec: `docs/superpowers/specs/2026-05-17-ci-upgrades-round-1-design.md`

    ## Test plan

    - [ ] CI green on this PR.
    - [ ] Only one MSRV job (`msrv-check / linux`) appears in the run.
    - [ ] `test (tau-plugin-compat / linux)` succeeds — confirms `@v8`
      download composes with `@v7` upload via the composite action.
    - [ ] After merge, watch the next `main` run complete its
      `Post Cache cargo registry, target, and sccache` step (proves
      the cache write isn't being cut off).

    🤖 Generated with [Claude Code](https://claude.com/claude-code)
    EOF
    )"

- [ ] **Step 4: Watch CI**

The PR URL printed by `gh pr create` is the destination. Confirm:

- `ci` workflow starts.
- `msrv-check / linux` is present; no `msrv-check / macos` / `msrv-check / windows` jobs.
- `test (tau-plugin-compat / linux)` reaches `Place binaries at cargo-expected paths` and succeeds.
- All other jobs green.

If anything fails, the recovery is task-scoped: fix the regressed item with one targeted follow-up commit on the same branch and re-push.

---

## Self-review checklist

After all tasks pass:

1. Spec coverage — every "Change" subsection in
   `docs/superpowers/specs/2026-05-17-ci-upgrades-round-1-design.md`
   corresponds to one task:
   - Item A → Task 1 ✓
   - Item B → Task 2 ✓
   - Item C → Task 3 ✓
2. No placeholders, no "similar to …", every step's command shown
   verbatim.
3. Type consistency: nothing here defines types — verified that the
   YAML expressions in commit messages and assertions match the
   final file contents.
