# Per-plugin container images — Phase 1 design

> **Status:** spec, awaiting plan. Sub-project I (Phase 1 of the four-phase
> per-plugin-images roadmap). Branch: `feat/per-plugin-images`. Supersedes the
> "Container-adapter HTTP plugin tests" follow-up tracked in
> [`sandboxing-followups.md`](2026-05-03-sandboxing-followups.md), and also
> closes the older sub-project D leftover (`shell` and `fs-read` Container
> adapter tests).

## Goal

Un-`#[ignore]` the 5 Container-adapter plugin tests in
`crates/tau-plugin-compat/tests/layer4_container.rs`:

- `anthropic_layer4_container_completes_via_cassette`
- `ollama_layer4_container_completes_via_cassette`
- `openai_layer4_container_completes_via_cassette`
- `shell_layer4_container_runs_echo_hello`
- `fs_read_layer4_container_reads_data_file`

All 5 share the symptom `PluginHandshakeFailed: EOF before handshake response`.
The hypothesis (to be confirmed in T1's debug session) is that the Container
adapter passes the plugin's *host* path (e.g.
`/Users/.../target/release/anthropic-plugin`) into `docker run` as the program
to exec. That path does not exist inside the container; `exec` fails; the
plugin never starts; the plugin host reads EOF on stdin.

Stop bind-mounting the plugin binary from a host path. Instead, bake each
plugin into its own container image (`tau-plugin-<name>:<version>`). The
container adapter resolves the image name from the `LockedPlugin` and runs
that image; the plugin binary lives at a known in-image path
(`/usr/local/bin/<name>`).

## Background

Sub-project H (PR #39, 2026-05-08) replaced sub-project F's veth+nftables
network filter with a userspace HTTP-CONNECT proxy. The proxy work is sound
— `crates/tau-sandbox-native/tests/strict_proxy.rs` integration tests pass on
stock Linux without privileges. The 3 HTTP plugin tests above were also
authored in PR #39 but failed in CI with the EOF symptom and were re-`#[ignore]`'d
pending an interactive Linux debug session.

The 2 non-HTTP tests (`shell`, `fs-read`) have been `#[ignore]`'d since
sub-project D landed with the *identical* symptom and identical reason
("Container's docker-run + binary-mount plumbing needs investigation"). They
predate the proxy work entirely; the bug is not proxy-specific. It's the
Container adapter's binary-mount story that has never worked for arbitrary
plugin binaries.

This sub-project (I) addresses the root cause directly — by removing the
host-path bind-mount and replacing it with a per-plugin image — and uses the
investigation as a forcing function to lay the foundation for the per-plugin
container image roadmap (Phases 1–4 below).

## Investigation (filled in by T1)

> **Placeholder.** T1 (the debug session) writes its findings here before any
> implementation code is committed. If the findings refute the hypothesis,
> the rest of the spec is revised before T2 starts.
>
> Expected to capture: exact `docker run` argv that failed; container stderr
> / `docker logs`; the actual error message from the failed exec; whether the
> root cause is missing-binary, libc mismatch, stdio plumbing, fork
> semantics, or something else.

## Locked decisions

| # | Decision | Future-phase swap |
|---|---|---|
| 1 | **Hybrid model**: every plugin still builds as a host binary (used by `tau-sandbox-native`) AND as a container image (used by `tau-sandbox-container`). | Phase 4 may revisit and deprecate host-binary builds. |
| 2 | **Per-plugin Dockerfile + shared base image** (`tau-plugin-base`). Each plugin's Dockerfile is small (`FROM` base, `COPY` binary, `ENTRYPOINT`); base hosts `tau-net-bridge` and ca-certificates. | Phase 3 documents this as the public convention. |
| 3 | **Image discovery by convention**: `tau-plugin-<plugin-name>:<plugin-version>`, computed from `LockedPlugin` (`name`, `version`). No manifest schema change in Phase 1. | Phase 3 adds an optional `[sandbox.container] image = "..."` override field. |
| 4 | **CI builds with GHA buildx cache** (`cache-from: type=gha, cache-to: type=gha`). No registry push. | Phase 4 swaps to GHCR push (image hash pinning, signing, SBOMs). |
| 5 | **Multi-stage Docker build** (`FROM rust:1.x AS builder` → `cargo build --release` → `FROM tau-plugin-base` → `COPY --from=builder`). Optimise only if profiling demands. | Phase 2/4 may introduce single-stage `COPY` of pre-built host binary if cold-build cost is intolerable. |
| 6 | **Debian-slim base** (`debian:bookworm-slim`). Easy to debug interactively. | Phase 4 swaps to distroless. |
| 7 | **Host-arch builds only**. Apple Silicon dev → linux/arm64; Linux CI → linux/amd64. Same Dockerfile. No multi-arch tagging. | Phase 4 introduces multi-arch matrix and manifest lists. |
| 8 | **`cargo xtask build-plugin-images [<name>]`** as the build entry point. New `xtask` workspace member. CI calls the same xtask. | Phase 3 may extend xtask with `publish-plugin-image` for SDK users. |

## Architecture

```
HOST                                    CONTAINER (per-plugin image)
─ Plugin host (tau-runtime)             ─ /usr/local/bin/<plugin>      ← baked in plugin image
─ tau-sandbox-container::runner         ─ /usr/local/bin/tau-net-bridge ← baked in tau-plugin-base
   ├─ resolves image: tau-plugin-       
   │  <name>:<version>                  [bind-mounts at run time]
   ├─ spawns proxy task                 ─ /run/tau-proxy.sock          (HTTP plans only)
   └─ docker/podman run ...             ─ FS Read/Write paths (from SandboxPlan)
─ Proxy task on Unix domain socket
```

**Non-HTTP plans.** `docker run tau-plugin-shell:0.1.0 [args]` — image's
`ENTRYPOINT` runs the plugin directly. No bridge involvement.

**HTTP plans.** Adapter passes `--entrypoint=/usr/local/bin/tau-net-bridge`,
plus bridge args (`--proxy-sock=/run/tau-proxy.sock --listen=127.0.0.1:8443
--`), plus the plugin path (`/usr/local/bin/<plugin>`). Bridge wraps the
plugin; `HTTPS_PROXY=http://127.0.0.1:8443` env injected; outbound HTTPS
routes via the bridge to the host's proxy task on the bind-mounted Unix
socket. Same proxy semantics as today.

## Components

### NEW

- **`crates/tau-plugin-base/Dockerfile`** — workspace member containing only a
  Dockerfile (no Rust crate). `FROM debian:bookworm-slim`; installs
  `ca-certificates`; multi-stage builds `tau-net-bridge` from the workspace
  source and copies it to `/usr/local/bin/`; creates a non-root `tau` user.
  Tag: `tau-plugin-base:<workspace-version>`.

- **`crates/tau-plugins/<name>/Dockerfile`** — one per plugin (5 total).
  Multi-stage: `FROM rust:1.x AS builder` → COPY workspace source → `cargo
  build --release -p tau-plugins-<name>` → `FROM tau-plugin-base` →
  `COPY --from=builder ... ENTRYPOINT ["/usr/local/bin/<name>"]`.

- **`xtask/` crate** (workspace member). Subcommands:
  - `build-base-image` — builds `tau-plugin-base`.
  - `build-plugin-images [--name <name>]` — builds the base if missing, then
    builds each plugin image (or just the named one).
  - Auto-detects runtime via tau's existing `ContainerRuntime::Auto` probe
    (Podman first, Docker fallback — PR #40 default). Errors helpfully if
    neither is on PATH.

### MODIFIED

- **`tau-sandbox-container::runner::wrap_command`**:
  - Image name resolution from `LockedPlugin` (`name`+`version`) by convention.
  - Drop host-path bind-mount of the plugin binary (no longer needed).
  - Drop the `tau-net-bridge` bind-mount (baked into base image now).
  - For HTTP plans: set `--entrypoint=/usr/local/bin/tau-net-bridge` + bridge
    args + plugin path.
  - For non-HTTP plans: rely on the image's own `ENTRYPOINT`; no override.

- **`tau-sandbox-container::runner` unit tests** — ~5 tests assert on argv
  shape (`-v <bridge>:/usr/local/bin/tau-net-bridge:ro`,
  `DEFAULT_BASE_IMAGE` constant, plugin path as last argv); all need
  updating to the new shape.

- **`tau-plugin-compat/tests/layer4_container.rs`**:
  - Un-`#[ignore]` 5 tests.
  - Add an `image_present_or_skip(name, version)` helper that runs `docker
    image inspect` (or `podman image inspect`) and skips with a helpful
    message ("run `cargo xtask build-plugin-images <name>` first") if
    missing. Same skip pattern as the existing `require_docker()`.

- **`.github/workflows/ci.yml`** — in the `test (tau-plugin-compat / linux)`
  job: a new step before the cargo invocation that runs `cargo xtask
  build-plugin-images` with `docker/setup-buildx-action@v3` configured for
  GHA cache (`cache-from: type=gha`, `cache-to: type=gha,mode=max`).

- **Workspace `Cargo.toml`** — add `xtask` and `tau-plugin-base` to
  `members`. (Note: `tau-plugin-base` is a Cargo-package wrapper around the
  Dockerfile — needed to make the workspace `cargo` commands ignore it
  cleanly. Alternative: keep the Dockerfile under `infra/` outside the
  workspace and have xtask reference it by path; decide during T2.)

### DELETED

- `DEFAULT_BASE_IMAGE` const in `tau-sandbox-container::runner`
  (`ghcr.io/tau-runtime/sandbox-base:v0.1` — never published).
- `ProxyConfig::bridge_path` field.
- `TAU_NET_BRIDGE_PATH` env-var lookup.
- Argv push of `-v <host_bridge>:/usr/local/bin/tau-net-bridge:ro`.

### UNCHANGED

- `tau-sandbox-proxy` crate (proxy task on the host).
- `tau-net-bridge` binary source (just lives in a different artefact path).
- `tau-sandbox-native` adapter (hybrid model: host binary still works).
- Plugin Rust manifests (`Cargo.toml`, `tau.toml`) — convention-based
  discovery, no schema change.
- The proxy Unix-socket bind-mount.

## Risks & mitigations

| Risk | Mitigation |
|---|---|
| Debug session refutes the "missing binary in container" hypothesis | Debug is **task 1**; hard gate before any code commit. Spec is revised if hypothesis falls. |
| `cargo build` inside Docker too slow even with buildx cache → CI test timeout | Q5 already deferred optimisation; profile during T7 (CI integration). If blocking, fall back to single-stage `COPY` of pre-built host binary for amd64-only. |
| Apple Silicon Podman vs. Linux Docker semantic differences | PR #40 made Podman the Auto default; explicitly test both runtimes during T2-T3 to surface differences early. |
| Bridge baked into base image arch mismatch (host-built bridge ≠ base-image arch) | Build the bridge **inside** `tau-plugin-base`'s Dockerfile via a multi-stage builder, not COPY'd from host. |
| `rust-toolchain.toml` not honoured inside Docker builder stage | Builder stage installs rustup + reads `rust-toolchain.toml` from the COPYed source tree. |
| Plugin crate name `tau-plugins-<name>` doesn't match `LockedPlugin.name` | Verified during T1 debug session. If mismatched: manifest field is the source of truth; the convention is documented, mapping noted in code. |

## Open questions

Resolved during implementation, not before:

- Exact buildx cache scoping in GHA (`key` template) — pattern-match an
  existing buildx-using workflow or follow the `docker/build-push-action`
  README; standard.
- Whether `Cargo.lock` and `rust-toolchain.toml` need to be `COPY`ed into
  the builder stage before `cargo build` for deterministic builds — yes,
  standard cargo-in-docker practice.
- `tau-plugin-base` as workspace member or `infra/`-external — decide during
  T2 based on what `cargo` does with a Dockerfile-only directory.

## Success criteria

- All 5 previously-`#[ignore]`'d Container-adapter tests pass, both locally
  on Apple Silicon (Podman) and in Linux CI (Docker).
- No regressions in `layer4_native.rs` or `strict_proxy.rs`.
- Two follow-up gap rows close in `sandboxing-followups.md` (sub-project D
  leftover + sub-project H leftover).
- ADR-0021 documents the per-plugin-image decision and the four-phase
  roadmap.
- New `xtask` invocation runs cleanly under both Podman and Docker.

## Out of scope (Phase 1)

- **Native-deps plugins** (Phase 2). Image build infra grows to handle
  `apt-get install`, `pip install`, etc. when the first plugin with
  non-Rust deps lands.
- **Public plugin SDK / third-party authoring** (Phase 3). Manifest schema
  override field; plugin-authoring guide; image conventions become the
  public contract; plugin install becomes "pull image".
- **Production-grade distribution** (Phase 4). GHCR push, sigstore signing,
  SBOM generation, multi-arch matrix (linux/amd64 + linux/arm64),
  distroless base image swap, plugin lockfile pins image digest, image-only
  deployment story.

Each of Phases 2–4 is its own future sub-project with its own spec and plan.
