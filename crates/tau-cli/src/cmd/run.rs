//! `tau run` — invoke an agent one-shot.

use crate::cli::RunArgs;
use crate::output::Output;

/// Stub: `tau run` is implemented in Task 13.
pub async fn run(_args: &RunArgs, _output: &mut Output) -> anyhow::Result<()> {
    anyhow::bail!("tau run: not implemented (Task 13)")
}
