# Sub-project J — close the 3 #[ignore]'d HTTP cassette tests

> **Status:** spec, executing inline. Cuts from main at `ae8c21c` (sub-project I PR #41).

## Goal

Close the 3 `#[ignore]`'d Container-adapter HTTP cassette tests deferred from sub-project I:
- `anthropic_layer4_container_completes_via_cassette`
- `ollama_layer4_container_completes_via_cassette`
- `openai_layer4_container_completes_via_cassette`

They pass locally on macOS Apple Silicon Podman (slirp4netns rootless networking lets container `127.0.0.1` reach host `127.0.0.1` directly — bypassing the proxy entirely; "passing for the wrong reason"). They fail in CI Linux Docker `--network bridge` despite the per-plugin image foundation, plain-HTTP proxy support, and `--add-host=host.docker.internal:host-gateway` wiring all shipped in sub-project I.

## Approach: diagnose first, fix second

Two-phase:
- **Phase A — Diagnose.** Wholesale instrument the entire container-sandbox flow with `tracing` events at all 4 layers (proxy, adapter, bridge, plugin host hooks). The 3 failing tests programmatically initialize a `tracing-subscriber` at TRACE level. Run locally against Docker Desktop's Linux VM (matches CI semantics — Docker engine, `--network bridge`, no slirp4netns). Analyze the trace to find the actual failure mechanism.
- **Phase B — Fix.** Implementation depends on Phase A's findings:
  - Cheap fix (1-line tweak in adapter / bridge / proxy) → ship in same PR.
  - Medium fix (test-side reconfiguration, e.g. cassette binds differently) → still same PR.
  - Architectural pivot (host-gateway fundamentally won't work; fall back to HTTPS cassettes per ADR-0021's path 3) → split into a separate sub-project.

## Locked decisions

| # | Decision |
|---|---|
| 1 | Two-phase: diagnose with full tracing, then fix based on findings. |
| 2 | Wholesale instrumentation: proxy + adapter + bridge + plugin host. All 4 layers. |
| 3 | Per-test programmatic `tracing-subscriber` init at TRACE level (no env var changes; no global RUST_LOG override). Scoped to the 3 cassette tests. |
| 4 | Local debug environment: Docker Desktop (already installed; `desktop-linux` context active). FOSS constraint applies to the pre-push gate (per dev-environment ADR), NOT to one-off local debug. |

## Components

### NEW
- Per-test `tracing-subscriber` init in 3 cassette tests in `crates/tau-plugin-compat/tests/layer4_container.rs` (~5 lines each).

### MODIFIED
- `crates/tau-sandbox-proxy/src/lib.rs` — `tracing::debug!` at: accept_loop iteration; handle_connection dispatch; handle_connect (request parsed, validation, SNI peeked, splice); handle_http (request parsed, validation, rewrite, splice).
- `crates/tau-sandbox-container/src/runner.rs::wrap_command` — `tracing::info!` event with image, forwarded env vars, argv, has_http flag.
- `crates/tau-sandbox-native/src/bin/tau-net-bridge.rs` — replace `eprintln!` with `tracing` events. Emit at: parsed args; `bring_lo_up` attempt + result; TCP listener bind; fork+exec lifecycle; per-connection splice; plugin exit.
- `crates/tau-runtime/src/plugin_host/...` — `tracing::warn!` on "EOF before handshake" with structured context (plugin name, sandbox kind, time elapsed). Plus optional `tracing::debug!` for the spawn lifecycle.

### Instrumentation discipline
- `target = "tau::sandbox-container"` / `tau::sandbox-proxy` / `tau::sandbox-native::bridge` etc. — predictable filter targets.
- `tracing::Span`s for connection-scoped state (one span per accepted proxy connection; events nested inside).

## Out of scope
- Extending the lefthook pre-push gate to include integration-tests (needs Podman-in-Podman setup; separate sub-project).
- Production tracing configuration / log shipping.
- Closing other unrelated `#[ignore]`'d tests.

## Verification
- All 3 previously-`#[ignore]`'d HTTP cassette tests pass on Linux CI.
- 2 already-passing tests (`shell`, `fs-read`) still pass.
- No regressions in `layer4_native`, `strict_proxy`, or any other plugin-compat tests.
- Full CI matrix green.
