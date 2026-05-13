# ADR-0027 — Docs deployment (GitHub Pages auto-CD)

**Status:** Accepted 2026-05-13.
**Branch / PR:** `feat/docs-publish` (PR pending).
**Spec:** `docs/superpowers/specs/2026-05-13-docs-deploy-design.md`.
**Plan:** `docs/superpowers/plans/2026-05-13-docs-deploy.md`.

## Context

Documentation lives in `docs/` (Diátaxis structure per QG8) as plain
markdown. There is no published site. Contributors and users read
docs by browsing the repository on github.com. This is acceptable for
a pre-1.0 project but creates two problems as the project matures:

1. There is no per-version snapshot. A user on `tau 0.5` reading
   docs at `HEAD` sees behavior that may not match their installed
   version.
2. ADRs, the Constitution, and the cheatsheet are not navigable as a
   single site with search.

## Decision

Publish `docs/` as a versioned [mdBook](https://rust-lang.github.io/mdBook/)
site on GitHub Pages. The site lives on a `gh-pages` branch with
this layout:

- `/` redirects to `/latest/`.
- `/latest/` is the most recently released build (overwritten each
  release).
- `/v0.X/` (pre-1.0) or `/vX/` (post-1.0) are frozen per-tag builds.
- `/preview/pr-NNN/` are ephemeral PR-preview builds.
- `/versions.json` is a manifest for a future version-picker UI.

Deploys are triggered by:

1. **Release tag push** matching `v[0-9]+.0.0` or `v0.[0-9]+.0`
   (major-equivalent bump per QG11): build → publish `/vX/` + replace
   `/latest/`.
2. **Push to `main` with `[publish-docs]` in the commit message**:
   replace `/latest/` only. Force-publish for hot-fixing docs without
   cutting a release.
3. **PR labeled `docs:publish`**: publish ephemeral `/preview/pr-NNN/`,
   comment URL on the PR. NEVER touches `/latest/`.
4. **`workflow_dispatch`**: manual run.

The deploy uses `peaceiris/actions-gh-pages@v3` with `keep_files: true`
to overlay builds into per-version subdirectories without disturbing
sibling directories. `/latest/` and `/preview/pr-NNN/` are wiped
before the overlay (because they represent a single "current" state);
`/vX/` are write-once.

A separate PR gate workflow (`docs-check.yml`) runs `mdbook build`
plus `mdbook-linkcheck` on every PR that touches `docs/**` or either
workflow file. The gate does not deploy.

## Consequences

**Positive:**

- Users on a specific tau version can read docs that match.
- The full site is searchable, navigable, and link-checked.
- Force-publish marker lets us fix doc typos without a release.
- PR previews make rendered-doc review possible before merge.

**Negative:**

- A second markdown source-of-truth surface exists (`SUMMARY.md`)
  that must be updated when files are added. Mitigated by the PR
  gate failing when `SUMMARY.md` references a missing file.
- The `gh-pages` branch must be created manually once before the
  first deploy.
- Fork PRs cannot trigger preview deploys (we use `pull_request`
  not `pull_request_target` for security). Acceptable trade-off.

**Neutral:**

- This is a docs-publishing concern only; no Rust code changes.
- rustdoc is not published as part of this ADR. A follow-up may
  publish `cargo doc` output under `/api/`.

## Alternatives considered

- **MkDocs / MkDocs Material**: nicer default theme, Python-based.
  Rejected because it pulls Python into the docs pipeline of an
  otherwise pure-Rust project, and mdBook is the idiomatic choice
  (Rust Book, Cargo Book, rustc dev guide all use it).
- **Docusaurus**: built-in versioning and i18n, MDX support. Rejected
  as overkill — drags Node into docs, far more configuration surface
  than the project needs.
- **Jekyll (GitHub Pages default)**: zero CI required, but very
  limited theming and search; harder to extend later.
- **`actions/deploy-pages` with `actions/upload-pages-artifact`**:
  cleaner atomic single-artifact model, but forces rebuilding the
  entire multi-version tree on every deploy. Rejected in favor of
  the overlay model on `gh-pages`.
- **Single-version (always overwrite)**: simplest, but loses
  historical docs. Rejected because per-tag snapshots are the
  motivating use case.
