//! `tau skill export` — strip tau.toml from an installed skill,
//! produce a vanilla Anthropic-format directory.
//!
//! Skills-5 D3. Implementation lands in Task 6 — this module provides
//! the type definitions so `cli.rs` can reference `SkillExportArgs`
//! and `mod.rs` can wire the dispatch arm without a compile error.

use std::path::PathBuf;

use crate::cli::SkillExportArgs;

/// Errors raised by `tau skill export`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExportError {
    /// No skill matches `name`. `suggestion` may be `Some(closest)`
    /// from levenshtein (Skills-3) if a near-match exists.
    #[error("skill not found: {name:?}")]
    SkillNotInstalled {
        /// The name that was requested.
        name: String,
        /// A near-match suggestion, if any.
        suggestion: Option<String>,
    },

    /// `--strict` was set and the export would drop tau-specific metadata.
    #[error(
        "would drop metadata: {dropped:?} (skill {name:?}); \
         remove --strict to proceed with a warning"
    )]
    WouldDropMetadata {
        /// The skill name.
        name: String,
        /// Description of what would be dropped.
        dropped: Vec<String>,
    },

    /// Output directory already exists and `--force` was not set.
    #[error("output directory {path:?} already exists; pass --force to overwrite")]
    OutputDirectoryExists {
        /// The conflicting output path.
        path: PathBuf,
    },

    /// I/O error during copy.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Run `tau skill export` — not yet implemented (Skills-5 Task 6).
pub fn run(_args: SkillExportArgs) -> Result<(), ExportError> {
    todo!("Skills-5 T6: tau skill export implementation")
}
