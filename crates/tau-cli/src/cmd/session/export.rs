//! `tau session export` — convert a session to jsonl/md/json.

use anyhow::Context;
use tau_pkg::Scope;

use crate::cli::{ExportFormat, SessionExportArgs};
use crate::output::Output;

/// Run `tau session export`.
pub async fn run(args: &SessionExportArgs, output: &mut Output) -> anyhow::Result<()> {
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

    match args.format {
        ExportFormat::Jsonl => {
            // Passthrough: read file and emit verbatim.
            let contents = std::fs::read_to_string(&path).context("reading session file")?;
            // Use print! to bypass the Output formatting.
            print!("{}", contents);
        }
        ExportFormat::Md => {
            let (header, entries) =
                crate::session::SessionReader::read(&path).map_err(|e| anyhow::anyhow!("{}", e))?;
            let rendered = crate::session::render_session(&header, &entries);
            print!("{}", rendered);
        }
        ExportFormat::Json => {
            // Single envelope JSON: { header, messages, turn_summaries }.
            let (header, entries) =
                crate::session::SessionReader::read(&path).map_err(|e| anyhow::anyhow!("{}", e))?;
            let messages: Vec<&tau_domain::Message> = entries
                .iter()
                .filter_map(|e| match e {
                    crate::session::SessionEntry::Message(m) => Some(m),
                    _ => None,
                })
                .collect();
            let turn_summaries: Vec<serde_json::Value> = entries
                .iter()
                .filter_map(|e| match e {
                    crate::session::SessionEntry::TurnSummary {
                        turn,
                        stop_reason,
                        input_tokens,
                        output_tokens,
                    } => Some(serde_json::json!({
                        "turn": turn,
                        "stop_reason": stop_reason,
                        "input_tokens": input_tokens,
                        "output_tokens": output_tokens,
                    })),
                    _ => None,
                })
                .collect();
            let envelope = serde_json::json!({
                "header": header,
                "messages": messages,
                "turn_summaries": turn_summaries,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&envelope).context("serializing envelope")?
            );
        }
    }

    // Suppress unused warning: output is required by the dispatch signature.
    let _ = output;
    Ok(())
}
