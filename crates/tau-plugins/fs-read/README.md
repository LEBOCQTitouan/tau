# `fs-read` tool plugin

Read bytes from a single absolute path under the calling agent's
`fs.read` capability scope.

## Trust model (v0.1, sandboxing deferred)

This plugin runs **unsandboxed** on the host process. The runtime
enforces capability checks at dispatch (`run.rs:272`); the plugin
enforces glob-allowlist scoping at invoke time. Beyond that, there
is **no memory / CPU / network isolation**. Constitution G12 +
ROADMAP Tier 3 priority 12 will add OS-level sandboxing in a future
sub-project. Until then, operators MUST treat installed plugins as
host-equivalent code.

## Usage

In your project `tau.toml`, declare the agent's grant:

```toml
[[agents.<id>.requires]]
plugin = "fs-read"

[[agents.<id>.capabilities]]
kind = "fs.read"
paths = ["${PROJECT}/src/**", "${PROJECT}/docs/**"]
```

The agent invokes the tool with:

```json
{ "path": "/absolute/path/to/file.txt" }
```

Response shape:

```json
{
  "contents": "<base64-encoded bytes>",
  "size": 1234
}
```

## Validation rules

- Path must be **absolute** (relative paths rejected with `BadArgs`).
- Path must NOT contain `..` segments (traversal rejected with `BadArgs`).
- Path must NOT contain NUL bytes.
- Path must match at least one glob in the agent's
  `FsCapability::Read.paths` grant (out-of-scope rejected with
  `BadArgs`).

IO errors (file not found, permission denied) are surfaced as
`ToolResult { is_error: true }` with the OS error in the content,
NOT as `ToolError` — the LLM may decide to retry.

## See also

- Spec: [`docs/superpowers/specs/2026-04-29-tool-plugins-design.md`](../../../docs/superpowers/specs/2026-04-29-tool-plugins-design.md)
- ADR-0008 §5 (IPC vocabulary).
