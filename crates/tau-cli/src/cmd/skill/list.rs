//! `tau skill list` — enumerate installed skill packages.

use crate::cli::SkillListArgs;
use crate::output::Output;

/// Implemented in Task 4.
pub async fn run(_args: &SkillListArgs, _output: &mut Output) -> anyhow::Result<()> {
    anyhow::bail!("tau skill list: not yet implemented (Task 4)")
}
