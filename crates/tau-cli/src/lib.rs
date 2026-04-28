#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! tau-cli internals. The `tau` binary is a thin wrapper around
//! [`run_main`]; this lib exists so integration tests can drive
//! command logic without subprocess overhead.

pub mod cli;
pub mod cmd;
pub mod config;
pub mod exit;
pub mod output;
pub mod tracing;

pub use config::{
    build_agent_definition, AgentEntry, AgentResolutionError, ProjectConfig, ProjectConfigError,
    PromptEntry, RequiresEntry,
};
pub use exit::ExitCode;
pub use output::{ColorChoice, Output};

use clap::Parser;

/// Top-level entry point used by `main` and integration tests.
///
/// Parses CLI arguments, dispatches to the appropriate `cmd::*::run`
/// handler, and maps the result to a process exit code.
///
/// At v0.1 of Task 4, all subcommand handlers are stubs that return
/// an "unimplemented" error. Tasks 10-14 land the real handlers.
pub async fn run_main() -> std::process::ExitCode {
    let cli = cli::Cli::parse();
    tracing::install(&cli);
    match dispatch(cli).await {
        Ok(()) => ExitCode::Success.into(),
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(&err).into()
        }
    }
}

async fn dispatch(cli: cli::Cli) -> anyhow::Result<()> {
    let mut output = Output::from_cli(&cli);
    match cli.command {
        cli::Command::Init(args) => cmd::init::run(&args, &mut output).await,
        cli::Command::Install(args) => cmd::install::run(&args, &mut output).await,
        cli::Command::List(args) => cmd::list::run(&args, &mut output).await,
        cli::Command::Run(args) => cmd::run::run(&args, &mut output).await,
        cli::Command::Chat(args) => cmd::chat::run(&args, &mut output).await,
    }
}
