//! `tau session list` — list sessions in the current scope.

use std::time::SystemTime;

use anyhow::Context;
use tau_pkg::Scope;

use crate::cli::SessionListArgs;
use crate::output::Output;

/// Run `tau session list`.
pub async fn run(args: &SessionListArgs, output: &mut Output) -> anyhow::Result<()> {
    let scope = if args.global {
        Scope::global().context("resolving global scope")?
    } else {
        let cwd = std::env::current_dir().context("getting current directory")?;
        Scope::resolve(&cwd).context("resolving project scope")?
    };

    let sessions_dir = scope.state_path().join("sessions");
    let mut metas = crate::session::list_sessions(&sessions_dir, args.agent.as_deref())
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let total = metas.len();
    if !args.all {
        metas.truncate(args.limit);
    }

    if output.is_json() {
        // Emit per-line events.
        output.json(&serde_json::json!({
            "event": "sessions",
            "total": total,
            "limit": if args.all { 0usize } else { args.limit },
        }))?;
        for meta in &metas {
            let created_at_str = humantime::format_rfc3339_seconds(meta.created_at).to_string();
            output.json(&serde_json::json!({
                "event": "session",
                "id": meta.id,
                "prefix": meta.short,
                "agent": meta.agent_id,
                "created_at": created_at_str,
                "turns": meta.turn_count,
                "title": meta.title,
            }))?;
        }
    } else {
        // Human table output.
        if metas.is_empty() {
            let scope_label = if args.global { "global" } else { "project" };
            output.human(&format!("No sessions in {} scope.", scope_label))?;
            return Ok(());
        }
        // Header row.
        output.human(&format!(
            "{:<10} {:<15} {:<19} {:>6}  {}",
            "ID", "AGENT", "CREATED", "TURNS", "TITLE"
        ))?;
        for meta in &metas {
            let created_at_str = format_timestamp_short(meta.created_at);
            let title = meta.title.as_deref().unwrap_or("-");
            output.human(&format!(
                "{:<10} {:<15} {:<19} {:>6}  {}",
                meta.short, meta.agent_id, created_at_str, meta.turn_count, title
            ))?;
        }
    }
    Ok(())
}

fn format_timestamp_short(t: SystemTime) -> String {
    // Format as "YYYY-MM-DD HH:MM" using humantime + truncation.
    let full = humantime::format_rfc3339_seconds(t).to_string();
    // full is like "2026-05-01T14:33:21Z" — replace 'T' with ' ' and drop seconds + 'Z'.
    if let Some(colon_pos) = full.rfind(':') {
        let truncated = &full[..colon_pos];
        truncated.replace('T', " ")
    } else {
        full.replace('T', " ")
    }
}
