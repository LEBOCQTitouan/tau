# Spec: Sandboxing — port + adapter, native + container

**Sub-project:** Tier 3 priority 12 of the tau project.

**Date:** 2026-05-02

**Closes:** [Constitution G12 — sandboxing](../../../CONSTITUTION.md)
(if such a document exists; otherwise, the Phase-1+ deferral
recorded in [ADR-0006](../../decisions/0006-tau-runtime.md) regarding
the v0.1 unsandboxed trust posture).

**Future ADR:** ADR-0014 will lock the six design decisions
documented here.

**Vision document:** [`docs/explanation/tau-as-language.md`](../../explanation/tau-as-language.md)
explains how the architecture chosen here serves as the foundation
for "tau as a compiled language for agentic workflows" — see
that document for the long-term design intent.

## Goals

1. **Replace the current unsandboxed trust posture** with real
   OS-level sandboxing of plugin processes. Today, any installed
   `tau install <git-url>` plugin runs with the user's full
   permissions; the capability model is enforced ONLY at the kernel
   layer (capability check before invoke + per-tool deny entries),
   not at the plugin process boundary.

2. **Hexagonal architecture** — a single `Sandbox` port with
   multiple interchangeable backend adapters. The user/admin picks
   the backend; plugins and projects cannot weaken the choice.

3. **Two adapters at v0.1**: native (Linux landlock + seccomp +
   namespaces, two tiers) and container (Docker/podman wrapper).

4. **Machine-agnostic configuration**: a single user config file
   (an ordered "adapter chain") works across machines; tau picks
   the strongest available adapter at startup. The same project
   bundle runs on a developer laptop, on CI, and on a remote
   server with deployment-appropriate sandboxing.

5. **Push validation errors leftward** — toward `tau install`,
   `tau resolve --check-sandbox`, and `tau run` startup, before
   any LLM call. Static cross-check between each plugin's required
   capability shapes and the active adapter's supported shapes.
   Hard fail on mismatch; no silent degradation.

6. **Defense-in-depth**: even if static checks pass, the OS
   sandbox enforces at runtime. A plugin attempting an undeclared
   syscall is killed; the agent loop survives via
   `ToolError::SandboxViolation` (priority 8's recoverable error
   path).

## Non-goals

- **macOS native adapter.** sandbox-exec / libsandbox FFI is its
  own ~3-week sub-project. Deferred.
- **Windows native adapter.** AppContainer via `windows-rs` is its
  own ~3-4-week sub-project. Deferred.
- **Remote sandbox adapters** (Vercel Sandbox, Sandcastle, etc.).
  Each is a future sub-project: API authentication, cold-start
  latency budgets, networking the IPC channel back to the host.
  Deferred.
- **WASM/wasmtime adapter.** Major plugin SDK rewrite (plugins
  must target `wasm32-wasip2`); WASI's network/process model
  doesn't accommodate the existing shell plugin without significant
  redesign. Phase 2 work, possibly tied to plugin marketplace
  goals.
- **`tau check` standalone subcommand.** `tau resolve
  --check-sandbox` provides a precursor in this sub-project; the
  full standalone verb is a future Phase 2 sub-project (A in the
  vision document).
- **`tau build --target <triple>`** and bundle format. Phase 2
  sub-project (C in the vision document).
- **Bypass / opt-out flags.** No `--no-sandbox` flag, no
  per-package "unsandboxed" manifest declaration. If a plugin
  cannot be sandboxed under the active adapter, it does not run.
  A future "high-trust target" backend with explicit user opt-in
  at install time can address legitimate unsandboxed use cases.

## Architecture

### Port: `tau_ports::Sandbox`

The `Sandbox` trait has lived in `tau-ports` since Phase 0 marked
PROVISIONAL. This sub-project promotes it to stable (drops the
PROVISIONAL warning), refines its surface, and locks the
semantics.

**Core trait shape (post-refinement):**

```rust
#[allow(async_fn_in_trait)]
pub trait Sandbox: Send + Sync {
    /// Per-sandbox handle. Concrete impls own the lifecycle.
    type Handle: Send + 'static;

    /// Plugin-visible name (matches package; for diagnostics).
    fn name(&self) -> &str;

    /// Probe the host for backend availability. Cheap; called at
    /// startup. Returns Available with optional metadata, or
    /// Unavailable with a human-readable reason.
    fn probe(&self) -> SandboxProbe;

    /// The set of `CapabilityShape` variants this adapter
    /// enforces. Used by Layer 3 startup validation to detect
    /// plugin-vs-adapter mismatches before spawning.
    fn supported_shapes(&self) -> CapabilityShapeSet;

    /// Validate a plan against this adapter's enforcement
    /// surface. Called after `probe()` succeeds; returns the
    /// list of unsupported shapes if any are missing.
    fn validate_plan(&self, plan: &SandboxPlan)
        -> Result<(), SandboxError>;

    /// Apply the sandbox to a plugin process spawn. Implementation
    /// receives a partially-configured `tokio::process::Command`
    /// and applies pre-exec hooks (Linux: prctl + seccomp +
    /// landlock_restrict_self). Returns a handle that the runtime
    /// owns for the plugin's lifetime.
    async fn wrap_spawn(
        &self,
        command: &mut tokio::process::Command,
        plan: &SandboxPlan,
    ) -> Result<Self::Handle, SandboxError>;
}
```

**`SandboxProbe`:**

```rust
#[non_exhaustive]
pub enum SandboxProbe {
    /// Adapter can be used on this host.
    Available {
        /// Optional metadata for diagnostics (e.g., "kernel
        /// 6.5; landlock V4; seccomp v2").
        details: String,
    },
    /// Adapter cannot be used on this host.
    Unavailable {
        /// Human-readable reason (e.g., "kernel 5.4 < 5.13
        /// required for landlock"; "no docker/podman in PATH").
        reason: String,
    },
}
```

**`SandboxPlan` (existing; stays mostly as-is):**

Existing fields: `capabilities`, `context`, `limits`. This
sub-project adds:

- `tier: SandboxTier` (an enum: `Light`, `Strict`). Maps to
  adapter-specific policy bundles.

### Capability shape vocabulary (`tau_domain::CapabilityShape`)

A NEW typed enum. Each `Capability` variant maps to one or more
shapes. Shapes are what adapters declare support for.

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityShape {
    // Filesystem
    /// fs.read with per-path allowlist (landlock; container bind).
    FsReadPerPath,
    /// fs.read at "any read access" granularity.
    FsReadBinary,
    /// fs.write with per-path allowlist.
    FsWritePerPath,
    /// fs.write at "any write access" granularity.
    FsWriteBinary,
    /// fs.exec with per-path allowlist.
    FsExecPerPath,
    /// fs.exec at "any exec" granularity.
    FsExecBinary,

    // Network
    /// network.http with per-host allowlist (netns + nftables).
    NetworkHttpPerHost,
    /// network.http at "any HTTP" granularity (seccomp socket()).
    NetworkHttpBinary,

    // Process
    /// process.spawn with per-command allowlist (seccomp execve
    /// path filter).
    ProcessSpawnPerCommand,
    /// process.spawn at "any exec" granularity.
    ProcessSpawnBinary,

    // Custom
    /// Custom capability — opaque to the sandbox. Adapters that
    /// see this in a plan return Unsupported.
    Custom,
}
```

A function on each capability variant maps to its required shape:

```rust
impl Capability {
    pub fn required_shape(&self) -> CapabilityShape { ... }
}
```

Example: `Capability::Process::Spawn { commands }` →
`CapabilityShape::ProcessSpawnPerCommand` (always — the capability
declaration is per-command, so the shape is per-command).

### Adapters as workspace crates

| Crate | Target triple | Tiers | Notes |
|---|---|---|---|
| `tau-sandbox-native` | `linux-native` | `Light`, `Strict` | landlock + seccomp + namespaces |
| `tau-sandbox-container` | `container-docker`, `container-podman` | (single) | Shells out to docker / podman binary |
| `tau-sandbox-mock` | `mock` | n/a | Test-only; refactored from `MockSandbox` in `tau-ports/src/fixtures.rs` |

### Runtime integration: `tau-runtime/src/sandbox/`

Module-level responsibilities:

- **`chain.rs`**: load adapter chain from config. Probe each in
  order. Pick first available. Construct the chosen `Box<dyn
  Sandbox>`.
- **`plan.rs`**: build `SandboxPlan` from a plugin's effective
  capabilities (post-`compute_effective`) + tier (post-floor
  enforcement) + resource limits.
- **`validation.rs`**: cross-check `plugin.required_shapes()`
  against `adapter.supported_shapes()`. Return
  `SandboxError::CapabilityShapeUnsupported { plugin, shape,
  adapter }` on mismatch.
- **`integration.rs`**: hook into existing `plugin_host::spawn`
  to call `adapter.wrap_spawn(...)` before exec.

The existing `plugin_host::spawn` flow gains a single new call
site; the adapter's `wrap_spawn` does the heavy lifting (per-
adapter implementation).

## Decision 1 — Hexagonal architecture (port + adapters)

The `Sandbox` trait is the port; multiple adapters implement it.
The runtime depends only on the trait.

**Rationale:** captures workload-specific isolation needs without
sacrificing security guarantees. Different deployments
(developer laptop / CI / production / paranoid) want different
isolation strengths; the trait + adapter pattern delivers this
without inventing N variations of the runtime.

This also captures future targets (remote sandboxes like Vercel
Sandbox / Sandcastle, WASM via wasmtime) without architectural
changes — each is an additional adapter implementation behind the
same port.

The trait drops the v0.1 PROVISIONAL warning. Future ADRs may
extend the trait with new methods, but the existing surface is
considered stable.

## Decision 2 — Tier model: floor enforcement, not cap

The native adapter exposes two tiers: `Light` and `Strict`.
Project `tau.toml` may declare a per-agent tier override:

```toml
[agents.coder.sandbox]
tier = "light"
```

**Crucial security rule: deployment config sets the FLOOR, not
the cap.** Project tau.toml can only RAISE strictness above the
deployment floor; it cannot weaken below it.

| Deployment tier | Project asks | Effective tier |
|---|---|---|
| `strict` | `light` | `strict` (request rejected silently with warning) |
| `strict` | `strict` | `strict` |
| `light` | `strict` | `strict` (request honored) |
| `light` | `light` | `light` |

**Rationale:** project tau.toml is version-controlled; an
attacker who modifies it should not gain the ability to weaken
sandboxing. The floor model preserves the security boundary while
permitting per-agent strengthening (legitimate use case: "this
agent does dangerous stuff, isolate it harder").

## Decision 3 — Pre-flight capability shape validation (Layer 3)

At `tau run` / `tau chat` startup, BEFORE any LLM call:

1. Probe each adapter in the chain. Pick first available.
2. For each registered plugin, fetch its `required_capability_shapes`
   (recorded in lockfile during `tau install`; see decision 4).
3. Compute the union of required shapes; cross-check against
   `adapter.supported_shapes()`.
4. On any mismatch: hard fail with
   `SandboxError::CapabilityShapeUnsupported`. Exit code 2.

**Output format on mismatch:**

```text
error: sandbox cannot enforce a capability required by plugin 'git-tools'

  Plugin:    git-tools@1.2.0
  Required:  Capability::Process::Spawn { commands: ["git", "make"] }
  Shape:     ProcessSpawnPerCommand
  Adapter:   native:light
  Reason:    native:light supports ProcessSpawnBinary, not
             ProcessSpawnPerCommand.

Resolutions:
  • Use a stronger tier: tau run --sandbox=native:strict
  • Use a different adapter: tau run --sandbox=container
  • Or persist your choice:  tau config set sandbox.tier strict
```

**Rationale:** silent degradation = silent security weakening. A
capability that says "only exec these specific commands" must
mean exactly that, or the capability is a lie. Auto-upgrade
("use a stronger tier than configured") would surprise the user
about what their config does. The hard-fail-with-clear-error
path makes the choice explicit.

## Decision 4 — Adapter chain for machine-agnostic config

Configuration:

```toml
# ~/.config/tau/config.toml — single file, syncable across machines
[sandbox]
adapter_chain = [
  { kind = "container", config = { runtime = "auto" } },
  { kind = "native", tier = "strict" },
  { kind = "native", tier = "light" },
]
# Optional: minimum-supported-shapes floor across all adapters.
minimum_required = ["FsReadPerPath", "ProcessSpawnPerCommand"]
```

**At startup, tau:**
1. Probes each adapter in chain order.
2. Picks the first one whose `probe()` returns `Available`.
3. If none available, fails with each adapter's `Unavailable.reason`
   listed. Exit code 2.
4. If `minimum_required` is set and the picked adapter doesn't
   meet it, fails with the gap listed. Exit code 2.

**Same config across machines yields appropriate sandboxing per
machine:**

| Machine | Has Docker? | Kernel | Picked adapter |
|---|---|---|---|
| Dev laptop (Linux 6.5, no Docker) | No | 6.5 | `native:strict` |
| Dev laptop with Podman | Yes | 6.5 | `container` |
| CI ubuntu runner | Yes | 6.5 | `container` |
| Old Linux server (5.4, no Docker) | No | 5.4 | `native:light` |

Plugin-vs-adapter validation (Decision 3) catches mismatches at
startup, BEFORE the LLM is called.

### Selection precedence

User/admin chooses; plugins/projects cannot weaken. Precedence
(later overrides earlier):

1. Built-in default: `[{kind="native", tier="strict"}, {kind="native", tier="light"}]` — works on any modern Linux without Docker.
2. User config: `~/.config/tau/config.toml [sandbox]`.
3. Per-scope config: `<scope>/.tau/config.toml [sandbox]`.
4. Env var: `TAU_SANDBOX=container` — selects a specific adapter (overrides chain).
5. CLI flag: `tau run --sandbox container` — same effect, per-invocation.

Project `tau.toml` cannot change the adapter (security boundary;
project files are version-controlled and attacker-modifiable).
Project tau.toml CAN declare per-agent tier OVERRIDES via
`[agents.<id>.sandbox] tier = "..."`, capped by Decision 2's
floor rule.

## Decision 5 — No bypass at v0.1

No `--no-sandbox` flag. No per-package "unsandboxed" manifest
declaration. Plugins that cannot be sandboxed under the active
adapter fail to spawn (Decision 3).

**Rationale:** the entire point of this sub-project is moving from
"trust everything" to "trust nothing without enforcement." A
bypass flag undermines that. If real demand surfaces for "this
specific plugin needs unsandboxed access and we trust it,"
address via a future "high-trust target backend" requiring
explicit user opt-in at install time.

## Decision 6 — Defense-in-depth runtime enforcement

Even when static validation (Layers 1-3) passes, the OS sandbox
enforces at runtime. A plugin attempting a syscall outside its
declared capabilities is killed (Linux: SIGSYS via seccomp; or
EACCES via landlock).

**Failure handling:**

1. OS catches the violation.
2. Plugin process dies (signal-induced or via plugin-side
   error handling that surfaces the violation to the IPC
   channel).
3. The IPC channel breaks; tau-runtime's plugin host detects this.
4. Current tool dispatch returns
   `ToolError::SandboxViolation { plugin, attempted, allowed }`.
5. Agent loop continues — error surfaces to the LLM as
   `MessagePayload::ToolError`. The LLM can recover, retry
   differently, or give up.

**Exit code:** 0 if the run completes (LLM recovered) or 1 if the
run terminates as `RunOutcome::Failed` (agent gave up). The
sandbox violation does NOT terminate tau — the agent loop is
resilient. This matches priority 8's `FatalError` handling.

## Validation hierarchy (the four layers)

| Layer | Catches | Mechanism |
|---|---|---|
| 1. Plugin author build (`cargo build`) | Code-vs-declaration drift | Plugin SDK type-state + `#[capabilities(...)]` macro |
| 2. `tau install` | Manifest-vs-binary CAPABILITIES disagreement | Cross-check tau.toml capabilities against the binary's embedded constant via the protocol handshake |
| 3. `tau run` startup | Plugin's required shapes ⊄ adapter's supported shapes; project asking for a tier weaker than the floor | Decision 3 cross-check; Decision 2 floor enforcement |
| 4. Runtime (during plugin execution) | Plugin behavior outside its declared capabilities | OS sandbox (seccomp / landlock / container isolation) |

**Plus a Layer 3 advisory pre-flight:** `tau resolve --check-sandbox`
runs Layer 3's logic without entering the agent loop, for
project authors validating their tau.toml in CI. Advisory only;
does not gate `tau run`.

## Adapter implementations

### `tau-sandbox-native`

Linux only at v0.1. macOS + Windows are future sub-projects.

**`Light` tier:**
- landlock filesystem isolation (per-path read/write/exec
  allowlist).
- No seccomp filtering.
- No namespaces.
- Per-host network filtering: not enforced (`NetworkHttpBinary`
  shape only — capability presence gates `socket(AF_INET)` via
  seccomp baseline; per-host filtering returns Unsupported).
- Per-command exec gating: not enforced (`ProcessSpawnBinary` shape
  only).
- Overhead: ~1-3 ms per spawn.

**`Strict` tier:**
- landlock filesystem isolation.
- seccomp syscall allowlist (whitelist tightly scoped to what
  the plugin SDK actually uses + capability-derived additions).
- Linux user + network namespaces (each plugin process in its own
  network namespace; outbound traffic via nftables filter rules).
- nftables rules for per-host network filtering (resolves declared
  hosts to IPs at plan-build time; updates rules dynamically).
- seccomp `execve` filter for per-command exec gating.
- Overhead: ~5-10 ms per spawn.

**Probe:** check `landlock_create_ruleset()` syscall availability
(minimum kernel 5.13 for landlock V1; preferred 6.0+ for V4).
Strict tier additionally requires CAP_NET_ADMIN-equivalent for
namespace setup (or `unprivileged_userns_clone`).

**Implementation libraries:**
- `landlock` crate.
- `seccompiler` crate.
- `nix` crate (already a workspace dep).
- No new heavy deps — total adapter binary footprint < 200KB.

### `tau-sandbox-container`

Single tier. Detects available container runtime: docker first,
podman second, fail if neither. Configurable via
`config.runtime = "docker" | "podman" | "auto"`.

**Per-spawn behavior:**
1. Build a docker / podman invocation: `--rm --name tau-plugin-<uuid>
   --user 1000:1000 --read-only --no-network` baseline.
2. Realize each capability:
   - `Filesystem::Read { paths }` → `--volume <host>:<container>:ro`
     for each path.
   - `Filesystem::Write { paths }` → `--volume <host>:<container>:rw`.
   - `Network::Http { hosts }` → custom network with DNS rewriting
     + iptables egress rules to allowed host IPs.
   - `Process::Spawn` → not specially constrained inside the
     container (the container itself is the boundary).
3. Plugin runs inside the container; IPC channel routed through
   the container's stdio (existing tau-plugin-protocol over MessagePack).
4. Container removed on plugin process exit.

**Probe:** check for `docker` or `podman` binary in PATH; verify
basic container can start (`docker run --rm hello-world`-style
smoke test, but using a tiny scratch image).

**Overhead:** ~100-500 ms per spawn (container startup
dominates). Acceptable for non-interactive workloads; for
interactive REPL work, users can pick `native:strict` instead.

**Supported shapes:**
- `FsReadPerPath`, `FsWritePerPath`, `FsExecPerPath` ✓
- `NetworkHttpPerHost` ✓
- `ProcessSpawnPerCommand` ✗ (returns Unsupported — containers
  don't filter exec by binary path)
- `ProcessSpawnBinary` ✓

A plugin requiring `ProcessSpawnPerCommand` cannot run under the
container adapter on v0.1 — the user must pick `native:strict`.
Future enhancement: extend container adapter with seccomp
profiles applied inside the container, restoring per-command
exec gating.

### `tau-sandbox-mock` (test-only)

Replaces the existing `MockSandbox` in `tau-ports/src/fixtures.rs`.
The fixture becomes a thin wrapper around the new mock crate.
Mock returns Available always; supports all shapes; performs no
actual isolation. Used by tau-runtime tests where the focus is
not the sandbox itself.

## Lockfile schema bump (v3 → v4)

The `LockedPlugin` struct gains a new field:

```rust
#[non_exhaustive]
pub struct LockedPlugin {
    pub manifest: PluginManifest,
    pub binary_path: PathBuf,
    pub built_at: SystemTime,
    pub binary_sha256: String,
    /// NEW (v4): the set of CapabilityShape variants this plugin
    /// declares as required. Computed at install time from the
    /// plugin's manifest + embedded CAPABILITIES constant.
    /// Used by Layer 3 startup validation.
    #[serde(default)]
    pub required_shapes: Vec<CapabilityShape>,
}
```

**Migration:** v3 lockfiles auto-upgrade on save — `required_shapes`
defaults to empty Vec. v3-leftover entries are flagged
`required_shapes_unknown` by Layer 3 validation, which falls back
to checking the plugin's manifest at runtime (slower path; emits
a tracing warning urging `tau install --rehash` or similar).

## Per-scope config schema

Add a `[sandbox]` section to `<scope>/.tau/config.toml`:

```toml
[sandbox]
adapter_chain = [
  { kind = "container", config = { runtime = "auto" } },
  { kind = "native", tier = "strict" },
]
minimum_required = ["FsReadPerPath", "ProcessSpawnPerCommand"]
```

User config (`~/.config/tau/config.toml`) uses the same schema.
Per-scope overrides user.

## Error handling (per ADR-0009 typed-error policy)

Extend `tau_ports::SandboxError` (existing — currently provisional)
with new variants:

```rust
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("requested feature not supported: {feature}")]
    Unsupported { feature: String },

    /// NEW
    #[error("plugin {plugin} requires {shape:?} which adapter {adapter} does not support")]
    CapabilityShapeUnsupported {
        plugin: String,
        shape: CapabilityShape,
        adapter: String,
    },

    /// NEW
    #[error("no adapter in the configured chain is available on this host: {reasons:?}")]
    NoAdapterAvailable { reasons: Vec<(String, String)> },

    /// NEW
    #[error("plugin {plugin} attempted {attempted_syscall} which capability {capability:?} does not allow")]
    SandboxViolation {
        plugin: String,
        attempted_syscall: String,
        capability: Option<String>,
    },

    /// Existing
    #[error("internal sandbox error: {0}")]
    Internal(String),
}
```

`tau_ports::ToolError` gains a `SandboxViolation` variant
(reflecting the runtime detection at Layer 4).

## Testing tier

### Unit tests

- `tau-sandbox-native`: probe success/failure; plan validation
  (each shape ↔ each tier matrix); seccomp filter correctness
  (mock syscalls).
- `tau-sandbox-container`: probe success/failure (Docker
  available / not); volume mount construction; capability →
  container args mapping.
- `tau-sandbox-mock`: trivial.
- `tau-runtime/src/sandbox/`: chain probe ordering; selection
  precedence (config / env / CLI flag); plan computation from
  capabilities + tier; validation cross-check.

### Integration tests

- `tau-sandbox-native` end-to-end on the existing Linux CI
  runner: spawn a real plugin under landlock, attempt a denied
  fs operation, verify the plugin process is killed and tau
  surfaces `ToolError::SandboxViolation`.
- `tau-sandbox-container` end-to-end with Docker preinstalled
  on the ubuntu runner: same.
- Mismatch tests: plugin requires `ProcessSpawnPerCommand`,
  active adapter is `native:light` → tau startup exits 2 with
  the documented error.

### CI matrix

No new CI jobs. Branch protection stays at 23 required checks.
Native tests run in `test (ubuntu-latest / *)` slots; container
tests run in the same slots gated on Docker availability (which
ubuntu-latest provides).

macOS and Windows runners SKIP the native + container tests
(adapter unavailable on those platforms in v0.1). They still run
the `tau_ports::Sandbox` trait conformance tests (mock adapter).

## Vision: tau as a compiled language for agentic workflows

This sub-project's architecture is the foundation for a longer-
term goal: tau as a compiled language. See
[`docs/explanation/tau-as-language.md`](../../explanation/tau-as-language.md)
for the full vision.

Key alignment:

- **`Sandbox` trait + adapter pattern = the "target system."**
  Each adapter is a compilation target; each adapter's
  `supported_shapes()` is the target's capability matrix.
- **`CapabilityShape` enum = the "type system."** Plugin authors
  declare required shapes; adapters declare supported shapes;
  validation is type-checking.
- **Layer 3 startup validation = the "type checker."** Detects
  type-incompatible plans (required ⊄ supported) before runtime.
- **Future `tau check` (Phase 2 sub-project A)** standalonizes
  Layer 3 validation as a CLI verb.
- **Future `tau build --target <triple>` (Phase 2 sub-project C)**
  produces a deployment artifact pinning the target triple +
  resolved capabilities + content hashes.
- **Future remote target backends (Phase 2 sub-project F)** — the
  hexagonal architecture admits Vercel Sandbox, Sandcastle,
  generic remote-execution providers without architectural change.

ADR-0014 will lock the architecture; future sub-projects build on
it without rework.

## Sub-project deliverables checklist

| Deliverable | Purpose |
|---|---|
| ADR-0014 | Lock the 6 design decisions + explicit Vision section. |
| `tau_ports::Sandbox` refinement | Drop PROVISIONAL; add `probe()`, `supported_shapes()`, `validate_plan()`, `wrap_spawn()`. |
| `tau_domain::CapabilityShape` enum | Typed feature identifiers. |
| `Capability::required_shape()` helper | Mapping from variant + fields to required shape. |
| `tau-sandbox-native` workspace crate | Linux landlock + seccomp + namespaces; Light + Strict tiers. |
| `tau-sandbox-container` workspace crate | Docker / podman wrapper. |
| `tau-sandbox-mock` workspace crate | Test adapter (refactored from existing fixture). |
| `tau-runtime/src/sandbox/` module | Chain probe, plan build, validation, plugin_host integration. |
| Lockfile schema v3 → v4 | `LockedPlugin.required_shapes` field. |
| `<scope>/.tau/config.toml` `[sandbox]` schema | User/scope config for adapter chain. |
| `tau resolve --check-sandbox` advisory mode | Pre-flight Layer 3 validation as advisory CLI mode. |
| `docs/explanation/tau-as-language.md` | Vision document for tau as a compiled language for agentic workflows. |
| ROADMAP "Phase 2" stub | List Phase 2 sub-projects A-G as the language-vision delivery path. |

## Task outline (~12-14 tasks)

The implementation plan (next step) will derive ~12-14 tasks.
Likely structure:

1. `CapabilityShape` enum in tau-domain + `Capability::required_shape()` helper + tests.
2. `Sandbox` trait refinement in tau-ports + new error variants + mock adapter migration.
3. `tau-sandbox-native` crate skeleton + probe + Light tier (landlock filesystem only).
4. `tau-sandbox-native` Strict tier (add seccomp + namespaces).
5. `tau-sandbox-native` per-host network filtering (nftables wiring) + per-command exec gating.
6. `tau-sandbox-container` crate (probe + Docker/podman invocation builder + capability realization).
7. `tau-runtime/src/sandbox/chain.rs` (config loading + probe + selection precedence).
8. `tau-runtime/src/sandbox/plan.rs` + `validation.rs` (Layer 3 cross-check).
9. `plugin_host` integration: `wrap_spawn` call site + lockfile schema bump.
10. `tau resolve --check-sandbox` advisory mode.
11. End-to-end integration tests (Linux native + container).
12. PAUSE — gate. Final verification + open PR.
13. PAUSE — gate. ADR-0014 (full body) + Vision section + ROADMAP Phase 2 stub + squash merge after CI green.
14. (Optional) Documentation polish + `docs/explanation/tau-as-language.md` cross-references.

## References

- ADR-0006 — runtime architecture, capability model, NG6 (no
  persistent agent memory in core), v0.1 unsandboxed posture.
- ADR-0006 §13 — sandbox port reservation.
- ADR-0007 §7 — three-bucket exit code policy reused for
  sandbox errors.
- ADR-0009 — typed-error policy.
- ADR-0011 — `FatalError` runtime error pattern reused for
  `SandboxViolation`.
- ADR-0012 — source-agnostic verify primitive (precedent for
  hexagonal architecture in tau-pkg).
- ADR-0013 — REPL persistence (precedent for hexagonal port +
  adapter pattern in tau-cli).
- `crates/tau-ports/src/sandbox.rs` — existing PROVISIONAL trait.
- `crates/tau-ports/src/fixtures.rs::MockSandbox` — existing
  mock to be refactored.
- `crates/tau-runtime/src/plugin_host/` — plugin spawn pipeline.
- `crates/tau-pkg/src/lockfile.rs` — lockfile schema (v3 from
  priority 7).
- `docs/explanation/tau-as-language.md` — long-term vision doc.
