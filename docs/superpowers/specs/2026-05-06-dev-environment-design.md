# Dev Environment + Pre-Push Test Gate — Design

**Status:** Proposed
**Date:** 2026-05-06
**Authors:** Titouan Lebocq
**Scope:** Sub-project — local Linux dev environment + automatic pre-commit/pre-push gate

## Goal

Catch cross-platform build and runtime issues locally before pushing, so CI is confirmation rather than discovery. Also unblock local debugging of F task 6.5 follow-ups (strict_net_filter integration test hang, Container-adapter network filtering) — both currently impossible to investigate on darwin.

Past pain this addresses:
- IPC sub-project broke on Windows; caught only at CI time.
- F task 6.5 hit `std::os::fd` (unix-only) cfg-gating miss; caught only at CI time.
- F task 6.5 produced two `#[ignore]`'d test classes that need a real Linux kernel to investigate; today there is no local reproduction path.

## Scope (this iteration)

**In scope:** Linux-only local dev environment with automatic pre-commit + pre-push gates on Apple Silicon Mac.

**Out of scope (deferred to follow-up PRs):**
- Windows VM (UTM + Windows 11 ARM). Largest one-time setup; PR2 candidate.
- macOS VM (Tart). Tart is Fair Source, not OSI-approved FOSS; won't add.
- x86_64 Linux runtime emulation (QEMU). Slow; CI handles x86_64 ground truth.

The Linux-only iteration delivers the highest immediate value: it unblocks the F task 6.5 follow-ups and catches the largest class of cross-platform bugs (cfg-gating misses + Linux runtime regressions). Windows comes in a follow-up PR.

## Constraints

- **All open source.** Excludes OrbStack, Docker Desktop, VMware Fusion, Parallels, Tart.
- **Run locally** on the Mac. No cloud services.
- **Editor-agnostic.** No VS Code lock-in.
- **Production-like circumstances.** Tests run with the privileges they need.
- **Apple Silicon (arm64) host.**

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│ Apple Silicon Mac (host)                                         │
│                                                                  │
│  git commit ──── lefthook pre-commit ─── ~30-60s (host, no VM)   │
│                  • cargo fmt --all -- --check                    │
│                  • cargo clippy --workspace --all-targets        │
│                  • cargo nextest run --workspace --all-targets   │
│                  • cargo check --target x86_64-unknown-linux-gnu │
│                                                                  │
│  git push ─────── lefthook pre-push ────  ~3-5min (Linux VM)     │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │ Podman machine (Linux VM, persistent, started on demand)   │  │
│  │                                                            │  │
│  │  ephemeral container per pre-push run:                     │  │
│  │  podman run --rm                                           │  │
│  │    --cap-add SYS_ADMIN --cap-add NET_ADMIN                 │  │
│  │    --security-opt seccomp=unconfined                       │  │
│  │    --security-opt apparmor=unconfined                      │  │
│  │    -v $WORKSPACE:/workspace -v cargo-cache:/cache          │  │
│  │    -v target-cache:/workspace/target/lefthook-podman       │  │
│  │    -w /workspace                                           │  │
│  │    rust:1.82-bookworm                                      │  │
│  │    bash -c 'cargo nextest run --workspace --all-targets'   │  │
│  │                                                            │  │
│  │  same image used for interactive debug:                    │  │
│  │  podman run -it ... rust:1.82-bookworm bash                │  │
│  └────────────────────────────────────────────────────────────┘  │
│                                                                  │
│  named volumes (persist across runs):                            │
│  • cargo-cache:/usr/local/cargo/registry — registry index        │
│  • target-cache:/workspace/target/lefthook-podman — incremental  │
└──────────────────────────────────────────────────────────────────┘
```

### Components

- **lefthook** (FOSS, MIT, Go binary): git hook runner. Wires pre-commit and pre-push automatically. No daemon.
- **Podman** (FOSS, Apache 2.0): Linux container runtime. Daemonless. Drop-in for `docker` CLI. `podman machine` manages a hidden Linux VM via Apple's Virtualization.framework.
- **rust:1.82-bookworm** Docker image: matches what CI's `test-net-filter / linux` job already uses. Exact reproducibility.
- **Apple Virtualization.framework**: the hypervisor layer. Built into macOS, free, hardware-accelerated. Used by Podman to run the Linux VM.

### Why no separate "interactive dev" container

Same image, same caps, same volumes are used whether the container runs `cargo nextest` (gate) or `bash` (interactive debug). The only difference is the invocation flag. One mental model; no lifecycle complexity from a long-lived dev container.

## Locked design decisions

### Decision 1 — Scope: Linux-only this iteration

**Decision:** Ship Linux dev environment + pre-commit/pre-push gates in this PR. Defer Windows VM (UTM) and macOS VM (Tart) to follow-up PRs.

**Rationale:** The Linux leg has the smallest setup cost and the largest immediate value (unblocks F task 6.5 follow-up debugging). Mac coverage is free since the host already runs the tests natively. Windows is the largest setup (~30 min one-time provisioning + SSH config) and is best handled as its own focused PR.

**Consequences:**
- Two F task 6.5 follow-ups become debuggable locally.
- The IPC class of cross-platform bug is NOT yet caught locally; it is still CI-only until the Windows follow-up PR ships.
- Pre-commit `cargo check --target x86_64-unknown-linux-gnu` catches build-time arch mismatches without needing the deeper VM machinery.

### Decision 2 — Tiering: pre-commit (host) + pre-push (container), both automatic

**Decision:** Two git hooks via lefthook. No manual user-invoked scripts.

- `pre-commit`: fast, host-only checks. Runs `cargo fmt`, `cargo clippy`, `cargo nextest run`, `cargo check --target x86_64-unknown-linux-gnu`. Wall-clock ~30-60s with parallelism.
- `pre-push`: deep gate. Privileged Podman container running `cargo nextest run --workspace --all-targets`. Wall-clock ~3-5 min cold, ~20s incremental.

**Rationale:** The user explicitly rejected manual-script-invoked deep gates. Two automatic hooks across the standard git lifecycle (commit → push) is the textbook two-tier pattern. Pre-commit catches the fast class (formatting, lints, native macOS tests, build-time cfg gates) at commit time; pre-push catches Linux-runtime regressions before any code leaves the machine. Bypass with `git commit --no-verify` / `git push --no-verify` for emergencies.

**Consequences:**
- Push is no longer instantaneous; expect ~3-5 min on a cold cache, ~20s warm.
- Cargo cache and target dir for the deep gate are persistent named Podman volumes — warm runs are fast.
- A docs-only commit skips Rust checks via lefthook's `glob: "*.rs"` filter (no waste).

### Decision 3 — Privilege model: selective caps, not `--privileged`

**Decision:** The pre-push container uses `--cap-add SYS_ADMIN --cap-add NET_ADMIN --security-opt seccomp=unconfined --security-opt apparmor=unconfined` — the minimum tau's strict-tier sandbox documents (per [ADR-0019](../../decisions/0019-per-host-network-filter.md)).

**Rationale:** `--privileged` would mirror the test-net-filter / linux CI job exactly, but it grants a superset of caps tau needs (e.g., `CAP_SYS_PTRACE`, `CAP_DAC_OVERRIDE`, `CAP_SYS_RAWIO`). Selective caps catch privilege drift: the day a future test silently grows a new cap dependency, the local gate fails, surfacing the regression at gate time rather than masking it under a permissive `--privileged`. This was the user's "isn't privileged a trap?" concern — selective caps directly answer it.

**Consequences:**
- If a new test legitimately needs another cap (rare), the cap list in `lefthook.yml` must be updated. This is the explicit cost of least-privilege; trade-off accepted.
- The exact cap list comments reference [ADR-0019](../../decisions/0019-per-host-network-filter.md) so future contributors know where the requirement comes from.

### Decision 4 — CI parity: update `test-net-filter / linux` to selective caps

**Decision:** Update the `.github/workflows/ci.yml` job that runs `docker run --privileged ...` to use the same selective-cap shape as the local lefthook deep gate.

**Rationale:** Decision 3's "catch privilege drift" guarantee only holds if BOTH local and CI enforce the same cap set. If CI stays at `--privileged` and local is selective, a test that grows a privilege requirement passes CI but fails local — local users would be tempted to widen the local cap set rather than recognize the drift. Parity ensures the drift detection works at both ends.

**Consequences:**
- Small CI workflow edit (~5 lines: replace `--privileged` with the four flags).
- Risk: if some currently-passing CI test silently relied on a cap outside the declared set, it would fail in CI after this change. Mitigation: roll out, monitor, expand cap list with documented justification if needed. Most likely outcome: zero breakage, because tau's surface only uses the documented caps.

### Decision 5 — Cache strategy: persistent named Podman volumes

**Decision:** Two named volumes mounted into the pre-push container:
- `cargo-cache:/usr/local/cargo/registry` — registry index + downloaded crates
- `target-cache:/workspace/target/lefthook-podman` — compiled artifacts

**Rationale:** A cold pre-push run with no cache is ~3 min (full workspace recompile). With named volumes for both the registry and target dir, warm runs are ~20s. Without persistence, every push pays the cold-build cost — unacceptable friction.

**Consequences:**
- `target/lefthook-podman/` is added to `.gitignore` patterns (already excluded by `**/target` glob; verify).
- `target/lefthook/{fmt,clippy,test,check-linux}` (the pre-commit dirs) live on the macOS host and are similarly excluded by the existing `**/target` glob.
- New target dirs do not collide with `target/main` (main agent) or `target/agent-*` (sub-agents) per [CLAUDE.md Rule 1](../../../CLAUDE.md). Documented in CLAUDE.md update.

### Decision 6 — Inline pre-push command (no sidecar script)

**Decision:** The pre-push deep-gate `podman run` command is inlined in `lefthook.yml`, not extracted to a separate shell file under `.lefthook/` or `scripts/`.

**Rationale:** The user explicitly rejected manual user-invoked scripts. A sidecar file invoked by lefthook would meet that letter, but inline keeps everything visible in one place and is short enough (~12 YAML lines) to be readable. Trade-off: YAML-quoting of multi-line shell is mildly ugly. Acceptable.

**Consequences:**
- Single source of truth for both hooks: `lefthook.yml`.
- If the deep-gate body grows beyond ~30 lines, revisit and consider extracting.

## Files

### Files added

- **`lefthook.yml`** (root) — pre-commit and pre-push hooks. Pre-push command inlined.
- **`docs/dev-environment.md`** — one-time setup steps, day-to-day usage, interactive-debug pattern, architecture-mismatch caveat, troubleshooting.

### Files updated

- **`CLAUDE.md`** — note that `lefthook install` is part of contributor setup; document the new `target/lefthook/*` and `target/lefthook-podman` paths reserved for hook use (does not collide with Rule 1's `target/main` and `target/agent-*`).
- **`.github/workflows/ci.yml`** — `test-net-filter / linux` job swaps `--privileged` for the selective-cap flag set (Decision 4).

### Files NOT created

- No standalone shell scripts. Decision 6.
- No `.cargo/config.toml` change. `cargo check --target X` skips linking; no cross-linker setup needed.
- No managed-VM lifecycle scripts. `podman machine init` / `start` is one-time, documented.

## Hook contents

### `lefthook.yml` — pre-commit

```yaml
pre-commit:
  parallel: true
  commands:
    fmt:
      glob: "*.rs"
      run: env CARGO_TARGET_DIR=target/lefthook/fmt cargo fmt --all -- --check
    clippy:
      glob: "*.{rs,toml}"
      run: env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/lefthook/clippy cargo clippy --workspace --all-targets -- -D warnings
    test-native:
      glob: "*.{rs,toml}"
      run: env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/lefthook/test cargo nextest run --workspace --all-targets
    check-linux-x86:
      glob: "*.{rs,toml}"
      run: env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/lefthook/check-linux cargo check --workspace --all-targets --target x86_64-unknown-linux-gnu
```

### `lefthook.yml` — pre-push (inlined deep gate)

```yaml
pre-push:
  commands:
    deep-gate:
      run: |
        podman run --rm \
          --cap-add SYS_ADMIN --cap-add NET_ADMIN \
          --security-opt seccomp=unconfined \
          --security-opt apparmor=unconfined \
          -v "$PWD":/workspace:Z \
          -v cargo-cache:/usr/local/cargo/registry \
          -v target-cache:/workspace/target/lefthook-podman \
          -w /workspace \
          docker.io/library/rust:1.82-bookworm \
          bash -c '
            set -e
            apt-get update -qq
            apt-get install -y -qq iproute2 nftables
            env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/lefthook-podman \
              cargo nextest run --workspace --all-targets
          '
```

### CI parity update (`.github/workflows/ci.yml`)

The `test-net-filter / linux` job's `docker run --privileged ...` line becomes:

```yaml
docker run --rm \
  --cap-add SYS_ADMIN --cap-add NET_ADMIN \
  --security-opt seccomp=unconfined \
  --security-opt apparmor=unconfined \
  -v "$PWD":/workspace \
  -w /workspace \
  rust:1.82-bookworm \
  bash -c '...'   # body unchanged from current job
```

## Setup (`docs/dev-environment.md` content outline)

```bash
# 1. Install runtime dependencies (FOSS only)
brew install lefthook podman                      # core tooling
rustup target add x86_64-unknown-linux-gnu        # for pre-commit cross-check

# 2. Initialize Podman's hidden Linux VM
#    --rootful is required so privileged-style containers work
podman machine init --cpus 4 --memory 8192 --rootful
podman machine start

# 3. Wire git hooks
lefthook install

# 4. Verify
lefthook run pre-commit --all-files     # should pass on a clean tree
```

The doc additionally covers:
- **Interactive Linux debugging**: `podman run -it --rm --cap-add SYS_ADMIN --cap-add NET_ADMIN --security-opt seccomp=unconfined --security-opt apparmor=unconfined -v "$PWD":/workspace:Z -v cargo-cache:/usr/local/cargo/registry -w /workspace docker.io/library/rust:1.82-bookworm bash`. Same image + caps + cache as the gate.
- **Bypassing the gate** for emergencies: `git commit --no-verify`, `git push --no-verify`. Documented but discouraged.
- **Architecture mismatch caveat**: the Podman machine on Apple Silicon is arm64 Linux. CI's `test-net-filter / linux` is x86_64 Linux. The pre-commit `cargo check --target x86_64-unknown-linux-gnu` catches build-time arch issues; runtime behavior on x86_64 is still ground truth in CI.
- **Troubleshooting**: Podman machine reset, hook didn't run, target dir collisions, etc.

## Architecture mismatch (known gaps)

Apple Silicon is arm64; CI Linux runners are x86_64. This setup will catch:
- ✅ All cfg-gating bugs (`cfg(unix)`, `cfg(target_os)`)
- ✅ All build-time API mismatches (the `std::os::fd` class)
- ✅ Most Linux runtime regressions (sandbox tests, IPC, syscall behavior on arm64 Linux)
- ✅ Privilege drift (Decision 3)

This setup will NOT catch:
- ❌ Arch-specific runtime bugs that only manifest on x86_64 (struct alignment, atomic ordering, syscall numbers in raw asm — landlock syscall numbers differ between x86_64 and arm64 Linux)
- ❌ glibc-version differences (Podman ships current Debian; CI ships Ubuntu)
- ❌ Windows-specific issues (deferred to follow-up PR)
- ❌ macOS-specific issues that require a fresh macOS VM (deferred indefinitely; Mac coverage stays via the host)

These remaining gaps are caught by CI. The pre-push gate covers ~95% of the cross-platform pain points; CI handles the rest.

## What this enables

After this PR ships:
1. **F task 6.5 follow-up #2 (strict_net_filter integration test hang)** becomes debuggable locally. `podman run -it ...` + gdb/strace can reproduce the hang interactively. Today this requires CI, which doesn't allow interactive debugging.
2. **F task 6.5 follow-up #1 (Container-adapter network filtering)** becomes debuggable locally via the same mechanism.
3. **The `cfg(unix)` class of bug** (T7-style miss during F 6.5) is caught at commit time, not CI time.
4. **Privilege drift** is caught at commit/push time, not silently masked by CI's `--privileged`.

## Testing the new setup

The dev environment itself needs verification before contributors rely on it. Plan:
1. Run `lefthook run pre-commit --all-files` on a clean tree — must pass.
2. Run `lefthook run pre-push --all-files` on a clean tree — must pass (validates Podman + rust:1.82-bookworm + cargo nextest under selective caps).
3. Update CI to selective caps and verify the modified `test-net-filter / linux` job still passes on the dev-environment PR.
4. Open a PR and let the standard CI matrix verify nothing else regressed.

## Out of scope (explicit)

- Windows VM (UTM + Windows 11 ARM): follow-up PR.
- macOS VM (Tart): not pursued; Tart is Fair Source.
- x86_64 Linux runtime via QEMU: not pursued; CI provides x86_64 ground truth.
- Pre-commit "fix" mode (`cargo fmt` instead of `cargo fmt -- --check`): out of scope; the gate is verify-only.
- IDE-specific configuration (VS Code's `.vscode/`, JetBrains): out of scope; the design is editor-agnostic.

## References

- [ADR-0019 — Per-host network filter](../../decisions/0019-per-host-network-filter.md): documents the cap requirements (CAP_SYS_ADMIN, CAP_NET_ADMIN-in-userns) referenced by Decision 3.
- [Sandboxing followups](2026-05-03-sandboxing-followups.md): tracks the F task 6.5 follow-ups this work unblocks.
- [CLAUDE.md cargo rules](../../../CLAUDE.md): the per-agent target dir convention this design extends.
- [lefthook documentation](https://lefthook.dev/): hook runner used by both hooks.
- [Podman documentation](https://podman.io/docs/): container runtime used by the pre-push deep gate.
