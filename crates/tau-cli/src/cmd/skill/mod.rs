//! `tau skill <subcommand>` — inspect installed skill packages.
//!
//! Skills-3 sub-project from ROADMAP §16. Two subcommands:
//! - [`list`] — enumerate installed skills (lockfile-only).
//! - [`show`] — detailed view of one skill, optionally including
//!   the SKILL.md body (rendered or raw).
//!
//! Skills-5 adds two more:
//! - [`import`] — convert an Anthropic-format source into a tau-skill dir.
//! - [`export`] — strip tau.toml from an installed skill (Task 6 stub).
//!
//! See `docs/superpowers/specs/2026-05-13-skills-3-discovery-design.md`
//! for the design and ADR-0027 for the decision rationale.

pub mod export;
pub mod import;
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
        SkillSubcommand::Import(args) => {
            import::run(args).map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(())
        }
        SkillSubcommand::Export(args) => {
            export::run(args).map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(())
        }
    }
}
