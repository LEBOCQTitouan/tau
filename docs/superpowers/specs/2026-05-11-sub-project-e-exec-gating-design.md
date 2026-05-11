# Sub-project E — Per-command exec gating: diagnose & close

**Date:** 2026-05-11
**Status:** Approved for implementation
**Branch:** `feat/sub-project-e-exec-gating`
**Supersedes:** §Sub-project E in [`2026-05-03-sandboxing-followups.md`](2026-05-03-sandboxing-followups.md) (whose original landlock-V2 framing turned out to be wrong — current code already uses V1's `AccessFs::Execute`).

## Goal

Close `crates/tau-plugin-compat/tests/layer4_native.rs::shell_layer4_native_runs_echo_hello` by identifying and fixing the EACCES that `execve` returns under strict-tier landlock, even when the target path appears to have `Execute` granted both at file level (via `exec_paths`) and at parent-directory level (via `BASELINE_SYSTEM_READ_PATHS`).

## Background

Inline diagnosis on 2026-05-11 (during the T7 closure session) established:

- Shell plugin resolving `"echo"` → `"/usr/bin/echo"` via PATH before `Command::new` eliminates `execvp`'s PATH-search hitting EACCES on `/usr/local/{sbin,bin}` (a known POSIX behavior: `execvp` stops on EACCES rather than falling through). But this alone does not close the test.
- With the resolved absolute path, the plugin still observes:
  - `std::fs::metadata("/usr/bin/echo")` → `Ok` (stat succeeds).
  - `std::fs::File::open("/usr/bin/echo")` → `Ok` (open with `O_RDONLY` succeeds, implying landlock grants `ReadFile`).
  - `Command::new("/usr/bin/echo").spawn()` → `Err(EACCES)` (execve in the forked child fails).
- The EACCES persists when `exec_paths` is granted `ReadFile | Execute` (not just `Execute`) on the file itself.
- The EACCES persists when `BASELINE_SYSTEM_READ_PATHS` already grants `ReadFile | ReadDir | Execute` on `/usr/bin` (the parent), which `PathBeneath` should propagate to `/usr/bin/echo`.

Open question: *what specific landlock-grant shape (flags, rule structure) makes execve succeed under the strict tier?* The followups doc estimated 1 week for this work; our experience is that an inline two-hour investigation didn't get there.

## Approach (locked)

**Diagnose-then-fix via a standalone minimal repro outside tau.**

A new fixture crate `landlock-exec-repro/` (NOT a workspace member, mirroring `crates/tau-plugin-compat/fixtures/controlled-env-binary/`) hosts a Linux-only Rust binary that mirrors `tau-sandbox-native::strict::apply_strict`'s pre_exec sequence on a stripped-down skeleton. Argv flags toggle each layer:

```
--landlock=off|baseline|baseline+exec
--landlock-exec-path=<PATH>
--landlock-exec-grants=<CSV of AccessFs flags>
--unshare-user
--unshare-net
--seccomp
--target=<PATH to execve>
```

A driver script `scripts/diagnose-exec-eacces.sh` runs the binary inside the standard Podman gate config (`cap-add SYS_ADMIN/NET_ADMIN`, `--security-opt seccomp/apparmor=unconfined`, `--security-opt label=disable`), iterating through a fixed matrix of configurations and printing a result table. The lowest-numbered "exec ok" row reveals the minimal sufficient config; comparing rows around the boundary identifies the responsible flag/rule shape.

Once the diagnosis lands, the *fix delta* (expected: 1–10 LOC plus a comment) is applied to `tau-sandbox-native`. The shell layer4 test un-`#[ignore]`s. Two regression unit tests join `tau-sandbox-native::strict::tests` (matching PR #55's pattern).

## Architecture

The fixture binary is intentionally a top-level execve replacement, not a process that forks-then-execve. It applies all sandbox layers inline to its own task, then directly calls `nix::unistd::execve(target, argv, envp)`. Exit codes encode the failure layer:

- `0` — unreachable (execve replaces the process; we never return).
- `64 + errno` — execve failed (clamped to exit-code range 64..=127). Covers all POSIX errnos we care about (EACCES=13 → 77, EPERM=1 → 65, ENOENT=2 → 66).
- `32 + layer#` — setup failed before reaching execve. Layers: 0=arg parse, 1=landlock build, 2=landlock create, 3=add rules, 4=restrict_self, 5=unshare, 6=seccomp compile, 7=seccomp apply.

The driver script is shell, not Rust — avoids a second build target and keeps the iteration tight. It builds the repro binary once per Podman run, then loops the matrix.

## Components

| Component | Path | Purpose |
|---|---|---|
| `landlock-exec-repro` binary | `crates/landlock-exec-repro/{Cargo.toml,src/main.rs}` (non-member) | Inline-sandboxed execve harness, ~150 LOC. |
| `diagnose-exec-eacces.sh` | `scripts/diagnose-exec-eacces.sh` | Builds the repro inside the Podman gate, runs the matrix, prints the result table. Survives in-tree as the canonical regression repro. |
| Fix (TBD by diagnosis) | `crates/tau-sandbox-native/src/light.rs` (most likely `install_landlock` ~line 230); possibly also `src/strict.rs::apply_strict` and/or `src/exec.rs::collect_exec_paths` | The minimal delta the diagnostic surfaces. One function or struct change with a comment citing the matrix row + (where applicable) the landlock-crate or kernel-source line that explains it. |
| Un-ignore | `crates/tau-plugin-compat/tests/layer4_native.rs:246` | Remove `#[ignore]` on `shell_layer4_native_runs_echo_hello`. |
| Regression unit tests | `crates/tau-sandbox-native/src/strict.rs` (Linux-only test module) | Two tests (see Testing). |
| Followups doc update | `docs/superpowers/specs/2026-05-03-sandboxing-followups.md` §Sub-project E | Mark ✅ DONE, replace V2 framing with the V1-sufficient reality, link to this spec + the repro binary. |

The repro crate is NOT a workspace member because a future kernel/landlock-crate change that breaks our setup should not block the workspace from compiling — running the repro after such a bump is the explicit signal to investigate. It uses the same external `landlock` + `nix` + `seccompiler` crates the production code uses.

## Diagnostic matrix

| # | Landlock | UserNs | NetNs | Seccomp | Exec-path grants | Hypothesis |
|---|---|---|---|---|---|---|
| 0 | off | off | off | off | n/a | sanity: baseline execve works |
| 1 | baseline only | off | off | off | none | does landlock baseline alone allow execve? |
| 2 | baseline + exec_path | off | off | off | `Execute` | current production state (light tier) |
| 3 | baseline + exec_path | off | off | off | `ReadFile \| Execute` | ReadFile-required hypothesis |
| 4 | baseline + exec_path | off | off | off | full V1 (`from_all`) | maximal-grant hypothesis |
| 5 | baseline + exec_path | on | on | off | `ReadFile \| Execute` | does namespace-isolation interact? |
| 6 | baseline + exec_path | on | on | on | `ReadFile \| Execute` | full strict-tier (current failing config) |
| 7 | baseline only (no exec_path rule) | on | on | on | n/a | does the file rule itself interfere? |
| 8 | PathBeneath on `/usr/bin` only (no file rule) | on | on | on | n/a | dir-level grant sufficient? |

The script prints (one row per matrix entry):

```
# config                                   exit  meaning
0 unsandboxed                              0     exec ok
1 lock(base)                               0     exec ok
2 lock(base+exec=Exe)                      ?     ?
...
```

The lowest "exec ok" row reveals the minimal sufficient config.

## Data flow

1. Driver script `apt-get install`s build deps inside the Podman container, then `cargo build --release` of the repro binary into `target/lefthook-podman/release/landlock-exec-repro`.
2. Driver script loops the matrix; each iteration invokes the binary with the row's argv, captures the exit code, and adds a row to the result table.
3. Driver script prints the table to stdout at the end. The lowest "exec ok" row is the answer.
4. The diagnostic table is preserved as evidence in the eventual PR description and as a comment in `crates/landlock-exec-repro/README.md`.

## Error handling

- Repro binary: `eprintln!` + nonzero exit codes; setup failures distinguished from execve failures by exit-code range.
- Driver script: tolerates per-config setup failures (e.g., kernel doesn't support a requested unshare flag) — those rows print `setup-err <layer>` and don't count as the "first working config."
- Production fix in `light.rs`: preserves the existing `Result<(), Box<dyn Error + Send + Sync>>` error shape. No new error variants in `SandboxError`.

## Testing

- **Fix-verification (load-bearing):**
  1. `shell_layer4_native_runs_echo_hello` (un-ignored) passes under the Podman gate.
  2. 2 new unit tests in `tau-sandbox-native::strict::tests` pass at lib-level (~ms each, no Podman needed):
     - `wrap_spawn_with_process_spawn_cap_grants_execute_on_resolved_path` — constructs a plan with `Process(Spawn { commands: ["echo"] })`, calls `apply_strict`, asserts `collect_exec_paths` resolved to an absolute path (via the same PATH-resolution helper).
     - `wrap_spawn_without_exec_capability_omits_exec_paths` — constructs a plan with only `Filesystem(Read)`, calls `apply_strict`, asserts no exec rules were added beyond the baseline.
  3. The 78-test `tau-sandbox-native --features integration-tests` suite stays green — guards against the fix breaking anything else.
  4. The 3 newly-un-ignored layer4_native HTTP tests from PR #53 (`anthropic_layer4_native_completes_via_cassette` + ollama + openai) stay green — guards against the fix breaking HTTP plugins.
- **Regression**: the repro crate + matrix script ship together. Running the script after any kernel/landlock-crate bump catches the same class of regression in seconds.

## Scope

**In scope:**
- `Capability::Process(Spawn)` exec gating, `Capability::Filesystem(Exec)` exec gating. Both are already collected by `collect_exec_paths` and granted `Execute`. The fix applies to both.
- Closing the shell layer4 test (un-`#[ignore]`).
- 2 regression unit tests + the standalone repro crate.

**Out of scope:**
- Landlock V2-specific features (refine-on-open, ioctl-dev). Current code is V1 and the diagnosis is expected to stay within V1. If diagnosis reveals V2 is required, that's a separate sub-project E2.
- Seccomp-bpf path filtering (the `Y` approach we rejected during brainstorm).
- Wrapper-binary gating via a `tau-exec-helper` (the `Z` approach we rejected during brainstorm).
- Changes to plan validation, capability schemas, or `SandboxError` variants.

## Time-box & hard gate

**One focused session.** If matrix rows #0–#8 don't reveal an actionable fix delta, escalate before any production code commit. Two escalation options:

1. Open the spec for sub-project E2 (V2 / seccomp-bpf / wrapper) based on what the matrix revealed.
2. Sharpen the existing `#[ignore]` comment on the shell test with the new evidence (landlock-grant shape ruled out, no kernel-source explanation found), revert any in-flight repro work, and move on.

## Verification (end-to-end at PR time)

1. `cargo fmt --all -- --check` clean.
2. `cargo clippy --workspace --all-targets -- -D warnings` clean.
3. `cargo nextest run -p tau-sandbox-native --lib` includes the 2 new tests, all pass.
4. Inside Podman gate: `cargo nextest run -p tau-sandbox-native --features integration-tests --tests` — 78/78 green.
5. Inside Podman gate: `cargo nextest run -p tau-plugin-compat --features integration-tests --tests -E "test(/layer4_native/)"` — 5/5 green (4 from PR #53 + the newly un-ignored shell test).
6. CI green on all 18 required checks.

## Out-of-band followups doc update

Edit `docs/superpowers/specs/2026-05-03-sandboxing-followups.md`:

- §Sub-project E heading → ✅ DONE (with the actual merge date filled in at PR time).
- Replace the "v0.1 no-op stub" / "landlock V2 (kernel ≥ 5.19)" framing with the actual story: V1 is sufficient; the v0.1 no-op stub had already been migrated to V1's `AccessFs::Execute` via `exec.rs::collect_exec_paths` + `light.rs::install_landlock` (date can be reconstructed from `git log -p crates/tau-sandbox-native/src/exec.rs`); the remaining EACCES was the diagnostic gap closed by this work.
- Link to the merged PR + `crates/landlock-exec-repro/` for the canonical regression repro.
