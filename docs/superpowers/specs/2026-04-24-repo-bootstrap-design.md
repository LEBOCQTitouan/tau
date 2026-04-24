# Tau Repo Bootstrap — Design Spec

**Date:** 2026-04-24
**Sub-project:** Bootstrap (sub-project 0; first of the Phase 0 sequence)
**Author:** Titouan Lebocq
**Status:** Approved for implementation planning

---

## 1. Scope & success criteria

### Scope

Land the empty tau monorepo on `main` with green CI on Linux, macOS, and Windows.
Zero domain logic. Every subsequent sub-project (1–6) plugs into this skeleton
without editing root-level config.

### Done when

- `cargo build --workspace` succeeds locally.
- `cargo test --workspace` succeeds locally (no real tests; rustc-generated
  stubs only).
- `cargo clippy --workspace --all-targets -- -D warnings` succeeds locally.
- `cargo fmt --all -- --check` succeeds locally.
- GitHub Actions CI matrix (Linux + macOS + Windows × stable + MSRV 1.91) is
  green on `main`. Windows is non-blocking per G15.
- Repo contains all six governance files mandated by QG10 plus
  `ROADMAP.md` (PG1) and `CHANGELOG.md` (QG21).
- ADR-0001 records the bootstrap decisions.
- Diátaxis doc skeleton exists with a stub README in each section (QG8).
- Repo pushed to `https://github.com/LEBOCQTitouan/tau` with CI passing on the
  default branch.

### Out of scope (explicit)

- Any code in `lib.rs`/`main.rs` beyond a doc-comment header and the lint
  attributes — no types, no functions.
- Any package-manager logic, plugin trait, `Message` type, sandboxing, or
  serve mode. Each is its own sub-project.
- `cargo-dist`, release automation, GitHub release artifacts (deferred).
- `cargo audit` and `cargo-deny` (QG16 explicitly defers to Phase 2).
- Coverage upload, dependency caching, commitlint hook (not earning their
  keep yet; QG quality posture).
- ROADMAP entries for sub-projects 6+ in detail (out-of-Phase-0).

---

## 2. Workspace layout

```
tau/
├── Cargo.toml                  # [workspace] only — no [package]
├── rust-toolchain.toml         # channel = "stable" (local dev only)
├── rustfmt.toml                # edition = "2021", max_width = 100
├── clippy.toml                 # empty stub
├── .gitignore                  # /target, /.tau, .DS_Store, *.swp
├── README.md
├── LICENSE-MIT
├── LICENSE-APACHE
├── CONTRIBUTING.md
├── SECURITY.md
├── CODE_OF_CONDUCT.md
├── GOVERNANCE.md
├── ROADMAP.md
├── CHANGELOG.md
├── CONSTITUTION.md             # copied from input docs
├── GUIDELINES_CHEATSHEET.md    # copied from input docs
├── .github/
│   └── workflows/
│       └── ci.yml
├── crates/
│   ├── tau-domain/             # lib
│   ├── tau-ports/              # lib
│   ├── tau-infra/              # lib
│   ├── tau-app/                # lib
│   ├── tau-pkg/                # lib
│   ├── tau-observe/            # lib
│   ├── tau-runtime/            # lib (public API surface; will re-export later)
│   └── tau-cli/                # bin (println!("tau") only)
└── docs/
    ├── README.md               # Diátaxis index
    ├── tutorials/README.md
    ├── how-to/README.md
    ├── reference/README.md
    ├── explanation/README.md
    ├── decisions/
    │   ├── README.md           # ADR index + how-to-add
    │   ├── template.md         # MADR-style template
    │   └── 0001-bootstrap.md
    └── superpowers/            # process artifacts (specs/plans), kept in repo
        ├── specs/
        │   └── 2026-04-24-repo-bootstrap-design.md
        └── plans/
            └── 2026-04-24-repo-bootstrap.md   # produced by writing-plans
```

### Crate roster — all 8 scaffolded empty

The constitution's "Crate scope" line names eight crates as canonical. All are
created in Plan 1 with empty `lib.rs`/`main.rs` so future PRs can fill them
without touching workspace-level config (no plumbing churn). Each crate's
`lib.rs` carries the lint attributes from §3 below.

`tau-cli` is the only `[[bin]]`.

`tau-runtime` is the public API surface (G6, QG12). It re-exports nothing in
Plan 1; later sub-projects add re-exports as the API stabilizes.

### Workspace `Cargo.toml`

Resolver = `"2"`. Members listed explicitly (no glob) to make additions
visible in PR diffs. `[workspace.package]` block defines `version = "0.0.0"`,
`edition = "2021"`, `license = "MIT OR Apache-2.0"`, `repository`,
`rust-version = "1.91"`, `authors`. Each crate's `Cargo.toml` inherits via
`{ workspace = true }` so per-crate metadata stays one line.

`[workspace.lints]` blocks are NOT used in Plan 1 (the inherit-from-workspace
syntax for lints is convenient but adds a layer of indirection that pays off
later, not now). Lints are set in each crate's `lib.rs`/`main.rs` head per §3.

---

## 3. Code-level conventions (set at bootstrap, enforced forever)

Every library `lib.rs` starts with:

```rust
#![forbid(unsafe_code)]              // QG4
#![deny(missing_docs)]               // QG9 — applies even with empty file
#![deny(rustdoc::broken_intra_doc_links)]   // QG9

//! Crate-level docstring describing the crate's role in tau.
```

`tau-cli/src/main.rs`:

```rust
#![forbid(unsafe_code)]              // QG4
// QG3 governs library code; binaries may panic. clippy still flags suspicious
// unwraps in the bin per QG1.

fn main() {
    println!("tau");
}
```

`#![deny(missing_docs)]` on an empty `lib.rs` is satisfied by the `//!` module
doc — verified locally before commit.

---

## 4. CI design

One workflow file: `.github/workflows/ci.yml`. Triggers: `push` to `main` and
`pull_request`.

### Three jobs

| Job | Runs on | Command | Notes |
|---|---|---|---|
| `fmt` | `ubuntu-latest` | `cargo fmt --all -- --check` | Fast fail; QG1 |
| `clippy` | `ubuntu-latest` | `cargo clippy --workspace --all-targets -- -D warnings` | One OS sufficient for lints; QG1 |
| `test` | matrix below | `cargo test --workspace --all-targets` then `cargo test --workspace --doc` | QG5 (test layers), QG6 (matrix) |

### Test matrix

`os = [ubuntu-latest, macos-latest, windows-latest]`
`toolchain = [stable, "1.91"]`
6 runners total.

`continue-on-error: ${{ matrix.os == 'windows-latest' }}` — Windows reports
but does not block (G15).

### What we are NOT doing in Plan 1

- No `actions/cache` or `Swatinem/rust-cache` (Plan 2 nice-to-have).
- No `cargo audit`, `cargo-deny`, `cargo-tarpaulin`/coverage (deferred per
  QG16; Phase 2).
- No release jobs, no `cargo publish` automation.
- No status badges in README beyond the GHA workflow badge (one badge keeps
  README scannable).

---

## 5. ADR-0001 — bootstrap decision record

QG18 mandates ADRs for "changes to project guidelines." Bootstrap pins
guideline implementations (license choice, MSRV value, workspace shape, CI
provider). PG3 ("non-ADR decisions in commit messages") is insufficient
because future ADRs need a referent to amend.

### Template (`docs/decisions/template.md`)

MADR-style:

```markdown
# ADR-NNNN: <title>

**Status:** Proposed | Accepted | Deprecated | Superseded by ADR-XXXX
**Date:** YYYY-MM-DD
**Deciders:** <names>

## Context
<situation, forces, constraints>

## Decision
<the choice>

## Consequences
<positive, negative, neutral follow-ons>

## Alternatives considered
<each alt + why rejected>
```

### ADR-0001 records

1. **License: `MIT OR Apache-2.0`.** Rust ecosystem standard. Alts: Apache-only
   (loses MIT compat with old GPL-2.0), MIT-only (no patent grant).
2. **MSRV: `1.91`.** QG7's stable-2 of locally installed `1.93.1`. Pinned in
   `[workspace.package].rust-version` and asserted by the MSRV CI matrix
   entry.
3. **Workspace: 8 crates, all scaffolded empty.** Per Constitution §1
   "Crate scope." Alt: defer empty crates until first content lands —
   rejected because retrofitting a workspace member is more diff than adding
   one upfront.
4. **CI: GitHub Actions, 3-OS × 2-toolchain matrix, Windows non-blocking.**
   Repo on GitHub. Alts: GitLab CI, sourcehut builds — rejected (repo not
   hosted there).
5. **Conventional Commits enforced by docs only (CONTRIBUTING.md).** No
   commitlint hook in Plan 1. Alt: `commitlint` + Husky — rejected because
   it adds a Node toolchain dependency to a Rust repo for marginal value
   on a solo-maintainer project. Revisit if non-conforming commits appear.
6. **Toolchain: `rust-toolchain.toml` pins `stable` for local dev.** CI
   matrix overrides per entry (uses `dtolnay/rust-toolchain` action with
   explicit `toolchain:` input).
7. **Doc framework: Diátaxis.** Per QG8. `docs/{tutorials,how-to,reference,
   explanation,decisions}/` with stub READMEs.
8. **Spec/plan artifacts live under `docs/superpowers/`.** Process
   documentation kept in-repo so future contributors see how decisions were
   made. Not part of the published Diátaxis tree.

---

## 6. Governance file content

Each file is short — Plan 1 produces minimum viable content; sub-projects
flesh out as needed.

| File | Plan 1 content |
|---|---|
| **README.md** | One-paragraph thesis from Constitution §1; "Status: Phase 0 — bootstrap" line; build instructions stub ("Phase 0 — nothing to install yet, see ROADMAP.md"); pointer to CONSTITUTION.md; license footer; one CI badge. |
| **CONTRIBUTING.md** | Conventional Commits required (QG17); PR checklist (CI green, tests, docs, ADR-if-applicable per QG18); QG22 overnight-delay rule; CONSTITUTION.md alignment bar (PG2); how to file an ADR. |
| **SECURITY.md** | Reporting channel: GitHub private security advisories on `LEBOCQTitouan/tau`; response: best-effort (solo maintainer); disclosure via GitHub Security Advisory if severity warrants. |
| **CODE_OF_CONDUCT.md** | Contributor Covenant 2.1 verbatim; contact = GitHub `noreply` address (`<id+LEBOCQTitouan@users.noreply.github.com>` — `<id>` resolved at commit time via `gh api user`). |
| **GOVERNANCE.md** | Solo-maintainer model; decisions by maintainer; ADR amendment process from Constitution §4; explicit note that the multi-maintainer model is to be revisited when a second maintainer joins, via an ADR amending this file. |
| **ROADMAP.md** | Current phase: 0 (bootstrap). Near-term sub-projects 1–5 listed in order (domain types, plugin traits, package manager, runtime, CLI). Out-of-scope = NG1–NG12 verbatim. Updated per PG1/PG4 at phase transitions. |
| **CHANGELOG.md** | Keep-a-Changelog format; single `## [Unreleased]` with `### Added` listing the bootstrap items. |

---

## 7. Commit strategy

Per user request, every step of the implementation plan ends with a commit.
Conventional Commits format. Targeting roughly one commit per file or per
related-file group. Expected ~15–20 commits across Plan 1, all on `main`
(no PR ceremony required for the inaugural bootstrap — the repo has nothing
to PR against until the first push).

QG22's overnight-delay rule applies once `main` exists on the remote: the
final "tag and push" step waits for fresh-eyes review the next day. Plan 1
will mark this as a manual checkpoint, not an automated step.

---

## 8. Risks & rollbacks

| Risk | Likelihood | Mitigation |
|---|---|---|
| MSRV 1.91 unavailable in CI runners | Low | `dtolnay/rust-toolchain@1.91` is supported by the action |
| `#![deny(missing_docs)]` fails on empty `lib.rs` | Low | Each `lib.rs` carries a `//!` module doc |
| Windows test-runner installs `1.91` slowly | Medium | Acceptable; Windows is non-blocking |
| `cargo fmt --check` fails on `cargo new` defaults | Low | rustfmt.toml stays minimal; verified locally before commit |
| ADR template confuses contributors | Low | Template is MADR (widely understood); README in `decisions/` explains |
| Pushing empty crates breaks `cargo publish` (later) | None in Plan 1 | No publishing in Plan 1 |

Rollback: `rm -rf` the working directory and start over. No external state
changed until the GitHub remote is created and pushed (one of the last steps).

---

## 9. Handoff to writing-plans

This spec is the input to the writing-plans skill. The plan it produces will:

- Decompose §2 (workspace), §3 (lints), §4 (CI), §5 (ADR), §6 (governance)
  into bite-sized tasks (2–5 minutes each).
- Show full file contents in each task — no placeholders.
- Commit at every step.
- Verify each commit with the relevant `cargo` subcommand before moving on.
- End with `git push -u origin main` and a manual QG22 fresh-eyes checkpoint.
