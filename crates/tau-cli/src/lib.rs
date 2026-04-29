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
    // Capture `cli.debug` before `dispatch` consumes the parsed `Cli`.
    // When set, the error path renders the full `anyhow` chain via
    // `{err:?}` instead of the single-line top-level message. This is
    // the integration-level surface for `--debug` (the tracing module
    // already promotes the filter to DEBUG independently).
    let debug = cli.debug;
    match dispatch(cli).await {
        Ok(()) => ExitCode::Success.into(),
        Err(err) => {
            // The AgentFailed marker is emitted by `cmd::run` when the
            // agent reaches `RunOutcome::Failed`. It must NOT print the
            // generic "error:" prefix — the run handler has already
            // emitted a structured failure to the user. All other
            // errors are kernel/CLI failures; they get the prefix and
            // map to `ExitCode::Error`.
            if err.downcast_ref::<crate::cmd::run::AgentFailed>().is_some() {
                ExitCode::AgentFailed.into()
            } else {
                if debug {
                    eprintln!("error: {err:?}");
                } else {
                    eprintln!("error: {err}");
                }
                ExitCode::from(&err).into()
            }
        }
    }
}

async fn dispatch(cli: cli::Cli) -> anyhow::Result<()> {
    let mut output = Output::from_cli(&cli);
    let record_protocol = cli.record_protocol.clone();
    match cli.command {
        cli::Command::Init(args) => cmd::init::run(&args, &mut output).await,
        cli::Command::Install(args) => cmd::install::run(&args, &mut output).await,
        cli::Command::List(args) => cmd::list::run(&args, &mut output).await,
        cli::Command::Run(args) => cmd::run::run(&args, record_protocol, &mut output).await,
        cli::Command::Chat(args) => cmd::chat::run(&args, record_protocol, &mut output).await,
        cli::Command::Plugin { action } => {
            cmd::plugin::dispatch(action, record_protocol, &mut output).await
        }
    }
}
