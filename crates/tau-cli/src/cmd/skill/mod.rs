//! `tau skill <subcommand>` — inspect installed skill packages.
//!
//! Skills-3 sub-project from ROADMAP §16. Two subcommands:
//! - [`list`] — enumerate installed skills (lockfile-only).
//! - [`show`] — detailed view of one skill, optionally including
//!   the SKILL.md body (rendered or raw).
//!
//! See `docs/superpowers/specs/2026-05-13-skills-3-discovery-design.md`
//! for the design and ADR-0027 for the decision rationale.

pub mod levenshtein;
pub mod list;
pub mod render;
pub mod show;

use crate::cli::SkillSubcommand;
use crate::output::Output;

/// Route `tau skill <subcommand>` to its impl module.
pub async fn dispatch(sub: SkillSubcommand, output: &mut Output) -> anyhow::Result<()> {
    match sub {
        SkillSubcommand::List(args) => list::run(&args, output).await,
        SkillSubcommand::Show(args) => show::run(&args, output).await,
    }
}
