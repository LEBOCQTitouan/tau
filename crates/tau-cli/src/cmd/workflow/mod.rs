//! `tau workflow {list, run, log, resume}` — workflow lifecycle commands.

use crate::cli::WorkflowSubcommand;
use crate::output::Output;

pub mod list;

// log, resume, run come in later tasks. Stub them for now so the
// dispatch compiles; replace in T11-T13.

/// Dispatch a workflow subcommand.
pub async fn dispatch(sub: WorkflowSubcommand, output: &mut Output) -> anyhow::Result<()> {
    match sub {
        WorkflowSubcommand::List => list::run(output),
        WorkflowSubcommand::Run(_args) => {
            anyhow::bail!("tau workflow run: not yet implemented (T11)")
        }
        WorkflowSubcommand::Log(_args) => {
            anyhow::bail!("tau workflow log: not yet implemented (T12)")
        }
        WorkflowSubcommand::Resume(_args) => {
            anyhow::bail!("tau workflow resume: not yet implemented (T13)")
        }
    }
}
