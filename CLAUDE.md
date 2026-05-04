# CARGO RULES — read before running any cargo command

This workspace has 8 crates sharing one `target/.cargo-lock`. Concurrent
cargo invocations queue on this lock and waste 2–4 minutes per build.
Every cargo command MUST follow these rules. No exceptions.

## Rule 1: Always set CARGO_TARGET_DIR

NEVER run bare `cargo`. ALWAYS prefix with `CARGO_TARGET_DIR=<path>`.

| Caller | CARGO_TARGET_DIR value |
|---|---|
| Main agent (top-level Bash tool) | `target/main` |
| Any subagent spawned via Agent tool | `target/agent-<role>` where `<role>` is the subagent's purpose (e.g. `spec-review`, `solution-review`, `impl`, `adversary`) |
| One-off diagnostic from main agent (cargo --version, cargo metadata, etc.) | `target/main` |

If you cannot determine your role, use `target/agent-misc`. Never omit the variable.

## Rule 2: Always scope to a single crate

Use `-p <crate>`. Never invoke cargo from the workspace root without `-p`.

✅ `CARGO_TARGET_DIR=target/main cargo test -p tau-domain`
❌ `cargo test`
❌ `cargo test --workspace`
❌ `CARGO_TARGET_DIR=target/main cargo test`  (no -p)

## Rule 3: Always wrap with timeout

| Command | Timeout |
|---|---|
| `cargo test` | 300s |
| `cargo build` / `cargo check` | 180s |
| `cargo clippy` | 240s |
| `cargo fmt --check` | 30s |

Format: `timeout 300 env CARGO_TARGET_DIR=target/main cargo test -p tau-domain`

## Rule 4: Before invoking cargo, check for active builds

If another cargo process is running on a shared target dir, your build
will queue on the lock. Quick check:

    pgrep -af cargo | grep -v grep

If you see another cargo invocation using the same CARGO_TARGET_DIR you
were about to use, EITHER wait for it OR pick a different target dir
(e.g. `target/agent-<role>-2`). Do not just launch and hope.

## Why these rules exist

Past sessions accumulated 24 lock-contended builds totaling ~36 minutes
of pure waiting. `sccache` (`RUSTC_WRAPPER=sccache`, set in user env)
ensures distinct target dirs share the rustc compile cache, so the disk
and CPU cost of multiple target dirs is negligible. The rule eliminates
contention without sacrificing speed.

## Reference command shape

Copy-paste template, fill in `<role>`, `<crate>`, and the actual cargo args:

    timeout 300 env CARGO_TARGET_DIR=target/agent-<role> cargo test -p <crate>
