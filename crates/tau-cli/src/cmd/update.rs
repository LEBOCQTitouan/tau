//! `tau update` — update an installed package to a newer or specific version.
//!
//! Per spec §2:
//!
//! - Resolves the active [`Scope`] (project or global).
//! - Parses the package name and optional version pin.
//! - Delegates to [`tau_pkg::update::update_package`].
//! - Emits a result-summary after the synchronous call returns:
//!   the library function runs end-to-end and returns an [`UpdateResult`];
//!   intermediate "installing…" progress events are not available without
//!   library hooks, so output is post-call summary only.
//!
//! Exit codes per ADR-0007 §7:
//! - 0: success.
//! - 2: any [`UpdateError`] (maps through `anyhow` to `ExitCode::Error`).

use std::str::FromStr;

use semver::Version;
use tau_domain::PackageName;
use tau_pkg::{update::update_package, LockFile, Scope};

use crate::cli::UpdateArgs;
use crate::output::Output;

/// Run `tau update`.
pub async fn run(args: &UpdateArgs, output: &mut Output) -> anyhow::Result<()> {
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

    // 3. Parse optional version pin.
    let version_pin: Option<Version> = args
        .version
        .as_deref()
        .map(|v| Version::parse(v).map_err(|e| anyhow::anyhow!("invalid version {:?}: {}", v, e)))
        .transpose()?;

    // 4. Look up the current active version from the lockfile (needed for
    //    JSON update_started event emitted before calling the library).
    let current_version: Option<String> = if scope.lockfile_path().exists() {
        LockFile::load(&scope.lockfile_path())
            .ok()
            .and_then(|lf| lf.find(&name).map(|p| p.active_version.to_string()))
    } else {
        None
    };

    // 5. JSON: emit update_started BEFORE calling the library.
    if output.is_json() {
        output.json(&serde_json::json!({
            "event": "update_started",
            "name": name.as_str(),
            "current": current_version.as_deref().unwrap_or("unknown"),
        }))?;
    }

    // 6. Delegate to tau_pkg::update_package.
    let result = update_package(&name, version_pin, &scope, args.prune)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // 7. Emit output.
    let transitive: Vec<serde_json::Value> = result
        .transitive_deps_changed
        .iter()
        .map(|(dep_name, dep_ver)| {
            serde_json::json!({ "name": dep_name.as_str(), "version": dep_ver.to_string() })
        })
        .collect();

    if output.is_json() {
        output.json(&serde_json::json!({
            "event": "update_completed",
            "name": name.as_str(),
            "from": result.from_version.to_string(),
            "to": result.to_version.to_string(),
            "pruned": args.prune,
            "transitive_deps_changed": transitive,
        }))?;
    } else {
        output.human(&format!(
            "\u{2713} Updated {}: {} \u{2192} {}",
            name.as_str(),
            result.from_version,
            result.to_version,
        ))?;
        output.human(&format!("  Active version: {}", result.to_version))?;
        if args.prune {
            output.human(&format!("  Pruned: {}", result.from_version))?;
        }
        if !result.transitive_deps_changed.is_empty() {
            let deps_str: Vec<String> = result
                .transitive_deps_changed
                .iter()
                .map(|(n, v)| format!("{}@{}", n.as_str(), v))
                .collect();
            output.human(&format!("  New transitive deps: {}", deps_str.join(", ")))?;
        }
    }

    Ok(())
}
