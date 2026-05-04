# Plugin compatibility verification — design

**Date:** 2026-05-04
**Status:** Accepted
**Branch:** `feat/plugin-compat-spec`
**Predecessors:**
- [ADR-0014](../../decisions/0014-sandboxing.md) — sandboxing infrastructure
- [ADR-0015](../../decisions/0015-sandbox-activation.md) — sandbox activation by default
- [Followups doc](2026-05-03-sandboxing-followups.md) — sub-project B (this work) and sub-project D (foundation absorbed here)

**Audience:** the implementer of sub-project B and any reviewer of the resulting plan + PR.

## Context

Sub-project A (sandbox activation by default) merged at `7fe6cfb` on 2026-05-04 (PR #23). Sandboxing is now ON by default for every plugin spawn. Five real plugins ship today: `anthropic`, `ollama`, `openai`, `fs-read`, `shell` (plus two toy plugins, `echo-llm` and `echo-tool`, used for plugin-loader testing).

What sub-project A's CI proved: 250+ unit tests pass; the workspace compiles cleanly across Linux/macOS/Windows × stable/1.91; the resolver correctly picks adapters from a static registry. **What it did not prove: that any of the 5 real plugins actually work end-to-end when sandbox enforcement is engaged.** Activation is theoretical until verified against the real plugin surface.

Two gaps need to close:

1. **Layer 2 install-time cross-check.** Today, the kernel host already issues `tool.describe_capabilities` calls per tool method (priority 12 work) — but the responses are advisory, not validated. A malicious or buggy plugin binary can claim capabilities its manifest doesn't declare; the kernel's capability check uses the manifest, so the binary's claims silently expand the attack surface. A Layer 2 cross-check at install time refuses installation when the binary's surface doesn't match the manifest.

2. **End-to-end plugin verification.** No automated test today exercises the full pipeline of "install plugin → resolve adapter → spawn under enforcement → run golden path → assert success" for any of the 5 real plugins. A regression in any plugin or any adapter goes undetected until a human runs `tau chat` and notices.

Sub-project B closes both gaps.

A third gap — sub-project D's "end-to-end landlock CI integration" — must be addressed simultaneously to deliver native-tier verification: the priority-12 ship had to remove 5 e2e test files because Ubuntu CI's `/bin → /usr/bin` symlinks combined with landlock V1 path resolution caused EACCES on real binary spawns. Sub-project B absorbs the controlled-environment-test-binary + landlock-symlink-fix portion of D's foundation (the rest of D — re-introducing the 5 removed test files — stays as a separate sub-project).

## Goal

Deliver a verified-end-to-end answer to "do the 5 real plugins work under sandbox enforcement?" with both Layer 2 (install-time cross-check) and Layer 4 (live spawn under both container and native adapters) coverage. Establish a foundation (`tau-plugin-compat` crate) that future sandbox sub-projects (E, F, J, K) can extend.

## Design decisions

### Decision 1 — Layer 2 = tool-port dynamic check (option B from brainstorm)

**Decision:** Layer 2 cross-check spawns the plugin binary, performs the handshake, and for tool-port plugins enumerates `tool.describe_capabilities` per method. The aggregated capability set is compared against the manifest's `[[capabilities]]` block. For LLM-backend / storage plugins, no wire-level capability description exists today; the cross-check returns the manifest's declared capabilities verbatim and logs a `tracing::debug!` noting the manifest-only path.

**Context:** the alternatives were (A) manifest-only static check (no wire mechanism — defeats the cross-check's purpose) and (C) universal wire-level extension to `HandshakeResponse` (full coverage but adds protocol-level scope across all 7 plugins and the SDK). Option B catches the security-critical case (tool plugins, where capability drift is the prototypical attack vector) without forcing a protocol bump in the same sub-project.

**Future work (deferred from B):** option C is tracked as a Phase 2 hardening pass once the bundle format is stable. It would extend `HandshakeResponse` with a `capabilities: Vec<Capability>` field; every plugin port returns its full capability set; Layer 2 becomes uniform across ports.

### Decision 2 — Cross-check timing = install-time only (option A from brainstorm)

**Decision:** the cross-check runs once during `tau install` (and `tau update`, which re-installs) at the moment the binary has just finished building. Spawn-time cross-check is not added.

**Context:** alternatives were (B) first-spawn-after-install lazy check (adds state-machine complexity to `LockedPlugin`; doesn't catch drift any earlier than spawn) and (C) install-time + every-spawn (overlaps with `tau verify`'s tree-hash check from priority 7, which is strictly stronger than re-running a capability handshake).

**Consequences:**
- `LockedPlugin.required_shapes` becomes the source of truth for runtime resolution. The runtime trusts the lockfile; the lockfile was verified at install time.
- A binary swap on disk after install is detected by `tau verify`, not by the sandbox layer. This separates concerns cleanly.
- Spawn-time cross-check remains an additive change later if a real need surfaces.

### Decision 3 — Live spawn depth = Layer 3 + container + native (option C from brainstorm)

**Decision:** verification depth covers all three layers: Layer 3 (`tau resolve --check-sandbox`, no real spawn), Layer 4 container (live spawn under Docker on Linux CI), and Layer 4 native (live spawn under landlock + seccomp + namespaces on Linux CI).

**Context:** the alternatives were (A) Layer 3 only (theoretical validation; no kernel verification) and (B) Layer 3 + container only (skips native; punts native verification to a future sub-project). Container coverage alone leaves the native adapter — the only adapter that exercises landlock + seccomp directly — unvalidated end-to-end.

**Consequences:** sub-project B couples to sub-project D's landlock-symlink resolution. See Decision 3a.

### Decision 3a — Resolve B-to-D coupling = absorb D's foundation into B (option D1 from brainstorm)

**Decision:** the controlled-environment test binary and the landlock-symlink fix from sub-project D's foundation move into B. Sub-project B grows from ~1 week to ~2 weeks; ships as one PR. Sub-project D's remaining scope (re-introducing the 5 removed e2e test files using the controlled-env binary now established by B) stays as a separate sub-project.

**Context:** alternatives were (D2) ship D first as a separate sub-project then return to B (one-week gap where activation is shipped but plugins are not verified), and (D3) ship B with native tests `#[ignore]`'d until D unblocks them later (write-only paper deliverable; bit-rot risk on the ignored tests).

**Consequences:**
- The plugin compat tests *are* the symlink fix's reproducer — they exercise the exact path that was broken in priority 12. Splitting B and D would create an artificial boundary where the harness exists in one place and the thing it's supposed to verify exists somewhere else.
- B's PR is larger (~2 weeks of work, ~25 commits) but ships a complete, verified milestone in one merge.
- The native adapter touchup (`tau-sandbox-native/src/light.rs::resolve_symlinks_for_landlock`) lands in B; D's later PR doesn't have to re-modify the adapter.

### Decision 4 — `tau install --rehash` dropped from scope

**Decision:** the `--rehash` flag from the original followups doc is removed from sub-project B's scope. It is not deferred; it is dropped.

**Context:** the existing command surface already covers every realistic use case:
- `tau update <pkg>` covers refetch + rebuild + lockfile rewrite.
- `tau install --force <pkg>` covers local-source rebuild without netfetch.
- `tau verify` covers read-only drift detection.
- Auto-upgrade-with-warn (priority 7 / priority 12 / sub-project A pattern) covers schema migration silently on next read.
- `--rehash` would only uniquely fill the niche "refresh lockfile metadata without rebuilding and without going to the network" — a developer convenience saving ~5 seconds per refresh, with no security-critical use case. YAGNI applies.

**Consequences:** if a real need surfaces post-ship, `--rehash` can be added in a later sub-project with concrete user motivation in hand. Adding flags is a one-way ratchet; surgically removing them after they're documented is harder.

### Decision 5 — All 5 real plugins declare `[sandbox] required_tier = "strict"` (option A from brainstorm)

**Decision:** each of `anthropic`, `ollama`, `openai`, `fs-read`, `shell` gets `[sandbox] required_tier = "strict"` in its `tau.toml`. Toy plugins (`echo-llm`, `echo-tool`) omit the block (default is None tier).

**Context:** the alternatives were (B) all real plugins declare Light (strictly weaker enforcement with identical host coverage) and (C) plugins declare nothing, project drives entirely (defeats ADR-0015 Decision 6's purpose; trusts every project author to know which plugins handle untrusted data).

**Consequences:**
- macOS/Windows users without Docker get exit 2 on first run with the guided diagnostic from sub-project A: "your project requires Strict; install Docker, switch to Linux, or pass `--no-sandbox` for an explicit opt-out". This is the correct friction.
- Per-run opt-out via `--no-sandbox` keeps dev workflow viable.
- Toy plugins remain useful as tier-None test fixtures.

### Decision 6 — New crate `tau-plugin-compat/` for the test harness (option C from brainstorm)

**Decision:** create a new workspace crate `crates/tau-plugin-compat/` holding the per-plugin verification harness, fixtures, and the controlled-environment test binary. The crate is `publish = false` (test infrastructure, not for crates.io).

**Context:** alternatives were (A) centralized in `tau-runtime/tests/` (pollutes runtime crate's test surface; rule-of-three not yet justified at one sub-project's worth of need) and (B) per-plugin alongside existing test surface (duplicates fixture boilerplate 5×). Option C accepts the workspace surface cost upfront in exchange for clean separation of concerns; future sub-projects (E, F, J, K) extend this crate rather than create competing harnesses.

**Consequences:**
- Branch protection rises from 25 to ~28-29 required checks (~3-4 new CI jobs: build on multiple platforms × toolchains, plus Linux-only test job).
- One GitHub-settings configuration change after first push to add the new checks to branch protection.
- The crate becomes the natural home for cross-plugin compatibility testing across sub-projects.

### Decision 7 — Cross-check fn lives in `tau-pkg::sandbox_check` (public module, option B from brainstorm)

**Decision:** the `cross_check_plugin_capabilities` function is public on `tau-pkg::sandbox_check`. Both production code (the install path in `tau-pkg`) and test code (the harness in `tau-plugin-compat`) call it directly.

**Context:** alternatives were (A) private to `tau-pkg::install` (makes Layer 2 untestable except through `install_with_options`'s side effects, which is bad for a security feature) and (C) new shared crate `tau-sandbox-check` (premature; one module, one function, one error type doesn't justify a workspace crate).

**Consequences:**
- `tau-plugin-compat` adds a `tau-pkg = { path = "../tau-pkg" }` dev-dependency.
- The public surface is small: one function, one `#[non_exhaustive]` error enum.
- Future option C (universal wire-level CAPABILITIES) would expand the same module; no new crate needed at that point either.

## Architecture

The work splits cleanly across three layers:

| Layer | Crate | Files modified or created |
|---|---|---|
| Production code | `tau-pkg` | new `src/sandbox_check.rs` (~150 LOC); modified `src/install.rs` (+30 LOC at step 8.5) |
| Native adapter touchup | `tau-sandbox-native` | modified `src/light.rs` (+30 LOC for symlink resolution; +1 error variant) |
| Test infrastructure | `tau-plugin-compat` (new) | full new crate: ~400 LOC of fixtures + drivers + harness; 20 integration tests |
| CLI integration | `tau-cli` | new `src/cmd/error_render::render_cross_check_error` (~50 LOC) + 3 insta snapshots |
| Plugin manifests | `tau-plugins/<5>/tau.toml` | each gets a 3-line `[sandbox] required_tier = "strict"` block |
| Workspace | root `Cargo.toml` + `.github/workflows/ci.yml` | add `tau-plugin-compat` to members; add new CI jobs |

Total new test count: **~35** (8 unit on cross-check + 4 unit on symlink fix + 5 install-path integration + 5 Layer 3 + 5 container + 5 native + 3 snapshot rendering).

## Components

### `tau-pkg::sandbox_check` (new module)

```rust
// crates/tau-pkg/src/sandbox_check.rs

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum CrossCheckError {
    #[error("plugin spawn failed: {0}")]
    SpawnFailed(String),
    #[error("plugin handshake failed: {0}")]
    HandshakeFailed(String),
    #[error("plugin '{plugin}' declares capability {claimed:?} via tool.describe_capabilities but manifest does not include it")]
    BinaryClaimsExtra { plugin: String, claimed: Capability },
    #[error("manifest of '{plugin}' declares capability {declared:?} but binary does not request it")]
    ManifestDeclaresUnused { plugin: String, declared: Capability },
}

pub async fn cross_check_plugin_capabilities(
    binary_path: &Path,
    manifest: &PackageManifest,
) -> Result<Vec<CapabilityShape>, CrossCheckError>;
```

Behavior per port:
- **Tool plugins:** spawn → handshake → enumerate `tool.describe_capabilities` for every method in `HandshakeResponse.methods` → union all results → compare against `manifest.capabilities` (both directions; both extra-in-binary and extra-in-manifest are hard fails).
- **LLM-backend / storage plugins:** spawn → handshake → return `manifest.capabilities` verbatim. Logs `tracing::debug!("port-X cross-check is manifest-only until wire mechanism lands")`.
- **All ports:** returns the resolved `Vec<CapabilityShape>` for the install path to write into `LockedPlugin.required_shapes`.

### `tau-pkg::install` (modified)

The 10-step lifecycle from priority 7 / sub-project A gains a step 8.5:

```
Step 8 (existing): build the binary
Step 8.5 (new):    cross-check via sandbox_check::cross_check_plugin_capabilities
                   — On Err, abort install; binary is left on disk (existing pattern,
                     user retries via `tau install --force`).
                   — On Ok(shapes), pass shapes into Step 9.
Step 9 (existing): write LockedPlugin to lockfile, with required_shapes = shapes from 8.5
```

`InstallError` gains a `CrossCheck(#[from] CrossCheckError)` variant.

### `tau-sandbox-native::light` (modified)

```rust
fn resolve_symlinks_for_landlock(path: &Path) -> Result<Vec<PathBuf>, LightError> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| LightError::SymlinkResolution {
            path: path.to_path_buf(),
            source: e,
        })?;

    if canonical == path {
        Ok(vec![path.to_path_buf()])
    } else {
        Ok(vec![path.to_path_buf(), canonical])
    }
}
```

Called from `apply_landlock`'s path-collection step. Both the symlink path AND the canonical target are added to the landlock ruleset, fixing the priority-12 `/bin → /usr/bin` Ubuntu issue.

`LightError` gains a `SymlinkResolution { path, source }` variant.

### `tau-plugin-compat` (new crate)

```
crates/tau-plugin-compat/
├── Cargo.toml                               # publish = false
├── src/lib.rs                               # fixture helpers, driver functions
├── fixtures/
│   ├── controlled-env-binary/               # controlled-environment test binary
│   │   ├── Cargo.toml
│   │   └── src/main.rs                      # ~50 LOC, predictable I/O
│   └── projects/                            # per-plugin tau.toml fixtures
│       ├── anthropic/  ollama/  openai/
│       ├── fs-read/    shell/
└── tests/
    ├── layer3_check_sandbox.rs              # 5 tests
    ├── layer4_container.rs                  # 5 tests
    └── layer4_native.rs                     # 5 tests
```

Cargo.toml deps: `tau-pkg`, `tau-domain`, `tau-runtime`, `tau-sandbox-native`, `tau-sandbox-container`, `assert_cmd`, `tempfile`, plus dev-deps for fixtures.

### Plugin manifest updates

Each of `anthropic/tau.toml`, `ollama/tau.toml`, `openai/tau.toml`, `fs-read/tau.toml`, `shell/tau.toml` gets:

```toml
[sandbox]
required_tier = "strict"
```

Toy plugins (`echo-llm`, `echo-tool`) unchanged.

### CLI error rendering

`tau-cli/src/cmd/error_render.rs` gains `render_cross_check_error(err: &CrossCheckError) -> String` with multi-line guided output (manifest claims, binary claims, discrepancy list, resolution steps). 3 insta snapshot tests cover the rendering surface.

## Data flow

### Install-time cross-check

```
   tau install <git-url>
           │
           ▼
   tau-pkg::install_with_options
   Steps 1-8: clone, parse manifest, build (existing)
           │
           ▼
   Step 8.5 (NEW): Layer 2 cross-check
   ───────────────────────────────────
   1. spawn binary as child process
   2. send meta.handshake
   3. await HandshakeResponse
      ├── port = Tool   → goto 4
      ├── port = Llm    → return manifest.capabilities verbatim
      └── port = Storage → same as Llm
   4. for each method in response.methods:
      send tool.describe_capabilities
   5. union all per-method capability sets
   6. set_diff(binary, manifest):
      ├── extra in binary  → BinaryClaimsExtra (abort install)
      ├── extra in manifest → ManifestDeclaresUnused (abort install)
      └── match           → return Vec<CapabilityShape>
   7. send meta.shutdown notification
           │
       Result?
       ┌───┴────┐
      Ok       Err
       │        │
       ▼        ▼
   continue   abort install (exit 2);
   to Step 9  binary on disk; user retries
       │      via `--force` after fixing manifest
       ▼
   Step 9: write LockedPlugin, required_shapes = result from 8.5
```

When install succeeds, `LockedPlugin.required_shapes` is the binary's *actually claimed* surface, verified equal to the manifest's *declared* surface. The lockfile becomes the source of truth for runtime resolution.

### Test harness flow (per-plugin compat tests)

```
   cargo test -p tau-plugin-compat --features integration-tests
           │
           ▼
   Per test (e.g. anthropic_native):
   1. tempdir = TempDir::new()
   2. copy fixture project from fixtures/projects/anthropic/ into tempdir
   3. tau install <local plugin path>
      ├── triggers Layer 2 cross-check
      ├── populates LockedPlugin
      └── writes lockfile in tempdir
   4. layer 3 check: tau resolve --check-sandbox; assert exit 0
   5. layer 4 live spawn under {container | native} adapter:
      build + install + drive a golden-path tau-chat (replay-cassette mode);
      assert exit 0 + expected stdout
   6. tempdir dropped → cleanup
```

HTTP plugins (anthropic/ollama/openai) use the existing cassette-replay infrastructure from priority 2 — no real API keys, no real network. fs-read uses a real tempdir; shell runs `echo "hello"`.

### Adapter resolution during compat tests

The adapter is forced via `PluginHostOptions.force_adapter_kind` (sub-project A's surface). `layer4_container.rs` tests force `Container`; `layer4_native.rs` tests force `Native`. The resolver still validates the plan against the chosen adapter (priority 12's Layer 3); the force only bypasses the priority-based pick.

## Error handling

### Cross-check errors at install time

`InstallError::CrossCheck(#[from] CrossCheckError)` propagates to the CLI. Exit code mapping (per ADR-0007 §7's three-bucket policy):

| Variant | Exit code |
|---|---|
| `BinaryClaimsExtra` | 2 (configuration error) |
| `ManifestDeclaresUnused` | 2 |
| `SpawnFailed` | 2 |
| `HandshakeFailed` | 2 |

User-facing rendering goes through `error_render::render_cross_check_error`, producing multi-line output with manifest claims, binary claims, the specific discrepancy, and resolution steps including the exact TOML stanza to add. Insta snapshots cover three variants (BinaryClaimsExtra, ManifestDeclaresUnused, SpawnFailed).

### Cross-check errors during live spawn tests

In `tau-plugin-compat/tests/layer4_*.rs`, a failed cross-check during `tau install` aborts the test with a clear assertion containing the underlying `CrossCheckError` for debugging.

### Distinction: cross-check vs sandbox-enforcement failures

- **Cross-check failure** → manifest/binary mismatch detected before any sandbox enforcement runs → exit 2 from `tau install`.
- **Sandbox enforcement failure** → plugin starts under enforcement, then a syscall is denied or a path is blocked → plugin exits non-zero or kernel SIGSYS-kills it → surfaces as a `tau chat` failure, NOT an install failure.

The two failure modes are tested separately: cross-check in Layer 3 tests, sandbox enforcement in Layer 4 tests.

### Fail-fast vs collect-all-errors

Cross-check errors fail fast (first error stops the cross-check). Layer 3 (`validate_plan_against_adapter` from priority 12) already returns ALL errors per pass — that's the right place for collect-all because Layer 3 errors are about *plan composition* where all errors matter equally. Layer 2 errors are usually one-shot; collecting all errors implies running the whole pipeline twice.

### Symlink-resolution errors

A new `LightError::SymlinkResolution { path, source }` variant surfaces during `apply_landlock` if a path can't be canonicalized. Exit code 2 propagated up through the existing sandbox error chain.

### Exit codes summary

| Failure mode | Surface | Exit code |
|---|---|---|
| Manifest/binary mismatch | `tau install` | 2 |
| Plugin spawn fails during cross-check | `tau install` | 2 |
| Plugin handshake malformed during cross-check | `tau install` | 2 |
| Sandbox resolution fails (project misconfigured) | `tau chat` | 2 |
| Plugin runs but exits non-zero | `tau chat` | propagated tool-error exit |
| Sandbox enforcement kills plugin (SIGSYS) | `tau chat` | propagated as ToolError |
| Symlink resolution fails during apply_landlock | `tau chat` | 2 (propagated up) |

All consistent with ADR-0007 §7's three-bucket policy.

## Testing strategy

### Test inventory

| Layer | File | Tests | Coverage |
|---|---|---|---|
| Unit (cross-check) | `tau-pkg/src/sandbox_check.rs` | ~8 | spawn-failure, handshake-malformed, tool-port aggregate, llm/storage manifest-only, binary-claims-extra, manifest-declares-unused, port-detection, success path |
| Unit (symlink fix) | `tau-sandbox-native/src/light.rs` | ~4 | non-symlink no-op, symlink resolves to canonical, both paths added to ruleset, missing-path returns SymlinkResolution error |
| Integration (install path) | `tau-pkg/tests/install_cross_check.rs` | ~5 | install with matching manifest succeeds, mismatch aborts, --force after fix succeeds, lockfile required_shapes populated, llm-port path manifest-only |
| Layer 3 (per-plugin) | `tau-plugin-compat/tests/layer3_check_sandbox.rs` | 5 | one per real plugin: install + check-sandbox passes |
| Layer 4 container | `tau-plugin-compat/tests/layer4_container.rs` | 5 | one per plugin: live spawn under container, golden path |
| Layer 4 native | `tau-plugin-compat/tests/layer4_native.rs` | 5 | one per plugin: live spawn under native (landlock + seccomp + namespaces), golden path |
| Cross-check error rendering | `tau-cli/tests/cmd_install_cross_check_render.rs` | ~3 | insta snapshots for the multi-line guided output |

Total: **~35 new tests**.

### Test isolation & determinism

- All Layer 3/4 tests use `tempfile::TempDir` for isolated scopes.
- HTTP plugins use cassette-replay (no real API keys, no real network from CI).
- `fs-read` writes a fixture file in tempdir, asserts plugin reads it back.
- `shell` runs `echo "hello"`, asserts output.
- `TAU_TESTING_ALLOW_MOCK_SANDBOX` env-var path is **not used** — these tests use real adapters intentionally.

### CI configuration

New jobs in `.github/workflows/ci.yml`:

```yaml
build (tau-plugin-compat):       # all platforms × stable, 1.91
  steps: cargo build -p tau-plugin-compat --all-features

test (tau-plugin-compat / linux):  # ubuntu-latest only
  steps: cargo test -p tau-plugin-compat --all-targets --features integration-tests
```

Adapter-specific gating:
- `layer4_native.rs` tests are gated `cfg(target_os = "linux")` AND `cfg(feature = "integration-tests")`. Compile on macOS/Windows but test bodies stay empty.
- `layer4_container.rs` tests check Docker availability; skip with a clear assertion message if Docker isn't present.

Branch protection rises from 25 → ~28-29 required checks (one GitHub-settings configuration change after first push).

No new GH Actions infrastructure: Docker is on `ubuntu-latest` out-of-the-box.

### Verification gates per task

Same gates as priority 12 / sub-project A:
1. `cargo fmt --all -- --check`
2. `cargo build --workspace`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `cargo test --workspace --all-targets`
5. `cargo test --workspace --doc`
6. (Linux only) `cargo test -p tau-plugin-compat --features integration-tests`

## Forward links

- **Sub-project D** (end-to-end landlock CI integration) is unblocked by B's symlink-resolution fix and can re-introduce the 5 e2e test files removed at priority-12 ship using B's controlled-environment binary.
- **Sub-project E** (per-command exec gating, landlock V2) gains a verified compat baseline to test against.
- **Sub-project F** (per-host network filtering via nftables-in-netns) gains the same.
- **Sub-projects J, K** (macOS sandbox-exec, Windows AppContainer) extend `tau-plugin-compat` with platform-specific compat suites.
- **Phase 2 sub-project A** (`tau check` standalone command) gains a verified production surface to re-expose; cross-check could become callable from `tau check` directly in addition to the install path.

## Out-of-scope for sub-project B

- Universal wire-level CAPABILITIES (option C from Q1) — Phase 2 hardening.
- `tau install --rehash` flag — dropped per Q4.
- The remaining sub-project D scope (re-introducing 5 e2e test files) — stays in D.
- Any change to the resolver, registry, error renderer surface from sub-project A — unchanged.
- Any change to capability override (priority 4) — unchanged.
- Any change to the `Sandbox` port — unchanged.
