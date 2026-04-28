//! `tau list` — list installed packages or available agents.

use crate::cli::ListArgs;
use crate::output::Output;

/// Stub: `tau list` is implemented in Task 12.
pub async fn run(_args: &ListArgs, _output: &mut Output) -> anyhow::Result<()> {
    anyhow::bail!("tau list: not implemented (Task 12)")
}
