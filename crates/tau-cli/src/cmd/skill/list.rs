//! `tau skill list` — enumerate installed skill packages.
//!
//! Reads the lockfile only (no per-skill disk seeks). Skills-2's
//! `LockedSkill.frontmatter` already cached name + description.

use anyhow::Context;
use serde::Serialize;

use crate::cli::SkillListArgs;
use crate::output::Output;

#[derive(Debug, Serialize)]
struct SkillListItem {
    name: String,
    version: String,
    description: String,
    source: String,
    install_path: String,
}

#[derive(Debug, Serialize)]
struct SkillListJson {
    skills: Vec<SkillListItem>,
}

/// Run `tau skill list`.
pub async fn run(args: &SkillListArgs, _output: &mut Output) -> anyhow::Result<()> {
    let cwd = std::env::current_dir().context("getting current directory")?;
    let scope = tau_pkg::Scope::resolve(&cwd).context("resolving scope")?;
    let lockfile_path = scope.lockfile_path();
    let lockfile = if lockfile_path.exists() {
        tau_pkg::lockfile::LockFile::load(&lockfile_path).context("loading lockfile")?
    } else {
        tau_pkg::lockfile::LockFile::default()
    };

    let mut items: Vec<SkillListItem> = lockfile
        .packages
        .iter()
        .filter_map(|pkg| {
            pkg.skill.as_ref().map(|skill| SkillListItem {
                name: pkg.name.as_str().to_string(),
                version: pkg.active_version.to_string(),
                description: skill.frontmatter.description.clone(),
                source: pkg.source.to_string(),
                install_path: scope
                    .path()
                    .join("packages")
                    .join(pkg.name.as_str())
                    .join(pkg.active_version.to_string())
                    .display()
                    .to_string(),
            })
        })
        .collect();
    items.sort_by(|a, b| a.name.cmp(&b.name));

    if args.json {
        let json = serde_json::to_string_pretty(&SkillListJson { skills: items })
            .context("serializing list to JSON")?;
        println!("{json}");
        return Ok(());
    }

    if items.is_empty() {
        println!("no skills installed.");
        println!("hint: install one with `tau install <git-url>`");
        return Ok(());
    }

    // Column layout: NAME (auto), VERSION (auto), DESCRIPTION (truncated to 60).
    const DESC_MAX: usize = 60;
    let name_w = items.iter().map(|i| i.name.len()).max().unwrap_or(4).max(4);
    let ver_w = items.iter().map(|i| i.version.len()).max().unwrap_or(7).max(7);

    println!(
        "{:<name_w$}  {:<ver_w$}  DESCRIPTION",
        "NAME",
        "VERSION",
        name_w = name_w,
        ver_w = ver_w,
    );
    for item in &items {
        let desc = if item.description.chars().count() > DESC_MAX {
            let mut s: String = item.description.chars().take(DESC_MAX - 1).collect();
            s.push('…');
            s
        } else {
            item.description.clone()
        };
        println!(
            "{:<name_w$}  {:<ver_w$}  {desc}",
            item.name,
            item.version,
            name_w = name_w,
            ver_w = ver_w,
        );
    }
    Ok(())
}
