//! `tau workflow log <run-id>` — pretty-print or JSON-dump a run log.

use anyhow::Context;
use tau_pkg::Scope;
use tau_workflow::persistence::{replay, StepStatus};

use crate::cli::WorkflowLogArgs;
use crate::output::Output;

/// Run `tau workflow log`.
pub async fn run(args: &WorkflowLogArgs, output: &mut Output) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let scope = Scope::resolve(&cwd).context("resolving package scope")?;

    let runs_dir = scope.path().join(".tau").join("workflow-runs");
    if !runs_dir.is_dir() {
        anyhow::bail!("no workflow runs found under {runs_dir:?}");
    }

    let mut found_path: Option<std::path::PathBuf> = None;
    for entry in std::fs::read_dir(&runs_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.contains(&args.run_id) && s.ends_with(".jsonl"))
            .unwrap_or(false)
        {
            found_path = Some(path);
            break;
        }
    }

    let path = found_path
        .ok_or_else(|| anyhow::anyhow!("no run log found for run id {:?}", args.run_id))?;
    let records = replay(&path).await.map_err(|e| anyhow::anyhow!(e)).context("replaying log")?;

    if args.json {
        for record in &records {
            output.human(&serde_json::to_string(record)?)?;
        }
        return Ok(());
    }

    let workflow_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.rsplit_once('-').map(|(name, _)| name))
        .unwrap_or("?");
    let completed_marker = if records
        .last()
        .map(|r| r.status == StepStatus::Ok)
        .unwrap_or(false)
    {
        "✓ completed"
    } else {
        "✗ failed"
    };
    output.human(&format!(
        "{workflow_name} / run {}                     {completed_marker}",
        args.run_id
    ))?;
    for record in &records {
        let status = match record.status {
            StepStatus::Ok => "ok",
            StepStatus::Failed => "failed",
        };
        output.human(&format!(
            "  [{}] {:<12} {:<10} {:.1}s   {}",
            record.step_index,
            record.step_id,
            record.kind,
            record.duration_ms as f64 / 1000.0,
            status,
        ))?;
        output.human(&format!("      input:  {:?}", record.input))?;
        output.human(&format!("      output: {:?}", record.output))?;
        if let Some(detail) = &record.detail {
            output.human(&format!("      error:  {detail}"))?;
        }
    }

    Ok(())
}
