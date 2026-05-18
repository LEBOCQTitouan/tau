# CI upgrades — round 1

Date: 2026-05-17
Branch: `feat/ci-upgrades` (worktree at `.claude/worktrees/ci-upgrades`)

Three independent, low-risk improvements to the GitHub Actions setup,
shipped as a single PR. Each touches one file, addresses one specific
gap surfaced by the 2026-05-17 CI audit, and has no behavioural
coupling with the others.

The full audit (with deferred items) is summarised at the end of this
document.

## Item A — preserve `main`-branch cache on fast-follow merges

### Problem

`.github/workflows/ci.yml:18-20` cancels in-flight runs on any push to
the same `(workflow, ref)` pair:

    concurrency:
      group: ci-${{ github.workflow }}-${{ github.ref }}
      cancel-in-progress: true

This is correct for PR branches (newer push supersedes older). It is
incorrect for `main`, because `.github/actions/setup-rust/action.yml:128`
only writes the Swatinem rust-cache on `refs/heads/main`:

    save-if: ${{ github.ref == 'refs/heads/main' }}

When two squash-merges land on `main` within a few minutes (a common
pattern given `.github/workflows/auto-update-prs.yml` chains
follow-on updates), the second push cancels the first run's post-step,
so the cache write never completes. Subsequent PRs restore from a
stale cache.

### Change

Make cancellation conditional on the ref:

    concurrency:
      group: ci-${{ github.workflow }}-${{ github.ref }}
      cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}

### Impact

- PR runs may restore a fresher rust-cache, reducing per-job
  compile-from-cold time.
- Two `main` CI runs can overlap during back-to-back merges. They
  contend only at the cache-write step, where GitHub Actions cache
  resolves duplicate keys with last-writer-wins — no corruption.
- Additional runner cost: at most one extra concurrent run during
  busy merge windows.

### Risk

None for correctness. Marginal compute cost.

## Item B — drop macOS and Windows MSRV legs

### Problem

`msrv-check` runs `cargo check --workspace --all-targets --locked` at
toolchain `1.91` across a 3-OS matrix (`.github/workflows/ci.yml:108-123`).
MSRV is a rustc-version property, not an OS property. The only places
this workspace gates code on `cfg(target_os)` are `tau-sandbox-windows`
(scaffold, mostly `unimplemented!`) and `tau-sandbox-darwin` (real
macOS adapter); both are also covered by `test-stable` on their
native OS at stable toolchain, which catches the realistic regression
class.

Empirical cost (latest CI, run 25990990203):

- `msrv-check / linux`: 50s
- `msrv-check / macos`: 60s   (10× macOS minutes multiplier)
- `msrv-check / windows`: 2m3s (2× Windows multiplier)

Neither macOS nor Windows MSRV legs sit on the wall-clock critical
path; they only consume runner-minutes.

### Change

Collapse the matrix in `.github/workflows/ci.yml`:

- `runs-on: ubuntu-latest`
- Remove `strategy.matrix.os`
- Job name simplified to `msrv-check / linux`
- `shared-key` becomes a constant: `linux-1.91`

### Impact

- ~14 Linux-equivalent runner-minutes saved per CI run (1 min macOS @
  10× + 2 min Windows @ 2×).
- Marginally faster PR completion when the macOS or Windows pools are
  contended.
- No wall-clock CI reduction (these legs are off critical path).

### Risk

An MSRV-1.91-only regression that surfaces *only* on macOS or Windows
*and* is masked by stable toolchain coverage. Probability assessed as
near-zero given current cfg-gated code. Caught at the next MSRV bump
or surfaced in stable Windows/macOS tests.

## Item C — align artifact-action versions

### Problem

Three majors of the artifact actions are referenced in the repo:

- `actions/upload-artifact@v7` (ci.yml line 202, docs-deploy.yml line 225)
- `actions/download-artifact@v4` (place-fixture-binaries/action.yml line 21)
- `actions/download-artifact@v8` (docs-deploy.yml line 250)

All three majors are on the post-v3 storage backend (immutable
artifacts, scoped per workflow run). Cross-major pairing is supported
by GitHub, but the drift adds review friction: anyone touching CI
must mentally track which version applies where, and Dependabot
opens version bumps independently rather than as one bundled PR.

### Change

Upgrade both `download-artifact` invocations to `@v8`:

- `.github/actions/place-fixture-binaries/action.yml`: `@v4` → `@v8`
- `.github/workflows/docs-deploy.yml`: already `@v8`, no change

Upload stays at `@v7` for now — Dependabot will bump it to v8 on the
next release; bumping it here too is out of scope for this PR.

### Impact

- One consistent download-artifact major across CI.
- Future Dependabot bumps land as one PR instead of two.

### Risk

`@v8` download against `@v7` upload is supported. The only observable
behaviour difference in `place-fixture-binaries` is that `@v8` fails
faster + with a clearer error if the named artifact is missing.

## Verification

No Rust code is touched, so the local cargo gate does not apply. The
verification surface is:

- `yamllint` on the modified files (`ci.yml`,
  `place-fixture-binaries/action.yml`) if available; otherwise a
  manual read of the diff.
- Push the branch as a PR. Confirm:
  - `msrv-check / linux` is the only MSRV job listed.
  - `build-fixtures` artifact uploads and the `test-*` jobs that
    `needs: build-fixtures-linux` successfully download via the
    composite action (proves `@v8` download works against `@v7` upload).
  - CI passes green.
- Item A's effect cannot be observed in-PR — it only takes effect on
  `main` runs over time. Verified by code review of the conditional
  expression.

## Out of scope

Deferred items from the 2026-05-17 audit (handled in future PRs if
prioritised):

- Move the debug `tau` binary into `build-fixtures-linux`'s artifact
  to shorten `test-tau-plugin-compat`'s critical-path contribution.
- SHA-pin third-party actions (`EmbarkStudios/cargo-deny-action`,
  `peaceiris/actions-gh-pages`, `taiki-e/install-action`,
  `mozilla-actions/sccache-action`, `Swatinem/rust-cache`,
  `rui314/setup-mold`, `dtolnay/rust-toolchain`,
  `anthropics/claude-code-action`). Dependabot already manages
  bumps weekly per `.github/dependabot.yml`.
- Add `concurrency:` group to `claude-review.yml`'s `release-summary`
  job to prevent duplicate release-summary issues on close-spaced tag
  publishes.
- Fix the 2-step push race in `docs-deploy.yml`'s wipe-then-overlay
  sequence.
- Consolidate `claude.yml` and `claude-review.yml`.

## Audit reference

Full CI audit data, including the per-job timing breakdown that
underpins the prioritisation above, lives in the conversation
transcript that produced this spec. Headline numbers:

- Latest `main` CI run total wall-clock: ~12m41s.
- Critical path: `build-fixtures-linux` (3m55s) →
  `test-tau-plugin-compat` (8m40s).
- All other jobs complete in 30s–2m and run in parallel.
