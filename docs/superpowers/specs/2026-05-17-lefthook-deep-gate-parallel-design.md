# Lefthook deep-gate parallelization — design

**Status:** Draft
**Date:** 2026-05-17

## Context

`lefthook.yml`'s `pre-push:deep-gate` is one bash pipeline running 10 Linux CI
jobs sequentially inside a single Podman container. The pipeline is dominated
by cargo build/test execution against a single persistent target-cache volume
(`target/lefthook-podman`). Warm wall-clock is ~3-4 min; cold is ~15-20 min.

The `pre-commit` hook is already `parallel: true` across four commands with
isolated `target/lefthook/*` dirs — well-parallelized. No changes there.

Inside one container, all cargo invocations share one target dir, so the
cargo lock serializes any concurrent *building* cargo. However, two
`cargo nextest run` invocations against a warm target dir each spend most of
their wall time *executing* compiled test binaries — a phase that does NOT
hold the cargo lock. Launching them with `&`/`wait` therefore yields real
wall-clock parallelism on test execution, even though their (cheap, warm)
build/fingerprint phases queue.

Setup steps (`apt-get`, `cargo-nextest` curl, `rustup install 1.91`) are not
cargo invocations and parallelize cleanly.

## Goals

1. Cut wall-clock of warm `lefthook run pre-push` by ≥60s without losing any
   coverage relative to the current 10-stage gate.
2. Preserve CI parity — every stage that runs today still runs.
3. Keep failure output readable: parallel-stage logs must replay grouped in
   stage order, not interleaved.
4. No new files outside `lefthook.yml`. (A pre-built base image is a
   plausible follow-up but out of scope here.)

## Non-Goals

- Changing `pre-commit` (already parallel with isolated target dirs).
- Splitting deep-gate into multiple lefthook commands (separate containers
  re-do apt/nextest/rustup install — regression on warm runs).
- Building a custom base image. Tracked as a follow-up.
- Changes to GitHub Actions CI workflows.

## Design

### Helper functions

Added at the top of the deep-gate bash heredoc.

```bash
# _par <label> <cmd...>
# Launch <cmd> in the background, capturing combined output to a per-stage
# logfile. Print "<pid>:<label>:<logpath>" to stdout so the caller can
# collect tokens and pass them to _wait_all.
_par() {
  local label=$1; shift
  local log=/tmp/par-$label.log
  ( "$@" ) >"$log" 2>&1 &
  echo "$!:$label:$log"
}

# _wait_all <token...>
# For each "<pid>:<label>:<log>" token, wait for the pid, then print the
# log inside an ::group::<label>...::endgroup:: block, in token order.
# Return 1 if any stage exited non-zero; 0 otherwise.
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

### Stage 0 — parallel setup (new)

Replaces the current sequential `apt-get update && apt-get install`,
`cargo-nextest` curl block, and the `rustup install 1.91` line that lives
inside Stage 1.

Run in parallel:
- `apt-deps`  → `apt-get update -qq && apt-get install -y -qq iproute2 nftables podman`
- `nextest-install` → arch-detect + curl `get.nexte.st` if `cargo-nextest` not on PATH
- `rustup-1.91` → `rustup toolchain install 1.91 --profile minimal --no-self-update`

Wait for all three; fail-fast if any exited non-zero.

Savings: `max(apt, nextest, rustup) - sum(...)`. On a warm run with persistent
cargo-cache volume but ephemeral rootfs, `apt-get` (~10–25s) usually
dominates, `rustup` is ~5–10s, `nextest` is ~1–2s (curl is fast, or it's
already installed). Expected: ~10–20s saved.

### Stages 1–5 — stay sequential

`msrv-check`, `test-fixtures-ports`, `feature-flag-matrix`, `build-fixtures`,
`test-stable`. These do significant *compilation* and share artifacts, so
they'd queue on the cargo lock if run in parallel and would also lose the
warm-rebuild-fingerprint benefit. Keep them serial. The only change here is
removing the `rustup toolchain install 1.91` line from Stage 1 (it ran in
Stage 0 already).

### Stage 6 — conformance, parallel

Three plugins (`anthropic`, `ollama`, `openai`), each with `--test conformance`.
Replace the serial `for plugin in ...; do cargo nextest run -p ...; done`
with three `_par` invocations + one `_wait_all`.

Build phases queue briefly on the cargo lock; test executions overlap.
Expected savings (warm): ~15–30s.

### Stages 7, 8, 10 — e2e tests, parallel

- `test-tau-runtime-e2e`: `cargo nextest run -p tau-runtime --features integration-tests --tests`
- `test-tau-sandbox-native-e2e`: `cargo nextest run -p tau-sandbox-native --features integration-tests --tests`
- `test-tau-plugin-compat`: `cargo nextest run -p tau-plugin-compat --features integration-tests --tests`

Three `_par` + `_wait_all`. Test execution is the wall-clock dominator for
this group.
Expected savings (warm): ~30–60s.

### Stage 9 — xtask-plugin-images, sequential, MUST precede the e2e block

`cargo run -p xtask -- build-plugin-images` spawns nested Podman containers
via DooD to build per-plugin docker images. `test-tau-plugin-compat`
(Stage 10 in the original ordering) consumes those images. Therefore
`xtask-plugin-images` MUST run before any parallel block that includes
plugin-compat.

Keep it sequential and run it after `test-stable` but before the e2e
parallel block. This preserves the existing dependency contract while
keeping it isolated from the parallel cargo blocks.

### New ordering

```
0. parallel setup            (apt-deps | nextest-install | rustup-1.91)
1. msrv-check                (cargo +1.91 check, sequential)
2. test-fixtures-ports       (sequential)
3. feature-flag-matrix       (sequential 7-crate loop)
4. build-fixtures            (sequential)
5. binary copy → target/release/
6. test-stable               (workspace nextest + doctests, sequential)
7. conformance PARALLEL      (anthropic | ollama | openai)
8. xtask-plugin-images       (sequential — prereq for plugin-compat)
9. e2e PARALLEL              (tau-runtime | tau-sandbox-native | tau-plugin-compat)
```

### Error semantics

- Any background stage exiting non-zero causes `_wait_all` to return 1,
  which under `set -e` aborts the whole gate with non-zero exit status.
- Other background stages in the same parallel block run to completion
  (i.e., we don't kill siblings on first failure). This is intentional:
  developers usually want to see all failures from one gate run, not just
  the first.
- `_wait_all` always prints every stage's log, in token order, between
  `::group::<label>` markers. Logs never interleave on stdout.

### Cargo target-dir + lock behavior

All cargo invocations continue to share `target/lefthook-podman`. We rely
on:
- cargo's per-target-dir lock to serialize the brief build/fingerprint
  phase of concurrent `nextest run` invocations (no lost work);
- test-binary execution being lock-free, so test execution overlaps;
- `CARGO_INCREMENTAL=0` (already exported) so the persistent target-cache
  volume stays usable across runs.

## Expected impact

- **Warm wall-clock:** −60s to −120s on a ~3–4 min baseline (25–50% faster).
- **Cold wall-clock:** modest improvement (~30–60s from parallel setup +
  e2e exec overlap); the cold path is dominated by sequential compilation
  which we deliberately don't disturb.
- **Failure surface:** unchanged — every stage still runs.

## Risks + mitigations

1. **Interleaved logs on failure → hard to debug.**
   Mitigated by `_par` writing each stage to its own logfile and
   `_wait_all` replaying them grouped in stage order.

2. **Background process leaks if the heredoc aborts mid-flight.**
   The container is `--rm`-ed; any leaked processes inside die with the
   container. No host-side risk.

3. **Memory/CPU oversubscription from 3 concurrent test binaries.**
   Each `nextest` already parallelizes within itself; running 3 in
   parallel on top is real load. Acceptable on dev machines (8+ cores,
   16+ GB) — the typical contributor's environment per `docs/dev-environment.md`.
   If this proves problematic we can cap with `NEXTEST_TEST_THREADS`.

4. **Feature-set rebuilds across parallel e2e invocations might fight
   over target-dir fingerprint state.**
   Each invocation is `-p X --features integration-tests`, so cargo's
   per-crate-feature fingerprinting handles them as independent build
   units. The lock queues them anyway. Worst case: same total work as
   serial; we lose nothing.

## Test plan

1. **Smoke:** `lefthook run pre-push` on the worktree's HEAD. Expect green.
2. **Wall-clock A/B:** record `time lefthook run pre-push` on warm cache
   before and after the change (best of 2 each). Confirm ≥60s improvement.
3. **Log readability:** inspect output for the parallel blocks; confirm
   `::group::<label>` blocks appear in stage order with no interleaved
   lines.
4. **Failure path:** temporarily break one test in one of the parallel
   stages; re-run and confirm:
   - whole gate exits non-zero;
   - the failing stage's log appears in the output;
   - other parallel siblings finish (their logs also appear).
5. **CI parity:** push to feature branch, confirm GHA jobs still green.

## Follow-ups (not in this spec)

- Pre-built `docker/lefthook-base.Dockerfile` with apt deps, nextest, and
  the MSRV toolchain pre-installed, tagged by Dockerfile hash and built
  lazily on first run. Eliminates Stage 0 cost entirely. Separate PR.
- Investigate whether splitting `xtask-plugin-images` into a per-image
  parallel build inside `xtask` itself is worth it.
