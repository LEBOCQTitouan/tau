# Phase 0 retrospective

**Phase:** 0 — bootstrap + foundational sub-projects
**Duration:** 2026-04-24 → 2026-04-28 (5 days)
**Status:** Complete
**Author:** Maintainer + Claude Opus 4.7

Per PG4, phases close with a retrospective that records what shipped,
what slipped, what was learned, and what changes to guidelines or plans
follow. Phase 0 ran from the bootstrap commit through the merge of
sub-project 5 (`tau-cli`). This document is the public artifact closing
the phase.

The mid-phase reflection at `docs/retrospectives/phase-0-mid.md` already
captured patterns from sub-projects 1-4. This document supersedes it for
the formal retrospective record while preserving the mid-phase memo as
the in-context capture that it was.

---

## 1. Phase 0 summary

**Original goal:** empty repo with green CI, full governance files, and
the hexagonal workspace skeleton in place; then five foundational
sub-projects (tau-domain, tau-ports, tau-pkg, tau-runtime, tau-cli)
producing working, testable software on its own per the
brainstorm→spec→plan→implementation cycle.

**Outcome:** the goal was met. Every sub-project shipped on time with
ADR coverage, comprehensive tests, and CI gating. The hexagonal
architecture (domain ⟶ ports ⟶ adapters) holds. The constitution and
process guidelines proved load-bearing.

**One unanticipated v0.1 limitation:** plugin loading was deferred to
Phase 1+ in sub-project 5 because the loading mechanism (dlopen,
abi_stable, out-of-process IPC, WASM) was a bigger design decision than
fits inside any single Phase 0 sub-project. v0.1 ships compiled-in mock
plugins gated by `cfg(feature = "test-mock")` for testing; users can
`tau install` source trees but the installed packages don't actually
execute until Phase 1+ ships the loader. ADR-0007 §18 documents this
prominently. This is the most material gap between Phase 0's intent
and what was delivered.

---

## 2. Phase 0 deliverables

### Sub-projects (5)

| # | Sub-project | Crate | Merge | ADRs |
|---|---|---|---|---|
| 1 | Pure data shapes | `tau-domain` | sign-off `2a06e18` (linear) | ADR-0002, ADR-0005 |
| 2 | Plugin trait surface | `tau-ports` | sign-off `cf29ae8` (linear) | ADR-0003 |
| 3 | Package manager | `tau-pkg` | sign-off `e7578b2` (linear) | ADR-0004 |
| 4 | Kernel | `tau-runtime` | squash `a50ed1d` | ADR-0006 |
| 5 | CLI binary | `tau-cli` | squash `b82422e` | ADR-0007 |

Plus the mid-phase reflection memo at `e7e798e` (between sub-projects 4
and 5).

### ADRs (6)

- **ADR-0001** Bootstrap decisions
- **ADR-0002** Manifest format, capability evolution, escape-hatch policy
- **ADR-0003** tau-ports trait surface
- **ADR-0004** tau-pkg package manager — public API, storage layout, lockfile
- **ADR-0005** Custom serde for `PackageSource` and `PackageKind`
- **ADR-0006** tau-runtime kernel + Tool capabilities amendment
- **ADR-0007** tau-cli + tau-runtime amendments (capability filter, run_with_history)

All Accepted. Two ADRs (0006 + 0007) bundle a downstream sub-project's
design with an additive amendment to the upstream crate it consumes —
the "bundled-amendment" pattern works when the amendment is solely
motivated by the downstream consumer.

### Tests (workspace-wide)

- **464 tests** passing across **60 test binaries** (workspace
  `cargo test --all-targets`).
- **21 doctests** passing, **49 ignored** (the `ignore`-marked doctests
  on `#[non_exhaustive]` types per the established E0639 workaround).
- **1 ignored** integration test (`run_kernel_errors::plugin_contract_violation`
  in tau-runtime; deferred to Phase 1's schema validation work).
- **Multiple proptests** in tau-domain (capability parsers, manifest
  validation), tau-runtime (capability satisfies-relation, 256 cases ×
  3 properties), and tau-cli (project tau.toml round-trip, 64 cases).

### CI

- **12 required status checks** on `main`: rustfmt, clippy, 4× test
  matrix (Linux/macOS × stable/1.91), 5× per-crate no-default-features
  builds (tau-domain, tau-ports, tau-pkg, tau-runtime, tau-cli), and
  the tau-ports test-fixtures-only job.
- Windows test jobs run with `continue-on-error: true` per Constitution
  G15 — exercised but not gating.
- Branch protection: `enforce_admins=true`, `strict=true`, no force-push,
  no deletion. The maintainer follows the same gate.

### Workspace structure

The 8-crate skeleton from ADR-0001 is realized:

| Crate | Role | Status |
|---|---|---|
| `tau-domain` | Pure data shapes | Shipped sub-project 1 |
| `tau-ports` | Plugin trait surface | Shipped sub-project 2 |
| `tau-pkg` | Package manager (sync) | Shipped sub-project 3 |
| `tau-runtime` | Kernel (async) | Shipped sub-project 4 |
| `tau-cli` | CLI binary | Shipped sub-project 5 |
| `tau-app` | Reserved for serve-mode binary | Bootstrap stub |
| `tau-infra` | Reserved for shared infra | Bootstrap stub |
| `tau-observe` | Reserved for observability extensions | Bootstrap stub |

Three crates remain bootstrap stubs (`tau-app`, `tau-infra`,
`tau-observe`); their roles are reserved for Phase 1+ work.

---

## 3. What shipped — patterns that worked

### Brainstorm → spec → plan → implementation → ADR cycle

Every sub-project followed the same rhythm: maintainer-driven brainstorm
producing a spec; spec derived into a plan; plan executed via subagent-
driven development (or in-line execution for sub-project 1); ADR filed
penultimately; sign-off commit closing the branch. The cycle is
predictable, auditable, and the per-step artifacts (spec, plan, ADR)
are useful long after the sub-project ships.

Concrete: sub-project 4's branch had 21 commits squash-merged into
`a50ed1d`. Sub-project 5's branch had 19 implementation commits + 4
mid-implementation fixes squash-merged into `b82422e`.

### Same-commit escape-hatch registration

The mechanical CI gate at
`crates/tau-domain/tests/escape_hatch_registry.rs` enforces that every
commit introducing an `Internal` error variant also appends an anchor
to `docs/explanation/escape-hatches.md`. The discipline caught misses:
two anchor registrations in sub-project 3 (`ScopeError::Internal`,
`RegistryError::Internal`) were initially missed and the gate stopped
them.

Sub-project 5 deliberately added zero new `Internal` variants — the
typed `ProjectConfigError` and `AgentResolutionError` taxonomies cover
every codepath. The "don't ship variants without triggering codepaths"
discipline (crystallized in the mid-phase memo) held end-to-end.

### Outcome / Error dichotomy

Crystallized during sub-project 4's brainstorm. Clean separation:

- `Ok(RunOutcome::Failed)` for agent-level failures (capability denial,
  max turns reached) — the agent ran but couldn't accomplish the task.
- `Err(RuntimeError)` for kernel-level errors (plugin errors, dispatch
  errors, contract violations) — the kernel itself broke.

In sub-project 5 the dichotomy mapped cleanly to CLI exit codes (3
buckets: 0 / 1 / 2). CI scripts can `case $?` on this without parsing
JSON — and `--json` output is there when scripts need precision.

The pattern generalizes to any future "run something" verb (workflow
runner, multi-agent orchestrator, etc.). It should propagate.

### Bundled ADRs for tightly-coupled trait amendments

ADR-0006 covered the kernel design + the additive `Tool::capabilities()`
amendment to ADR-0003. ADR-0007 covered tau-cli + two additive
amendments to tau-runtime (capability filter, `run_with_history`).

The bundling works when:
- The amendment is solely motivated by the downstream consumer.
- The downstream sub-project ships the amendment as part of its work.
- The amendment is additive (no breaking changes to existing callers).

If a future amendment doesn't satisfy these — say, a tau-pkg change
motivated by both tau-cli and a hypothetical Phase 1+ workflow runner —
it gets its own ADR.

### TOML round-trip for testing `#[non_exhaustive]` types

Every sub-project after sub-project 1 leaned on this pattern:

```rust
#[derive(serde::Deserialize)]
struct CapWrapper { cap: Capability }

let wrapped: CapWrapper = toml::from_str(
    r#"cap = { kind = "fs.read", paths = ["/tmp/**"] }"#
).unwrap();
```

Cleaner than threading constructors. More readable than mutating
defaults. Should remain the v0.1 idiom for testing `#[non_exhaustive]`
types in cross-crate contexts.

### Hand-authored fixture pattern (lockfile + manifest)

Sub-project 5's tests for `cmd::run` and `cmd::chat` initially planned
to use git fixtures (`file://` URLs cloned via subprocess). The
implementer pivoted to hand-authoring `.tau/tau-lock.toml` +
`.tau/packages/<name>/<version>/tau.toml` directly. Faster (no git
binary invocation per test), more deterministic, no bare-repo HEAD
quirks. The git-fixture pattern remains the right choice for
`tau install` integration tests where the install pipeline IS what's
under test.

### Subagent-driven development for plan execution

Sub-projects 4 and 5 were executed via the `subagent-driven-development`
skill. Per-task implementer subagents with controller review. The
controller (this conversation) caught:
- Spec/reality mismatches (e.g., `tau_pkg::list` returns
  `Vec<LockedPackage>`, not `Vec<InstalledPackage>` as sub-project 5's
  spec hypothesized).
- Mid-implementation deviations that needed bundling (8 tau-domain /
  tau-ports constructors in sub-project 4 Task 10).
- Cross-platform foot-guns (CRLF, hardcoded `/tmp` paths).

The skill's two-stage review (spec compliance + code quality) was not
formally invoked per task — the controller reviewed each implementer's
report directly. This worked for the maintainer-driven workflow but
should be revisited if the team scales beyond one maintainer.

---

## 4. What slipped — recurring foot-guns and surprises

### `#[non_exhaustive]` cross-crate construction

The most repeated pattern across the phase. Specs assumed struct-literal
construction would work; it doesn't across crate boundaries on
`#[non_exhaustive]` types. Concrete instances:

- Sub-project 4 Task 10: 8 mid-implementation constructors landed in
  tau-domain and tau-ports (`Message::new`, `AgentStatus::failed`,
  `PackageId::new`, `CompletionRequest::new`,
  `LlmProviderMessage::{user,assistant,tool_result}`, `ToolUse::new`,
  `TokenUsage::new`, `SessionContext::new`).
- Sub-project 5 Task 9: spec assumed `Scope::resolve_project_only`,
  `InstalledPackage` shape, `PackageSource::Git { url }` — none matched
  reality.

The mid-phase memo crystallized the spec-phase pre-flight checklist.
Sub-project 5 still hit it because the spec was written *before* the
checklist was crystallized. Phase 1 specs should run the checklist
explicitly.

### `async fn in trait` is not dyn-compatible

Sub-project 4's spec wrote `Arc<dyn LlmBackend>` directly. Doesn't
compile. The `DynLlmBackend` / `DynTool` / `DynStorage` wrapper-trait
shim with blanket impls (~250 lines) is the standard idiomatic
workaround. Removable when tau-ports gains a `trait_variant`-generated
dyn variant.

This was the single largest mid-implementation invention of the phase.
It cost about 4 hours of design + implementation. A spec-phase pre-flight
that asked "are async traits involved? are they dyn-compatible?" would
have caught it.

### `file://` git clone CI failures across 3 OSes

Sub-project 3 hit three distinct issues:
1. git 2.38+'s `protocol.file.allow=user` default silently blocking
   `file://` clones (CVE-2022-39253 mitigation).
2. Bare repo `HEAD` not pointing at `main` after `clone --bare`.
3. Windows path-to-URL conversion producing backslashes.

Each surfaced only after CI ran on the relevant OS. The fixes are now
captured (passing `-c protocol.file.allow=always`, explicit
`symbolic-ref HEAD refs/heads/main`, `url::Url::from_file_path` for
forward-slash). Phase 1 specs touching git fixtures should reference
sub-project 3's Task 14 explicitly.

### Spec drift between summary tables and per-task detail

Sub-project 4 plan §Task 20 (final verification) expected
`tests/proptest_capability_satisfies.rs` as a file; sub-project 4 Task
17 correctly chose in-module proptests instead (Option B for
`pub(crate)` access). The deviation was correct but had to be
rationalized at sign-off rather than caught earlier.

Plan-writing skill should cross-reference the summary table against
per-task detail. Tackled mechanically by the writing-plans skill's
self-review step ("type consistency"). Sub-project 5 had fewer such
surprises; the skill is improving.

### Plugin loading deferred to Phase 1+

Not an unanticipated foot-gun so much as a scope decision that should
have been made earlier. Sub-projects 1-4 produced libraries; sub-project
5 was the first that needed real plugin loading to be useful. The
deferral was the right call (plugin loading is a meaningful design
decision deserving its own ADR), but it means Phase 0 ships a CLI that
can install but not actually run anything outside the test-mock.

This is **the** most material gap in Phase 0's deliverables.

### Windows snapshot tests still flaky after Phase 0 closes

Sub-project 5's `help_snapshots.rs` snapshots passed locally on macOS
but failed on Windows due to clap's terminal-width-dependent text
wrapping. CRLF normalization was applied (commit `bd47d77`) but
post-merge investigation suggests the wrapping difference, not just
CRLF, is the root cause. Windows is non-blocking per Constitution G15
so the merge proceeded; Phase 1 should either gate snapshots with
`#[cfg(not(windows))]` or normalize the text more thoroughly.

### Other minor surprises

- Sub-project 4 Task 5 hit `forbid(unsafe_code)` blocking
  `unsafe { env::set_var }` for tests; pivoted to `#[cfg(test)] pub(crate)`
  test-only constructors. Documented in sub-project 4's commit history.
- Sub-project 5 Task 9 added 2 extra `AgentResolutionError` variants
  beyond the spec text (`Registry`, `InvalidIdentifier`) for triggering
  codepaths the spec missed. Still satisfies "no error variants without
  triggering codepaths" — just additive.
- Sub-project 5 Task 13's `--max-turns` integration test had to set
  `max_turns: 1` rather than asserting "loop forever" because the mock
  backend's tool-use behaviour is single-turn-only. Not a defect, just a
  scope reality.
- 4 mid-implementation fixes in sub-project 5 (clippy unused import,
  cross-platform paths, CRLF, rustfmt) added overhead between Tasks
  19 and the final merge. Most were caught by remote CI rather than
  local — local CI was rustfmt-permissive in ways the CI's rustfmt
  wasn't.

---

## 5. What was learned — guidelines worth keeping

The mid-phase memo captured most of these. Sub-project 5 confirmed them
and added a few. Consolidating here:

### Spec-phase trait pre-flight checklist (now codified)

Three checks every spec should run before the plan is derived:

1. **Are all types referenced in code snippets `#[non_exhaustive]`?** If
   yes, do public constructors exist for cross-crate use? If not, list
   the constructors as line items in the spec, NOT as silent discoveries
   during implementation.
2. **Are async trait methods involved?** If yes, will the kernel /
   consuming crate need dyn-compatibility? Native `async fn in trait` is
   not dyn-compatible; design accordingly (wrapper traits,
   `trait_variant::make`, or commit to monomorphic generics).
3. **Are tests at integration-test level vs. unit-test level constrained
   by `pub(crate)` visibility?** If yes, the spec should say which level
   for which test.

These three caught us repeatedly. Phase 1 specs run them explicitly.

### Don't ship error variants without triggering codepaths

Crystallized in the mid-phase memo, validated by sub-project 5 (which
shipped only error variants with concrete failure paths and zero
`#[ignore]`'d "future trigger" tests beyond `plugin_contract_violation`,
which has its own deferral note).

### Spec language vs implementation reality

Sub-project 4 specced "~45 events across 9 subsystems" for tracing
vocabulary; Task 10 actually emitted ~22. Sub-project 5 specced
`tau_pkg::list` returning `Vec<InstalledPackage>` but it returns
`Vec<LockedPackage>`. Specs are estimates with error bars, not
commitments.

ADRs ratify reality, not aspiration. Implementation reports should
explicitly call out where the spec's claim differs from what shipped.
Sub-project 5's commit messages did this consistently.

### Local CI must match remote CI

Sub-project 5's last 4 fixes were all "CI caught it that local didn't":
- Clippy unused-import gated on a feature.
- `cross_platform.rs` Linux paths failing on Windows.
- CRLF in snapshots.
- rustfmt's stable version more aggressive than the dev machine's.

Mitigation for Phase 1: run `cargo fmt --check` and `cargo clippy
--all-features -- -D warnings` against the `stable` toolchain explicitly
before pushing, and where possible exercise the no-default-features
compile path. The `feedback_branch_protection_workflow` memory captures
the principle but the pre-push checklist could be tighter.

### Bundled ADRs for tightly-coupled amendments work

ADR-0006 + ADR-0007 both used the pattern. The discipline:
- The amendment must be solely motivated by the downstream consumer.
- The amendment is additive (no breaking changes).
- The bundle gets a single ADR rather than two.

Don't bundle promiscuously — ADRs are cheap, and unrelated changes
deserve separate decisions.

### Hand-authored fixtures > git fixtures for unit tests

Where the test isn't exercising the install pipeline directly, writing
the lockfile + manifest TOML on disk is faster, more deterministic, and
free of git-binary quirks (HEAD pointing, protocol-allow, path
normalization). Reserve git fixtures for the install path.

### Same-commit escape-hatch registration is non-negotiable

Mechanical CI test enforces it; the discipline caught misses; Phase 0
ended with all anchors registered correctly. Keep it.

---

## 6. Architecture audit — does the design hold?

Phase 0 closes with the hexagonal architecture and 5-crate runtime
surface intact. A short audit before Phase 1 priorities:

### Things to keep as-is

- **Domain ⟶ ports ⟶ adapters direction.** No back-edges. tau-domain
  knows nothing about tau-ports; tau-ports knows nothing about tau-pkg
  or tau-runtime; tau-runtime knows nothing about tau-cli.
- **Per-operation typed errors with `#[from]` composition.** Established
  in sub-project 1, validated in 3, 4, and 5. Top-level umbrella errors
  rejected (ADR-0004 §12, ADR-0006 alternative F).
- **`#[non_exhaustive]` everywhere.** Cross-crate construction is the
  cost; forward-compat additive evolution is the benefit. The pattern
  works. Phase 1 keeps it.
- **Bundled ADRs for tightly-coupled amendments.** Pattern works; keep.
- **3-bucket exit codes mapped to the Outcome/Error dichotomy.** Works
  for tau-cli; should propagate to any future "run something" verb.

### Things to revisit in Phase 1

- **The `DynLlmBackend` / `DynTool` / `DynStorage` shim** in tau-runtime
  is ~250 lines of boilerplate. When tau-ports gains a `trait_variant`-
  generated dyn variant (or when Rust's native `async fn in trait`
  gains dyn-compatibility), the shim can be removed. Phase 1 candidate:
  `trait_variant::make` on the four port traits with an explicit
  `Send`-bounded variant for `tokio::spawn` use cases.
- **Plugin loading mechanism** (the elephant in the room). dlopen vs
  abi_stable vs out-of-process IPC vs WASM is a large design decision
  needing its own ADR. **First Phase 1 sub-project candidate.**
- **`tau-app` / `tau-infra` / `tau-observe` reserved crates.** Their
  roles need definition in Phase 1. `tau-app` is intended for the
  serve-mode binary (was in original Phase 1 preview); `tau-infra` and
  `tau-observe` may absorb plugin-loading + advanced tracing
  respectively, or may consolidate / rename based on actual Phase 1
  needs.
- **REPL persistence.** v0.1's in-memory-only approach is correct for
  v0.1 but Phase 1 likely wants `tau chat --resume <id>`. Schema +
  garbage-collection policy questions.
- **Streaming LLM responses.** `LlmBackend::stream` exists but
  tau-runtime doesn't invoke it. Additive `Runtime::run_streaming` lands
  alongside the first real LLM-backend plugin that benefits from it
  (latency-sensitive UX).

### Things explicitly NOT to revisit (per Constitution NG1-NG12)

The non-goals from Constitution §2 hold. Phase 0 didn't violate any of
them. Phase 1 doesn't either: the priorities below all fit within
"runtime, not framework" (NG12), "developer tool, not end-user product"
(NG11), and the rest of the NG list.

One non-goal is worth re-emphasizing: **NG6 — no persistent agent memory
in core**. REPL persistence (`tau chat --resume`) is CLI-side state, not
agent memory. Workflow runners (Phase 1+) likewise persist *workflow
state*, not agent state. The line between them must stay sharp.

---

## 7. Phase 1 priorities

Concrete ordered list. The original `ROADMAP.md` Phase 1 (preview)
section had four items (serve mode, sandboxing, performance budgets,
cargo-audit/cargo-deny). Phase 1's actual priorities, informed by
Phase 0's deliverables and deferrals, are below. They re-include the
preview's four items at appropriate priority.

### Tier 1 — unblocks Phase 1 itself

These need to ship before most other work makes sense.

1. **Plugin loading mechanism.** Without it, `tau install` is a
   record-keeping operation. Multiple options (dlopen / abi_stable /
   out-of-process IPC / WASM) need an ADR-driven decision. **First
   sub-project of Phase 1.** Probable winner: out-of-process IPC over
   stdio (matches future serve-mode design + sandbox-friendly), but the
   brainstorm decides.
2. **First real LLM-backend plugin.** Likely an Anthropic / OpenAI HTTP
   adapter via reqwest. Validates the loading mechanism end-to-end and
   gives tau actual user value. Probably an out-of-tree package that
   `tau install` pulls in, not bundled with tau-cli.
3. **First real Tool plugin.** `fs-read` + `shell` as initial set —
   validates multi-plugin composition and the capability check at
   runtime. Exercises the tool dispatch loop with non-mock inputs.

### Tier 2 — completes Phase 0 deferrals

4. **Capability override implementation** (project tau.toml
   `[agents.<id>.capabilities]` with intersect-only semantics). Schema
   slot reserved in ADR-0007 §4. Small ADR + small implementation.
5. **Transitive dependency resolution** (`tau install <agent>` pulls
   `requires.tools` automatically). ADR-0004 §10 deferral; complements
   priority 1.
6. **Schema validation for tool args** (activates
   `RuntimeError::PluginContractViolation` and the
   `plugin_contract_violation` `#[ignore]`'d test). JSON Schema validation
   via `jsonschema` crate or hand-rolled.
7. **`tau update` / `tau verify` / `tau uninstall` subcommands.** Spec
   §1 of sub-project 5 deferred these. Each is a smallish addition once
   plugin loading lands.
8. **Streaming LLM responses** (`Runtime::run_streaming` additive). Lands
   alongside the first LLM-backend plugin that benefits from it.

### Tier 3 — extends the runtime

9. **Multi-agent orchestration** (G10's deferred half). Inter-agent
   message routing in tau-runtime, a multi-agent run loop, possibly a
   new CLI verb (`tau orchestrate`). Substantial; likely its own
   sub-project with its own ADR.
10. **Workflow / pipeline runner** (the maintainer's broader vision —
    deterministic step-by-step pipelines). New crate (`tau-workflow`?)
    OR new tau-runtime feature. Big design discussion: LLM-driven skill
    flows vs pure-deterministic runners. Own ADR.
11. **REPL persistence** (`tau chat --resume <id>`, `tau chat
    --list-sessions`). Schema + GC policy. Mid-sized.
12. **Sandboxing implementation** (Constitution G12). The `Sandbox`
    trait is provisional in tau-ports; v0.1 doesn't invoke it. Real
    sandboxing requires OS-level work (jails, namespaces, Seatbelt) +
    its own ADR.

### Tier 4 — operational quality

13. **Performance budgets enforced in CI** (Constitution QG14). G16's
    100ms startup / 50MB memory budgets need automated validation.
    Likely a `criterion` + a CI threshold check.
14. **`cargo audit` + `cargo-deny` in CI** (Constitution QG16). Standard
    Rust supply-chain checks.
15. **Serve mode** (JSON-RPC over stdio — Constitution G6, QG12). The
    second public surface alongside the embeddable Rust API. Probably
    lands in `tau-app`. Coordinates with priority 1's plugin-loading
    mechanism if it picks IPC.
16. **Windows test parity.** Windows is non-blocking per G15 but the
    snapshot tests in sub-project 5 should pass everywhere they claim
    to. Either fix the wrapping issue or scope-reduce the snapshots
    that vary by terminal.

### Out of scope for Phase 1 (per Constitution NG1-NG12)

- Centralized package registry (NG4).
- Persistent agent memory in core (NG6).
- Hosted service (NG3).
- Identity / credentials (NG9).
- Telemetry / training data collection (NG10).

These are forever-deferred. Phase 1 doesn't reopen them.

---

## 8. Process changes for Phase 1

### Updated workflow

- **Spec-phase pre-flight checklist is now mandatory.** Specs touching
  cross-crate types must check `#[non_exhaustive]` constructors, async
  dyn-compatibility, and test-visibility constraints up front.
- **Local pre-push CI parity.** Run `cargo fmt --check` and `cargo
  clippy --workspace --all-targets --all-features -- -D warnings` plus
  every `--no-default-features` build before pushing. The 4 mid-merge
  fixes in sub-project 5 wouldn't have happened with this discipline.
- **Spec implementation reports must call out reality vs spec.** The
  implementer subagent already does this; Phase 1 controllers should
  treat these as routine, not exceptional.

### Constitution amendments

None proposed. The Constitution held up across Phase 0 without edits.
PG4's retro process worked. QG18's ADR discipline worked. The G/NG
partition stayed stable.

If anything, the mid-phase memo + this retrospective demonstrate that
the discipline encoded in the Constitution is load-bearing. Don't
amend what's working.

### Plan execution mode going forward

Subagent-driven development worked well for sub-projects 4 and 5.
Recommend keeping it as the default for plan-driven work. The two-stage
review (spec compliance + code quality) was not formally invoked per
task; controller-driven review served the maintainer-driven workflow.
Revisit if multi-maintainer scaling becomes relevant.

### Memory hygiene

The auto-memory system has captured branch-protection workflow and
plan-driven workflow as project memories. Phase 1 should add (when the
relevant decisions land):
- The plugin-loading mechanism choice.
- Phase 1 sub-project ordering (after the first 1-2 ship and validate
  the priorities above).

---

## 9. Closing

Phase 0 produced a working, testable runtime stack from an empty repo
in 5 days, on schedule, with full ADR coverage, comprehensive tests, and
a clean CI gate. The hexagonal architecture holds. The constitution
proved load-bearing. The brainstorm→spec→plan→implementation cycle
proved repeatable.

The single unanticipated v0.1 limitation (plugin loading deferred) is
the focal point of Phase 1's first sub-project. Everything else flows
from there.

Phase 1 begins with the plugin-loading brainstorm.
