# tau-cli (sub-project 5) design

**Date:** 2026-04-28
**Sub-project:** 5 of Phase 0 (last before retrospective)
**Status:** Spec — derived via the brainstorming flow; plan + implementation follow.

`tau-cli` is the CLI binary for tau Phase 0 — the `tau` executable produced
by `crates/tau-cli`. It exposes the kernel (`tau-runtime`) and package
manager (`tau-pkg`) to end users via five subcommands. It is the first
sub-project that produces a binary rather than a library and the last
Phase-0 step before the formal retrospective.

This spec applies the carry-overs documented in
`docs/retrospectives/phase-0-mid.md` preemptively (spec-phase pre-flight on
`#[non_exhaustive]` constructors, dyn-compatibility, test visibility; no
ship of error variants without triggering codepaths; `cargo test --doc`
runs separately).

---

## 1. Scope & success criteria

### Scope

Five subcommands, in order of complexity:

| Subcommand | Purpose | Backed by |
|---|---|---|
| `tau init` | Scaffold a project `tau.toml` at cwd | tau-cli only (writes file) |
| `tau install <git-url>` | Install a package from git into the resolved scope | `tau_pkg::install` |
| `tau list [packages\|agents]` | List installed packages or project-declared agents | `tau_pkg::list` + project `tau.toml` parser |
| `tau run <agent_id> [prompt]` | One-shot invocation of an agent | `tau_runtime::Runtime::run` |
| `tau chat <agent_id>` | Interactive REPL with an agent (in-memory history) | `tau_runtime::Runtime::run_with_history` |

Two additive amendments to `tau-runtime` ship in this sub-project:

1. **Capability-filtered tools in `CompletionRequest.tools`** — the kernel
   filters the tool set exposed to the LLM to only tools whose
   `Tool::capabilities()` is satisfied by the agent package's capability
   grants. Refines ADR-0006's typed-capability story.
2. **`Runtime::run_with_history(agent_def, manifest, history, initial_message, options)`**
   — additive entry point for REPL history threading.
   `Runtime::run(...)` becomes a thin wrapper calling
   `run_with_history(... vec![] ...)`.

ADR-0007 covers tau-cli's design + both tau-runtime amendments (bundled per
the ADR-0006 precedent).

### Done when

- All five subcommands work end-to-end against installed packages and
  project-declared agents.
- Exit codes follow the 3-bucket taxonomy (0 / 1 / 2).
- Tracing emits to stderr at INFO by default; `-v / -vv / --quiet / --debug`
  and `RUST_LOG` extend; default scoped to `tau=*`.
- `tau run --json` and other JSON-supporting subcommands emit a stable
  schema covered by `insta` snapshot tests.
- `tau chat` REPL works with rustyline + termimad, four slash commands
  (`/exit`, `/help`, `/clear`, `/history`), in-memory history.
- `--dry-run` lands on `install`, `run`, `init`, `chat` (not `list`) with
  `[dry-run]` stderr prefix and exit 0/2 per validation outcome.
- Branch protection on `main` gains `build (tau-cli no-default-features)`
  after merge.

### Out of scope (deferred to Phase 1+)

- `tau uninstall`, `tau update`, `tau verify` (Phase 1 features).
- **Per-agent capability override** — `[agents.<id>.capabilities]` schema
  slot reserved at v0.1 and rejected with a clear error pointing at
  Phase 1+. Necessary to add later; intersect-only semantics committed by
  this rejection.
- **`requires.packages`** transitive resolution (Phase 1+; auto-install).
  v0.1 only ships `requires.tools` as advisory check.
- Persisted REPL session history (`tau chat --resume`).
- Multi-line REPL input beyond `\` continuation; pretty terminal-mode
  features (arrow-key history visualization, color rendering verification).
- `tau workflow` / `tau orchestrate` / multi-agent coordination (Phase 1+
  per G10).
- TUI mode for `tau chat`.
- `tau ls tools`, `tau ls llm-backends` (extensible noun reserved; only
  `packages` and `agents` ship at v0.1).
- JSON tracing format (`--log-format json`) for production deployments.

---

## 2. Module layout & dependencies

```
crates/tau-cli/
├── Cargo.toml
├── src/
│   ├── main.rs           # #[tokio::main] entry; thin wrapper around lib::run_main
│   ├── lib.rs            # pub mod cli/cmd/exit/output/tracing/config; pub use for tests
│   ├── cli.rs            # clap derive: Cli + Command + per-subcommand args
│   ├── exit.rs           # ExitCode { Success=0, AgentFailed=1, Error=2 } + From impls
│   ├── output.rs         # stdout/stderr discipline, color detection, JSON writer
│   ├── tracing.rs        # tracing-subscriber setup (-v/--quiet/--debug/RUST_LOG)
│   ├── config/
│   │   ├── mod.rs        # public surface for config parsing
│   │   ├── project.rs    # ProjectConfig (parses [project] + [agents.<id>])
│   │   └── agent.rs      # AgentEntry; conversion to tau_domain::AgentDefinition
│   └── cmd/
│       ├── mod.rs
│       ├── install.rs    # tau install <git-url>
│       ├── list.rs       # tau list [packages|agents]
│       ├── run.rs        # tau run <agent_id> [prompt]
│       ├── init.rs       # tau init
│       └── chat.rs       # tau chat <agent_id>
└── tests/
    ├── common/
    │   └── mod.rs        # temp_project_with_agent, scripted_llm_package, etc.
    ├── help_snapshots.rs       # insta snapshots: tau --help + per-subcommand
    ├── json_schemas.rs         # insta JSON snapshots for every --json shape
    ├── cmd_install.rs
    ├── cmd_list.rs
    ├── cmd_run.rs
    ├── cmd_init.rs
    ├── cmd_chat.rs
    ├── exit_codes.rs           # matrix of (subcommand × scenario) → exit code
    ├── error_display.rs        # default vs --debug error rendering
    ├── color.rs                # --color always|auto|never; NO_COLOR
    ├── dry_run.rs              # per-subcommand --dry-run no-state-change verification
    ├── tracing_emission.rs     # custom Layer captures events on tau run happy path
    └── cross_platform.rs       # file:// URL normalization, line endings
```

### Cargo.toml additions

`crates/tau-cli/Cargo.toml`:

```toml
[dependencies]
tau-domain         = { workspace = true, features = ["serde"] }
tau-pkg            = { workspace = true }
tau-runtime        = { workspace = true }
tau-ports          = { workspace = true }                 # for plugin discovery
clap               = { version = "4", features = ["derive", "env"] }
tokio              = { workspace = true, features = ["macros", "rt", "rt-multi-thread"] }
tracing            = { workspace = true }
tracing-subscriber = { version = "0.3", features = ["fmt", "env-filter"] }
serde              = { workspace = true }
serde_json         = "1"
toml               = { workspace = true }
thiserror          = { workspace = true }
anyhow             = "1"                                  # CLI-side error chain printing
termimad           = "0.30"                               # Markdown rendering for tau chat
rustyline          = "14"
is-terminal        = "0.4"

[dev-dependencies]
assert_cmd         = "2"
insta              = { version = "1", features = ["yaml", "json"] }
predicates         = "3"
proptest           = { workspace = true }
tempfile           = "3"
tau-ports          = { workspace = true, features = ["test-fixtures"] }

[features]
default = []
```

`anyhow` is added at the workspace level (new workspace dep) since other
crates may want CLI-side error context wrapping in the future. If only
tau-cli needs it for v0.1, it stays as a tau-cli-only dep.

`tracing-subscriber`, `termimad`, `rustyline`, `is-terminal`, `clap`,
`assert_cmd`, `insta`, `predicates`, `tempfile` are new direct deps for
tau-cli; not added to `[workspace.dependencies]` unless re-used.

### tau-runtime amendment additions

`crates/tau-runtime/Cargo.toml` is unchanged (no new deps for the
amendments; both reuse existing infrastructure).

---

## 3. Component design

### 3.1 CLI argument parser (`cli.rs`)

clap v4 derive. Top-level:

```rust
#[derive(Parser, Debug)]
#[command(name = "tau", version, about = "tau runtime CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Verbosity level. Repeat for more (-v: DEBUG, -vv: TRACE).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Suppress non-error output (sets tracing to WARN).
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Show full error chain + DEBUG-level tracing on failures.
    #[arg(long, global = true)]
    pub debug: bool,

    /// Color mode for terminal output.
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub color: ColorMode,

    /// Emit structured JSON output instead of human-readable.
    /// Only supported on `install`, `list`, `run`, `init`.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Init(InitArgs),
    Install(InstallArgs),
    List(ListArgs),
    Run(RunArgs),
    Chat(ChatArgs),
}
```

Per-subcommand structs:

```rust
#[derive(Args, Debug)]
pub struct InitArgs {
    /// Overwrite an existing tau.toml.
    #[arg(long)]
    pub force: bool,
    /// Print what would be created without writing files.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct InstallArgs {
    /// Git URL of the package to install.
    pub url: String,
    /// Install into the global scope (~/.tau) instead of the project scope.
    #[arg(long)]
    pub global: bool,
    /// Validate the install without writing to disk or updating the lockfile.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// What to list. Defaults to `packages`.
    #[arg(value_enum, default_value = "packages")]
    pub resource: ListResource,
    #[arg(long, conflicts_with = "all")]
    pub global: bool,
    #[arg(long)]
    pub all: bool,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    pub agent_id: String,
    /// Prompt for the agent. If omitted, read from stdin.
    pub prompt: Option<String>,
    /// Override RunOptions::max_turns.
    #[arg(long)]
    pub max_turns: Option<u32>,
    /// Validate setup without invoking the LLM.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug)]
pub struct ChatArgs {
    pub agent_id: String,
    #[arg(long)]
    pub max_turns: Option<u32>,
    /// Validate setup without entering the REPL.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum ListResource {
    Packages,
    Agents,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum ColorMode {
    Always,
    Auto,
    Never,
}
```

`--json` is rejected at runtime for `tau chat` (REPL is interactive); the
parser accepts it, the chat handler errors with a clear message.

### 3.2 Project tau.toml schema (`config::project`)

Distinct from the package-level `tau.toml` (per ADR-0002). The Cargo
precedent: the same filename hosts `[package]` (per-crate metadata) and
`[workspace]` (workspace-level). tau follows the same pattern: a single
filename, two distinct roles:

- **Package `tau.toml`** (per ADR-0002): defines a single distributable
  package. Found at the root of a cloned package's repo. Contains `name`,
  `version`, `kind`, `source`, `[[capabilities]]`, `[[dependencies]]`.
- **Project `tau.toml`** (this sub-project): defines a project's local
  agent surface. Found at the root of a user's project. Contains
  `[project]` and `[agents.<id>]` named tables.

A directory MAY have only the project tau.toml (the user's project root)
or only the package tau.toml (a cloned package repo) but never both.

Schema:

```toml
[project]
name        = "my-project"            # required, free-form string
description = ""                       # optional

[agents.<id>]
display_name = "..."                  # required
package      = "<name>@<semver-req>"  # required; resolves against installed packages
llm_backend  = "<name>"               # required; resolves against installed llm-backend kind

# Optional sub-tables; all may be omitted.

[agents.<id>.requires]
tools = ["<package-name>", ...]       # advisory at v0.1 (error if missing at run start)
# packages = [...]                    # Phase 1+: non-tool transitive dep auto-install

[agents.<id>.capabilities]            # RESERVED FOR PHASE 1+
# Presence at v0.1 is a hard error pointing at the Phase 1+ roadmap entry.
# Phase 1+ semantics: intersect with the package's declared grants
# (cannot expand). The schema slot exists now to lock in those semantics.

[agents.<id>.config]                  # free-form; passed to AgentDefinition.config
key1 = "value1"
nested = { key2 = "value2" }

[agents.<id>.prompt]
system      = "..."                   # OR
system_file = "path/to/prompt.md"     # mutually exclusive — parser enforces exactly one
```

**Schema rules:**

- `[project].name` is the only mandatory project field.
- `[agents.<id>]` is a TOML named table; agent ids are unique by TOML semantics.
- Within `[agents.<id>]`: `display_name`, `package`, `llm_backend` are required.
- All sub-tables are optional individually; presence triggers per-table validation.
- `[agents.<id>.capabilities]` triggers a hard parse error at v0.1: see
  the dedicated error variant in §3.4.
- `[agents.<id>.prompt]` accepts `system` XOR `system_file`; both → error;
  neither → no system prompt.
- `package` parses as a name + semver requirement. Resolution against the
  scope's installed packages picks the highest-version match; no match → error.
- `llm_backend` is a plain name (no version). Resolution picks the active
  version per the lockfile; missing → error.

Parsing implementation: serde derive on `UncheckedProjectConfig` →
`validate() -> Result<ProjectConfig, ProjectConfigError>` mirrors
tau-domain's manifest pattern. Toml parsed via the workspace `toml` dep.

### 3.3 AgentEntry → AgentDefinition conversion (`config::agent`)

Pure function:

```rust
pub fn build_agent_definition(
    entry: &AgentEntry,
    project_root: &Path,
    scope: &Scope,
) -> Result<(AgentDefinition, PackageManifest), AgentResolutionError>;
```

Steps:
1. Resolve `entry.package` (name + semver req) against `tau_pkg::list(scope)`;
   pick highest-version match. Error variants: `PackageNotFound`,
   `PackageVersionUnsatisfied`.
2. Read the resolved package's manifest via `tau_pkg::read_manifest(...)`.
   Error: `ManifestRead`.
3. Resolve `entry.llm_backend` against installed packages of `kind = llm-backend`.
   Error: `LlmBackendNotFound`.
4. If `entry.requires.tools` present, verify each named tool package is
   installed (via `tau_pkg::registry::get`). Error: `RequiredToolMissing`.
5. If `entry.capabilities` present, error `CapabilityOverrideUnsupported`
   with a Phase 1+ pointer.
6. Read `prompt.system` or `prompt.system_file` (path relative to
   `project_root`) into `Option<String>`. Error on file read failure
   includes path.
7. Construct `AgentDefinition::new(...)` chained with `.with_system_prompt(...)`
   and `.with_config(...)`. Returns the manifest alongside for the kernel.

### 3.4 Error taxonomy (`config::project::ProjectConfigError`, etc.)

Per the phase-0-mid memo: do NOT ship variants without a triggering
codepath. v0.1 surface:

```rust
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ProjectConfigError {
    #[error("project tau.toml not found in scope (run `tau init` to create one)")]
    NotFound,

    #[error("failed to read project tau.toml at {path:?}: {source}")]
    Read { path: PathBuf, #[source] source: std::io::Error },

    #[error("failed to parse project tau.toml at {path:?}: {source}")]
    Parse { path: PathBuf, #[source] source: toml::de::Error },

    #[error("agent {id:?}: {message}")]
    AgentValidation { id: String, message: String },

    #[error("agent {id:?}: capability override is not supported at v0.1; \
             see Phase 1+ roadmap entry: docs/retrospectives/phase-0-mid.md \
             §'What's NOT in scope for this memo'")]
    CapabilityOverrideUnsupported { id: String },

    #[error("agent {id:?}: prompt requires exactly one of `system` or `system_file`")]
    PromptAmbiguous { id: String },
}
```

Conversion errors:

```rust
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum AgentResolutionError {
    #[error("agent {agent_id:?} package {package:?} not installed in scope (run `tau install {url}`)")]
    PackageNotFound { agent_id: String, package: String, url: String },

    #[error("agent {agent_id:?} package {package:?} matches no installed version satisfying requirement {req:?}")]
    PackageVersionUnsatisfied { agent_id: String, package: String, req: String },

    #[error("agent {agent_id:?} llm backend {backend:?} not installed (run `tau install <backend-url>`)")]
    LlmBackendNotFound { agent_id: String, backend: String },

    #[error("agent {agent_id:?} requires.tools entry {tool:?} not installed in scope")]
    RequiredToolMissing { agent_id: String, tool: String },

    #[error("agent {agent_id:?}: failed to read manifest: {source}")]
    ManifestRead { agent_id: String, #[source] source: tau_pkg::ManifestReadError },

    #[error("agent {agent_id:?}: prompt file {path:?} read failed: {source}")]
    PromptFileRead { agent_id: String, path: PathBuf, #[source] source: std::io::Error },
}
```

CLI-level errors (top-level user-facing): wrapped via `anyhow::Context`.
Display defaults to top-level only; `--debug` expands.

### 3.5 Exit code mapping (`exit::ExitCode`)

```rust
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Success = 0,
    AgentFailed = 1,
    Error = 2,
}

impl From<&tau_runtime::RunOutcome> for ExitCode {
    fn from(outcome: &tau_runtime::RunOutcome) -> Self {
        match outcome {
            tau_runtime::RunOutcome::Completed { .. } => ExitCode::Success,
            tau_runtime::RunOutcome::Failed { .. } => ExitCode::AgentFailed,
            // No catch-all needed: RunOutcome is #[non_exhaustive] but currently
            // 2-variant; new variants would be a tau-runtime breaking change
            // covered by an ADR amendment.
        }
    }
}

impl From<&anyhow::Error> for ExitCode {
    fn from(_: &anyhow::Error) -> Self {
        ExitCode::Error
    }
}
```

`main` returns `std::process::ExitCode` constructed from the above.

### 3.6 Output discipline (`output`)

Three writers selected per the global flags:

```rust
pub struct Output {
    stdout: Box<dyn Write + Send>,    // ALWAYS goes to stdout
    stderr: Box<dyn Write + Send>,    // status, tracing, errors
    json: bool,                        // global --json mode
    color: ColorChoice,                // resolved from --color + NO_COLOR + is_terminal
}

impl Output {
    pub fn from_cli(cli: &Cli) -> Self;
    pub fn human<W: Display>(&mut self, w: &W) -> Result<()>;       // human-readable to stdout
    pub fn json<T: Serialize>(&mut self, v: &T) -> Result<()>;      // JSON to stdout
    pub fn status(&mut self, msg: impl Display) -> Result<()>;      // INFO line to stderr
    pub fn dry_run(&mut self, line: impl Display) -> Result<()>;    // "[dry-run] ..." to stderr
    pub fn warn(&mut self, msg: impl Display) -> Result<()>;        // WARN to stderr (yellow if color)
    pub fn error(&mut self, msg: impl Display) -> Result<()>;       // ERROR to stderr (red if color)
}
```

JSON schema per subcommand documented as insta snapshots in
`tests/json_schemas.rs`. Output module enforces:
- `--json` set: only `json()` emits to stdout. `human()` becomes a no-op
  unless explicitly bypassed.
- `--quiet` set: `status()` becomes a no-op.
- Color: ANSI escapes added by termimad/by `Output::warn`/`error`
  conditional on resolved `ColorChoice`.

### 3.7 Tracing config (`tracing`)

```rust
pub fn build_filter(cli: &Cli) -> EnvFilter {
    if let Ok(env) = std::env::var("RUST_LOG") {
        return EnvFilter::new(env);
    }

    let level = if cli.verbose >= 2 {
        "trace"
    } else if cli.debug || cli.verbose >= 1 {
        "debug"
    } else if cli.quiet {
        "warn"
    } else {
        "info"
    };

    // Default scope to `tau=*` so plugin tracing doesn't flood unless
    // explicitly enabled via RUST_LOG.
    EnvFilter::new(format!("tau={level}"))
}

pub fn install(cli: &Cli) {
    let filter = build_filter(cli);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
```

`-vv` → TRACE. `--debug` is the combined "everything" knob (DEBUG tracing
+ full error chain at print time). `--quiet` overrides nothing if `-v` is
set (clap's `conflicts_with = "verbose"` enforces).

`tau chat` adjusts: REPL handler installs the filter at WARN baseline
(REPL UX would be ruined by INFO-level events between turns), upgraded by
`-v` / `--debug`.

### 3.8 Subcommand handlers (`cmd::*`)

Each handler signature:

```rust
pub async fn run(args: &SubArgs, output: &mut Output, scope: &Scope) -> Result<ExitCode>;
```

Handlers are async to consume `Runtime::run` directly. Sync tau-pkg calls
run inline; long-running calls (git clone) tolerated by the multi-thread
runtime at v0.1 (see §3.10 risks).

### 3.9 Runtime construction (`cmd::run`, `cmd::chat`)

Both commands need the same plumbing — extracted into a helper:

```rust
async fn build_runtime_for_agent(
    agent_id: &str,
    scope: &Scope,
    project: &ProjectConfig,
) -> Result<(Runtime, AgentDefinition, PackageManifest)> {
    // 1. Find AgentEntry by id in project.
    // 2. Resolve → (AgentDefinition, PackageManifest) via build_agent_definition().
    // 3. Discover all installed plugin-kind packages in scope (via tau-pkg::list).
    // 4. Construct each as a plugin instance:
    //    - kind == "llm-backend" → register via with_llm_backend(...)
    //    - kind == "tool"        → register via with_tool(...)
    //    - kind == "storage"     → register via with_storage(...)
    //    - kind == "agent"       → skip (agents are runtime targets, not plugins)
    //    Plugin construction: dynamic dispatch via the plugin's library
    //    (subject to plugin-loading mechanism — see §3.11 Open Issues).
    // 5. RuntimeBuilder.build() → validates ≥1 LLM, no name collisions.
}
```

### 3.10 tau-runtime amendment 1: capability-filtered tools

In `crates/tau-runtime/src/run.rs`, the multi-turn loop's
`CompletionRequest.tools` is filtered:

```rust
let granted = package_manifest.capabilities();
let exposed_tools: Vec<(&str, ToolSpec)> = self
    .tools()
    .iter()
    .filter_map(|(name, tool)| {
        let required = tool.capabilities();
        if let Some(missing) = crate::capability::check_capabilities(granted, required) {
            tracing::warn!(
                name = "runtime.tool_filtered",
                tool_name = name.as_str(),
                missing_kind = ?capability_kind_str(missing),
                "tool filtered out: missing capability"
            );
            None
        } else {
            Some((name.as_str(), tool.schema()))
        }
    })
    .collect();

tracing::debug!(
    name = "runtime.tools_filtered",
    granted_count = granted.len(),
    total_tools = self.tools().len(),
    exposed_tools = exposed_tools.len(),
);
```

**Implications:**
- LLM never sees tools the agent can't use → smaller prompts, no spurious
  denials.
- Capability check at invoke time stays as defense-in-depth (catches bugs
  in the filter or future dynamic-capability tools).
- Behavioral change: previously, `Runtime::run` exposed all tools. No
  external consumers exist yet (sub-project 4 just shipped); change is
  observable via `CompletionRequest.tools` and the new tracing events.
- New tracing events: `runtime.tool_filtered` (WARN, per filtered tool),
  `runtime.tools_filtered` (DEBUG, summary). Vocabulary additive.
- No new error variant. Filtering is silent-but-traced; no
  `RuntimeError::ToolFiltered` (no triggering codepath that should error).

### 3.11 tau-runtime amendment 2: `Runtime::run_with_history`

```rust
impl Runtime {
    /// Run an agent with a pre-existing conversation history. The kernel
    /// translates `history` to `LlmProviderMessage`s and prepends them to
    /// the `CompletionRequest.messages` for turn 1.
    pub async fn run_with_history(
        &self,
        agent_def: AgentDefinition,
        package_manifest: PackageManifest,
        history: Vec<Message>,
        initial_message: Message,
        options: RunOptions,
    ) -> Result<RunOutcome, RuntimeError> {
        // ... same loop as run(...), but turn 1 pre-loads `history` into the
        // messages buffer before appending `initial_message`.
    }

    /// Convenience: run with empty history.
    pub async fn run(
        &self,
        agent_def: AgentDefinition,
        package_manifest: PackageManifest,
        initial_message: Message,
        options: RunOptions,
    ) -> Result<RunOutcome, RuntimeError> {
        self.run_with_history(agent_def, package_manifest, vec![], initial_message, options).await
    }
}
```

`Runtime::run`'s previous body moves into `run_with_history`; existing
callers keep working unchanged. `tau chat` accumulates `Vec<Message>` in
its REPL session and passes it to each `run_with_history` call.

No new error variant; no new tracing events (existing
`runtime.agent_run` span captures the run regardless of history shape;
`message.added` trace fires for each pre-loaded history message at
session start).

### 3.12 Plugin loading

**Open issue surfaced for the plan**: tau-pkg installs packages as source
trees under `.tau/packages/<name>/<version>/`. To register a package's
`LlmBackend`/`Tool`/`Storage` impl with the runtime, tau-cli must:

- Compile the package's library (cargo build) and dynamic-load it (dlopen,
  abi_stable, etc.); OR
- Embed plugin binaries; OR
- Compile-time link plugins into tau-cli (defeats the package-manager
  premise — every plugin install requires a tau-cli rebuild); OR
- Use a separate plugin runner (out-of-process IPC).

**Decision deferred to writing-plans phase**. v0.1 minimum: tau-cli ships
with a small set of compiled-in plugins (e.g., a built-in echo tool, a
mock LLM backend gated by a feature flag for testing). Real plugin
loading lands in Phase 1+ with its own ADR. This is a meaningful scope
limitation: at v0.1, `tau install` actually installs source trees, but
`tau run` only sees compiled-in plugins until Phase 1's plugin-loading
mechanism lands. Document explicitly.

This means v0.1 `tau install` is FUNCTIONAL (clones, validates, registers
in lockfile) but the installed packages aren't yet executable — they
become so once Phase 1 ships dynamic loading. Users who run `tau install`
at v0.1 see a successful install + a hint that the package will become
runnable in Phase 1.

### 3.13 REPL design (`cmd::chat`)

Components:

- **Line editor**: `rustyline` with default history (in-session only).
  `\` line continuation joins the next line with `\n`.
- **Slash command parser**: `parse_input(&str) -> ReplInput`:
  ```rust
  pub enum ReplInput {
      Slash(SlashCommand),
      Prompt(String),
  }
  pub enum SlashCommand { Exit, Help, Clear, History }
  ```
  Strict prefix match on `/`; anything not matching one of the 4 commands
  → `Prompt(text)` (no error for unknown slash forms — pass through).
- **Session state**:
  ```rust
  pub struct Session {
      runtime: Runtime,
      agent_def: AgentDefinition,
      manifest: PackageManifest,
      options: RunOptions,
      history: Vec<Message>,
      total_tokens: TokenUsage,
  }

  impl Session {
      pub async fn handle(&mut self, input: ReplInput) -> SessionAction;
  }

  pub enum SessionAction {
      Continue,
      Exit,
      Error(RuntimeError),    // aborts session
  }
  ```
- **Rendering**: `termimad::MadSkin::default_dark()` (or `default_light()`
  selected by `Output::color` mode) renders the agent's final assistant
  message Markdown. User input echoed plain-text by rustyline.
- **Welcome banner**: prints agent name, package, `/help` hint.
- **Lifecycle**: validate setup (= `tau run --dry-run` validation) →
  print banner → loop. On `Err(RuntimeError)` mid-session: print error,
  exit 2. On `Ok(RunOutcome::Failed)`: render failure detail (red),
  append to history, continue. On `/exit` or Ctrl-D: print
  `session ended; total tokens: ...`, exit 0.

### 3.14 Subcommand-by-subcommand operational summary

| Subcommand | Reads | Writes | Calls | Exit codes |
|---|---|---|---|---|
| `tau init` | cwd | `tau.toml` (unless `--dry-run`) | — | 0 / 2 |
| `tau install` | git remote | `<scope>/.tau/packages/...`, lockfile | `tau_pkg::install` | 0 / 2 |
| `tau list` | scope, project tau.toml (for `agents`) | — | `tau_pkg::list` | 0 / 2 |
| `tau run` | scope, project tau.toml | — | `Runtime::run`, all dependencies | 0 / 1 / 2 |
| `tau chat` | scope, project tau.toml, stdin | — | `Runtime::run_with_history`, all dependencies | 0 / 2 |

---

## 4. Parsers

### 4.1 Project `tau.toml`

`UncheckedProjectConfig` → `validate()` → `ProjectConfig`. Mirrors
tau-domain's manifest pattern.

Validations:
- `[project].name` non-empty.
- Each `[agents.<id>]` has all required fields non-empty.
- `package` parses as `name@<semver-req>` or `name` (latter = `*`).
- `prompt.system` XOR `prompt.system_file` if `[prompt]` table present.
- `[capabilities]` sub-table presence → `CapabilityOverrideUnsupported`.

Proptest: round-trip — generate a valid `ProjectConfig`, serialize to
TOML, parse-and-validate, compare equal.

### 4.2 Slash-command parser

Pure: `parse_input(&str) -> ReplInput`. No error path (unknown slash
commands fall through to `Prompt`). Proptest: random input never panics,
always returns a constructed `ReplInput`.

### 4.3 CLI args

clap derive handles parsing; tau-cli adds runtime validations:
- `--json` rejected on `tau chat`.
- `--global` and `--all` mutually exclusive on `tau list` (clap's
  `conflicts_with`).
- `--max-turns 0` rejected (clap's value range).

---

## 5. Testing strategy

### Layers

| Layer | Tool | Scope |
|---|---|---|
| Unit | `#[test]` | Internal helpers per module |
| Snapshot | `insta` | `--help` text + `--json` schema fixtures |
| Proptest | `proptest` | `ProjectConfig` round-trip + slash-command parser |
| Integration | `assert_cmd` + `tempfile` + `file://` git fixtures | Per-subcommand end-to-end |

### Per-subcommand integration matrix

`tau install`: local `file://` install, `--global`, bad URL, manifest
validation failure, `--dry-run` no-mutation, `--json`.

`tau list`: empty scope, populated scope, `--global`/`--all`,
`agents` resource, missing project tau.toml + `agents`,
`--json`, `--dry-run` rejected.

`tau run`: scripted-LLM happy path, capability-denied, max_turns,
LLM-not-registered, tool-not-registered, missing agent id, missing
project tau.toml, `requires.tools` missing, `--dry-run` no LLM call,
`--json` for completed AND failed, `--max-turns N` override.

`tau init`: happy path, second-invocation conflict, `--force`,
`--dry-run`, `--json`, project name from cwd basename.

`tau chat`: stdin-piped happy path, slash-command unit tests, session
unit tests, error abort behavior, `--dry-run`.

### Cross-cutting

- Exit code matrix
- Error display modes (default vs `--debug`)
- Color (`--color` + `NO_COLOR`)
- Dry-run no-state-change verification
- JSON schema stability via insta JSON snapshots
- Tracing emission via custom Layer (sub-project 4 pattern)
- Cross-platform (Windows path normalization, line endings)

### REPL test strategy

Three layers (parser unit + session unit with mocked Runtime + stdin-pipe
end-to-end). Terminal-mode features (arrow keys, color rendering)
explicitly NOT covered at v0.1.

### Mock LLM backend strategy

Two mechanisms:

1. **Compiled-in mock backend** gated by `cfg(test)` (or a `test-mock`
   feature) lives in `tau_cli::cmd::run_internal::mock_backend`. Allows
   in-process integration tests without subprocess overhead.
2. **Fixture LLM-backend package** under `tests/fixtures/echo-llm/`:
   a real installable agent-kind package whose plugin returns canned
   responses. Tests `tau install <fixture>` + `tau run` end-to-end.

Per the §3.11 plugin-loading caveat at v0.1: option 2 currently doesn't
work end-to-end because plugin loading lands in Phase 1+. Tests at v0.1
use option 1; option 2 fixtures are kept ready for Phase 1+.

### Estimated test count

~50-60 new tests in tau-cli; ~5 new tests in tau-runtime (covering both
amendments). Comparable to sub-project 4's 62+7+3.

---

## 6. ADR-0007 plan

Filed at `docs/decisions/0007-tau-cli.md`. Status flow:
**Proposed** → **Accepted** at Task 22's sign-off.

### Decisions documented

1. **5-subcommand surface** at v0.1: `install`, `list`, `run`, `init`, `chat`.
2. **`#[tokio::main]`** at the entry; sync tau-pkg calls inline.
3. **Project `tau.toml`** with `[project]` and named-table `[agents.<id>]`
   entries + optional sub-tables.
4. **Capability override is a Phase 1+ requirement.** Schema slot reserved;
   v0.1 hard-errors. Intersect-only semantics committed.
5. **Per-agent `requires.tools`** advisory check at v0.1.
6. **One-shot `tau run` + separate `tau chat` REPL.**
7. **Three-bucket exit codes** (0 / 1 / 2).
8. **Strict stdout/stderr split** + `--json` mode.
9. **Verbosity model**: default INFO, `-v` DEBUG, `-vv` TRACE, `-q` WARN,
   `--debug` (= DEBUG + full error chain), `RUST_LOG` overrides.
10. **Color via `is-terminal`** + `--color always|auto|never` + `NO_COLOR`.
11. **REPL**: rustyline + termimad, in-memory-only history, four slash
    commands.
12. **Top-level error message by default**; `--debug` expands.
13. **`--dry-run`** on `install`, `run`, `init`, `chat` (not `list`).
14. **tau-runtime amendment 1**: capability-filtered tools.
15. **tau-runtime amendment 2**: `Runtime::run_with_history`.
16. **`AgentDefinition` construction lives in tau-cli.**
17. **No new error variants without triggering codepaths.**
18. **Plugin loading deferred to Phase 1+**. v0.1 ships compiled-in mock
    plugins for testing; `tau install` registers source trees but they
    don't execute until Phase 1's loading mechanism lands.

### Cross-references

ADR-0002 (manifest format) — `tau.toml` filename clash discussion.
ADR-0003 (tau-ports) — `Runtime::run_with_history` doesn't change
`LlmBackend::complete`'s contract.
ADR-0004 (tau-pkg) — `tau init` gitignore-hint behavior.
ADR-0006 (tau-runtime) — bundling-pattern precedent, typed-capability
story refined by decision 14.

Each decision: rationale + alternatives considered + trigger to revisit.

---

## 7. Commit / sub-task strategy

Refined into per-task detail in writing-plans. Outline (~22 tasks
mirroring sub-project 4's count):

**Setup (Tasks 1-3):**
1. Workspace + crate Cargo.toml with new deps.
2. tau-runtime amendment: capability-filtered tools.
3. tau-runtime amendment: `Runtime::run_with_history`.

**CLI scaffolding (Tasks 4-7):**
4. clap top-level + 5 subcommand argument structures.
5. `exit::ExitCode` + `From` impls.
6. `output` module.
7. `tracing` module.

**Project config (Tasks 8-9):**
8. `config::ProjectConfig` parser + validator.
9. `AgentEntry → tau_domain::AgentDefinition` conversion.

**Subcommand bodies (Tasks 10-15):**
10. `cmd::init`.
11. `cmd::install`.
12. `cmd::list`.
13. `cmd::run`.
14. `cmd::chat`.
15. Cross-cutting: error display, color, dry-run conventions.

**Polish + CI + docs (Tasks 16-19):**
16. `tests/help_snapshots.rs`.
17. `tests/json_schemas.rs`.
18. CI: `build (tau-cli no-default-features)` job.
19. ADR-0007 file + index update.

**User-driven gates (Tasks 20-22):**
20. Final local verification.
21. ADR-0007 fresh-eyes review (24h or self-review per QG22).
22. Plan 6 sign-off + ROADMAP row 5 → complete + plan checkboxes +
    branch protection update + squash-merge.

Each task = one Conventional Commits commit; verification commands per
task identical to Phase 0 pattern (cargo build/clippy/test/fmt --check +
per-subcommand test).

---

## 8. Risks & rollbacks

| Risk | Mitigation | Rollback path |
|---|---|---|
| **Plugin loading deferred** means v0.1 ships a CLI that can install but not actually run user-supplied plugins | Documented prominently in ADR-0007 §18 + `tau install`'s success message hints "runnable in Phase 1+". v0.1 ships a compiled-in mock backend for testing | Phase 1+ ADR adds a real loading mechanism (dlopen / abi_stable / out-of-process IPC). v0.1 install paths don't change |
| **REPL UX subtleties** (terminal-mode quirks, signal handling) bite users | Three-layer test strategy (parser unit + session unit + stdin-pipe e2e); document v0.1 limitations explicitly in `tau chat --help` | Phase 1+ revisits with TTY emulator tests |
| **`#[tokio::main]` couples CLI to tokio forever** | Documented in ADR-0007 §2; `Runtime::run` stays async-runtime-agnostic | Phase 1+ could swap; tau-cli is the only consumer affected |
| **Capability-filter amendment changes existing tau-runtime behavior** | No external consumers exist (sub-project 4 just shipped); change observable only through `CompletionRequest.tools` and new tracing events | One-commit revert of the filter; behavior reverts to expose-all |
| **`Runtime::run_with_history` adds public API**; future REPL features may want extensibility | At v0.1 minimal: `Vec<Message>` + new initial message; future features additive on `RunOptions` | Same — no public API consumers yet |
| **Project `tau.toml` filename clash with package `tau.toml`** confuses users | Documented in ADR-0007 §3 with the Cargo `[package]` vs `[workspace]` precedent; `tau init` only creates project tau.toml; `tau install` only consumes package tau.toml from cloned repos | Rename project file to `tau-project.toml` if confusion proves real (additive: try new name first, fall back to old with deprecation warning) |
| **`requires.tools` advisory + capability-filter overlap** | Distinct error messages; documented behavior | None needed — surfaces user misconfiguration |
| **Phase 1+ capability override** must intersect-not-expand | Spec + ADR-0007 §4 explicitly state intersect-only; rejection error message points at the design constraint | Schema reserved; if Phase 1 picks different semantics, rejection becomes deprecation + new field |
| **Cross-platform**: Windows path normalization and TTY detection | Pre-flight: `tests/cross_platform.rs`; `is-terminal` handles TTY portably | Per Constitution G15, Windows non-blocking |
| **Plugin discovery** (auto-load all installed plugins of certain kinds) collides with the v0.1 plugin-loading limitation | At v0.1, discovery returns empty for non-mock kinds; documented as deferred | Phase 1+ ADR replaces stub with real loader |

---

## 9. Handoff to writing-plans

After spec write + self-review + user approval:
1. Invoke `superpowers:writing-plans` skill with the spec at
   `docs/superpowers/specs/2026-04-28-tau-cli-design.md` as input.
2. Plan written to `docs/superpowers/plans/2026-04-28-tau-cli.md` with
   the ~22-task decomposition expanded into per-task commit-level detail.
3. Plan-erratum carry-overs from sub-projects 1-4 (per phase-0-mid memo)
   applied preemptively:
   - Doctests on `#[non_exhaustive]` types marked `ignore`.
   - `cargo test --doc` runs separately.
   - Let-else for destructuring `#[non_exhaustive]` enums in tests.
   - **Spec-phase trait pre-flight done**: tau-cli owns its types
     (no cross-crate `#[non_exhaustive]` issues); async traits not
     involved at the CLI layer; tests have full access to internal
     modules via `lib.rs`.
4. Implementation via `superpowers:subagent-driven-development`.
