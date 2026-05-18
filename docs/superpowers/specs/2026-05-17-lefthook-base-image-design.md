# Lefthook deep-gate pre-built base image — design

**Status:** Draft
**Date:** 2026-05-17

## Context

`lefthook.yml`'s `pre-push:deep-gate` boots a fresh ephemeral Podman
container on every run from `docker.io/library/rust:1.82-bookworm`,
then inside that container:

1. `apt-get update -qq && apt-get install -y -qq iproute2 nftables podman` — ~15–40s
2. `curl -LsSf https://get.nexte.st/... | tar zxf - -C /usr/local/cargo/bin` — ~1–5s if missing, instant if present
3. `rustup toolchain install 1.91 --profile minimal --no-self-update` — ~5–15s if missing, ~1s if cached

Total setup overhead: ~20–60s per gate run. Because the container is
`--rm`-ed, these installs are repeated every time the gate runs. The
persistent cargo-cache + target-cache volumes don't help here — they
only cache cargo's downloads and build artifacts, not apt or rustup.

## Goals

1. Eliminate ~20–60s of repeated setup work per `lefthook run pre-push`.
2. Keep CI parity — same toolchain versions, same packages.
3. Make the image self-rebuilding when its inputs change (no manual
   image management).
4. Keep this PR independent of the in-flight parallelization PR
   (#121) — they touch the same heredoc block but each is independently
   valuable on `main`.

## Non-Goals

- Pushing the image to a remote registry. The image is built and stored
  locally; first run on a fresh machine pays a one-time build cost.
- Including a sccache install inside the image. sccache backends (S3,
  Redis, ghac) need credentials we don't want baked in.

## Design

### New file: `docker/lefthook-base.Dockerfile`

```dockerfile
# Base image for the lefthook pre-push deep-gate.
# Built lazily by scripts/lefthook-build-base.sh, tagged by Dockerfile
# SHA-256 so any edit auto-triggers a rebuild on the next gate run.
#
# Keep in sync with lefthook.yml — the FROM image, the apt packages,
# the nextest install, and the MSRV toolchain all mirror what the gate
# previously installed inline.
FROM docker.io/library/rust:1.82-bookworm

# Packages the deep-gate needs: nftables + iproute2 for the strict-tier
# network filter exercised by integration tests; podman for the DooD
# nested-container spawn used by xtask-plugin-images.
RUN apt-get update -qq \
 && apt-get install -y -qq iproute2 nftables podman \
 && rm -rf /var/lib/apt/lists/*

# cargo-nextest matches CI; pick the right binary for the host arch.
RUN set -eu; \
    ARCH=$(uname -m); \
    case "$ARCH" in \
      aarch64) URL=https://get.nexte.st/latest/linux-arm ;; \
      *)       URL=https://get.nexte.st/latest/linux ;; \
    esac; \
    curl -LsSf "$URL" | tar zxf - -C /usr/local/cargo/bin

# MSRV pin (must match ci.yml). `--no-self-update` so rustup itself
# doesn't change between builds.
RUN rustup toolchain install 1.91 --profile minimal --no-self-update
```

### lefthook.yml changes

Before the `podman run` call in `pre-push.deep-gate.run`, add a
hash-tagged lazy build:

```bash
DOCKERFILE=docker/lefthook-base.Dockerfile
TAG="localhost/tau-lefthook-base:$(shasum -a 256 "$DOCKERFILE" | cut -c1-12)"
if ! podman image exists "$TAG"; then
  echo "Building $TAG (first run after Dockerfile change; ~3-5 min)" >&2
  podman build -t "$TAG" -f "$DOCKERFILE" docker/
fi
```

Then change the `podman run` image reference from
`docker.io/library/rust:1.82-bookworm` to `"$TAG"`.

Inside the bash heredoc, replace the apt-get + nextest + rustup setup
block with a comment noting they're pre-installed in the image. The
rustup line inside the msrv-check group also goes away.

### Tagging strategy

Tag = `localhost/tau-lefthook-base:<first 12 hex chars of sha256(Dockerfile)>`.

- Edit Dockerfile → new tag → first gate run after edit rebuilds.
- No edit → existing tag → instant container start.
- `localhost/` prefix marks it as a local-only image; podman won't
  attempt to pull from a registry.

### Disk footprint

The base image is ~2GB on disk (rust:1.82-bookworm is ~1.76GB; our
layers add ~200–300MB for apt packages + nextest + 1.91 toolchain).
This is negligible vs. the existing 63GB cargo-cache + target-cache.

Old images (after Dockerfile edits) are NOT auto-pruned by this PR.
Document a one-liner cleanup in the new Dockerfile's header comment.
We can add an automated prune later if it becomes a problem.

## Risks + mitigations

1. **Stale image masking package updates.** With apt packages frozen in
   the image, security updates for iproute2/nftables/podman don't land
   until the Dockerfile is edited. Acceptable for a local CI mirror.

2. **MSRV bump requires a Dockerfile edit.** Today MSRV bumps already
   require touching ci.yml. We just add one more file to the
   coordinated change. Document this at the top of the Dockerfile.

3. **First-run build time is visible.** ~3–5 min to build the image
   the first time. Mitigated by the `echo "Building ..."` line so the
   user understands the wait isn't a hang.

4. **`podman image exists` not available on older podman.** Available
   since podman 3.0 (early 2021). The CLAUDE.md setup instructs
   `brew install podman`; current Homebrew ships 5.x. Safe.

5. **Hash-tag collision risk negligible** at 12 hex chars (2^48 space)
   for a single-Dockerfile workspace.

## Expected impact

- **Per-run savings:** ~20–60s warm (the entire apt + rustup + nextest
  install block, gone). The exact figure depends on whether nextest
  and rustup-1.91 were already cached.
- **First run on fresh machine:** ~3–5 min slower (one-time image
  build). Acceptable.
- **Composes with PR #121:** orthogonal optimization. After both land,
  the gate has a pre-built image AND parallelized internal stages.

## Test plan

1. **Image builds clean.**
   ```
   shasum -a 256 docker/lefthook-base.Dockerfile
   podman build -t test -f docker/lefthook-base.Dockerfile docker/
   podman run --rm test apt list --installed 2>/dev/null | grep -E '(iproute2|nftables|podman)'
   podman run --rm test cargo nextest --version
   podman run --rm test rustup toolchain list | grep 1.91
   ```
   Expect all four checks to succeed.

2. **Gate uses the new image.** `lefthook run pre-push --force` and
   confirm: (a) the "Building <tag>" message appears on first run,
   (b) no "Building" message on subsequent runs, (c) Stage 0 group
   markers no longer appear (replaced by comment), (d) exit code 0.

3. **Wall-clock A/B.** Measure `time lefthook run pre-push --force`
   before and after (best of 2). Confirm ≥15s improvement on warm cache.

4. **CI parity.** Push to feature branch, confirm all CI jobs still
   green. (CI doesn't use this image; this is purely a local-gate
   optimization.)

## Follow-ups (not in this spec)

- Once PR #121 also lands and this PR lands, the Stage 0 parallel-setup
  block (apt + rustup + nextest) becomes redundant: the apt packages
  are pre-installed, rustup-1.91 is in the image, and cargo-nextest is
  in the image. A small follow-up PR can delete the Stage 0 block
  entirely, simplifying the heredoc.
- Optional: auto-prune old `localhost/tau-lefthook-base:*` tags when a
  new one is built. Probably not worth the complexity for now.
