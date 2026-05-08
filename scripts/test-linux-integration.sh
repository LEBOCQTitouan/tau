#!/usr/bin/env bash
# scripts/test-linux-integration.sh — run tau-plugin-compat integration tests
# inside the Podman machine VM, with nested-container spawn via the VM's
# rootful Podman socket. This gives us Linux netavark networking semantics
# (close to CI Linux Docker bridge) for the integration tests.
#
# Why this exists: the macOS Podman default runs containers with
# slirp4netns rootless networking that lets container 127.0.0.1 reach the
# host's 127.0.0.1 directly. CI Linux Docker --network bridge does NOT
# do this. This script reproduces CI's networking semantics locally so
# we can debug the difference without round-tripping through CI.
#
# Architecture:
#   macOS shell
#       │ (podman machine ssh)
#       ▼
#   Podman VM (Fedora CoreOS, aarch64) ── /Users mounted via 9p
#       │
#       │ podman run rust:1.82-bookworm   (the test runner container)
#       │   │
#       │   │ -v /run/podman/podman.sock:/var/run/podman.sock  (DooD)
#       │   │ -v /Users/.../tau:/workspace                      (9p)
#       │   │
#       │   ▼
#       │ cargo nextest run --features integration-tests
#       │   │
#       │   │ each test invokes ContainerSandbox.spawn() which calls
#       │   │ `podman run` against /var/run/podman.sock  (= VM's rootful
#       │   │ podman socket)
#       │   ▼
#       └── nested per-plugin containers, with netavark bridge networking
#           (CI-equivalent semantics).
#
# Prerequisites:
#   brew install podman
#   podman machine init && podman machine start  (rootful or rootless OK;
#       VM's /run/podman/podman.sock works either way)
#
# Usage:
#   scripts/test-linux-integration.sh [<test-name>]
#
# Examples:
#   scripts/test-linux-integration.sh                       # all integration tests
#   scripts/test-linux-integration.sh layer4_container      # just one file
set -euo pipefail

cd "$(dirname "$0")/.."
WORKSPACE_ROOT="$PWD"
FILTER="${1:-}"

IMAGE="docker.io/library/rust:1.82-bookworm"
# Reuse the lefthook gate's persistent volumes so a warm run skips the
# ~10-min cold build of the workspace.
CARGO_VOL="cargo-cache"
TARGET_VOL="target-cache"

echo "==> Workspace:    $WORKSPACE_ROOT"
echo "==> VM image:     $IMAGE"
echo "==> Filter:       ${FILTER:-<none, runs all integration-tests>}"
echo

# Run via podman machine ssh so the bind-mount source paths
# (/run/podman/podman.sock) resolve VM-internally rather than being
# 9p-forwarded from macOS.
podman machine ssh -- bash -s -- "$WORKSPACE_ROOT" "$FILTER" "$IMAGE" "$CARGO_VOL" "$TARGET_VOL" <<'OUTER'
set -euo pipefail

WORKSPACE_ROOT="$1"
FILTER="$2"
IMAGE="$3"
CARGO_VOL="$4"
TARGET_VOL="$5"

cd "$WORKSPACE_ROOT"

# Ensure the workspace is reachable inside the VM.
if [[ ! -f Cargo.toml ]]; then
  echo "ERROR: workspace root '$WORKSPACE_ROOT' not reachable inside the VM" >&2
  echo "       (expected Cargo.toml; got nothing). Is /Users mounted via 9p?" >&2
  exit 2
fi

# The rootful Podman socket lives at /run/podman/podman.sock inside the
# VM. Check it exists.
if [[ ! -S /run/podman/podman.sock ]]; then
  echo "ERROR: /run/podman/podman.sock missing in the VM" >&2
  exit 2
fi

# Run the Rust container; nested containers spawn against the VM's
# rootful podman socket.
sudo podman run --rm \
  --cap-add SYS_ADMIN --cap-add NET_ADMIN \
  --security-opt seccomp=unconfined \
  --security-opt apparmor=unconfined \
  --security-opt label=disable \
  -v "$WORKSPACE_ROOT":/workspace \
  -v "$CARGO_VOL":/usr/local/cargo/registry \
  -v "$TARGET_VOL":/workspace/target/lefthook-podman \
  -v /run/podman/podman.sock:/var/run/podman.sock \
  -w /workspace \
  -e CONTAINER_HOST=unix:///var/run/podman.sock \
  -e TAU_CONTAINER_RUNTIME=podman \
  -e CARGO_INCREMENTAL=0 \
  -e RUST_BACKTRACE=1 \
  -e FILTER="$FILTER" \
  "$IMAGE" \
  bash -c '
    set -euo pipefail

    # Install nextest + podman CLI client (the daemon is the host VM).
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

    podman version 2>&1 | head -3 || true
    podman info --format "{{.Host.NetworkBackend}}" 2>&1 | head -2 || true

    unset CARGO_TARGET_DIR
    # Cap the image build at 10 minutes — well above the cold-build cost
    # (~5 min per image × 5 plugins, mitigated by buildx layer cache after
    # the first run) but bounded so a hung build does not eat all night.
    timeout 600 cargo run -p xtask -- build-plugin-images --target-dir target/lefthook-podman

    # Cap the test run at 5 minutes. The 5 layer4_container tests should
    # complete in seconds when working, but a hung container spawn or
    # nextest deadlock would otherwise sit indefinitely.
    if [[ -n "${FILTER:-}" ]]; then
      timeout 300 cargo nextest run \
        -p tau-plugin-compat \
        --features integration-tests \
        --test "$FILTER" \
        --target-dir target/lefthook-podman \
        --no-fail-fast
    else
      timeout 300 cargo nextest run \
        -p tau-plugin-compat \
        --features integration-tests \
        --target-dir target/lefthook-podman \
        --no-fail-fast
    fi
  '
OUTER
