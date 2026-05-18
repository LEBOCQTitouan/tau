# ADR-0018: CI optimization — five-phase migration (sub-project E)

**Status:** Accepted (Decision 3 partially superseded — see "Subsequent revisions" below)
**Date:** 2026-05-06
**Deciders:** Titouan Lebocq
**Supersedes:** —
**Closes:**
- The 23-required-check / ~33-min PR critical-path CI baseline that had accumulated since priority-12 shipped.

## Subsequent revisions

- **2026-05-17 (PR #122, "ci: round 1 upgrades"):** The `msrv-check` job was collapsed from a 3-OS matrix (`linux`, `macos`, `windows`) to Linux-only. Decision 3 below still holds — MSRV remains a `cargo check` job, not `cargo test` — but the cross-OS dimension is gone, on the basis that MSRV is a rustc-version property and the OS-gated code paths are already exercised by `test-stable` on stable toolchain. Section "14 required checks across 3 jobs types" in Decision 6 below is therefore now overstated by 2 checks. See `docs/superpowers/specs/2026-05-17-ci-upgrades-round-1-design.md` for full rationale.

## Context

At the start of sub-project E (CI optimization), the CI configuration was the product of incremental organic growth: each sub-project added jobs as needed without ever cleaning up redundancy. The pre-Phase-A state was:

- **23 required checks** gating `main`.
- **~33-minute PR critical path** dominated by redundant compilation: the 5 real plugin binaries were built independently in every Linux job that needed them.
- **One undifferentiated `test:` job** per OS that ran everything — doctests, `cargo test --ignored` integration tests, and unit tests — in a single step, blocking on the slowest test in the set.
- **No cross-job compilation cache**: each runner compiled from scratch; `Swatinem/rust-cache` was not wired in.
- **No sccache** (though `RUSTC_WRAPPER=sccache` was in the dev env, it was absent from CI runners).
- **Windows running as `continue-on-error: true`** — failures were advisory, not gating.
- **7 `--no-default-features` jobs** for individual crates, running as separate workflow jobs with full compiler invocations each.
- **6 redundant plugin release-build jobs** that duplicated what `build-fixtures-linux` (Phase C) would centralize.
- **MSRV verification** done via `cargo test` on the full test suite, slower than necessary since MSRV is a compile-time contract.

The spec estimated baseline at 29 required checks; the actual baseline from branch protection was 23 (the delta is because several running-but-not-gating advisory checks were counted in the spec's analysis but were never in the required list).

The spec estimated final count at 18 required checks; actual final is 14 (the spec's floor assumed all 6 advisory checks-in-use would remain; they were not required and were cleanly dropped).

## Decisions

### Decision 1 — Conservative within-constraint optimization path

Full e2e coverage on every OS × toolchain combination is preserved. The optimization goal was to eliminate redundant *compilation* and redundant *advisory* checks, not to thin the test matrix or relax coverage guarantees. Every test that ran before sub-project E still runs after; it just runs cheaper.

**Rationale:** the spec's brainstorming explicitly rejected test-matrix slimming and smoke-test substitution as coverage regressions.

**Consequences:**
- No tests were removed from any OS or toolchain.
- The 14-check floor is structurally sound: each check covers a real test surface.
- Regression risk of the optimization work itself is bounded; any phase can be reverted independently.

### Decision 2 — Windows promoted from advisory to hard gate (W2)

As part of Phase B, `continue-on-error: true` was removed from all Windows jobs. Windows became a hard gate on the same footing as Linux and macOS.

**Rationale:** a 4-of-4 Windows audit during Phase B confirmed no Windows-specific failures; the advisory flag was historical caution that had outlived its purpose. Keeping it advisory meant Windows failures were silent to the merge gate.

**Consequences:**
- Any Windows-specific breakage now blocks `main` merges.
- CI noise risk from Windows flakes is real but acceptable given the clean audit.

### Decision 3 — MSRV is a compile-time contract; use `cargo check`, not `cargo test`

The `msrv-check / {linux, macos, windows}` jobs run `cargo check` under the MSRV toolchain rather than `cargo test`. The `test-stable` jobs run the full test suite.

**Rationale:** MSRV is about API and syntax availability — a compile-time question. Running the full test suite under MSRV burns runner minutes on tests that already run under stable and adds nothing to the MSRV guarantee.

**Consequences:**
- `msrv-check` jobs are fast (check-only, no test binary linking or execution).
- MSRV violations surface as compile errors, which is the correct signal.
- Behavioral regressions on the MSRV compiler are not caught, but that risk is accepted: the MSRV toolchain is not used in production; the stable toolchain is.

### Decision 4 — Upload specific compiled binaries; rely on rust-cache + sccache for the rest

`target/` directories are never uploaded as artifacts. The `build-fixtures-linux` job (Phase C) builds 9 specific binaries (5 real plugins + 2 toy plugins + tau-cli + controlled-env) into an `_artifacts/` staging directory, uploads that as `linux-fixture-binaries`, and downstream jobs download it. Per-job `Swatinem/rust-cache` with a shared `shared-key` handles incremental reuse within a job type; `mozilla-actions/sccache-action@v0.0.10` provides cross-job rustc caching.

**Rationale:** uploading the full `target/` (multi-GB per crate configuration) was considered and rejected (see Alternatives). Uploading only the final binaries is cheap, deterministic, and eliminates the ~9-minute per-PR redundant plugin compilation.

**Consequences:**
- `build-fixtures-linux` is the single point of failure for the binary artifact. If it fails, all downstream jobs fail. This is acceptable: the build job is fast and its failures are informative.
- Executable bits are stripped by `actions/upload-artifact` — downstream jobs must `chmod +x` the binaries after download (a real fix shipped during Phase C).
- `actions/upload-artifact` preserves directory tree relative to the upload root — the `_artifacts/` staging step was added to give a clean download target (another real fix shipped during Phase C).

### Decision 5 — No reusable workflow files at this CI scale

The composite action `.github/actions/setup-rust` absorbs duplication (toolchain + cache wiring). Reusable workflow files (`.github/workflows/reusable-*.yml` via `workflow_call`) were considered and rejected.

**Rationale:** at 14 required checks across 3 jobs types (test-stable, msrv-check, e2e), the duplication remaining after the composite action is limited. Reusable workflows add indirection that makes CI debugging harder without proportionate reduction in maintenance burden at this scale.

**Consequences:**
- Future job additions copy-paste from the existing job blocks. This is acceptable at the current scale.
- If the workflow grows significantly (10+ job types), reusable workflows should be reconsidered.

### Decision 6 — Phased migration over big-bang

The optimization was delivered in five independently revertable phases (A–E), each landing as a separate PR.

**Rationale:** big-bang CI rewrites are high-risk: a mistake that breaks the merge gate on `main` is a production incident. Phased delivery means each phase can be verified green before the next starts, and any phase can be reverted if it causes problems.

**Consequences:**
- 5 PRs instead of 1; each requires a branch-protection config update after merge to add/remove required check names.
- The phase boundaries (A: tooling; B: matrix split; C: artifact passing; D: job drop; E: feature-flag consolidation) were chosen to be maximally independent.

### Decision 7 — Phase A and Phase B are valid resting points

The design was explicitly structured so that, if phases C–E proved problematic, the repo could remain at Phase A or Phase B indefinitely.

**Phase A** (shared cache + nextest + sccache + mold) is a pure win: it reduces wall time and cache misses with zero structural change to the job graph.

**Phase B** (matrix split + MSRV as check-only + Windows hard gate) is a structural improvement with minor risk (Windows flakes, MSRV-only check). It is a stable long-term configuration even without phases C–E.

**Consequences:**
- Phases C–E can each be reverted without rolling back A or B.
- The spec and plan document this explicitly; if follow-up phases prove too fragile, rolling back to Phase B leaves CI in a better state than pre-optimization.

### Decision 8 — Required check count drops 23 → 14

The spec estimated 29 → 18; actual is 23 → 14.

The 6-check spec-vs-actual baseline delta: the spec's analysis counted all running checks in the CI configuration as required; in practice, several checks were running-but-not-gating (no entry in the branch protection required-checks list). The optimization correctly identified and dropped those.

The 4-check spec-vs-actual final delta: the spec assumed 6 advisory checks would remain; they were not in the required list and were cleanly eliminated.

**Consequences:**
- 14 required checks is a lean, well-justified set.
- Any re-introduction of required checks should be deliberate (a new test surface, not a redundant rebuild).

### Decision 9 — Doctests stay on `cargo test --doc`

`cargo nextest` does not support doctests. The `test-stable` jobs run `cargo nextest run` for all non-doctest tests AND `cargo test --doc` for doctests in the same job step.

**Rationale:** nextest's doctest support is incomplete upstream. Skipping doctests would regress coverage on a class of tests that verifies API documentation is accurate. Running doctests under legacy `cargo test` for only the doctest invocation is the lowest-friction compatibility path.

**Consequences:**
- Two test invocations per `test-stable` job (nextest for unit/integration, cargo test --doc for doctests).
- If nextest gains complete doctest support in a future release, the `--doc` invocation can be dropped.

### Decision 10 — Matrix `-- --ignored` integration test block removed in Phase B

The `test:` matrix had a `features: ["-- --ignored"]` entry that ran `cargo test -- --ignored` to exercise `#[ignore]`'d integration tests. This was removed in Phase B.

**Rationale:** the `-- --ignored` block was a legacy holdover from before the dedicated e2e jobs existed. The sub-project D e2e jobs (`test (tau-sandbox-native e2e / linux)`, `test (tau-runtime e2e / linux)`, `test (tau-plugin-compat / linux)`) cover the integration test scope properly, with correct Linux-only gating and `integration-tests` feature flags. Running them again inside the matrix was redundant and caused `cargo nextest` to exit 4 on empty filter sets (see Errata).

**Consequences:**
- Integration tests run exactly once (in the dedicated e2e jobs), not twice.
- The `--no-tests=pass` flag was added to the nextest invocations to guard against the exit-4 edge case on other empty filter sets.

## What diverged from the spec/plan (errata)

These are honest departures from the original spec and plan, documented for future reference:

1. **sccache-action version:** spec said `v0.0.6`; actual install was `v0.0.10`. The legacy GitHub Actions cache v1 API was sunset in February 2025; v0.0.6 stopped working on GHA runners. Bumped to v0.0.10 which uses the v2 cache API.

2. **`cargo nextest --run-ignored only` exits 4 on empty filter sets:** nextest returns exit code 4 when `--run-ignored only` finds no tests to run (the filter matches nothing). Fixed by adding `--no-tests=pass` to nextest invocations. This was not anticipated in the spec.

3. **macOS test flake exposed by nextest parallelism:** `tau-cli::session::store::tests::list_sessions_returns_descending_by_created_at` flaked under nextest's parallel execution because nextest runs test binaries in parallel (unlike cargo test's serial execution within a binary). The test relied on monotonic ordering from `std::time::SystemTime`, which macOS's clock resolution made non-monotonic under tight loops. Fixed by adding `.config/nextest.toml` with `retries = 2`. The underlying test was also fixed.

4. **`actions/upload-artifact` preserves directory tree:** when uploading files, the artifact contains the full directory tree relative to the repository root. Added an `_artifacts/` staging directory in the `build-fixtures-linux` job to give a clean, predictable download root. The spec did not anticipate this behavior.

5. **`actions/upload-artifact` strips executable bits:** downloaded binaries lack the executable bit. Added `chmod +x` after every artifact download step. The spec did not anticipate this behavior.

6. **Layer 3 tests hardcode `target/debug/tau`:** the plugin-compat Layer 3 tests resolve the tau binary as `target/debug/tau` (not a configurable path). Restored `cargo build -p tau-cli --bin tau` (debug, not release) in the `test-tau-plugin-compat` job to satisfy this constraint. The spec assumed only release builds would be needed.

7. **Plugin binary names have `-plugin` suffix:** the spec and plan referred to plugin binary names as `anthropic`, `ollama`, `openai`, `fs-read`, `shell`. The actual `[[bin]] name` values in the Cargo manifests are `anthropic-plugin`, `ollama-plugin`, `openai-plugin`, `fs-read-plugin`, `shell-plugin`. All `build-fixtures-linux` invocations and artifact copy steps use the correct suffixed names.

8. **Spec baseline / final check counts:** spec estimated 29 → 18; actual is 23 → 14. Explained fully in Decision 8.

## Consequences

**Positive:**
- PR critical path reduced from ~33 minutes to ≤25 minutes (estimated; actual varies by cache hit rate).
- Required check count reduced from 23 to 14 — branch protection is simpler and more meaningful.
- Windows is now a hard gate, surfacing cross-platform regressions immediately.
- MSRV verification is fast and unambiguous (compile-time check only).
- `cargo nextest` runs in CI; local dev can match CI behavior with a one-time `cargo install cargo-nextest --locked`.
- sccache provides cross-job rustc caching; per-job rust-cache provides incremental reuse within a job type. Both layers are now active.
- `mold` linker on Linux reduces link time for large Rust binaries.
- Plugin binaries are compiled once per PR (in `build-fixtures-linux`) and reused by all downstream jobs.
- Each phase was independently verified green; no extended CI outage during migration.

**Negative:**
- `build-fixtures-linux` is a new single point of failure for all Linux e2e jobs. A flake or build failure in that job blocks the entire downstream pipeline.
- Two test invocations per `test-stable` job (nextest + `--doc`) adds minor complexity vs. a single `cargo test`.
- The `_artifacts/` staging step and `chmod +x` post-download are non-obvious CI idioms that future contributors must maintain.

**Neutral:**
- Branch protection required-check names must be manually updated after any job rename. This is a GitHub limitation, not a workflow issue.
- `.config/nextest.toml` `retries = 2` masks parallelism-exposed flakes rather than fixing their root causes. This is an acceptable trade-off for keeping CI green while root-cause fixes are applied.
- The composite action `.github/actions/setup-rust` absorbs toolchain + cache setup duplication; it is not a reusable workflow (see Decision 5).

## Alternatives considered

### Self-hosted runners
Rejected. Self-hosted runners provide more CPU/memory/disk and persistent caches, but they require ongoing infrastructure management (provisioning, rotation, security patching). The sccache + rust-cache combination achieves the cache-sharing goal on GitHub-hosted runners without operational overhead.

### Matrix slimming (reduce OS × toolchain combinations)
Rejected. Removing macOS or Windows from the test matrix would regress cross-platform coverage. The spec explicitly names this as a non-goal: optimization must not sacrifice coverage.

### Smoke tests instead of full e2e on all OS
Rejected. Smoke tests (running a single representative test instead of the full suite) trade coverage for speed. The failures that sub-project D's e2e tests caught (Execute access flag, binary-parent auto-add) would not have been caught by a smoke test. The optimization achieves its speed goals through compilation reuse, not test reduction.

### `target/` artifact passing between jobs
Rejected. A compiled `target/` directory is 2–10 GB per configuration. Uploading it as an artifact would saturate artifact storage and add upload/download overhead that exceeds the compilation time saved. The design uploads only the final binary artifacts (~50 MB total for 9 binaries).

### `cargo-make` or similar task runner
Rejected. `cargo-make` would add a non-Rust dependency to the developer toolchain and the CI runner setup. The composite action and shell steps in the workflow are sufficient for the current level of CI complexity. The added indirection of a task runner is not justified.

### `cargo-hakari` for workspace-hack crate
Rejected. `cargo-hakari` optimizes feature unification across workspace crates to reduce the number of crate recompilations. At the current workspace size (8 crates), the benefit is marginal compared to the maintenance burden of keeping the workspace-hack crate up to date on every `Cargo.toml` change.

## References

- Spec: [`docs/superpowers/specs/2026-05-06-ci-optimization-design.md`](../superpowers/specs/2026-05-06-ci-optimization-design.md)
- Plan: [`docs/superpowers/plans/2026-05-06-ci-optimization.md`](../superpowers/plans/2026-05-06-ci-optimization.md)
- Phase A PR: [#26](https://github.com/LEBOCQTitouan/tau/pull/26) — tooling (shared-key cache + nextest + sccache + mold)
- Phase B PR: [#27](https://github.com/LEBOCQTitouan/tau/pull/27) — matrix split (test-stable + msrv-check) + Windows hard gate
- Phase C PR: [#28](https://github.com/LEBOCQTitouan/tau/pull/28) — build-fixtures-linux + e2e artifact passing
- Phase D PR: [#29](https://github.com/LEBOCQTitouan/tau/pull/29) — drop 6 redundant plugin release-build jobs
- Phase E PR: [#30](https://github.com/LEBOCQTitouan/tau/pull/30) — feature-flag-matrix consolidation + 7 ndf jobs dropped
- Related: [ADR-0017](0017-e2e-landlock-and-driver.md) — sub-project D, which introduced the e2e job infrastructure this optimization builds on
