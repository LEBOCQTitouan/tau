# Skills-4: Runtime Invocation — design

## Context

Fourth of 6 sub-projects decomposed from ROADMAP §16 (Skills as
first-class packages, Constitution G10). Skills-1 (PR #63, `1d71032`)
shipped the manifest types + `parse_skill_md`; Skills-2 (PR #64,
`93dbe95`) wired the install pipeline + `LockedSkill` cache;
Skills-3 (PR #66, `7bec3ab`) added `tau skill list` + `tau skill
show`.

Skills-4 is the runtime piece — what makes installed skills actually
usable by agents. When an agent emits `skill.<name>.spawn`, the
runtime resolves `<name>` to an installed skill, builds a child
agent run from the skill's declared system_prompt + capabilities,
and recursively invokes `Runtime::run_with_history` via the v1.1
agent-spawn machinery shipped in PR #60.

## Goal

A parent agent that has been granted
`Capability::Skill(SkillCapability::Spawn { allowed_skills: ["critic"] })`
can emit:

```json
{
  "tool_name": "skill.critic.spawn",
  "args": { "message": "review draft.md" }
}
```

…and the runtime:

1. Looks up the `critic` skill in the scope lockfile (using Skills-2's
   `LockedSkill.frontmatter` cache + a single `tau.toml` disk read).
2. Reads its `SKILL.md` body via `parse_skill_md` (Skills-1).
3. Builds a child `AgentDefinition` whose `system_prompt` is the
   SKILL.md body (or the caller's `system_prompt` override if
   present), with `${SKILL_DIR}` left symbolic in the prompt.
4. Computes the child's effective grant by taking the skill's
   declared capabilities, substituting `${SKILL_DIR}` to the absolute
   install path, and (optionally) narrowing via the caller's
   `scope_paths` arg.
5. Verifies the resulting grant is ⊆ the parent's grant (existing v1.1
   capability subset law).
6. Spawns the child via the existing v1.1 recursive
   `agent.<kind>.spawn` machinery (`run_with_history` →
   `run_streaming_inner` → child RunOutcome → tool result back to
   parent).

End-to-end, the parent gets back the child's final assistant text as
the `ToolResult` for the spawn call, exactly like the v1.1
`agent.<kind>.spawn` path.

## Decision (locked during brainstorm)

Three architectural decisions locked during the design conversation:

### D1: Separate URI namespace + separate capability variant

Skills get their own top-level virtual-tool namespace, parallel to
`task.*`, `run.*`, and the existing v1.1 `agent.<kind>.spawn`:

```
task.*                  → TaskList operations
run.*                   → Run plan / notes
agent.<kind>.spawn      → Spawn a custom-kind agent (v1.1)
skill.<name>.spawn      → Spawn an installed skill (Skills-4)
```

Paired with a new capability variant:

```rust
// tau-domain — additive
pub enum Capability {
    // ... existing variants ...
    Agent(AgentCapability),                 // unchanged
    Skill(SkillCapability),                 // NEW
}

pub enum SkillCapability {
    Spawn { allowed_skills: Vec<String> },
}
```

TOML form:
```toml
[[capabilities]]
kind = "skill.spawn"
allowed_skills = ["critic", "fact-checker"]
```

Rejected alternatives:
- **Reuse `agent.<kind>.spawn` URI + `Agent::Spawn { allowed_kinds }`**:
  causes namespace collision when a skill named `worker` and a custom
  kind named `worker` both exist; conflates two semantically distinct
  spawn paths.
- **Embedded form `agent.skill.<name>.spawn`**: skills aren't really
  a flavor of "agent"; they're a separate first-class abstraction with
  their own resolution logic. Top-level `skill.*` namespace mirrors
  that.

### D2: Caller `scope_paths` narrows fs.* paths only

Caller's spawn arg is intentionally narrow: only path-based narrowing
of filesystem capabilities. Caller cannot add capabilities, cannot
broaden paths, and cannot modify non-fs capabilities (net.http,
task_list, etc.).

```rust
struct SkillSpawnArgs {
    message: String,                          // required
    system_prompt: Option<String>,            // v1.2 override (PR #61)
    scope_paths: Option<Vec<String>>,         // Skills-4: fs.* narrowing
}
```

When `scope_paths` is `None`: child gets the skill's declared
capabilities verbatim (after `${SKILL_DIR}` substitution).

When `scope_paths` is `Some(paths)`:
- Each `scope_path` MUST be covered by at least one declared fs.* path
  (across all of fs.read / fs.write / fs.exec).
- For each fs.* capability the skill declares: child's effective paths
  = intersection of declared paths with `scope_paths`.
- Empty intersection per kind → drop that kind's capability for the
  child entirely.

Rejected alternatives:
- **Option X (no caller-side knob)**: simplest, but loses real
  per-spawn narrowing use cases (per-invocation data scoping,
  multi-tenant sharing, defense-in-depth on partially-trusted skills).
- **Option A (caller `grant: Vec<Capability>` overrides skill's
  declared)**: too much rope — caller could drop important skill
  grants and break the skill silently. Override/merge precedence
  rules are surface area we don't need.
- **Option B (caller grant merges with skill's declared)**: subset
  check fails on `${SKILL_DIR}` paths the parent doesn't grant
  (carve-out needed); narrowing impossible without an explicit
  "deny" mechanism.

### D3: Full multi-turn `MockLlmBackend` test fixture

Skills-4 ships the multi-turn `MockLlmBackend` fixture that we've
deferred since PR #59. This fixture is necessary for end-to-end
testing of skill resolution + spawn + child run + result propagation.
**As a bonus, it unblocks the 5 `#[ignore]`'d pattern test skeletons
in `crates/tau-cli/tests/cmd_orchestration.rs`** (they were waiting
on the same fixture; un-ignoring them is a Skills-4 follow-up).

Cost: ~2-3 days of fixture wiring on top of the ~2-3 day core impl.
Total: 5-6 days. Worth it because the fixture is foundational for
all future multi-agent + skill testing.

Rejected alternative:
- **Option B (unit-only tests + defer e2e to a follow-up PR)**:
  faster ship (~2-3 days) but leaves Skills-4 with weaker coverage
  than Skills-1/2/3, and the fixture has to be built eventually
  anyway. Coupling the design to a real e2e scenario lets us iterate
  on the fixture API alongside the feature it supports.

## Architecture

### Resolution flow

```
parent agent emits virtual tool call:
  skill.critic.spawn { message: "...", scope_paths: ["/workspace/A/**"] }
       ↓
[stream::run_streaming_inner: virtual-tool intercept]
       ↓
[is_virtual("skill.critic.spawn") → true]
       ↓
[required_capability("skill.critic.spawn") → Capability::Skill(SkillCapability::Spawn { allowed_skills: vec![] })]
       ↓
[capability check: parent's grant must contain SkillCapability::Spawn
 with "critic" ∈ allowed_skills]
       ↓
[validate_skill_spawn(tool_name, args, parent, parent_grant) → SkillSpawnRequest]
       │
       ├── parse "critic" from tool name
       ├── parse args (message, optional system_prompt, optional scope_paths)
       ├── tau-pkg::find_installed_skill(scope, "critic") → Option<InstalledSkill>
       │   (one disk read: install_path/tau.toml + Skills-2's cached frontmatter from lockfile)
       ├── if not found → SkillNotInstalled
       ├── read SKILL.md body (only if caller didn't override system_prompt)
       ├── substitute ${SKILL_DIR} → install_path in skill's declared capabilities
       ├── apply scope_paths intersection (D2)
       ├── verify resulting grant ⊆ parent_grant (subset law)
       └── return SkillSpawnRequest { kind, grant, message, system_prompt }
       ↓
[v1.1 recursive spawn path — unchanged]
[Box::pin(child_runtime.run_with_history(child_def, manifest, [], child_msg, child_opts)).await]
       ↓
[child Run completes → final_message]
       ↓
[parent gets ToolResult { content: [Text(final_text)], is_error: false }]
```

The "v1.1 recursive spawn path — unchanged" is the load-bearing
claim: Skills-4 reuses the exact same `run_with_history` recursion
machinery that PR #60 shipped, with the only difference being that
the spawn args + child agent_def come from the skill's manifest
instead of being supplied inline by the caller.

### Components

#### tau-domain (additive)

**New `SkillCapability` enum + `Capability::Skill` variant:**

```rust
// crates/tau-domain/src/package/capability.rs
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SkillCapability {
    Spawn { allowed_skills: Vec<String> },
}

pub enum Capability {
    // ... existing variants ...
    Skill(SkillCapability),
}
```

TOML serialization round-trip (analogous to existing
`agent.spawn` form):
```toml
[[capabilities]]
kind = "skill.spawn"
allowed_skills = ["critic", "fact-checker"]
```

`Capability::required_shape()` returns `CapabilityShape::SkillSpawn`
(new variant in `CapabilityShape`, analogous to `AgentSpawn`).

#### tau-pkg (new helper)

```rust
// crates/tau-pkg/src/lib.rs or new module
pub struct InstalledSkill {
    pub name: PackageName,
    pub version: Version,
    pub install_path: PathBuf,
    pub manifest: SkillManifest,            // re-exported from tau-domain
    pub frontmatter: SkillFrontmatterSnapshot,
    pub capabilities: Vec<Capability>,      // skill's declared capabilities
}

/// Resolve an installed skill by name. Loads the scope lockfile,
/// finds the LockedPackage with skill metadata matching `name`, then
/// reads + parses the package's tau.toml at install_path.
///
/// Returns None if no installed skill matches.
pub fn find_installed_skill(
    scope: &Scope,
    name: &str,
) -> Result<Option<InstalledSkill>, FindSkillError>;
```

`find_installed_skill` does one disk read per invocation
(`<install_path>/tau.toml`). The frontmatter + content_sha256 come
from the lockfile (Skills-2's cache). No `SKILL.md` read — that's
done at the runtime layer only when needed.

#### tau-runtime (most of the work)

**New module `crates/tau-runtime/src/orchestration/skill_resolve.rs`:**

Three core functions:

```rust
/// Substitute `${SKILL_DIR}` in any path-bearing capability's `paths`
/// vec with `install_path.display()`. Non-path capabilities pass
/// through unchanged. Pure function.
pub fn substitute_skill_dir(
    caps: &[Capability],
    install_path: &Path,
) -> Vec<Capability>;

/// Apply caller's `scope_paths` to a (post-substitution) capability
/// list. Each scope_path must be covered by at least one declared
/// fs.* path (typo detection — hard fail). For each fs.* capability,
/// intersect its declared paths with scope_paths; drop the
/// capability if the intersection is empty. Non-fs capabilities
/// pass through unchanged.
pub fn apply_scope_paths(
    caps: Vec<Capability>,
    scope_paths: &[String],
) -> Result<Vec<Capability>, OrchestrationError>;

/// End-to-end skill resolution. Looks up the skill, computes its
/// effective grant under the caller's args, verifies the subset law
/// against the parent's grant. Returns a fully-validated
/// SkillSpawnRequest ready for the existing v1.1 spawn machinery.
pub fn resolve_skill_for_spawn(
    skill_name: &str,
    args: &SkillSpawnArgs,
    parent_grant: &[Capability],
    scope: &Scope,
) -> Result<SkillSpawnRequest, OrchestrationError>;

pub struct SkillSpawnRequest {
    pub skill_name: String,
    pub install_path: PathBuf,
    pub system_prompt: String,
    pub grant: Vec<Capability>,
    pub message: String,
}
```

**Modify `virtual_tools.rs`:**

Three changes:

1. `is_virtual(tool_name)` recognizes `skill.<name>.spawn` in addition
   to `agent.<kind>.spawn`.
2. `required_capability(tool_name)` returns
   `Capability::Skill(SkillCapability::Spawn { allowed_skills: vec![] })`
   for `skill.<name>.spawn` names (the actual `allowed_skills` check
   happens inside `validate_skill_spawn`, parallel to how
   `validate_agent_spawn` works).
3. New `validate_skill_spawn` parallel to `validate_agent_spawn` —
   delegates to `skill_resolve::resolve_skill_for_spawn`.

**Modify `stream.rs` (`run_streaming_inner`'s tool-dispatch arm):**

The existing `is_agent_spawn` branch handles `agent.<kind>.spawn`.
Add a parallel `is_skill_spawn` branch:

```rust
let is_skill_spawn = tool_use.name.starts_with("skill.")
    && tool_use.name.ends_with(".spawn");
if is_skill_spawn {
    // Same shape as is_agent_spawn:
    //   1. validate_skill_spawn → SkillSpawnRequest
    //   2. read SKILL.md body if no caller override
    //   3. build child AgentDefinition (kind = skill_name; same package as parent for v1)
    //   4. emit TraceEventKind::Spawn
    //   5. record_agent_spawn() for budget counter
    //   6. Box::pin(child_runtime.run_with_history(...)).await
    //   7. extract final text; return as ToolResult
    // ...same recursion path as v1.1's agent.spawn branch
}
```

The existing `is_agent_spawn` branch remains, unchanged. Skills-4 adds
a sibling.

#### tau-cli (no direct changes)

No new CLI surface in Skills-4. `tau run` already routes through
`spawn_root_agent` for multi-agent runs (PR #59); when the agent emits
`skill.<name>.spawn`, the kernel's virtual-tool intercept handles it.

Indirect effect: `tau run` now functionally invokes skills if the
agent does. No new flags or subcommands.

### Error model

New `OrchestrationError` variants:

```rust
// crates/tau-runtime/src/orchestration/error.rs (additive)
#[non_exhaustive]
pub enum OrchestrationError {
    // ... existing variants ...

    /// `skill.<name>.spawn`: no installed skill matches `name`.
    SkillNotInstalled { name: String },

    /// `skill.<name>.spawn`: the lockfile entry exists but the
    /// install_path on disk is missing or unreadable.
    SkillInstallPathMissing {
        name: String,
        expected_path: PathBuf,
    },

    /// `skill.<name>.spawn`: SKILL.md exists but parsing failed.
    SkillContentInvalid {
        name: String,
        detail: String,
    },

    /// `skill.<name>.spawn` with `scope_paths`: caller specified a
    /// path that isn't covered by any of the skill's declared fs.*
    /// paths. Likely a typo or intent mismatch.
    SkillScopePathNotCovered {
        path: String,
    },

    /// `skill.<name>.spawn`: parent's `Capability::Skill(SkillCapability::Spawn)`
    /// doesn't include `name` in `allowed_skills`. (Parallel to
    /// existing `SpawnNotAuthorized` for `agent.<kind>.spawn`.)
    SkillSpawnNotAuthorized { parent: AgentId, name: String },
}
```

All errors surface as `ToolResult { is_error: true, content: [Text(err_msg)] }`
to the parent agent (same shape as the v1.1 spawn-error path). The
parent's LLM can choose to retry, surface to the user, or fail
gracefully.

### Drift detection

If `LockedSkill.content_sha256` doesn't match the on-disk SKILL.md
when the runtime re-reads it, emit a `tracing::warn!` and proceed
using the on-disk content. **Don't fail the spawn** — interactive
skill development is a normal workflow. `tau verify` (Skills-2)
surfaces `SkillContentDrift` for users who want explicit detection.

### Sub-skill composition — advisory only

If a skill's `tau.toml` declares `[[skill.requires_skills]] =
["fact-checker"]` and the skill spawns `skill.fact-checker.spawn`,
the runtime does **not** verify that `fact-checker` is in the
parent skill's `requires_skills`. The only enforcement is the
existing `Capability::Skill(SkillCapability::Spawn { allowed_skills })`
on the parent agent.

`requires_skills` is advisory in v1: it tells the install pipeline
to transitively install dependencies (Skills-2 behavior) and
documents the skill's expected sub-skills for human readers. If
skill authors want hard guarantees, they declare the spawn
capability explicitly.

### Body parse caching

Re-read `SKILL.md` once per spawn — no caching across spawns within
a Run. Reasons:
- A typical Run does 1-3 skill spawns; the read is cheap.
- Caching adds drift complexity (`content_sha256` checked at install,
  but mid-run edits would need a stat to invalidate).
- YAGNI for v1.

If perf becomes an issue (lots of spawns per Run), add a
content-by-install_path memo inside `RunState`. Easy additive
enhancement.

## File structure

| Path | Status | Responsibility |
|---|---|---|
| `crates/tau-domain/src/package/capability.rs` | Modify | Add `Capability::Skill(SkillCapability)` variant + `SkillCapability::Spawn`. Add `CapabilityShape::SkillSpawn`. Update serde de/ser. |
| `crates/tau-pkg/src/lib.rs` (or new `skill_resolve.rs`) | Modify | New `find_installed_skill(scope, name) -> Option<InstalledSkill>` helper. |
| `crates/tau-runtime/src/orchestration/skill_resolve.rs` | Create | Three helpers: `substitute_skill_dir`, `apply_scope_paths`, `resolve_skill_for_spawn`. Pure functions; ~150 LOC + ~10 unit tests. |
| `crates/tau-runtime/src/orchestration/virtual_tools.rs` | Modify | Extend `is_virtual` + `required_capability` for `skill.<name>.spawn`. Add `validate_skill_spawn` parallel to `validate_agent_spawn`. |
| `crates/tau-runtime/src/orchestration/error.rs` | Modify | 5 new variants (SkillNotInstalled, SkillInstallPathMissing, SkillContentInvalid, SkillScopePathNotCovered, SkillSpawnNotAuthorized). |
| `crates/tau-runtime/src/stream.rs` | Modify | Add `is_skill_spawn` branch in the per-tool-dispatch loop, parallel to existing `is_agent_spawn`. |
| `crates/tau-runtime/src/orchestration/mod.rs` | Modify | Re-export `skill_resolve` helpers + `SkillSpawnRequest`. |
| `crates/tau-runtime/tests/common/mock_llm.rs` | Create | Multi-turn `MockLlmBackend` test fixture. Scripted turn-responses + tool-call assertions. ~200 LOC. |
| `crates/tau-runtime/tests/skill_spawn_e2e.rs` | Create | End-to-end skill-spawn tests using MockLlmBackend + a critic fixture skill. ~6 tests. |
| `crates/tau-cli/tests/cmd_orchestration.rs` | Modify | **Un-ignore** the 5 pattern test skeletons that were waiting on MockLlmBackend (this is the "bonus" from D3). |
| `docs/decisions/0028-skills-runtime-invocation.md` | Create | ADR documenting D1/D2/D3 + rejected alternatives. |

## Test coverage

### Unit tests (`skill_resolve` module, in tau-runtime --lib)

~10 unit tests:

- `substitute_skill_dir_replaces_in_fs_read_paths`
- `substitute_skill_dir_replaces_in_fs_write_paths`
- `substitute_skill_dir_passes_through_non_fs_caps`
- `apply_scope_paths_none_returns_declared_verbatim`
- `apply_scope_paths_intersects_fs_read_paths`
- `apply_scope_paths_drops_capability_when_intersection_empty`
- `apply_scope_paths_returns_err_when_scope_path_not_covered`
- `apply_scope_paths_passes_through_non_fs_caps`
- `resolve_skill_for_spawn_returns_skill_not_installed_when_absent`
- `resolve_skill_for_spawn_verifies_subset_law_against_parent`

### Integration tests (tau-runtime --tests)

~6 e2e tests in `tests/skill_spawn_e2e.rs` using the new MockLlmBackend:

- `parent_spawns_skill_and_receives_child_response`
- `parent_overrides_system_prompt_skill_provides_grant`
- `parent_narrows_scope_paths_child_grant_is_intersected`
- `parent_lacks_skill_spawn_capability_spawn_rejected`
- `skill_not_installed_returns_is_error_tool_result`
- `skill_install_path_missing_returns_is_error_tool_result`

### MockLlmBackend tests (fixture standalone)

~3 unit tests in `tests/common/mock_llm.rs`:

- `mock_backend_emits_scripted_turn_in_order`
- `mock_backend_records_received_tool_calls_for_assertions`
- `mock_backend_panics_when_script_exhausted_unexpectedly`

### Pattern test skeletons un-ignored

The 5 `#[ignore]`'d tests in `crates/tau-cli/tests/cmd_orchestration.rs`
that were waiting on MockLlmBackend get un-ignored as a Skills-4
bonus. Each pattern test wires the fixture for its specific shape
(linear, worker-pool, plan-revise, supervisor-critic, hierarchical).
These un-ignore commits land in Skills-4's PR — the patterns now
have real end-to-end coverage.

### Total new tests

~22 new tests (10 unit + 6 e2e + 3 fixture + 5 un-ignored pattern).

## Estimated effort

5-6 days. Components:

- tau-domain capability variant + serde + 2 round-trip tests (~0.5d)
- tau-pkg `find_installed_skill` helper + 2 unit tests (~0.5d)
- `skill_resolve` module + 10 unit tests (~1d)
- `validate_skill_spawn` + virtual_tools wiring (~0.5d)
- 5 new OrchestrationError variants (~0.25d)
- `stream::run_streaming_inner` `is_skill_spawn` branch (~0.5d)
- MockLlmBackend fixture + 3 fixture tests (~1.5d)
- 6 skill_spawn_e2e tests (~0.5d)
- Un-ignore 5 pattern test skeletons + wire each (~0.5d)
- ADR-0028 (~0.25d)

## Out of scope (deferred to Skills-5+)

- **Agent Skills spec export / import** — Skills-5
- **Reference skill packages + user docs** — Skills-6
- **Sub-skill composition enforcement** — `requires_skills` is
  advisory in v1; explicit enforcement is a future tightening if
  needed.
- **Body parse caching across spawns** — performance optimization;
  add when proven necessary.
- **Caller-side capability merge (`grant_extend`)** — explicit
  additive grant alongside `scope_paths`. Add when a use case
  surfaces; D2 chose narrowing-only for v1.
- **Non-fs scope narrowing** (e.g. `scope_hosts` for net.http) —
  same shape as `scope_paths` but for other capability dimensions.
  Add when needed; one-knob-at-a-time.
- **Custom-kind agent spawn** (`agent.<kind>.spawn` for non-skill
  kinds) — exists since v1.1 but has no defined runtime behavior
  for non-skill kinds. Skills-4 doesn't change this. Future work if
  custom-kind spawn becomes meaningful.

## Considered and rejected

### Reuse `agent.<kind>.spawn` URI for skills

Considered: simpler — one URI namespace for all spawns. Rejected:
- Namespace collision: a custom kind named `worker` and an installed
  skill named `worker` can't coexist cleanly.
- Capability conflation: parent's `Agent::Spawn { allowed_kinds:
  ["worker"] }` would grant either a custom-kind spawn or a skill
  spawn, depending on resolution order. Surprising.

### Caller-supplied `grant: Vec<Capability>` override

Considered: gives caller full control over child's grant. Rejected:
- Skill author intent (declared capabilities) becomes optional. Caller
  can drop important grants and break the skill silently with
  capability-denied at runtime.
- Override/merge precedence rules are surface area we don't need.
- The narrowing use case is captured by `scope_paths` (D2) without
  the complexity.

### No caller-side knob at all (Option X)

Considered: simplest possible API. Rejected:
- Loses per-spawn narrowing for legitimate use cases (data scoping,
  multi-tenant, defense-in-depth on untrusted skills).
- Forces skill authors to ship multiple narrower variants of the same
  skill instead of letting callers parametrize.

### Defer MockLlmBackend to a follow-up PR

Considered: ships Skills-4 in 2-3 days instead of 5-6. Rejected:
- Leaves Skills-4 with weaker test coverage than Skills-1/2/3.
- The fixture has to be built eventually for the 5 `#[ignore]`'d
  pattern tests; coupling its API design to Skills-4's real e2e
  scenarios produces a better fixture API than designing it in the
  abstract.

## ADR

ADR-0028 (or next available) documents the three locked decisions +
the rejected alternatives. Pending items for the ADR:
- D1 (separate URI + capability)
- D2 (scope_paths narrowing-only)
- D3 (MockLlmBackend included)
- Drift detection behavior (warn, don't fail)
- Sub-skill `requires_skills` as advisory only
- Custom-kind `agent.<kind>.spawn` path unchanged

## References

- Spec: `docs/superpowers/specs/2026-05-14-skills-4-runtime-invocation-design.md`
  (this doc)
- Skills-1 ADR: `docs/decisions/0025-skills-foundation.md`
- Skills-2 ADR: `docs/decisions/0026-skills-install-pipeline.md`
- Skills-3 ADR: `docs/decisions/0027-skills-discovery.md`
- Multi-agent v1.1 PR: #60 (recursive `agent.<kind>.spawn`)
- Multi-agent v1.2 PR: #61 (per-spawn `system_prompt` override)
- Priority queue: `docs/superpowers/specs/2026-05-12-post-multi-agent-priority-queue.md`
- ROADMAP §16
