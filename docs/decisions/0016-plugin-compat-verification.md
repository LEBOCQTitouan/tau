# ADR-0016: Plugin compatibility verification — Layer 2 install-time cross-check + per-plugin harness

**Status:** Accepted
**Date:** 2026-05-04
**Deciders:** Titouan Lebocq
**Supersedes:** —
**Closes:**
- Sub-project B from [the sandboxing follow-ups](../superpowers/specs/2026-05-03-sandboxing-followups.md) — the 5 real shipped plugins now declare `[sandbox] required_tier = "strict"`, install carries Layer 2 cross-check at step 8.7, and a verification harness exists in `tau-plugin-compat`.
- The foundation portion of sub-project D (controlled-environment test binary + landlock-symlink fix in `tau-sandbox-native::light::resolve_symlinks_for_landlock`) — D's remainder (re-introducing 5 e2e test files) stays as a separate sub-project.
**Amends:**
- [ADR-0015](0015-sandbox-activation.md) Decision 6 — plugin manifests can now declare `[sandbox] required_tier`. The 5 real plugins (`anthropic`, `ollama`, `openai`, `fs-read`, `shell`) declare `Strict`. Toy plugins (`echo-llm`, `echo-tool`) omit the block.
**Refines:**
- [ADR-0008](0008-plugin-loading.md) — install lifecycle gains step 8.7 between source SHA-256 (8.6) and lockfile write (9). On cross-check failure, install aborts with `InstallError::CrossCheck { message }`; user retries via `tau install --force` after fixing the manifest.

## Context

Sub-project A ([ADR-0015](0015-sandbox-activation.md)) shipped sandboxing on by default at commit `7fe6cfb` on 2026-05-04. Every plugin spawn now flows through `resolve_adapter` against the static adapter registry. That work proved 250+ unit tests pass; the workspace compiles cleanly across Linux/macOS/Windows × stable/1.91.

What sub-project A's CI did **not** prove: that the 5 real shipped plugins actually work end-to-end when sandbox enforcement is engaged. Activation is theoretical until verified against the real plugin surface.

Two further gaps:

1. **Layer 2 install-time cross-check is unimplemented.** The kernel host already issues `tool.describe_capabilities` calls per tool method (priority 12 work) — but the responses are advisory, not validated against the manifest. A buggy or compromised plugin binary can claim capabilities its manifest doesn't declare; the kernel's capability check uses the manifest, so the binary's claims silently expand the attack surface.

2. **No automated end-to-end plugin verification.** Nothing today exercises "install plugin → resolve adapter → spawn under enforcement → run golden path → assert success" for any of the 5 real plugins. Regressions go undetected until a human runs `tau chat`.

Sub-project D's "end-to-end landlock CI integration" is the third gap (re-introducing 5 e2e test files removed at priority-12 ship). Sub-project B absorbs D's *foundation* (the symlink-resolution fix + a controlled-environment test binary), leaving D's remaining work focused on actually re-introducing those tests using B's foundation.

Seven discrete decisions follow.

## Decision 1 — Layer 2 cross-check is tool-port dynamic; cross-port universal mechanism deferred

**Decision:** the Layer 2 install-time cross-check spawns the plugin binary, performs `meta.handshake`, and for tool-port plugins enumerates `tool.describe_capabilities` per method. The aggregated capability set is compared bidirectionally against the manifest's `[[capabilities]]` block (binary-claims-extra and manifest-declares-unused both hard-fail). For LLM-backend / storage plugins the cross-check returns the manifest's capabilities verbatim with a `tracing::debug!` noting the manifest-only path.

**Context:** alternatives were (A) manifest-only static check (no wire mechanism — defeats the cross-check's purpose) and (C) universal wire-level extension to `HandshakeResponse` (full coverage but adds protocol-level scope across all 7 plugins and the SDK). Option B catches the security-critical case (tool plugins, where capability drift is the prototypical attack vector) without forcing a protocol bump in this sub-project.

**Consequences:**
- Tool plugins' install path now validates: binary's actually-claimed capabilities match the manifest exactly. Drift in either direction aborts install.
- LLM-backend and storage plugins fall through to manifest-only Layer 2; the cross-check still runs (the binary must spawn + handshake successfully) but per-method enumeration doesn't apply because those ports lack the wire mechanism.
- A future hardening pass can add `meta.describe_capabilities` (or extend `HandshakeResponse` with a `capabilities: Vec<Capability>` field) to enable universal cross-port checking. Tracked as a Phase 2 hardening item.

**Alternatives considered:**
- **Manifest-only static check.** Rejected: doesn't catch the actual attack vector (binary diverges from manifest claims).
- **Universal wire-level cross-port mechanism.** Deferred: scope creep into protocol design when the security-critical surface is already covered by the tool-port path.

## Decision 2 — Cross-check timing is install-time only; spawn-time skipped

**Decision:** the cross-check runs once during `tau install` (and `tau update`, which re-installs) at the moment the binary has just finished building. Spawn-time cross-check is not added.

**Context:** alternatives were (B) first-spawn-after-install lazy check (adds state-machine complexity to `LockedPlugin`; doesn't catch drift any earlier than spawn) and (C) install-time + every-spawn (overlaps with `tau verify`'s tree-hash check from priority 7, which is strictly stronger than re-running a capability handshake).

**Consequences:**
- `LockedPlugin.required_shapes` becomes the source of truth for runtime resolution. The runtime trusts the lockfile; the lockfile was verified at install time.
- A binary swap on disk after install is detected by `tau verify`, not by the sandbox layer. This separates concerns cleanly.
- An `InstallOptions::skip_cross_check: bool` field (default `false`) provides an escape hatch for tests that build stub plugin binaries that don't implement the handshake. Production code never sets it, so cross-check always fires there.

**Alternatives considered:**
- **First-spawn-after-install lazy.** Rejected: doesn't catch drift earlier; adds state-machine complexity.
- **Install-time + every-spawn.** Rejected: overlaps with `tau verify`'s tree-hash check, which is strictly stronger than re-running a handshake.

## Decision 3 — Live spawn depth = Layer 3 + container + native; absorb sub-project D's foundation

**Decision:** verification depth covers all three layers: Layer 3 (`tau resolve --check-sandbox`, no real spawn), Layer 4 container (live spawn under Docker on Linux CI), and Layer 4 native (live spawn under landlock + seccomp + namespaces on Linux CI). The controlled-environment test binary and the landlock-symlink fix from sub-project D's foundation are absorbed into B; D's remaining scope (re-introducing 5 e2e test files using the controlled-env binary) stays as a separate sub-project.

**Context:** the alternatives were (A) Layer 3 only (theoretical validation; no kernel verification) and (B) Layer 3 + container only (skips native; punts native verification to a future sub-project). Container coverage alone leaves the native adapter — the only adapter that exercises landlock + seccomp directly — unvalidated end-to-end. Decoupling B from D would create an artificial boundary where the harness exists in one place and the thing it's supposed to verify exists somewhere else.

**Consequences:**
- The plugin compat tests *are* the symlink fix's reproducer.
- B's PR is larger (~16 commits over the implementation tasks) but ships a complete, verified milestone in one merge.
- The native adapter touchup (`tau-sandbox-native/src/light.rs::resolve_symlinks_for_landlock`) lands in B; sub-project D's later PR doesn't have to re-modify the adapter.

**Honest scope notes:**
- Layer 4 container Tier-A tests (shell + fs-read via `tau plugin run --script`) verify build + handshake but do NOT route through the sandbox adapter. Marked `#[ignore]` with rationale because `tau plugin run --script` hardcodes the handshake port to `llm_backend`, incompatible with tool-port plugins. Sub-project D will provide a port-aware driver.
- Layer 4 container Tier-B tests (anthropic, ollama, openai cassette-replay) marked `#[ignore]` pending sub-project D's cassette-replay-through-sandboxed-process infrastructure.
- All 5 Layer 4 native tests marked `#[ignore]` and gated `cfg(target_os = "linux")` for the same reason.
- Layer 3 tests (`tau resolve --check-sandbox`) all pass: 5/5 plugins on Linux CI.

## Decision 4 — `tau install --rehash` dropped from scope

**Decision:** the `--rehash` flag from the original sub-project B scope is removed. Not deferred; dropped.

**Context:** the existing command surface already covers every realistic use case:
- `tau update <pkg>` covers refetch + rebuild + lockfile rewrite.
- `tau install --force <pkg>` covers local-source rebuild without netfetch.
- `tau verify` covers read-only drift detection.
- Auto-upgrade-with-`tracing::warn!` (priority 7 / priority 12 / sub-project A pattern) covers schema migration silently on next read.
- `--rehash` would only uniquely fill the niche "refresh lockfile metadata without rebuilding and without going to the network" — a developer convenience saving ~5 seconds per refresh, with no security-critical use case. YAGNI applies.

**Consequences:** if a real need surfaces post-ship, `--rehash` can be added in a later sub-project with concrete user motivation. Adding flags is a one-way ratchet; surgically removing them after they're documented is harder.

## Decision 5 — All 5 real plugins declare `[sandbox] required_tier = "strict"`

**Decision:** each of `anthropic`, `ollama`, `openai`, `fs-read`, `shell` gets `[sandbox] required_tier = "strict"` in its `tau.toml`. Toy plugins (`echo-llm`, `echo-tool`) omit the block (default is None tier).

**Context:** alternatives were (B) all real plugins declare Light (strictly weaker enforcement with identical host coverage) and (C) plugins declare nothing, project drives entirely (defeats ADR-0015 Decision 6's purpose; trusts every project author to know which plugins handle untrusted data).

**Consequences:**
- macOS / Windows users without Docker get exit 2 on first run with the guided diagnostic from sub-project A: "your project requires Strict; install Docker, switch to Linux, or pass `--no-sandbox` for an explicit opt-out". This is the correct friction.
- Per-run opt-out via `--no-sandbox` keeps dev workflow viable.
- Toy plugins remain useful as tier-None test fixtures.

**Alternatives considered:**
- **Light tier.** Rejected: same host availability as Strict but strictly weaker enforcement (compromised HTTP plugins can still exfiltrate via the network under Light).
- **No declaration; project drives.** Rejected: makes the plugin author transparent to the project, defeating ADR-0015 Decision 6's purpose.

## Decision 6 — New crate `tau-plugin-compat/` for the test harness

**Decision:** create a new workspace crate `crates/tau-plugin-compat/` (`publish = false`) holding the per-plugin verification harness, fixtures, and the controlled-environment test binary.

**Context:** alternatives were (A) centralized in `tau-runtime/tests/` (pollutes runtime crate's test surface; rule-of-three not yet justified at one sub-project's worth of need) and (B) per-plugin alongside existing test surface (duplicates fixture boilerplate 5×).

**Consequences:**
- Branch protection rises from 25 to 27 required checks (2 new CI jobs: `build (tau-plugin-compat)` + `test (tau-plugin-compat / linux)`).
- One GitHub-settings configuration change after first push to add the new checks to branch protection.
- The crate becomes the natural home for cross-plugin compatibility testing across sub-projects D, E, F, J, K.
- `tau-cli` is a dev-dependency on `tau-plugin-compat` so cargo builds the `tau` binary into target/debug for the harness's `tau_bin()` helper to discover. CI explicitly builds `tau-cli` first to ensure the binary exists in the test job's environment.

**Alternatives considered:**
- **Centralize in `tau-runtime/tests/`.** Rejected: pollutes runtime crate's test surface.
- **Per-plugin under `tau-plugins/<name>/tests/`.** Rejected: duplicates fixture boilerplate 5×.

## Decision 7 — Cross-check function lives in `tau-pkg::sandbox_check` (public module)

**Decision:** the `cross_check_plugin_capabilities(binary_path, manifest) -> Result<Vec<CapabilityShape>, CrossCheckError>` function is public on `tau-pkg::sandbox_check`. Both production code (the install path in `tau-pkg`) and test code (the harness in `tau-plugin-compat`) call it directly.

**Context:** alternatives were (A) private to `tau-pkg::install` (makes Layer 2 untestable except through `install_with_options`'s side effects, which is bad for a security feature) and (C) new shared crate `tau-sandbox-check` (premature; one module, one function, one error type doesn't justify a workspace crate).

**Consequences:**
- `tau-plugin-compat` adds a `tau-pkg = { path = "../tau-pkg" }` dev-dependency.
- The public surface is small: one function, one `#[non_exhaustive]` error enum (4 variants).
- `InstallError::CrossCheck { message: String }` (not `#[from] CrossCheckError`) because `tau-domain::Capability` derives `PartialEq` but not `Eq`, and `InstallError` requires `Eq`. The `Display` string preserves all necessary information.
- The cross-check IPC bridge: `tokio::process::Command` + `tau_plugin_protocol::Frame` + `FramedReader`/`FramedWriter`. `cross_check_plugin_capabilities` is async; `install_with_options` is sync. Bridge via a current-thread tokio runtime spun up just for this step.
- `kill_on_drop(true)` on the spawned `Command` ensures the child is reaped on every error-return path. IPC reads / writes are wrapped in `tokio::time::timeout` (10s for handshake response, 5s per per-method `tool.describe_capabilities` response).

**Alternatives considered:**
- **Private to `install_with_options`.** Rejected: makes Layer 2 untestable except via install side effects.
- **New shared crate `tau-sandbox-check`.** Rejected: premature for one module / one function.

## Implementation summary

The 16 commits on `feat/plugin-compat-spec` (PR #24) realize these decisions via:

| Layer | Crate | Files |
|---|---|---|
| Plugin tier declarations | `tau-plugins/<5>` | `<plugin>/tau.toml` |
| Cross-check production module | `tau-pkg` | `src/sandbox_check.rs`, `src/lib.rs`, `Cargo.toml` |
| Install path integration (step 8.7) | `tau-pkg` | `src/install.rs`, `src/error.rs`, `tests/install_cross_check.rs` |
| Native adapter symlink fix | `tau-sandbox-native` | `src/light.rs` |
| Test harness crate | `tau-plugin-compat` (new) | `Cargo.toml`, `src/lib.rs`, `fixtures/`, `tests/layer3_*.rs`, `tests/layer4_*.rs` |
| Controlled-env binary | `tau-plugin-compat` | `fixtures/controlled-env-binary/` (standalone Cargo project, NOT a workspace member) |
| CLI error rendering | `tau-cli` | `src/cmd/error_render.rs` (`render_cross_check_error`), `tests/cmd_install_cross_check_render.rs`, snapshots |
| CI jobs | `.github/workflows/ci.yml` | `build (tau-plugin-compat)` + `test (tau-plugin-compat / linux)` |
| CLAUDE.md cargo conventions | (root) | `CLAUDE.md` (new; documents per-agent `CARGO_TARGET_DIR` to avoid concurrent-build contention) |

Test coverage delta: ~35 new tests across 8 files (8 unit on cross-check + 4 unit on symlink fix + 5 install-path integration + 5 Layer 3 + 5 Layer 4 container + 5 Layer 4 native + 3 snapshot rendering). Of those, the Layer 4 tests (10) are `#[ignore]`'d pending sub-project D's e2e infrastructure.

CI matrix: 27 jobs across Linux + macOS + Windows × stable + 1.91. All green on the final commit.

## Forward links

- **Sub-project D** (end-to-end landlock CI integration) is unblocked by B's foundation: the symlink-resolution fix and controlled-environment binary now exist; D's remaining scope is re-introducing the 5 e2e test files removed at priority-12 ship plus building the port-aware driver that flips the 10 `#[ignore]`'d tests in `tau-plugin-compat/tests/layer4_*.rs`.
- **Sub-project E** (per-command exec gating, landlock V2) gains a verified compat baseline.
- **Sub-project F** (per-host network filtering via nftables-in-netns) gains the same.
- **Sub-projects J, K** (macOS sandbox-exec, Windows AppContainer) extend `tau-plugin-compat` with platform-specific compat suites.
- **Phase 2 sub-project A** (`tau check` standalone command) gains a verified production surface to re-expose; `cross_check_plugin_capabilities` could become callable from `tau check` directly in addition to the install path.
- **Phase 2 hardening** (deferred): universal cross-port `meta.describe_capabilities` wire mechanism extending Decision 1's tool-port-only check to LLM-backend and storage plugins.
