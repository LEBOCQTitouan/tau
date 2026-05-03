# Sandbox activation — declarative requirements + adapter registry + guided setup

**Status:** Design (pre-plan)
**Date:** 2026-05-04
**Audience:** future implementer of sub-project A; reviewer of design choices.
**Supersedes (in scope of activation):** the chain-based design proposed in
[`2026-05-03-sandboxing-followups.md`](2026-05-03-sandboxing-followups.md)
sub-project A. The chain mechanism shipped in priority 12 (ADR-0014) was a v0.1
stopgap; this design replaces it before activation entrenches it in user-facing
config.

## Background

Tier 3 priority 12 (sandboxing infrastructure, [ADR-0014](../../decisions/0014-sandboxing.md))
shipped to `main` at commit `2215cf1` on 2026-05-03. The sandbox port,
two adapters (`tau-sandbox-native`, `tau-sandbox-container`), Layer 3
pre-flight validation, plugin-host integration, lockfile schema v3 → v4,
and `tau resolve --check-sandbox` are all live.

But default activation isn't done: all four plugin spawn call sites in
`crates/tau-runtime/src/plugin_host/mod.rs` pass `None` for the sandbox
argument. Plugins still run unsandboxed by default.

This sub-project activates sandboxing AND course-corrects two architectural
choices from priority 12:

1. **Replace the adapter chain with declarative requirements.** Industry
   tooling (Cargo, Rust toolchains, Bazel, Nix, Kubernetes CRI) uses
   "project declares what it needs; system resolves to a concrete
   implementation per machine." Priority 12 shipped a left-to-right
   probe-and-fall-back chain, which is unusual outside of Bazel's
   per-user `.bazelrc.user`. We move to the industry pattern.

2. **Plugins gain symmetric tier requirements.** Today, plugins declare
   capabilities (which derive shape requirements) but cannot declare a
   minimum tier. A credentials-handling plugin should be able to refuse
   to load if the host can only deliver passthrough. We add
   `[sandbox] required_tier` to plugin manifests.

The Bazel platform/toolchain pattern (declare requirements, register
implementations, resolve at build time) is the architectural inspiration.
The full target-triple registry is Phase 2 sub-project B; this design is
the v0.2 building block on the way there.

## Decisions locked from the brainstorm

These are non-negotiable; the implementation plan derives from them.

### D1 — ON by default with `--no-sandbox` escape hatch

Sandboxing activates by default for any project that has a
`<scope>/config.toml` (which is every tau project — the file exists from
priority 7's lifecycle commands). The escape hatch is a global CLI flag
`--no-sandbox` that's shorthand for `--sandbox passthrough` (see D4),
auditable in shell history, per-invocation only.

**Rationale.** ADR-0014 Decision 6 says no silent under-enforcement.
ON-by-default with explicit opt-out is the only stance consistent with
that. The flag prevents the `alias tau='tau --no-sandbox'` antipattern
while giving developers a clear release valve for one-off debug runs.

### D2 — Adapter lives on `PluginHostOptions.sandbox_adapter`

`tau-cli::cmd::plugin_loader::load_plugins` constructs the adapter
(via `resolve_adapter`, see below) and stores it on
`PluginHostOptions.sandbox_adapter: Option<Arc<SandboxAdapter>>`.
`load_*` functions zip this with a per-plugin `SandboxPlan` and pass to
`spawn_and_handshake`.

**Rationale.** The runtime crate stays sandbox-agnostic about WHERE
adapters come from (CLI does the resolution). Cross-cutting concerns
already live on `PluginHostOptions` (timeouts, recording); the adapter
fits there. The per-plugin variation is the `SandboxPlan` (different
plugins need different shapes), which is genuinely per-call data.

### D3 — Hard refuse when no adapter satisfies; opt-out is explicit at two granularities

When `resolve_adapter` returns no match, exit code 2 with a guided error
message (see Architecture §3). Opt-out is explicit at two layers:

- **Per-invocation:** `tau chat --no-sandbox` (or any subcommand that
  spawns plugins). Resolves to passthrough for this run only.
- **Per-scope (persistent):** `[sandbox] required_tier = "none"` in
  `<scope>/config.toml`. Allows passthrough to satisfy the requirement
  permanently. Persisted in the project's git repo, auditable in
  diff/blame.

No third path. No silent fall-through to passthrough; no auto-disable
on probe failure.

### D4 — `passthrough` adapter (`--no-sandbox` semantics)

Add a new `Passthrough` variant to the adapter registry. It implements
`tau_ports::Sandbox` like any other adapter:

- `probe()` always reports `Available { tier: None, details: "passthrough (no isolation)" }`.
- `supported_shapes()` returns the union of all known shapes (so it
  passes any Layer 3 shape check).
- `validate_plan(_)` always returns `Ok(())`.
- `wrap_spawn(_, _)` is a no-op; returns `SandboxHandle::noop()`.

`--no-sandbox` is shorthand for `--sandbox passthrough`. Persistent opt-out
is `required_tier = "none"` (which makes passthrough — the only
tier-None adapter — satisfy the requirement).

**Rationale.** Reframing "no sandbox" as a sandbox adapter eliminates the
`Option<>` branch at the spawn site, gives uniform observability ("selected
adapter: passthrough" logs alongside every other adapter), and allows
chain-style fallback semantics (if the user wants them) to be expressed as
relaxed requirements rather than as a list ordering. ADR-0014 Decision 6's
"no silent under-enforcement" is preserved: passthrough is never selected
unless `required_tier = "none"` (explicit) or `--no-sandbox` (explicit).

### D5 — Replace the chain with declarative requirements + adapter registry + resolver

The most substantive change. See Architecture §1-3.

The `[sandbox]` block in scope config drops `chain` and `minimum_tier`;
gains `required_tier` and (optional) `required_shapes`. The runtime
ships an internal adapter registry (not user-facing) and a resolver
that filters registered adapters by platform compatibility, delivered
tier ≥ required, supported shapes ⊇ required, and plugin tier
requirements ≤ delivered tier; sorts by priority; picks the highest.

**Rationale.** The chain shipped as a v0.1 stopgap. Industry practice
(Cargo, Rust, Nix, K8s, Bazel) is "declare what you need; system
resolves." Switching now (before activation entrenches the chain at
the user surface) costs less than migrating later.

### D6 — Plugin-side tier declarations

Plugin `tau.toml` manifests gain an optional `[sandbox]` block:

```toml
[sandbox]
required_tier = "strict"  # plugin refuses to load if adapter delivers less
required_shapes = []      # optional; auto-derived from declared capabilities
```

Both fields are optional with `#[serde(default)]`. Plugins that don't
need to assert tier requirements (most of them) leave the block empty
or omit it. Plugins that do (credentials handling, crypto, untrusted
network calls) declare their floor.

The resolver checks plugin requirements as part of adapter filtering:
an adapter is only a candidate if EVERY plugin's `required_tier` is
≤ adapter's delivered tier.

**Rationale.** Symmetric model: project, plugin, and adapter all
declare what they need/provide. Resolver finds the intersection.
Without plugin-side tier declarations, a project with mixed-trust
plugins has to set its tier to the strictest floor; with them, the
plugin author signals their needs and the runtime enforces.

## Architecture

### §1 — Project requirements (scope config schema v2 → v3)

The `[sandbox]` block in `<scope>/config.toml` becomes:

```toml
[sandbox]
required_tier = "strict"      # required: "strict", "light", or "none"
required_shapes = []           # optional; auto-derived from plugins if absent
```

Field semantics:

- **`required_tier`** (`SandboxTier`): the minimum sandbox tier this project
  requires. Defaults to `"strict"` when the field is absent (sensible-default
  for tau's security posture). Setting `"none"` is the persistent opt-out
  (allows passthrough to satisfy).

- **`required_shapes`** (`Vec<CapabilityShape>`): explicit shape
  requirements. Optional. When absent, the resolver auto-derives the union
  of shapes from each plugin's declared capabilities (via
  `Capability::required_shape()`). Useful when the project wants to be
  more strict than its plugins (e.g., require the adapter to support a
  shape that no installed plugin uses today, to forbid future plugin
  installs from missing the capability surface).

**Schema migration v2 → v3:**

| v2 field | v3 outcome |
|---|---|
| `chain: Vec<SandboxAdapterConfig>` | Removed. On load, if present, emit `tracing::warn!` once-per-process: "v2 [sandbox] chain is deprecated; project requirements derived from old config — run `tau sandbox setup` to write a v3 config." Best-effort migration: derive `required_tier` from `minimum_tier` if set, else default to `"strict"`. Ignore chain entries (the v3 resolver handles platform matching automatically). |
| `minimum_tier: Option<SandboxMinimumTier>` | Maps directly to `required_tier`. If unset in v2, default to `"strict"` in v3. |

Bumping `MAX_SUPPORTED_SCOPE_CONFIG_SCHEMA_VERSION` from 2 to 3.
v2 configs auto-load with the migration warn; v3 is canonical going
forward. `tau sandbox setup` rewrites the file in v3 form.

In practice, given priority 12 shipped on 2026-05-03 and activation lands
within a week, very few projects have a v2 `chain` field committed. The
auto-migration is mostly forward-compatibility for the few that do.

### §2 — Adapter registry (internal, not user-facing)

Lives in `tau-runtime::sandbox::registry`. Each registered adapter
declares metadata:

```rust
struct AdapterRegistration {
    kind: SandboxAdapterKind,         // Native, Container, Remote, Passthrough
    platforms: PlatformSet,            // {Linux} | {Linux, MacOS, Windows} | Any
    tiers_supported: Vec<SandboxTier>, // [Light, Strict] etc.
    shapes_supported: CapabilityShapeSet,
    priority: u32,                      // higher = preferred when multiple match
    construct: fn(&AdapterOptions) -> Box<dyn Sandbox>,
}
```

v0.2 ships four registrations:

| Adapter | Platforms | Tiers | Priority |
|---|---|---|---|
| **Native** | Linux only | Light, Strict | 100 |
| **Container** | Linux, macOS, Windows (requires docker/podman binary) | Strict | 50 |
| **Remote** | Any (requires backend config) | Strict | 25 |
| **Passthrough** | Any | None | 0 |

The registry is internal; users don't write registrations or modify the
priority. New adapters are added via tau's source code (or, in Phase 2,
via target-triple registry sub-project B).

The platform check at registry-time is rough (e.g., "Native is Linux-only");
the actual runtime feasibility is verified by `probe()` (e.g., "this Linux
kernel has landlock V1"). Two-stage: registry filters out impossible
adapters; probe filters out unavailable ones.

### §3 — Resolver

```rust
pub fn resolve_adapter(
    project: &SandboxRequirements,
    plugins: &[PluginManifest],
) -> Result<Arc<SandboxAdapter>, ResolutionError> {
    let platform = detect_platform();
    let required_shapes = project
        .required_shapes
        .clone()
        .unwrap_or_else(|| derive_shapes_from_plugins(plugins));

    // Highest plugin-side tier requirement.
    let plugin_tier_floor: SandboxTier = plugins
        .iter()
        .filter_map(|p| p.sandbox.required_tier)
        .max()
        .unwrap_or(SandboxTier::None);
    let effective_required_tier = project.required_tier.max(plugin_tier_floor);

    let mut tried: Vec<(String, ResolutionRejection)> = Vec::new();
    let mut candidates: Vec<&AdapterRegistration> = Vec::new();

    for registration in REGISTRY.iter() {
        // §3.1 — platform match
        if !registration.platforms.contains(platform) {
            tried.push((registration.kind.name(), ResolutionRejection::PlatformMismatch));
            continue;
        }
        // §3.2 — probe
        let adapter = (registration.construct)(&AdapterOptions::default());
        let probe = block_on(adapter.probe());
        let delivered = match probe {
            SandboxProbe::Available { tier, .. } => tier,
            SandboxProbe::Unavailable { reason } => {
                tried.push((registration.kind.name(), ResolutionRejection::ProbeUnavailable(reason)));
                continue;
            }
        };
        // §3.3 — tier match
        if delivered < effective_required_tier {
            tried.push((registration.kind.name(), ResolutionRejection::TierTooLow {
                delivered, required: effective_required_tier
            }));
            continue;
        }
        // §3.4 — shape support
        if !required_shapes.is_subset_of(&registration.shapes_supported) {
            tried.push((registration.kind.name(), ResolutionRejection::ShapesUnsupported {
                missing: required_shapes.difference(&registration.shapes_supported)
            }));
            continue;
        }
        candidates.push(registration);
    }

    // §3.5 — pick highest-priority match
    candidates.sort_by_key(|r| std::cmp::Reverse(r.priority));
    let chosen = candidates.first().ok_or_else(|| {
        ResolutionError::NoAdapterMatches { tried, platform, required_tier: effective_required_tier }
    })?;
    Ok(Arc::new((chosen.construct)(&AdapterOptions::default())))
}
```

The error case carries enough structured information to render guided
messages (see §6).

### §4 — Plugin manifest schema additions

Plugin `tau.toml` gains optional `[sandbox]` block:

```toml
[plugin]
name = "credentials-store"
version = "0.1.0"

[capabilities]
fs.read = { paths = ["${PROJECT}/.env"] }

[sandbox]
required_tier = "strict"   # optional; default = "none" (i.e., no plugin-side floor)
required_shapes = []        # optional; auto-derived from capabilities
```

`tau-domain::PluginManifest` gains a `sandbox: PluginSandboxRequirements`
field (`#[serde(default)]`). Existing plugin manifests continue to parse
without changes.

### §5 — CLI surface

**Existing subcommands gain a global `--no-sandbox` flag.**

The flag lives at the `Cli` level (so it's available on every subcommand
that triggers a plugin spawn). When set, it overrides resolved adapter
to passthrough for this invocation only. Plugin-side `required_tier`
declarations are bypassed when `--no-sandbox` is active (the user is
explicitly opting out).

**New: `--sandbox <kind>` flag** for forcing a specific adapter:

```
tau chat --sandbox container my-agent
tau run --sandbox native my-agent ...
```

Valid kinds: `native`, `container`, `passthrough` (and future: `remote`).
Bypasses adapter selection but NOT probing or validation: tau still
probes the named adapter on the local host, and rejects the run if the
adapter reports `Unavailable` (e.g., `--sandbox native` on macOS exits 2
with a clear "native not applicable on this platform" error). Plugin-side
`required_tier` checks still apply (e.g., `--sandbox passthrough` on a
project whose plugin demands tier=strict will fail Layer 3 unless
`--no-sandbox` is also set, which acknowledges the bypass intent
explicitly). Useful for debugging ("does this bug repro under container?").

`--no-sandbox` is exactly equivalent to `--sandbox passthrough` AND
disabling plugin-side tier requirement checks (since the user is opting
out of the entire sandboxing system, including the plugin-level guards).

**New subcommand: `tau sandbox status`** (diagnostic):

```
$ tau sandbox status
platform: macOS 14.0 (Darwin arm64)

adapters detected:
  native:        not applicable on this platform
  container:     available, tier=strict, podman 5.2.0
  remote:        not configured
  passthrough:   available, tier=none

project requirements (<scope>/config.toml):
  required_tier: strict
  required_shapes: auto-derived from plugins
    - filesystem-read       (from anthropic, fs-read)
    - process-exec          (from shell)
    - network-http          (from anthropic)

plugin requirements:
  anthropic:              tier=any   shapes=[network-http]                ✓
  fs-read:                tier=any   shapes=[filesystem-read]             ✓
  shell:                  tier=any   shapes=[process-exec]                ✓

resolution: container  (priority=50, only adapter satisfying tier=strict)

next plugin spawn will use: container  (one fresh container per spawn)
```

Non-interactive, safe to run anywhere, exit 0 always (it's a status
report; configuration errors are reported in the output rather than via
exit code).

**New subcommand: `tau sandbox setup`** (interactive wizard):

```
$ tau sandbox setup
detecting platform... macOS 14.0 (Darwin arm64)
probing adapters...
  ✗ native       — not applicable (requires Linux)
  ✓ container    — available (podman 5.2.0)
  ✗ remote       — not configured
  ✓ passthrough  — always available

select required tier for this project:
  [1] strict     — full kernel/container isolation (recommended for prod)
  [2] light      — filesystem isolation only (Linux native only)
  [3] none       — no enforcement; allows passthrough (development scratch)
> 1

writing <scope>/config.toml:

  [sandbox]
  required_tier = "strict"

selected adapter on this machine: container

the project's [sandbox] block will resolve to:
  - on this Mac (with podman): container
  - on Linux teammates' boxes (with landlock): native
  - on machines without docker/podman/landlock: error (guided)

run `tau sandbox status` anytime to see the current resolution.
```

Non-interactive variant for CI scaffolding:

```
$ tau sandbox setup --tier strict --non-interactive
writing <scope>/config.toml...
  [sandbox]
  required_tier = "strict"
done.
```

### §6 — Guided error messages

The biggest UX delta vs priority 12. When resolution fails, the error
includes (a) what was required, (b) what was detected, (c) per-adapter
status with rejection reasons, and (d) actionable next steps:

```
$ tau chat my-agent
error: no sandbox adapter satisfies project requirements

  project: required tier=strict, shapes=[fs.read, fs.write, net.http]
  detected platform: macOS 14.0 (Darwin arm64)

  adapter status on this machine:
    native:       not applicable on this platform (requires Linux)
    container:    UNAVAILABLE — neither docker nor podman on PATH
                  → install Docker Desktop: https://docker.com/desktop
                  → or install podman:       brew install podman
    remote:       not configured
                  → see `tau sandbox setup` to configure a remote backend
    passthrough:  available, but tier=none < required=strict

  options to proceed:
    [a] install a container runtime (recommended; preserves required tier)
    [b] reduce required_tier in <scope>/config.toml:
          [sandbox]
          required_tier = "light"   # filesystem isolation only (still secure)
        — or "none" (no enforcement) for development-only opt-out
    [c] run with --no-sandbox (this invocation only, bypasses all checks)
```

This replaces the priority 12 `NoAdapterAvailable { tried }` error with a
structured-rendering version. The data structure is the same; the renderer
is richer.

Plugin-side mismatches get similar treatment:

```
$ tau resolve --check-sandbox
✓ anthropic
✓ fs-read
✗ credentials-plugin

3 plugins checked: 2 ok, 1 error

credentials-plugin: requires tier=strict; selected adapter (passthrough)
delivers tier=none.

  to resolve, choose one:
    - upgrade [sandbox] required_tier in <scope>/config.toml from "none" to "strict"
      (this is the same constraint the plugin asks for)
    - remove credentials-plugin from agents that don't actually use it
    - the plugin author must reduce its required_tier (NOT recommended for credentials)
```

### §7 — Data flow

End-to-end path from `tau chat` invocation to plugin spawn:

```
User: tau chat my-agent
  │
  ▼
tau-cli::main → cmd::chat::run
  │
  ├─► Scope::resolve()                           — find <scope> dir
  ├─► Scope::read_config() → ScopeConfig (v3)    — parse [sandbox] block
  └─► load AgentDefinition                       — find agent in tau.toml
      │
      ▼
load_plugins(entry, scope, ...)
  │
  ├─► check --no-sandbox flag / TAU_NO_SANDBOX env
  │     IF set: build SandboxRequirements with required_tier = none
  │             (forces passthrough match)
  │     ELSE:   build SandboxRequirements from scope.config.sandbox
  │
  ├─► load lockfile, look up plugins
  ├─► collect plugin manifests, derive plugin tier requirements
  │
  ├─► resolve_adapter(requirements, plugin_manifests)
  │     ├─► detect_platform()
  │     ├─► filter REGISTRY by platform/tier/shape/plugin-tier
  │     ├─► probe each candidate
  │     └─► pick highest-priority match
  │   → either Arc<SandboxAdapter> OR ResolutionError
  │
  ├─► IF ResolutionError: render guided message, exit 2
  │
  ├─► IF success: store on PluginHostOptions.sandbox_adapter
  │
  ├─► For each plugin:
  │     build_plan(manifest_caps, project_override, ctx, limits)
  │     validate_plan_against_adapter(plan, &adapter)  ← Layer 3 (priority 12 logic)
  │     plugin_host::load_*(..., options, Some(&plan))
  │       └─► PluginProcess::spawn_and_handshake
  │             └─► adapter.wrap_spawn(plan, cmd.as_std_mut())  ← Layer 4
  │             └─► cmd.spawn() — child runs sandboxed
  │
  └─► Runtime::builder().with_dyn_*().build()
      runtime.run_streaming() — agent loop (priority 12 architecture; no change here)
```

### §8 — Telemetry / observability

Every `resolve_adapter` call emits a `tracing::info!` line:

```
sandbox: resolved adapter='container' tier='strict' shapes=[fs.read, ..]
         (priority=50; required_tier='strict')
```

Failed resolutions emit `tracing::error!` before exiting. Per-plugin
spawn emits `tracing::debug!` with the validated plan + adapter name
(already present from priority 12; unchanged).

The passthrough adapter logs at `tracing::warn!` level — it's selected
intentionally, but visibility matters: "sandbox: resolved adapter='passthrough'
(no enforcement; required_tier='none')". This is the once-per-process
warning that replaces priority 12's `unshare_flags_for_plan` warn.

## What changes vs priority 12

| Component | Priority 12 | This sub-project |
|---|---|---|
| `[sandbox]` schema | `chain: Vec<SandboxAdapterConfig>` + `minimum_tier` | `required_tier` + `required_shapes` |
| Adapter selection | `select_adapter` walks chain, first-Available wins | `resolve_adapter` filters registry, highest-priority match wins |
| Plugin manifest | Capabilities only | Capabilities + optional `[sandbox] required_tier` |
| Layer 3 validation | Plan shapes vs adapter shapes | Same + plugin tier vs adapter tier |
| Adapter activation | `None` at all spawn sites | Resolver result on `PluginHostOptions.sandbox_adapter` |
| CLI surface | `tau resolve --check-sandbox` | Same + `--no-sandbox`, `--sandbox <kind>`, `tau sandbox status`, `tau sandbox setup` |
| Error messages | `NoAdapterAvailable { tried }` plain | Structured guided multi-option errors |
| Schema version | Scope config v2 | Scope config v3 (auto-migration from v2) |
| Passthrough adapter | Did not exist | New; Mock-equivalent for production use, gated by explicit opt-in |

## Testing strategy

Sub-project A's own coverage delta:

- **Resolver** — exhaustive unit tests on the platform/tier/shape filtering matrix. Zero-tests-passing-trivially is unacceptable (see priority 12 lessons). Tests cover: every adapter on every platform, tier-too-low rejection, shape-missing rejection, plugin-tier-too-high rejection, multiple-candidates tied (highest priority wins), no-candidates (guided error), passthrough-explicit-opt-in.
- **Adapter registry** — round-trip test that every registered adapter has a complete metadata record (no missing platforms/tiers/shapes/priority).
- **Schema migration v2 → v3** — fixtures with v2 `chain` configs auto-migrate to v3 with the warn; v3 configs round-trip cleanly; mixed configs (some v2 fields + some v3) reject with a clear error.
- **CLI flags** — integration tests for `--no-sandbox`, `--sandbox <kind>`, `tau sandbox status`, `tau sandbox setup` (interactive + non-interactive). All use `assert_cmd::cargo_bin("tau")` + `TempDir` fixtures.
- **Guided error rendering** — snapshot tests on the multi-option error output for: no-adapter-available, plugin-tier-mismatch, platform-not-supported, all-three-stacked.
- **`tau resolve --check-sandbox`** — extends to also surface plugin tier mismatches.
- **Plugin manifest** — round-trip test that the new `[sandbox]` block parses with `#[serde(default)]` and rejects malformed values.

End-to-end coverage on Linux CI (the existing job, no new matrix slots):

- A test agent runs against the activated sandbox; assert that fs-read denies an out-of-allowlist path; assert that shell denies a non-allowlisted command.
- Same e2e tests that priority 12 removed (sub-project D from the followups doc) are NOT re-introduced here — they remain Sub-project D's responsibility. This sub-project relies on existing unit + Layer 3 coverage for the activation path.

## What this sub-project does NOT do

- **Real-kernel landlock e2e tests on CI** — still sub-project D from the followups doc.
- **Layer 2 install-time cross-check** — still sub-project B from the followups doc.
- **Per-command exec gating (landlock V2)** — still sub-project E.
- **Per-host network filter (nftables-in-netns)** — still sub-project F.
- **`tau check` standalone command** — Phase 2 sub-project A (different from this sub-project).
- **Tau target triple registry** — Phase 2 sub-project B; this design is the precursor.
- **macOS sandbox-exec / Windows AppContainer adapters** — sub-projects J, K.

## Naming check

- `passthrough` (adapter kind name) — chosen over `bare`, `none`, `bypass`, `unsandboxed` for readability and lack of confusion with "host" (which could mislead).
- `required_tier` (project + plugin field) — symmetric naming. Reads naturally in TOML.
- `tau sandbox status` / `tau sandbox setup` — `tau sandbox` is the new verb group; status is non-mutating, setup is mutating + interactive by default.
- `--no-sandbox` (CLI flag) — kept exactly. `--sandbox <kind>` is the more general form; `--no-sandbox` is the well-known shorthand for `--sandbox passthrough`.

## Followup question parked

During brainstorm, two parked items deserve their own future sub-projects:

1. **Plugin transport pluggability.** Today plugins talk to tau over local stdio IPC. Some users want remote execution (HTTP, WebSocket, SSH-tunneled) — for instance, running plugins on a different machine than the agent loop. Worth its own sub-project, separate from sandboxing.

2. **Naming review of `load_llm_backend` and siblings.** The current `load_*` verb conflates "spawn the plugin process" and "wire it into the runtime registry." Better names might be `start_*`, `spawn_*_plugin`, or `mount_*`. Worth surfacing in a small refactor sub-project that touches plugin_host's public API.

Both are recorded here so they don't get lost; neither is in scope for this sub-project.

## References

- ADR-0014 — sandboxing port + adapter pattern, six decisions of priority 12.
- `docs/explanation/tau-as-language.md` — vision doc; Phase 2 sub-projects A-G.
- `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` — original sub-project A scope (this design supersedes the chain-based mechanism described there; gaps tracked elsewhere in that doc remain valid).
- Bazel platforms/toolchains documentation — the inspiration for the declarative-requirements model. Particularly `bazel-platforms` and the resolution algorithm in `bazel/src/main/java/com/google/devtools/build/lib/skyframe/toolchains/`.
- Cargo target triples + `~/.cargo/config.toml` layered config — analogous separation of project declaration from per-machine override.

## Approval gate

This spec is the design output of the brainstorming session; it requires
user review before transitioning to the implementation plan. After approval,
the next step is `superpowers:writing-plans` to derive the per-task
implementation plan from this spec.
