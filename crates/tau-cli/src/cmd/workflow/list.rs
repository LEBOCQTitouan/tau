//! `tau workflow list` — show workflows declared under workflows/.

use std::fs;

use crate::output::Output;

/// Run `tau workflow list`.
pub fn run(output: &mut Output) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let workflows_dir = cwd.join("workflows");

    if !workflows_dir.is_dir() {
        output.human("No workflows/ directory in this project.")?;
        return Ok(());
    }

    let mut names: Vec<String> = fs::read_dir(&workflows_dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                path.file_stem().and_then(|s| s.to_str()).map(String::from)
            } else {
                None
            }
        })
        .collect();
    names.sort();

    if names.is_empty() {
        output.human("No workflow TOML files found under workflows/.")?;
        return Ok(());
    }

    for name in names {
        output.human(&name)?;
    }
    Ok(())
}
