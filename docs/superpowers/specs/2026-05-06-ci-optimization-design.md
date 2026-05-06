# CI optimization — design

> Spec for the CI optimization sub-project. Branch: `feat/ci-optimization-spec`. Targets a phased migration from the current 29-check / ~33-min PR critical path to an 18-check / ≤ 25-min path without compromising coverage, while upgrading Windows to a hard gate.

## Context

After sub-project D shipped (commit `6c8be31`), tau's CI gates `main` with **29 required checks** and runs PRs in ~33 min wall-clock / ~85 min CI compute. Sub-project D added `Swatinem/rust-cache@v2` via the `setup-rust` composite action plus `CARGO_INCREMENTAL=0` workflow-level. Caching is in place but jobs don't share the cache (default per-job key) and substantial duplicate work remains:

- Multiple jobs build large overlapping subsets of the workspace.
- 5 dedicated plugin release-build jobs (anthropic / ollama / openai / fs-read / shell) plus a 6th release-build job for the toy plugins (`echo-llm`, `echo-tool`) duplicate work the matrix test already does.
- 5 explicit `--no-default-features` jobs check feature-flag breakage per crate. Two additional jobs named `build (tau-plugin-protocol)` and `build (tau-plugin-sdk)` are also `--no-default-features` jobs in disguise (their YAML keys are `no-default-features-protocol` and `no-default-features-sdk`), so 7 jobs total are doing this kind of work.
- 3 Linux e2e jobs each rebuild plugins + tau-cli + the controlled-env fixture.
- The conformance test rebuilds plugins from scratch.
- Windows test failures are currently advisory (`continue-on-error: true` on the matrix). This spec upgrades Windows to a hard gate as part of the migration.
- The matrix Linux entries also run `cargo test ... -- --ignored` for sandbox integration tests, redundant with the dedicated e2e jobs added in sub-project D.

This spec describes a target architecture that eliminates duplication while preserving real e2e coverage on every (OS × toolchain) combination, with a phased migration that's independently revertable at each step.

## Goals

In priority order:

1. **Reduce wall-clock CI time on PRs.** Current ~33 min → target ≤ 25 min.
2. **Reduce CI compute / minutes per PR.** Current ~85 min → target ≤ 50 min.
3. **Preserve full coverage on every (OS × toolchain) combination.** Linux + macOS + Windows; stable + 1.91. No "smoke test" tier; no OS dropped.
4. **Upgrade Windows from advisory to hard gate.** `continue-on-error: false`.
5. **Preserve granular failure attribution.** When CI fails, the failed check name still points at a specific crate / area.
6. **Maintainable workflow YAML.** Adopt enhanced composite action where it reduces duplication.

## Non-goals

- **Self-hosted or paid runners.** Out of scope per cost.
- **Removing OS coverage.** All three OSes retain real e2e tests.
- **Artifact-passing of `target/` directories.** Cargo's target layout is internal; the design uploads only specific compiled binaries.
- **Workspace-hack (cargo-hakari).** Not worth the complexity at this workspace size.
- **`cargo-make`-style workflow consolidation.** Cargo invocations stay readable in YAML.
- **Reusable workflow files.** The composite action absorbs the duplication that's worth absorbing; reusable workflows would add indirection without proportional gain at this scale. Revisit if `ci.yml` exceeds ~500 lines.
- **Deferring any current PR-time check to post-merge.** Conservative path; everything stays at PR time.

## Constraints

- All current behavior coverage must remain represented at PR time.
- Branch-protection check names must be deterministic.
- No new cost beyond GitHub Actions cache storage (10 GB free quota for public repos).
- Must work on Linux, macOS, and Windows.
- Must work with stable + 1.91 (MSRV).

---

## Target architecture

### Final required-check list (18 checks)

```
 1. rustfmt                                cargo fmt --all -- --check
 2. clippy                                 cargo clippy --workspace --all-targets -- -D warnings
 3. test-stable / linux                    cargo nextest run --workspace --all-targets
                                           cargo test  --workspace --doc
 4. test-stable / macos                    (same)
 5. test-stable / windows                  (same; HARD gate, continue-on-error: false)
 6. msrv-check  / linux                    cargo check --workspace --all-targets --locked
 7. msrv-check  / macos                    (same)
 8. msrv-check  / windows                  (same; HARD gate)
 9. test-fixtures-ports / linux            cargo nextest run -p tau-ports --features test-fixtures
10. build (tau-plugin-test-support)        cargo build + cargo test (default features)
11. build (tau-plugin-conformance)         cargo build (default features)
12. build (tau-plugin-compat)              cargo build with + without integration-tests
13. build-fixtures / linux                 builds 5 plugins + 2 toy plugins + tau-cli +
                                           controlled-env (release); uploads as
                                           `linux-fixture-binaries`
14. plugin-compat / linux                  needs: build-fixtures; downloads artifact;
                                           cargo nextest run -p tau-plugin-compat
                                                --features integration-tests
15. sandbox-native-e2e / linux             needs: build-fixtures; cargo nextest run
                                                -p tau-sandbox-native --features integration-tests
16. runtime-e2e / linux                    needs: build-fixtures; cargo nextest run
                                                -p tau-runtime --features integration-tests
17. conformance / linux                    needs: build-fixtures; cargo nextest run
                                                -p anthropic -p ollama -p openai --test conformance
18. feature-flag-matrix / linux            loops cargo check -p X --no-default-features over
                                           [tau-domain, tau-ports, tau-pkg, tau-runtime,
                                            tau-cli, tau-plugin-protocol, tau-plugin-sdk]
```

Note on `feature-flag-matrix`: it covers 7 crates — the 5 with explicit `no-default-features-*` job keys today (tau-domain, tau-ports, tau-pkg, tau-runtime, tau-cli) plus the 2 whose existing user-visible names are misleading (`build (tau-plugin-protocol)` / `build (tau-plugin-sdk)` are actually `--no-default-features` jobs).

### Job dependency graph

```
[rustfmt]   [clippy]   [feature-flag-matrix / linux]   [test-fixtures-ports / linux]
[test-stable / {3 OS}]                    [msrv-check / {3 OS}]
[build (tau-plugin-test-support)]         [build (tau-plugin-conformance)]
[build (tau-plugin-compat)]
       ↑
       └── all run in parallel, no inter-dependencies

[build-fixtures / linux]
       │
       ├─→ [plugin-compat / linux]
       ├─→ [sandbox-native-e2e / linux]
       ├─→ [runtime-e2e / linux]
       └─→ [conformance / linux]
```

Critical path: `max(non-fixture-jobs, build-fixtures + max(e2e-jobs))`. Estimated ~22-25 min wall-clock.

### Test-stable / linux sub-step ordering

The `test-stable / linux` job runs three separate cargo invocations:

1. `cargo nextest run --workspace --all-targets` — primary unit + integration tests.
2. `cargo test --workspace --doc` — doc tests (nextest support for doctests is incomplete; keep explicit cargo test for them).
3. **NOT** `cargo test ... -- --ignored` for sandbox integration tests — the dedicated `sandbox-native-e2e / linux`, `runtime-e2e / linux`, and `plugin-compat / linux` jobs cover those with proper `--features integration-tests`. The current matrix's `-- --ignored` block is removed.

### Diff vs current

| Change | From | To |
|---|---|---|
| Test matrix split | 6 × `test (X / Y)` | 3 × `test-stable / X` + 3 × `msrv-check / X` |
| Windows status | `continue-on-error: true` | hard gate (`continue-on-error: false`) |
| Plugin release builds (real) | 5 × `build (X-plugin)` | absorbed into `build-fixtures / linux` |
| Plugin release builds (toy) | 1 × `build (tau-plugins)` | absorbed into `build-fixtures / linux` |
| No-default-features jobs | 5 explicit + 2 misnamed = 7 | 1 × `feature-flag-matrix / linux` |
| Linux e2e plugin building | rebuilt 3× independently | built once in `build-fixtures`, downloaded |
| Conformance test | rebuilds plugins | downloads prebuilt |
| Matrix Linux `--ignored` integration tests | inside matrix job | covered by dedicated e2e jobs (removed from matrix) |

29 → 18 required checks.

---

## Components

### Enhanced `setup-rust` composite action

Inputs:

- `toolchain` (default: `stable`) — Rust toolchain to install.
- `shared-key` (required) — pinned per `(os, toolchain)`. Examples: `ubuntu-stable`, `ubuntu-1.91`, `macos-stable`. All jobs on the same `(os, toolchain)` share one cache entry.
- `with-sccache` (default: `false`) — installs `mozilla-actions/sccache-action`, sets `RUSTC_WRAPPER=sccache` and `SCCACHE_GHA_ENABLED=true`.
- `with-mold` (default: `false`) — Linux only; installs mold via `rui314/setup-mold@v1`, sets `RUSTFLAGS=-C link-arg=-fuse-ld=mold`.

Steps in order:

1. Install toolchain (dtolnay/rust-toolchain).
2. Install mold (conditional on `with-mold` and `runner.os == 'Linux'`).
3. Install sccache (conditional on `with-sccache`).
4. `Swatinem/rust-cache@v2` with `shared-key: ${{ inputs.shared-key }}`, `save-if: ${{ github.ref == 'refs/heads/main' }}`.

The `save-if` policy keeps cache writes scoped to `main`. PRs read-only — no write contention between parallel jobs and no PR-specific cache pollution.

### Tooling decisions

| Tool | Where applied | Why | Why not elsewhere |
|---|---|---|---|
| `cargo nextest` | All `cargo test` invocations workspace-wide except doctests | Up to 3× faster on workspaces with many test binaries; first-class CI features | Doctests use `cargo test --doc` (nextest doctest support is incomplete) |
| `mold` linker | Linux jobs only | ~10× linker speed; especially helps tau's many small binaries | Not available on macOS/Windows |
| `sccache` (GHA backend) | Test/check jobs only (not release builds) | Caches individual rustc invocations; cross-job rustc-call dedup | Sccache slows release builds up to 50%; release jobs use rust-cache only |
| `rust-cache` `shared-key` | All jobs producing `target/` | Cross-job cache sharing per `(os, toolchain)` | – |

### Artifact contract for `build-fixtures`

`linux-fixture-binaries` artifact uploaded by `build-fixtures` and consumed by 4 downstream jobs:

```
linux-fixture-binaries/
├── anthropic           (release binary)
├── ollama              (release binary)
├── openai              (release binary)
├── fs-read             (release binary)
├── shell               (release binary)
├── echo-llm            (release binary)        ← absorbed from build (tau-plugins)
├── echo-tool           (release binary)        ← absorbed from build (tau-plugins)
├── tau                 (release binary, the CLI)
└── tau-controlled-env  (release binary, fixture)
```

Downstream jobs `mv` these into the cargo-expected paths (`target/release/<name>` and the controlled-env binary's per-Cargo-project path) before invoking `cargo nextest`. Cargo's freshness check sees binaries with newer mtimes than sources and doesn't rebuild.

Retention: 1 day (PR-scoped artifacts; no history needed).

If cargo decides to rebuild despite the prebuilt binary present (e.g., rust-cache restored a `target/` with a different fingerprint), the optimization is wasted but tests still run. Mitigation: place prebuilt binaries AFTER cache restore but BEFORE cargo invocation; `touch` to bump mtime if needed.

### What stays inline vs gets factored out

| Concern | Lives in |
|---|---|
| Toolchain + cache + sccache + mold setup | `.github/actions/setup-rust/action.yml` (composite) |
| Per-job command logic | inline in `.github/workflows/ci.yml` |
| "Loop over crates with feature-flag check" pattern | inline shell loop in `feature-flag-matrix` job |
| "Loop over plugin builds" pattern | single `cargo build --release -p X -p Y -p Z` invocation in `build-fixtures` |

---

## Migration plan

Five phases. Each phase is a separate PR with its own branch-protection delta. Each phase is independently revertable.

### Phase A — Tier 1 wins (no check name changes)

- Update `setup-rust` composite action: add `shared-key`, `with-sccache`, `with-mold` inputs.
- Update all existing jobs to pass `shared-key: <os>-<toolchain>`.
- Replace `cargo test` with `cargo nextest run` for non-doctest invocations (install via `taiki-e/install-action@nextest`). Doctests stay on `cargo test --doc`.
- Enable `with-sccache: true` on test/check jobs (NOT release builds).
- Enable `with-mold: true` on Linux jobs.

Branch protection: **no change**. All 29 existing check names continue to be emitted. PRs just get faster.

**Risk:** tooling regression (nextest/mold incompatibility). Mitigation: phase A's PR is its own canary; if green, the tools work.

**Rollback:** `git revert <phase-A-commit>`. No branch-protection state to undo.

**Estimated savings at end of Phase A:** ~30-40% wall-clock reduction.

### Phase B — Test matrix split + Windows hard gate

Renames 6 matrix entries. The MSRV variant runs `cargo check` instead of `cargo nextest run`. **Windows changes from `continue-on-error: true` to false**; this is the strictness upgrade per design decision W2.

Removes the `-- --ignored` integration test block from the Linux matrix entries (the dedicated e2e jobs cover them).

Branch-protection delta:

```
Remove:
  test (ubuntu-latest / stable),  test (macos-latest / stable),  test (windows-latest / stable)
  test (ubuntu-latest / 1.91),    test (macos-latest / 1.91),    test (windows-latest / 1.91)
Add:
  test-stable / linux,   test-stable / macos,   test-stable / windows
  msrv-check  / linux,   msrv-check  / macos,   msrv-check  / windows
```

Net: 6 in, 6 out. Still 29 checks at this point.

**Risk:** Windows hard-gate exposes pre-existing Windows flakes that were silently failing. Mitigation: Phase B's PR is preceded by a diagnostic run on `feat/ci-optimization-spec` that catalogues Windows test history; any consistently-failing Windows tests must be fixed or `#[cfg(not(target_os = "windows"))]`-skipped before Phase B merges.

**Rollback:** `git revert` + restore old branch-protection list + restore `continue-on-error: true`.

### Phase C — Build-fixtures + e2e refactor

Adds `build-fixtures / linux`. Refactors `plugin-compat`, `sandbox-native-e2e`, `runtime-e2e`, `conformance` to download the artifact instead of self-building.

Branch-protection delta:

```
Add:
  build-fixtures / linux
(no removals; the 4 e2e check names stay the same; test-fixtures-ports renamed in Phase E)
```

Net: +1 check (29 → 30 transient).

**Risk:** artifact contract breaks. Mitigation: phase-C PR keeps a fallback `cargo build` path; once green, follow-up commit removes the fallback.

**Rollback:** revert + remove `build-fixtures / linux` from branch protection.

### Phase D — Plugin release fold

Drops the 5 dedicated plugin builds and the toy-plugins build. They're redundant with `build-fixtures / linux` (which builds all 7 in release as a required check).

Branch-protection delta:

```
Remove:
  build (anthropic-plugin),  build (ollama-plugin),  build (openai-plugin)
  build (fs-read-plugin),    build (shell-plugin)
  build (tau-plugins)
```

Net: -6 checks (30 → 24).

**Risk:** zero — `build-fixtures / linux` does the exact same work and is a required check.

**Rollback:** revert; checks come back automatically next CI run on main.

### Phase E — Feature-flag-matrix + test-fixtures-ports rename

Adds `feature-flag-matrix / linux`. Drops 5 explicit `--no-default-features` jobs PLUS the 2 misnamed `build (tau-plugin-protocol)` and `build (tau-plugin-sdk)` jobs (which are `--no-default-features` builds in disguise). Renames `test (tau-ports test-fixtures only)` to `test-fixtures-ports / linux` for naming consistency.

Branch-protection delta:

```
Remove:
  build (no-default-features)                     ← actually tau-domain
  build (tau-ports no-default-features)
  build (tau-pkg no-default-features)
  build (tau-runtime no-default-features)
  build (tau-cli no-default-features)
  build (tau-plugin-protocol)                     ← misnamed; --no-default-features
  build (tau-plugin-sdk)                          ← misnamed; --no-default-features
  test (tau-ports test-fixtures only)
Add:
  feature-flag-matrix / linux
  test-fixtures-ports / linux
```

Net: 8 removed, 2 added → -6. 24 → 18.

Phase-by-phase math:
- Start: 29
- Phase A: 29 (no changes)
- Phase B: 29 (6 in, 6 out)
- Phase C: 30 (+1)
- Phase D: 24 (-6: 5 plugins + 1 toy)
- Phase E: 18 (-6: 8 removed, 2 added)

**Risk:** consolidated check name loses granular fail attribution. Mitigation: shell loop emits `::group::<crate>` markers so failed log section identifies the crate.

**Rollback:** revert + restore the 7 ndf check names + the test-fixtures-ports rename.

### Phase ordering and dependencies

```
Phase A ─→ Phase B ─→ Phase C ─→ Phase D ─→ Phase E
(tooling)  (matrix     (build-    (drop 5+1   (feature-flag-
            split +     fixtures + plugin       matrix +
            Windows     e2e        builds)      test-fixtures
            hard gate)  refactor)               rename)
                                              
                       ↑ Phase D depends on Phase C
                       (build-fixtures must exist before
                        dropping the plugin build jobs)
```

Each phase lands as its own PR; user reviews, merges, updates branch protection, then we open the next.

### Branch-protection update mechanics

Each phase's PR description includes explicit "Add to required checks" / "Remove from required checks" instructions. The user makes the GitHub-settings change manually (matches the pattern from sub-projects A, B, D).

### Open PR handling

Phases B, D, and E rename or remove check names. PRs opened before each phase will fail branch protection until rebased. PR descriptions include:

> After this merges, any open PRs will need to rebase onto main and re-run CI to satisfy the updated branch protection.

### Aborting mid-migration

Stopping after Phase A (tier 1 only) leaves you with most of the speed wins (~30-40% reduction) and ZERO check-name changes. That's a valid resting point if any later phase reveals problems. Each phase's previous state is fully recoverable via `git revert` + branch-protection settings revert.

Stopping after Phase B (Windows hard-gate) but before Phase C is also valid — it's the strictness change without the consolidation work.

---

## Verification

### Success metrics (measured after Phase E lands)

| Metric | Baseline (current) | Target (post-migration) | How measured |
|---|---|---|---|
| Wall-clock per PR | ~33 min | ≤ 25 min | GitHub Actions PR run page, mean of 5 PRs |
| CI compute per PR | ~85 min | ≤ 50 min | Sum of "Billable time" across jobs |
| Required check count | 29 | 18 | Branch protection settings |
| rust-cache hit rate | unmeasured | > 80% on PRs against main | rust-cache action logs |
| Test-stable matrix wall-clock | ~7 min | ~3-4 min | Per-job duration |
| Windows test stability | advisory (failures ignored) | hard gate; 0 spurious failures over 10 PR runs | Branch protection check history |

If post-migration numbers don't hit these, the design didn't deliver and we revisit.

### Per-phase verification

Each phase's PR must demonstrate:

1. **The phase's own CI is green** — the PR is its own canary.
2. **A representative second PR also passes** — open a trivial test PR right after merge to confirm main's CI works for non-author contributors.
3. **No regression in critical-path wall-clock** vs the previous phase.

For Phase B specifically:

4. **Windows test history audit** before merging: check the last 20 main-branch CI runs on Windows; document any flakes; resolve before flipping `continue-on-error: false`.

If any of these fail, revert that phase before opening the next.

### What's explicitly NOT verified

- macOS/Windows sandbox-native code (`cfg(target_os = "linux")`-gated; unchanged from current state).
- Real-network plugin behavior (conformance uses cassette replay).
- CI performance drift over time (out of scope; future work).

---

## Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `cargo nextest` skips a test that `cargo test` ran | low | medium | Phase A PR shows identical pass-count vs baseline; compare `nextest list` to `cargo test --list` before merging |
| `mold` breaks linking on a specific crate | low | high (Linux jobs all-fail) | Phase A canary; if broken, set `with-mold: false` and proceed without that win |
| sccache GHA backend exceeds 10 GB cache quota | medium | medium (eviction → cold builds) | Monitor `Cache Storage` usage after Phase A; restrict sccache scope if approaching limit |
| rust-cache `shared-key` write contention | medium | low | `save-if: github.ref == 'refs/heads/main'` means PR jobs read-only; no contention |
| `actions/upload-artifact` produces stale binaries | low | high (e2e fails confusingly) | Phase C keeps fallback `cargo build` path until green; jobs verify artifact contents |
| Cargo freshness check rebuilds despite prebuilt binary | medium | low (optimization wasted but tests run) | Place binaries AFTER cache restore, BEFORE cargo invocation; `touch` to bump mtime; verify with `--verbose` cargo logging |
| Branch-protection update missed between phase merge and follow-up PRs | medium | medium (other contributors blocked) | PR description includes explicit branch-protection update steps |
| MSRV regression caught by `check` differs from `test` MSRV | very low | low | `cargo check` is a strict superset of "compiles"; if it fails, real compile failure exists |
| Phase E's loop loses per-crate failure attribution | certain | low | `::group::<crate>` markers identify failing crate; acceptable trade-off |
| Cache pollution from a misbehaving crate breaks cross-job sharing | low | medium | `shared-key` keyed on Cargo.lock hash; bump shared-key suffix (`ubuntu-stable-v2`) if pollution observed |
| Windows hard-gate exposes pre-existing flakes | medium | high (PRs blocked) | Phase B preceded by Windows test history audit; flaky tests fixed or skipped via `cfg(not(target_os = "windows"))` before flip |
| nextest doctest support changes between versions | low | low | Pin nextest version; doctests run via `cargo test --doc` regardless |

---

## Open questions for the implementation plan

These decisions are deferred to plan-time:

1. Exact mold install method (`rui314/setup-mold@v1` vs `apt-get install mold` vs release-binary download).
2. sccache cache size budget within the 10 GB GHA quota; whether sccache scope needs restricting.
3. Canonical nextest install action (`taiki-e/install-action@nextest` vs alternatives).
4. `save-if` semantics with `shared-key`: verify rust-cache deduplicates concurrent main writes correctly.
5. Whether `feature-flag-matrix` should also test `--all-features` (not just `--no-default-features`).
6. Whether `build (tau-plugin-test-support)` and `build (tau-plugin-conformance)` jobs benefit from sccache (small crates; sccache overhead may exceed savings; benchmark once).
7. Phase A's nextest config (`nextest.toml` for retry policies vs default).
8. Exact Windows test-history audit procedure for Phase B (number of runs to inspect, threshold for "flaky").
9. Whether the implementation plan finds any additional `--no-default-features` job equivalents beyond the 7 catalogued here.

---

## Documentation deliverables

Captured in the implementation plan's final task:

- Update `CLAUDE.md` if local-dev story changes (e.g., contributors should run `cargo nextest run` locally).
- New ADR-0018: "CI optimization architecture" — captures the decisions in this spec.
- Update `docs/reference/` with a CI reference doc if useful.

---

## Decisions recorded

1. **Conservative within-constraint optimization path.** Full e2e on every (OS × toolchain) combination is preserved; no smoke-test tier; no OS dropped.
2. **W2: Windows upgrades from advisory to hard gate** as part of Phase B. PRs can't merge with Windows test failures.
3. **MSRV is a compile-time contract**, verified by `cargo check`, not `cargo test`. Industry-standard pattern (tokio, serde, clap).
4. **Cargo `target/` is internal**; we upload only specific compiled binaries via `actions/upload-artifact`. Per-job rust-cache + cross-job sccache covers the rest.
5. **No reusable workflow files** at this CI scale; the composite action absorbs duplication. Revisit at ~500-line `ci.yml`.
6. **Phased migration over big-bang**; each phase independently revertable.
7. **Phase A and Phase B are valid resting points** if later phases reveal problems.
8. **Required check count drops 29 → 18** by absorbing 6 plugin-release jobs into `build-fixtures` (Phase C/D) and 7 no-default-features jobs (5 explicit + 2 misnamed) into `feature-flag-matrix` (Phase E).
9. **Doctests stay on `cargo test --doc`** in `test-stable / linux`; nextest doctest support is incomplete.
10. **The matrix Linux `-- --ignored` integration test block is removed** in Phase B; the dedicated e2e jobs cover that scope properly with `--features integration-tests`.
