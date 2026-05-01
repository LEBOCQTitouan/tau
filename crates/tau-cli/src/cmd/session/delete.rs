//! `tau session delete` — remove a session file.

use std::io::{BufRead, Write};

use anyhow::Context;
use tau_pkg::Scope;

use crate::cli::SessionDeleteArgs;
use crate::output::Output;

/// Run `tau session delete`.
pub async fn run(args: &SessionDeleteArgs, output: &mut Output) -> anyhow::Result<()> {
    let scope = if args.global {
        Scope::global().context("resolving global scope")?
    } else {
        let cwd = std::env::current_dir().context("getting current directory")?;
        Scope::resolve(&cwd).context("resolving project scope")?
    };

    let sessions_dir = scope.state_path().join("sessions");
    let sid = crate::session::resolve_id_prefix(&sessions_dir, &args.id)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let path = sessions_dir.join(format!("{}.jsonl", sid.as_str()));

    // Read header for the confirmation prompt (cheap; just first line).
    let (header, entries) =
        crate::session::SessionReader::read(&path).map_err(|e| anyhow::anyhow!("{}", e))?;
    let turn_count = entries
        .iter()
        .filter(|e| matches!(e, crate::session::SessionEntry::TurnSummary { .. }))
        .count();

    if !args.force && !output.is_json() {
        // Interactive confirmation prompt.
        let created_at_str = humantime::format_rfc3339_seconds(header.created_at).to_string();
        eprint!(
            "About to delete session {} ({}, {} turns, {}).\nContinue? [y/N] ",
            sid.short(),
            header.agent_id,
            turn_count,
            created_at_str,
        );
        std::io::stderr().flush().ok();

        let stdin = std::io::stdin();
        let mut line = String::new();
        stdin
            .lock()
            .read_line(&mut line)
            .context("reading confirmation")?;
        let trimmed = line.trim().to_lowercase();
        if !matches!(trimmed.as_str(), "y" | "yes") {
            output.human("Aborted.")?;
            return Ok(());
        }
    }

    std::fs::remove_file(&path).context("removing session file")?;

    if output.is_json() {
        output.json(&serde_json::json!({
            "event": "deleted",
            "id": sid.as_str(),
        }))?;
    } else {
        output.human(&format!("Deleted session {}", sid.short()))?;
    }

    Ok(())
}
