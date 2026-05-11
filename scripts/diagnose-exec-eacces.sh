#!/usr/bin/env bash
# scripts/diagnose-exec-eacces.sh — sub-project E diagnostic driver
#
# Builds crates/landlock-exec-repro/ inside the lefthook Podman gate
# config and runs a fixed matrix of landlock + namespace + seccomp
# configurations against /usr/bin/echo. Prints a result table.
#
# See docs/superpowers/specs/2026-05-11-sub-project-e-exec-gating-design.md
# for the methodology.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

podman run --rm \
  --cap-add SYS_ADMIN --cap-add NET_ADMIN \
  --security-opt seccomp=unconfined \
  --security-opt apparmor=unconfined \
  --security-opt label=disable \
  -v "$REPO_ROOT":/workspace \
  -v cargo-cache:/usr/local/cargo/registry \
  -v target-cache:/workspace/target/lefthook-podman \
  -w /workspace/crates/landlock-exec-repro \
  docker.io/library/rust:1.82-bookworm \
  bash -c '
    set -e
    export CARGO_INCREMENTAL=0
    cargo build --release --target-dir /workspace/target/lefthook-podman 2>&1 | tail -3
    BIN=/workspace/target/lefthook-podman/release/landlock-exec-repro

    run_row() {
      local label="$1"; shift
      local out exit
      set +e
      out=$("$BIN" "$@" 2>&1)
      exit=$?
      set -e
      local meaning
      case "$exit" in
        0)   meaning="exec ok" ;;
        32)  meaning="setup-err: arg parse" ;;
        33)  meaning="setup-err: landlock build" ;;
        34)  meaning="setup-err: landlock create" ;;
        35)  meaning="setup-err: landlock add_rule" ;;
        36)  meaning="setup-err: landlock restrict_self" ;;
        37)  meaning="setup-err: unshare" ;;
        38)  meaning="setup-err: seccomp compile" ;;
        39)  meaning="setup-err: seccomp apply" ;;
        65)  meaning="execve EPERM (errno=1)" ;;
        66)  meaning="execve ENOENT (errno=2)" ;;
        77)  meaning="execve EACCES (errno=13)" ;;
        *)   meaning="exit=$exit (out=$out)" ;;
      esac
      printf "%-50s  %3d  %s\n" "$label" "$exit" "$meaning"
    }

    TARGET=/usr/bin/echo
    printf "%-50s  %s  %s\n" "# config" "exit" "meaning"
    printf -- "---\n"
    run_row "0 unsandboxed"            --target="$TARGET"
    run_row "1 lock(base)"             --landlock=baseline --target="$TARGET"
    run_row "2 lock(base+exec=Exe)"    --landlock=baseline+exec --exec-path="$TARGET" --exec-grants=Execute --target="$TARGET"
    run_row "3 lock(base+exec=Rd+Exe)" --landlock=baseline+exec --exec-path="$TARGET" --exec-grants=ReadFile,Execute --target="$TARGET"
    run_row "4 lock(base+exec=AllV1)"  --landlock=baseline+exec --exec-path="$TARGET" --exec-grants=FromAllV1 --target="$TARGET"
    run_row "5 lock(base+exec=Rd+Exe)+ns" --landlock=baseline+exec --exec-path="$TARGET" --exec-grants=ReadFile,Execute --unshare-user --unshare-net --target="$TARGET"
    run_row "6 lock(base+exec=Rd+Exe)+ns+sc" --landlock=baseline+exec --exec-path="$TARGET" --exec-grants=ReadFile,Execute --unshare-user --unshare-net --seccomp --target="$TARGET"
    run_row "7 lock(base)+ns+sc"       --landlock=baseline --unshare-user --unshare-net --seccomp --target="$TARGET"
    run_row "8 lock(dir-only=Rd+RdDir+Exe)+ns+sc" --landlock=dir-only --dir-only-path=/usr/bin --dir-only-grants=ReadFile,ReadDir,Execute --unshare-user --unshare-net --seccomp --target="$TARGET"
  '
