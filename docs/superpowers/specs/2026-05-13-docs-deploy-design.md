# Docs deployment — GitHub Pages auto-CD design

**Status:** Draft (spec)
**Date:** 2026-05-13
**Author:** brainstormed via Claude Code

## Goal

Publish the contents of `docs/` as a versioned mdBook site on
GitHub Pages. Releases trigger an automatic deploy; contributors
can also force a deploy from a normal push or from a pull request
without cutting a release.

## Non-goals

- Rendering rustdoc (`cargo doc`) into the published site. That is
  a separate concern and may be added later under `/api/`.
- Custom domain configuration. The initial deploy lives at
  `https://<owner>.github.io/tau/`. Domain wiring is a follow-up.
- A multi-version dropdown picker UI. Deferred until at least two
  versions are live (YAGNI).
- Translating ADRs or how-to docs into bespoke landing pages.
  `SUMMARY.md` links to existing files in place.

## Glossary

- **Major-equivalent bump**: per QG11 in `CONSTITUTION.md`, while
  pre-1.0 the minor digit signals breaking changes
  (`0.X.Y` → `0.(X+1).0`). Post-1.0 the major digit does. A
  "major-equivalent bump" means whichever applies for the current
  series. The auto-publish trigger fires on both shapes.
- **Frozen version**: a `/vX/` (or `/v0.X/`) directory on the
  `gh-pages` branch that was built at the moment a tag was pushed
  and is never modified after that.
- **Latest**: a `/latest/` directory that is overwritten on every
  release-triggered deploy. Root `/` redirects there.

## Architecture

### Generator

[mdBook](https://rust-lang.github.io/mdBook/), Rust-native, used
by The Rust Book, Cargo Book, and the rustc dev guide. Single
binary, fast, idiomatic for a Rust workspace.

Plugins:

- `mdbook-linkcheck` — fail the build if internal links break.

Configuration lives at `docs/book.toml` with `src = "."` so
mdBook reads the existing markdown files in place; it does NOT
require moving anything under a new `src/` directory.

A `docs/SUMMARY.md` file defines the table of contents. Initial
version hand-written, referencing existing files under
`tutorials/`, `how-to/`, `reference/`, `explanation/`,
`decisions/`. Maintained by hand thereafter; CI gate (see Testing)
flags missing pages.

### Site layout on `gh-pages`

```
/                  meta-refresh to /latest/  (single static file)
/latest/           copy of the newest tagged build
/v0.5/             frozen build for tag v0.5.0 (pre-1.0: v0.MINOR)
/v0.6/             frozen build for tag v0.6.0
/v1/               frozen build for tag v1.0.0 (post-1.0: vMAJOR)
/v2/               frozen build for tag v2.0.0
/preview/pr-123/   ephemeral preview for PR #123
/versions.json     manifest of versions for a future picker
```

The path key rule:

- Pre-1.0 tags `v0.X.0` map to `/v0.X/`. Patch tags `v0.X.Y` (Y>0)
  do NOT get a new directory because they are non-breaking.
- Post-1.0 tags `vX.0.0` map to `/vX/`. Minor and patch tags
  `vX.Y.Z` with Y or Z > 0 do not.

`versions.json` is regenerated on each release deploy by scanning
the directories on `gh-pages` and is intended for a future
version-picker UI. It is written today so the data is already
collected when the picker is added.

### Triggers

One workflow file: `.github/workflows/docs-deploy.yml`.

| Event                                                     | Result                                                                                                 |
|-----------------------------------------------------------|--------------------------------------------------------------------------------------------------------|
| `push` of tag matching `v[0-9]+.0.0` OR `v0.[0-9]+.0`     | Build → publish `/vX/` (frozen) AND replace `/latest/`. Update `versions.json`.                        |
| `push` to `main` with `[publish-docs]` in commit message  | Build → replace `/latest/` ONLY. No new `/vX/`. Force-publish for hot-fixing docs without a release.   |
| `pull_request` `[labeled, synchronize]` with label `docs:publish` | Build → publish `/preview/pr-NNN/`. Comment URL on the PR. NEVER touch `/latest/`. |
| `pull_request` `[unlabeled, closed]`                      | Delete `/preview/pr-NNN/` from `gh-pages`.                                                             |
| `workflow_dispatch`                                       | Manual run. Inputs: `target` (`latest` \| `preview` \| `version`), `version` (string, used if `target=version`). |

Rationale for the PR preview rule: publishing PR content to
`/latest/` would let unreviewed (potentially wrong) documentation
become the user-facing default. Preview deploys make the rendered
output reviewable without that risk.

### Deploy mechanism

[`peaceiris/actions-gh-pages@v3`](https://github.com/peaceiris/actions-gh-pages)
with `keep_files: true` and `destination_dir` set to the target
subpath. `keep_files: true` overlays the new build into the target
without wiping sibling directories, which is what preserves prior
`/vX/` builds across deploys.

**Important nuance:** `keep_files: true` overlays rather than
replaces the destination directory. A file that existed in a
prior `/latest/` build but is absent in the new one will persist
unless explicitly removed. For `/latest/` and `/preview/pr-NNN/`
targets (which represent a single "current" state) the workflow
runs a `rm -rf` of the destination on the checked-out `gh-pages`
worktree before the deploy step. For `/vX/` targets nothing is
deleted; the directory is created fresh on a release tag and is
write-once thereafter.

The alternative — `actions/deploy-pages` with `actions/upload-pages-artifact`
— uploads a single artifact representing the entire site and
re-publishes it atomically. That model is cleaner for single-version
sites but forces us to rebuild the entire multi-version tree every
deploy. The overlay model is simpler given our requirements.

### GitHub Pages source

GitHub repo Settings → Pages → "Deploy from a branch", branch
`gh-pages`, folder `/ (root)`. One-time manual setup.

### Permissions

The workflow declares:

```yaml
permissions:
  contents: write        # push to gh-pages
  pull-requests: write   # comment preview URLs on PRs
```

### Caching

GitHub Actions cache for `~/.cargo/bin/mdbook` and
`~/.cargo/bin/mdbook-linkcheck`. First run installs both
(~60–90s). Subsequent runs restore from cache in ~5s.

Cache key: `mdbook-${{ runner.os }}-${{ hashFiles('.github/workflows/docs-deploy.yml') }}`
— invalidates the cache when the workflow (and therefore the
pinned versions) changes.

## Components

### New files (in this PR)

- `docs/book.toml` — mdBook configuration. `src = "."`, output
  dir `book/`. Linkcheck and search plugins enabled.
- `docs/SUMMARY.md` — table of contents. Hand-written initial
  version covering: README, ROADMAP, CONSTITUTION, CHANGELOG, then
  tutorials, how-to, reference, explanation, decisions. Each
  entry links to the existing file path.
- `.github/workflows/docs-deploy.yml` — the main deploy workflow
  described above.
- `.github/workflows/docs-check.yml` — PR gate. Runs `mdbook build`
  and `mdbook-linkcheck` on every PR touching `docs/**` or either
  workflow file. Does NOT deploy.
- `docs/decisions/0027-docs-deployment.md` — MADR-format ADR
  recording the choice of mdBook, multi-version overlay,
  force-publish marker, and PR-preview-only rule.
- `CHANGELOG.md` — entry under `[Unreleased]` → `Added`.

### Files NOT added

- No theme overrides. mdBook's default theme ships acceptable for
  v1. Customization is deferred.
- No JavaScript additions. Default search is sufficient.
- No `_redirects` / `_headers` files. Root meta-refresh is enough.

### Manual one-time user steps

1. Create an empty `gh-pages` branch:
   ```
   git switch --orphan gh-pages
   git commit --allow-empty -m "init gh-pages"
   git push -u origin gh-pages
   git switch -
   ```
2. Repo Settings → Pages → source = `gh-pages` / `/ (root)`.
3. (Optional, later) configure custom domain via Settings → Pages.

The first auto-deploy then populates `/latest/` and the first
`/vX/` directory.

## Data flow

```
                                               ┌─────────────────┐
   git push tag v0.6.0 ──────────────────────► │ docs-deploy.yml │
   git push main "[publish-docs] ..." ───────► │  (build + push) │
   PR labeled docs:publish ──────────────────► │                 │
   manual workflow_dispatch ─────────────────► └────────┬────────┘
                                                        │
                                                        │ mdbook build
                                                        ▼
                                               ┌─────────────────┐
                                               │  book/ output   │
                                               └────────┬────────┘
                                                        │ peaceiris/actions-gh-pages
                                                        │ keep_files: true
                                                        │ destination_dir: latest|vX|preview/pr-N
                                                        ▼
                                               ┌─────────────────┐
                                               │  gh-pages branch │
                                               └────────┬────────┘
                                                        │
                                                        ▼
                                               GitHub Pages serves
                                               https://<owner>.github.io/tau/
```

## Error handling

- `mdbook build` failure → workflow fails, no deploy. Job summary
  surfaces the build error.
- `mdbook-linkcheck` failure → workflow fails. Surfaced the same
  way.
- `peaceiris/actions-gh-pages` failure → workflow fails. The
  `gh-pages` branch is unchanged (the action commits atomically).
- PR preview cleanup failure (rare; race with PR-close webhook) →
  workflow fails non-fatally with a warning annotation. A scheduled
  weekly cleanup job (deferred, out of scope) is the long-term
  catch-net.
- First-run bootstrap: if `gh-pages` does not exist, the workflow
  fails with a clear error pointing to the manual-setup step. We
  do NOT auto-create it because that creates a footgun if the user
  intended Pages to be served from somewhere else.

## Testing strategy

- `docs-check.yml` runs on every PR that touches `docs/**` or
  either workflow. It does `mdbook build` plus `mdbook-linkcheck`
  and fails on errors. This is the regression gate.
- End-to-end smoke test (manual, performed once after merge):
  1. Tag a throwaway `v0.999.0-test` on `main`.
  2. Verify the workflow runs and the site at
     `https://<owner>.github.io/tau/` shows `/v0.999/` and that
     `/latest/` matches.
  3. Delete the tag and the `/v0.999/` directory on `gh-pages`.
- PR-preview smoke test (manual, performed once): open a docs PR,
  add label `docs:publish`, verify a preview URL is commented on
  the PR and the page renders. Close the PR; verify the preview
  directory is removed.

## Security considerations

- The workflow has `contents: write` permission. This is required
  to push to `gh-pages`. No other branch is targeted by the
  workflow.
- The PR-preview trigger uses `pull_request` (not
  `pull_request_target`) so fork PRs cannot access secrets. This
  means forks cannot trigger preview deploys; only branches on the
  main repo can. Acceptable trade-off — preview deploys are an
  internal-contributor convenience.
- The commit-message force-publish marker `[publish-docs]` is
  controlled by branch-protection on `main` (PRs only, no direct
  pushes). A malicious marker in a PR commit message has no effect
  until the PR merges, which requires review.

## YAGNI cuts (deferred)

- **Version-picker UI dropdown.** Deferred until at least two live
  versions exist; `versions.json` is already produced so the
  picker can read it without further backfill.
- **"You are viewing an older version" banner** on `/vX/` pages
  when `/latest/` is newer. Defer.
- **Per-version search index.** mdBook's default search is enough.
- **rustdoc under `/api/`.** Separate workflow, not in this spec.
- **Scheduled cleanup of stale preview directories.** Out of scope;
  the PR-close trigger is sufficient if it succeeds.

## Open questions

None at design freeze. Confirmed during brainstorming:

- Trigger: tag push matching `v[0-9]+.0.0` or `v0.[0-9]+.0`.
- Generator: mdBook.
- Versioning: multi-version with `/latest/` alias, no picker UI in v1.
- Force-publish: commit-message marker for push-to-main; PR label
  for ephemeral previews; `workflow_dispatch` for ad-hoc.

## References

- ADR-0027 (to be written in this PR) — docs deployment decision.
- QG11 in `CONSTITUTION.md` — pre-1.0 minor-bump-as-breaking rule.
- `docs/README.md` — Diátaxis structure used by the existing docs.
