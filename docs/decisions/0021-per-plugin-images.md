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

All 5 originally-`#[ignore]`'d Container-adapter integration tests in
`crates/tau-plugin-compat/tests/layer4_container.rs` are closed:

- `shell_layer4_container_runs_echo_hello`
- `fs_read_layer4_container_reads_data_file`
- `anthropic_layer4_container_completes_via_cassette`
- `ollama_layer4_container_completes_via_cassette`
- `openai_layer4_container_completes_via_cassette`

The 2 non-HTTP tests (`shell`, `fs-read`) close cleanly via the
per-plugin-image fix alone — the plugin binary now lives at a known
in-image path and exec succeeds.

The 3 HTTP cassette tests required two additional changes layered on top:

1. **Plain-HTTP forwarding in `tau-sandbox-proxy`** — the existing CONNECT
   path is HTTPS-only; cassette servers speak plain HTTP. The proxy now
   detects the first request line and dispatches CONNECT or HTTP. The
   HTTP path validates the `Host` against the allowlist, opens TCP, and
   rewrites the request line to RFC 7230 origin-form before splicing.

2. **`--add-host=host.docker.internal:host-gateway` + URL rewrite in
   tests** — Docker `--network bridge` does NOT route container
   `127.0.0.1` to the host's `127.0.0.1` (Podman's slirp4netns does, but
   relying on that means the test bypasses the proxy entirely). The
   stable cross-runtime hostname `host.docker.internal` (Docker 20.10+,
   Podman 4.7+) resolves to the bridge gateway via the magic
   `host-gateway` value. Tests rewrite the cassette URL from
   `http://127.0.0.1:<port>` to `http://host.docker.internal:<port>` and
   include `host.docker.internal` in the plugin's HTTP allowlist.

The plain-HTTP proxy support is the production-relevant change — plugins
talking to local services (Ollama, etc.) now route through the proxy
even for plain HTTP, with allowlist enforcement.

See `docs/superpowers/specs/2026-05-08-per-plugin-images-design.md` for the
full design including locked decisions 1-8 and Phase 1 risks.
