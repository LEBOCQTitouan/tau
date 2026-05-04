# ADR-0015: Sandbox activation by default — declarative requirements + adapter registry + resolver

**Status:** Accepted
**Date:** 2026-05-04
**Deciders:** Titouan Lebocq
**Supersedes:** —
**Closes:**
- Sub-project A from [the sandboxing follow-ups](../superpowers/specs/2026-05-03-sandboxing-followups.md) — sandboxing is now ON by default for every plugin spawn.
- ADR-0014 §1 (chain-based selection): the `Sandbox` port and the existing adapters are unchanged, but the *selection mechanism* is replaced (see Decision 5 below).
**Amends:**
- [ADR-0014](0014-sandboxing.md) §1 — `tau-runtime::sandbox::chain::select_adapter` is removed. Selection now flows through `tau-runtime::sandbox::resolver::resolve_adapter` against a static `AdapterRegistration` slice.
- [ADR-0014](0014-sandboxing.md) §5 — the `<scope>/config.toml [sandbox]` section is migrated from v2 (chain + minimum_tier) to v3 (required_tier + required_shapes). v2 lockfiles auto-load with a `tracing::warn!` and best-effort migration; v3 is canonical.
**Refines:**
- [ADR-0008](0008-plugin-loading.md) — `PluginHostOptions` now carries `sandbox_adapter`, `force_passthrough`, and `force_adapter_kind` fields; every spawn site (`describe_plugin`, `load_llm_backend`, `load_tool`, `load_storage`) flows through the resolver.

## Context

Tier 3 priority 12 ([ADR-0014](0014-sandboxing.md)) shipped the sandbox infrastructure: a `tau_ports::Sandbox` port, two concrete adapter crates (`tau-sandbox-native`, `tau-sandbox-container`), Layer 3 pre-flight validation, and a chain-based `select_adapter` function. Activation was deferred: production plugin spawn sites still passed `None` for the sandbox argument. The result: zero plugins were actually sandboxed.

Sub-project A from the followups doc closes that gap, but the original "chain" model surfaced design issues that justify revisiting it before activating it everywhere:

- **Chains conflate intent with mechanism.** A chain like `[{ kind = "native" }, { kind = "container" }, { kind = "passthrough" }]` smushes together (a) what the project requires (Strict isolation), (b) which mechanisms can deliver it on this host, and (c) how to fall back. The user is forced to author the resolution algorithm.
- **Industry pattern is "declare requirements, let the toolchain pick the toolchain."** Bazel's platforms/toolchains is the closest analogue: the BUILD declares `target_platform(cpu="arm64", os="linux")`, the toolchain registry advertises which toolchains support which `(cpu, os)` tuples, and Bazel's resolver picks. The user never writes a list of toolchains in priority order.
- **Probe results are runtime data; chain authoring is upfront.** A chain in `tau.toml` can't react to "Docker isn't installed on this host" as cleanly as a resolver invoked at runtime against a registry of "what does each adapter advertise?".
- **Plugin-side requirements are first-class.** A plugin that genuinely requires Strict tier (e.g. one that handles untrusted user input) needs to express that, and the project + plugin requirements need to *unify* before the resolver picks an adapter. A chain has no place for this.
- **Activation default needs a defensible failure mode.** macOS without Docker has no Linux-native landlock, no container runtime — should tau refuse to start, fall back to no isolation with a warn, or something else? The chain model didn't have a clean answer.

The design space (full options table in [the design doc](../superpowers/specs/2026-05-04-sandbox-activation-design.md)) was explored fresh. The chain was rejected in favor of declarative requirements + a static adapter registry + a runtime resolver. Six discrete decisions follow.

## Decision 1 — Sandboxing is ON by default; opt-out is explicit and visible

**Decision:** every `Runtime::run` builds a `SandboxPlan` per plugin, calls `resolve_adapter` to obtain a concrete `SandboxAdapter`, and passes both into `wrap_spawn` at the plugin host layer. The escape hatch is the explicit `--no-sandbox` global flag (or `[sandbox] required_tier = "none"` in scope config), which selects the `passthrough` adapter — never silent fall-through.

**Context:** the alternative was opt-in (a flag like `--enable-sandbox`). Defense in depth means the secure default must be the path of least resistance; if the user opts out, they declare it. Constitution G12 and ADR-0014 §6 already established "OS-level enforcement is the backstop"; not activating it by default would leave that promise unredeemed.

**Consequences:**
- Existing tau projects with no `[sandbox]` block default to `required_tier = strict`. On Linux, the native adapter satisfies this. On macOS without Docker, resolution fails with a guided diagnostic (see Decision 3).
- The `passthrough` adapter (Decision 4) is a registered first-class adapter, not a "no sandbox" sentinel. `--no-sandbox` is honored by routing to passthrough; the user always knows which adapter is in play.
- Operators get a one-line CI rule: "no project ships without a `[sandbox]` block AND that block resolves on the target platform". `tau resolve --check-sandbox` is the gate.

**Alternatives considered:**
- **Opt-in flag.** Rejected: violates secure-by-default. Defense-in-depth is undermined when the kernel layer is dormant unless explicitly turned on.
- **Auto-fall-soft to passthrough on resolution failure.** Rejected: silent demotion is the worst possible failure mode for a security feature. The user must learn that resolution failed, not discover via a post-mortem.

## Decision 2 — `PluginHostOptions.sandbox_adapter`, not Runtime::builder DI

**Decision:** the resolved `Arc<SandboxAdapter>` is carried on `PluginHostOptions` (along with `force_passthrough: bool` and `force_adapter_kind: Option<SandboxAdapterKind>` for the CLI overrides), not injected through a builder method on `Runtime`. The CLI's `tau-cli/src/cmd/plugin_loader.rs::load_plugins` is the single integration point that reads scope config, calls `resolve_adapter`, and threads results through.

**Context:** the alternative was `Runtime::builder().with_sandbox(adapter)` — closer to the `with_*` methods already on the builder. But the builder is constructed in `Runtime::run`, before scope config is loaded. Pushing sandbox resolution into the builder would have required the builder to depend on `tau_pkg::scope`, which inverts the existing dependency graph (`tau-pkg` depends on `tau-runtime` for `RuntimeBuilder` reuse).

**Consequences:**
- `Runtime` itself is unaware of sandboxing. The kernel still calls `plugin_host::spawn_and_handshake`; the sandbox enforcement happens during `wrap_spawn`, transparent to the agent loop.
- The CLI's `plugin_loader` becomes the natural home for resolver + adapter wiring — it already loads scope config, lockfile, and plugin manifests.
- Tests that don't go through the CLI (the runtime's own integration tests) can construct `PluginHostOptions { sandbox_adapter: None, .. }` to skip sandboxing entirely. The mock adapter is still injectable via the `TAU_TESTING_ALLOW_MOCK_SANDBOX=1` env-var override (preserved from priority 12).
- A future "library mode" caller (Phase 2 work) sets the field directly without going through the CLI surface.

**Alternatives considered:**
- **`Runtime::builder().with_sandbox(adapter)`.** Rejected: inverts dep graph; adds a builder method that's only called from one place.
- **Static config / lazy_static.** Rejected: testing is harder; resolution becomes implicit.

## Decision 3 — Hard refuse on resolution failure (no silent fall-through)

**Decision:** when `resolve_adapter` returns `Err(ResolutionError)`, the runtime exits with code 2 and a guided multi-option diagnostic. There is no fall-back to passthrough, no warning-and-continue, no degraded mode. The user must either (a) install a satisfying adapter (e.g. Docker), (b) lower `required_tier` in scope config, or (c) pass `--no-sandbox` explicitly.

**Context:** the failure mode of a security feature is what defines it. Silent demotion gives users a sandbox icon in their config and zero enforcement at runtime. Three-bucket exit codes (ADR-0007 §7) give us "configuration error" = exit 2; this is the canonical case.

**Consequences:**
- macOS users without Docker who don't have a `[sandbox]` block and don't pass `--no-sandbox` get exit 2 on first run. The diagnostic shows: which adapters were tried, why each was rejected (platform / probe / tier / shape / plugin tier), and three remediations they can pick from. `tau sandbox setup` (Decision 5 corollary) walks them through option (b) interactively.
- The `tau resolve --check-sandbox` subcommand exits 2 on the same conditions, giving CI a pre-flight gate without needing to spawn plugins.
- The error rendering surface (`crates/tau-cli/src/cmd/error_render.rs`) gets formal multi-option output with insta snapshot tests.

**Alternatives considered:**
- **Fall-soft to passthrough with a warn.** Rejected: see Decision 1's discussion. The whole point of activation is that the kernel layer is real, not optional.
- **Refuse only if no `[sandbox]` block exists; permit fall-soft if the block lists alternatives.** Rejected: the chain model in disguise; we already rejected chains.

## Decision 4 — `passthrough` is a registered first-class adapter, not "None"

**Decision:** add a `passthrough` adapter (`crates/tau-runtime/src/sandbox/passthrough.rs`, ~30 LOC) that implements `tau_ports::Sandbox` directly. It probes `Available` everywhere, advertises every shape as supported, validates every plan as Ok, and `wrap_spawn` returns the command unchanged. It's registered in the adapter registry with `tier = None` and `priority = 0` (lowest). `SandboxAdapter` gains a `Passthrough` variant.

**Context:** the priority-12 design represented "no sandbox" as `Option<SandboxAdapter>::None`. That was always going to age badly: every consumer of `Option<&SandboxAdapter>` gets a special case for `None`, the overrides logic (`--no-sandbox`) has to short-circuit before reaching the adapter layer, and the registry has to model "the always-available fall-back" as a separate concept from "the available adapters".

**Decision** treats "no isolation" as a real adapter with its own behavior: it satisfies `required_tier = none`, fails `required_tier = light` and stronger via the tier filter, and fails plugin tier requirements via the plugin-tier filter. The user-facing name is "passthrough", consistent across `tau sandbox status`, error messages, and `--sandbox passthrough`.

**Consequences:**
- `--no-sandbox` is sugar for `--sandbox passthrough` (forces the adapter regardless of registry filtering).
- `tau sandbox status` always shows passthrough as Available, with an explicit "delivers no isolation" note. Users see what they'd be getting.
- The registry's lowest-priority entry catches all `required_tier = none` requests when no other adapter probed Available.
- Plugin-tier filtering: a plugin declaring `required_tier = strict` rejects passthrough even when the project says `required_tier = none` (see Decision 6).

**Alternatives considered:**
- **Keep `Option<SandboxAdapter>::None`.** Rejected: special cases ripple through every caller; the override logic gets increasingly fragile.
- **Name it `bare`, `host`, or `none`.** Rejected: `none` collides with the tier name; `bare` is jargon; `host` is ambiguous (host vs guest namespace). `passthrough` is the established term in the network/security space for "let the bytes through unchanged".

## Decision 5 — Declarative requirements + adapter registry + resolver (Bazel-style)

**Decision:** the scope config schema migrates from v2 chain (`[sandbox] chain = [{ kind = "native" }, { kind = "container" }, { kind = "passthrough" }]`) to v3 declarative (`[sandbox] required_tier = "strict"`, optional `required_shapes = [...]`). A static `AdapterRegistration` slice in `tau-runtime::sandbox::registry` enumerates the four adapters with their `(platforms, tiers_supported, shapes_supported, priority)` metadata. `resolve_adapter` filters the registry through a 5-stage pipeline (platform → probe → tier → shape → plugin tier) and picks the highest-priority remaining candidate.

**Context:** Bazel's platforms/toolchains is the closest analogue. The BUILD file declares the *target platform* (`cpu="arm64", os="linux"`); each toolchain declares which `target_compatible_with` constraints it satisfies; Bazel's resolver picks. The user writes intent ("I need to build for arm64-linux"), the toolchain author writes capability ("I support arm64-linux", with priority), and the resolver produces a concrete pick at build time.

The same shape applies to sandboxing:
- The project author writes intent: `required_tier = "strict"`, optionally `required_shapes = ["FilesystemRead", "ProcessExec"]`.
- The adapter author registers capability: native says `(linux, [light, strict], [Filesystem*, Process*, Network*], priority=100)`; container says `(any, [light, strict], [Filesystem*, Process*, Network*], priority=50)`; remote says `(any, [light, strict], [Filesystem*, Process*], priority=25, probe=Unavailable)`; passthrough says `(any, [none], [all], priority=0)`.
- The resolver — invoked at every `Runtime::run` — computes the intersection and picks.

**Consequences:**
- New adapters land additively. Adding a `tau-sandbox-macos` adapter (sub-project J) becomes one new `AdapterRegistration` entry plus the adapter crate. No changes to consumers.
- The error message for "nothing matches" can list every adapter that was tried and why — the resolver has all the data; it just renders it via `ResolutionError::NoAdapterMatches { tried: Vec<(name, ResolutionRejection)> }`.
- Tests can probe the resolver directly (`resolve_adapter(&requirements, &plugins).await`) without spawning a runtime. The 5-stage pipeline is unit-testable in isolation.
- Schema migration from v2 to v3 is best-effort: v2 lockfiles auto-load with a `tracing::warn!` (consistent with the lockfile v3→v4 precedent from priority 7) and the resolver picks a sensible v3 equivalent. v3 is the canonical form going forward.
- A new family of CLI subcommands grows naturally: `tau sandbox status` (read-only diagnostic), `tau sandbox setup` (interactive + non-interactive scope config writer). The resolver is the single source of truth that `status` queries.

**Alternatives considered:**
- **Keep the chain model.** Rejected for the reasons in Context: conflates intent with mechanism, doesn't model plugin-side requirements cleanly, can't react to runtime probe data.
- **Custom scoring function instead of priority + filter pipeline.** Rejected: more flexible than needed; harder to reason about; the priority sort suffices for the four adapters we have, and adding new ones doesn't require rebalancing scores.
- **Dynamic adapter loading (plugins as adapters).** Rejected: out of scope; the registry is intentionally compile-time so adapters can be type-checked. Future work could add a registration ABI.

## Decision 6 — Plugin-side tier declarations (symmetric to project requirements)

**Decision:** add `[sandbox]` to the plugin manifest (`PluginSandboxRequirements { required_tier: Option<PluginRequiredTier>, required_shapes: Vec<CapabilityShape> }`). The resolver's 5th filter stage rejects any adapter whose tier is below the maximum tier required across all plugins in the load set. `ResolutionError::PluginTierMismatch` is a separate error variant from `NoAdapterMatches` so the diagnostic can identify which plugin is asking for what.

**Context:** a plugin that processes untrusted input (e.g. a parser, a code formatter, an HTTP client) has a defensible reason to insist on Strict tier even if the project author would have been content with Light. Without a plugin-side declaration, the project's `[sandbox]` block is the only signal — and the project author may not know which plugins need stronger isolation.

The symmetry mirrors `dependencies` resolution: the project declares "I want package X"; the plugin declares "I require package Y at version Z". The dep resolver intersects. The sandbox resolver does the same with tiers.

**Consequences:**
- Existing plugin manifests with no `[sandbox]` block default to `required_tier = None` (no extra requirement beyond the project's). This is `#[serde(default)]` on `PluginSandboxRequirements`, so unmodified plugin manifests remain valid.
- The five existing plugins (anthropic, ollama, openai, fs-read, shell) ship with no `[sandbox]` block in this sub-project. Sub-project B adds explicit declarations as part of the per-plugin compatibility verification pass.
- `tau resolve --check-sandbox` is extended to surface plugin-tier mismatches even when the project's `required_tier = none` would otherwise have skipped tier checks. (Without this, a project with `required_tier = none` would silently let a plugin's `required_tier = strict` slip through.)
- The diagnostic for plugin-tier mismatch names the offending plugin: "plugin `foo` requires Strict but resolved adapter `passthrough` provides None — please install Docker or run on Linux".

**Alternatives considered:**
- **Plugin-side declarations only (no project-side).** Rejected: the project author needs to declare "I want this whole agent to run Strict" for the case where a plugin's manifest is silent.
- **Project-side only (no plugin-side).** Rejected: makes the project author responsible for knowing each plugin's needs. Doesn't scale to a real plugin ecosystem.
- **`required_shapes` on plugins, no `required_tier`.** Rejected: shapes are a finer-grained vocabulary that plugins use to express *what they need to do* (FilesystemRead, ProcessExec, ...). Tier is a coarser vocabulary expressing *how strong an isolation regime they require*. They're not interchangeable; both exist on the plugin side, with `required_shapes` already present from priority 12.

## Implementation summary

The 12 commits on `feat/sandbox-activation-spec` (PR #23) realize these decisions via:

| Layer | Crate | Files |
|---|---|---|
| Schema migration v2 → v3 | `tau-pkg` | `src/scope.rs` |
| Plugin manifest schema | `tau-domain` | `src/package/sandbox.rs` |
| Adapter registry | `tau-runtime` | `src/sandbox/registry.rs` |
| Passthrough adapter | `tau-runtime` | `src/sandbox/passthrough.rs` |
| Resolution error taxonomy | `tau-runtime` | `src/sandbox/resolution_error.rs` |
| Resolver (5-stage pipeline) | `tau-runtime` | `src/sandbox/resolver.rs` |
| Plugin host integration | `tau-runtime` | `src/plugin_host/mod.rs` |
| CLI integration | `tau-cli` | `src/cmd/plugin_loader.rs` |
| Global CLI flags | `tau-cli` | `src/cli.rs` |
| Error renderer | `tau-cli` | `src/cmd/error_render.rs` |
| Sandbox subcommands | `tau-cli` | `src/cmd/sandbox.rs` |
| `--check-sandbox` extension | `tau-cli` | `src/cmd/resolve.rs` |

Test coverage: ~250 unit tests across the workspace pass on the 25-job CI matrix (Linux/macOS/Windows × stable/1.91); 24 doc tests pass. No new CI jobs were added (branch protection stays at 25 required checks, unchanged from the priority-12 baseline).

The mock adapter is unchanged in `tau-ports/src/fixtures.rs`; the `TAU_TESTING_ALLOW_MOCK_SANDBOX=1` env-var injection path bypasses the registry directly. Sub-project H from the followups doc handles the eventual cleanup.

## Forward links

- **Sub-project B** (plugin compatibility verification) builds directly on this work: per-plugin `tau resolve --check-sandbox` runs, plugin manifest `[sandbox]` blocks added where appropriate, Layer 2 install-time cross-check.
- **Sub-project D** (e2e CI) gains a clean activation point: e2e tests can rely on resolved adapters rather than mocking the chain.
- **Phase 2 sub-project A** (`tau check` standalone) reuses `resolve_adapter` + `validate_plan_against_adapter` as the pre-flight validation surface.
