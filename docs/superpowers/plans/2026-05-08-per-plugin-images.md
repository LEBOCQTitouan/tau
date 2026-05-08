# Per-plugin Container Images Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Un-`#[ignore]` the 5 Container-adapter plugin tests in `layer4_container.rs` by replacing the broken host-path bind-mount with per-plugin Docker images that bake the plugin binary into the image.

**Architecture:** Each plugin gets a multi-stage Dockerfile (builder stage compiles the plugin from workspace source; runtime stage `FROM tau-plugin-base` copies the binary in and sets the entrypoint). Shared `tau-plugin-base` image bundles `tau-net-bridge` + `ca-certificates` on `debian:bookworm-slim`. Container adapter resolves image name as `tau-plugin-<bin>:dev` from the `Command`'s program path. CI builds images via `cargo xtask build-plugin-images` with GHA buildx cache.

**Tech Stack:** Rust 1.x (stable, per `rust-toolchain.toml`); Docker / Podman (`docker buildx` with GHA cache for CI); `debian:bookworm-slim` base; tokio for the proxy task on host (unchanged); GitHub Actions for CI.

**Spec:** `docs/superpowers/specs/2026-05-08-per-plugin-images-design.md` (committed at `a8f1c3f`).

**Branch:** `feat/per-plugin-images` (already cut from main).

---

## File Structure

| Path | Owner | Purpose |
|---|---|---|
| `crates/tau-plugin-base/Dockerfile` | NEW | Shared base image. Multi-stage: builds `tau-net-bridge` from workspace source; runtime stage = `debian:bookworm-slim` + bridge + ca-certs + non-root user. NOT a Cargo crate (no `Cargo.toml`); cargo silently ignores. |
| `crates/tau-plugins/<name>/Dockerfile` (×5) | NEW | Per-plugin Dockerfile. Multi-stage: builder compiles `tau-plugins-<name>` from workspace source → runtime stage `FROM tau-plugin-base:dev` + COPY binary + ENTRYPOINT. |
| `xtask/Cargo.toml` + `xtask/src/main.rs` | NEW | New workspace member. Subcommands: `build-base-image`, `build-plugin-images [--name <bin>]`. Auto-detects podman/docker via `tau-sandbox-container`'s probe. |
| `Cargo.toml` (workspace) | MODIFY | Add `xtask` to `members`. |
| `crates/tau-sandbox-container/src/runner.rs` | MODIFY | `wrap_command`: resolve image name from `cmd.get_program()` basename; drop host-path bind-mount; drop bridge bind-mount; for HTTP plans set `--entrypoint=/usr/local/bin/tau-net-bridge`. Delete `DEFAULT_BASE_IMAGE` const, `ProxyConfig::bridge_path`, `TAU_NET_BRIDGE_PATH` lookup. |
| `crates/tau-plugin-compat/tests/layer4_container.rs` | MODIFY | Un-`#[ignore]` 5 tests (lines 251, 343, 465, 567, 658). Add `image_present_or_skip(&str) -> bool` helper. |
| `.github/workflows/ci.yml` | MODIFY | In `test-tau-plugin-compat` job: add `docker/setup-buildx-action@v3` step + `cargo xtask build-plugin-images` step before `cargo nextest run`. Cache via GHA. |
| `docs/decisions/0021-per-plugin-images.md` | NEW | ADR: per-plugin container images, four-phase roadmap, locked decisions. |
| `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` | MODIFY | Close 2 gap rows (sub-project D leftover + sub-project H leftover) by linking to PR. |
| `CONTRIBUTING.md` | MODIFY | Add a "Running container tests locally" section: prerequisite `cargo xtask build-plugin-images` before `cargo test --test layer4_container`. |
| `docs/superpowers/specs/2026-05-08-per-plugin-images-design.md` | MODIFY (T1 only) | Fill in the `## Investigation` section with T1's debug findings. |

---

## Implementer prerequisites (read before starting)

- **macOS Apple Silicon dev:** Podman is the default container runtime per PR #40. Install via `brew install podman` if missing; `podman machine init && podman machine start` to bring up the Linux VM.
- **All cargo invocations:** must follow `CLAUDE.md` Rule 1 — `CARGO_TARGET_DIR=target/agent-impl` (subagent) or `target/main` (main agent), `CARGO_INCREMENTAL=0`, wrapped in `timeout`. Reference command: `timeout 240 env CARGO_INCREMENTAL=0 RUSTC_WRAPPER= CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p <crate>`.
- **`RUSTC_WRAPPER=`:** clear sccache; sccache has been observed to fail with `Operation not permitted` on this host (see PR #40 logs).
- **BASE_SHA verification:** any "pre-existing failure" claim must be checked against `a8f1c3f` (spec commit) before proceeding.
- **Docker arch:** Apple Silicon → builds default `linux/arm64`; CI Linux x86_64 → `linux/amd64`. Same Dockerfile.

---

## Task 1: Debug session — confirm root cause (HARD GATE)

**Files:**
- Modify: `docs/superpowers/specs/2026-05-08-per-plugin-images-design.md` — fill in the `## Investigation` section.

**Why this is a hard gate.** The spec assumes the EOF-before-handshake symptom is caused by `docker run` trying to exec a host-path binary that doesn't exist inside the container. If this hypothesis is wrong, the rest of the plan is invalid. T1 produces evidence; T2 only starts after the evidence confirms the hypothesis.

**Implementer prerequisites:**
- Podman running on macOS (or a Linux box / VM).
- The 5 plugin binaries built: `cargo build --release -p tau-plugins-shell -p tau-plugins-fs-read -p tau-plugins-anthropic -p tau-plugins-ollama -p tau-plugins-openai`.
- Docker/podman image `ghcr.io/tau-runtime/sandbox-base:v0.1` pulled (or fall back to `debian:bookworm-slim` if it doesn't exist — note this finding in the spec).

- [ ] **Step 1: Reproduce the EOF symptom by hand**

```bash
# Working dir: /Users/titouanlebocq/code/tau
PLUGIN_BIN="$(pwd)/target/release/shell-plugin"
ls -la "$PLUGIN_BIN"   # confirm it exists on host

# Run the same docker invocation tau-sandbox-container would build, minus the
# bridge wrapping (shell-plugin doesn't need the proxy).
podman run --rm -i \
  --user nobody \
  --cap-drop=ALL \
  --security-opt=no-new-privileges \
  --read-only \
  --pids-limit 256 \
  --ipc=none \
  --tmpfs /tmp:size=64m \
  --network none \
  ghcr.io/tau-runtime/sandbox-base:v0.1 \
  "$PLUGIN_BIN" \
  </dev/null
```

Expected: error message indicating the host path doesn't exist inside the container (e.g. `OCI runtime exec failed`, or `no such file or directory`). Capture verbatim into the spec.

If `ghcr.io/tau-runtime/sandbox-base:v0.1` is unpullable (404), substitute `debian:bookworm-slim` for the image and note this finding.

- [ ] **Step 2: Prove the hypothesis with a positive control**

```bash
# Bind-mount the binary to confirm: when the path IS present, exec works.
podman run --rm -i \
  --user nobody \
  --cap-drop=ALL \
  --security-opt=no-new-privileges \
  --read-only \
  --pids-limit 256 \
  --ipc=none \
  --tmpfs /tmp:size=64m \
  --network none \
  -v "$PLUGIN_BIN":/usr/local/bin/shell-plugin:ro \
  debian:bookworm-slim \
  /usr/local/bin/shell-plugin --version 2>&1 | head -20
```

Expected: either the plugin starts and responds (probably hangs waiting for stdin JSON-RPC), or it errors with a *different* message that's about its own startup (e.g. missing libc symbol if there's a libc mismatch — also valuable evidence). Capture output verbatim.

If the binary starts cleanly here but failed in Step 1, hypothesis confirmed.

If it errors here too (libc mismatch or similar), hypothesis is partially right but a deeper cause exists — write findings into spec, then **STOP and ask the user** before proceeding.

- [ ] **Step 3: Test the bridge-wrapped variant (HTTP plans)**

```bash
# Run the bridge-wrapped form by hand, mimicking what wrap_command produces
# for an HTTP plan today. Bridge bind-mount + plugin bind-mount.
PLUGIN_BIN="$(pwd)/target/release/anthropic-plugin"
BRIDGE_BIN="$(pwd)/target/release/tau-net-bridge"

# First confirm the bridge binary exists (it's a [[bin]] in tau-sandbox-native).
ls -la "$BRIDGE_BIN" || cargo build --release -p tau-sandbox-native --bin tau-net-bridge

# Sanity: the bridge runs on host?
"$BRIDGE_BIN" --help 2>&1 | head -5  # may not have --help; non-zero exit OK

# Now wrap inside container — note: WITHOUT proxy socket bind-mount this will
# fail at bridge's connect-to-proxy step, not at exec. Different failure mode.
podman run --rm -i \
  --user nobody \
  --cap-drop=ALL \
  --security-opt=no-new-privileges \
  --read-only \
  --pids-limit 256 \
  --ipc=none \
  --tmpfs /tmp:size=64m \
  --network bridge \
  -v "$BRIDGE_BIN":/usr/local/bin/tau-net-bridge:ro \
  debian:bookworm-slim \
  /usr/local/bin/tau-net-bridge \
    --proxy-sock=/run/tau-proxy.sock \
    --listen=127.0.0.1:8443 \
    -- /usr/local/bin/anthropic-plugin 2>&1 | head -30
```

Expected one of:
- "exec: /usr/local/bin/anthropic-plugin: not found" → confirms the binary-not-mounted root cause for the bridge-wrapped flow.
- Bridge prints a "bridge: bring lo up failed" warning, then errors connecting to proxy socket (which isn't there) → bridge starts; the exec failure is hypothesis-confirmed. *This is the expected path.*
- Anything else → record verbatim and pause.

- [ ] **Step 4: Write findings into the spec's `## Investigation` section**

Replace the Placeholder block in
`docs/superpowers/specs/2026-05-08-per-plugin-images-design.md` with the actual
findings. Template (fill in concretely):

```markdown
## Investigation

**Date:** 2026-05-08

**Hypothesis tested:** "EOF before handshake" is caused by `docker run`
attempting to exec a host-path plugin binary that doesn't exist inside the
container.

**Evidence (reproduce-by-hand on Apple Silicon Podman):**

1. **Negative control (today's failing flow):**
   - Command: `podman run ... ghcr.io/tau-runtime/sandbox-base:v0.1 <host-path>`
   - Output: <paste verbatim — expect "no such file or directory" or "OCI runtime exec failed">
   - Conclusion: <e.g. "exec fails inside container; binary is not present at the host path inside the container">

2. **Positive control (binary bind-mounted into container):**
   - Command: `podman run ... -v <host-path>:/usr/local/bin/<bin>:ro debian:bookworm-slim /usr/local/bin/<bin>`
   - Output: <paste verbatim>
   - Conclusion: <e.g. "binary starts cleanly when path is reachable; libc-compatible">

3. **Bridge-wrapped variant (HTTP plans):**
   - Command: <as above with bridge bind-mount + bridge as entrypoint>
   - Output: <paste verbatim>
   - Conclusion: <e.g. "same root cause; bridge starts, then fails to exec the plugin>

**Hypothesis status:** confirmed / refuted / partially confirmed.

**Implications for the design:** <e.g. "the per-plugin-image plan as
specified is correct; no design changes needed". OR: "in addition to the
binary-mount issue, libc X.Y is required — base image must include glibc
≥ X.Y">.

**Sign-off:** ready to proceed to T2.
```

- [ ] **Step 5: Commit the spec update**

```bash
git add docs/superpowers/specs/2026-05-08-per-plugin-images-design.md
git commit -m "$(cat <<'EOF'
docs(spec): T1 debug findings — confirm host-path-binary exec failure

Investigation section filled with negative/positive control results from
interactive Podman session. Hypothesis confirmed: docker run with a host
path as the program fails because the path doesn't exist inside the
container.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

**Hard gate:** if Step 4's "Hypothesis status" is **refuted**, stop and re-brainstorm with the user. Do not proceed to T2.

**Focused gate (no Rust):**
- Spec's `## Investigation` section is non-empty and concrete.
- Three "Conclusion" lines are filled in.
- "Sign-off" line says "ready to proceed to T2".

---

## Task 2: Build `tau-plugin-base` Dockerfile

**Files:**
- Create: `crates/tau-plugin-base/Dockerfile`
- Create: `crates/tau-plugin-base/.dockerignore`

**Why a multi-stage build for the base.** The base image needs the `tau-net-bridge` binary baked in. Building the bridge inside the Dockerfile (rather than COPYing a host-built one) avoids arch-mismatch (host darwin-arm64 vs. image linux/amd64 or linux/arm64) and makes the base image self-contained.

- [ ] **Step 1: Create the Dockerfile**

```dockerfile
# crates/tau-plugin-base/Dockerfile
# syntax=docker/dockerfile:1.6

# ---------- Builder stage: compile tau-net-bridge ----------
FROM rust:1-bookworm AS builder

WORKDIR /workspace

# Copy the rust-toolchain spec first so rustup pre-installs the right channel.
COPY rust-toolchain.toml ./

# Copy minimum needed to build tau-sandbox-native's tau-net-bridge bin target.
# We pull in workspace metadata + the source crates the bridge depends on.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY xtask ./xtask

RUN cargo build --release -p tau-sandbox-native --bin tau-net-bridge

# ---------- Runtime stage ----------
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 1000 tau \
    && useradd --system --uid 1000 --gid tau --shell /usr/sbin/nologin tau

COPY --from=builder /workspace/target/release/tau-net-bridge /usr/local/bin/tau-net-bridge

# Default user is tau (non-root); per-plugin Dockerfiles can override.
USER tau
```

- [ ] **Step 2: Create `.dockerignore`**

```
# crates/tau-plugin-base/.dockerignore
target/
**/target/
.git/
.github/
docs/
*.md
LICENSE-*
```

- [ ] **Step 3: Build the base image and verify**

```bash
# Working dir: workspace root
podman build \
  -f crates/tau-plugin-base/Dockerfile \
  -t tau-plugin-base:dev \
  .

# Verify the bridge binary is present and runs.
podman run --rm tau-plugin-base:dev /usr/local/bin/tau-net-bridge --proxy-sock=/tmp/x --listen=127.0.0.1:1 -- /bin/true
```

Expected from the run: the bridge attempts to bind 127.0.0.1:1 (port reserved; will fail with permission denied as user `tau`), then exits with non-zero. Output should be a clear bridge stderr message — not a "command not found" or "no such file" error. That confirms the binary is in place and runs.

If running as user `tau` blocks `/bin/true` because of `nologin` shell — note: `/bin/true` is exec'd directly, no shell needed. If still blocked, run without USER line and re-investigate.

- [ ] **Step 4: Confirm both runtimes work**

If Docker Desktop or Docker Engine is also installed, repeat Step 3 with `docker build` / `docker run`. Note any divergence in the commit message.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-plugin-base/Dockerfile crates/tau-plugin-base/.dockerignore
git commit -m "$(cat <<'EOF'
feat(plugin-base): tau-plugin-base Dockerfile (multi-stage build of tau-net-bridge)

T2 of sub-project I. Bakes tau-net-bridge into the base image so plugin
images don't need a runtime bind-mount of the bridge binary. Multi-stage
to avoid arch mismatch (bridge built for the image's arch, not host's).

Verified: builds successfully on Apple Silicon Podman; bridge binary
present at /usr/local/bin/tau-net-bridge.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

**Focused gate:**
- `podman build -f crates/tau-plugin-base/Dockerfile -t tau-plugin-base:dev .` exits 0.
- `podman image inspect tau-plugin-base:dev` shows the image exists.
- `podman run --rm tau-plugin-base:dev /usr/local/bin/tau-net-bridge --proxy-sock=/tmp/x --listen=127.0.0.1:1 -- /bin/true` produces bridge stderr (not "exec format error" / "no such file").

---

## Task 3: First plugin image — `shell-plugin` as proof of concept

**Files:**
- Create: `crates/tau-plugins/shell/Dockerfile`
- Create: `crates/tau-plugins/shell/.dockerignore`

**Why shell first.** Smallest plugin (1 binary, no native deps, no HTTP). Proves the per-plugin-image story end-to-end with the simplest possible plugin before generalising to the 4 others.

- [ ] **Step 1: Create the Dockerfile**

```dockerfile
# crates/tau-plugins/shell/Dockerfile
# syntax=docker/dockerfile:1.6

# ---------- Builder stage: compile shell-plugin ----------
FROM rust:1-bookworm AS builder

WORKDIR /workspace

COPY rust-toolchain.toml ./
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY xtask ./xtask

RUN cargo build --release -p tau-plugins-shell --bin shell-plugin

# ---------- Runtime stage ----------
FROM tau-plugin-base:dev

COPY --from=builder /workspace/target/release/shell-plugin /usr/local/bin/shell-plugin

USER tau

ENTRYPOINT ["/usr/local/bin/shell-plugin"]
```

- [ ] **Step 2: Create `.dockerignore`**

```
# crates/tau-plugins/shell/.dockerignore
target/
**/target/
.git/
.github/
docs/
*.md
```

- [ ] **Step 3: Build and run a smoke test**

```bash
# Working dir: workspace root. tau-plugin-base:dev must exist (T2).
podman build \
  -f crates/tau-plugins/shell/Dockerfile \
  -t tau-plugin-shell-plugin:dev \
  .

# Sanity: the image runs and the binary is callable. shell-plugin reads JSON-RPC
# from stdin; sending </dev/null produces an EOF read on its end and it exits.
# Important: no "no such file" error.
podman run --rm -i tau-plugin-shell-plugin:dev </dev/null 2>&1 | head -20
```

Expected: shell-plugin starts (may print a tracing log line about handshake), then exits because stdin closed before any JSON-RPC was sent. Output must be from the plugin itself, not from podman/docker about a missing executable.

- [ ] **Step 4: Reproduce the (previously failing) test scenario manually**

This is the proof that the image-based approach fixes the test. Build a JSON-RPC handshake by hand and pipe it into the container.

```bash
# Minimal handshake JSON-RPC frame (length-prefixed; verify exact format
# against crates/tau-plugin-protocol/src/frame.rs first if uncertain).
# If the frame format is too involved to write by hand, skip this Step and
# rely on T6's actual test invocation as the proof.
echo "SKIP: deferring full handshake reproduction to T6 test run"
```

- [ ] **Step 5: Commit**

```bash
git add crates/tau-plugins/shell/Dockerfile crates/tau-plugins/shell/.dockerignore
git commit -m "$(cat <<'EOF'
feat(plugins): shell plugin Dockerfile (proof-of-concept for sub-project I)

T3 of sub-project I. First per-plugin image. Multi-stage: builder stage
compiles shell-plugin from workspace source; runtime stage FROM
tau-plugin-base:dev with binary at /usr/local/bin/shell-plugin and
ENTRYPOINT set.

Verified: image builds; smoke run shows the binary executes (not
"no such file"). Full integration test pending T6 after the container
adapter is updated to resolve image names by convention.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

**Focused gate:**
- `podman build -f crates/tau-plugins/shell/Dockerfile -t tau-plugin-shell-plugin:dev .` exits 0.
- `podman run --rm -i tau-plugin-shell-plugin:dev </dev/null` runs the binary (output is plugin's own stderr, not a podman error).

---

## Task 4: `xtask` workspace member

**Files:**
- Create: `xtask/Cargo.toml`
- Create: `xtask/src/main.rs`
- Modify: `Cargo.toml` (workspace) — add `xtask` to `members`.
- Modify: `Cargo.lock` — gets touched automatically; commit it.

**Why xtask.** Per locked decision 8: `cargo xtask build-plugin-images` is the entry point for both local dev and CI. New workspace member.

- [ ] **Step 1: Add `xtask` to workspace members**

Modify `Cargo.toml` at the workspace root. Add `"xtask"` to the `members` array (after `crates/tau-sandbox-proxy`, alphabetical-ish):

```toml
members = [
    # ... existing crates ...
    "crates/tau-sandbox-proxy",
    "xtask",
]
```

- [ ] **Step 2: Create `xtask/Cargo.toml`**

```toml
# xtask/Cargo.toml
[package]
name = "xtask"
version.workspace = true
edition.workspace = true
publish = false

[dependencies]
anyhow = "1"
clap = { version = "4", features = ["derive"] }
```

(If `version.workspace = true` and `edition.workspace = true` aren't already in the workspace `[workspace.package]`, set explicit values: `version = "0.0.0"`, `edition = "2021"`.)

- [ ] **Step 3: Create `xtask/src/main.rs`**

```rust
//! Workspace task runner. Currently exposes:
//!
//! - `cargo xtask build-base-image` — builds `tau-plugin-base:dev`.
//! - `cargo xtask build-plugin-images [--name <bin>]` — builds the base if
//!   missing, then builds each plugin's image (or just the named one).
//!
//! Auto-detects the container runtime (podman first, docker fallback) by
//! probing `<runtime> --version` with a 2-second timeout. Mirrors the
//! convention from `tau-sandbox-container::probe`.

use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "Workspace task runner.")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Build the shared `tau-plugin-base:dev` image.
    BuildBaseImage,
    /// Build per-plugin images (all, or just `--name <bin>`).
    BuildPluginImages {
        /// Cargo `[[bin]]` name (e.g. `shell-plugin`). Builds all if omitted.
        #[arg(long)]
        name: Option<String>,
    },
}

/// All plugins shipped in-tree that get a Dockerfile in Phase 1.
const PLUGINS: &[&str] = &[
    "shell-plugin",
    "fs-read-plugin",
    "anthropic-plugin",
    "ollama-plugin",
    "openai-plugin",
];

fn main() -> Result<()> {
    let cli = Cli::parse();
    let runtime = detect_runtime()?;
    eprintln!("xtask: using container runtime `{runtime}`");

    match cli.command {
        Cmd::BuildBaseImage => build_base(&runtime),
        Cmd::BuildPluginImages { name } => match name {
            Some(n) => {
                ensure_base(&runtime)?;
                build_plugin(&runtime, &n)
            }
            None => {
                ensure_base(&runtime)?;
                for p in PLUGINS {
                    build_plugin(&runtime, p)?;
                }
                Ok(())
            }
        },
    }
}

/// Probe `podman` first, then `docker`. Mirrors PR #40's
/// `ContainerRuntime::Auto` ordering.
fn detect_runtime() -> Result<String> {
    for bin in ["podman", "docker"] {
        if probe_one(bin) {
            return Ok(bin.to_string());
        }
    }
    bail!("no container runtime found on PATH (tried podman, docker)")
}

fn probe_one(bin: &str) -> bool {
    let res = Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match res {
        Ok(c) => c,
        Err(_) => return false,
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(2) {
                    let _ = child.kill();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return false,
        }
    }
}

fn ensure_base(runtime: &str) -> Result<()> {
    // Always rebuild the base; the buildx cache makes warm rebuilds cheap.
    build_base(runtime)
}

/// Build args helper. Honors `BUILDX_CACHE_FROM` / `BUILDX_CACHE_TO`
/// env vars (set by CI). If unset, builds with default (no GHA cache).
/// Both directives must be a single `type=...` string each, e.g.
/// `BUILDX_CACHE_FROM=type=gha`.
fn build_args(dockerfile: &str, tag: &str) -> Vec<String> {
    let mut args = vec!["build".to_string()];
    if let Ok(v) = std::env::var("BUILDX_CACHE_FROM") {
        if !v.is_empty() {
            args.push(format!("--cache-from={v}"));
        }
    }
    if let Ok(v) = std::env::var("BUILDX_CACHE_TO") {
        if !v.is_empty() {
            args.push(format!("--cache-to={v}"));
        }
    }
    args.push("-f".to_string());
    args.push(dockerfile.to_string());
    args.push("-t".to_string());
    args.push(tag.to_string());
    args.push(".".to_string());
    args
}

fn build_base(runtime: &str) -> Result<()> {
    eprintln!("xtask: building tau-plugin-base:dev ...");
    let args = build_args("crates/tau-plugin-base/Dockerfile", "tau-plugin-base:dev");
    let status = Command::new(runtime)
        .args(&args)
        .status()
        .with_context(|| format!("invoke {runtime} build (base image)"))?;
    if !status.success() {
        bail!("`{runtime} build` for tau-plugin-base failed (exit {status})");
    }
    Ok(())
}

fn build_plugin(runtime: &str, bin_name: &str) -> Result<()> {
    let crate_subdir = bin_name
        .strip_suffix("-plugin")
        .unwrap_or(bin_name);
    let dockerfile = format!("crates/tau-plugins/{crate_subdir}/Dockerfile");
    let tag = format!("tau-plugin-{bin_name}:dev");
    eprintln!("xtask: building {tag} ...");
    let args = build_args(&dockerfile, &tag);
    let status = Command::new(runtime)
        .args(&args)
        .status()
        .with_context(|| format!("invoke {runtime} build for {bin_name}"))?;
    if !status.success() {
        bail!("`{runtime} build` for {bin_name} failed (exit {status})");
    }
    Ok(())
}
```

- [ ] **Step 4: Build xtask**

```bash
timeout 180 env CARGO_INCREMENTAL=0 RUSTC_WRAPPER= CARGO_TARGET_DIR=target/agent-impl cargo build -p xtask
```

Expected: exits 0, produces `target/agent-impl/debug/xtask`.

- [ ] **Step 5: Smoke-test xtask end-to-end**

```bash
# Sanity check the runtime probe + base build path.
timeout 180 env CARGO_INCREMENTAL=0 RUSTC_WRAPPER= CARGO_TARGET_DIR=target/agent-impl cargo run -p xtask -- build-base-image

# Then build the shell plugin image we already built by hand in T3.
timeout 300 env CARGO_INCREMENTAL=0 RUSTC_WRAPPER= CARGO_TARGET_DIR=target/agent-impl cargo run -p xtask -- build-plugin-images --name shell-plugin
```

Expected: both succeed. xtask prints `xtask: using container runtime ...` and `xtask: building ... ...`. The base image and `tau-plugin-shell-plugin:dev` are present afterward (`podman image ls` shows them).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock xtask/
git commit -m "$(cat <<'EOF'
feat(xtask): build-plugin-images entry point (sub-project I T4)

New `xtask` workspace member with two subcommands:
  - build-base-image — builds tau-plugin-base:dev
  - build-plugin-images [--name <bin>] — builds the base if missing,
    then builds per-plugin images

Auto-detects podman/docker via 2-sec --version probe (podman first per
PR #40). 5 plugins tracked in a const array; each maps to
crates/tau-plugins/<crate>/Dockerfile via `bin.strip_suffix("-plugin")`.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

**Focused gate:**
- `cargo build -p xtask` succeeds.
- `cargo run -p xtask -- build-base-image` succeeds.
- `cargo run -p xtask -- build-plugin-images --name shell-plugin` succeeds.
- `podman image inspect tau-plugin-shell-plugin:dev` shows the image.

---

## Task 5: Update `tau-sandbox-container::runner::wrap_command`

**Files:**
- Modify: `crates/tau-sandbox-container/src/runner.rs` — image name resolution; drop bind-mounts; entrypoint override for HTTP plans.

- [ ] **Step 1: Read the current `runner.rs` to understand argv shape and unit tests.**

```bash
sed -n '1,80p' crates/tau-sandbox-container/src/runner.rs
```

Note: 25 unit tests in this file assert on argv shape. Most relevant for this change:
- `image_default_is_present_before_program` (around line 348)
- `proxy_args_added_when_network_http_present` (around line 419)
- All tests pass `ResolvedRuntime::Docker` and `/bin/echo` as the program.

- [ ] **Step 2: Replace `DEFAULT_BASE_IMAGE` with image-name resolver and update `build_run_args`**

In `crates/tau-sandbox-container/src/runner.rs`:

(a) Delete the `DEFAULT_BASE_IMAGE` const at lines 28-32.

(b) Replace `ProxyConfig` to drop `bridge_path`:

```rust
/// Proxy configuration for Network(Http) plans.
#[derive(Debug)]
pub(crate) struct ProxyConfig {
    /// Absolute path to the proxy Unix socket in the parent's temp dir.
    pub(crate) sock_path: PathBuf,
}
```

(c) In `wrap_command`, drop the `TAU_NET_BRIDGE_PATH` lookup and the `bridge_path` field. Replace the `#[cfg(unix)]` block where `proxy_config` is built:

```rust
#[cfg(unix)]
let (proxy_handle, proxy_config) = if has_network_http {
    let mut allowed_hosts: Vec<String> = Vec::new();
    for cap in &plan.capabilities {
        if let Capability::Network(NetCapability::Http { hosts, .. }) = cap {
            allowed_hosts.extend(hosts.iter().cloned());
        }
    }
    tau_sandbox_proxy::validate_hosts(&allowed_hosts).map_err(|e| SandboxError::Proxy {
        message: format!("host validation: {e}"),
    })?;
    let handle =
        tau_sandbox_proxy::spawn_proxy(allowed_hosts).map_err(|e| SandboxError::Proxy {
            message: format!("spawn_proxy: {e}"),
        })?;
    let sock_path = handle.sock_path().to_path_buf();
    let config = ProxyConfig { sock_path };
    (Some(handle), Some(config))
} else {
    (None, None)
};
```

(d) Update `build_run_args` signature to also receive the resolved image name (not the program path; image is per-plugin):

```rust
pub(crate) fn build_run_args(
    plan: &SandboxPlan,
    runtime: ResolvedRuntime,
    image: &str,
    program: &str,
    program_args: &[String],
    forwarded_envs: &[(String, String)],
    proxy: Option<&ProxyConfig>,
) -> Vec<String> {
    let _ = runtime;
    let mut argv: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "-i".into(),
        "--user".into(),
        "nobody".into(),
        "--cap-drop=ALL".into(),
        "--security-opt=no-new-privileges".into(),
        "--read-only".into(),
        "--pids-limit".into(),
        "256".into(),
        "--ipc=none".into(),
        "--tmpfs".into(),
        "/tmp:size=64m".into(),
    ];

    let has_http = plan
        .capabilities
        .iter()
        .any(|c| matches!(c, Capability::Network(NetCapability::Http { .. })));
    argv.push("--network".into());
    if has_http {
        argv.push("bridge".into());
    } else {
        argv.push("none".into());
    }

    for cap in &plan.capabilities {
        match cap {
            Capability::Filesystem(FsCapability::Read { paths, .. }) => {
                for p in paths {
                    let cleaned = clean_mount_path(p);
                    argv.push("-v".into());
                    argv.push(format!("{cleaned}:{cleaned}:ro"));
                }
            }
            Capability::Filesystem(FsCapability::Write { paths, .. }) => {
                for p in paths {
                    let cleaned = clean_mount_path(p);
                    argv.push("-v".into());
                    argv.push(format!("{cleaned}:{cleaned}:rw"));
                }
            }
            _ => {}
        }
    }

    if let Some(proxy_cfg) = proxy {
        let sock = proxy_cfg.sock_path.display().to_string();
        argv.push("-v".into());
        argv.push(format!("{sock}:/run/tau-proxy.sock:rw"));
        argv.push("-e".into());
        argv.push("HTTPS_PROXY=http://127.0.0.1:8443".into());
        // Override entrypoint to the bridge baked in tau-plugin-base.
        argv.push("--entrypoint=/usr/local/bin/tau-net-bridge".into());
    }

    for (k, v) in forwarded_envs {
        argv.push("-e".into());
        argv.push(format!("{k}={v}"));
    }

    argv.push(image.into());

    if proxy.is_some() {
        argv.push("--proxy-sock=/run/tau-proxy.sock".into());
        argv.push("--listen=127.0.0.1:8443".into());
        argv.push("--".into());
        argv.push(program.into());
        for a in program_args {
            argv.push(a.clone());
        }
    } else {
        // Image's ENTRYPOINT is the plugin binary; pass only program args
        // (caller may provide flags). The `program` parameter is the host
        // path to the plugin — we ignore it for non-HTTP plans because the
        // image already runs the right binary.
        for a in program_args {
            argv.push(a.clone());
        }
    }

    argv
}
```

(e) In `wrap_command`, resolve the image name from `cmd.get_program()` basename:

```rust
let original_program = cmd.get_program().to_string_lossy().into_owned();
let bin_name = std::path::Path::new(&original_program)
    .file_name()
    .and_then(|n| n.to_str())
    .ok_or_else(|| SandboxError::WrapFailed {
        message: format!("cannot derive plugin bin name from program path: {original_program}"),
    })?;
let image = format!("tau-plugin-{bin_name}:dev");
```

And then update the `argv = build_run_args(...)` call to pass `&image`:

```rust
let argv = build_run_args(
    plan,
    runtime,
    &image,
    &original_program,
    &original_args,
    &forwarded_envs,
    proxy_config.as_ref(),
);
```

- [ ] **Step 3: Update unit tests**

The 25 unit tests in `runner.rs` need to call `build_run_args` with the new `image` parameter. Replace each call site:

```rust
let argv = build_run_args(&plan, ResolvedRuntime::Docker, "/bin/echo", &[], &[], None);
```

with

```rust
let argv = build_run_args(
    &plan,
    ResolvedRuntime::Docker,
    "tau-plugin-test:dev",
    "/bin/echo",
    &[],
    &[],
    None,
);
```

Also update assertions that rely on `DEFAULT_BASE_IMAGE`:
- `image_default_is_present_before_program` should now assert that `"tau-plugin-test:dev"` appears in argv before the program.
- `proxy_args_added_when_network_http_present` should additionally assert `--entrypoint=/usr/local/bin/tau-net-bridge` is in argv.
- Drop any test asserting on the `tau-net-bridge` bind-mount (e.g. `-v ...tau-net-bridge:/usr/local/bin/tau-net-bridge:ro`); the bridge is no longer bind-mounted.

- [ ] **Step 4: Run focused tests**

```bash
timeout 180 env CARGO_INCREMENTAL=0 RUSTC_WRAPPER= CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p tau-sandbox-container --lib
```

Expected: all tests pass. Iterate until clean.

- [ ] **Step 5: Run clippy**

```bash
timeout 240 env CARGO_INCREMENTAL=0 RUSTC_WRAPPER= CARGO_TARGET_DIR=target/agent-impl cargo clippy -p tau-sandbox-container --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/tau-sandbox-container/src/runner.rs
git commit -m "$(cat <<'EOF'
feat(sandbox-container): per-plugin image resolution + drop bridge bind-mount

T5 of sub-project I. wrap_command now resolves the image as
`tau-plugin-<bin>:dev` from the Command's program path, instead of
running everything in a single hard-coded base image with the plugin
binary bind-mounted. The bridge is no longer bind-mounted: it lives in
tau-plugin-base, which every plugin's image extends.

For HTTP plans, --entrypoint=/usr/local/bin/tau-net-bridge overrides the
plugin image's ENTRYPOINT and the bridge args + plugin path are passed
positionally.

Removed: DEFAULT_BASE_IMAGE const, ProxyConfig::bridge_path,
TAU_NET_BRIDGE_PATH env-var lookup, host bridge bind-mount in argv.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

**Focused gate:**
- `cargo nextest run -p tau-sandbox-container --lib` passes (all 25-ish unit tests).
- `cargo clippy -p tau-sandbox-container --all-targets -- -D warnings` clean.

---

## Task 6: Roll out remaining plugin Dockerfiles + un-`#[ignore]` 5 tests

**Files:**
- Create: `crates/tau-plugins/fs-read/Dockerfile` + `.dockerignore`
- Create: `crates/tau-plugins/anthropic/Dockerfile` + `.dockerignore`
- Create: `crates/tau-plugins/ollama/Dockerfile` + `.dockerignore`
- Create: `crates/tau-plugins/openai/Dockerfile` + `.dockerignore`
- Modify: `crates/tau-plugin-compat/tests/layer4_container.rs` — un-`#[ignore]` 5 tests; add `image_present_or_skip` helper.

- [ ] **Step 1: Create the 4 remaining Dockerfiles**

For each plugin, the Dockerfile is identical to T3's `shell` Dockerfile except for the cargo crate / bin name. Template (substitute `<crate>` and `<bin>`):

```dockerfile
# crates/tau-plugins/<crate>/Dockerfile
# syntax=docker/dockerfile:1.6

FROM rust:1-bookworm AS builder
WORKDIR /workspace
COPY rust-toolchain.toml ./
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY xtask ./xtask
RUN cargo build --release -p tau-plugins-<crate> --bin <bin>

FROM tau-plugin-base:dev
COPY --from=builder /workspace/target/release/<bin> /usr/local/bin/<bin>
USER tau
ENTRYPOINT ["/usr/local/bin/<bin>"]
```

Substitutions:

| `<crate>` | `<bin>` | Path |
|---|---|---|
| `fs-read` | `fs-read-plugin` | `crates/tau-plugins/fs-read/Dockerfile` |
| `anthropic` | `anthropic-plugin` | `crates/tau-plugins/anthropic/Dockerfile` |
| `ollama` | `ollama-plugin` | `crates/tau-plugins/ollama/Dockerfile` |
| `openai` | `openai-plugin` | `crates/tau-plugins/openai/Dockerfile` |

Create matching `.dockerignore` (same content as T3's) for each.

- [ ] **Step 2: Build all 5 images via xtask**

```bash
timeout 600 env CARGO_INCREMENTAL=0 RUSTC_WRAPPER= CARGO_TARGET_DIR=target/agent-impl cargo run -p xtask -- build-plugin-images
```

Expected: all 5 images build. May take ~5 min cold; warm-cache rebuilds < 1 min.

- [ ] **Step 3: Add `image_present_or_skip` helper in `layer4_container.rs`**

In `crates/tau-plugin-compat/tests/layer4_container.rs`, after the existing `require_docker` helper, add:

```rust
/// Skip the test gracefully if the per-plugin image isn't built locally.
///
/// Returns `true` if the image is present, `false` (with an eprintln SKIP
/// message) if not. Tests should early-return when this returns `false`.
fn image_present_or_skip(bin_name: &str) -> bool {
    let tag = format!("tau-plugin-{bin_name}:dev");
    // Probe podman first, then docker (matching ContainerRuntime::Auto).
    for runtime in ["podman", "docker"] {
        let out = std::process::Command::new(runtime)
            .args(["image", "inspect", &tag])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if matches!(out, Ok(s) if s.success()) {
            return true;
        }
    }
    eprintln!(
        "SKIP: {tag} not present locally; run `cargo xtask build-plugin-images --name {bin_name}` first"
    );
    false
}
```

- [ ] **Step 4: Un-`#[ignore]` the 5 tests and add image-presence skip checks**

For each of the 5 tests, **delete** the `#[ignore = "..."]` attribute and add an `image_present_or_skip` check at the top of the test body (after `require_docker`).

The 5 tests (verify line numbers against HEAD; spec-time positions are 251, 343, 465, 567, 658):

| Test fn | Bin |
|---|---|
| `shell_layer4_container_runs_echo_hello` | `shell-plugin` |
| `fs_read_layer4_container_reads_data_file` | `fs-read-plugin` |
| `anthropic_layer4_container_completes_via_cassette` | `anthropic-plugin` |
| `ollama_layer4_container_completes_via_cassette` | `ollama-plugin` |
| `openai_layer4_container_completes_via_cassette` | `openai-plugin` |

For each:

```rust
// REMOVE this attribute:
// #[ignore = "..."]

#[tokio::test]
async fn shell_layer4_container_runs_echo_hello() {
    if let Err(reason) = require_docker() {
        eprintln!("SKIP: {reason}");
        return;
    }
    if !image_present_or_skip("shell-plugin") {
        return;
    }
    // ... rest unchanged ...
}
```

- [ ] **Step 5: Run the 5 previously-ignored tests locally**

```bash
timeout 300 env CARGO_INCREMENTAL=0 RUSTC_WRAPPER= CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-plugin-compat \
  --features integration-tests \
  --test layer4_container 2>&1 | tail -40
```

Expected: all 5 previously-`#[ignore]`'d tests pass. Other tests in the file unchanged. Iterate on Dockerfiles or argv if tests still fail (debug stderr will surface concrete cause).

- [ ] **Step 6: Commit**

```bash
git add crates/tau-plugins/fs-read/Dockerfile crates/tau-plugins/fs-read/.dockerignore \
        crates/tau-plugins/anthropic/Dockerfile crates/tau-plugins/anthropic/.dockerignore \
        crates/tau-plugins/ollama/Dockerfile crates/tau-plugins/ollama/.dockerignore \
        crates/tau-plugins/openai/Dockerfile crates/tau-plugins/openai/.dockerignore \
        crates/tau-plugin-compat/tests/layer4_container.rs

git commit -m "$(cat <<'EOF'
feat(plugins+plugin-compat): 4 plugin Dockerfiles + un-#[ignore] 5 container tests

T6 of sub-project I. Each of fs-read, anthropic, ollama, openai gets a
multi-stage Dockerfile mirroring the shell plugin's. layer4_container.rs
gains an `image_present_or_skip(bin)` helper and the 5 previously-
#[ignore]'d tests un-#[ignore] with skip checks (gracefully passes if
the per-plugin image isn't built locally).

Verified: all 5 tests pass locally on Apple Silicon Podman after
`cargo xtask build-plugin-images`.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

**Focused gate:**
- All 5 plugin images build via xtask.
- `cargo nextest run -p tau-plugin-compat --features integration-tests --test layer4_container` shows 5 newly-passing tests (formerly ignored).

---

## Task 7: CI integration — buildx cache + xtask invocation

**Files:**
- Modify: `.github/workflows/ci.yml` — `test-tau-plugin-compat` job gets a buildx setup step + `cargo xtask build-plugin-images` step before the nextest invocation.

- [ ] **Step 1: Read the current job (lines 241-279) to know exactly where to insert**

```bash
sed -n '241,280p' .github/workflows/ci.yml
```

- [ ] **Step 2: Insert buildx setup + xtask step**

Modify `.github/workflows/ci.yml`. In the `test-tau-plugin-compat` job (around line 241), after the `Build tau binary (debug)` step and before the `Test tau-plugin-compat` step, insert:

```yaml
      - name: Set up Docker buildx
        uses: docker/setup-buildx-action@v3
        with:
          driver: docker-container
      - name: Build per-plugin images (sub-project I)
        run: cargo run -p xtask -- build-plugin-images
        env:
          # xtask reads these and adds --cache-from / --cache-to flags to
          # each `docker build` call. type=gha uses the GitHub Actions
          # cache backend that buildx ships with.
          BUILDX_CACHE_FROM: type=gha
          BUILDX_CACHE_TO: type=gha,mode=max
          BUILDKIT_PROGRESS: plain
          DOCKER_BUILDKIT: "1"
```

Then, in the same job's `Test tau-plugin-compat` step (the one that runs `cargo nextest run`), no change needed — the tests will skip if images aren't present, and they'll be present after the xtask step.

- [ ] **Step 3: yaml lint**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))" && echo OK
```

Expected: `OK`.

- [ ] **Step 4: Verify rustfmt + clippy still pass**

```bash
timeout 30 cargo fmt --all -- --check
timeout 240 env CARGO_INCREMENTAL=0 RUSTC_WRAPPER= CARGO_TARGET_DIR=target/agent-impl cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both clean. (`cargo fmt` covers all Rust files including new xtask; clippy runs over the whole workspace including xtask.)

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "$(cat <<'EOF'
ci: build per-plugin images before plugin-compat tests (sub-project I T7)

Adds buildx setup + `cargo xtask build-plugin-images` step before the
test-tau-plugin-compat job runs nextest. Buildx GHA cache configured via
setup-buildx-action@v3.

GHA cache scoping uses buildx defaults (cache scoped per workflow + ref).
Cold-cache CI run adds ~3-5 min for 6 image builds (1 base + 5 plugins);
warm-cache adds ~30s.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

**Focused gate:**
- `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"` exits 0.
- `cargo fmt --all -- --check` clean.
- `cargo clippy --workspace --all-targets -- -D warnings` clean.

---

## Task 8: Documentation — ADR + gap-row close + CONTRIBUTING note

**Files:**
- Create: `docs/decisions/0021-per-plugin-images.md`
- Modify: `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` — close 2 gap rows.
- Modify: `CONTRIBUTING.md` — add a note about `cargo xtask build-plugin-images` prereq for container tests.

- [ ] **Step 1: Write ADR-0021**

Create `docs/decisions/0021-per-plugin-images.md`:

```markdown
# ADR-0021: per-plugin container images

**Status:** Accepted (2026-05-08)

**Supersedes:** the implicit "single base image + bind-mounted plugin binary"
approach used by the Container adapter through sub-project H.

## Context

The Container sandbox adapter spawned plugins via `docker run <base-image>
<host-path-to-plugin>`. The host path doesn't exist inside the container, so
exec failed and the plugin host saw EOF before the plugin's handshake could
be sent. Five integration tests in `crates/tau-plugin-compat/tests/
layer4_container.rs` were `#[ignore]`'d — three from sub-project H's HTTP
work, two from sub-project D's earlier fs/shell work — all sharing this
symptom.

## Decision

Replace the bind-mount approach with **per-plugin Docker images** built on a
shared `tau-plugin-base` image. Each plugin has its own multi-stage
Dockerfile. The Container adapter resolves the image name as
`tau-plugin-<bin>:dev` from the `Command`'s program path, runs it, and
overrides ENTRYPOINT to `tau-net-bridge` for HTTP plans.

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
- ADR-0020 (sandbox proxy) unchanged. ADR-0019 (per-host net filter) remains
  superseded.

See `docs/superpowers/specs/2026-05-08-per-plugin-images-design.md` for the
full design including locked decisions 1-8 and Phase 1 risks.
```

- [ ] **Step 2: Close 2 gap rows in `sandboxing-followups.md`**

Find the gap row in `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` that mentions `Container-adapter HTTP plugin tests` (around line 403), and the older row from sub-project D about `shell` and `fs-read` Container tests. For each, append at the end of the row's "Status" cell:

```
**Closed by sub-project I (PR #<TBD>) on 2026-05-08.**
```

The PR number will be filled in at T9; for now use `<TBD>`.

- [ ] **Step 3: Add CONTRIBUTING note**

Find or create a section in `CONTRIBUTING.md` about local testing. Add:

```markdown
## Running container-sandbox tests locally

The `tau-plugin-compat` integration tests under
`tests/layer4_container.rs` require per-plugin Docker images to be built
locally. Run once after pulling, and again whenever you edit a plugin's
source:

    cargo xtask build-plugin-images          # all 5 plugins
    cargo xtask build-plugin-images --name shell-plugin   # just one

Tests skip gracefully (with a hint message) if the relevant image is not
present.
```

- [ ] **Step 4: Verify docs render and rustfmt is unaffected**

```bash
timeout 30 cargo fmt --all -- --check
```

- [ ] **Step 5: Commit**

```bash
git add docs/decisions/0021-per-plugin-images.md \
        docs/superpowers/specs/2026-05-03-sandboxing-followups.md \
        CONTRIBUTING.md
git commit -m "$(cat <<'EOF'
docs: ADR-0021 per-plugin images + close 2 gap rows + CONTRIBUTING note

T8 of sub-project I. ADR-0021 documents the per-plugin-image decision and
sketches the four-phase roadmap (this PR = Phase 1). Two gap rows in
sandboxing-followups.md close: sub-project H's "Container-adapter HTTP
plugin tests" and sub-project D's "shell + fs-read Container tests".
CONTRIBUTING gets a "Running container-sandbox tests locally" section
explaining the xtask prereq.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

**Focused gate:**
- `cargo fmt --all -- --check` clean.
- ADR-0021 file exists and renders as markdown.
- 2 gap rows in `sandboxing-followups.md` mention the close.

---

## Task 9: USER GATE — open PR, monitor CI

**Files:** none (push + PR open).

- [ ] **Step 1: Push branch**

```bash
git push -u origin feat/per-plugin-images
```

- [ ] **Step 2: Open PR**

```bash
gh pr create --title "feat(sandbox-container): per-plugin Docker images (sub-project I, Phase 1)" --body "$(cat <<'EOF'
## Summary

- Replace the broken "single base image + bind-mount host plugin binary" Container-adapter flow with per-plugin Docker images. Each plugin gets a multi-stage Dockerfile (builder compiles plugin from workspace source; runtime stage `FROM tau-plugin-base:dev` + ENTRYPOINT). Shared base bakes `tau-net-bridge` + `ca-certificates`.
- New `xtask` workspace member with `build-plugin-images` subcommand. CI calls the same xtask under `docker/setup-buildx-action@v3` with GHA cache.
- Un-`#[ignore]` 5 Container-adapter integration tests (3 from sub-project H, 2 from sub-project D).
- ADR-0021 documents the four-phase roadmap; Phases 2-4 are sketched but explicitly out of scope.

Closes 2 gap rows in `sandboxing-followups.md`.

## Test plan
- [ ] All 5 previously-`#[ignore]`'d tests in `tau-plugin-compat::layer4_container` pass on Linux CI
- [ ] No regressions in `tau-plugin-compat::layer4_native` (host-binary path)
- [ ] No regressions in `tau-sandbox-native::strict_proxy` integration tests
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo fmt --all -- --check` clean
- [ ] Buildx GHA cache hits on subsequent CI runs (~30s vs cold ~3-5 min)

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 3: Watch CI to completion**

```bash
gh pr checks <pr-number> --watch --fail-fast 2>&1 | tail -40
```

Expected: all checks green. Surface any new check name added (the buildx step might add a check; if so, ask the user about updating branch protection).

- [ ] **Step 4: PAUSE — request user review**

Print PR URL. Wait for the user to either approve squash-merge (T10) or request changes. Do not merge without explicit user instruction.

**Focused gate (USER GATE):**
- PR opens; URL surfaced to user.
- Full CI matrix passes.
- User explicitly requests merge before T10.

---

## Task 10: USER GATE — squash-merge + memory update

**Files:** none (merge + memory file).

- [ ] **Step 1: Squash-merge after user approval**

```bash
gh pr merge <pr-number> --squash --delete-branch
git checkout main
git pull --ff-only
```

- [ ] **Step 2: Update memory**

Edit `/Users/titouanlebocq/.claude/projects/-Users-titouanlebocq-code-tau/memory/project_sandbox_proxy_2026_05_08.md` (the H-iteration memory). Add to its "Branches/PRs" section:

```
- PR #<N> (sub-project I Phase 1, per-plugin images): merged 2026-05-08 at <sha> ← Container adapter now uses per-plugin Docker images; closes the 5 previously-#[ignore]'d Container-adapter plugin tests.
```

Update the file's `name` and `description` frontmatter to reflect the I-phase landing.

- [ ] **Step 3: Update `MEMORY.md` index** if title changed.

**Focused gate (USER GATE):**
- PR merged; main synced; branch deleted.
- Memory file updated to reflect sub-project I shipping.
- Index entry in `MEMORY.md` accurate.

---

## Future work (NOT this PR)

Captured in spec's "Out of scope" section and ADR-0021's Phase 2-4 sketch:

- **Phase 2 — Native-deps plugins.** Image build infra grows to handle
  `apt-get install`, `pip install` patterns when the first plugin with
  non-Rust deps lands.
- **Phase 3 — Public plugin SDK.** Manifest schema gains
  `[sandbox.container] image = "..."` override; plugin-authoring guide;
  image conventions become the public contract; plugin install becomes
  "pull image".
- **Phase 4 — Production-grade distribution.** GHCR push pipeline;
  sigstore signing; SBOM generation; multi-arch matrix; distroless base
  swap; plugin lockfile pins image digest; image-only deployment story.

Each is its own future sub-project with its own spec and plan.
