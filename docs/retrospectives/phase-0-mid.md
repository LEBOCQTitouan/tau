# Phase 0 mid-phase reflection

**Date:** 2026-04-28
**Status:** Mid-phase memo (not the formal Phase 0 retrospective)

PG4 reserves `docs/retrospectives/phase-NN.md` for phase boundaries.
Phase 0 does not close until sub-project 5 (`tau-cli`) ships, so the
formal retrospective lands then. This memo captures patterns and
foot-guns observed across sub-projects 1-4 while the work is fresh, so
sub-project 5 inherits the lessons rather than rediscovering them.

The four sub-projects covered:

| # | Sub-project | Crate | ADR | Sign-off commit | Merge style |
|---|---|---|---|---|---|
| 1 | Pure data shapes | `tau-domain` | ADR-0002, ADR-0005 | `2a06e18` | linear (rebase / ff) |
| 2 | Plugin trait surface | `tau-ports` | ADR-0003 | `cf29ae8` | linear (rebase / ff) |
| 3 | Package manager | `tau-pkg` | ADR-0004 | `e7578b2` | linear (rebase / ff) |
| 4 | Kernel | `tau-runtime` | ADR-0006 | `a50ed1d` | squash-merge |

## What shipped — patterns that worked

### Brainstorm → spec → plan → implement cycle

Each sub-project produced at least one ADR and a `feat/<topic>` PR
gated by CI. The cycle is predictable and auditable: spec landed first
as its own commit on the feat branch, plan landed second, implementation
tasks landed one commit each, ADR landed penultimate, then a sign-off
commit closed the branch.

Merge strategies varied. Sub-projects 1-3 landed via rebase / fast-forward,
keeping the per-task commit history visible on `main`. Sub-project 4
was the first squash-merge: 21 commits (spec + plan + 17 implementation
+ ADR + sign-off) collapsed into `a50ed1d`. Squash trades per-task
visibility for `main` log readability; the per-task history is still
recoverable via the merged-PR view on GitHub. Sub-project 5 should pick
deliberately rather than by default.

### Same-commit escape-hatch registration

The mechanical CI gate at
`crates/tau-domain/tests/escape_hatch_registry.rs` enforces that every
commit introducing an `Internal` error variant also appends an anchor
to `docs/explanation/escape-hatches.md`. The discipline pays for itself:
in sub-project 3 alone, two anchor registrations (`ScopeError::Internal`,
`RegistryError::Internal`) were initially missed and the gate caught
both before they could ship.

### Outcome / Error dichotomy

Crystallized during sub-project 4's brainstorm. Clean separation:

- `Ok(RunOutcome::Failed)` for agent-level failures (capability
  denial, max turns reached) — the agent ran but couldn't accomplish
  the task.
- `Err(RuntimeError)` for kernel-level errors (plugin errors, dispatch
  errors, contract violations) — the kernel itself broke.

The split makes pattern-matching at the call site clean and gives
callers two different recovery strategies. The pattern should propagate
to sub-project 5's CLI exit-code design.

### Bundled ADRs for tightly-coupled amendments

ADR-0006 covered both the kernel design and the additive
`Tool::capabilities()` amendment to ADR-0003. The bundle is justified
because the trait amendment is solely motivated by the consuming crate
(tau-runtime). Future trait amendments motivated by their own
sub-project should get their own ADRs.

### TOML round-trip for testing `#[non_exhaustive]` types

Sub-project 4's capability proptests and integration tests both leaned
on it: deserialize a TOML fragment to construct test capabilities,
since cross-crate struct-literal construction is blocked. Pattern:

```rust
#[derive(serde::Deserialize)]
struct CapWrapper { cap: Capability }

let wrapped: CapWrapper = toml::from_str(
    r#"cap = { kind = "fs.read", paths = ["/tmp/**"] }"#
).unwrap();
```

Cleaner than mutating defaults, more readable than threading
constructors. Recommend explicitly for tests against `#[non_exhaustive]`
types in sub-project 5.

## What slipped — recurring foot-guns

### `#[non_exhaustive]` cross-crate construction

Sub-project 4 needed 8 mid-implementation constructors landed in Task 10
because the spec assumed struct-literal construction would just work
across crates: `Message::new`, `AgentStatus::failed`, `PackageId::new`,
`CompletionRequest::new`,
`LlmProviderMessage::{user,assistant,tool_result}`, `ToolUse::new`,
`TokenUsage::new`, `SessionContext::new`. Each is a small additive API,
but they were all foreseeable at spec time and missed.

`#[non_exhaustive]` blocks struct-literal construction across crate
boundaries. Pattern recognition: if a spec writes a `Foo { a, b, c }`
literal anywhere outside the crate that defines `Foo`, and `Foo` is
`#[non_exhaustive]`, it won't compile. Spec-phase pre-flight catches it.

### `async fn in trait` is not dyn-compatible

Sub-project 4's spec wrote `Arc<dyn LlmBackend>` and
`Box<dyn Tool<Session = ()>>` directly, but these traits use native
`async fn in trait` (per ADR-0003) which is not dyn-compatible under
Rust 1.93 (E0038). The straightforward translation didn't compile.

Resolution: invented `DynLlmBackend` / `DynTool` / `DynStorage`
wrapper-trait shims with boxed-future signatures and blanket impls for
`T: LlmBackend + 'static` (etc.). ~250 lines of boilerplate. The shim
is documented in ADR-0006 §3 and removable when tau-ports gains a
`trait_variant`-generated dyn-compatible variant.

This was also fully foreseeable at spec time and missed.

### `file://` git clone CI failures

Sub-project 3 hit three distinct issues, each surfacing only after the
CI gate ran:

1. git 2.38+'s `protocol.file.allow=user` default silently blocks
   `file://` clones (CVE-2022-39253 mitigation). Required passing
   `-c protocol.file.allow=always` in `Git::clone`.
2. Test fixture's bare repo `HEAD` had to point to `main` for clone to
   produce a populated working tree.
3. Windows tests failed because path-to-URL conversion produced
   backslashes; required forward-slash normalization.

Each of these would have been catchable with a local CI rehearsal across
the three target OSes (Linux, macOS, Windows). Sub-project 5 should
budget for OS-specific surprises in the path-handling parts of `tau
init` and `tau install`.

### Spec drift between summary table and per-task detail

The plan format mixes a summary table (file → task mapping) with
per-task detail. Drift between the two is hard to spot at review time.

Concrete example: sub-project 4 plan §Task 20 (final verification)
expected `tests/proptest_capability_satisfies.rs` to exist as an
integration test file, but Task 17 (proptest implementation) correctly
chose in-module proptests in `capability.rs` for `pub(crate)` access.
The deviation was correct but had to be rationalized at sign-off rather
than caught in the plan.

Mitigation: plan-writing skill should cross-reference the summary table
against per-task detail, OR the structure-check sub-step should read
"per-task placement" rather than enumerating exact file paths.

## What was learned — guidelines worth crystallizing

### Spec-phase trait pre-flight

Three checks every spec should run before the plan is derived:

1. **Are all types referenced in code snippets `#[non_exhaustive]`?** If
   yes, do public constructors exist for cross-crate use? If not, list
   the constructors as line items in the spec, NOT as silent
   discoveries during implementation.
2. **Are async trait methods involved?** If yes, will the kernel /
   consuming crate need dyn-compatibility? Native `async fn in trait`
   is not dyn-compatible; design accordingly (wrapper traits,
   `trait_variant::make`, or commit to monomorphic generics).
3. **Are tests at integration-test level vs. unit-test level
   constrained by `pub(crate)` visibility?** If yes, the spec should
   say which level for which test, not leave it to the implementer to
   discover at write time.

### Don't ship error variants without a triggering codepath

Sub-project 4 wired `RuntimeError::PluginContractViolation` at task 4
but had no v0.1 trigger (the `deserialize_tool_args` helper is a
passthrough). The Task 15 integration test had to be `#[ignore]`'d.

Pattern: don't add error variants speculatively. Either implement the
trigger OR defer the variant until the use case is concrete (Phase 1
schema validation, in this case).

### Spec language vs. implementation reality

Sub-project 4 spec §6 originally said "~45 events across 9 subsystems".
Actual implementation emitted ~22 events. The number was a brainstorm
estimate that survived into the spec without re-grounding. ADR-0006
records the actual vocabulary; spec-phase numbers should be
estimates-with-error-bars, not commitments.

## Implications for sub-project 5 (tau-cli)

### Exit-code taxonomy mirrors Outcome / Error

```
0 — agent completed successfully (RunOutcome::Completed)
1 — agent failed gracefully (RunOutcome::Failed{PolicyDenied, OutOfResources})
2 — kernel or CLI broke (Err(RuntimeError) or CLI argument errors)
```

Decide in the brainstorm. The dichotomy maps cleanly.

### First end-user binary

Sub-projects 1-4 produced libraries; `tau-cli` produces a binary that
real users invoke. Different concerns:

- stdout vs. stderr discipline (results vs. logs)
- color / verbose / quiet flag conventions
- clap subcommand structure
- config discovery (project root walk-up or `$TAU_HOME`?)
- `tau init` semantics per ADR-0004 §6 (print hint, don't mutate
  `.gitignore`)
- handling of binary mode vs. interactive (TUI deferred to Phase 1?)

### Cross-crate API gaps will surface

Sub-project 5 is the first time tau-pkg and tau-runtime are exercised
end-to-end through a real consumer. Expect at least one missing
constructor, one error variant that's reachable but awkward to
pattern-match, and one place where the public surface needed a
convenience method that wasn't anticipated.

Budget for additive amendments to tau-pkg and tau-runtime, each landing
as part of the same task that surfaces them, with ADR coverage if the
amendment is non-trivial.

### Plan pre-flight checklist

Before deriving the plan from the spec, run the three pre-flight checks
above (`#[non_exhaustive]` constructors, dyn-compat, test visibility)
and enumerate any gaps as commit-by-commit line items in the plan.

## What's NOT in scope for this memo

- The formal Phase 0 retrospective (PG4) — happens after sub-project 5
  ships, and considers Phase 1 priorities, ADR audit, crate
  reorganization, etc.
- Process changes that affect more than the next sub-project — those
  belong in Constitution amendments via the §4 process.
