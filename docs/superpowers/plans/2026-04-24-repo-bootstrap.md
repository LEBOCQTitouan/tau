# Tau Repo Bootstrap Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the empty tau monorepo on `main` with green CI on Linux, macOS, and Windows, satisfying the Phase 0 governance and quality guidelines from `CONSTITUTION.md`.

**Architecture:** Cargo workspace with 8 hexagonal crates (`tau-domain`, `tau-ports`, `tau-infra`, `tau-app`, `tau-pkg`, `tau-observe`, `tau-runtime`, `tau-cli`). Zero domain logic — every crate is a stub with the lints from QG3/QG4/QG9 enabled so subsequent sub-projects only add code, never plumbing. CI is one GitHub Actions workflow with three jobs (fmt, clippy, test) where test runs on a 3-OS × 2-toolchain matrix and Windows is non-blocking per G15.

**Tech Stack:** Rust 1.93.1 stable, MSRV 1.91 (QG7 stable-2), `cargo` workspace resolver "2", GitHub Actions, MADR-style ADRs. No third-party crates added in Plan 1.

**Spec:** `docs/superpowers/specs/2026-04-24-repo-bootstrap-design.md`

**Working directory:** `/Users/titouanlebocq/code/tau` (currently empty, no git repo). All commands assume this is `cwd`.

**Note for agents:** this plan is *not* run inside a worktree (the brainstorming skill assumes a worktree exists; bootstrap predates that — there is nothing to branch from). Work directly in the working directory.

**Commit policy:** every task ends with a Conventional Commits-formatted commit. Push happens only at Task 18 (after all local verification passes). QG22's overnight-delay rule applies to the push, not to per-commit work — that is the manual checkpoint at Task 19.

---

## File Structure

Each crate's `lib.rs` (or `main.rs` for the binary) is the only Rust file in Plan 1. No `tests/` directories yet — those land when the crates have content. Governance files all live at repo root. The Diátaxis tree (`docs/`) holds documentation; `docs/superpowers/` holds process artifacts (specs and plans).

| Path | Responsibility | Created in |
|---|---|---|
| `.gitignore` | Exclude build artifacts and OS junk | Task 1 |
| `CONSTITUTION.md`, `GUIDELINES_CHEATSHEET.md` | Source-of-truth charter (copied from input) | Task 1 |
| `LICENSE-MIT`, `LICENSE-APACHE` | Dual license (QG10) | Task 2 |
| `Cargo.toml` (root) | Workspace declaration, shared package metadata | Task 3 |
| `rust-toolchain.toml` | Local toolchain pin (CI overrides per matrix entry) | Task 3 |
| `rustfmt.toml`, `clippy.toml` | Formatter & linter config (QG1) | Task 3 |
| `crates/<name>/Cargo.toml` | Per-crate metadata, inherits from workspace | Task 4 (libs), Task 5 (cli) |
| `crates/<name>/src/lib.rs` | Library entry, lints + module doc | Task 4 |
| `crates/tau-cli/src/main.rs` | Binary entry, prints "tau" | Task 5 |
| `docs/{tutorials,how-to,reference,explanation}/README.md` | Diátaxis section indexes (QG8) | Task 6 |
| `docs/decisions/README.md`, `template.md` | ADR index + MADR template | Task 6 |
| `docs/decisions/0001-bootstrap.md` | ADR for bootstrap decisions (QG18) | Task 7 |
| `README.md` | Project entry point | Task 8 |
| `CHANGELOG.md` | Keep-a-Changelog format (QG21) | Task 9 |
| `CONTRIBUTING.md` | Contribution rules (QG17, QG22, PG2) | Task 10 |
| `SECURITY.md` | Vulnerability reporting (QG15) | Task 11 |
| `CODE_OF_CONDUCT.md` | Contributor Covenant 2.1 (QG10) | Task 12 |
| `GOVERNANCE.md` | Solo-maintainer model + amendment process (QG10) | Task 13 |
| `ROADMAP.md` | Current phase + near-term + non-goals (PG1) | Task 14 |
| `.github/workflows/ci.yml` | fmt + clippy + test matrix (QG1, QG6) | Task 15 |

---

## Task 1: Initialize repository and import the constitution

**Files:**
- Create: `/Users/titouanlebocq/code/tau/.git/` (via `git init`)
- Create: `/Users/titouanlebocq/code/tau/.gitignore`
- Create: `/Users/titouanlebocq/code/tau/CONSTITUTION.md`
- Create: `/Users/titouanlebocq/code/tau/GUIDELINES_CHEATSHEET.md`
- Already present: `/Users/titouanlebocq/code/tau/docs/superpowers/specs/2026-04-24-repo-bootstrap-design.md`
- Already present: `/Users/titouanlebocq/code/tau/docs/superpowers/plans/2026-04-24-repo-bootstrap.md` (this file)

- [x] **Step 1.1: Initialize git with `main` as the default branch**

```bash
cd /Users/titouanlebocq/code/tau
git init -b main
```

Expected: `Initialized empty Git repository in /Users/titouanlebocq/code/tau/.git/`.

- [x] **Step 1.2: Verify there is no `user.email` / `user.name` issue locally**

```bash
git config user.email && git config user.name
```

Expected: both print non-empty values. If either is empty, do NOT modify global config — instead set repo-local config:

```bash
git config --local user.name "Titouan Lebocq"
git config --local user.email "75916953+LEBOCQTitouan@users.noreply.github.com"
```

- [x] **Step 1.3: Write `.gitignore`**

```
# Rust
/target
**/*.rs.bk
Cargo.lock     # tracked only for binary crates; root workspace stays untracked

# Tau project-local state
/.tau

# OS / editor junk
.DS_Store
*.swp
*.swo
.idea/
.vscode/

# Coverage
*.profraw
tarpaulin-report.html
```

Note: `Cargo.lock` is intentionally untracked at workspace root because tau is primarily a library distribution surface (`tau-runtime` is a published crate). The `tau-cli` binary is built downstream, not redistributed pre-built in Phase 0. If we add release artifacts later, this decision is revisited via ADR.

- [x] **Step 1.4: Copy constitution and cheatsheet from the input directory**

```bash
cp /Users/titouanlebocq/Downloads/instructions/CONSTITUTION.md /Users/titouanlebocq/code/tau/CONSTITUTION.md
cp /Users/titouanlebocq/Downloads/instructions/GUIDELINES_CHEATSHEET.md /Users/titouanlebocq/code/tau/GUIDELINES_CHEATSHEET.md
```

Expected: both files exist in `/Users/titouanlebocq/code/tau/`. Verify with:

```bash
ls -la /Users/titouanlebocq/code/tau/CONSTITUTION.md /Users/titouanlebocq/code/tau/GUIDELINES_CHEATSHEET.md
```

Note: `CLAUDE_PROMPT.md` is *not* copied — it is meta-guidance for the assistant, not a project artifact. (If you disagree, copy it too — adding it later is one commit.)

- [x] **Step 1.5: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add .gitignore CONSTITUTION.md GUIDELINES_CHEATSHEET.md docs/superpowers/specs/2026-04-24-repo-bootstrap-design.md docs/superpowers/plans/2026-04-24-repo-bootstrap.md
git commit -m "chore: initialize repository with constitution and bootstrap plan

Imports CONSTITUTION.md and GUIDELINES_CHEATSHEET.md as the
project's source of truth (per CLAUDE_PROMPT.md instruction to
read CONSTITUTION.md before any architectural decision).

Spec and plan documents live under docs/superpowers/ as process
artifacts kept in-repo for future reviewers.

Refs: G1, G11, PG1"
```

Expected: one commit, files listed above included.

- [x] **Step 1.6: Verify the commit looks right**

```bash
git log --stat -1
```

Expected: 5 files added, no `Cargo.toml` yet.

---

## Task 2: Add dual MIT / Apache-2.0 license files

**Files:**
- Create: `/Users/titouanlebocq/code/tau/LICENSE-MIT`
- Create: `/Users/titouanlebocq/code/tau/LICENSE-APACHE`

- [x] **Step 2.1: Write `LICENSE-MIT`**

```
MIT License

Copyright (c) 2026 Titouan Lebocq

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [x] **Step 2.2: Fetch the canonical Apache 2.0 license text**

```bash
curl -fsSL https://www.apache.org/licenses/LICENSE-2.0.txt -o /Users/titouanlebocq/code/tau/LICENSE-APACHE
```

Expected: file is ~11 KB, starts with `                                 Apache License`, ends with the appendix on how to apply the license.

Verify:

```bash
head -3 /Users/titouanlebocq/code/tau/LICENSE-APACHE
wc -l /Users/titouanlebocq/code/tau/LICENSE-APACHE
```

Expected first line includes `Apache License`. Line count is approximately 202.

- [x] **Step 2.3: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add LICENSE-MIT LICENSE-APACHE
git commit -m "docs: add dual MIT and Apache-2.0 license

Standard Rust ecosystem dual-licensing per ADR-0001 (forthcoming).
LICENSE-MIT inlined with copyright line; LICENSE-APACHE fetched
verbatim from apache.org.

Refs: QG10"
```

---

## Task 3: Workspace `Cargo.toml` and toolchain config

**Files:**
- Create: `/Users/titouanlebocq/code/tau/Cargo.toml`
- Create: `/Users/titouanlebocq/code/tau/rust-toolchain.toml`
- Create: `/Users/titouanlebocq/code/tau/rustfmt.toml`
- Create: `/Users/titouanlebocq/code/tau/clippy.toml`

- [x] **Step 3.1: Write workspace `Cargo.toml`**

Path: `/Users/titouanlebocq/code/tau/Cargo.toml`

```toml
[workspace]
resolver = "2"
members = [
    "crates/tau-domain",
    "crates/tau-ports",
    "crates/tau-infra",
    "crates/tau-app",
    "crates/tau-pkg",
    "crates/tau-observe",
    "crates/tau-runtime",
    "crates/tau-cli",
]

[workspace.package]
version = "0.0.0"
edition = "2021"
rust-version = "1.91"
license = "MIT OR Apache-2.0"
repository = "https://github.com/LEBOCQTitouan/tau"
authors = ["Titouan Lebocq <75916953+LEBOCQTitouan@users.noreply.github.com>"]
```

Note: members are listed explicitly (no glob) so PR diffs visibly track every workspace addition. `version = "0.0.0"` because nothing is published yet; the first published version in any sub-project becomes `0.1.0` per QG11 SemVer.

- [x] **Step 3.2: Write `rust-toolchain.toml`**

Path: `/Users/titouanlebocq/code/tau/rust-toolchain.toml`

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

This pins local development to whatever stable is currently installed (1.93.1 today). CI overrides per matrix entry.

- [x] **Step 3.3: Write `rustfmt.toml`**

Path: `/Users/titouanlebocq/code/tau/rustfmt.toml`

```toml
edition = "2021"
max_width = 100
```

Two settings only. Anything beyond defaults is justified individually via ADR.

- [x] **Step 3.4: Write `clippy.toml`**

Path: `/Users/titouanlebocq/code/tau/clippy.toml`

```toml
# Empty stub. Clippy lint configuration is added per-need with ADR justification.
# Lint denial level is set per-crate in lib.rs / main.rs and via CI flags.
```

- [x] **Step 3.5: Verify the workspace declaration parses**

```bash
cd /Users/titouanlebocq/code/tau
cargo metadata --format-version 1 --no-deps 2>&1 | head -1
```

Expected: an error of the form `error: failed to load manifest for workspace member ".../crates/tau-domain"` because the member crates do not yet exist. This is the **expected failure** at this step — it confirms the workspace recognizes the listed members.

If you instead see `error: virtual manifests must be configured with [workspace]`, your `[workspace]` block is malformed — fix it.

- [x] **Step 3.6: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add Cargo.toml rust-toolchain.toml rustfmt.toml clippy.toml
git commit -m "build: scaffold cargo workspace and toolchain config

Workspace declares 8 members per Constitution §1 Crate scope;
shared package metadata (license, MSRV, repo, authors) lives in
[workspace.package] so each crate's Cargo.toml stays one-line.

MSRV pinned to 1.91 per QG7 (stable-2 of locally installed
1.93.1).

Refs: G6, QG7, QG10, QG11"
```

---

## Task 4: Scaffold the seven library crates

**Files (created):**
- Create: `/Users/titouanlebocq/code/tau/crates/{tau-domain,tau-ports,tau-infra,tau-app,tau-pkg,tau-observe,tau-runtime}/Cargo.toml`
- Create: `/Users/titouanlebocq/code/tau/crates/{tau-domain,tau-ports,tau-infra,tau-app,tau-pkg,tau-observe,tau-runtime}/src/lib.rs`

- [x] **Step 4.1: Create directory structure**

```bash
cd /Users/titouanlebocq/code/tau
for c in tau-domain tau-ports tau-infra tau-app tau-pkg tau-observe tau-runtime; do
    mkdir -p "crates/$c/src"
done
ls crates/
```

Expected: `tau-app  tau-domain  tau-infra  tau-observe  tau-pkg  tau-ports  tau-runtime`.

- [x] **Step 4.2: Write Cargo.toml for each library crate**

Each library `Cargo.toml` follows this template (only `name` and `description` differ between crates):

```toml
[package]
name = "<NAME>"
description = "<DESCRIPTION>"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
```

(No `[lib]` section needed — Cargo infers `src/lib.rs` by default.)

The seven (`name`, `description`) pairs:

| Crate | Description |
|---|---|
| `tau-domain` | Core domain types for tau (messages, agents, packages, plugin descriptors). |
| `tau-ports` | Port (trait) definitions for tau's hexagonal architecture. |
| `tau-infra` | Infrastructure adapters that implement tau's ports. |
| `tau-app` | Application orchestration layer for tau's runtime. |
| `tau-pkg` | Tau package manager: install, resolve, and manage extension packages. |
| `tau-observe` | Observability primitives for tau (structured logging, tracing). |
| `tau-runtime` | Public Rust API surface for embedding tau as a library. |

Write each file. Example for `tau-domain`:

Path: `/Users/titouanlebocq/code/tau/crates/tau-domain/Cargo.toml`

```toml
[package]
name = "tau-domain"
description = "Core domain types for tau (messages, agents, packages, plugin descriptors)."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
```

Repeat for the other six, substituting `name` and `description`.

- [x] **Step 4.3: Write `src/lib.rs` for each library crate**

Each library `src/lib.rs` follows this template (only the module doc differs):

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! <CRATE-LEVEL DOC>
```

The seven crate-level docs:

| Crate | Crate-level doc |
|---|---|
| `tau-domain` | `Core domain types for tau. Pure data — no I/O, no plugin contracts. See the constitution (G5) for why messages are the universal interaction primitive.` |
| `tau-ports` | `Port (trait) definitions for tau's hexagonal architecture. Adapters in `tau-infra` implement these traits.` |
| `tau-infra` | `Infrastructure adapter implementations of the ports defined in `tau-ports`.` |
| `tau-app` | `Application orchestration for tau's runtime. Wires ports to adapters.` |
| `tau-pkg` | `Tau package manager. Resolves, installs, and verifies extension packages declared by users via `tau install`.` |
| `tau-observe` | `Observability primitives for tau: structured logging, tracing, and the "observe" verb of the four-verb core (G1).` |
| `tau-runtime` | `Public Rust API surface for embedding tau as a library. One of tau's two stable surfaces (G6, QG12); the other is the serve-mode protocol.` |

Example for `tau-domain`:

Path: `/Users/titouanlebocq/code/tau/crates/tau-domain/src/lib.rs`

```rust
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Core domain types for tau. Pure data — no I/O, no plugin contracts.
//! See the constitution (G5) for why messages are the universal interaction primitive.
```

Repeat for the other six. **Important:** `tau-runtime/src/lib.rs` contains an intra-doc link to a non-existent type (`tau-domain::Message`) — do NOT add such a link in Plan 1, since `broken_intra_doc_links` is denied. The doc-strings above are link-free; keep them that way.

- [x] **Step 4.4: Verify the workspace builds**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
```

Expected: every library crate compiles (will report 0 warnings). Output ends with something like `Finished dev [unoptimized + debuginfo] target(s) in <N>s`.

If you see `error: cannot find macro` or `error: unresolved import`, the `lib.rs` template was edited incorrectly — restore it.

- [x] **Step 4.5: Verify the workspace tests run**

```bash
cargo test --workspace --all-targets
cargo test --workspace --doc
```

Expected: zero tests, all "ok" results, exit code 0. Doc-test pass is critical because `#![deny(missing_docs)]` is on; the module `//!` doc satisfies it.

- [x] **Step 4.6: Verify clippy is clean**

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: no warnings, exit code 0.

- [x] **Step 4.7: Verify rustfmt is satisfied**

```bash
cargo fmt --all -- --check
```

Expected: no output, exit code 0.

- [x] **Step 4.8: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add crates/
git commit -m "build: scaffold seven hexagonal library crates

Creates tau-domain, tau-ports, tau-infra, tau-app, tau-pkg,
tau-observe, tau-runtime as empty library crates with the
mandated lints enabled from day one:

  #![forbid(unsafe_code)]                    (QG4)
  #![deny(missing_docs)]                     (QG9)
  #![deny(rustdoc::broken_intra_doc_links)]  (QG9)

Each module-level docstring describes the crate's role per the
constitution's hexagonal architecture (Identity §1).

Subsequent sub-projects fill these crates with content; no
workspace-level config changes required.

Refs: G1, G6, QG3, QG4, QG9"
```

---

## Task 5: Scaffold `tau-cli` binary crate

**Files:**
- Create: `/Users/titouanlebocq/code/tau/crates/tau-cli/Cargo.toml`
- Create: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/main.rs`

- [x] **Step 5.1: Create directory**

```bash
mkdir -p /Users/titouanlebocq/code/tau/crates/tau-cli/src
```

- [x] **Step 5.2: Write `Cargo.toml`**

Path: `/Users/titouanlebocq/code/tau/crates/tau-cli/Cargo.toml`

```toml
[package]
name = "tau-cli"
description = "Command-line interface for tau."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true

[[bin]]
name = "tau"
path = "src/main.rs"
```

Note: the binary is named `tau` (not `tau-cli`) so users invoke `tau install`, not `tau-cli install`.

- [x] **Step 5.3: Write `src/main.rs`**

Path: `/Users/titouanlebocq/code/tau/crates/tau-cli/src/main.rs`

```rust
#![forbid(unsafe_code)]

//! tau command-line interface entry point.
//!
//! Phase 0 bootstrap: prints "tau" and exits. Subcommands land in later sub-projects.

fn main() {
    println!("tau");
}
```

Note: `#![deny(missing_docs)]` is *not* set on the binary because there are no public items to document — the binary's stable surface is its CLI flags (QG12), governed separately. `#![forbid(unsafe_code)]` still applies (QG4).

- [x] **Step 5.4: Verify the binary builds and runs**

```bash
cd /Users/titouanlebocq/code/tau
cargo build --workspace
cargo run -p tau-cli
```

Expected: `Finished ...` then `tau` printed on its own line.

- [x] **Step 5.5: Verify clippy + fmt**

```bash
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: both exit 0 silently.

- [x] **Step 5.6: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add crates/tau-cli/
git commit -m "feat(cli): scaffold tau-cli binary

Phase 0 bootstrap: binary prints \"tau\" and exits. The binary
is named \"tau\" (not \"tau-cli\") so users invoke \`tau install\`
rather than \`tau-cli install\`.

Subcommands land in later sub-projects; this commit only
establishes the CLI entry point and verifies the workspace
produces an executable.

Refs: G3"
```

---

## Task 6: Diátaxis documentation skeleton + ADR template

**Files:**
- Create: `/Users/titouanlebocq/code/tau/docs/README.md`
- Create: `/Users/titouanlebocq/code/tau/docs/tutorials/README.md`
- Create: `/Users/titouanlebocq/code/tau/docs/how-to/README.md`
- Create: `/Users/titouanlebocq/code/tau/docs/reference/README.md`
- Create: `/Users/titouanlebocq/code/tau/docs/explanation/README.md`
- Create: `/Users/titouanlebocq/code/tau/docs/decisions/README.md`
- Create: `/Users/titouanlebocq/code/tau/docs/decisions/template.md`

- [x] **Step 6.1: Create directories**

```bash
cd /Users/titouanlebocq/code/tau
mkdir -p docs/tutorials docs/how-to docs/reference docs/explanation docs/decisions
```

- [x] **Step 6.2: Write `docs/README.md`**

````markdown
# Tau documentation

Tau documentation follows the [Diátaxis](https://diataxis.fr) framework
(QG8). Each subdirectory holds one of the four documentation modes plus
ADRs.

| Directory | Purpose | Reader question |
|---|---|---|
| [`tutorials/`](tutorials/) | Learning-oriented | "Teach me." |
| [`how-to/`](how-to/) | Task-oriented | "Show me how to do X." |
| [`reference/`](reference/) | Information-oriented | "Tell me precisely what X does." |
| [`explanation/`](explanation/) | Understanding-oriented | "Why is X this way?" |
| [`decisions/`](decisions/) | Architecture Decision Records | "Why did we choose X?" |

## Where to start

- New to tau? Start with `tutorials/` (currently empty — see
  [`ROADMAP.md`](../ROADMAP.md) for status).
- Want to do a specific thing? Look in `how-to/`.
- Need a fact about a flag, type, or protocol? Look in `reference/`.
- Want the why? Read [`../CONSTITUTION.md`](../CONSTITUTION.md) and
  the docs under `explanation/`.
- Want the history of a decision? Read the relevant ADR in
  `decisions/`.

## Process artifacts

`docs/superpowers/` holds specs and implementation plans produced by the
brainstorming and writing-plans skills. These are *process* documentation,
not end-user documentation — kept in-repo so reviewers see how decisions
were made.
````

- [x] **Step 6.3: Write the four Diátaxis section READMEs**

Each follows this minimal pattern. Save these one at a time.

Path: `/Users/titouanlebocq/code/tau/docs/tutorials/README.md`

```markdown
# Tutorials

Learning-oriented documentation: lessons that take a newcomer through a
meaningful first experience with tau.

Empty in Phase 0 — see the [ROADMAP](../../ROADMAP.md) for status.
```

Path: `/Users/titouanlebocq/code/tau/docs/how-to/README.md`

```markdown
# How-to guides

Task-oriented documentation: recipes for accomplishing a specific goal
when you already know what you want.

Empty in Phase 0 — see the [ROADMAP](../../ROADMAP.md) for status.
```

Path: `/Users/titouanlebocq/code/tau/docs/reference/README.md`

```markdown
# Reference

Information-oriented documentation: precise, factual descriptions of
tau's interfaces, formats, and protocols. Generated reference (CLI from
clap, schema from schemars) is produced by CI per QG8 and lives outside
the authored tree.

Empty in Phase 0 — see the [ROADMAP](../../ROADMAP.md) for status.
```

Path: `/Users/titouanlebocq/code/tau/docs/explanation/README.md`

```markdown
# Explanation

Understanding-oriented documentation: discursive material on architecture,
trade-offs, and the rationale behind decisions that have not yet hardened
into ADRs.

Empty in Phase 0 — see the [ROADMAP](../../ROADMAP.md) for status.
```

- [x] **Step 6.4: Write `docs/decisions/README.md`**

````markdown
# Architecture Decision Records (ADRs)

This directory holds tau's ADRs. Each ADR is a numbered Markdown file
(`NNNN-title.md`) recording one decision in MADR style.

## When an ADR is required

Per QG18, ADRs are required for:

- Changes to project guidelines (anything in `CONSTITUTION.md`).
- Additions to or breaking changes in public APIs (`tau-runtime` exports,
  serve-mode IPC schema).
- Changes to the serve-mode protocol.
- Changes to the package manifest format.
- Changes to plugin trait boundaries.

Other changes (bugfixes, refactors within a crate, docs updates) do not
require ADRs and are recorded in commit messages and PR discussion (PG3).

## Filing an ADR

1. Copy [`template.md`](template.md) to `NNNN-<short-title>.md` where
   `NNNN` is one greater than the highest existing ADR number.
2. Fill in Context, Decision, Consequences, and Alternatives.
3. Open a PR. The ADR's status starts as **Proposed**.
4. The maintainer reviews. On acceptance, status changes to **Accepted**
   and the PR is merged.
5. Per the Constitution §4 amendment process, guideline-changing ADRs
   wait at least 24 hours between draft and merge.

## Index

| ADR | Title | Status |
|---|---|---|
| [0001](0001-bootstrap.md) | Bootstrap decisions | Accepted |
````

- [x] **Step 6.5: Write `docs/decisions/template.md`**

```markdown
# ADR-NNNN: <title>

**Status:** Proposed | Accepted | Deprecated | Superseded by ADR-XXXX
**Date:** YYYY-MM-DD
**Deciders:** <names>

## Context

<The forces at play, the constraints in effect, the situation that
motivates this decision. Cite guideline numbers (G1, QG3, etc.) where
relevant.>

## Decision

<The choice made, in concrete terms. Not "we should consider X" — "we
do X".>

## Consequences

<Positive, negative, and neutral consequences of the decision. Include
any new obligations the decision creates (tests, docs, follow-up
ADRs).>

## Alternatives considered

<Each alternative considered, and the specific reason it was rejected.
"Easier" or "preference" is not a reason — name the trade-off.>
```

- [x] **Step 6.6: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add docs/README.md docs/tutorials docs/how-to docs/reference docs/explanation docs/decisions/README.md docs/decisions/template.md
git commit -m "docs: scaffold Diátaxis documentation tree

Adds the four Diátaxis sections (tutorials, how-to, reference,
explanation) plus the ADR directory with index and MADR-style
template. Each section README explains its purpose so new
contributors know where to add documentation.

Generated reference (CLI from clap, schema from schemars) is
deferred — CI produces it per QG8 once those crates have content.

Refs: QG8, QG18"
```

---

## Task 7: ADR-0001 — record bootstrap decisions

**Files:**
- Create: `/Users/titouanlebocq/code/tau/docs/decisions/0001-bootstrap.md`

- [x] **Step 7.1: Write ADR-0001**

Path: `/Users/titouanlebocq/code/tau/docs/decisions/0001-bootstrap.md`

````markdown
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
````

- [x] **Step 7.2: Verify the link from `decisions/README.md` works**

```bash
cd /Users/titouanlebocq/code/tau
test -f docs/decisions/0001-bootstrap.md && echo OK
```

Expected: `OK`.

- [x] **Step 7.3: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add docs/decisions/0001-bootstrap.md
git commit -m "docs(adr): ADR-0001 record bootstrap decisions

Records the ten bootstrap-time decisions that pin guideline
implementations: license, MSRV, workspace shape, CI shape,
Conventional Commits enforcement mechanism, toolchain pinning,
docs framework, process-artifact location, governance files,
and per-crate lint denials.

These decisions are referenced by every subsequent ADR that
amends them.

Refs: QG7, QG8, QG9, QG10, QG11, QG18"
```

---

## Task 8: README.md

**Files:**
- Create: `/Users/titouanlebocq/code/tau/README.md`

- [x] **Step 8.1: Write `README.md`**

````markdown
# tau

[![CI](https://github.com/LEBOCQTitouan/tau/actions/workflows/ci.yml/badge.svg)](https://github.com/LEBOCQTitouan/tau/actions/workflows/ci.yml)

> Tau installs and runs agents in the terminal — solo or orchestrated,
> globally or per-project — with skills, tools, MCP servers, LLM
> backends, and pipelines provided as installable packages.

**Status:** Phase 0 — bootstrap. Nothing to install yet. See
[`ROADMAP.md`](ROADMAP.md) for what is shipping next.

## What tau is

Tau is a minimal, terminal-native Rust runtime. Core does four things
(G1):

1. Installs packages.
2. Runs agents (solo or orchestrated).
3. Passes messages between entities.
4. Observes what happens.

Everything domain-specific — LLM backends, tools, pipelines, skills,
MCP servers, SDKs — is a package. Core ships empty (G11).

The full charter is in [`CONSTITUTION.md`](CONSTITUTION.md). Every
decision about what tau is and how it is built defers to that
document. A one-line summary of all 59 guidelines is in
[`GUIDELINES_CHEATSHEET.md`](GUIDELINES_CHEATSHEET.md).

## What tau is not

Tau is not an LLM, not a hosted service, not a package marketplace, not
a workflow engine, not an AI safety harness, not a credential manager.
See [`CONSTITUTION.md` §2](CONSTITUTION.md) for the full list of 12
non-goals.

## Building

Bootstrap-only at the moment:

```bash
cargo build --workspace
cargo test --workspace --all-targets
cargo run -p tau-cli  # prints "tau"
```

Requires Rust 1.91 or newer (MSRV).

## Documentation

Documentation follows [Diátaxis](https://diataxis.fr). See
[`docs/`](docs/) for the structure. Architecture Decision Records
(ADRs) live in [`docs/decisions/`](docs/decisions/).

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). Contributions must align with
the constitution; alignment is the bar, not LLM-vs-human provenance
(PG2).

## License

Dual-licensed under either of:

- Apache License 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([`LICENSE-MIT`](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.

Contributions submitted for inclusion are dual-licensed as above without
additional terms or conditions, per the Apache 2.0 §5 inbound=outbound
norm.
````

- [x] **Step 8.2: Verify links resolve to local files**

```bash
cd /Users/titouanlebocq/code/tau
for f in CONSTITUTION.md GUIDELINES_CHEATSHEET.md ROADMAP.md CONTRIBUTING.md LICENSE-APACHE LICENSE-MIT docs docs/decisions; do
    test -e "$f" || echo "MISSING: $f"
done
```

Expected: only `ROADMAP.md` and `CONTRIBUTING.md` print as `MISSING:` — both land in later tasks. README links to them in anticipation.

- [x] **Step 8.3: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add README.md
git commit -m "docs: add README

Project entry point. Pulls thesis from Constitution §1 and points
to CONSTITUTION.md and the Diátaxis tree for everything else. CI
badge points at the workflow added in Task 15; will go green once
the workflow lands and runs.

Refs: QG10, G1, G11"
```

---

## Task 9: CHANGELOG.md

**Files:**
- Create: `/Users/titouanlebocq/code/tau/CHANGELOG.md`

- [x] **Step 9.1: Write `CHANGELOG.md`**

```markdown
# Changelog

All notable changes to tau are recorded here. Format:
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/). Tau
follows [Semantic Versioning](https://semver.org); pre-1.0, breaking
changes bump the minor (`0.X.Y` → `0.(X+1).0`) per QG11.

## [Unreleased]

### Added

- Initial repository bootstrap: empty Cargo workspace with eight crates
  (`tau-domain`, `tau-ports`, `tau-infra`, `tau-app`, `tau-pkg`,
  `tau-observe`, `tau-runtime`, `tau-cli`).
- `tau-cli` binary that prints `"tau"` and exits.
- Dual-license: MIT OR Apache-2.0.
- Governance files: README, CONTRIBUTING, SECURITY, CODE_OF_CONDUCT,
  GOVERNANCE, ROADMAP (QG10).
- Diátaxis documentation skeleton with ADR directory and MADR template.
- ADR-0001 recording bootstrap decisions.
- GitHub Actions CI: fmt, clippy, and test on Linux + macOS + Windows
  ×  stable + MSRV 1.91.
- Imported `CONSTITUTION.md` and `GUIDELINES_CHEATSHEET.md` as the
  project's source of truth.

### Changed

- Nothing yet.

### Deprecated

- Nothing yet.

### Removed

- Nothing yet.

### Fixed

- Nothing yet.

### Security

- Nothing yet.

[Unreleased]: https://github.com/LEBOCQTitouan/tau/commits/main
```

- [x] **Step 9.2: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add CHANGELOG.md
git commit -m "docs: add CHANGELOG with Unreleased bootstrap entries

Keep-a-Changelog format per QG21. Single [Unreleased] section
with the bootstrap items in 'Added'; promoted to a versioned
section when the first 0.x release ships.

Refs: QG21, QG11"
```

---

## Task 10: CONTRIBUTING.md

**Files:**
- Create: `/Users/titouanlebocq/code/tau/CONTRIBUTING.md`

- [x] **Step 10.1: Write `CONTRIBUTING.md`**

````markdown
# Contributing to tau

Thanks for considering a contribution. Tau is run as a solo-maintainer
project today (see [`GOVERNANCE.md`](GOVERNANCE.md)), but external
contributions are welcome when they align with the constitution.

## Before you start

1. **Read [`CONSTITUTION.md`](CONSTITUTION.md).** It is the source of
   truth for what tau is, what tau is not, and how tau is built. The
   one-line summary in
   [`GUIDELINES_CHEATSHEET.md`](GUIDELINES_CHEATSHEET.md) is for
   reference once you have read the full text.

2. **Check the alignment bar.**
   - Phase 0–1: bug fixes and documentation PRs are welcome directly.
     Feature contributions need an issue discussion first (PG2).
   - Phase 2+: any aligned PR is welcome regardless of origin. LLM-
     generated PRs face the same alignment bar as human-authored ones;
     provenance does not excuse misalignment.

3. **Check the non-goals.** Tau has 12 explicit non-goals
   ([`CONSTITUTION.md` §2](CONSTITUTION.md)). If your idea trips one,
   it does not belong in tau core — it might fit in a plugin or a
   downstream project.

## Workflow

1. Fork the repo and branch off `main`.
2. Make your change. Follow the rules in the next section.
3. Ensure CI passes locally (commands below).
4. Open a PR. Reference any related issue.
5. Wait for review. Per QG22, even maintainer PRs wait overnight before
   merge — fresh eyes catch what tired eyes miss.

## Code rules

- **Conventional Commits** (QG17). Format: `<type>(<scope>): <subject>`,
  followed by an optional body. Types: `feat`, `fix`, `docs`, `style`,
  `refactor`, `perf`, `test`, `build`, `ci`, `chore`. Scope is the crate
  name (`tau-domain`, `tau-cli`, etc.) or empty for repo-wide changes.
  Body explains *why*, not just *what* (PG3).

- **Tests required** (QG5). Four mandatory layers:
  - Unit tests inline with code.
  - Integration tests in `tests/` per crate.
  - Doc tests on every public API item.
  - CLI behavioral tests via `assert_cmd` for `tau-cli`.
  Plus property tests (`proptest`) for parsers of external input
  (manifest, IPC messages, config), and fuzz targets for the IPC
  protocol once it lands.

- **Docs required** (QG9). `#![deny(missing_docs)]` is enforced on
  every library crate. Every public item gets at least one rustdoc
  example.

- **No `.unwrap()` / `.expect()` / `panic!()` in library code** (QG3).
  Propagate errors with `thiserror`-typed errors. The binary
  (`tau-cli`) may use `anyhow` and may panic.

- **No `unsafe`** without an ADR (QG4).

- **No new dependency without justification in the PR description**
  (QG25): why this crate, why not std, what is the license, how
  actively maintained.

- **No silent tech debt** (QG24). If your change introduces something
  that needs follow-up, file an issue tagged `tech-debt` and link it
  from the PR.

## Local checks

Before opening a PR, run the same commands CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace --all-targets
cargo test --workspace --doc
```

All five exit `0` on success.

## Filing an ADR

If your change touches anything in the QG18 list — guidelines, public
APIs, the serve-mode protocol, the package manifest format, plugin
trait boundaries — your PR must include an ADR. Copy
[`docs/decisions/template.md`](docs/decisions/template.md) and number
sequentially. Per Constitution §4, guideline-changing ADRs wait at
least 24 hours between draft and merge.

## License

By contributing, you agree your contribution is dual-licensed under
MIT or Apache-2.0 at the project's option, per the Apache 2.0 §5
inbound=outbound norm.
````

- [x] **Step 10.2: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add CONTRIBUTING.md
git commit -m "docs: add CONTRIBUTING with Conventional Commits and QG22

Codifies the contribution rules from the constitution: alignment
bar (PG2), Conventional Commits (QG17), four mandatory test
layers (QG5), missing-docs denial (QG9), no panics in library
code (QG3), unsafe forbidden (QG4), dependencies justified
(QG25), no silent debt (QG24), QG22 overnight delay before
merge.

Refs: PG2, PG3, QG3, QG4, QG5, QG9, QG17, QG18, QG22, QG24, QG25"
```

---

## Task 11: SECURITY.md

**Files:**
- Create: `/Users/titouanlebocq/code/tau/SECURITY.md`

- [x] **Step 11.1: Write `SECURITY.md`**

```markdown
# Security policy

## Reporting a vulnerability

Please report security vulnerabilities via **GitHub private security
advisories** rather than public issues:

<https://github.com/LEBOCQTitouan/tau/security/advisories/new>

Include:

- A description of the vulnerability and its impact.
- Steps to reproduce.
- Affected versions (commit SHA or tag).
- Any proposed mitigation.

## Response

This is a solo-maintainer project. Response is best-effort; expect
acknowledgement within a week. If the report is confirmed, the
maintainer will:

1. Work with you privately to develop a fix.
2. Prepare a coordinated disclosure timeline (typically 30–90 days
   depending on severity).
3. Issue a fix release.
4. Publish a GitHub Security Advisory; if severity warrants, request
   a CVE through GitHub.

## Scope

Security issues in tau core and the published `tau-runtime` crate are
in scope. Issues in third-party packages installed via `tau install`
should be reported to those packages' maintainers — tau does not
mediate disclosure for ecosystem packages (NG4, NG7).

Per the constitution, tau is not an AI safety harness (NG8). Reports
about agent output quality, alignment, or truthfulness are out of
scope; please direct them to the agent author or the LLM backend
provider.

## Supply chain

`cargo audit` and `cargo-deny` are scheduled for Phase 2 (QG16). Until
then, dependency vulnerabilities may be reported through this channel
even if they would normally be flagged by automation.
```

- [x] **Step 11.2: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add SECURITY.md
git commit -m "docs: add SECURITY policy with private advisory channel

Vulnerability reports go through GitHub private security
advisories; response is best-effort given solo-maintainer
context. Out-of-scope items (third-party packages, agent
quality) point to the appropriate venues per NG4, NG7, NG8.

Refs: QG15, NG4, NG7, NG8"
```

---

## Task 12: CODE_OF_CONDUCT.md

**Files:**
- Create: `/Users/titouanlebocq/code/tau/CODE_OF_CONDUCT.md`

- [x] **Step 12.1: Fetch the canonical Contributor Covenant 2.1**

```bash
cd /Users/titouanlebocq/code/tau
curl -fsSL https://www.contributor-covenant.org/version/2/1/code_of_conduct/code_of_conduct.md -o CODE_OF_CONDUCT.md
```

Verify:

```bash
head -3 CODE_OF_CONDUCT.md
grep -c "Contributor Covenant" CODE_OF_CONDUCT.md
```

Expected: title is `# Contributor Covenant Code of Conduct`. The grep finds at least 3 mentions.

- [x] **Step 12.2: Insert the contact email**

The downloaded file contains a placeholder line `[INSERT CONTACT METHOD]`. Replace it with the project's noreply address.

```bash
cd /Users/titouanlebocq/code/tau
sed -i.bak 's|\[INSERT CONTACT METHOD\]|75916953+LEBOCQTitouan@users.noreply.github.com|g' CODE_OF_CONDUCT.md
rm CODE_OF_CONDUCT.md.bak
grep -c "LEBOCQTitouan" CODE_OF_CONDUCT.md
```

Expected: at least 1 match. Confirm no `INSERT CONTACT` remains:

```bash
grep -c "INSERT CONTACT" CODE_OF_CONDUCT.md || echo "no placeholders remaining"
```

Expected output: `no placeholders remaining` (the `|| echo` handles the grep-zero-matches exit code).

- [x] **Step 12.3: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add CODE_OF_CONDUCT.md
git commit -m "docs: add Contributor Covenant 2.1 code of conduct

Verbatim Contributor Covenant 2.1, with the contact placeholder
replaced by the maintainer's GitHub noreply address. Standard
Rust ecosystem choice.

Refs: QG10"
```

---

## Task 13: GOVERNANCE.md

**Files:**
- Create: `/Users/titouanlebocq/code/tau/GOVERNANCE.md`

- [x] **Step 13.1: Write `GOVERNANCE.md`**

````markdown
# Governance

## Model

Tau is a solo-maintainer project. The maintainer (Titouan Lebocq) makes
all final decisions about scope, design, and what merges. This is
recorded explicitly because it has implications for response times,
risk concentration, and the bar for accepting outside contributions.

Solo maintenance is **provisional**. When a second maintainer joins,
this file must be amended via an ADR that:

1. Names the second maintainer.
2. Defines the decision process for disagreement (e.g. "consensus, or
   the original maintainer breaks ties").
3. Defines the bus-factor mitigation (key rotation, signing key
   handoff, repo admin transfer plan).

Until that ADR exists, treat the bus factor as 1 — relevant for
downstream consumers planning long-term dependencies on tau.

## Decision rights

| Decision class | Who decides | How recorded |
|---|---|---|
| Bug fix, refactor within a crate, docs update | Maintainer or contributor | Commit message + PR (PG3) |
| New feature, public-API addition or break, protocol change, manifest change, plugin trait change, guideline change | Maintainer | ADR in `docs/decisions/` (QG18) |
| Release | Maintainer | CHANGELOG entry + git tag (QG21) |
| Security disclosure | Maintainer + reporter | GitHub Security Advisory (SECURITY.md) |

## Amending the constitution

Per [`CONSTITUTION.md` §4](CONSTITUTION.md), the constitution changes
only via ADRs. ADRs that propose guideline changes:

1. Explain what guideline is being added, modified, or removed.
2. Explain the situation that motivated the change.
3. State the replacement text explicitly.
4. Reference any PRs, issues, or retrospectives that contributed.

For a solo-maintainer project the maintainer decides; in the
overnight-delay spirit of QG22, guideline-changing ADRs wait at least
24 hours between drafting and merging, except for typo or formatting
corrections.

## Code of conduct enforcement

Reports go to the address in [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
The maintainer enforces. With one maintainer there is no escalation
path; if the report concerns the maintainer, escalate to GitHub Trust &
Safety (<https://support.github.com/contact/report-abuse>).

## License changes

Tau is dual-licensed MIT OR Apache-2.0. Relicensing requires consent
from every contributor whose code is still in the tree. Tau accepts
contributions only under inbound=outbound (Apache 2.0 §5), which
preserves this option for a future relicense ADR but does not pre-grant
it.

## Forks

Forks are welcome under either of the project licenses. The "tau"
trademark — to the extent one exists — is held by the maintainer; forks
should choose a different name to avoid downstream confusion.
````

- [x] **Step 13.2: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add GOVERNANCE.md
git commit -m "docs: add GOVERNANCE describing solo-maintainer model

Documents the solo-maintainer model explicitly, including the
bus-factor implication and the ADR-required path to a multi-
maintainer model. Records decision rights per change class
(routine vs ADR-required per QG18) and the amendment process
from Constitution §4.

Refs: QG10, QG18, QG22, Constitution §4"
```

---

## Task 14: ROADMAP.md

**Files:**
- Create: `/Users/titouanlebocq/code/tau/ROADMAP.md`

- [x] **Step 14.1: Write `ROADMAP.md`**

````markdown
# Tau roadmap

This document tracks current phase, near-term priorities, and
explicit out-of-scope items. Updated at phase transitions per PG1 and
PG4.

For per-issue tracking, see [GitHub
Issues](https://github.com/LEBOCQTitouan/tau/issues).

## Current phase: 0 — bootstrap

**Goal:** empty repo with green CI, full governance files, and the
hexagonal workspace skeleton in place. No domain logic.

**Status:** in progress (this commit is part of the bootstrap).

**Done when:** the bootstrap implementation plan
(`docs/superpowers/plans/2026-04-24-repo-bootstrap.md`) is complete and
CI is green on `main` for Linux, macOS, and Windows.

## Near term — Phase 0 sub-projects

Each sub-project below produces working, testable software on its own
and ships in its own brainstorm → spec → plan → implementation cycle.

| # | Sub-project | Produces |
|---|---|---|
| 0 | Repo bootstrap *(this one)* | Empty workspace + governance + CI |
| 1 | `tau-domain` Message + Agent + Package types | Pure-types crate with `thiserror` errors, doc tests, proptest for parsers |
| 2 | `tau-ports` plugin traits | Trait definitions for LLM backend, tool, storage, sandbox |
| 3 | `tau-pkg` package manager | `tau install` from git URLs, capability declarations parsed (G14), scope resolution (G8) |
| 4 | `tau-runtime` agent lifecycle + message passing | Spawn an agent, deliver messages, observe via structured logs (solo path only) |
| 5 | `tau-cli` real subcommands | `tau install`, `tau run`, `tau ls` |

Once 1–5 land, Phase 0 is complete. A retrospective per PG4 closes the
phase and updates this file with Phase 1 priorities.

## Phase 1 (preview)

Subject to retrospective:

- Serve mode (JSON-RPC over stdio) — second public surface (G6, QG12)
- Sandboxing implementation — fulfils G12 (mechanism TBD via ADR)
- Performance budgets enforced in CI (QG14)
- `cargo audit` and `cargo-deny` (QG16)

## Out of scope (forever)

These are tau's explicit non-goals from
[`CONSTITUTION.md` §2](CONSTITUTION.md). They will not be added to
core regardless of demand:

- **NG1.** Tau is not an LLM or an agent.
- **NG2.** Tau is not a coding-specific tool.
- **NG3.** Tau is not a hosted service.
- **NG4.** Tau is not a package marketplace.
- **NG5.** Tau is not a general-purpose workflow engine.
- **NG6.** Tau does not provide persistent agent memory in core.
- **NG7.** Tau does not evaluate agent quality.
- **NG8.** Tau is not an AI safety harness.
- **NG9.** Tau does not manage identity, authentication, or
  credentials.
- **NG10.** Tau does not collect telemetry or training data.
- **NG11.** Tau is a developer tool, not an end-user tool.
- **NG12.** Tau is a runtime, not a framework.

Adjacent ideas may belong in plugins or downstream projects (such as
`stature`, the opinionated coding pipeline planned as a separate
project).
````

- [x] **Step 14.2: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add ROADMAP.md
git commit -m "docs: add ROADMAP with Phase 0 sub-projects and non-goals

Tracks current phase (0, bootstrap) and the five sub-projects
that complete Phase 0: tau-domain types, tau-ports traits,
tau-pkg package manager, tau-runtime agent lifecycle, tau-cli
real subcommands.

Lists NG1-NG12 verbatim as forever-out-of-scope to set
expectations and reduce scope-creep pressure (PG1).

Refs: PG1, PG4, NG1-NG12"
```

---

## Task 15: GitHub Actions CI workflow

**Files:**
- Create: `/Users/titouanlebocq/code/tau/.github/workflows/ci.yml`

- [x] **Step 15.1: Create directory**

```bash
mkdir -p /Users/titouanlebocq/code/tau/.github/workflows
```

- [x] **Step 15.2: Write `ci.yml`**

Path: `/Users/titouanlebocq/code/tau/.github/workflows/ci.yml`

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

# Cancel superseded runs on the same ref.
concurrency:
  group: ci-${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  fmt:
    name: rustfmt
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt
      - run: cargo fmt --all -- --check

  clippy:
    name: clippy
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - run: cargo clippy --workspace --all-targets -- -D warnings

  test:
    name: test (${{ matrix.os }} / ${{ matrix.toolchain }})
    runs-on: ${{ matrix.os }}
    continue-on-error: ${{ matrix.os == 'windows-latest' }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
        toolchain: [stable, "1.91"]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@master
        with:
          toolchain: ${{ matrix.toolchain }}
      - run: cargo test --workspace --all-targets
      - run: cargo test --workspace --doc
```

Notes:
- `dtolnay/rust-toolchain@stable` is a tagged release for the action; the
  `toolchain` value `stable` means "current Rust stable channel".
- `dtolnay/rust-toolchain@master` is used for the matrix toolchain entry
  because the action's tagged releases require the `toolchain` input to
  be a fixed string at YAML-parse time and `master` accepts the matrix
  expansion. (This is the documented pattern in the action's README.)
- `continue-on-error` only marks Windows non-blocking *for the workflow's
  pass/fail status*. Windows results still appear in the GitHub Actions
  UI for visibility, satisfying G15's "tracked but do not block" intent.
- `fail-fast: false` ensures Linux/macOS results are visible even if
  Windows fails first.
- No `actions/cache` step. Caching lands as a Phase 2 ADR if CI time
  becomes painful.

- [x] **Step 15.3: Verify the YAML parses (syntactic check only)**

```bash
cd /Users/titouanlebocq/code/tau
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))" 2>&1 || echo "yaml parse failed"
```

Expected: no output (success) or a clear pyyaml error if the file is malformed. `python3` and PyYAML are commonly available on macOS; if PyYAML is missing, install it (`pip3 install pyyaml`) or skip this step — the next push will validate the file via GitHub Actions itself.

- [x] **Step 15.4: Stage and commit**

```bash
cd /Users/titouanlebocq/code/tau
git add .github/workflows/ci.yml
git commit -m "ci: add GitHub Actions workflow with fmt/clippy/test matrix

Three jobs: fmt and clippy on ubuntu-latest only, test on a
3-OS x 2-toolchain matrix (Linux + macOS + Windows x stable +
MSRV 1.91). Windows is non-blocking per G15; fail-fast disabled
so Linux/macOS results stay visible.

No caching, no cargo-audit, no cargo-deny in Plan 1; QG14 and
QG16 schedule those for Phase 2.

Refs: QG1, QG5, QG6, QG7, G15"
```

---

## Task 16: Final local verification before push

**No file changes. No commit.** This task confirms the full repo
satisfies every "Done when" criterion from the spec before pushing to
GitHub.

- [x] **Step 16.1: Confirm all governance files exist**

```bash
cd /Users/titouanlebocq/code/tau
for f in README.md LICENSE-MIT LICENSE-APACHE CONTRIBUTING.md SECURITY.md CODE_OF_CONDUCT.md GOVERNANCE.md ROADMAP.md CHANGELOG.md CONSTITUTION.md GUIDELINES_CHEATSHEET.md; do
    test -f "$f" || echo "MISSING: $f"
done
echo "governance check complete"
```

Expected: only the final `governance check complete` line — no `MISSING:` lines.

- [x] **Step 16.2: Confirm all crates exist with `Cargo.toml` and source**

```bash
cd /Users/titouanlebocq/code/tau
for c in tau-domain tau-ports tau-infra tau-app tau-pkg tau-observe tau-runtime tau-cli; do
    test -f "crates/$c/Cargo.toml" || echo "MISSING: crates/$c/Cargo.toml"
done
test -f crates/tau-cli/src/main.rs || echo "MISSING: crates/tau-cli/src/main.rs"
for c in tau-domain tau-ports tau-infra tau-app tau-pkg tau-observe tau-runtime; do
    test -f "crates/$c/src/lib.rs" || echo "MISSING: crates/$c/src/lib.rs"
done
echo "crate check complete"
```

Expected: only `crate check complete` — no `MISSING:` lines.

- [x] **Step 16.3: Confirm Diátaxis tree exists**

```bash
cd /Users/titouanlebocq/code/tau
for d in tutorials how-to reference explanation decisions; do
    test -f "docs/$d/README.md" || echo "MISSING: docs/$d/README.md"
done
test -f docs/decisions/template.md || echo "MISSING: docs/decisions/template.md"
test -f docs/decisions/0001-bootstrap.md || echo "MISSING: docs/decisions/0001-bootstrap.md"
echo "docs check complete"
```

Expected: only `docs check complete`.

- [x] **Step 16.4: Run the full local CI equivalent**

```bash
cd /Users/titouanlebocq/code/tau
cargo fmt --all -- --check && \
  cargo clippy --workspace --all-targets -- -D warnings && \
  cargo build --workspace && \
  cargo test --workspace --all-targets && \
  cargo test --workspace --doc && \
  echo "ALL CHECKS PASS"
```

Expected last line: `ALL CHECKS PASS`. If any command fails, fix the
issue and re-run before proceeding to Task 17. Common failure modes:

- `fmt --check` fails: run `cargo fmt --all`, inspect the diff, commit
  as `style: apply rustfmt`.
- `clippy` fails on a generated stub: check the `lib.rs` template was
  applied verbatim per Task 4.3.
- `test --doc` fails with `missing_docs`: confirm each `lib.rs` has a
  `//!` module doc.

- [x] **Step 16.5: Verify the git log shows clean Conventional Commits**

```bash
cd /Users/titouanlebocq/code/tau
git log --oneline
```

Expected: 15 commits, each starting with one of: `chore:`, `docs:`,
`docs(adr):`, `build:`, `feat(cli):`, `ci:`. No commit lacks a type
prefix.

If a commit message is malformed and you have not pushed yet, you may
amend the most recent one (`git commit --amend`) or do an interactive
rebase. **Do not amend if you have already pushed.** (This task runs
before push, so amending is safe.)

---

## Task 17: Create the GitHub remote and push

**Files:** none. This task creates external state on GitHub.

⚠️ **Risk note:** this task creates a public repository on
github.com/LEBOCQTitouan/tau. Make sure the working directory contains
only what you intend to publish.

- [x] **Step 17.1: Confirm the working directory is what you want public**

```bash
cd /Users/titouanlebocq/code/tau
git status
git log --oneline | wc -l
ls -la
```

Expected: `git status` shows clean working tree. Log count is 15.
Listing contains exactly the files documented in the spec (no
`.env`, no `node_modules`, no scratch files).

If anything unexpected appears (especially anything with credentials),
**stop**. Investigate and clean up before proceeding.

- [x] **Step 17.2: Create the remote (will fail if it already exists, which is fine)**

```bash
cd /Users/titouanlebocq/code/tau
gh repo create LEBOCQTitouan/tau \
  --public \
  --source=. \
  --description "Minimal terminal-native Rust runtime for installing and running agents." \
  --remote=origin \
  --push=false
```

Expected: `✓ Created repository LEBOCQTitouan/tau on GitHub` followed
by `✓ Added remote https://github.com/LEBOCQTitouan/tau.git`.

If the repo already exists (e.g. you created it manually earlier), the
command fails with `GraphQL: Name already exists on this account`. In
that case, just add the remote:

```bash
git remote add origin https://github.com/LEBOCQTitouan/tau.git
```

Confirm the remote:

```bash
git remote -v
```

Expected: two lines, both for `origin`, both pointing at
`https://github.com/LEBOCQTitouan/tau.git`.

- [x] **Step 17.3: Push `main` and set upstream**

```bash
cd /Users/titouanlebocq/code/tau
git push -u origin main
```

Expected: branch `main` pushed, upstream set to `origin/main`. CI
starts automatically on the push.

- [x] **Step 17.4: Watch the CI run**

```bash
cd /Users/titouanlebocq/code/tau
gh run watch
```

(Selects the most recent run interactively if there's only one.)

Expected: all three jobs (`fmt`, `clippy`, `test`) finish. The `test`
matrix has 6 entries; the 4 Linux + macOS entries must succeed (block
the green badge), the 2 Windows entries report independently
(`continue-on-error`, do not block).

- [x] **Step 17.5: Confirm green status**

```bash
cd /Users/titouanlebocq/code/tau
gh run list --workflow ci.yml --limit 1
```

Expected: `completed  success  CI  ...` for the most recent run. The
README badge will turn green within a minute.

If the workflow shows `failure`:

- For a Linux or macOS test failure: fetch logs (`gh run view <id> --log-failed`), reproduce locally, fix, commit, push.
- For a Windows-only failure: investigate but do not block the bootstrap. File an issue tagged `windows`. The workflow status remains "success" because of `continue-on-error`.
- For a fmt or clippy failure: should have been caught by Task 16.4. Reproduce locally, fix, commit, push.

---

## Task 18: QG22 overnight checkpoint

**No file changes. No commit. Manual delay.**

QG22 requires that work waits overnight before final acceptance. Plan 1
finishes the bootstrap; the "acceptance" point is *after* the overnight
delay, not at the moment Task 17 turns CI green.

- [x] **Step 18.1: Note the time and stop**

Note the timestamp of the last commit (or the merge to `main`):

```bash
cd /Users/titouanlebocq/code/tau
git log -1 --format=%cI
```

Wait at least until the next calendar day before the next batch of
work (sub-project 1: `tau-domain` types). This is not idle time — use
it for any of:

- Reading the constitution and cheatsheet again with fresh eyes.
- Reviewing the published repo from a logged-out browser to see what
  external readers will see first.
- Checking that the README badge shows green.
- Reviewing the diff of the last 14 commits as a single rollup
  (`git log --reverse -p HEAD~14..HEAD`).

If you find something that should change, file an issue tagged
`bootstrap-followup` rather than amending the bootstrap commits — the
bootstrap is now part of repo history.

- [x] **Step 18.2: Sign off Plan 1**

When you have completed the overnight delay and have no findings (or
have filed issues for findings):

- Mark this task done.
- Update `CHANGELOG.md`'s `[Unreleased]` section if anything changed
  during the delay (likely nothing).
- Begin sub-project 1 by running `/superpowers:brainstorming` to spec
  the `tau-domain` types.

---

## Done

When all 18 tasks are checked off, sub-project 0 is complete. The
"Done when" criteria from the spec are all satisfied:

- ✅ `cargo build --workspace` succeeds (Task 16.4).
- ✅ `cargo test --workspace` succeeds (Task 16.4).
- ✅ `cargo clippy --workspace --all-targets -- -D warnings` succeeds (Task 16.4).
- ✅ `cargo fmt --all -- --check` succeeds (Task 16.4).
- ✅ GitHub Actions CI matrix is green on `main` (Task 17.5).
- ✅ All six QG10 governance files plus `ROADMAP.md` exist (Tasks 8–14, verified Task 16.1).
- ✅ ADR-0001 records bootstrap decisions (Task 7).
- ✅ Diátaxis doc skeleton exists with stub READMEs (Task 6).
- ✅ Repo pushed to `https://github.com/LEBOCQTitouan/tau` with CI passing (Task 17).

Next up: sub-project 1, `tau-domain` types. Start with
`/superpowers:brainstorming`.
