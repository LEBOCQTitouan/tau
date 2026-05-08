# ADR-0021: per-plugin container images

**Status:** Accepted (2026-05-08)

**Supersedes:** the implicit "single base image + bind-mounted plugin binary"
approach used by the Container adapter through sub-project H.

## Context

The Container sandbox adapter spawned plugins via `docker run <base-image>
<host-path-to-plugin>`. The host path doesn't exist inside the container, so
exec failed and the plugin host saw EOF before the plugin's handshake could
be sent. Five integration tests in `crates/tau-plugin-compat/tests/layer4_container.rs`
were `#[ignore]`'d — three from sub-project H's HTTP work, two from
sub-project D's earlier fs/shell work — all sharing this symptom.

## Decision

Replace the bind-mount approach with **per-plugin Docker images** built on a
shared `tau-plugin-base` image. Each plugin has its own multi-stage Dockerfile.
The Container adapter resolves the image name as `tau-plugin-<bin>:dev` from
the `Command`'s program path, runs it, and overrides ENTRYPOINT to
`tau-net-bridge` for HTTP plans.

This is **Phase 1** of a four-phase roadmap:

1. **Phase 1 (this ADR):** existing 5 plugins baked into images; CI builds
   via `cargo xtask build-plugin-images` with GHA buildx cache; Debian-slim
   base; host-arch builds only; convention-based image discovery.
2. **Phase 2:** native-deps plugin support (image build infra grows to
   handle `apt-get install`, `pip install`, etc.).
3. **Phase 3:** public plugin SDK / third-party authoring story; manifest
   schema gains an optional `[sandbox.container] image = "..."` override
   field; image conventions become the public contract.
4. **Phase 4:** production-grade distribution — GHCR push, sigstore
   signing, SBOM generation, multi-arch matrix (linux/amd64 + linux/arm64),
   distroless base swap, plugin lockfile pins image digest.

## Consequences

- Container adapter no longer bind-mounts the plugin binary or
  `tau-net-bridge`; both live inside the per-plugin image.
- Plugin builds happen twice during a CI run: once for the host artifact
  pipeline (used by the Native adapter), once inside the Dockerfile builder
  stage. Locked decision 5: optimise only if profiling demands.
- Local dev iteration adds a `cargo xtask build-plugin-images` step before
  container tests run.
- CI pins `TAU_CONTAINER_RUNTIME=docker` (job-level env) so xtask + adapter
  agree on which runtime owns the per-plugin image storage. Local dev
  leaves it unset → both default to podman.
- ADR-0020 (sandbox proxy) unchanged. ADR-0019 (per-host net filter) remains
  superseded.

## Tests closed by this ADR

Of the 5 originally-`#[ignore]`'d Container-adapter integration tests in
`crates/tau-plugin-compat/tests/layer4_container.rs`, **2 are closed**:

- `shell_layer4_container_runs_echo_hello`
- `fs_read_layer4_container_reads_data_file`

The 3 HTTP-cassette tests (`anthropic_*`, `ollama_*`, `openai_*`) remain
`#[ignore]`'d. This PR does extend `tau-sandbox-proxy` with plain-HTTP
forwarding (alongside the existing CONNECT path) and sets
`HTTP_PROXY`/`HTTPS_PROXY` (uppercase + lowercase) on the container,
which made the 3 tests pass locally on macOS Apple Silicon Podman. But
the same code fails on Linux Docker `--network bridge` in CI: Podman's
slirp4netns rootless networking lets container `127.0.0.1` reach the
host's `127.0.0.1` directly (bypassing the proxy), while Docker's bridge
network does not. The "local pass" was for the wrong reason.

Sub-project J needs to either:

1. Make the cassette URL reachable from the container without bypassing
   the proxy: explicit `--add-host=host.docker.internal:host-gateway`
   on Linux + a cassette URL rewrite hook so the plugin gets a hostname
   that resolves to the host gateway.
2. Or: investigate why `reqwest` doesn't appear to use `HTTP_PROXY` for
   loopback URLs in the Docker case (possibly implicit no-proxy filtering
   for `127.0.0.1`/`localhost` even when `HTTP_PROXY` is set).
3. Or: serve the cassette over HTTPS with a self-signed cert + a test-only
   plugin trust override; the existing CONNECT path then handles it.

Each path has its own tradeoffs; sub-project J's brainstorm picks one.

See `docs/superpowers/specs/2026-05-08-per-plugin-images-design.md` for the
full design including locked decisions 1-8 and Phase 1 risks.
