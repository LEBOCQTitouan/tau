# Layer 4 plugin-compat startup-IO Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the 5 `#[ignore]`'d tests in `crates/tau-plugin-compat/tests/layer4_native.rs` actually run by extending the runtime's landlock baseline with universally-needed paths and adding a per-plugin fixture helper for plugin-specific startup-IO.

**Architecture:** Two PRs. PR 1 (`feat/layer4-startup-io-baseline`) extends `tau-sandbox-native::light::install_landlock`'s `system_read_paths` baseline with paths every Rust binary needs (e.g. `/proc/self`, `/dev/urandom`), introduces a `tau_plugin_compat::startup_io` module with a `startup_io_paths_for(plugin_bin)` helper, and un-`#[ignore]`s the 2 simple plugins (shell + fs-read). PR 2 (`feat/layer4-startup-io-http`) populates the helper for the 3 HTTP plugins and un-`#[ignore]`s those tests.

**Tech Stack:** Rust 2021, landlock V1, `tau-sandbox-native` (Linux landlock + seccomp adapter), `tau-plugin-compat` (Layer 4 driver), nextest for test execution, lefthook + Podman for pre-push gate.

**Spec:** `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md` (committed at `7063ad7`).

---

## Pre-flight checks (apply to every task)

- BASE_SHA = `7063ad7`. If a test is failing, verify it failed at this SHA before claiming "pre-existing failure".
- All cargo invocations use `timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl cargo nextest run -p <crate>` (subagent) or `target/main` (main agent). Per CLAUDE.md.
- If sccache fails with EPERM, prefix with `RUSTC_WRAPPER=` to clear it.
- Per-task focused gate (single-crate nextest); full workspace test only at T5/T10 USER GATEs.
- Investigation tasks (T1, T7) emit findings to a new "Investigation findings" section in `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md`. NO code commit — spec edit only.

---

## File structure

| File | Responsibility | PR |
|---|---|---|
| `crates/tau-sandbox-native/src/light.rs` (MODIFY) | Hosts the landlock baseline. Extends `system_read_paths` with new entries + per-entry justification comments. Tests live in the same file's `mod tests`. | 1 |
| `crates/tau-plugin-compat/src/startup_io.rs` (NEW) | Per-plugin startup-IO helper. Single public `startup_io_paths_for(plugin_bin: &str) -> Vec<&'static str>`. Empty match arms for shell + fs-read in PR 1; HTTP plugin arms populated in PR 2. | 1 (created), 2 (extended) |
| `crates/tau-plugin-compat/src/lib.rs` (MODIFY) | Add `pub mod startup_io;` export. | 1 |
| `crates/tau-plugin-compat/tests/layer4_native.rs` (MODIFY) | Un-`#[ignore]` 2 tests in PR 1 (shell + fs-read), 3 tests in PR 2 (HTTP plugins). Each test wires the helper into its plan. | 1, 2 |
| `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md` (MODIFY) | Append "Investigation findings" section at the end with discovered paths + reasoning per plugin. | 1, 2 |

---

# PR 1 — `feat/layer4-startup-io-baseline`

## Task 1: Investigation (HARD GATE) — derive baseline candidate paths for shell + fs-read

**Files:**
- Modify: `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md` (append "Investigation findings" section)

**No code commit.** Spec edit only. Subagent emits findings; main agent reviews before unblocking T2.

- [ ] **Step 1: Run the shell test as-is to confirm the EOF symptom**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-plugin-compat --test layer4_native \
    shell_layer4_native_runs_echo_hello -- --include-ignored 2>&1 | tail -40
```

Expected: test fails with `PluginHandshakeFailed: EOF before handshake response` or similar. Record the exact error message in the findings section.

- [ ] **Step 2: Apply the analytical candidate baseline locally and re-run**

Add the candidate paths to `crates/tau-sandbox-native/src/light.rs`'s `system_read_paths` array (temporary local edit — do NOT commit yet):

```rust
let system_read_paths: &[&str] = &[
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/lib",
    "/lib64",
    "/usr/lib",
    "/usr/lib64",
    "/etc",
    // Candidate baseline additions for investigation:
    "/proc/self",
    "/proc/sys/kernel",
    "/sys/devices/system/cpu",
    "/dev/urandom",
    "/dev/null",
];
```

Re-run the shell test:

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-plugin-compat --test layer4_native \
    shell_layer4_native_runs_echo_hello -- --include-ignored 2>&1 | tail -40
```

If it passes: candidate baseline is sufficient for shell. Record the working set.
If it fails: proceed to Step 3 (strace fallback).

- [ ] **Step 3 (fallback): Strace-in-Podman to identify missed paths**

Only if Step 2 didn't succeed. Run inside the lefthook Podman gate's container:

```bash
podman run --rm -v $PWD:/work -w /work \
  -e CARGO_INCREMENTAL=0 \
  -e CARGO_TARGET_DIR=/work/target/podman \
  ghcr.io/lebocqtitouan/tau-podman-gate:latest \
  bash -c "apt-get install -y strace && \
    cargo build -p tau-plugins-shell --release && \
    strace -f -e trace=openat -o /tmp/trace.txt \
      target/podman/release/shell-plugin & \
    sleep 2; kill %1; \
    grep '\"/' /tmp/trace.txt | awk -F'\"' '{print \$2}' | sort -u"
```

Record discovered paths in the findings section. Categorize as:
- universal (every Rust binary): goes in `light.rs` baseline
- plugin-specific: goes in `startup_io.rs` (PR 2 territory for HTTP plugins; for shell/fs-read should be empty)

- [ ] **Step 4: Repeat Steps 2-3 for fs-read**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-plugin-compat --test layer4_native \
    fs_read_layer4_native_reads_data_file -- --include-ignored 2>&1 | tail -40
```

Document any fs-read-specific paths. If none beyond the shell baseline, note that explicitly.

- [ ] **Step 5: Revert the temporary edit to light.rs**

```bash
git checkout -- crates/tau-sandbox-native/src/light.rs
```

The "real" T2 commit will reintroduce the paths with proper comments.

- [ ] **Step 6: Append findings to spec**

Append a new section to `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md`:

```markdown
## Investigation findings

### PR 1 (shell + fs-read)

**Date:** 2026-05-09. **Investigator:** [agent-id].

**EOF symptom:** [paste exact error from Step 1]

**Discovered baseline paths (universal — go into `light.rs::system_read_paths`):**

| Path | Why | Source (test? strace?) |
|---|---|---|
| /proc/self | tokio runtime introspection (thread count, status) | analytical + observed in strace |
| /dev/urandom | rand crate, getrandom fallback | analytical |
| ... | ... | ... |

**Plugin-specific paths (go into `startup_io.rs`):**

- shell: none
- fs-read: none

**Outcome:** Analytical candidate baseline [was / was not] sufficient. [Strace was / was not] needed. Both shell + fs-read tests pass with the documented baseline.
```

- [ ] **Step 7: Commit the spec edit**

```bash
git add docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md
git commit -m "docs(spec): T1 investigation findings — baseline paths for shell + fs-read

Per spec's Investigation strategy section. Documents the path set
discovered through the analytical → strace fallback flow. Locks
the baseline that T2 implements.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

**HARD GATE:** Main agent reviews findings before T2. If the analytical baseline AND strace both fail to surface a working path set, escalate to user — likely indicates a non-IO issue (seccomp denial, signal handling, pre_exec failure) that needs separate debugging.

---

## Task 2: Extend `light.rs::system_read_paths` with discovered baseline

**Files:**
- Modify: `crates/tau-sandbox-native/src/light.rs:226-236` (the `system_read_paths` array)
- Modify: `crates/tau-sandbox-native/src/light.rs` (add unit tests in `mod tests`)

- [ ] **Step 1: Refactor `system_read_paths` to a top-level constant for testability**

Replace the inline array at `light.rs:226-236` with a constant, and reference the constant from `install_landlock`:

```rust
// Near the top of light.rs (after imports):

/// Baseline filesystem paths that EVERY plugin needs read access to under
/// landlock. These are runtime mechanics (binary load, dyld, libc, kernel
/// introspection, entropy) — not application data. The user's plan-derived
/// `read_paths` still narrow application access; these system paths exist
/// purely so the runtime mechanics work.
///
/// Each entry must be justified — Constitution G12 wants narrow defaults.
/// Add a one-line comment explaining why a path is in the baseline before
/// extending this list.
pub(crate) const BASELINE_SYSTEM_READ_PATHS: &[&str] = &[
    // Binary load + dyld + libc (priority-12 baseline)
    "/bin",                    // shell + fs-read locate echo, basic utilities
    "/sbin",                   // distro-dependent /bin/sbin split
    "/usr/bin",                // post-merge layout (Debian-merged-/usr)
    "/usr/sbin",               // post-merge layout
    "/lib",                    // distro-dependent /lib /usr/lib split (libc, libm, libdl)
    "/lib64",                  // 64-bit dyld on glibc systems
    "/usr/lib",                // post-merge layout
    "/usr/lib64",              // post-merge layout
    "/etc",                    // /etc/resolv.conf for DNS, /etc/ssl/certs for TLS roots, locale config
    // Sub-project layer4-startup-io baseline additions (2026-05-09):
    // [Replace this comment block with the actual paths discovered in T1.
    //  Each path on its own line with a // comment explaining why.]
];
```

Then in `install_landlock` (around line 226), replace the inline `let system_read_paths` with a reference to the constant:

```rust
// In install_landlock, replace:
//     let system_read_paths: &[&str] = &[ ... ];
// with:
let system_read_paths: &[&str] = BASELINE_SYSTEM_READ_PATHS;
```

The `for sys_path in system_read_paths { ... }` loop below stays unchanged.

- [ ] **Step 2: Populate the new entries based on T1 findings**

For each path discovered in T1's findings table that's universal (not plugin-specific), add it to `BASELINE_SYSTEM_READ_PATHS` with a one-line comment. Example structure (adapt to actual findings):

```rust
    // Sub-project layer4-startup-io baseline additions (2026-05-09):
    "/proc/self",                       // tokio thread-count introspection, std::process::id, mio
    "/proc/sys/kernel",                 // tokio probes for kernel feature support
    "/sys/devices/system/cpu",          // num_cpus crate (tokio runtime sizing)
    "/dev/urandom",                     // rand crate, getrandom fallback
    "/dev/null",                        // std::process closed-stdio sentinel
```

If T1 found additional paths, add them here. If T1 found a path is NOT needed, do NOT add it (YAGNI).

- [ ] **Step 3: Add unit test asserting baseline content**

In `crates/tau-sandbox-native/src/light.rs`'s `#[cfg(test)] mod tests` block (existing), add:

```rust
#[test]
fn baseline_system_read_paths_includes_legacy_entries() {
    use super::BASELINE_SYSTEM_READ_PATHS;

    // Priority-12 baseline must remain (regression protection).
    let expected_legacy = ["/bin", "/sbin", "/usr/bin", "/usr/sbin",
                          "/lib", "/lib64", "/usr/lib", "/usr/lib64", "/etc"];
    for p in expected_legacy {
        assert!(
            BASELINE_SYSTEM_READ_PATHS.contains(&p),
            "legacy baseline path {p} must remain in BASELINE_SYSTEM_READ_PATHS"
        );
    }
}

#[test]
fn baseline_system_read_paths_includes_runtime_mechanics() {
    use super::BASELINE_SYSTEM_READ_PATHS;

    // Sub-project layer4-startup-io baseline additions (regression protection).
    // Adjust this list to match the actual T1 findings.
    let expected_new = ["/proc/self", "/dev/urandom", "/dev/null"];
    for p in expected_new {
        assert!(
            BASELINE_SYSTEM_READ_PATHS.contains(&p),
            "runtime-mechanics baseline path {p} must be in BASELINE_SYSTEM_READ_PATHS"
        );
    }
}

#[test]
fn baseline_system_read_paths_no_application_data() {
    use super::BASELINE_SYSTEM_READ_PATHS;

    // Constitution G12: baseline is for runtime mechanics, not app data.
    // Reject paths that would let plugins read user data without an explicit
    // FsCapability::Read grant.
    let forbidden = ["/home", "/root", "/var/lib", "/srv", "/opt", "/tmp",
                     "/mnt", "/media"];
    for p in forbidden {
        assert!(
            !BASELINE_SYSTEM_READ_PATHS.contains(&p),
            "{p} must NOT be in baseline (would expand sandbox beyond runtime mechanics)"
        );
    }
}
```

(Adjust `expected_new` to match the actual T1 findings.)

- [ ] **Step 4: Run the new + existing tests**

Run:

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-sandbox-native --lib 2>&1 | tail -30
```

Expected: 3 new tests pass; all existing `light.rs` tests pass.

- [ ] **Step 5: Run the e2e tests on Linux to confirm no regression**

If executing on a Linux host or via the Podman gate:

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-sandbox-native --features integration-tests \
  2>&1 | tail -30
```

If on darwin-arm64 host: skip — the e2e tests are Linux-only and will be exercised by the lefthook gate at T5. Document this in the commit message.

- [ ] **Step 6: Commit**

```bash
git add crates/tau-sandbox-native/src/light.rs
git commit -m "feat(sandbox-native): extend landlock baseline with runtime-mechanics paths

Adds /proc/self, /dev/urandom, /dev/null (and any others discovered in
T1) to BASELINE_SYSTEM_READ_PATHS. Refactors the previously-inline array
into a top-level constant so the baseline content can be unit-tested
directly.

Each new entry has a one-line justifying comment per Constitution G12.
Runtime mechanics, not application data — three regression tests
enforce this invariant:

- baseline_system_read_paths_includes_legacy_entries
- baseline_system_read_paths_includes_runtime_mechanics
- baseline_system_read_paths_no_application_data

Closes the production gap that previously caused plugins running under
strict tier to EOF before handshake (their startup-IO surface wasn't in
the SandboxPlan).

Spec: docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md
T1 findings appended on commit 7063ad7's predecessor.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 3: Create `tau-plugin-compat/src/startup_io.rs`

**Files:**
- Create: `crates/tau-plugin-compat/src/startup_io.rs`
- Modify: `crates/tau-plugin-compat/src/lib.rs` (add `pub mod startup_io;`)

- [ ] **Step 1: Create the new module**

Write to `crates/tau-plugin-compat/src/startup_io.rs`:

```rust
//! Per-plugin startup-IO path helpers for Layer 4 tests.
//!
//! The `tau-sandbox-native::light::BASELINE_SYSTEM_READ_PATHS` runtime
//! constant covers paths that every Rust binary needs (libc/dyld bootstrap,
//! `/proc/self`, `/dev/urandom`, etc.). This module supplies the long-tail
//! of plugin-specific paths beyond that baseline — for example, an HTTP
//! plugin that reads a distribution-specific TLS root cert bundle that
//! isn't covered by the `/etc` baseline entry.
//!
//! Tests in `tau-plugin-compat/tests/layer4_native.rs` call
//! `startup_io_paths_for(plugin_bin)` and add the returned paths as an
//! additional `cap_fs_read` entry in the test's SandboxPlan.

/// Return plugin-specific filesystem paths needed at startup that aren't
/// already covered by `tau-sandbox-native`'s
/// `BASELINE_SYSTEM_READ_PATHS`.
///
/// `plugin_bin` is the plugin binary's basename (e.g. `"shell-plugin"`,
/// `"anthropic-plugin"`). Unknown plugins return an empty slice — the
/// caller is responsible for either providing a binding here or relying
/// solely on the runtime baseline.
///
/// Empty arms for shell + fs-read in PR 1 reflect that those plugins
/// don't touch any plugin-specific paths beyond the runtime baseline
/// (T1 findings). HTTP plugin arms (anthropic, ollama, openai) are
/// populated in PR 2 (`feat/layer4-startup-io-http`).
pub fn startup_io_paths_for(plugin_bin: &str) -> &'static [&'static str] {
    match plugin_bin {
        // PR 1 — simple plugins. No plugin-specific paths needed beyond
        // the runtime baseline (per T1 findings).
        "shell-plugin" => &[],
        "fs-read-plugin" => &[],
        // PR 2 — HTTP plugins. Populated in feat/layer4-startup-io-http.
        "anthropic-plugin" => &[],
        "ollama-plugin" => &[],
        "openai-plugin" => &[],
        // Unknown plugin: caller bears responsibility.
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::startup_io_paths_for;

    #[test]
    fn shell_plugin_has_no_extras_in_pr1() {
        assert!(startup_io_paths_for("shell-plugin").is_empty(),
            "shell does not need plugin-specific startup paths in PR 1");
    }

    #[test]
    fn fs_read_plugin_has_no_extras_in_pr1() {
        assert!(startup_io_paths_for("fs-read-plugin").is_empty(),
            "fs-read does not need plugin-specific startup paths in PR 1");
    }

    #[test]
    fn unknown_plugin_returns_empty() {
        assert!(startup_io_paths_for("nonexistent-plugin").is_empty());
    }
}
```

- [ ] **Step 2: Export from `lib.rs`**

In `crates/tau-plugin-compat/src/lib.rs`, find the existing `pub mod driver;` line and add immediately after it:

```rust
pub mod startup_io;
```

(Verify the alphabetical-or-related grouping convention by reading the file first; insert appropriately.)

- [ ] **Step 3: Build the crate**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo build -p tau-plugin-compat 2>&1 | tail -10
```

Expected: clean compile.

- [ ] **Step 4: Run unit tests**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-plugin-compat --lib 2>&1 | tail -20
```

Expected: 3 new `startup_io` tests pass; existing `tau-plugin-compat` lib tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-plugin-compat/src/startup_io.rs crates/tau-plugin-compat/src/lib.rs
git commit -m "feat(plugin-compat): startup_io_paths_for helper for Layer 4 tests

New crates/tau-plugin-compat/src/startup_io.rs module. Single public
function startup_io_paths_for(plugin_bin) returning plugin-specific
filesystem paths needed at startup beyond the runtime baseline in
tau-sandbox-native::light::BASELINE_SYSTEM_READ_PATHS.

PR 1 ships empty match arms for all 5 plugins — shell + fs-read need
no plugin-specific paths per T1 findings; HTTP plugin arms (anthropic,
ollama, openai) are populated in PR 2 (feat/layer4-startup-io-http).

Lays the API surface so PR 2's per-plugin extras land as data-only
edits without touching the call sites in layer4_native.rs.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 4: Un-`#[ignore]` shell + fs-read tests, wire helper

**Files:**
- Modify: `crates/tau-plugin-compat/tests/layer4_native.rs:167` (shell test) — remove `#[ignore]`, wire helper
- Modify: `crates/tau-plugin-compat/tests/layer4_native.rs:264` (fs-read test) — remove `#[ignore]`, wire helper

- [ ] **Step 1: Update the shell test**

Find the block at `crates/tau-plugin-compat/tests/layer4_native.rs:166-194`:

```rust
#[tokio::test]
#[ignore = "Plugin EOFs before handshake under strict tier — needs fs.read for plugin's runtime state (config, tmp, /proc, etc.). Each plugin's startup I/O surface needs cataloging for proper plan derivation. Defer to a sub-project D follow-up that builds plugin-specific plans, or sub-project F."]
async fn shell_layer4_native_runs_echo_hello() {
    // ...
    let spawn_cap: Capability = domain_fixtures::cap_process_spawn(&["echo"]);
    let plan = SandboxPlan::new(vec![spawn_cap.clone()], None, None);
```

Replace with:

```rust
#[tokio::test]
async fn shell_layer4_native_runs_echo_hello() {
    // ...
    let spawn_cap: Capability = domain_fixtures::cap_process_spawn(&["echo"]);

    // Per-plugin startup-IO paths beyond the runtime baseline. Empty for
    // shell — runtime baseline (BASELINE_SYSTEM_READ_PATHS) is sufficient.
    let extras = tau_plugin_compat::startup_io::startup_io_paths_for("shell-plugin");
    let mut caps = vec![spawn_cap.clone()];
    if !extras.is_empty() {
        caps.push(domain_fixtures::cap_fs_read(extras));
    }
    let plan = SandboxPlan::new(caps, None, None);
```

(Two changes: remove the `#[ignore]` line; replace the plan construction with the new caps-vector form.)

- [ ] **Step 2: Update the fs-read test**

Find the block at `crates/tau-plugin-compat/tests/layer4_native.rs:263-296`:

```rust
#[tokio::test]
#[ignore = "Plugin EOFs before handshake under strict tier — needs fs.read for plugin's runtime state (config, tmp, /proc, etc.). Each plugin's startup I/O surface needs cataloging for proper plan derivation. Defer to a sub-project D follow-up that builds plugin-specific plans, or sub-project F."]
async fn fs_read_layer4_native_reads_data_file() {
    // ...
    let fs_read_cap: Capability = domain_fixtures::cap_fs_read(&[&tmpdir_glob]);
    let plan = SandboxPlan::new(vec![fs_read_cap.clone()], None, None);
```

Replace with:

```rust
#[tokio::test]
async fn fs_read_layer4_native_reads_data_file() {
    // ...
    let fs_read_cap: Capability = domain_fixtures::cap_fs_read(&[&tmpdir_glob]);

    // Per-plugin startup-IO paths beyond the runtime baseline. Empty for
    // fs-read — runtime baseline (BASELINE_SYSTEM_READ_PATHS) is sufficient.
    let extras = tau_plugin_compat::startup_io::startup_io_paths_for("fs-read-plugin");
    let mut caps = vec![fs_read_cap.clone()];
    if !extras.is_empty() {
        caps.push(domain_fixtures::cap_fs_read(extras));
    }
    let plan = SandboxPlan::new(caps, None, None);
```

(Two changes: remove the `#[ignore]` line; replace the plan construction.)

- [ ] **Step 3: Compile-check**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo build --tests -p tau-plugin-compat 2>&1 | tail -10
```

Expected: clean compile.

- [ ] **Step 4: Run the un-`#[ignore]`'d tests on Linux**

If on Linux directly:

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-plugin-compat --test layer4_native \
    shell_layer4_native_runs_echo_hello \
    fs_read_layer4_native_reads_data_file 2>&1 | tail -30
```

If on darwin-arm64 host (no native landlock), run via the lefthook Podman gate:

```bash
podman run --rm -v $PWD:/work -w /work \
  -e CARGO_INCREMENTAL=0 \
  -e CARGO_TARGET_DIR=/work/target/podman \
  ghcr.io/lebocqtitouan/tau-podman-gate:latest \
  bash -c "cargo nextest run -p tau-plugin-compat --test layer4_native \
    shell_layer4_native_runs_echo_hello \
    fs_read_layer4_native_reads_data_file"
```

Expected: both tests PASS.

If either FAILS: hard-stop. Re-run T1 to identify the missed path. Do NOT bypass with `#[ignore]` — that's the bug we're trying to close.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-plugin-compat/tests/layer4_native.rs
git commit -m "test(plugin-compat): un-#[ignore] shell + fs-read Layer 4 native tests

Both tests now pass on Linux thanks to:
- T2's tau-sandbox-native::light::BASELINE_SYSTEM_READ_PATHS extension
  (covers /proc/self, /dev/urandom, etc. — the runtime mechanics).
- T3's startup_io_paths_for helper (empty arms for both plugins; the
  runtime baseline is sufficient for them).

Each test now wires startup_io_paths_for in to lay the API surface
for PR 2 — the call form is identical for the HTTP plugins; only the
match arm content differs.

Closes 2 of 5 #[ignore]'d Layer 4 native tests. Remaining 3 (HTTP
plugins: anthropic, ollama, openai) close in PR 2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 5: USER GATE — open PR 1, monitor CI

**Main agent only — no subagent.**

- [ ] **Step 1: Verify clean working tree + correct branch**

```bash
git status && git log --oneline main..feat/layer4-startup-io-baseline
```

Expected: clean tree; 4 commits on the branch (T1 spec edit, T2 baseline, T3 startup_io module, T4 test un-ignore).

- [ ] **Step 2: Run lefthook pre-push gate**

```bash
git push -u origin feat/layer4-startup-io-baseline
```

This triggers the lefthook pre-push hook, which runs the 10 Linux CI jobs in a Podman container (~3-4 min warm, ~30-45 min cold).

If it fails for legitimate code reasons: fix forward, commit, retry push.
If it fails for environmental reasons (Podman VM disk full, etc.): use `--no-verify` ONLY after confirming with user.

- [ ] **Step 3: Open PR**

```bash
gh pr create --title "feat(sandbox-native): runtime-mechanics baseline + un-#[ignore] shell/fs-read Layer 4 tests" --body "$(cat <<'EOF'
## Summary

PR 1 of 2 closing the Layer 4 plugin-compat startup-IO gap. Closes 2 of 5 `#[ignore]`'d tests in `crates/tau-plugin-compat/tests/layer4_native.rs` (shell + fs-read). PR 2 will close the 3 HTTP plugin tests.

- **`tau-sandbox-native::light::BASELINE_SYSTEM_READ_PATHS`** — refactored from inline array to top-level constant, extended with runtime-mechanics paths every Rust binary needs (`/proc/self`, `/dev/urandom`, etc., per T1 investigation findings). 3 regression tests enforce the invariants (legacy entries remain; new entries present; no application-data paths leaked in).
- **`tau-plugin-compat::startup_io`** — new module with `startup_io_paths_for(plugin_bin)` helper. Empty arms for shell + fs-read; HTTP plugin arms populated in PR 2.
- **`layer4_native.rs`** — un-`#[ignore]`'d shell + fs-read tests; both wire the helper in (no-op for these two; lays API surface for PR 2).

Spec: `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md`. T1 investigation findings appended in the first commit.

## Test plan

- [ ] CI green on the 14 required checks (especially `test (tau-plugin-compat / linux)` and `test (tau-sandbox-native e2e / linux)`)
- [ ] Lefthook pre-push gate green locally
- [ ] Spec self-check: every new path in `BASELINE_SYSTEM_READ_PATHS` has a one-line justifying comment

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Monitor CI**

Use the Monitor tool with a poll loop on `gh pr checks <PR#> --json name,state` emitting a line per check transition out of pending. Pause for user approval before T6.

---

## Task 6: USER GATE — squash-merge PR 1

**Main agent only — no subagent.**

- [ ] **Step 1: Verify all 14 required checks green**

```bash
gh pr checks <PR#> --json name,state | jq '[.[] | select(.state != "SUCCESS")] | length'
```

Expected: `0`.

- [ ] **Step 2: Squash-merge**

```bash
gh pr merge <PR#> --squash --delete-branch
```

- [ ] **Step 3: Pull main**

```bash
git checkout main && git pull --ff-only && git log --oneline -3
```

Expected: top commit is the squash-merged PR 1.

---

# PR 2 — `feat/layer4-startup-io-http`

## Task 7: Investigation (HARD GATE) — identify HTTP plugin startup-IO

**Files:**
- Modify: `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md` (extend "Investigation findings" section)

**No code commit.** Spec edit only.

- [ ] **Step 1: Cut PR 2 branch from main**

```bash
git checkout main && git pull --ff-only
git checkout -b feat/layer4-startup-io-http
```

- [ ] **Step 2: Run the anthropic test as-is to confirm EOF symptom**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-plugin-compat --test layer4_native \
    anthropic_layer4_native_completes_via_cassette \
    -- --include-ignored 2>&1 | tail -40
```

Expected: still fails with EOF (PR 1's baseline closed simple plugins; HTTP plugins need TLS bootstrap paths beyond `/etc`).

- [ ] **Step 3: Strace the anthropic plugin's reqwest TLS init**

```bash
podman run --rm -v $PWD:/work -w /work \
  -e CARGO_INCREMENTAL=0 \
  -e CARGO_TARGET_DIR=/work/target/podman \
  ghcr.io/lebocqtitouan/tau-podman-gate:latest \
  bash -c "apt-get install -y strace && \
    cargo build -p tau-plugins-anthropic --release && \
    strace -f -e trace=openat -o /tmp/trace.txt \
      target/podman/release/anthropic-plugin & \
    sleep 5; kill %1; \
    grep '\"/' /tmp/trace.txt | awk -F'\"' '{print \$2}' | sort -u"
```

Look for paths NOT under `/bin /sbin /usr/bin /usr/sbin /lib /lib64 /usr/lib /usr/lib64 /etc /proc/self /dev/urandom /dev/null` etc. Common culprits:

- `/etc/ssl/openssl.cnf` (covered by `/etc`)
- `/etc/ca-certificates/extracted/` (covered by `/etc`)
- `~/.config/` user config (would need explicit grant — likely NOT needed in test mode)
- `/usr/share/ca-certificates/` (NOT in baseline — likely needs adding)
- `/var/lib/ca-certificates/` (NOT in baseline — likely needs adding)
- `/etc/pki/` (covered by `/etc`)

Record discovered paths.

- [ ] **Step 4: Repeat for ollama and openai**

Same strace flow, plugin binary path adjusted. Expect significant overlap (all three use reqwest).

- [ ] **Step 5: Categorize findings**

Per spec's hybrid model:

- Universal (every reqwest user) → goes into `BASELINE_SYSTEM_READ_PATHS` as a follow-up commit on PR 2 if needed (or defer to a separate small PR).
- Plugin-specific (anthropic-only paths, etc.) → goes into `startup_io_paths_for("anthropic-plugin")` etc.

Default lean: prefer plugin-specific match arms over baseline expansion (YAGNI; if all 3 plugins need the same path, then move to baseline).

- [ ] **Step 6: Append findings to spec**

Append to the "Investigation findings" section:

```markdown
### PR 2 (anthropic, ollama, openai)

**Date:** 2026-05-09. **Investigator:** [agent-id].

**EOF symptom (anthropic):** [paste exact error]

**Discovered HTTP plugin paths:**

| Path | Plugin(s) | Why | Universal or specific? |
|---|---|---|---|
| /usr/share/ca-certificates | all 3 | reqwest TLS root cert bundle (Debian-style) | universal — extend baseline |
| ... | ... | ... | ... |

**Plugin-specific paths (per match arm):**

- anthropic-plugin: [paths or "none"]
- ollama-plugin: [paths or "none"]
- openai-plugin: [paths or "none"]

**Outcome:** [Strace surfaced N additional paths. M go in baseline (universal), K go in startup_io match arms (plugin-specific).]
```

- [ ] **Step 7: Commit the spec edit**

```bash
git add docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md
git commit -m "docs(spec): T7 investigation findings — HTTP plugin startup-IO

Per spec's Investigation strategy section. Documents the additional
path set discovered for the 3 HTTP plugins (anthropic, ollama, openai)
beyond PR 1's runtime baseline. Locks the helper match arm content
that T8 implements.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

**HARD GATE:** Main agent reviews findings before T8. If reqwest's TLS init touches paths that aren't bind-able under landlock V1 (e.g. dynamic-named per-process paths), escalate to user — may need rustls + webpki-roots compiled-in switch instead of native-tls.

---

## Task 8: Populate `startup_io_paths_for` for HTTP plugins (+ baseline extension if needed)

**Files:**
- Modify: `crates/tau-plugin-compat/src/startup_io.rs` (HTTP plugin arms)
- (Optional) Modify: `crates/tau-sandbox-native/src/light.rs` (extend `BASELINE_SYSTEM_READ_PATHS` if T7 found universal HTTP paths)

- [ ] **Step 1: Update `startup_io.rs` HTTP plugin match arms**

Replace the empty arms in `startup_io_paths_for` with the discovered paths. Example structure (adapt to actual T7 findings):

```rust
        // PR 2 — HTTP plugins. reqwest TLS bootstrap paths beyond the
        // runtime baseline. Per T7 investigation findings:
        "anthropic-plugin" => &[
            // [Replace with actual paths from T7. Keep one-line comment per path
            //  explaining the why, just like the baseline.]
        ],
        "ollama-plugin" => &[
            // [...]
        ],
        "openai-plugin" => &[
            // [...]
        ],
```

If T7 found NO plugin-specific paths and ALL the new paths are universal, leave HTTP arms as `&[]` and add the universal paths to `BASELINE_SYSTEM_READ_PATHS` instead (Step 2).

- [ ] **Step 2 (conditional): Extend `BASELINE_SYSTEM_READ_PATHS` for universal HTTP paths**

Only if T7 found paths needed by ALL HTTP plugins. Add to `crates/tau-sandbox-native/src/light.rs`:

```rust
    // Sub-project layer4-startup-io HTTP baseline additions (2026-05-09):
    "/usr/share/ca-certificates", // reqwest TLS root bundle (Debian + Ubuntu)
    "/var/lib/ca-certificates",   // reqwest TLS root bundle (some distros' merged layout)
```

Update the `baseline_system_read_paths_includes_runtime_mechanics` test in `light.rs:mod tests` to include the new paths in `expected_new`.

- [ ] **Step 3: Update `startup_io.rs` tests**

Replace the PR 1 tests asserting `is_empty()` for HTTP plugins with content assertions:

```rust
#[test]
fn anthropic_plugin_has_tls_paths() {
    let paths = startup_io_paths_for("anthropic-plugin");
    // Expected paths derived from T7 findings.
    // Adjust this list to match what was actually populated above.
    assert!(!paths.is_empty(),
        "anthropic plugin needs at least the TLS root cert bundle path");
}

#[test]
fn ollama_plugin_has_tls_paths() {
    let paths = startup_io_paths_for("ollama-plugin");
    assert!(!paths.is_empty(),
        "ollama plugin needs at least the TLS root cert bundle path");
}

#[test]
fn openai_plugin_has_tls_paths() {
    let paths = startup_io_paths_for("openai-plugin");
    assert!(!paths.is_empty(),
        "openai plugin needs at least the TLS root cert bundle path");
}
```

(Or, if T7 found that ALL paths are universal and HTTP arms stay empty, leave the PR 1 `is_empty()` tests in place for HTTP plugins — but the tests in T9 will be the real load-bearing verification.)

- [ ] **Step 4: Build + run unit tests**

```bash
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-plugin-compat --lib 2>&1 | tail -20
timeout 300 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo nextest run -p tau-sandbox-native --lib 2>&1 | tail -20
```

Expected: both clean.

- [ ] **Step 5: Commit**

```bash
git add crates/tau-plugin-compat/src/startup_io.rs crates/tau-sandbox-native/src/light.rs
git commit -m "feat(plugin-compat): populate startup_io_paths_for HTTP plugins

Per T7 findings, the 3 HTTP plugins (anthropic, ollama, openai) each
need [N] additional read paths beyond PR 1's runtime baseline for
reqwest TLS init.

[Document the universal vs per-plugin split here. Universal paths
went into BASELINE_SYSTEM_READ_PATHS; plugin-specific went into
startup_io.rs match arms.]

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 9: Un-`#[ignore]` the 3 HTTP plugin tests

**Files:**
- Modify: `crates/tau-plugin-compat/tests/layer4_native.rs:452` (anthropic) — remove `#[ignore]`, wire helper
- Modify: `crates/tau-plugin-compat/tests/layer4_native.rs:549` (ollama) — remove `#[ignore]`, wire helper
- Modify: `crates/tau-plugin-compat/tests/layer4_native.rs:639` (openai) — remove `#[ignore]`, wire helper

- [ ] **Step 1: Update the anthropic test**

Find the block around line 449-470 (the anthropic test):

```rust
#[tokio::test]
#[ignore = "Plugin EOFs before handshake under strict tier — anthropic-plugin's HTTP client init touches state outside plan's read paths. Defer to a sub-project D follow-up that builds plugin-specific plans, or sub-project F."]
async fn anthropic_layer4_native_completes_via_cassette() {
    // ... existing setup ...
    let net_cap: Capability = domain_fixtures::cap_net_http(...);
    let plan = SandboxPlan::new(vec![net_cap.clone()], None, None);
```

(Verify the actual surrounding code by reading the file at the line first; the `cap_net_http` call signature may differ.)

Replace with:

```rust
#[tokio::test]
async fn anthropic_layer4_native_completes_via_cassette() {
    // ... existing setup ...
    let net_cap: Capability = domain_fixtures::cap_net_http(...);

    // Per-plugin startup-IO paths beyond the runtime baseline (reqwest TLS
    // init touches per-distro CA cert paths not covered by /etc).
    let extras = tau_plugin_compat::startup_io::startup_io_paths_for("anthropic-plugin");
    let mut caps = vec![net_cap.clone()];
    if !extras.is_empty() {
        caps.push(domain_fixtures::cap_fs_read(extras));
    }
    let plan = SandboxPlan::new(caps, None, None);
```

(Two changes: remove `#[ignore]` line; replace plan construction.)

- [ ] **Step 2: Update the ollama test**

Find the block around line 545-570 and apply the identical pattern, with `"ollama-plugin"` as the helper key.

- [ ] **Step 3: Update the openai test**

Find the block around line 635-660 and apply the identical pattern, with `"openai-plugin"` as the helper key.

- [ ] **Step 4: Compile-check**

```bash
timeout 180 env CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=target/agent-impl \
  cargo build --tests -p tau-plugin-compat 2>&1 | tail -10
```

Expected: clean compile.

- [ ] **Step 5: Run all 5 layer4_native tests on Linux**

```bash
podman run --rm -v $PWD:/work -w /work \
  -e CARGO_INCREMENTAL=0 \
  -e CARGO_TARGET_DIR=/work/target/podman \
  ghcr.io/lebocqtitouan/tau-podman-gate:latest \
  bash -c "cargo nextest run -p tau-plugin-compat --test layer4_native"
```

Expected: ALL 5 tests pass. PR 1's 2 tests should still pass (no regression). PR 2's 3 tests now pass.

If any of the 3 HTTP tests FAILS: hard-stop. Re-run T7 to identify the missed path. Likely candidates: dynamic-named TLS root paths, distribution-specific layouts.

- [ ] **Step 6: Commit**

```bash
git add crates/tau-plugin-compat/tests/layer4_native.rs
git commit -m "test(plugin-compat): un-#[ignore] anthropic/ollama/openai Layer 4 native tests

All 3 HTTP plugin tests now pass on Linux thanks to T8's
startup_io_paths_for population (and any baseline extensions for
universal reqwest TLS paths).

Closes the final 3 of 5 #[ignore]'d Layer 4 native tests. Combined
with PR 1, the entire layer4_native.rs suite is now active in CI.

Spec: docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md.
T7 investigation findings appended in this branch's first commit.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
"
```

---

## Task 10: USER GATE — open PR 2, monitor CI

**Main agent only — no subagent.**

- [ ] **Step 1: Verify clean working tree + correct branch**

```bash
git status && git log --oneline main..feat/layer4-startup-io-http
```

Expected: clean tree; 3 commits on the branch (T7 spec edit, T8 helper population, T9 test un-ignore).

- [ ] **Step 2: Run lefthook pre-push gate + push**

```bash
git push -u origin feat/layer4-startup-io-http
```

- [ ] **Step 3: Open PR**

```bash
gh pr create --title "feat(plugin-compat): un-#[ignore] HTTP Layer 4 tests (anthropic/ollama/openai)" --body "$(cat <<'EOF'
## Summary

PR 2 of 2 closing the Layer 4 plugin-compat startup-IO gap. Closes the remaining 3 of 5 `#[ignore]`'d tests in `crates/tau-plugin-compat/tests/layer4_native.rs` (anthropic, ollama, openai).

- **`tau-plugin-compat::startup_io`** — populated HTTP plugin match arms with reqwest TLS bootstrap paths (per T7 investigation findings).
- **(Conditional) `tau-sandbox-native::light::BASELINE_SYSTEM_READ_PATHS`** — extended with universal HTTP paths if T7 found any common to all 3 plugins.
- **`layer4_native.rs`** — un-`#[ignore]`'d the 3 HTTP plugin tests; each wires the helper.

Combined with PR 1, the entire `layer4_native.rs` suite is now active.

Spec: `docs/superpowers/specs/2026-05-09-layer4-startup-io-design.md`. T7 investigation findings appended in the first commit.

## Test plan

- [ ] CI green on the 14 required checks
- [ ] All 5 `layer4_native.rs` tests pass on `test (tau-plugin-compat / linux)`
- [ ] No regression in `test (tau-sandbox-native e2e / linux)` if baseline was extended

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Monitor CI**

Same Monitor flow as T5. Pause for user approval before T11.

---

## Task 11: USER GATE — squash-merge PR 2

**Main agent only — no subagent.**

- [ ] **Step 1: Verify all 14 required checks green**

```bash
gh pr checks <PR#> --json name,state | jq '[.[] | select(.state != "SUCCESS")] | length'
```

Expected: `0`.

- [ ] **Step 2: Squash-merge**

```bash
gh pr merge <PR#> --squash --delete-branch
```

- [ ] **Step 3: Sync main + memory update**

```bash
git checkout main && git pull --ff-only && git log --oneline -3
```

Update `~/.claude/projects/-Users-titouanlebocq-code-tau/memory/MEMORY.md` with a new entry for the shipped sub-project. Update the `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` "Spinoff — Layer 4 plugin-compat startup-IO cataloging" section to mark it DONE with PR refs (could roll into a future audit pass like PR #47).

---

## Self-review checklist

After all tasks complete, verify:

- [ ] All 5 `layer4_native.rs` tests pass on Linux CI (the goal)
- [ ] No regressions in `tau-sandbox-native` e2e tests
- [ ] No regressions in container-side `layer4_container.rs` (already closed by ADR-0021)
- [ ] Each baseline path has a one-line comment explaining why
- [ ] No application-data paths leaked into `BASELINE_SYSTEM_READ_PATHS` (Constitution G12)
- [ ] Spec's "Investigation findings" section is filled with concrete data, not placeholders
