//! `tau skill import` — convert an Anthropic-format source into a
//! tau-skill directory.
//!
//! Skills-5 D2 (explicit import flow). Clones or copies the source,
//! detects format, synthesizes a `tau.toml` alongside SKILL.md, leaves
//! the result for the user to inspect or edit before `tau install`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use crate::cli::SkillImportArgs;

/// Errors raised by `tau skill import`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ImportError {
    /// Source directory has a `tau.toml` already — not an Anthropic
    /// skill; user should `tau install` directly.
    #[error("source already has tau.toml at {path:?}; use `tau install` on that path instead")]
    SourceAlreadyTauSkill {
        /// The path where tau.toml was found.
        path: PathBuf,
    },

    /// Source has neither tau.toml nor SKILL.md.
    #[error("not a skill package: {path:?} has neither tau.toml nor SKILL.md")]
    NotASkillPackage {
        /// The path that was checked.
        path: PathBuf,
    },

    /// Output directory exists and `--force` was not set.
    #[error("output directory {path:?} already exists; pass --force to overwrite")]
    OutputDirectoryExists {
        /// The existing output directory path.
        path: PathBuf,
    },

    /// I/O error during clone, read, or write.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// SKILL.md parse or synthesis failed.
    #[error("synthesizing manifest: {0}")]
    Synthesize(#[from] tau_pkg::SynthesizeError),

    /// TOML serialization of the synthesized manifest failed.
    #[error("serializing tau.toml: {detail}")]
    SerializeManifest {
        /// The underlying TOML serialization error message.
        detail: String,
    },

    /// Source clone failed (git operation, network, etc.).
    #[error("cloning source: {detail}")]
    CloneFailed {
        /// The underlying error detail (git stderr or spawn error).
        detail: String,
    },

    /// Source string could not be parsed as a valid package source.
    #[error("invalid source {raw_source:?}: {detail}")]
    InvalidSource {
        /// The raw source string that could not be parsed.
        raw_source: String,
        /// The underlying parse error message.
        detail: String,
    },
}

/// Run `tau skill import`.
pub fn run(args: SkillImportArgs) -> Result<(), ImportError> {
    // 1. Handle --force / existing dir.
    if args.output.exists() {
        if !args.force {
            return Err(ImportError::OutputDirectoryExists {
                path: args.output.clone(),
            });
        }
        std::fs::remove_dir_all(&args.output)?;
    }

    // 2. Clone or copy source into output.
    clone_source_to(&args.source, &args.output)?;

    // 3. Detect format.
    use tau_domain::SkillFormat;
    match tau_domain::detect_format(&args.output) {
        SkillFormat::Tau => {
            return Err(ImportError::SourceAlreadyTauSkill {
                path: args.output.clone(),
            });
        }
        SkillFormat::Invalid => {
            return Err(ImportError::NotASkillPackage {
                path: args.output.clone(),
            });
        }
        SkillFormat::Anthropic => {} // proceed
        // SkillFormat is #[non_exhaustive]; handle future variants by treating them
        // as invalid (safest default — we don't know how to synthesize them).
        _ => {
            return Err(ImportError::NotASkillPackage {
                path: args.output.clone(),
            });
        }
    }

    // 4. Build a PackageSource for the synthesized manifest.
    //    For local paths we convert to a file:// URL (the only form
    //    PackageSource::from_str accepts for local sources). For git
    //    URLs the source string is used verbatim.
    let source_for_manifest = build_package_source(&args.source)?;

    // 5. Synthesize manifest.
    let manifest = tau_pkg::synthesize_anthropic_skill(&args.output, source_for_manifest)?;

    // 6. Serialize to tau.toml and write alongside SKILL.md.
    let toml_text =
        toml::to_string_pretty(&manifest).map_err(|e| ImportError::SerializeManifest {
            detail: e.to_string(),
        })?;
    std::fs::write(args.output.join("tau.toml"), toml_text)?;

    // 7. Print hint.
    println!(
        "Wrote {}/tau.toml.\nRun `tau install {}` to install.",
        args.output.display(),
        args.output.display()
    );

    Ok(())
}

/// Determine whether `s` looks like a local filesystem path.
///
/// Heuristic: absolute path, relative `.`/`..` path, or a string that
/// is an existing directory (even if it has unusual characters). Git
/// URLs (including `file://`) always contain `://` or `@` with `:` (scp form).
///
/// `file://` URLs are intentionally treated as git URLs (not local paths)
/// because a bare git repo at a `file://` path is git metadata — it has
/// no SKILL.md at the top level. Routing `file://` through `git clone`
/// produces a proper working tree with SKILL.md present.
fn is_local_path(s: &str) -> bool {
    // Any URL scheme (https://, file://, git://, etc.) → git clone path.
    if s.contains("://") {
        return false;
    }
    // scp-style git@ or host:path → remote.
    if s.contains('@') || (s.contains(':') && !s.contains('/') && s.find(':') < s.find('/')) {
        // heuristic: if ':' appears before first '/', likely scp.
        // But we need to handle Windows paths like C:\foo — however
        // tau is POSIX-first (darwin + linux CI), so single-char
        // drive letters are unlikely in practice.
        return false;
    }
    true
}

/// Clone a remote git URL or copy a local directory into `output`.
fn clone_source_to(source: &str, output: &Path) -> Result<(), ImportError> {
    if is_local_path(source) {
        // Strip file:// prefix for fs copy.
        let src_path: PathBuf = if let Some(stripped) = source.strip_prefix("file://") {
            // On POSIX file:///abs/path → strip "file://" → /abs/path.
            // On POSIX file:////... is unusual; just use stripped.
            PathBuf::from(stripped)
        } else {
            PathBuf::from(source)
        };
        copy_dir_recursive(&src_path, output)?;
    } else {
        // Remote git URL — shell out to `git clone`.
        let status = Command::new("git")
            // Allow file:// protocol in case the URL itself is file://.
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "protocol.file.allow")
            .env("GIT_CONFIG_VALUE_0", "always")
            .arg("clone")
            .arg(source)
            .arg(output)
            .output()
            .map_err(|e| ImportError::CloneFailed {
                detail: format!("spawning git clone: {e}"),
            })?;

        if !status.status.success() {
            return Err(ImportError::CloneFailed {
                detail: String::from_utf8_lossy(&status.stderr).into_owned(),
            });
        }
    }
    Ok(())
}

/// Recursively copy `src` directory tree into `dst` (which must not exist).
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
        // Symlinks are intentionally skipped (would need to resolve loop
        // detection; Anthropic skill sources don't typically use symlinks).
    }
    Ok(())
}

/// Build a [`tau_domain::PackageSource`] from the user-supplied source string.
///
/// Local paths → `file:///absolute/path` URL (accepted by PackageSource).
/// Git URLs → pass through to `PackageSource::from_str`.
fn build_package_source(source: &str) -> Result<tau_domain::PackageSource, ImportError> {
    if is_local_path(source) {
        // Convert to an absolute path then to a file:// URL.
        let path = if let Some(stripped) = source.strip_prefix("file://") {
            PathBuf::from(stripped)
        } else {
            let p = PathBuf::from(source);
            if p.is_absolute() {
                p
            } else {
                std::env::current_dir().map_err(ImportError::Io)?.join(p)
            }
        };
        // Forward-slash path for file:// URL (needed on Windows, harmless on POSIX).
        let path_str = path.to_string_lossy();
        let url_str = if path_str.starts_with('/') {
            format!("file://{path_str}")
        } else {
            format!("file:///{path_str}")
        };
        tau_domain::PackageSource::from_str(&url_str).map_err(|e| ImportError::InvalidSource {
            raw_source: source.to_owned(),
            detail: e.to_string(),
        })
    } else {
        tau_domain::PackageSource::from_str(source).map_err(|e| ImportError::InvalidSource {
            raw_source: source.to_owned(),
            detail: e.to_string(),
        })
    }
}
