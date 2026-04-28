//! Command-line argument parser. clap v4 derive.
//!
//! [`Cli`] is the top-level parser; [`Command`] enumerates the five
//! v0.1 subcommands. Each subcommand has its own `*Args` struct.
//!
//! Global flags (verbose, quiet, debug, color, json) are accessible
//! from any subcommand via the parsed [`Cli`].

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Top-level parser for the `tau` binary.
#[derive(Parser, Debug)]
#[command(name = "tau", version, about = "tau runtime CLI")]
pub struct Cli {
    /// The subcommand to dispatch.
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

/// All v0.1 subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Scaffold a project `tau.toml`.
    Init(InitArgs),
    /// Install a package from a Git URL.
    Install(InstallArgs),
    /// List installed packages or available agents.
    List(ListArgs),
    /// Invoke an agent one-shot.
    Run(RunArgs),
    /// Open a REPL chat session with an agent.
    Chat(ChatArgs),
}

/// Arguments for `tau init`.
#[derive(Args, Debug)]
pub struct InitArgs {
    /// Overwrite an existing tau.toml.
    #[arg(long)]
    pub force: bool,
    /// Print what would be created without writing files.
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for `tau install`.
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

/// Arguments for `tau list`.
#[derive(Args, Debug)]
pub struct ListArgs {
    /// What to list. Defaults to `packages`.
    #[arg(value_enum, default_value = "packages")]
    pub resource: ListResource,
    /// Restrict the listing to globally-installed entries.
    /// (Ignored when `resource` is `agents`; agents are always project-scoped.)
    #[arg(long, conflicts_with = "all")]
    pub global: bool,
    /// List both project and global entries side-by-side.
    /// (Ignored when `resource` is `agents`; agents are always project-scoped.)
    #[arg(long)]
    pub all: bool,
    /// Rejected explicitly: `tau list` is read-only.
    #[arg(long, hide = true)]
    pub dry_run: bool,
}

/// Arguments for `tau run`.
#[derive(Args, Debug)]
pub struct RunArgs {
    /// Identifier of the agent to invoke.
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

/// Arguments for `tau chat`.
#[derive(Args, Debug)]
pub struct ChatArgs {
    /// Identifier of the agent to chat with.
    pub agent_id: String,
    /// Override RunOptions::max_turns.
    #[arg(long)]
    pub max_turns: Option<u32>,
    /// Validate setup without entering the REPL.
    #[arg(long)]
    pub dry_run: bool,
}

/// Resource kinds accepted by `tau list`.
#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum ListResource {
    /// List installed packages.
    Packages,
    /// List available agents.
    Agents,
}

/// Color output mode for terminal rendering.
#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum ColorMode {
    /// Always emit ANSI color codes.
    Always,
    /// Auto-detect based on terminal capabilities.
    Auto,
    /// Never emit color codes.
    Never,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_install_url() {
        let cli = Cli::parse_from(["tau", "install", "https://github.com/foo/bar.git"]);
        let Command::Install(args) = cli.command else {
            panic!("expected Install: {:?}", cli.command)
        };
        assert_eq!(args.url, "https://github.com/foo/bar.git");
        assert!(!args.global);
        assert!(!args.dry_run);
    }

    #[test]
    fn parses_install_with_global_and_dry_run() {
        let cli = Cli::parse_from([
            "tau",
            "install",
            "https://github.com/foo/bar.git",
            "--global",
            "--dry-run",
        ]);
        let Command::Install(args) = cli.command else {
            panic!()
        };
        assert!(args.global);
        assert!(args.dry_run);
    }

    #[test]
    fn parses_run_with_prompt() {
        let cli = Cli::parse_from(["tau", "run", "agent-x", "hello world"]);
        let Command::Run(args) = cli.command else {
            panic!()
        };
        assert_eq!(args.agent_id, "agent-x");
        assert_eq!(args.prompt.as_deref(), Some("hello world"));
    }

    #[test]
    fn parses_run_without_prompt() {
        let cli = Cli::parse_from(["tau", "run", "agent-x"]);
        let Command::Run(args) = cli.command else {
            panic!()
        };
        assert_eq!(args.agent_id, "agent-x");
        assert_eq!(args.prompt, None);
    }

    #[test]
    fn parses_run_max_turns_override() {
        let cli = Cli::parse_from(["tau", "run", "agent-x", "hi", "--max-turns", "5"]);
        let Command::Run(args) = cli.command else {
            panic!()
        };
        assert_eq!(args.max_turns, Some(5));
    }

    #[test]
    fn parses_list_default_resource_is_packages() {
        let cli = Cli::parse_from(["tau", "list"]);
        let Command::List(args) = cli.command else {
            panic!()
        };
        assert!(matches!(args.resource, ListResource::Packages));
    }

    #[test]
    fn parses_list_agents_resource() {
        let cli = Cli::parse_from(["tau", "list", "agents"]);
        let Command::List(args) = cli.command else {
            panic!()
        };
        assert!(matches!(args.resource, ListResource::Agents));
    }

    #[test]
    fn parses_chat() {
        let cli = Cli::parse_from(["tau", "chat", "agent-x"]);
        let Command::Chat(args) = cli.command else {
            panic!()
        };
        assert_eq!(args.agent_id, "agent-x");
    }

    #[test]
    fn parses_init_with_force() {
        let cli = Cli::parse_from(["tau", "init", "--force"]);
        let Command::Init(args) = cli.command else {
            panic!()
        };
        assert!(args.force);
    }

    #[test]
    fn parses_global_verbose_flag() {
        let cli = Cli::parse_from(["tau", "-v", "list"]);
        assert_eq!(cli.verbose, 1);
    }

    #[test]
    fn parses_global_double_verbose_flag() {
        let cli = Cli::parse_from(["tau", "-vv", "list"]);
        assert_eq!(cli.verbose, 2);
    }

    #[test]
    fn parses_global_color_flag() {
        let cli = Cli::parse_from(["tau", "--color", "never", "list"]);
        assert!(matches!(cli.color, ColorMode::Never));
    }

    #[test]
    fn list_global_and_all_are_mutually_exclusive() {
        let result = Cli::try_parse_from(["tau", "list", "--global", "--all"]);
        assert!(result.is_err(), "expected mutual exclusion error");
    }
}
