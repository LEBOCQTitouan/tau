//! `tau uninstall` — remove an installed package from the active scope.
//!
//! Per spec §4:
//!
//! - Resolves the active [`Scope`] (project or global).
//! - Parses the package name and optional version.
//! - Delegates to [`tau_pkg::uninstall`].
//! - Prints either a human-readable summary with a remediation hint
//!   (§4.2) or, when `--json` is set, four per-line JSON events (§4.3).
//!
//! Errors propagate via `anyhow::Error` and exit 2 through the
//! top-level `dispatch` in [`crate::lib`].

use std::str::FromStr;

use semver::Version;
use tau_domain::PackageName;
use tau_pkg::{uninstall, Scope, UninstallError};

use crate::cli::UninstallArgs;
use crate::output::Output;

/// Run `tau uninstall`.
pub async fn run(args: &UninstallArgs, output: &mut Output) -> anyhow::Result<()> {
    // 1. Resolve scope.
    let scope = if args.global {
        Scope::global()?
    } else {
        let cwd = std::env::current_dir()?;
        Scope::resolve(&cwd)?
    };

    // 2. Parse package name.
    let name = PackageName::from_str(&args.package)
        .map_err(|e| anyhow::anyhow!("invalid package name {:?}: {}", args.package, e))?;

    // 3. Parse optional version.
    let version: Option<Version> = args
        .version
        .as_deref()
        .map(|v| Version::parse(v).map_err(|e| anyhow::anyhow!("invalid version {:?}: {}", v, e)))
        .transpose()?;

    // 4. JSON: emit uninstall_started before doing anything.
    if output.is_json() {
        output.json(&serde_json::json!({
            "event": "uninstall_started",
            "name": name.as_str(),
            "version": version.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "all".to_owned()),
        }))?;
    }

    // 5. Delegate to tau_pkg::uninstall.
    let pkg_dir = scope.packages_dir().join(name.as_str());
    let lockfile_path = scope.lockfile_path();

    uninstall(&name, version.as_ref(), &scope).map_err(map_uninstall_error)?;

    // 6. Emit output.
    if output.is_json() {
        output.json(&serde_json::json!({
            "event": "removed_dir",
            "path": pkg_dir.to_string_lossy(),
        }))?;

        // Count remaining entries in the lockfile (0 if file is gone).
        let entries_remaining = count_lockfile_entries(&lockfile_path);

        output.json(&serde_json::json!({
            "event": "lockfile_updated",
            "entries_remaining": entries_remaining,
        }))?;

        output.json(&serde_json::json!({
            "event": "uninstall_completed",
            "name": name.as_str(),
            "version": version.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "all".to_owned()),
        }))?;
    } else {
        let version_display = version
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "*".to_owned());
        output.human(&format!(
            "\u{2713} Uninstalled {}@{}.\n  Removed: {}\n  Lockfile: {}",
            name.as_str(),
            version_display,
            pkg_dir.display(),
            lockfile_path.display(),
        ))?;
        output.human("")?;
        output.human(&format!(
            "If any project still depends on {}:\n  \u{2022} Remove or update the [[agents.<id>.requires.tools]] entry\n    for {} in the project's tau.toml.\n  \u{2022} Or re-install on next run: cd <project> && tau resolve",
            name.as_str(),
            name.as_str(),
        ))?;
    }

    Ok(())
}

/// Count remaining `[[package]]` entries in the lockfile.
fn count_lockfile_entries(lockfile_path: &std::path::Path) -> usize {
    if !lockfile_path.exists() {
        return 0;
    }
    let contents = std::fs::read_to_string(lockfile_path).unwrap_or_default();
    contents.matches("[[package]]").count()
}

/// Map `UninstallError` to an `anyhow::Error`. All variants map to
/// exit code 2 (the generic `ExitCode::Error` bucket per ADR-0007 §7).
fn map_uninstall_error(e: UninstallError) -> anyhow::Error {
    anyhow::anyhow!("{}", e)
}
