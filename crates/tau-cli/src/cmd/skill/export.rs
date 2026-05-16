//! `tau skill export` — strip tau.toml from an installed skill,
//! produce a vanilla Anthropic-format directory.
//!
//! Skills-5 D3. Walks the installed package directory at
//! `<scope>/.tau/packages/<name>/<version>/`; copies every file
//! except `tau.toml` to the output dir. Emits an stderr warning
//! if dropping capabilities; `--strict` makes that warning a
//! hard error.

use std::path::{Path, PathBuf};

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

    /// `--strict` was set and the export would drop tau-specific
    /// metadata.
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

    /// Failed to look up the installed skill.
    #[error("locating skill: {0}")]
    FindSkill(#[from] tau_pkg::FindSkillError),

    /// Scope resolution failed (no `.tau/` in cwd or cwd unreadable).
    #[error("no scope: {detail}")]
    NoScope {
        /// Human-readable detail of why scope resolution failed.
        detail: String,
    },
}

/// Run `tau skill export`.
pub fn run(args: SkillExportArgs) -> Result<(), ExportError> {
    // 1. Resolve scope from cwd.
    let cwd = std::env::current_dir().map_err(|e| ExportError::NoScope {
        detail: e.to_string(),
    })?;
    let scope = tau_pkg::Scope::resolve(&cwd).map_err(|e| ExportError::NoScope {
        detail: e.to_string(),
    })?;

    // 2. Look up the installed skill.
    let installed = tau_pkg::find_installed_skill(&scope, &args.name)?.ok_or_else(|| {
        ExportError::SkillNotInstalled {
            name: args.name.clone(),
            suggestion: suggest_skill_name(&scope, &args.name),
        }
    })?;

    // 3. Collect any tau-specific metadata that would be dropped.
    let mut dropped: Vec<String> = Vec::new();
    for cap in &installed.capabilities {
        dropped.push(capability_kind_str(cap));
    }
    if !installed.skill.requires_skills.is_empty() {
        dropped.push(format!(
            "requires_skills ({})",
            installed.skill.requires_skills.len()
        ));
    }

    // 4. --strict check: hard-error if anything would be dropped.
    if args.strict && !dropped.is_empty() {
        return Err(ExportError::WouldDropMetadata {
            name: args.name.clone(),
            dropped,
        });
    }

    // 5. Handle --force / existing output directory.
    if args.output.exists() {
        if !args.force {
            return Err(ExportError::OutputDirectoryExists {
                path: args.output.clone(),
            });
        }
        std::fs::remove_dir_all(&args.output)?;
    }
    std::fs::create_dir_all(&args.output)?;

    // 6. Walk install_path recursively, copy every file except tau.toml
    //    (at any depth).
    copy_dir_except_tau_toml(&installed.install_path, &args.output)?;

    // 7. Warn on stderr if metadata was dropped (non-strict path).
    if !dropped.is_empty() {
        eprintln!(
            "note: {} item(s) dropped on Anthropic export ({}); \
             Anthropic format does not preserve them",
            dropped.len(),
            dropped.join(", "),
        );
    }

    println!("Exported {} to {}", args.name, args.output.display());
    Ok(())
}

/// Recursive copy of `src` into `dst`, omitting any file named exactly
/// `tau.toml` at any depth.
fn copy_dir_except_tau_toml(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let name = entry.file_name();
        if ft.is_dir() {
            let new_dst = dst.join(&name);
            std::fs::create_dir_all(&new_dst)?;
            copy_dir_except_tau_toml(&entry.path(), &new_dst)?;
        } else if ft.is_file() {
            if name == "tau.toml" {
                continue;
            }
            std::fs::copy(entry.path(), dst.join(&name))?;
        }
    }
    Ok(())
}

/// Map a `Capability` to a short string for the drop-warning message.
///
/// Mirrors `capability_kind_str` in tau-runtime (Option A: private
/// reimplementation avoids a cross-crate refactor). Includes a wildcard
/// arm for `#[non_exhaustive]` forward-compatibility.
fn capability_kind_str(cap: &tau_domain::Capability) -> String {
    use tau_domain::{
        AgentCapability, FsCapability, NetCapability, ProcessCapability, SkillCapability,
    };
    match cap {
        tau_domain::Capability::Filesystem(FsCapability::Read { .. }) => "fs.read".into(),
        tau_domain::Capability::Filesystem(FsCapability::Write { .. }) => "fs.write".into(),
        tau_domain::Capability::Filesystem(FsCapability::Exec { .. }) => "fs.exec".into(),
        tau_domain::Capability::Network(NetCapability::Http { .. }) => "net.http".into(),
        tau_domain::Capability::Process(ProcessCapability::Spawn { .. }) => "process.spawn".into(),
        tau_domain::Capability::Agent(AgentCapability::Spawn { .. }) => "agent.spawn".into(),
        tau_domain::Capability::TaskList { .. } => "task_list".into(),
        tau_domain::Capability::Plan { .. } => "plan".into(),
        tau_domain::Capability::Skill(SkillCapability::Spawn { .. }) => "skill.spawn".into(),
        tau_domain::Capability::Custom { name, .. } => name.clone(),
        _ => "unknown".into(),
    }
}

/// Build a levenshtein "did you mean?" suggestion for an unknown skill name.
///
/// Loads the lockfile (if present), extracts installed skill names, and
/// delegates to Skills-3's `closest_match` helper with a threshold of 2.
fn suggest_skill_name(scope: &tau_pkg::Scope, query: &str) -> Option<String> {
    let lockfile_path = scope.lockfile_path();
    if !lockfile_path.exists() {
        return None;
    }
    let lockfile = tau_pkg::lockfile::LockFile::load(&lockfile_path).ok()?;
    let candidates: Vec<String> = lockfile
        .packages
        .iter()
        .filter(|p| p.skill.is_some())
        .map(|p| p.name.as_str().to_string())
        .collect();
    super::levenshtein::closest_match(query, &candidates, 2).map(str::to_owned)
}
