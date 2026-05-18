# Write a tool plugin

You want to add a new tool that an agent can invoke. This recipe
walks through the minimum viable plugin: a single-method tool
following the same shape as the in-tree `fs-read` reference
implementation.

If you don't yet know the model, read
[Packages](../explanation/packages.md) and
[Capabilities and consent](../explanation/capabilities-and-consent.md)
first. For the wire details, [ADR-0008](../decisions/0008-plugin-loading.md)
is the canonical reference.

## What you'll end up with

```
my-tool/
├── Cargo.toml          # crate manifest
├── tau.toml            # tau package manifest
├── src/
│   ├── main.rs         # binary entry point
│   ├── lib.rs          # exposed for tests
│   ├── plugin.rs       # Tool impl
│   └── config.rs       # FsReadConfig-style typed config
└── tests/
    └── integration.rs  # via tau-plugin-test-support
```

The binary will be spawned by `tau-runtime::plugin_host` as a
subprocess, will speak MessagePack-RPC over stdio per ADR-0008, and
will be sandboxed under the user's chosen tier.

## Step 1 — minimal `Cargo.toml`

```toml
[package]
name             = "my-tool"
version.workspace      = true
edition.workspace      = true
rust-version.workspace = true

[[bin]]
name = "my-tool-plugin"
path = "src/main.rs"

[lib]
name = "my_tool_plugin_lib"
path = "src/lib.rs"

[dependencies]
tau-domain          = { workspace = true, features = ["serde"] }
tau-ports           = { workspace = true, features = ["serde", "test-fixtures"] }
tau-plugin-protocol = { workspace = true }
tau-plugin-sdk      = { workspace = true }
serde               = { workspace = true }
serde_json          = "1"
tokio               = { workspace = true, features = ["macros", "rt", "rt-multi-thread"] }
thiserror           = { workspace = true }

[dev-dependencies]
tau-plugin-protocol = { workspace = true, features = ["test-support"] }
```

The `[[bin]]` name is what `tau.toml`'s `[plugin] bin = "..."`
references. The `[lib]` exposes types for your integration tests.

## Step 2 — `tau.toml` package manifest

```toml
name        = "my-tool"
version     = "0.1.0"
description = "Read a single integer from a JSON file at an allow-listed path."

[plugin]
provides = "tool"
kind     = "rust-cargo"
bin      = "my-tool-plugin"

[sandbox]
required_tier = "strict"

[[capabilities]]
kind  = "fs.read"
paths = []
```

What the runtime needs:

- `[plugin].provides = "tool"` — tells the host to load this as a
  `Tool` implementer (`tau-ports::Tool`).
- `[plugin].bin` — must match the `[[bin]] name` in `Cargo.toml`.
- `[sandbox].required_tier` — declare the floor your plugin requires.
- `[[capabilities]]` — every shape your tool needs. Empty `paths`
  is fine here; the agent's grant fills in concrete paths at run
  time via `SessionContext.granted_capabilities`.

For the full schema, see the [package manifest
reference](../reference/package-manifest-schema.md).

## Step 3 — `src/main.rs`: the shim

The binary itself is three lines of plumbing — `tau-plugin-sdk`
handles the rest:

```rust
use my_tool_plugin_lib::plugin::MyToolPlugin;
use tau_plugin_sdk::{run_tool_with_config, SdkError};

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    run_tool_with_config::<MyToolPlugin>(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    ).await
}
```

`run_tool_with_config` performs the handshake with the runtime,
deserializes the `[config]` block from the agent definition into
`MyToolPlugin::Config`, constructs the plugin via
`Configure::from_config`, and runs the dispatch loop until the
parent closes stdin.

## Step 4 — `src/plugin.rs`: the `Tool` impl

The interesting code. Two traits to implement: `Configure` (the
SDK trait that lets `run_tool_with_config` build your plugin) and
`Tool` (the `tau-ports` trait that defines the wire contract).

```rust
use tau_domain::Value;
use tau_plugin_sdk::{ConfigError, Configure};
use tau_ports::{
    fixtures::{make_tool_result, make_tool_spec},
    SessionContext, Tool, ToolContent, ToolError, ToolResult, ToolSpec,
};
use serde_json::json;

use crate::config::MyToolConfig;

pub struct MyToolPlugin {
    config: MyToolConfig,
}

impl Configure for MyToolPlugin {
    type Config = MyToolConfig;

    fn from_config(config: Self::Config) -> Result<Self, ConfigError> {
        Ok(Self { config })
    }
}

pub struct MySession {
    allowed_paths: Vec<String>,
}

impl Tool for MyToolPlugin {
    type Session = MySession;

    fn name(&self) -> &str { "my-tool" }

    fn schema(&self) -> ToolSpec {
        let schema_json = json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"],
        });
        let schema: Value =
            serde_json::from_str(&serde_json::to_string(&schema_json).unwrap()).unwrap();
        make_tool_spec(
            "my-tool".to_string(),
            "Read an integer from a JSON file.".to_string(),
            schema,
        )
    }

    fn init(&self, ctx: &SessionContext) -> Result<Self::Session, ToolError> {
        // Extract the agent's granted fs.read paths from the context.
        let allowed_paths = ctx
            .granted_capabilities
            .iter()
            .filter_map(|cap| match cap {
                tau_domain::Capability::Filesystem(
                    tau_domain::FsCapability::Read { paths }
                ) => Some(paths.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        Ok(MySession { allowed_paths })
    }

    async fn invoke(
        &self,
        session: &mut Self::Session,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        // Parse args, glob-check against session.allowed_paths,
        // read the file, return the result.
        // (Error mapping + path validation omitted for brevity —
        //  see crates/tau-plugins/fs-read/src/plugin.rs for the
        //  full pattern.)
        Ok(make_tool_result(vec![ToolContent::text("42".to_string())]))
    }
}
```

What the kernel guarantees, so you don't have to re-check:

- **Capability gating already happened wire-side.** By the time
  `invoke` runs, the agent has been verified to hold a capability
  matching your tool's required shape. You still re-check inside
  the tool — both as defense-in-depth and to enforce the *payload*
  of the capability (specific paths, hosts, methods).
- **Sandbox already happened OS-side.** Your subprocess runs under
  landlock / sandbox-exec / AppContainer with the kernel rules
  derived from the same grant. A path traversal attempt fails at
  the kernel; you're returning an error before getting there only
  for cleanliness.

For path validation patterns, look at `crates/tau-plugins/fs-read/
src/path_check.rs` — `validate_path` (no `..`, must be absolute)
and `admit_with_deny` (allow + deny glob lists).

## Step 5 — `src/lib.rs`

```rust
pub mod config;
pub mod plugin;
```

Two pub mods so integration tests can import. That's it.

## Step 6 — `src/config.rs`: typed config

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MyToolConfig {
    // Per-project tunables: timeouts, formatting toggles, etc.
    // Empty is fine if your tool has nothing to configure.
}
```

The kernel forwards a `[agents.<id>.config]` table from the
project's `tau.toml` to this struct verbatim. `deny_unknown_fields`
catches typos at handshake time.

## Step 7 — integration test

```rust
// tests/integration.rs
use my_tool_plugin_lib::plugin::MyToolPlugin;
use tau_plugin_test_support::{InProcessHost, granted};
use tau_domain::{Capability, FsCapability};

#[tokio::test]
async fn reads_an_allowed_path() {
    let tmp = tempfile::tempdir().unwrap();
    let f = tmp.path().join("answer.json");
    std::fs::write(&f, r#"{"value": 42}"#).unwrap();

    let host = InProcessHost::<MyToolPlugin>::new(
        granted(vec![Capability::Filesystem(FsCapability::Read {
            paths: vec![format!("{}/**", tmp.path().display())],
        })]),
    );

    let result = host
        .invoke("my-tool", serde_json::json!({ "path": f }))
        .await
        .unwrap();
    assert_eq!(result.content.first().unwrap().text(), "42");
}
```

`tau-plugin-test-support` exposes `InProcessHost` so you can test
the plugin without spawning a subprocess — the handshake +
dispatch are mocked in-process. For full-subprocess CI coverage,
[`tau-plugin-compat`](../explanation/crate-map.md) drives the
"spawn → handshake → invoke" path against real `tau-runtime`.

## Step 8 — install it locally

```bash
# Build the plugin binary.
cargo build --release -p my-tool

# Add it to your project tau.toml as a tool dependency.
# [[agents.example.requires.tools]]
# name   = "my-tool"
# source = "file:///path/to/my-tool"

tau install file:///path/to/my-tool
tau chat example
```

The first install prompts for capability consent (G14). Subsequent
runs use the lockfile.

## What ships with the in-tree plugins

These shipped plugins are good real-code references:

- `tau-plugins/fs-read/` — the simplest production-quality tool.
  Read this first.
- `tau-plugins/shell/` — process spawn, output capping, timeout
  handling.
- `tau-plugins/anthropic/`, `openai/`, `ollama/` — LlmBackend port
  (different trait, same scaffolding).

## See also

- [Packages](../explanation/packages.md) — what the package model
  guarantees about your plugin.
- [Capabilities and consent](../explanation/capabilities-and-consent.md)
  — the grant model your `invoke` runs under.
- [Package manifest schema](../reference/package-manifest-schema.md)
  — full `tau.toml` schema.
- [Crate map](../explanation/crate-map.md) — `tau-plugin-sdk` vs
  `tau-plugin-base` vs `tau-plugin-protocol`.
- [Architecture overview](../explanation/architecture-overview.md)
  — where your plugin sits in the request path.
- [ADR-0008](../decisions/0008-plugin-loading.md) — wire format,
  handshake, message loop.
