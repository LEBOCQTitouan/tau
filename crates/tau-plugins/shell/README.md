# `shell` tool plugin

Run an allow-listed subprocess with a wall-clock timeout and 1 MiB
output cap. The agent's `process.spawn` capability carries the
command-name allowlist that constrains which programs are admissible.

## Trust model (v0.1, sandboxing deferred)

This plugin runs **unsandboxed** on the host process. The runtime
enforces capability checks at dispatch (`run.rs:272`); the plugin
enforces command-name allowlist scoping at invoke time. Beyond that,
there is **no memory / CPU / network isolation**. A spawned process
can exhaust system resources up to the wall-clock timeout. The
allowlist is per-name, **NOT per-argv** — `rm` on the allowlist with
arg `-rf /` is admitted.

Constitution G12 + ROADMAP Tier 3 priority 12 will add OS-level
sandboxing in a future sub-project. Until then, operators MUST treat
installed plugins as host-equivalent code AND audit every command
they put on an agent's allowlist.

## Usage

In your project `tau.toml`, declare the agent's grant:

```toml
[[agents.<id>.requires]]
plugin = "shell"

[[agents.<id>.capabilities]]
kind = "process.spawn"
commands = ["echo", "ls", "cargo"]
```

The agent invokes the tool with:

```json
{
  "command": "cargo",
  "args": ["test", "--workspace"],
  "timeout_secs": 120,
  "cwd": "/abs/path/to/project"
}
```

Response shape:

```json
{
  "stdout": "<captured stdout, possibly truncated>",
  "stderr": "<captured stderr, possibly truncated>",
  "exit_code": 0,
  "timed_out": false,
  "stdout_truncated": false,
  "stderr_truncated": false
}
```

## Behavior

- **Timeout:** wall-clock from spawn (NOT idle reset on output).
  Default 30s, max 600s. Per-call override via `timeout_secs`.
  On timeout: SIGKILL + reap; partial buffers returned with
  `timed_out: true, exit_code: -1`.
- **Output cap:** each of stdout / stderr capped at 1 MiB. Excess
  truncated; the corresponding `*_truncated: bool` flag set.
- **No env inheritance:** spawned process gets a clean environment
  (`env_clear()`).
- **No stdin:** spawned process's stdin is `/dev/null`.

## Validation rules

- `command` required; non-empty; must match agent's
  `ProcessCapability::Spawn.commands` allowlist.
- `args` optional array of strings; defaults to empty.
- `timeout_secs` optional positive integer; clamped to
  `[1, max_timeout_secs]`.
- `cwd` optional absolute path string.

A non-zero exit code surfaces as `ToolResult { is_error: true }` —
the LLM may decide to retry or interpret the failure.

## See also

- Spec: [`docs/superpowers/specs/2026-04-29-tool-plugins-design.md`](../../../docs/superpowers/specs/2026-04-29-tool-plugins-design.md)
- ADR-0008 §5 (IPC vocabulary).
