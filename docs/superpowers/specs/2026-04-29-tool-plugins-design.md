# First real Tool plugins: `fs-read` + `shell` — Phase 1 priority 3

**Status:** Draft (this spec) → Implementation plan derived → no ADR
needed for this sub-project (purely additive — see §2).

**Sub-project scope:** Phase 1 priority 3 from the
[ROADMAP](../../../ROADMAP.md). First real Tool plugins (the kernel
already has 3 LLM-backend plugins; Tier 1 priority 3 closes out by
introducing tools). First sub-project that exercises capability checks
at runtime end-to-end — both plugin-side declaration via tau.toml AND
agent-side allow-list intersection in `tau-runtime::run.rs:272`.

---

## 1. Summary

Two minimal Tool plugins ship together because (a) one tool alone
doesn't demonstrate the capability-vocabulary surface and (b)
`fs-read` + `shell` cover the two main capability classes (`fs.read`
and `process.spawn`). The plugins are deliberately small at v0.1 — the
goal is to prove the kernel's tool-dispatch + capability-check path
works end-to-end, not to ship a full filesystem/process toolkit.

The plugins:

- Live in-tree under `crates/tau-plugins/{fs-read,shell}/`, parallel
  to the LLM-backend plugins.
- Implement `tau_ports::Tool` via `tau_plugin_sdk::run_tool_with_config`
  (the SDK already supports tools per ADR-0008; no SDK amendment).
- Each ship a single tool method (multi-method expansion deferred to
  a later sub-project — see §11.1).
- Declare required capabilities in their tau.toml `[[capabilities]]`
  table per ADR-0002 §3 (canonical TOML form: `kind = "fs.read"` /
  `kind = "process.spawn"`).
- Run **unsandboxed** on the host process. Sandboxing is Constitution
  G12 + ROADMAP Tier 3 #12 — explicitly out of scope. The trust model
  is documented in §10.

### 1.1 Scope confirmed

**Ships:**

- `crates/tau-plugins/fs-read/` — `read_file` tool. One method:
  `{path: String} → {contents: bytes, size: u64}`. Capability:
  `fs.read` (paths via glob list).
- `crates/tau-plugins/shell/` — `exec` tool. One method:
  `{command: String, args: Vec<String>, timeout_secs: Option<u64>, cwd: Option<String>} → {stdout: bytes, stderr: bytes, exit_code: i32, timed_out: bool, stdout_truncated: bool, stderr_truncated: bool}`.
  Capability: `process.spawn` (commands via name allowlist).
- 2 new CI jobs: `build (fs-read-plugin)` and `build (shell-plugin)`
  (release builds; integration tests run inside the existing workspace
  test job). 21 → 23 required CI checks gating `main`.
- ~12 unit tests per plugin + 4-5 integration tests per plugin (no
  conformance suite — see §2 decision Q3).
- No new workspace deps. No new `tau-ports` / `tau-runtime` /
  `tau-plugin-sdk` changes. No new ADR.

**Does NOT ship:**

- Multi-method `fs-read` (`list_dir`, `stat`, `read_dir`) — deferred to
  a follow-up sub-project per user-locked Q1.
- Shell `stdin`, env-var override, signal forwarding — deferred per Q1.
- Output streaming (vs the 1 MiB cap) — adds significant complexity to
  the IPC layer; not justified at v0.1.
- Tool conformance suite extension — Q3 decision; revisit when 3+ tool
  plugins exist.
- Schema validation activation in tau-runtime (`PluginContractViolation`)
  — ROADMAP Tier 2 priority 6, separate sub-project. Tools self-validate
  args in `invoke()` and return `ToolError::BadArgs` on failure.
- Sandboxing — Constitution G12, ROADMAP Tier 3 priority 12.

### 1.2 Constitution alignment

| Constraint | This sub-project's answer |
|---|---|
| `forbid(unsafe_code)` | Plain Rust; subprocess spawning via `tokio::process::Command`. |
| **G6** runtime not framework | Plugins are thin shims over `std::fs::read` and `tokio::process::Command`. |
| **G9** observable by default | Both plugins emit `tracing` events under `target = "fs_read_plugin::*"` and `"shell_plugin::*"`. |
| **G12** sandboxed by default | NOT met at v0.1 — explicitly deferred per ROADMAP Tier 3 #12. Trust model documented in §10. |
| **G14** capabilities enforced at dispatch | Met: tau-runtime already checks `Tool::capabilities()` against the agent's package manifest at `run.rs:272`. This sub-project provides the FIRST real test of that path. |
| **NG7** does not evaluate quality | Tool result correctness is plugin-side (real OS calls); the runtime does not inspect content. |

---

## 2. Decisions

This sub-project does NOT introduce its own ADR. Reasons:

1. **Purely additive** — two new workspace crates; no existing API
   changes.
2. **No protocol changes** — uses ADR-0008's wire vocabulary.
3. **No new error variants** in `tau-ports` / `tau-runtime` /
   `tau-plugin-sdk`. The plugins emit existing typed `ToolError`
   variants per ADR-0009's typed-error policy.
4. **No new capability variants** in `tau-domain`. Both `fs.read` and
   `process.spawn` are already typed (ADR-0002 §3).
5. **No package manifest schema changes**.

The sub-project-local engineering decisions the brainstorm settled:

| # | Decision | Rationale |
|---|---|---|
| Q1 | **Plugin scope:** minimal — one tool method per plugin. Multi-method `fs-read` (list_dir/stat) and shell expansion (stdin/env) deferred to follow-up sub-project. | v0.1 proves the dispatch + capability path; further methods are mechanical follow-ons. User-locked. |
| Q2 | **shell timeout + caps:** wall-clock 30s default, 600s max; per-call override via `timeout_secs`; 1 MiB stdout / 1 MiB stderr; truncate + flag on overflow; SIGKILL on deadline. | Wall-clock matches `timeout(1)`; predictable; resists bytes-per-tick stalling. |
| Q3 | **Tool conformance suite:** punt. Per-plugin integration tests only at v0.1; revisit when ≥3 tool plugins exist. | Rule-of-three: at N=2 the parameterized contract would be writing the suite against itself. |
| Q4 | **Schema validation activation:** defer to ROADMAP Tier 2 #6. Tools self-validate via `ToolError::BadArgs`. | Out of sub-project scope; cross-cuts kernel. |
| Q5 | **Sandboxing:** out of scope (G12, Tier 3 #12). v0.1 trust model documented in §10. | Explicit non-goal until the sandbox sub-project lands. |
| Q6 | **Path scoping for fs-read:** glob-pattern allow-list via the existing `FsCapability::Read { paths: Vec<String> }`. The plugin checks: every requested `path` must match at least one glob in the active capability set. The plugin RECEIVES this list (post-intersection) from the runtime via `SessionContext`-equivalent glue (see §5.2). NO hard-coded paths in the plugin. | Reuses the typed capability variant; matches ADR-0002 vocabulary. |
| Q7 | **Symlink handling:** read symlinks as a normal file (i.e., follow the symlink), but the GLOB CHECK is performed against the path the agent provided BEFORE canonicalization. Path-traversal protection: paths containing `..` segments are rejected with `ToolError::BadArgs`; absolute paths are required. | Defensive default. Future symlink-aware mode is a config knob. |
| Q8 | **Command allow-list for shell:** every requested `command` must appear in the active `ProcessCapability::Spawn { commands }` list. NO arbitrary command execution. | Already typed in tau-domain; matches ADR-0002 vocabulary. |
| Q9 | **`cwd` handling for shell:** when set, must be an absolute path that is reachable on the host. The plugin does NOT validate the path against any capability — the agent's process.spawn capability is sufficient authorization (cwd is a runtime-of-process detail, not a separate capability class). | YAGNI; `cwd` is a UX nicety. |

### 2.1 Decisions explicitly out of scope

| Topic | Where it lives |
|---|---|
| `fs-read::list_dir`, `fs-read::stat` | Future sub-project (user-locked deferral) |
| Shell `stdin`, env-var override | Future sub-project |
| Output streaming (vs 1 MiB cap) | Future sub-project; needs IPC frame refactor |
| Tool conformance suite | Revisit at N≥3 tool plugins |
| Schema validation in tau-runtime | ROADMAP Tier 2 priority 6 |
| Sandboxing | ROADMAP Tier 3 priority 12 |
| Long-running process tracking (background jobs) | Out of scope; one invoke = one subprocess |

---

## 3. Architecture

### 3.1 Workspace layout

```
crates/tau-plugins/fs-read/
├── Cargo.toml                    -- bin: fs-read-plugin
├── tau.toml                      -- provides=tool; requires fs.read
└── src/
    ├── main.rs                   -- #[tokio::main] → run_tool_with_config
    ├── lib.rs                    -- pub mod plugin; pub mod config
    ├── plugin.rs                 -- FsReadPlugin: Tool impl
    ├── config.rs                 -- FsReadConfig (no-op v0.1; reserved)
    └── path_check.rs             -- glob-match validation + traversal-guard

crates/tau-plugins/shell/
├── Cargo.toml                    -- bin: shell-plugin
├── tau.toml                      -- provides=tool; requires process.spawn
└── src/
    ├── main.rs                   -- #[tokio::main] → run_tool_with_config
    ├── lib.rs                    -- pub mod plugin; pub mod config
    ├── plugin.rs                 -- ShellPlugin: Tool impl
    ├── config.rs                 -- ShellConfig (default_timeout_secs etc.)
    ├── exec.rs                   -- tokio::process::Command + timeout/caps
    └── command_check.rs          -- command-name allow-list check

.github/workflows/ci.yml          -- + 2 new jobs: build (fs-read-plugin), build (shell-plugin)
```

### 3.2 Dependencies

**No new workspace deps.** Existing workspace deps suffice:

- `tau-domain`, `tau-ports`, `tau-plugin-protocol`, `tau-plugin-sdk`
- `serde`, `serde_json`, `thiserror`, `tokio`, `tracing`
- `globset` is the only NEW per-plugin dep (for `fs-read`'s path-glob match). It's NOT added to the workspace dep table — added directly under `crates/tau-plugins/fs-read/Cargo.toml` `[dependencies]`. Rationale: only one plugin currently needs glob matching; promoting to workspace is premature.

### 3.3 Dataflow

```
tau-cli
  └─ tau-runtime::Runtime::run
      ├─ load Tool plugins via plugin_host (per ADR-0008)
      │   ├─ spawn target/release/fs-read-plugin
      │   └─ spawn target/release/shell-plugin
      ├─ for each agent turn:
      │   ├─ agent's package manifest declares e.g.
      │   │   [[capabilities]] kind="fs.read" paths=["${PROJECT}/**"]
      │   ├─ LLM emits tool_call → MessagePayload::ToolCall { name, args }
      │   ├─ runtime resolves tool, checks Tool::capabilities() against
      │   │   agent's package capabilities (run.rs:272 — already wired)
      │   ├─ on capability mismatch: RuntimeError::CapabilityDenied
      │   ├─ on capability match: dispatch via DynTool to plugin process
      │   │   ├─ Tool::init(SessionContext) — opens session
      │   │   ├─ Tool::invoke(&mut session, args) — performs the call
      │   │   └─ Tool::teardown(session) — closes session
      │   └─ plugin returns ToolResult; runtime wraps as ToolReply
      └─ frames out via stdout
```

---

## 4. `fs-read` plugin

### 4.1 Tool surface

```rust
fn name(&self) -> &str { "fs-read" }
fn schema(&self) -> ToolSpec {
    ToolSpec {
        name: "fs-read".into(),
        description: "Read the bytes of a file at an absolute path.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute path to the file. No `..` segments allowed." }
            },
            "required": ["path"],
        }),
    }
}
fn capabilities(&self) -> &[Capability] {
    // The runtime's capability check uses `capability_satisfies`: an
    // agent's `Filesystem(Read{paths: [...]})` satisfies a tool's
    // declared `Filesystem(Read{paths: []})` because empty paths
    // means "any read" semantically (the runtime treats the tool's
    // declaration as the floor; the agent's grant is the ceiling).
    // Both must be Filesystem(Read), so we declare the structural
    // capability; path-glob validation happens at invoke time using
    // the AGENT's grant (received via session/runtime context).
    &[Capability::Filesystem(FsCapability::Read { paths: vec![] })]
}
```

### 4.2 `invoke` semantics

Per spec §6 + §7:

1. Parse `args` as `{path: String}`. Reject empty / null-byte-containing
   paths with `ToolError::BadArgs`.
2. Reject paths containing `..` segments with `ToolError::BadArgs`
   (path-traversal guard per Q7).
3. Reject non-absolute paths with `ToolError::BadArgs`.
4. Look up the agent's `FsCapability::Read.paths` glob list (received
   from the runtime — see §5.2 for the plumbing). For each glob,
   compile it with `globset::Glob::new(g)?.compile_matcher()`. The
   path is admissible iff at least one glob matches.
5. On glob mismatch: return `ToolError::BadArgs { reason: "path not in
   capability scope: <path>" }`. (We use `BadArgs` rather than
   `CapabilityDenied` because the kernel's capability check at
   `run.rs:272` already passed — this is a finer-grained
   plugin-side check.)
6. On admissible path: call `tokio::fs::read(&path).await`. On `Ok(bytes)`,
   return `ToolResult { content: vec![ToolContent::Json { data: ... }],
   is_error: false }` where data is `{"contents": "<base64-encoded>",
   "size": <bytes.len()>}`. Base64-encoding because `ToolContent::Json`
   takes a `tau_domain::Value` and binary data isn't natively
   representable.
7. On `Err(io_err)`: return `ToolResult { content: vec![ToolContent::Text
   { text: "fs-read: <error message>" }], is_error: true }` —
   semantic-error path, NOT trait-method-error. The agent's LLM sees
   the error string; the runtime does not retry.
8. Internal failures (e.g. failed to encode result): return
   `ToolError::Internal { message: ... }`.

### 4.3 Path validation module (`path_check.rs`)

```rust
pub(crate) fn validate_path(path: &str) -> Result<&str, BadArgs> {
    if path.is_empty() {
        return Err(BadArgs::Empty);
    }
    if path.bytes().any(|b| b == 0) {
        return Err(BadArgs::NullByte);
    }
    if !std::path::Path::new(path).is_absolute() {
        return Err(BadArgs::NotAbsolute);
    }
    if path.split(std::path::MAIN_SEPARATOR).any(|seg| seg == "..") {
        return Err(BadArgs::Traversal);
    }
    Ok(path)
}

pub(crate) fn admit(path: &str, allowed_globs: &[String]) -> bool {
    use globset::Glob;
    allowed_globs.iter().any(|g| {
        Glob::new(g).ok()
            .map(|gl| gl.compile_matcher().is_match(path))
            .unwrap_or(false)
    })
}
```

`BadArgs` is an internal enum that maps to `ToolError::BadArgs { reason }` strings. Centralizing the error strings here makes them testable without spelunking through plugin.rs.

---

## 5. `shell` plugin

### 5.1 Tool surface

```rust
fn name(&self) -> &str { "shell" }
fn schema(&self) -> ToolSpec {
    ToolSpec {
        name: "shell".into(),
        description: "Run a shell command (allow-listed name + args). Wall-clock timeout; output capped at 1 MiB per stream.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "command":      { "type": "string", "description": "Command name (must be in agent's process.spawn allow-list)." },
                "args":         { "type": "array", "items": { "type": "string" }, "default": [] },
                "timeout_secs": { "type": "integer", "minimum": 1, "maximum": 600, "description": "Wall-clock timeout. Default 30; max 600." },
                "cwd":          { "type": "string", "description": "Optional absolute working directory." }
            },
            "required": ["command"],
        }),
    }
}
fn capabilities(&self) -> &[Capability] {
    &[Capability::Process(ProcessCapability::Spawn { commands: vec![] })]
}
```

### 5.2 Capability-list plumbing

Both plugins need access to the agent's GRANTED capabilities (not just
the structural declaration) at invoke time so they can:

- `fs-read`: check the path against `FsCapability::Read.paths`.
- `shell`: check the command against `ProcessCapability::Spawn.commands`.

The runtime's existing capability check at `run.rs:272` only verifies
that the agent HAS a satisfying capability — it doesn't pass the typed
`paths`/`commands` lists through to the plugin. We need that data on
the plugin side.

**Two implementation options:**

**A) Pass via `SessionContext` extension (RECOMMENDED).** Extend
`tau_ports::SessionContext` (which is `#[non_exhaustive]`) with an
additive field: `granted_capabilities: Vec<Capability>`. tau-runtime
populates it from the agent's package manifest before calling
`Tool::init`. Plugins read from `&mut Self::Session` (which the
plugin produced from the SessionContext).

This is purely additive on `tau_ports` — `#[non_exhaustive]` allows
the field. The plugin-protocol IPC encodes Vec<Capability> via the
existing `tau-domain` serde impls. No new ADR (the SessionContext
extension is a non-breaking additive change documented in this spec).

**B) Pass via plugin handshake config.** The runtime crafts a fresh
plugin instance per agent and passes the capability list via the
handshake `config` JSON. Drawbacks: instances aren't currently
per-agent (one plugin process serves all agents); making them
per-agent is a tau-runtime change.

**Decision: A.** Adds ONE field to `SessionContext`. Documented as a
sub-project amendment to ADR-0008 §5 (the SessionContext shape is
listed there as part of the IPC vocabulary). One commit message line
suffices; no new ADR.

### 5.3 `invoke` semantics

1. Parse args as `{command: String, args: Vec<String> = [], timeout_secs: Option<u64>, cwd: Option<String>}`. Reject empty `command` with `ToolError::BadArgs`.
2. Look up the agent's `ProcessCapability::Spawn.commands` allow-list (via SessionContext). If `command` is not in the list, return `ToolError::BadArgs { reason: "command not in capability scope: <command>" }`.
3. If `cwd` is `Some(p)`, validate `p` is absolute (`ToolError::BadArgs` otherwise). Don't validate that `p` exists — let the spawn fail with a semantic error if it doesn't.
4. Compute effective `timeout_secs`: `args.timeout_secs.unwrap_or(30).min(600).max(1)` (clamp to [1, 600]).
5. Spawn the subprocess via `tokio::process::Command::new(command).args(args).envs([]).current_dir(cwd?).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())`. **No environment inheritance** — tools should be reproducible. **No stdin** — explicit decision per Q1.
6. Wait for completion with `tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output())`.
7. On timeout: SIGKILL the subprocess; capture whatever stdout/stderr was buffered up to that point; return `ToolResult { content: ..., is_error: false }` with `timed_out: true, exit_code: -1`. (The wall-clock timeout is a normal completion path, not a `ToolError::DeadlineExceeded`. The latter is reserved for the `SessionContext.deadline` — a session-level deadline.)
8. On normal completion: capture stdout/stderr. Truncate each to 1 MiB and set the corresponding `*_truncated: bool` flag.
9. Return `ToolResult { content: vec![ToolContent::Json { data: ... }], is_error: false }` with the structured response. Set `is_error: true` only when `exit_code != 0` (semantic error to the LLM).

### 5.4 Output capping

```rust
const MAX_OUTPUT_BYTES: usize = 1024 * 1024; // 1 MiB

fn cap_and_flag(buf: Vec<u8>) -> (Vec<u8>, bool) {
    if buf.len() > MAX_OUTPUT_BYTES {
        (buf[..MAX_OUTPUT_BYTES].to_vec(), true)
    } else {
        (buf, false)
    }
}
```

The cap is post-hoc (after the process completes). For v0.1 this is
acceptable because the OS pipe buffers cap pipe usage at 64 KiB
typically — a process producing 1 GiB blocks on its own write side
and the timeout fires. A future streaming variant can cap on-the-fly.

### 5.5 Timeout semantics

Wall-clock from spawn. Output activity does NOT reset the timer. On
deadline:

1. `child.kill().await` (SIGKILL on Unix).
2. Best-effort `child.wait_with_output().await` to scrape the partial
   stdout/stderr buffers.
3. Return with `timed_out: true, exit_code: -1`.

---

## 6. Configuration shape (`config.rs`)

Both plugins ship a minimal `Config` struct for the SDK runner. Most
fields are reserved for future expansion.

### 6.1 `FsReadConfig`

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FsReadConfig {
    // Reserved for future expansion (e.g. follow_symlinks: bool).
    // v0.1 has no knobs.
}
```

The empty config still goes through `Configure::from_config` (per
ADR-0008's SDK `with_config` runner contract) so deserializing the
handshake's `config: serde_json::Value` round-trips.

### 6.2 `ShellConfig`

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellConfig {
    /// Default wall-clock timeout in seconds when args.timeout_secs
    /// is None. Default 30; clamped to [1, 600].
    #[serde(default = "default_timeout_secs")]
    pub default_timeout_secs: u64,

    /// Maximum wall-clock timeout in seconds (caps args.timeout_secs).
    /// Default 600 (10 min).
    #[serde(default = "default_max_timeout_secs")]
    pub max_timeout_secs: u64,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            default_timeout_secs: default_timeout_secs(),
            max_timeout_secs: default_max_timeout_secs(),
        }
    }
}

fn default_timeout_secs() -> u64 { 30 }
fn default_max_timeout_secs() -> u64 { 600 }
```

`Configure::from_config` validates `default_timeout_secs <= max_timeout_secs` and rejects via `ConfigError::InvalidValue` otherwise.

---

## 7. Error model

Per ADR-0009's typed-error policy, both plugins emit typed
`ToolError` variants:

| Failure | ToolError variant |
|---|---|
| Empty / null-byte / non-absolute path; `..` segment; cwd not absolute; missing required field | `BadArgs { reason }` |
| Path not in `fs.read` capability scope | `BadArgs { reason }` (already capability-checked at runtime; this is the finer-grained plugin scope check) |
| Command not in `process.spawn` capability allow-list | `BadArgs { reason }` |
| Subprocess spawn failed (binary not found, permission denied) | `Internal { message }` (the runtime treats this as a plugin failure; the agent's LLM sees the failure indirectly) |
| `tokio::fs::read` IO failure (file not found, permission denied) | NOT a `ToolError`. Returned as `ToolResult { is_error: true, content: text describing the OS error }`. The agent's LLM may decide to retry / give up. |
| Subprocess succeeded but `exit_code != 0` | `ToolResult { is_error: true, content: structured stdout/stderr/exit_code }`. |
| Subprocess timed out | `ToolResult { is_error: false (or true; see §5.3), timed_out: true }`. |
| `SessionContext.deadline` already passed | `DeadlineExceeded` |
| `serde_json::to_value` failure (extremely defensive) | `Internal { message }` |

`ToolError::CapabilityDenied` is NOT used by these plugins — the kernel
already checks structural capability before dispatch (`run.rs:272`).
The plugin's role is finer-grained scope checks (path globs, command
names) which surface as `BadArgs` because they signal "the agent's
request was malformed for the granted scope".

`ToolError::SessionDead` is NOT applicable — both plugins are stateless
(`Session = ()`).

---

## 8. Testing

### 8.1 `fs-read` tests

**Unit tests (~10 in `path_check.rs` + `plugin.rs`):**

- `validate_path_empty_rejected`
- `validate_path_null_byte_rejected`
- `validate_path_relative_rejected`
- `validate_path_traversal_rejected` (path with `..` segment)
- `validate_path_happy_path_returns_path`
- `admit_matches_glob`
- `admit_no_match_returns_false`
- `admit_empty_glob_list_returns_false`
- `invoke_reads_existing_file` (uses `tempfile::NamedTempFile`)
- `invoke_returns_is_error_true_on_missing_file`
- `invoke_capability_scope_violation_returns_bad_args`

**Integration tests (~3 in `tests/invoke.rs`):**

Full plugin lifecycle via `tau_plugin_sdk` test harness — spawn the
plugin process, send init/invoke/teardown frames over MessagePack-RPC,
assert responses. Reuses the same per-plugin TCP-style harness pattern
the plugin loading mechanism's `echo-tool` exercise established. (NOT
the cassette replayer — there's no HTTP here.)

- `integration_read_tempfile_succeeds`
- `integration_read_outside_glob_scope_bad_args`
- `integration_traversal_rejected`

### 8.2 `shell` tests

**Unit tests (~12 in `command_check.rs` + `exec.rs` + `plugin.rs`):**

- `command_in_allowlist_returns_ok`
- `command_not_in_allowlist_returns_bad_args`
- `cwd_relative_rejected`
- `cwd_absolute_accepted`
- `timeout_clamped_to_max`
- `timeout_zero_rejected_or_clamped_to_min`
- `cap_and_flag_under_limit_returns_buf_no_flag`
- `cap_and_flag_over_limit_truncates_and_flags`
- `exec_echo_returns_stdout` (runs `/bin/echo hi` or platform equiv)
- `exec_nonzero_exit_returns_is_error_true`
- `exec_timeout_kills_and_flags_timed_out`
- `exec_command_not_found_returns_internal`

**Integration tests (~4 in `tests/invoke.rs`):**

- `integration_echo_returns_expected_stdout`
- `integration_long_running_killed_by_timeout`
- `integration_command_outside_allowlist_bad_args`
- `integration_large_stdout_truncated_and_flagged` (uses `yes` or platform equiv to produce >1 MiB)

The `yes`-based test is gated on Unix (`#[cfg(unix)]`); Windows
equivalent uses a small helper script if needed.

### 8.3 No conformance suite

Per Q3, the existing `tau-plugin-conformance` crate stays
LlmBackend-only at v0.1. Tool plugins use per-plugin integration
tests above.

---

## 9. CI

2 new CI jobs in `.github/workflows/ci.yml`:

```yaml
build-fs-read-plugin:
  name: build (fs-read-plugin)
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - run: cargo build --release -p fs-read

build-shell-plugin:
  name: build (shell-plugin)
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: dtolnay/rust-toolchain@stable
    - run: cargo build --release -p shell
```

Both job names must match exactly — the sub-project sign-off ceremony
(plan Task 12) updates branch protection 21 → 23 required checks.

Integration tests run in the existing workspace `test (...)` matrix
job — no new test job needed.

---

## 10. Trust model (v0.1, sandboxing deferred)

**Both plugins run UNSANDBOXED on the host process.** This is an
explicit v0.1 limitation per Constitution G12 + ROADMAP Tier 3 #12.

Implications:

1. **`fs-read` can read any file the host process can read** — but the
   runtime's capability check + the plugin's glob-allow-list confine
   reads to the paths the agent explicitly granted via tau.toml.
2. **`shell` can run any subprocess that the host process can spawn** —
   confined by the agent's `process.spawn` command allow-list. A
   command `rm` on the allow-list with arg `-rf /` would be allowed;
   the allow-list is per-name, NOT per-argv.
3. **No memory / CPU / network isolation.** A spawned process can
   exhaust system resources; the timeout caps duration but not RSS.
4. **No filesystem restriction beyond glob allow-list.** A read of
   `/etc/passwd` succeeds if the agent declares `paths = ["/**"]`
   (which would be a misconfiguration).

The trust model is: **the project owner trusts every agent to use its
declared capabilities responsibly**. The agent's package manifest is a
human-curated trust boundary; the runtime enforces it per-call but
does not contain a malicious agent's bytes.

A future Tier 3 sub-project (G12) will add OS-level sandboxing
(seccomp/landlock on Linux, App Sandbox on macOS, Job Objects on
Windows). At that point the trust model tightens; until then,
operators MUST treat installed plugins as host-equivalent code.

This is documented inline in each plugin's `tau.toml` description and
in the README.md for the plugins (created in plan Task 9).

---

## 11. Out of scope (explicit deferrals)

| Topic | Where it lives |
|---|---|
| `fs-read::list_dir`, `fs-read::stat`, `fs-read::read_dir` | Future sub-project (user-locked Q1) |
| Shell `stdin`, env-var override, signal forwarding | Future sub-project |
| Output streaming (vs 1 MiB cap) | Future sub-project; needs IPC frame refactor for chunked Tool results |
| Background / long-running jobs | Out of scope; one invoke = one subprocess |
| Tool conformance suite | Revisit at N≥3 tool plugins |
| Schema validation in tau-runtime (`PluginContractViolation`) | ROADMAP Tier 2 priority 6 |
| Sandboxing | ROADMAP Tier 3 priority 12 |
| `fs-write` tool | Separate sub-project; the `fs.write` capability already exists in tau-domain |
| `http-fetch` tool | Separate sub-project; `net.http` capability already exists |

### 11.1 The `fs-read` multi-method follow-up

Per user-locked Q1, a follow-up sub-project will expand `fs-read` to:

- `read_file` (already shipped here)
- `list_dir`: `{path: String} → {entries: [{name, is_dir, size}]}` —
  capability `fs.read` (same).
- `stat`: `{path: String} → {size, modified, is_dir, is_symlink}` —
  capability `fs.read` (same).

These are mechanical follow-ons; they reuse `path_check.rs` verbatim.

### 11.2 The `shell` expansion follow-up

- `stdin: Option<bytes>` argument.
- `env: Option<BTreeMap<String, String>>` argument (overlay on
  inherited env? Or full override? Decided in the follow-up
  sub-project's brainstorm).
- `signal: Option<Signal>` for graceful shutdown (SIGTERM before
  SIGKILL).

---

## 12. Implementation plan outline (~13 tasks)

| # | Task | Notes |
|---|---|---|
| 1 | Workspace scaffold: 2 new crates (`fs-read`, `shell`) registered; placeholder `lib.rs`/`main.rs` stubs; tau.toml manifests | Also adds `globset` to fs-read's per-crate `[dependencies]` (NOT workspace). |
| 2 | `tau_ports::SessionContext.granted_capabilities` additive field | One-field amendment per §5.2 decision A. tau-runtime populates from package manifest. |
| 3 | `fs-read` config + path_check (10 unit tests) | spec §4, §6.1 |
| 4 | `fs-read` plugin.rs `Tool` impl (1 unit test for invoke) | spec §4 |
| 5 | `fs-read` integration tests (3 tests) | spec §8.1 |
| 6 | `shell` config (validate default ≤ max) + command_check (4 unit tests) | spec §5, §6.2 |
| 7 | `shell` exec.rs (subprocess spawn + timeout + capping; 8 unit tests) | spec §5.3, §5.4 |
| 8 | `shell` plugin.rs `Tool` impl | spec §5 |
| 9 | `shell` integration tests (4 tests) + per-plugin README.md inserts (trust model) | spec §8.2, §10 |
| 10 | tau-runtime: populate `SessionContext.granted_capabilities` at dispatch (`run.rs:272`-adjacent) | spec §5.2 |
| 11 | CI: 2 new release-build jobs in ci.yml | spec §9 |
| 12 | Final local verification + mark PR ready (user-driven gate) | (gate) |
| 13 | ROADMAP + branch protection 21 → 23 + squash merge (user-driven gate) | (gate) |

13 tasks. Tasks 12-13 are user-driven gates per the established pattern. **No ADR sign-off.**

---

## 13. Cross-references

- [ADR-0008](../../decisions/0008-plugin-loading.md) — first real Tool consumers; `Tool` trait dispatch end-to-end.
- [ADR-0007](../../decisions/0007-tau-cli.md) §4 — capability override scaffolding (the runtime side that this sub-project's plugins are the FIRST consumers of).
- [ADR-0002](../../decisions/0002-manifest-format.md) §3 — capability TOML canonicalization (the plugins' tau.toml uses this format).
- [ADR-0009](../../decisions/0009-llm-error-typing-and-conformance.md) — typed-error policy applies to `ToolError` mapping in this sub-project.
- [ROADMAP](../../../ROADMAP.md) Phase 1 priority 3 — marked complete on sub-project sign-off.
- [Constitution](../../../CONSTITUTION.md) G12 (sandboxing deferred), G14 (capabilities enforced).

## 14. Open follow-ups

- **`fs-read` multi-method** (§11.1) — natural next sub-project; fully scoped here.
- **`shell` expansion** (§11.2) — stdin / env / signal handling.
- **`fs-write` tool plugin** — separate sub-project; tau-domain already types `fs.write` capability with `max_bytes`.
- **`http-fetch` tool plugin** — separate sub-project; uses `net.http` capability.
- **Tool conformance suite** — revisit when ≥3 tool plugins exist.
- **Sandboxing** — ROADMAP Tier 3 priority 12.
- **Schema validation activation** — ROADMAP Tier 2 priority 6.
