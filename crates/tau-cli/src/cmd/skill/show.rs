//! `tau skill show <name>` — inspect one installed skill.

use crate::cli::SkillShowArgs;
use crate::output::Output;

/// Implemented in Task 5.
pub async fn run(_args: &SkillShowArgs, _output: &mut Output) -> anyhow::Result<()> {
    anyhow::bail!("tau skill show: not yet implemented (Task 5)")
}
