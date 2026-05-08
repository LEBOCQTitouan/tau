#!/usr/bin/env bash
# scripts/test-linux-integration.sh — run tau-plugin-compat integration tests
# inside a Linux Podman container, with nested Docker access via the host's
# Podman socket. Mirrors what GHA Linux runners do (Linux Docker engine +
# integration-tests feature) so cross-platform networking semantics match.
#
# Why this exists: the macOS Podman default runs containers with
# slirp4netns rootless networking that lets container 127.0.0.1 reach the
# host's 127.0.0.1 directly. CI Linux Docker --network bridge does NOT
# do this. This script reproduces CI's networking semantics locally so
# we can debug the difference without round-tripping through CI.
#
# Prerequisites:
#   brew install podman
#   podman machine init && podman machine start
#
# Usage:
#   scripts/test-linux-integration.sh [<test-filter>]
#
# Examples:
#   scripts/test-linux-integration.sh                    # all integration tests
#   scripts/test-linux-integration.sh layer4_container   # just one file
set -euo pipefail

cd "$(dirname "$0")/.."
WORKSPACE_ROOT="$PWD"
FILTER="${1:-}"

# Use the same image + caches as the lefthook pre-push gate so warm runs
# are fast.
IMAGE="docker.io/library/rust:1.82-bookworm"
CARGO_CACHE_VOLUME="cargo-cache"
TARGET_CACHE_VOLUME="target-lefthook-podman-integration"

# Find the host's Podman socket path so the inner container can reach it
# for nested-container spawns. Apple Silicon Podman exposes it via
# `podman machine inspect`.
SOCK_PATH="$(podman machine inspect --format '{{.ConnectionInfo.PodmanSocket.Path}}' 2>/dev/null || echo '')"
if [[ -z "$SOCK_PATH" ]]; then
  echo "ERROR: cannot locate podman machine socket. Is the machine running?" >&2
  echo "Try: podman machine start" >&2
  exit 1
fi

echo "==> Workspace:        $WORKSPACE_ROOT"
echo "==> Inner image:      $IMAGE"
echo "==> Podman socket:    $SOCK_PATH"
echo "==> Filter:           ${FILTER:-<none>}"
echo

podman run --rm \
  --cap-add SYS_ADMIN --cap-add NET_ADMIN \
  --security-opt seccomp=unconfined \
  --security-opt apparmor=unconfined \
  -v "$WORKSPACE_ROOT":/workspace:Z \
  -v "$CARGO_CACHE_VOLUME":/usr/local/cargo/registry \
  -v "$TARGET_CACHE_VOLUME":/workspace/target/lefthook-podman \
  -v "$SOCK_PATH":/var/run/podman.sock \
  -w /workspace \
  -e CONTAINER_HOST=unix:///var/run/podman.sock \
  -e TAU_CONTAINER_RUNTIME=podman \
  -e CARGO_INCREMENTAL=0 \
  -e RUST_BACKTRACE=1 \
  -e FILTER="$FILTER" \
  "$IMAGE" \
  bash -c '
    set -euo pipefail

    # Install nextest + podman client + bridge-net deps.
    if ! command -v cargo-nextest >/dev/null; then
      ARCH=$(uname -m)
      case "$ARCH" in
        aarch64) NEXTEST_URL="https://get.nexte.st/latest/linux-arm" ;;
        *)       NEXTEST_URL="https://get.nexte.st/latest/linux" ;;
      esac
      curl -LsSf "$NEXTEST_URL" | tar zxf - -C /usr/local/cargo/bin
    fi
    if ! command -v podman >/dev/null; then
      apt-get update -qq
      apt-get install -y -qq podman iproute2 nftables curl
    fi

    # The bind-mounted socket from the macOS host points at the *outer*
    # podman, so xtask + the test harness will spawn nested containers
    # against the same Linux machine they themselves run in. That makes
    # the bridge networking the kernel actually exercises CI-equivalent.
    podman version 2>&1 | head -3 || true

    # Build per-plugin images (xtask uses TAU_CONTAINER_RUNTIME=podman).
    unset CARGO_TARGET_DIR
    cargo run -p xtask -- build-plugin-images --target-dir target/lefthook-podman

    # Run the integration tests.
    if [[ -n "${FILTER:-}" ]]; then
      cargo nextest run \
        -p tau-plugin-compat \
        --features integration-tests \
        --test "$FILTER" \
        --target-dir target/lefthook-podman \
        --no-fail-fast
    else
      cargo nextest run \
        -p tau-plugin-compat \
        --features integration-tests \
        --target-dir target/lefthook-podman \
        --no-fail-fast
    fi
  '
