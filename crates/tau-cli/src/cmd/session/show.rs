//! `tau session show` — print transcript of a session.

use anyhow::Context;
use tau_pkg::Scope;

use crate::cli::SessionShowArgs;
use crate::output::Output;

/// Run `tau session show`.
pub async fn run(args: &SessionShowArgs, output: &mut Output) -> anyhow::Result<()> {
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

    if output.is_json() {
        // JSONL passthrough — emit each line as a JSON event.
        let contents = std::fs::read_to_string(&path).context("reading session file")?;
        for line in contents.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(line)
                .with_context(|| format!("parsing JSONL line: {line}"))?;
            output.json(&value)?;
        }
    } else {
        let (header, entries) =
            crate::session::SessionReader::read(&path).map_err(|e| anyhow::anyhow!("{}", e))?;
        let rendered = crate::session::render_session(&header, &entries);
        output.human(&rendered)?;
    }
    Ok(())
}
