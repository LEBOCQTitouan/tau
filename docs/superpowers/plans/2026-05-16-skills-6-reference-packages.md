# Skills-6 Reference Packages Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship 3 reference skill packages (`critic`, `fact-checker`, `pr-reviewer`) under a new `skills/` directory + 6 mdBook documentation pages covering the Diátaxis quadrants + 9 integration tests, closing ROADMAP §16.

**Architecture:** Add-only — no refactoring of existing tempdir test fixtures. New top-level `skills/` directory (sibling to `crates/`) with one subdirectory per reference skill. mdBook pages land under `docs/{tutorials,how-to,reference,explanation}/` and are indexed in `docs/SUMMARY.md`. Two new integration test files exercise install + show + export end-to-end.

**Tech Stack:** Hand-authored TOML + Markdown. Rust integration tests use existing patterns from Skills-5's `crates/tau-pkg/tests/install_anthropic_format.rs` + `crates/tau-cli/tests/cmd_skill_export.rs`. mdBook builds via PR #67's GitHub Pages workflow.

**Branch:** `feat/skills-6-reference-packages` (worktree at `/Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design`, cut from `origin/main` at `e4c5238`).
**Spec:** `docs/superpowers/specs/2026-05-16-skills-6-reference-packages-design.md` (PR #115, open).
**Depends on:** Skills-1 (`1d71032`), Skills-2 (`93dbe95`), Skills-3 (`7bec3ab`), Skills-4 (`1f6f331`), Skills-5 (`419fd2c`).

**CLAUDE.md rules in effect:**
- Every cargo invocation: `timeout <secs> env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/<role> cargo <cmd> -p <crate>`.
- All git operations: `git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design ...`.
- Push via `git push --no-verify`; commit via `git commit --no-verify`.
- 4-5 sibling worktrees active for other Claude sessions — work ONLY in `feat-skills-6-design`.

---

## File structure

| Path | Status | Responsibility |
|---|---|---|
| `skills/README.md` | Create | Table of reference skills + install instructions |
| `skills/critic/tau.toml` | Create | Manifest: kind=skill, capabilities=[] |
| `skills/critic/SKILL.md` | Create | Anthropic-compatible frontmatter + body |
| `skills/fact-checker/tau.toml` | Create | Manifest with `[[capabilities]] kind="fs.read" paths=["${SKILL_DIR}/references/**"]` |
| `skills/fact-checker/SKILL.md` | Create | Body referencing bundled refs/ |
| `skills/fact-checker/references/style-guide.md` | Create | Reference content |
| `skills/fact-checker/references/common-claims.md` | Create | Reference content |
| `skills/pr-reviewer/tau.toml` | Create | Manifest with `[[capabilities]] kind="process.spawn" commands=["git","rg"]` |
| `skills/pr-reviewer/SKILL.md` | Create | Body documenting git+rg workflow |
| `crates/tau-pkg/tests/install_reference_skills.rs` | Create | 4 install integration tests against in-tree skills |
| `crates/tau-cli/tests/reference_skills_e2e.rs` | Create | 5 CLI integration tests (install + show + export) |
| `docs/tutorials/build-your-first-skill.md` | Create | Diátaxis tutorial: narrative walkthrough using critic |
| `docs/how-to/install-a-skill.md` | Create | Diátaxis how-to: recipe |
| `docs/how-to/author-a-skill.md` | Create | Diátaxis how-to: recipe |
| `docs/how-to/export-a-skill.md` | Create | Diátaxis how-to: recipe |
| `docs/reference/skill-manifest-schema.md` | Create | Diátaxis reference: complete schema |
| `docs/explanation/two-layer-skills.md` | Create | Diátaxis explanation: design reasoning |
| `docs/SUMMARY.md` | Modify | Index 6 new mdBook pages |
| `docs/decisions/0030-skills-reference-packages.md` | Create | ADR |
| `.gitattributes` | Modify (or create) | Force `* text=auto eol=lf` for `skills/**/SKILL.md` to prevent Windows CRLF mangling the byte-identical export roundtrip test |

---

## Task 1: `skills/critic/` package

**Files:**
- Create: `skills/critic/tau.toml`
- Create: `skills/critic/SKILL.md`

**Subagent:** haiku.

- [ ] **Step 1: Create the manifest**

```bash
mkdir -p /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design/skills/critic
```

Write `skills/critic/tau.toml`:

```toml
name = "critic"
version = "0.1.0"
description = "Reviews drafts for clarity, completeness, and rhetorical quality."
authors = ["tau contributors <dev@tau>"]
source = "https://github.com/LEBOCQTitouan/tau.git"
kind = "skill"
dependencies = []
capabilities = []

[skill]
```

- [ ] **Step 2: Create the SKILL.md content**

Write `skills/critic/SKILL.md`:

```markdown
---
name: critic
description: Reviews drafts for clarity, completeness, and rhetorical quality.
---

You are a writing critic. Read the user's draft and respond with:

1. **What works.** Two or three concrete strengths.
2. **What's unclear.** Specific passages that lose the reader, with brief
   suggestions for sharpening.
3. **What's missing.** Any audience-facing assumption the draft doesn't earn.

Be specific, not generic. Quote the draft when calling something out.
```

- [ ] **Step 3: Verify manifest parses as a valid `UncheckedManifest`**

Run a quick smoke check by invoking the `tau` binary in dry-run mode (or use `cargo run -p tau-cli`). The simplest verification: any test in T4 that calls `install_with_options` will fail loudly if the manifest is malformed.

For now, sanity-check via `toml` parse:

```bash
cd /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design
python3 -c "import tomllib; print(tomllib.loads(open('skills/critic/tau.toml').read()))" 2>&1 | head -10
```

Expected: a Python dict printout with no parse error. (Python 3.11+ ships `tomllib`.) If Python isn't available, skip this step — T4's install test will catch malformed manifests authoritatively.

- [ ] **Step 4: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design add skills/critic
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design commit --no-verify -m "$(cat <<'EOF'
feat(skills): add critic reference skill package (Skills-6 T1)

First of three reference skills. Capability-less (capabilities=[]),
demonstrates the pure-prompt Anthropic-roundtrip path. Used by
Skills-6's tutorial as the running example and as fixture for the
roundtrip e2e test (T5).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `skills/fact-checker/` package

**Files:**
- Create: `skills/fact-checker/tau.toml`
- Create: `skills/fact-checker/SKILL.md`
- Create: `skills/fact-checker/references/style-guide.md`
- Create: `skills/fact-checker/references/common-claims.md`

**Subagent:** haiku.

- [ ] **Step 1: Create the manifest**

```bash
mkdir -p /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design/skills/fact-checker/references
```

Write `skills/fact-checker/tau.toml`:

```toml
name = "fact-checker"
version = "0.1.0"
description = "Validates factual claims against bundled reference materials."
authors = ["tau contributors <dev@tau>"]
source = "https://github.com/LEBOCQTitouan/tau.git"
kind = "skill"
dependencies = []

[[capabilities]]
kind = "fs.read"
paths = ["${SKILL_DIR}/references/**"]

[skill]
```

- [ ] **Step 2: Create the SKILL.md content**

Write `skills/fact-checker/SKILL.md`:

```markdown
---
name: fact-checker
description: Validates factual claims against bundled reference materials.
---

You are a fact-checker. Use the bundled references at `references/` to
validate claims in the user's input:

- `references/style-guide.md` — house style conventions (acceptable
  phrasings, units, citation format).
- `references/common-claims.md` — vetted statements + their supporting
  evidence.

For each claim in the input:

1. Find the closest match in `references/common-claims.md`.
2. If matched, cite the reference: "Per references/common-claims.md, …".
3. If unmatched but plausible, mark it `[NEEDS VERIFICATION]` rather
   than asserting confidence.
4. If contradicted, quote the reference and call out the contradiction.

When uncertain, say so. Don't fabricate citations.
```

- [ ] **Step 3: Create `references/style-guide.md`**

Write `skills/fact-checker/references/style-guide.md`:

```markdown
# House style guide (excerpt for fact-checker fixture)

## Units

- Distance: kilometers (km) for >1 km, meters (m) otherwise.
- Mass: kilograms (kg) for >1 kg, grams (g) otherwise.
- Temperature: Celsius (°C). Convert Fahrenheit inputs to Celsius before
  reporting.

## Citations

- Cite source as: `[author, year, page]` inline OR `Per <reference-file>`
  if drawing from this fact-checker's bundled references.
- Direct quotes use double quotes. Paraphrases do not.

## Hedging

- Use `[NEEDS VERIFICATION]` rather than asserting unverified specifics.
- Use "approximately" or "circa" for round numbers; cite a precise figure
  only when it can be supported.
```

- [ ] **Step 4: Create `references/common-claims.md`**

Write `skills/fact-checker/references/common-claims.md`:

```markdown
# Common claims (excerpt for fact-checker fixture)

## Geography

- The Pacific Ocean is the largest ocean on Earth, covering approximately
  165,250,000 km² (NOAA, 2020).
- Mount Everest's summit is approximately 8,848.86 m above sea level
  (Survey of India + Nepal joint reannouncement, 2020).

## Computing

- The Rust programming language reached version 1.0 in May 2015 (Rust
  Foundation release notes).
- Git was created by Linus Torvalds in 2005 (initial commit
  `e83c5163316f89bfbde7d9ab23ca2e25604af290`).

## Tau project

- tau is an agentic runtime + CLI implemented in Rust.
- Skills are first-class packages in tau (ROADMAP §16); shipped via
  Skills-1 through Skills-6.
```

- [ ] **Step 5: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design add skills/fact-checker
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design commit --no-verify -m "$(cat <<'EOF'
feat(skills): add fact-checker reference skill package (Skills-6 T2)

Second of three reference skills. Demonstrates the fs.read capability
+ ${SKILL_DIR} substitution + multi-file payload (references/ subdir
with style-guide.md + common-claims.md).

Validates Skills-1's ${SKILL_DIR_VAR} substitution + Skills-4's
runtime resolution of bundled files at spawn time.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `skills/pr-reviewer/` package

**Files:**
- Create: `skills/pr-reviewer/tau.toml`
- Create: `skills/pr-reviewer/SKILL.md`

**Subagent:** haiku.

- [ ] **Step 1: Create the manifest**

```bash
mkdir -p /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design/skills/pr-reviewer
```

Write `skills/pr-reviewer/tau.toml`:

```toml
name = "pr-reviewer"
version = "0.1.0"
description = "Reviews git diffs against the project's coding style + finds nearby callers."
authors = ["tau contributors <dev@tau>"]
source = "https://github.com/LEBOCQTitouan/tau.git"
kind = "skill"
dependencies = []

[[capabilities]]
kind = "process.spawn"
commands = ["git", "rg"]

[skill]
```

- [ ] **Step 2: Create the SKILL.md content**

Write `skills/pr-reviewer/SKILL.md`:

```markdown
---
name: pr-reviewer
description: Reviews git diffs against the project's coding style + finds nearby callers.
---

You are a code reviewer for a Rust project. Workflow:

1. Run `git diff <base>...HEAD` (or whichever ref the user supplies) to
   gather the proposed changes.
2. For each non-trivial change, use `rg <symbol>` to find nearby callers
   or related code the change might affect.
3. Render a review covering:
   - **Well-considered.** Patterns that match the existing codebase.
   - **Risky.** Changes that touch shared invariants without obvious test
     coverage, or that break documented interfaces.
   - **Missing tests.** Code paths the diff adds without corresponding
     test changes.
   - **Style nits.** Only if non-trivial (formatting is for `cargo fmt`).

Be direct. Cite filenames + line numbers. Quote the diff when calling
something out.
```

- [ ] **Step 3: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design add skills/pr-reviewer
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design commit --no-verify -m "$(cat <<'EOF'
feat(skills): add pr-reviewer reference skill package (Skills-6 T3)

Third of three reference skills. Demonstrates the process.spawn
capability with commands=["git", "rg"] — the third major capability
axis after pure-prompt (critic) and fs.read (fact-checker).

Per Skills-2's sandbox_check, process.spawn for these binaries is
conformant across all three tau sandbox tiers (passthrough/strict/
container).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `tau-pkg/tests/install_reference_skills.rs` — 4 integration tests

**Files:**
- Create: `crates/tau-pkg/tests/install_reference_skills.rs`

**Subagent:** sonnet.

**Prior-art reference:** `crates/tau-pkg/tests/install_anthropic_format.rs` is the canonical pattern. It uses `mod fixtures` for git-fixture helpers + `install_with_options` for installs. Read it end-to-end before writing.

- [ ] **Step 1: Recon prior patterns**

```bash
grep -nE "mod fixtures|fn make_tau_skill_fixture|install_with_options|skip_cross_check" /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design/crates/tau-pkg/tests/install_anthropic_format.rs | head -10
```

Note: Skills-5 used `mod fixtures` which is `crates/tau-pkg/tests/fixtures/mod.rs` (helper for git bare-repo setup). The in-tree skills already live at `<workspace_root>/skills/<name>/` and are installable directly via a local-path source — we do NOT need to create bare repos for them. Use `tau install ./skills/critic` semantics (filesystem-path source).

- [ ] **Step 2: Write the test file skeleton**

Create `crates/tau-pkg/tests/install_reference_skills.rs`:

```rust
//! Integration tests for Skills-6 reference skill packages.
//!
//! Each test installs an in-tree skill from `<workspace_root>/skills/<name>/`
//! using the tau-pkg install pipeline + asserts on the resulting lockfile.
//!
//! These exercise the FULL install pipeline (clone → detect format →
//! validate manifest → write LockedPackage) against the actual reference
//! skill content shipped in T1–T3, not a synthesized fixture.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use tau_domain::PackageSource;
use tau_pkg::{install_with_options, InstallOptions, LockFile, Scope};
use tempfile::TempDir;

/// Locate the workspace root from `CARGO_MANIFEST_DIR` + `..`.
/// (CARGO_MANIFEST_DIR is `crates/tau-pkg`; the workspace root is two
/// dirs up.)
fn workspace_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR set in cargo test runs");
    Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolvable from CARGO_MANIFEST_DIR")
        .to_path_buf()
}

/// Path to the in-tree skill directory under `<workspace>/skills/<name>/`.
fn in_tree_skill_path(name: &str) -> PathBuf {
    workspace_root().join("skills").join(name)
}

/// Construct install options suitable for skill installs:
/// - skip_build (skills don't compile anything)
/// - skip_cross_check (skills don't need plugin cross-check)
fn test_install_options() -> InstallOptions {
    let mut opts = InstallOptions::default();
    opts.skip_cross_check = true;
    opts.build.skip_build = true;
    opts
}

/// Set up a project scope tempdir with a `.tau/config.toml` so that
/// `Scope::resolve` finds a project root at the temp directory.
fn setup_project_scope(tmp: &Path) -> Scope {
    std::fs::create_dir_all(tmp.join(".tau")).unwrap();
    std::fs::write(
        tmp.join(".tau").join("config.toml"),
        "schema_version = 3\n\n[sandbox]\nrequired_tier = \"none\"\n",
    )
    .unwrap();
    Scope::resolve(tmp).unwrap()
}

/// Install `<workspace>/skills/<name>/` into a scope via a `file://` URL.
fn install_in_tree_skill(scope_root: &Path, name: &str) -> Result<(), tau_pkg::InstallError> {
    let scope = setup_project_scope(scope_root);
    let skill_path = in_tree_skill_path(name);
    let url = format!("file://{}", skill_path.display());
    let source = PackageSource::from_str(&url).expect("valid file:// URL");
    let opts = test_install_options();
    install_with_options(&scope, source, None, opts)?;
    Ok(())
}

// ----------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------

#[test]
fn install_critic_from_in_tree_path() {
    let tmp = tempfile::tempdir().unwrap();
    install_in_tree_skill(tmp.path(), "critic").expect("install succeeds");

    let lockfile_path = tmp.path().join(".tau").join("tau-lock.toml");
    let lf = LockFile::load(&lockfile_path).expect("lockfile loads");
    assert_eq!(lf.packages.len(), 1);
    let pkg = &lf.packages[0];
    assert_eq!(pkg.name.as_str(), "critic");
    assert_eq!(pkg.active_version.to_string(), "0.1.0");
    assert!(
        pkg.synthesized_from.is_none(),
        "in-tree skills have a tau.toml; synthesized_from should be None"
    );

    // Install dir should contain SKILL.md (copied from source).
    let install_dir = tmp.path().join(".tau").join("packages").join("critic").join("0.1.0");
    assert!(install_dir.join("SKILL.md").exists(), "SKILL.md missing");
    assert!(install_dir.join("tau.toml").exists(), "tau.toml missing");
}

#[test]
fn install_fact_checker_preserves_references_dir() {
    let tmp = tempfile::tempdir().unwrap();
    install_in_tree_skill(tmp.path(), "fact-checker").expect("install succeeds");

    let install_dir = tmp.path().join(".tau").join("packages").join("fact-checker").join("0.1.0");
    assert!(install_dir.join("SKILL.md").exists());
    assert!(install_dir.join("references").join("style-guide.md").exists());
    assert!(install_dir.join("references").join("common-claims.md").exists());

    // Lockfile records the fs.read capability with the SKILL_DIR-relative path.
    let lockfile_path = tmp.path().join(".tau").join("tau-lock.toml");
    let lf = LockFile::load(&lockfile_path).expect("lockfile loads");
    let pkg = lf.packages.iter().find(|p| p.name.as_str() == "fact-checker")
        .expect("fact-checker in lockfile");
    // Capabilities live in the manifest's tau.toml on disk, not in the
    // lockfile per se. Re-parse the installed tau.toml to assert.
    let toml_text = std::fs::read_to_string(install_dir.join("tau.toml")).unwrap();
    assert!(toml_text.contains("fs.read"), "fs.read capability missing");
    assert!(
        toml_text.contains("${SKILL_DIR}/references/**"),
        "${{SKILL_DIR}}/references/** path missing"
    );
    let _ = pkg; // currently unused beyond presence assertion
}

#[test]
fn install_pr_reviewer_records_process_spawn_cap() {
    let tmp = tempfile::tempdir().unwrap();
    install_in_tree_skill(tmp.path(), "pr-reviewer").expect("install succeeds");

    let install_dir = tmp.path().join(".tau").join("packages").join("pr-reviewer").join("0.1.0");
    let toml_text = std::fs::read_to_string(install_dir.join("tau.toml")).unwrap();
    assert!(toml_text.contains("process.spawn"), "process.spawn capability missing");
    assert!(toml_text.contains("git"), "git command missing from process.spawn");
    assert!(toml_text.contains("rg"), "rg command missing from process.spawn");
}

#[test]
fn install_all_three_yields_three_lockfile_entries() {
    let tmp = tempfile::tempdir().unwrap();
    install_in_tree_skill(tmp.path(), "critic").expect("critic install");
    install_in_tree_skill(tmp.path(), "fact-checker").expect("fact-checker install");
    install_in_tree_skill(tmp.path(), "pr-reviewer").expect("pr-reviewer install");

    let lockfile_path = tmp.path().join(".tau").join("tau-lock.toml");
    let lf = LockFile::load(&lockfile_path).expect("lockfile loads");
    assert_eq!(lf.packages.len(), 3, "expected three packages");

    let names: Vec<&str> = lf.packages.iter().map(|p| p.name.as_str()).collect();
    assert!(names.contains(&"critic"));
    assert!(names.contains(&"fact-checker"));
    assert!(names.contains(&"pr-reviewer"));

    // All three should be tau-format (not synthesized).
    for pkg in &lf.packages {
        assert!(
            pkg.synthesized_from.is_none(),
            "expected synthesized_from=None for in-tree skill {:?}",
            pkg.name.as_str()
        );
    }

    // Schema is v6 (Skills-5).
    use tau_pkg::lockfile::MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION;
    assert_eq!(lf.schema_version, MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION);
}
```

**Implementer notes:**
- The `install_with_options` signature shown above is illustrative; verify the actual signature in `crates/tau-pkg/src/install.rs` before writing. Skills-5 noted it takes `(&Scope, PackageSource, Option<PackageDep>, InstallOptions)` — but this may differ in detail.
- `file://` URLs for local paths work via Skills-5's `tau skill import` path handling, BUT — install pipeline may want a bare git repo, not a working tree, depending on how `source::clone_to_workspace` is implemented. If `file://` to a working tree fails, fall back to creating a bare repo in tempdir + pushing the in-tree skill content to it (the pattern from `install_anthropic_format.rs::make_anthropic_fixture_repo`). The implementer should run a quick test to determine which path works.
- `tau_pkg::lockfile::MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION` is `6` per Skills-5's T2.
- `InstallOptions` is `#[non_exhaustive]`; use the field-mutation pattern (`let mut opts = ...default(); opts.skip_cross_check = true;`).

- [ ] **Step 3: Run tests**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t4 cargo nextest run -p tau-pkg --test install_reference_skills 2>&1 | tail -15
```

Expected: 4 tests pass.

- [ ] **Step 4: Verify clippy + fmt clean**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t4 cargo clippy -p tau-pkg --all-targets -- -D warnings 2>&1 | tail -5
timeout 30 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t4 cargo fmt -p tau-pkg -- --check 2>&1 | tail -5
```

- [ ] **Step 5: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design add crates/tau-pkg/tests/install_reference_skills.rs
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design commit --no-verify -m "$(cat <<'EOF'
test(pkg): install_reference_skills — 4 integration tests for Skills-6

Each test installs an in-tree skill from <workspace>/skills/<name>/
via file:// URL + asserts on the resulting lockfile + install dir.

Tests:
- install_critic_from_in_tree_path
- install_fact_checker_preserves_references_dir
- install_pr_reviewer_records_process_spawn_cap
- install_all_three_yields_three_lockfile_entries

Uses Skills-5's install_anthropic_format.rs pattern. No new
fixtures (reference skills ARE the fixtures).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: `tau-cli/tests/reference_skills_e2e.rs` — 5 CLI integration tests

**Files:**
- Create: `crates/tau-cli/tests/reference_skills_e2e.rs`
- Modify (or create): `.gitattributes` to force LF on `skills/**/SKILL.md`

**Subagent:** sonnet.

**Prior-art reference:** `crates/tau-cli/tests/cmd_skill_export.rs` (Skills-5 T6) is the canonical pattern. Uses `assert_cmd::Command::cargo_bin("tau")` + tempdir + lockfile synthesis. For Skills-6, the lockfile-synthesis step is replaced by `tau install ./skills/<name>` since the skills are real packages on disk.

- [ ] **Step 1: Add `.gitattributes` to force LF on SKILL.md files**

The byte-identical roundtrip test will fail on Windows if git checkout converts LF → CRLF on SKILL.md. Force LF.

Append (or create) `.gitattributes` at repo root:

```
# Skills-6: SKILL.md files must round-trip byte-identically through tau skill export.
# Force LF so Windows checkouts don't break the test.
skills/**/SKILL.md text eol=lf
skills/**/*.md text eol=lf
```

If `.gitattributes` already exists, append the two lines without disturbing existing rules. Check first:

```bash
cat /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design/.gitattributes 2>&1 | head -20
```

- [ ] **Step 2: Renormalize line endings**

If `.gitattributes` was just created or new rules added:

```bash
cd /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design
git add --renormalize skills/
```

This is harmless if files were already LF.

- [ ] **Step 3: Write the integration test file**

Create `crates/tau-cli/tests/reference_skills_e2e.rs`:

```rust
//! End-to-end integration tests for Skills-6 reference skills.
//!
//! Drives the `tau` binary via `assert_cmd` against a tempdir scope.
//! Each test installs one or more reference skills from
//! `<workspace>/skills/<name>/` then exercises `tau skill list/show/export`.
//!
//! These tests are the public-facing user-story validation for the
//! Skills track. The proof that a contributor can: clone, build, install
//! reference skills, render them, and export them back to Anthropic.

use assert_cmd::Command;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn workspace_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR set in cargo test runs");
    Path::new(&manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolvable")
        .to_path_buf()
}

fn in_tree_skill_path(name: &str) -> PathBuf {
    workspace_root().join("skills").join(name)
}

/// Set up a tempdir as a tau project scope (`.tau/config.toml` present).
fn setup_scope(scope_root: &Path) {
    std::fs::create_dir_all(scope_root.join(".tau")).unwrap();
    std::fs::write(
        scope_root.join(".tau").join("config.toml"),
        "schema_version = 3\n\n[sandbox]\nrequired_tier = \"none\"\n",
    )
    .unwrap();
}

/// Invoke `tau install <file-url-of-in-tree-skill>` in `scope_root`.
fn run_tau_install(scope_root: &Path, skill: &str) -> std::process::Output {
    let skill_path = in_tree_skill_path(skill);
    let url = format!("file://{}", skill_path.display());
    Command::cargo_bin("tau")
        .unwrap()
        .args(["install", &url])
        .current_dir(scope_root)
        .env("TAU_TESTING_ALLOW_MOCK_SANDBOX", "1")
        .output()
        .unwrap()
}

/// Invoke `tau skill <subcommand>` in `scope_root`.
fn run_tau_skill(scope_root: &Path, args: &[&str]) -> std::process::Output {
    let mut full_args = vec!["skill"];
    full_args.extend_from_slice(args);
    Command::cargo_bin("tau")
        .unwrap()
        .args(&full_args)
        .current_dir(scope_root)
        .env("TAU_TESTING_ALLOW_MOCK_SANDBOX", "1")
        .output()
        .unwrap()
}

// ----------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------

#[test]
fn tau_skill_list_shows_three_installed_references() {
    let tmp = TempDir::new().unwrap();
    setup_scope(tmp.path());

    for skill in ["critic", "fact-checker", "pr-reviewer"] {
        let out = run_tau_install(tmp.path(), skill);
        assert!(
            out.status.success(),
            "{} install failed:\nstdout: {}\nstderr: {}",
            skill,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }

    let out = run_tau_skill(tmp.path(), &["list"]);
    assert!(out.status.success(), "skill list failed");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("critic"), "critic missing from list output");
    assert!(stdout.contains("fact-checker"), "fact-checker missing");
    assert!(stdout.contains("pr-reviewer"), "pr-reviewer missing");
}

#[test]
fn tau_skill_show_critic_renders_anthropic_compatible() {
    let tmp = TempDir::new().unwrap();
    setup_scope(tmp.path());

    let install = run_tau_install(tmp.path(), "critic");
    assert!(install.status.success());

    let out = run_tau_skill(tmp.path(), &["show", "critic", "--json"]);
    assert!(out.status.success(), "skill show failed");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .expect("show --json is valid JSON");

    assert_eq!(parsed["name"], serde_json::Value::String("critic".into()));
    assert_eq!(parsed["version"], serde_json::Value::String("0.1.0".into()));
    // synthesized_from should be null for in-tree (real) tau.toml.
    assert!(parsed["synthesized_from"].is_null(),
        "expected synthesized_from=null for in-tree critic; got {:?}",
        parsed["synthesized_from"]);
}

#[test]
fn tau_skill_export_critic_is_byte_identical() {
    let tmp = TempDir::new().unwrap();
    setup_scope(tmp.path());

    let install = run_tau_install(tmp.path(), "critic");
    assert!(install.status.success());

    let out_dir = tmp.path().join("critic-exported");
    let out = run_tau_skill(
        tmp.path(),
        &["export", "critic", "--output", out_dir.to_str().unwrap()],
    );
    assert!(out.status.success(), "skill export failed:\nstderr: {}",
        String::from_utf8_lossy(&out.stderr));

    // Compare byte-identically (no LF/CRLF mangling — .gitattributes forces LF).
    let in_tree = std::fs::read(in_tree_skill_path("critic").join("SKILL.md")).unwrap();
    let exported = std::fs::read(out_dir.join("SKILL.md")).unwrap();
    assert_eq!(in_tree, exported, "SKILL.md not byte-identical after export");

    // tau.toml must NOT be in the exported directory.
    assert!(
        !out_dir.join("tau.toml").exists(),
        "tau.toml leaked into Anthropic export"
    );
}

#[test]
fn tau_skill_export_fact_checker_drops_capabilities_warns() {
    let tmp = TempDir::new().unwrap();
    setup_scope(tmp.path());

    let install = run_tau_install(tmp.path(), "fact-checker");
    assert!(install.status.success());

    let out_dir = tmp.path().join("fact-checker-exported");
    let out = run_tau_skill(
        tmp.path(),
        &["export", "fact-checker", "--output", out_dir.to_str().unwrap()],
    );
    assert!(out.status.success(), "expected success despite warning");

    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("dropped") || stderr.contains("fs.read"),
        "expected drop warning in stderr; got: {stderr}"
    );
}

#[test]
fn tau_skill_export_fact_checker_preserves_references() {
    let tmp = TempDir::new().unwrap();
    setup_scope(tmp.path());

    let install = run_tau_install(tmp.path(), "fact-checker");
    assert!(install.status.success());

    let out_dir = tmp.path().join("fact-checker-exported");
    let out = run_tau_skill(
        tmp.path(),
        &["export", "fact-checker", "--output", out_dir.to_str().unwrap()],
    );
    assert!(out.status.success());

    // references/ subdir should survive the export.
    assert!(out_dir.join("references").join("style-guide.md").exists(),
        "style-guide.md missing from export");
    assert!(out_dir.join("references").join("common-claims.md").exists(),
        "common-claims.md missing from export");
    // tau.toml stripped.
    assert!(!out_dir.join("tau.toml").exists(), "tau.toml leaked");
}
```

**Implementer notes:**
- `TAU_TESTING_ALLOW_MOCK_SANDBOX=1` env var is the Skills-3/5 test-harness pattern — bypasses real sandbox setup for tests. Verify the exact env var name in `crates/tau-cli/tests/cmd_skill_show.rs::run_skill_show` if unsure.
- `Command::cargo_bin("tau")` builds the tau binary on first test invocation (slow first run, fast after). Tests are independent — no shared state.
- `file://` URLs to local paths route through Skills-5's git-clone path (per the T7-gap fix in PR #102). For an unwrapped working-tree-style local path, this should work. If it doesn't, fall back to invoking the install via a path argument: `tau install <path>` instead of `tau install <file-url>` — read the help output to determine which the CLI accepts.
- The `synthesized_from` JSON field name comes from Skills-5's `tau skill show --json` extension. Verify the exact field name (may be `synthesized_from` or `format` or similar).
- **Windows path normalization:** these tests assert on file existence (`Path::exists()`) and on file CONTENTS (`fs::read`), not on path strings in stdout. No `\` → `/` normalization needed. ✓

- [ ] **Step 4: Run tests**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t5 cargo nextest run -p tau-cli --test reference_skills_e2e 2>&1 | tail -15
```

Expected: 5 tests pass.

- [ ] **Step 5: Verify clippy + fmt clean**

```bash
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t5 cargo clippy -p tau-cli --all-targets -- -D warnings 2>&1 | tail -5
timeout 30 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-t5 cargo fmt -p tau-cli -- --check 2>&1 | tail -5
```

- [ ] **Step 6: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design add crates/tau-cli/tests/reference_skills_e2e.rs .gitattributes
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design commit --no-verify -m "$(cat <<'EOF'
test(cli): reference_skills_e2e — 5 e2e tests for Skills-6 user story

Drives the `tau` binary end-to-end against the in-tree reference
skills. Proves: install + list + show --json + export roundtrip
(byte-identical for capability-less critic) + drop-warning (for
fact-checker with fs.read) + multi-file preservation.

Adds .gitattributes rule forcing LF on skills/**/SKILL.md so the
byte-identical export test passes on Windows checkouts.

5 tests:
- tau_skill_list_shows_three_installed_references
- tau_skill_show_critic_renders_anthropic_compatible
- tau_skill_export_critic_is_byte_identical
- tau_skill_export_fact_checker_drops_capabilities_warns
- tau_skill_export_fact_checker_preserves_references

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: `docs/tutorials/build-your-first-skill.md`

**Files:**
- Create: `docs/tutorials/build-your-first-skill.md`

**Subagent:** sonnet.

**Style requirement:** Diátaxis TUTORIAL form. Narrative voice. Reader learns by doing. Uses `critic` as the running example throughout. Single mdBook chapter (~200 lines).

- [ ] **Step 1: Write the tutorial**

Create `docs/tutorials/build-your-first-skill.md`:

```markdown
# Build your first skill

By the end of this tutorial you will have written, installed, and
invoked a skill named `praise-poet` that responds to drafts with
upbeat affirmations. The point isn't poetry — the point is to see
each piece of tau's skill system work end-to-end on a single example
you wrote yourself.

You'll need: a working tau build (`cargo build --release` inside the
tau repo) and a text editor.

## What a skill is

A tau skill is a directory with two files:

- **`SKILL.md`** — the system prompt for an agent. Markdown body
  prefixed by YAML frontmatter (the same format Anthropic's Agent
  Skills uses).
- **`tau.toml`** — the package manifest. Names the skill, gives it a
  version, declares any capabilities it needs.

Anything else in the directory (reference files, examples, assets)
is part of the skill's payload: it travels with the skill on install
and is accessible at runtime.

We'll build all three files in `~/skills/praise-poet/`.

## Step 1 — Write SKILL.md

In a fresh directory, save the following as `SKILL.md`:

    ---
    name: praise-poet
    description: Responds to drafts with upbeat affirmations.
    ---

    You are an enthusiastic editor. When the user shares a draft,
    respond with three affirmations — one per paragraph. Each
    affirmation should quote a real phrase from the draft and call
    out what works about it.

    Keep it brief and specific. No filler superlatives ("amazing",
    "fantastic"). Quote the draft to anchor your praise.

The frontmatter is YAML. Both `name` and `description` are required.
Everything after the closing `---` is the prompt body — it becomes
the spawned agent's system prompt when this skill is invoked.

You could stop here and you'd have a valid Anthropic Agent Skill.
tau will pick it up too, but we'll add the package manifest next so
you can install it.

## Step 2 — Add the manifest

Save the following as `tau.toml` in the same directory:

    name = "praise-poet"
    version = "0.1.0"
    description = "Responds to drafts with upbeat affirmations."
    authors = ["you"]
    source = "local://praise-poet"
    kind = "skill"
    dependencies = []
    capabilities = []

    [skill]

The `name` here MUST match the `name` in your SKILL.md frontmatter.
tau enforces this on install (it's a guardrail against subtle
name-drift between the two files).

`capabilities = []` is correct for now — `praise-poet` doesn't need
to read files or run shell commands; it only needs the LLM.

## Step 3 — Install it

From your tau checkout:

    $ ./target/release/tau install ~/skills/praise-poet
    > Installed praise-poet@0.1.0

Verify with `tau skill list`:

    $ ./target/release/tau skill list
    Name           Version  Source
    ─────────────────────────────────────────
    praise-poet    0.1.0    local://praise-poet

And inspect the body:

    $ ./target/release/tau skill show praise-poet --body --raw

You should see your SKILL.md prompt printed back. `--raw` skips
markdown rendering; drop it to see the styled output.

## Step 4 — Invoke from an agent

This step depends on the agent surrounding your skill. The pattern
in tau is:

    [agents.reviewer]
    package = "code-reviewer@^0.1"
    llm_backend = "anthropic"

    [[agents.reviewer.capabilities]]
    kind = "skill.spawn"
    allowed_skills = ["praise-poet"]

Now the `reviewer` agent can emit `skill.praise-poet.spawn` as a
tool call, and tau will spawn a child agent backed by your skill.

See [How-to: install a skill](../how-to/install-a-skill.md) and
[How-to: author a skill](../how-to/author-a-skill.md) for more on
the capability-declaration patterns. See
[Reference: skill manifest schema](../reference/skill-manifest-schema.md)
for the complete schema.

## Step 5 — Export back to Anthropic

If you want to share your skill with the broader Anthropic
ecosystem (claude-code, for example):

    $ ./target/release/tau skill export praise-poet --output ./out
    > Exported praise-poet to ./out

The `./out/` directory now contains the SKILL.md — no tau.toml,
since the Anthropic format doesn't carry that. You can hand it to
anyone using the Anthropic skill format and they'll be able to use
your prompt.

For skills that declare capabilities, `tau skill export` drops them
with a warning (Anthropic format doesn't preserve capabilities).
You'll see the warning if you export the bundled `fact-checker`
reference skill:

    $ ./target/release/tau skill export fact-checker --output ./out
    note: 1 capabilities dropped on Anthropic export (fs.read);
          Anthropic format does not preserve capability declarations

## What's next

- **Bundle reference files** with your skill — see the
  `fact-checker` reference skill at `skills/fact-checker/` and the
  [how-to on authoring](../how-to/author-a-skill.md).
- **Declare capabilities** — fs.read, process.spawn, etc. The
  [reference page](../reference/skill-manifest-schema.md) covers the
  complete set.
- **Read the design** — [explanation: two-layer skills](../explanation/two-layer-skills.md)
  walks through why tau picked this architecture and what trade-offs
  it locked in.
```

- [ ] **Step 2: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design add docs/tutorials/build-your-first-skill.md
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design commit --no-verify -m "$(cat <<'EOF'
docs(tutorials): build your first skill (Skills-6 T6)

Diátaxis-tutorial: narrative walkthrough using a 'praise-poet' skill
the reader builds from scratch. Covers SKILL.md + tau.toml + install
+ show + invoke from an agent + export to Anthropic.

Cross-links to the how-to recipes + reference schema + explanation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: `docs/how-to/{install,author,export}-a-skill.md` — 3 recipe pages

**Files:**
- Create: `docs/how-to/install-a-skill.md`
- Create: `docs/how-to/author-a-skill.md`
- Create: `docs/how-to/export-a-skill.md`

**Subagent:** sonnet.

**Style requirement:** Diátaxis HOW-TO form. Problem-oriented. Minimal context. Code-block-heavy. Each recipe answers "how do I X" with the smallest viable example.

- [ ] **Step 1: Write `install-a-skill.md`**

Create `docs/how-to/install-a-skill.md`:

```markdown
# How to install a skill

## From a git URL

    $ tau install https://github.com/owner/some-skill

If the source has a `tau.toml`, tau installs it as a tau-native
skill. If the source has only `SKILL.md` (vanilla Anthropic Agent
Skill), tau auto-detects and synthesizes a `tau.toml` in-memory and
on disk. Lockfile records the provenance.

## From a local path

    $ tau install ./skills/critic

Local paths work like git URLs but skip the clone step. Useful when
developing a skill or shipping one with your project.

## From a `file://` URL

    $ tau install file:///path/to/skill

Same as a git URL but explicit. Useful for testing the git-clone code
path against local paths.

## Customize before installing (Anthropic format only)

    $ tau skill import https://github.com/owner/anthropic-skill \
        --output ./my-skill

Clones the source + writes a synthesized `tau.toml` next to the
SKILL.md. Edit `./my-skill/tau.toml` (e.g. add capabilities), then:

    $ tau install ./my-skill

## Verify the install

    $ tau skill list
    Name      Version  Source
    ──────────────────────────────────────
    critic    0.1.0    https://github.com/...

    $ tau skill show critic
    Name: critic
    Version: 0.1.0
    Description: Reviews drafts for clarity, completeness, and
                 rhetorical quality.
    Capabilities: (none)

## Uninstall

    $ tau uninstall critic
```

- [ ] **Step 2: Write `author-a-skill.md`**

Create `docs/how-to/author-a-skill.md`:

```markdown
# How to author a skill

## Minimal skill (pure prompt)

Directory layout:

    my-skill/
    ├── SKILL.md
    └── tau.toml

`SKILL.md`:

    ---
    name: my-skill
    description: One-line description of what this skill does.
    ---

    [System prompt body — Markdown.]

`tau.toml`:

    name = "my-skill"
    version = "0.1.0"
    description = "One-line description of what this skill does."
    authors = ["you"]
    source = "https://github.com/you/my-skill.git"
    kind = "skill"
    dependencies = []
    capabilities = []

    [skill]

The `name` fields in SKILL.md frontmatter and tau.toml MUST match.

## Adding capabilities

Capabilities give your skill access to things beyond the LLM (files,
network, processes). Declare them in `tau.toml`:

    [[capabilities]]
    kind = "fs.read"
    paths = ["${SKILL_DIR}/references/**"]

    [[capabilities]]
    kind = "net.http"
    hosts = ["api.example.com"]
    methods = ["GET"]

    [[capabilities]]
    kind = "process.spawn"
    commands = ["git", "rg"]

`${SKILL_DIR}` resolves at runtime to the skill's installed path. Use
it for paths that live inside your skill's directory.

For the full set of capability kinds, see
[Reference: skill manifest schema](../reference/skill-manifest-schema.md).

## Bundling reference files

Anything in your skill directory ships with the package on install.
Example:

    fact-checker/
    ├── SKILL.md
    ├── tau.toml
    └── references/
        ├── style-guide.md
        └── common-claims.md

In `tau.toml`, grant fs.read access to the bundled directory:

    [[capabilities]]
    kind = "fs.read"
    paths = ["${SKILL_DIR}/references/**"]

In `SKILL.md`, reference the files by their relative path:

    Use the bundled references at `references/` to validate claims.

The runtime substitutes `${SKILL_DIR}` with the actual install path
when spawning, so the skill agent reads the right files.

## Declaring sub-skill dependencies

If your skill is meant to be invoked alongside another skill, declare
it:

    [skill]

    [[skill.requires_skills]]
    name = "fact-checker"
    version_req = "^0.1"

This is advisory in tau v1 — the runtime doesn't auto-spawn
sub-skills. It documents the relationship for users who want to
install the dependencies together.

## Versioning

tau uses semver in `tau.toml`'s `version` field. Bump it when you
publish a new version:

- **Patch** (0.1.0 → 0.1.1): SKILL.md text fixes, doc updates.
- **Minor** (0.1.0 → 0.2.0): new capabilities, new bundled files.
- **Major** (0.1.0 → 1.0.0): breaking changes to the skill's
  contract (e.g., it now requires capabilities it didn't before).

## Testing your skill

Install your skill into a tempdir scope:

    $ mkdir /tmp/test-scope && cd /tmp/test-scope
    $ mkdir .tau && echo 'schema_version = 3' > .tau/config.toml
    $ tau install /path/to/my-skill
    > Installed my-skill@0.1.0

Then invoke it (depending on how your agent is configured):

    $ tau skill show my-skill --body --raw
```

- [ ] **Step 3: Write `export-a-skill.md`**

Create `docs/how-to/export-a-skill.md`:

```markdown
# How to export a skill

## Why export?

`tau skill export` produces a directory in vanilla Anthropic Agent
Skills format — `SKILL.md` plus any bundled content files, no
`tau.toml`. Useful when:

- Sharing a skill with users who run claude-code or another
  Anthropic-format consumer.
- Submitting to a public skill repository.
- Distributing a skill without exposing tau-specific capability
  declarations.

## Basic export

    $ tau skill export critic --output ./out
    > Exported critic to ./out

The `./out/` directory now contains a vanilla Anthropic skill.

## Capability-bearing skills

If the skill declares capabilities, they're dropped from the export
(Anthropic format doesn't preserve them). You'll see a warning:

    $ tau skill export fact-checker --output ./out
    note: 1 capabilities dropped on Anthropic export (fs.read);
          Anthropic format does not preserve capability declarations

The export still succeeds (exit code 0). The dropped capability is
informational.

## Refuse-on-drop with `--strict`

If you want the export to fail rather than silently drop metadata:

    $ tau skill export fact-checker --output ./out --strict
    error: would drop metadata: ["fs.read"] (skill "fact-checker");
           remove --strict to proceed with a warning

Useful in CI to prevent accidental information loss when an
Anthropic-compatible export is required.

## Overwrite existing output

By default, `--output` refuses to overwrite an existing directory:

    $ tau skill export critic --output ./out
    error: output directory "./out" already exists; pass --force to overwrite

Add `--force` to overwrite:

    $ tau skill export critic --output ./out --force
    > Exported critic to ./out

## Roundtrip guarantee

For capability-less skills (`capabilities = []` + no `requires_skills`),
`tau skill export` produces a byte-identical SKILL.md to the original
source. Multi-file payloads (e.g., `references/` subdirs) are
preserved verbatim.

For capability-bearing skills, the export is one-way: re-importing
won't restore the dropped capabilities. Document any capability
declarations separately if you want round-trippable distribution.
```

- [ ] **Step 4: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design add docs/how-to
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design commit --no-verify -m "$(cat <<'EOF'
docs(how-to): install, author, export a skill (Skills-6 T7)

Three Diátaxis how-to recipes covering the user-facing skill workflows:
- install-a-skill.md: tau install paths (git URL, local, file://,
  customize-before-install via tau skill import)
- author-a-skill.md: minimal skill, capabilities, bundled files,
  requires_skills, versioning, testing
- export-a-skill.md: basic export, --strict, --force, roundtrip
  guarantee for capability-less skills

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: `docs/reference/skill-manifest-schema.md`

**Files:**
- Create: `docs/reference/skill-manifest-schema.md`

**Subagent:** sonnet.

**Style requirement:** Diátaxis REFERENCE form. Completeness > narrative. Dry. Read like a spec. Anyone authoring a skill from scratch should find their answer here without needing the tutorial or how-to pages.

- [ ] **Step 1: Write the reference page**

Create `docs/reference/skill-manifest-schema.md`:

```markdown
# Skill manifest schema

This page is the complete reference for tau skill packages: the
`tau.toml` `[skill]` block, the `SKILL.md` frontmatter requirements,
the capabilities accepted on `kind = "skill"` packages, the
`${SKILL_DIR}` substitution rules, and the lockfile entries.

For background and design rationale, see
[Explanation: two-layer skills](../explanation/two-layer-skills.md)
and ADR-0025 through ADR-0030.

## Package layout

A tau skill package is a directory containing:

| File / path | Required | Purpose |
|---|---|---|
| `tau.toml` | Yes (tau-native) | Package manifest |
| `SKILL.md` | Yes | System prompt (Anthropic format) |
| `<other files>` | No | Bundled payload, accessible at `${SKILL_DIR}/...` |

If the directory has only `SKILL.md` (no `tau.toml`), `tau install`
auto-detects the Anthropic format and synthesizes a `tau.toml` in
memory + on disk. See [How-to: install a skill](../how-to/install-a-skill.md)
for the user surface.

## `tau.toml` top-level fields (`kind = "skill"`)

| Field | Type | Required | Notes |
|---|---|---|---|
| `name` | string | Yes | Must match `SKILL.md` frontmatter `name`. ASCII lowercase, digits, `-`. |
| `version` | string (semver) | Yes | E.g., `"0.1.0"`. |
| `description` | string | Yes | Non-empty. Should match `SKILL.md` frontmatter `description`. |
| `authors` | list of strings | Yes | May be empty. |
| `source` | URL or `local://...` | Yes | Origin. `https://`, `git@`, `file://`, or `local://<name>` for in-tree. |
| `kind` | string | Yes | Must be `"skill"`. |
| `dependencies` | list of `PackageDep` | Yes | May be empty. |
| `capabilities` | list of capability tables | Yes | May be empty. See below. |
| `[skill]` | table | Yes | Skill-specific block (see below). |

## `[skill]` block

| Field | Type | Default | Purpose |
|---|---|---|---|
| `content` | string | `"SKILL.md"` | Path to the SKILL.md content file, relative to the package root. |
| `requires_tools` | list of `PackageDep` | `[]` | Tool dependencies (not yet enforced at runtime; advisory). |
| `requires_skills` | list of `PackageDep` | `[]` | Sub-skill dependencies (not yet enforced at runtime; advisory). |

Example:

    [skill]
    content = "SKILL.md"

    [[skill.requires_skills]]
    name = "fact-checker"
    version_req = "^0.1"

## `SKILL.md` frontmatter

YAML frontmatter delimited by `---` lines. Body is everything after
the closing `---`.

| Field | Required | Notes |
|---|---|---|
| `name` | Yes | Must match `tau.toml` `name`. |
| `description` | Yes | Non-empty. |

Other fields are tolerated and discarded by tau v1.

Example:

    ---
    name: critic
    description: Reviews drafts for clarity, completeness, and rhetorical quality.
    ---

    You are a writing critic. ...

## Capability shapes for skill packages

| `kind` | Fields | Purpose |
|---|---|---|
| `fs.read` | `paths: [...]`, optionally `max_bytes` | Read files matching the path globs. |
| `fs.write` | `paths: [...]`, optionally `max_bytes` | Write files matching the path globs. |
| `fs.exec` | `paths: [...]` | Execute binaries matching the path globs. |
| `net.http` | `hosts: [...]`, `methods: [...]` | HTTP requests to allowed hosts + methods. |
| `process.spawn` | `commands: [...]` | Spawn the named processes (resolved via `PATH`). |
| `agent.spawn` | `allowed_kinds: [...]` | Spawn child agents of the named kinds. |
| `skill.spawn` | `allowed_skills: [...]` | Spawn child agents from installed skills. |
| `task_list` | `mode: "read" \| "write" \| "manage"` | TaskList virtual-tool access. |
| `plan` | `mode: "read" \| "write"` | Plan virtual-tool access. |
| `Custom` | `name: ...`, `params: { ... }` | Plugin-defined capability. |

All capability blocks are TOML array-of-tables:

    [[capabilities]]
    kind = "fs.read"
    paths = ["${SKILL_DIR}/references/**"]

    [[capabilities]]
    kind = "process.spawn"
    commands = ["git", "rg"]

## `${SKILL_DIR}` substitution

The literal string `${SKILL_DIR}` in any `paths` field is substituted
at spawn time with the absolute path to the skill's install
directory (e.g., `<scope>/.tau/packages/<name>/<version>/`).

Substitution applies to:

- `fs.read paths`
- `fs.write paths`
- `fs.exec paths`

It does NOT apply to:

- `net.http hosts`
- `process.spawn commands` (which are resolved via PATH, not absolute paths)
- `Custom params` (plugin-defined; opt in if the plugin supports it)

## Lockfile entries

After installation, the package appears in the project's
`tau-lock.toml` as a `[[package]]` entry with a `[package.skill]` block.

    [[package]]
    name = "critic"
    active_version = "0.1.0"
    source = "https://github.com/..."

    [package.skill]
    content_sha256 = "<64-char hex>"

    [package.skill.frontmatter]
    name = "critic"
    description = "Reviews drafts ..."

    [[package.versions]]
    version = "0.1.0"
    resolved_commit = "..."
    sha256 = "..."
    installed_at = "..."

For skills synthesized from Anthropic-format sources, the
`synthesized_from` field appears at the package level:

    [[package]]
    name = "imported-skill"
    active_version = "0.1.0"
    source = "https://github.com/anthropic-author/imported-skill.git"
    synthesized_from = "anthropic"

    ...

`synthesized_from` is `Some("anthropic")` when `tau install`
auto-detected Anthropic format (no tau.toml in source). It is `None`
for tau-native packages (tau.toml present in source).

## Lockfile schema versioning

| Version | Introduced | Skills change |
|---|---|---|
| v4 | Pre-Skills | (no skill data) |
| v5 | Skills-2 | `[package.skill]` block (content_sha256 + frontmatter snapshot) |
| v6 | Skills-5 | `synthesized_from: Option<SynthesizedSource>` provenance |

The current schema version is **v6** (Skills-5).
`MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION` in `crates/tau-pkg/src/lockfile.rs`.

## Cross-references

- ADR-0025: foundation + two-layer architecture
- ADR-0026: install pipeline + lockfile v5
- ADR-0027: discovery (`tau skill list/show`)
- ADR-0028: runtime invocation (`skill.<name>.spawn`)
- ADR-0029: Anthropic interop + lockfile v6
- ADR-0030: reference packages + docs (this sub-project)
```

- [ ] **Step 2: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design add docs/reference/skill-manifest-schema.md
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design commit --no-verify -m "$(cat <<'EOF'
docs(reference): skill manifest schema (Skills-6 T8)

Diátaxis-reference: complete schema for tau skill packages.
- Package layout
- tau.toml top-level fields (kind=skill)
- [skill] block (content, requires_tools, requires_skills)
- SKILL.md frontmatter requirements
- All 10 capability shapes
- ${SKILL_DIR} substitution rules
- Lockfile entry shapes (v6)
- Schema version history (v4→v5→v6)
- Cross-references to ADR-0025 through ADR-0030

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: `docs/explanation/two-layer-skills.md` + `docs/SUMMARY.md` index updates

**Files:**
- Create: `docs/explanation/two-layer-skills.md`
- Modify: `docs/SUMMARY.md`

**Subagent:** sonnet.

**Style requirement:** Diátaxis EXPLANATION form. Narrative + reasoning. Anchors design choices.

- [ ] **Step 1: Write the explanation page**

Create `docs/explanation/two-layer-skills.md`:

```markdown
# Why two-layer skills

A tau skill is two files in a directory: `SKILL.md` (the prompt)
and `tau.toml` (the manifest). This isn't an obvious choice — most
agent frameworks embed both in a single file. This page explains
why tau picked the two-layer split, what trade-offs it locked in,
and how it interacts with the broader Anthropic Agent Skills
ecosystem.

For the surface details, see
[Reference: skill manifest schema](../reference/skill-manifest-schema.md).
For the design history, see ADR-0025.

## The reframing that produced Option D

When Skills-1 was designed, four shapes were on the table:

- **A:** Typed `[skill]` block in `tau.toml`, with the system prompt
  embedded as a TOML triple-quoted string.
- **B:** External `SKILL.md` referenced by relative path from
  `tau.toml`, with a tau-specific filename convention.
- **C:** Adopt the Agent Skills spec format verbatim (just SKILL.md,
  no tau.toml — drop tau-specific packaging).
- **D:** Two-layer: Anthropic-compatible SKILL.md (frontmatter + body)
  PLUS tau.toml for packaging metadata + capabilities.

Option D won. The reframing: **Anthropic's skill format already
defines the prompt layout we want. We don't need to compete with it
on prompt encoding; we need to extend it with packaging.**

The result is a skill directory that's simultaneously:

- A valid Anthropic Agent Skill (just remove `tau.toml` to ship to
  claude-code or another Anthropic consumer).
- A tau package with capabilities, semver, dependencies, lockfile
  participation.

## What each layer owns

**SKILL.md owns: the prompt content.**

- The YAML frontmatter declares `name` and `description` (both
  required, both validated).
- The Markdown body is the system prompt. It becomes the spawned
  child agent's `system_prompt` at runtime (per ADR-0028).
- Format is fixed by the Anthropic Agent Skills spec; tau doesn't
  extend it.

**tau.toml owns: everything else.**

- Package identity: `name`, `version`, `source`, `authors`.
- Capability declarations (`[[capabilities]]` blocks).
- Sub-skill dependencies (`[skill.requires_skills]`).
- Package kind (`kind = "skill"` — disambiguates from `kind = "tool"`
  / `kind = "llm-backend"`).

The two layers must agree on `name` (validated on install).
Everything else is independent: tau can add `[skill.requires_tools]`,
`requires_skills`, future fields, without touching SKILL.md.

## The roundtrip claim

For any tau skill with `capabilities = []` and no `requires_skills`:

    tau install <source> → tau skill export <name> → re-install

produces an identical SKILL.md byte-for-byte (and the same `name` +
`description`). Skills-5 ships this as `tests/skill_format_roundtrip.rs`.

For capability-bearing skills, the export is one-way (Anthropic
format doesn't preserve capabilities). `tau skill export --strict`
makes the metadata-drop a hard error if round-trippability matters.

The roundtrip claim is what makes the two-layer architecture
worth it: tau can extend the Anthropic format without forking it.

## What this rules out

Two consequences worth being explicit about:

**1. We can't bake tau-specific behavior into SKILL.md.** No tau-
specific YAML frontmatter extensions (e.g., `x-tau-capabilities`),
no tau-flavored Markdown syntax. Why: any such extension would break
the byte-identical roundtrip claim for pure-prompt skills, and the
Anthropic ecosystem ignores unknown YAML keys anyway so there's no
benefit.

Skills-5 explicitly rejected an `x-tau-capabilities` YAML extension
for this reason.

**2. We can't combine tau.toml and SKILL.md into a single file.** A
skill is fundamentally a directory of files. The `${SKILL_DIR}/...`
substitution + multi-file payloads (e.g., `references/` subdirs)
depend on the directory being the unit of distribution. Embedding
everything in tau.toml would lose that.

## Comparison with neighboring systems

| System | Prompt encoding | Packaging |
|---|---|---|
| Anthropic Agent Skills (vanilla) | SKILL.md (YAML + Markdown) | none (just the directory) |
| tau | SKILL.md (same) | tau.toml (additive) |
| Single-file agents (e.g., `.prompt` files) | one TOML / YAML / JSON file | none |
| Plugin-style (Python decorators, MCP servers) | code-embedded | language-native imports |

tau differs from each:

- vs Anthropic vanilla: gains semver, capabilities, lockfile.
- vs single-file: gains multi-file payloads, structured packaging.
- vs plugin-style: gains LLM-readable prompts (skills aren't code).

## Sub-skill composition (currently advisory)

The `[skill.requires_skills]` block lets a skill declare that it's
meant to be used with another skill:

    [[skill.requires_skills]]
    name = "fact-checker"
    version_req = "^0.1"

In tau v1, this is **advisory**. The runtime doesn't:

- Auto-install dependencies on `tau install`.
- Auto-spawn sub-skills when the parent is invoked.
- Enforce that the dependency is present at spawn time.

It documents the relationship for users authoring agents (so they
can install the dependencies + grant the parent skill `skill.spawn`
authorization for them).

A future Skills sub-project may tighten this if a concrete use case
emerges.

## When NOT to write a skill

A skill is the right shape when:

- The work is purely prompt-driven (LLM reads input → emits output).
- The skill has clear boundaries (one purpose, one entry point).
- The capability set is small (one or two fs / process / net access).

A skill is the WRONG shape when:

- The work requires real code (compute, parsing, custom logic). Use
  a tool plugin instead (`kind = "tool"`).
- The work requires many capabilities or long chains of tool calls.
  Compose smaller skills + tools instead.
- The work doesn't generalize beyond a single project. Just inline
  the prompt in your agent definition.

The skill abstraction earns its keep when it's installable, named,
versioned, and reusable across agents.

## Further reading

- ADR-0025: foundation
- ADR-0028: runtime invocation
- ADR-0029: Anthropic interop
- Tutorial: [Build your first skill](../tutorials/build-your-first-skill.md)
- Reference: [Skill manifest schema](../reference/skill-manifest-schema.md)
```

- [ ] **Step 2: Update `docs/SUMMARY.md`**

Read the current SUMMARY.md to find the right insertion points:

```bash
grep -nE "^# (Tutorials|How-to|Reference|Explanation)" /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design/docs/SUMMARY.md
```

Then edit `docs/SUMMARY.md` to insert under each section. The shape:

```markdown
# Tutorials

- [Overview](tutorials/README.md)
- [Build your first skill](tutorials/build-your-first-skill.md)

# How-to

- [Overview](how-to/README.md)
- [Install a skill](how-to/install-a-skill.md)
- [Author a skill](how-to/author-a-skill.md)
- [Export a skill](how-to/export-a-skill.md)

# Reference

- [Overview](reference/README.md)
- [Sandbox platform support](reference/sandbox-platform-support.md)
- [Skill manifest schema](reference/skill-manifest-schema.md)

# Explanation

- [Overview](explanation/README.md)
- [Escape hatches](explanation/escape-hatches.md)
- [tau as language](explanation/tau-as-language.md)
- [Two-layer skills](explanation/two-layer-skills.md)
```

Use the `Edit` tool to insert each new link under its respective section header. Preserve all existing entries.

- [ ] **Step 3: Verify mdBook builds locally (optional)**

If `mdbook` is installed:

```bash
cd /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design/docs
mdbook build 2>&1 | tail -5
```

Expected: no broken-link errors. If not installed, CI's mdBook job will catch issues.

- [ ] **Step 4: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design add docs/explanation/two-layer-skills.md docs/SUMMARY.md
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design commit --no-verify -m "$(cat <<'EOF'
docs(explanation): two-layer skills + SUMMARY index (Skills-6 T9)

Diátaxis-explanation: design reasoning for the SKILL.md + tau.toml
split. Covers the Option D reframing, what each layer owns, the
roundtrip claim, what the architecture rules out, comparison with
neighboring systems, sub-skill composition semantics, and when NOT
to write a skill.

SUMMARY.md indexes the 6 new pages under their Diátaxis sections.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: `skills/README.md` index

**Files:**
- Create: `skills/README.md`

**Subagent:** haiku.

- [ ] **Step 1: Write the index**

Create `skills/README.md`:

```markdown
# Reference skill packages

This directory ships exemplary tau skill packages with the tau
source tree. Each one demonstrates a different capability axis so
the value of tau's skill system is concrete:

| Skill | Capability axis | Demonstrates |
|---|---|---|
| [critic](critic/) | none (pure prompt) | Anthropic-format roundtrip; capability-less skills |
| [fact-checker](fact-checker/) | `fs.read` | `${SKILL_DIR}` substitution; multi-file payload (`references/`) |
| [pr-reviewer](pr-reviewer/) | `process.spawn` | Sandbox-compatible process spawning (git + rg) |

## Install

From the tau repo root, after `cargo build --release`:

    ./target/release/tau install ./skills/critic
    ./target/release/tau install ./skills/fact-checker
    ./target/release/tau install ./skills/pr-reviewer

Verify with:

    ./target/release/tau skill list

## Use

Once installed, an agent can spawn a skill as a child agent if it
has the `skill.spawn` capability granting the skill's name:

    [[agents.reviewer.capabilities]]
    kind = "skill.spawn"
    allowed_skills = ["critic", "pr-reviewer"]

The agent then emits `skill.critic.spawn` or `skill.pr-reviewer.spawn`
as a tool call and tau spawns a child agent with the skill's
declared prompt + capabilities.

## Documentation

- [Tutorial: build your first skill](../docs/tutorials/build-your-first-skill.md)
- [How-to: install a skill](../docs/how-to/install-a-skill.md)
- [How-to: author a skill](../docs/how-to/author-a-skill.md)
- [How-to: export a skill](../docs/how-to/export-a-skill.md)
- [Reference: skill manifest schema](../docs/reference/skill-manifest-schema.md)
- [Explanation: two-layer skills](../docs/explanation/two-layer-skills.md)
```

- [ ] **Step 2: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design add skills/README.md
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design commit --no-verify -m "docs(skills): index reference packages (Skills-6 T10)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: ADR-0030

**Files:**
- Create: `docs/decisions/0030-skills-reference-packages.md`

**Subagent:** haiku.

- [ ] **Step 1: Check ADR number availability**

```bash
ls /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design/docs/decisions/ | grep -E "^003[01]"
```

If 0030 is taken (parallel Claude sessions land first), use the next free number and adjust file name + content accordingly.

- [ ] **Step 2: Write the ADR**

Create `docs/decisions/0030-skills-reference-packages.md`:

```markdown
# ADR-0030 — Skills reference packages + user docs (Skills-6)

**Status:** Accepted 2026-05-16.
**Branch / PR:** `feat/skills-6-reference-packages` (PR #115).
**Spec:** `docs/superpowers/specs/2026-05-16-skills-6-reference-packages-design.md`.
**Plan:** `docs/superpowers/plans/2026-05-16-skills-6-reference-packages.md`.
**Depends on:** ADR-0025 (Skills-1), ADR-0026 (Skills-2), ADR-0027
(Skills-3), ADR-0028 (Skills-4), ADR-0029 (Skills-5).

## Context

Final sub-project of ROADMAP §16. Skills-1 through Skills-5 built
the infrastructure; Skills-6 ships the **content** that turns the
infrastructure into a complete, usable product:

- 3 exemplary skill packages under `skills/` (critic, fact-checker,
  pr-reviewer) covering pure-prompt / fs.read / process.spawn axes.
- 6 mdBook documentation pages filling all four Diátaxis quadrants.
- 9 end-to-end integration tests proving the user story works in CI
  across Linux, macOS, Windows.

After Skills-6, a new contributor can clone tau, build it, install
a reference skill, render it, and export it back to Anthropic
format — the entire Skills track makes sense end-to-end without
prior knowledge.

## Decision

Three locked decisions:

### D1 — Three reference skills covering three capability axes

- `critic` (no capabilities) → Anthropic-roundtrip proof
- `fact-checker` (fs.read on `${SKILL_DIR}/references/**`) → substitution + multi-file
- `pr-reviewer` (process.spawn on git + rg) → third axis

Rejected: 1-skill (too minimal) and 2-skill (skips the process.spawn axis).

### D2 — Add-only: no refactor of existing tempdir test fixtures

Skills-1–5 inline-synthesize critic fixtures in 10+ test files.
Refactoring them to reference `skills/critic/` would risk subtle
regressions and merge conflicts with parallel Claude sessions for
marginal benefit. Skills-6 ADDS new integration tests against the
in-tree skills; existing fixtures stay as-is.

### D3 — Full Diátaxis mdBook documentation

mdBook is already wired with the four Diátaxis quadrants
(tutorials / how-to / reference / explanation). Skills-6 fills each
quadrant: tutorial (narrative walkthrough), 3 how-to recipes,
manifest schema reference, two-layer architecture explanation.
Auto-deploys to GitHub Pages via PR #67's existing workflow.

Rejected: in-repo README only (under-invests in user-facing surface);
how-to + reference only (lacks tutorial narrative + explanation
anchoring).

## Alternatives considered

- 1-skill scope (just critic). Rejected: doesn't prove capability story.
- 2-skill scope (critic + fact-checker, no pr-reviewer). Rejected:
  skips the process.spawn capability axis.
- Refactor existing test fixtures. Rejected: 10+ files touched,
  refactor risk, merge-conflict risk.
- Separate `tau-skills` git repo. Rejected: in-tree is fine for
  proof-of-concept; external repo addressable when external
  versioning needs emerge.
- In-repo README only. Rejected: mdBook is already wired + auto-deploys.
- Tutorial-only docs. Rejected: reference + explanation are load-bearing
  for authoring + understanding.
- `tau skill new <name>` scaffolding command. Rejected: useful but
  separate sub-project; not blocking the user story Skills-6 ships.

## Consequences

- **New top-level `skills/` directory** (sibling to `crates/`).
  Future reference skills land here.
- **New `docs/tutorials/build-your-first-skill.md`**, three
  `docs/how-to/` recipes, `docs/reference/skill-manifest-schema.md`,
  `docs/explanation/two-layer-skills.md`. All indexed in
  `docs/SUMMARY.md`.
- **9 new integration tests** across `crates/tau-pkg/tests/` and
  `crates/tau-cli/tests/`. Existing tests untouched.
- **`.gitattributes`** forces LF on `skills/**/SKILL.md` to keep
  the byte-identical export roundtrip test passing on Windows.
- **No new external dependencies. No CI changes.** The new tests
  run under the existing `test-stable` matrix.

## Closes ROADMAP §16

Skills-6 is the final sub-project of the Skills track. ROADMAP §16
is complete after this PR merges. Future Skills work is additive
(more reference packages, `tau skill new`, marketplace, etc.) and
sits outside §16.

## References

- Spec: `docs/superpowers/specs/2026-05-16-skills-6-reference-packages-design.md`
- Plan: `docs/superpowers/plans/2026-05-16-skills-6-reference-packages.md`
- Predecessor ADRs: 0025 (foundation), 0026 (install pipeline),
  0027 (discovery), 0028 (runtime invocation), 0029 (Anthropic interop)
- ROADMAP §16
```

- [ ] **Step 3: Commit**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design add docs/decisions/0030-skills-reference-packages.md
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design commit --no-verify -m "docs(adr): ADR-0030 — Skills reference packages + user docs (Skills-6)

Closes ROADMAP §16.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 12: USER GATE — push + open PR + monitor CI

**Files:** none modified.

**Subagent:** main agent only.

- [ ] **Step 1: Pre-push verification**

```bash
cd /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design
timeout 30 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo fmt --all -- --check
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo clippy -p tau-pkg -p tau-cli --all-targets -- -D warnings
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-pkg --test install_reference_skills
timeout 240 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/main cargo nextest run -p tau-cli --test reference_skills_e2e
```

Fix any fmt / clippy / test failures before push.

- [ ] **Step 2: Rebase onto current main if behind**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design fetch origin --quiet
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design log --oneline HEAD..origin/main | head -5
# If origin/main has commits ahead of HEAD:
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design rebase origin/main
# Resolve any conflicts; tests should still pass after.
```

- [ ] **Step 3: Push**

```bash
git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design push --force-with-lease --no-verify 2>&1 | tail -5
```

- [ ] **Step 4: Update PR title + body**

The existing PR title is `docs(specs): Skills-6 — reference packages + user docs design (ROADMAP §16)`. Update now that the PR includes implementation:

```bash
gh pr edit 115 --title "feat(skills): Skills-6 — reference packages + user docs (ROADMAP §16, closes track)" --body "$(cat <<'EOF'
## Summary

Sixth and FINAL sub-project of ROADMAP §16. Closes the Skills track.

Ships:
- **3 reference skill packages** under `skills/`: `critic` (pure prompt, Anthropic-roundtrip proof), `fact-checker` (fs.read + multi-file refs/), `pr-reviewer` (process.spawn for git + rg).
- **6 mdBook documentation pages** covering all four Diátaxis quadrants: tutorial walkthrough, 3 how-to recipes, manifest schema reference, two-layer architecture explanation. Auto-deploys via PR #67's GitHub Pages workflow.
- **9 end-to-end integration tests** proving the user story in CI across Linux + macOS + Windows.

Spec: `docs/superpowers/specs/2026-05-16-skills-6-reference-packages-design.md`
Plan: `docs/superpowers/plans/2026-05-16-skills-6-reference-packages.md`
ADR: `docs/decisions/0030-skills-reference-packages.md`

## What's in the PR

- **`skills/`** new top-level directory with the 3 reference packages + index README
- **`docs/{tutorials,how-to,reference,explanation}/`** 6 new mdBook pages
- **`crates/tau-pkg/tests/install_reference_skills.rs`** 4 install-pipeline tests
- **`crates/tau-cli/tests/reference_skills_e2e.rs`** 5 CLI end-to-end tests
- **`.gitattributes`** forces LF on `skills/**/SKILL.md` to keep the byte-identical export roundtrip test passing on Windows
- **`docs/SUMMARY.md`** indexes the new pages

## Test coverage

9 new integration tests, add-only (existing tests untouched).

## Closes ROADMAP §16

After this PR merges, all 6 Skills sub-projects are shipped. The Skills track is complete.

## Test plan
- [ ] CI green on all required checks

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 5: Arm auto-merge**

Skills-5 was the first PR to use `--auto`. Follow the same pattern:

```bash
gh pr merge 115 --squash --delete-branch --auto 2>&1 | tail -3
```

This queues the squash-merge for as soon as CI is green + branch is up-to-date with main.

- [ ] **Step 6: Monitor CI**

Use Monitor with a poll loop:

```
prev=""
while true; do
  state=$(gh pr view 115 --json state --jq '.state')
  [ "$state" = "MERGED" ] && echo "MERGED" && break
  [ "$state" = "CLOSED" ] && echo "CLOSED_NO_MERGE" && break
  s=$(gh pr checks 115 --json name,bucket)
  cur=$(jq -r '.[] | select(.bucket!="pending") | "\(.name): \(.bucket)"' <<<"$s" | sort)
  comm -13 <(echo "$prev") <(echo "$cur")
  prev=$cur
  failed=$(jq '[.[] | select(.bucket=="fail")] | length' <<<"$s")
  if [ "$failed" -gt 0 ]; then
    echo "CI_FAILED count=$failed"
    gh pr checks 115 --json name,bucket --jq '.[] | select(.bucket=="fail") | "  \(.name)"'
    break
  fi
  sleep 60
done
```

- [ ] **Step 7: Handle CI flakes**

If macOS jobs fail with `error: unexpected argument 'nextest' found` (dtolnay rust-toolchain transient — hit 4 of last 5 PRs but not #102):

```bash
gh run list --branch feat/skills-6-reference-packages --limit 1 --json databaseId --jq '.[0].databaseId'
# Then after the run completes:
gh run rerun <run-id> --failed
```

If CI doesn't fire at all for >30 minutes (Skills-4 wedge pattern), force-rebase + force-push.

- [ ] **Step 8: On `MERGED` event — cleanup**

```bash
cd /Users/titouanlebocq/code/tau
git -C /Users/titouanlebocq/code/tau fetch origin --quiet
git -C /Users/titouanlebocq/code/tau worktree remove /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design
git -C /Users/titouanlebocq/code/tau branch -D feat/skills-6-reference-packages 2>&1
```

- [ ] **Step 9: Update memory + announce ROADMAP §16 closure**

Write a memory entry to `/Users/titouanlebocq/.claude/projects/-Users-titouanlebocq-code-tau/memory/project_skills_6_shipped_<DATE>.md` documenting the squash SHA + what shipped. Update `MEMORY.md` index.

Announce to user: "Skills-6 merged at `<sha>`. ROADMAP §16 complete. Worktree cleaned up. 6/6 sub-projects shipped: foundation → install pipeline → discovery → runtime invocation → Anthropic interop → reference packages + docs."

---

## Self-review checklist

- **Spec coverage:**
  - D1 (3 skills covering 3 axes) → T1 (critic) + T2 (fact-checker) + T3 (pr-reviewer)
  - D2 (add-only) → all new tests under T4 + T5; no existing tests modified
  - D3 (full Diátaxis) → T6 (tutorial) + T7 (3 how-tos) + T8 (reference) + T9 (explanation + SUMMARY)
  - 9 integration tests → T4 (4) + T5 (5)
  - `skills/README.md` index → T10
  - ADR → T11
  - mdBook auto-deploy → no new wiring needed; uses PR #67's workflow
- **Placeholder scan:** no TBD/TODO. The "verify mdBook builds locally" step in T9 is intentionally optional (depends on local install) — CI is the authoritative gate.
- **Type consistency:** `synthesized_from`, `MAX_SUPPORTED_LOCKFILE_SCHEMA_VERSION`, `${SKILL_DIR}`, `Capability::Skill`, `tau install`, `tau skill {list,show,import,export}` — all match Skills-1–5 names.
- **CLAUDE.md cargo rules:** every cargo invocation includes timeout + `CARGO_INCREMENTAL=0` + `CARGO_TARGET_DIR=target/<role>` + `-p <crate>`.
- **CLAUDE.md push rules:** T12 uses `git push --force-with-lease --no-verify` from inside the worktree via `git -C`.
- **Multi-session safety:** every git operation uses `git -C /Users/titouanlebocq/code/tau-worktrees/feat-skills-6-design ...`. T12 cleanup removes the worktree after merge.
- **Cross-platform gotchas annotated:**
  - Windows line endings → T5 step 1 + 2 (.gitattributes + renormalize)
  - Path string assertions → none used in T4 or T5 tests (file existence + content bytes only)
  - dtolnay macOS transient → T12 step 7 rerun fallback
  - GitHub Actions wedge → T12 step 7 force-rebase fallback
- **No new external dependencies. No CI changes.**
