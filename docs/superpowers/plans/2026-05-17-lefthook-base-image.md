# Lefthook pre-built base image — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement task-by-task.

**Goal:** Pre-bake the deep-gate container's apt deps + cargo-nextest + rustup 1.91 toolchain into a Dockerfile-hashed local image so the gate stops paying ~20–60s of setup time per run.

**Architecture:** New `docker/lefthook-base.Dockerfile` replicates the inline installs. `lefthook.yml` adds a lazy `podman build` keyed on the Dockerfile's sha256 prefix, then uses that tag in `podman run`. The inline setup block in the bash heredoc is replaced with a comment.

**Tech Stack:** Podman, bash, lefthook, Dockerfile.

**Spec:** `docs/superpowers/specs/2026-05-17-lefthook-base-image-design.md`.

---

## Files

- Create: `docker/lefthook-base.Dockerfile`
- Modify: `lefthook.yml` (pre-push.deep-gate.run — the bash heredoc + the podman run line)

---

## Task 1: Add the Dockerfile

**Files:**
- Create: `docker/lefthook-base.Dockerfile`

- [ ] **Step 1: Create the Dockerfile**

```dockerfile
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
```

- [ ] **Step 2: Verify it builds manually**

```bash
podman build -t test-lefthook-base -f docker/lefthook-base.Dockerfile docker/
```

Expected: builds successfully in ~3-5 min on a cold machine, faster if rust:1.82-bookworm is already pulled.

- [ ] **Step 3: Verify the contents**

```bash
podman run --rm test-lefthook-base sh -c 'apt list --installed 2>/dev/null | grep -E "(iproute2|nftables|podman)/"'
podman run --rm test-lefthook-base cargo nextest --version
podman run --rm test-lefthook-base rustup toolchain list
```

Expected:
- All three apt packages appear as `<name>/<dist>` lines.
- cargo-nextest prints a version (e.g., `cargo-nextest 0.9.x`).
- `rustup toolchain list` includes `1.91.0-aarch64-unknown-linux-gnu` (or x86_64).

- [ ] **Step 4: Clean up the test tag**

```bash
podman rmi test-lefthook-base
```

- [ ] **Step 5: Commit**

```bash
git add docker/lefthook-base.Dockerfile
git -c user.name="titouanlebocq" -c user.email="lebocq.tit@gmail.com" \
  commit --no-verify -m "build(lefthook): add lefthook-base Dockerfile

Pre-bakes apt deps + cargo-nextest + rustup 1.91 toolchain so the
deep-gate container stops paying that install cost on every run."
```

---

## Task 2: Wire the lazy build + use the new image

**Files:**
- Modify: `lefthook.yml` — `pre-push.commands.deep-gate.run`

- [ ] **Step 1: Add the lazy build block**

Find the start of the `run: |` block:

```yaml
      run: |
        podman run --rm \
```

Insert before `podman run` (keeping the same indentation as `podman run`):

```yaml
      run: |
        DOCKERFILE=docker/lefthook-base.Dockerfile
        TAG="localhost/tau-lefthook-base:$(shasum -a 256 "$DOCKERFILE" | cut -c1-12)"
        if ! podman image exists "$TAG"; then
          echo "Building $TAG (first run after Dockerfile change; ~3-5 min)" >&2
          podman build -t "$TAG" -f "$DOCKERFILE" docker/
        fi
        podman run --rm \
```

- [ ] **Step 2: Swap the FROM image in the podman run command**

Find this line in the `podman run ...` block:

```yaml
          docker.io/library/rust:1.82-bookworm \
```

Replace with:

```yaml
          "$TAG" \
```

- [ ] **Step 3: Replace the inline setup block in the bash heredoc**

Find inside the heredoc (after `set -e`):

```bash
            set -e
            apt-get update -qq
            apt-get install -y -qq iproute2 nftables podman

            # Install cargo-nextest (not bundled with the rust image).
            if ! command -v cargo-nextest >/dev/null; then
              ARCH=$(uname -m)
              case "$ARCH" in
                aarch64) NEXTEST_URL="https://get.nexte.st/latest/linux-arm" ;;
                *)       NEXTEST_URL="https://get.nexte.st/latest/linux" ;;
              esac
              curl -LsSf "$NEXTEST_URL" | tar zxf - -C /usr/local/cargo/bin
            fi

            # Use --target-dir (not CARGO_TARGET_DIR env) so child `cargo build`
```

Replace with:

```bash
            set -e

            # Setup (apt deps, cargo-nextest, rustup 1.91 toolchain) is
            # pre-installed in the base image — see docker/lefthook-base.Dockerfile.

            # Use --target-dir (not CARGO_TARGET_DIR env) so child `cargo build`
```

- [ ] **Step 4: Remove the inline rustup install in Stage 1 (msrv-check)**

Find:

```bash
            # ─── 1. msrv-check / linux ────────────────────────────
            # CI MSRV pin is 1.91 (see ci.yml). Install + use rustup
            # to invoke it inline so we do not pollute the default
            # toolchain in the persistent volume.
            echo "::group::msrv-check"
            rustup toolchain install 1.91 --profile minimal --no-self-update >/dev/null
            cargo +1.91 check --workspace --all-targets --locked --target-dir $TARGET
            echo "::endgroup::"
```

Replace with:

```bash
            # ─── 1. msrv-check / linux ────────────────────────────
            # CI MSRV pin is 1.91 (see ci.yml). 1.91 toolchain is
            # pre-installed in the base image.
            echo "::group::msrv-check"
            cargo +1.91 check --workspace --all-targets --locked --target-dir $TARGET
            echo "::endgroup::"
```

- [ ] **Step 5: Commit**

```bash
git add lefthook.yml
git -c user.name="titouanlebocq" -c user.email="lebocq.tit@gmail.com" \
  commit --no-verify -m "build(lefthook): use pre-built base image for deep-gate

Lazy podman build keyed on Dockerfile sha256 (auto-rebuilds on edit).
Inline apt + nextest + rustup-1.91 setup removed — pre-installed in
the image. Saves ~20-60s per warm gate run."
```

---

## Task 3: Verify the gate works with the new image

**Files:**
- None modified.

- [ ] **Step 1: Trigger the first (image-building) run**

```bash
podman images localhost/tau-lefthook-base --format '{{.Tag}}' | head -5
# expected: empty (no existing tag yet)

time lefthook run pre-push --force
```

Expected on first run:
- Output starts with `Building localhost/tau-lefthook-base:<hash> ...`.
- Image build completes, then the gate continues.
- The bash heredoc no longer prints apt-get/curl/rustup output for setup.
- Exit code 0.

- [ ] **Step 2: Trigger a second (image-cached) run**

```bash
time lefthook run pre-push --force
```

Expected:
- No `Building ...` message.
- Wall-clock at least 15s faster than the first run minus the image-build time, OR ~20–60s faster than baseline if comparing against pre-PR.
- Exit code 0.

- [ ] **Step 3: Confirm the image tag matches the Dockerfile**

```bash
shasum -a 256 docker/lefthook-base.Dockerfile | cut -c1-12
podman images localhost/tau-lefthook-base --format '{{.Tag}}'
```

Expected: same 12-char hex string.

- [ ] **Step 4: Confirm a Dockerfile edit triggers a rebuild**

```bash
# Temporary edit
echo "# trivial comment" >> docker/lefthook-base.Dockerfile
shasum -a 256 docker/lefthook-base.Dockerfile | cut -c1-12
lefthook run pre-push --force 2>&1 | head -5
# expected: "Building localhost/tau-lefthook-base:<new-hash> ..."

# Revert
git checkout docker/lefthook-base.Dockerfile
```

Expected: a fresh image build triggers on the edited Dockerfile.

- [ ] **Step 5: Commit nothing**

Verification only.

---

## Task 4: Open PR

**Files:**
- None.

- [ ] **Step 1: Push the branch**

Per CLAUDE.md AGENT PUSH RULES — `git push` direct from agent runtime is blocked when the gate is on. Use:

```bash
git push --no-verify -u origin worktree-lefthook-base-image
```

The change is yaml + Dockerfile only (no Rust), so `--no-verify` is acceptable per the rules. The local gate was already verified in Task 3.

- [ ] **Step 2: Create the PR**

```bash
gh pr create --title "build(lefthook): pre-built base image for deep-gate" --body "$(cat <<'EOF'
## Summary
- New `docker/lefthook-base.Dockerfile` pre-bakes the container deps the deep-gate previously installed inline every run: apt packages (iproute2, nftables, podman), `cargo-nextest`, and the MSRV `1.91` rustup toolchain.
- `lefthook.yml` builds this image lazily, tagged with the first 12 hex chars of the Dockerfile sha256. Edits → new tag → auto-rebuild on the next gate run. No edit → instant container start.
- Inline setup block + the inline `rustup install 1.91` in Stage 1 removed (now provided by the image).
- Independent of PR #121 (parallelization). Each PR is independently valuable; they touch the same heredoc block but compose cleanly.

## Test plan
- [x] `podman build` succeeds; runtime verifies apt packages, nextest, and `1.91` toolchain are present.
- [x] First `lefthook run pre-push --force` builds the image, gate green.
- [x] Second run skips the build, gate green and ~20-60s faster than baseline.
- [x] Dockerfile edit triggers a rebuild (new hash tag).
- [ ] CI green (this PR doesn't change CI; CI is unaffected).

## Notes
- Pushed `--no-verify` per CLAUDE.md AGENT PUSH RULES — YAML+Dockerfile only, no Rust.
- After this and #121 both land, the now-redundant Stage 0 parallel-setup block in #121 can be removed in a small follow-up.

Spec: `docs/superpowers/specs/2026-05-17-lefthook-base-image-design.md`
Plan: `docs/superpowers/plans/2026-05-17-lefthook-base-image.md`

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Self-review

- Spec coverage: ✓ Dockerfile (Task 1), lazy build + image swap (Task 2), inline setup removal (Task 2), verification including rebuild trigger (Task 3), PR (Task 4).
- No placeholders.
- Identifier consistency: `TAG`, `DOCKERFILE`, `localhost/tau-lefthook-base:<hash>` used identically across tasks.
- Independence from #121 verified: this PR touches the same heredoc block but only the *setup* section; #121 touches the *body* (stage parallelization). Conflict on merge is small and mechanical.
