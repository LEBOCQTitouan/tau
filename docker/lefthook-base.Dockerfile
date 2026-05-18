# Base image for the lefthook pre-push deep-gate.
#
# Built lazily by lefthook.yml's pre-push hook, tagged with the
# first 12 hex chars of the Dockerfile's sha256 so any edit triggers
# an automatic rebuild on the next gate run.
#
# Keep in sync with .github/workflows/ci.yml: the FROM image, apt
# packages, nextest version, and MSRV toolchain all mirror what CI
# installs.
#
# To prune old image tags after editing:
#   podman images localhost/tau-lefthook-base --format '{{.ID}} {{.Tag}}'
#   podman rmi localhost/tau-lefthook-base:<old-tag>
FROM docker.io/library/rust:1.82-bookworm

# nftables + iproute2 for the strict-tier network filter exercised
# by integration tests; podman for the DooD nested-container spawn
# used by xtask-plugin-images.
RUN apt-get update -qq \
 && apt-get install -y -qq iproute2 nftables podman \
 && rm -rf /var/lib/apt/lists/*

# cargo-nextest matches CI. Pick the right binary for the host arch.
RUN set -eu; \
    ARCH=$(uname -m); \
    case "$ARCH" in \
      aarch64) URL=https://get.nexte.st/latest/linux-arm ;; \
      *)       URL=https://get.nexte.st/latest/linux ;; \
    esac; \
    curl -LsSf "$URL" | tar zxf - -C /usr/local/cargo/bin

# MSRV pin (must match ci.yml). --no-self-update so rustup itself
# does not change between builds.
RUN rustup toolchain install 1.91 --profile minimal --no-self-update
