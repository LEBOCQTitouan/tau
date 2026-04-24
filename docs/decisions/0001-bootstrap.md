# ADR-0001: Bootstrap decisions

**Status:** Accepted
**Date:** 2026-04-24
**Deciders:** Titouan Lebocq

## Context

Tau begins as an empty repository. The first commits make several
decisions that pin the implementation of constitutional guidelines —
license selection (QG10), MSRV value (QG7), workspace shape (Constitution
§1 Crate scope), CI provider (QG6), governance file content (QG10), and
documentation framework (QG8). PG3 ("decisions in commit messages")
is insufficient for these because future ADRs must amend them by
reference, and commit messages are not stable referents.

This ADR records the bootstrap state so future ADRs can supersede
specific items without ambiguity.

## Decision

1. **License: `MIT OR Apache-2.0`.** Dual-licensed via `LICENSE-MIT` and
   `LICENSE-APACHE` at repo root; declared in `[workspace.package].license`.

2. **MSRV: `1.91`.** Pinned in `[workspace.package].rust-version`.
   Verified by the MSRV row of the CI test matrix. Bumped by minor-version
   change only (QG7); never silently in patch releases (QG11).

3. **Workspace shape: 8 crates, all scaffolded empty.** `tau-domain`,
   `tau-ports`, `tau-infra`, `tau-app`, `tau-pkg`, `tau-observe`,
   `tau-runtime`, `tau-cli`. Members listed explicitly in root
   `Cargo.toml` (no glob).

4. **CI: GitHub Actions, 3-OS × 2-toolchain matrix.** Linux + macOS +
   Windows × stable + 1.91. Windows is non-blocking (`continue-on-error`)
   per G15. Three jobs: `fmt`, `clippy`, `test`. No caching, no
   `cargo audit`/`cargo-deny`, no coverage in Plan 1 — Phase 2 additions
   per QG14/QG16.

5. **Conventional Commits enforced by docs only.** `CONTRIBUTING.md`
   describes the format. No `commitlint` or git hooks in Plan 1.

6. **Toolchain: `rust-toolchain.toml` pins `stable` for local dev.**
   CI overrides per matrix entry via `dtolnay/rust-toolchain` action.

7. **Docs framework: Diátaxis.** Per QG8. Sections under `docs/`:
   `tutorials/`, `how-to/`, `reference/`, `explanation/`, `decisions/`.

8. **Process artifacts under `docs/superpowers/`.** Specs (from the
   brainstorming skill) and plans (from the writing-plans skill) are
   kept in-repo so reviewers see how decisions were made. Not part of
   the published Diátaxis tree.

9. **Governance files: README, LICENSE-{MIT,APACHE}, CONTRIBUTING,
   SECURITY, CODE_OF_CONDUCT, GOVERNANCE, ROADMAP, CHANGELOG** at repo
   root, all present from the bootstrap commit. Satisfies QG10 in full.

10. **Lint denials enabled per-crate from day one.**
    `#![forbid(unsafe_code)]` (QG4), `#![deny(missing_docs)]` (QG9),
    `#![deny(rustdoc::broken_intra_doc_links)]` (QG9) on every library
    `lib.rs`. Binary (`tau-cli/main.rs`) carries `forbid(unsafe_code)`
    only; its public surface is the CLI, not Rust items.

## Consequences

**Positive:**

- Future contributors land code, not infrastructure. Every workspace-level
  question is decided.
- Lint denials caught at first commit; no retro-enforcement campaign needed.
- The MSRV CI row catches accidental MSRV drift before merge.
- Diátaxis structure is in place; new docs have an obvious home.

**Negative:**

- Eight empty crates inflate the repo before any code lands. Acceptable —
  retrofitting a workspace member is a larger diff than landing one upfront,
  and the constitution names all eight.
- Dual licensing requires every contributor to be aware of the MIT-or-Apache
  choice; documented in `CONTRIBUTING.md`.

**Neutral:**

- Phase 2 will add `cargo audit` and `cargo-deny` (QG16), `actions/cache`
  for CI speed, and likely commit signing. Each is its own ADR when added.

## Alternatives considered

- **License: Apache-2.0 only.** Loses MIT compatibility with old GPL-2.0
  consumers; rejected for a runtime tool that may be embedded in
  permissively-licensed downstream projects.
- **License: MIT only.** No explicit patent grant; rejected because tau
  has plugin traits that may attract patent claims as the ecosystem
  matures.
- **Workspace: defer empty crates until first content lands.** Rejected
  because retrofitting a workspace member is more diff than landing one
  empty (PR-noise economics).
- **CI: GitLab CI or sourcehut builds.** Rejected because the repo is
  hosted on GitHub; cross-host CI adds tooling without value.
- **Commitlint / Husky for Conventional Commits.** Rejected because it
  adds a Node toolchain dependency to a Rust repo for a solo-maintainer
  project. Revisit if non-conforming commits accumulate (QG23-style
  reactive enforcement).
- **Skip ADR-0001; record decisions in commit messages only.** Rejected
  per QG18; bootstrap pins guideline implementations that need a stable
  referent for amendment.
