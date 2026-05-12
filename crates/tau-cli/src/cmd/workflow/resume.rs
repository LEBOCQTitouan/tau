//! `tau workflow resume <run-id> [--force]` — continue an interrupted run.

use std::sync::Arc;

use anyhow::Context;
use tau_pkg::Scope;
use tau_workflow::{check_drift, persistence::replay, RunOpts, Runner, Workflow};

use crate::cli::WorkflowResumeArgs;
use crate::output::Output;

/// Run `tau workflow resume`.
pub async fn run(args: &WorkflowResumeArgs, output: &mut Output) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let scope = Scope::resolve(&cwd).context("resolving package scope")?;

    // Locate the JSONL log by run id.
    let runs_dir = scope.path().join(".tau").join("workflow-runs");
    let log_entry = std::fs::read_dir(&runs_dir)?
        .filter_map(|e| e.ok())
        .find(|e| {
            e.file_name()
                .to_str()
                .map(|s| s.contains(&args.run_id) && s.ends_with(".jsonl"))
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow::anyhow!("no run log for {:?}", args.run_id))?;
    let log_path = log_entry.path();

    // Workflow name is the prefix of the filename before -<run_id>.jsonl.
    let workflow_name = log_path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.rsplit_once('-').map(|(name, _)| name.to_string()))
        .ok_or_else(|| anyhow::anyhow!("cannot derive workflow name from {log_path:?}"))?;

    let wf_path = cwd
        .join("workflows")
        .join(format!("{}.toml", workflow_name));
    let workflow = Workflow::from_path(&wf_path).with_context(|| format!("parsing {wf_path:?}"))?;

    let records = replay(&log_path).await.context("replaying log")?;

    if let Err(e) = check_drift(&workflow, &records) {
        if !args.force {
            return Err(anyhow::anyhow!(
                "{e}\n\nThe workflow file has changed since the original run. \
                 Use `--force` to override.\n"
            ));
        }
        tracing::warn!(
            "workflow drift detected on resume; --force was supplied so proceeding anyway"
        );
    }

    // Recover the original input string from the FIRST record (its `input`
    // field is the user-supplied ${input} at run time).
    let original_input = records.first().map(|r| r.input.clone()).unwrap_or_default();

    let agents = crate::cmd::workflow::build_agents_map(&workflow, &cwd, &scope)?;
    let runtime = Arc::new(
        crate::cmd::workflow::build_runtime_for_workflow(&cwd, &scope, &agents)
            .await
            .context("building runtime for workflow")?,
    );
    let runner = Runner::new(runtime, scope.path().to_path_buf());

    let outcome = runner
        .run(
            &workflow,
            RunOpts {
                input: original_input,
                run_id: Some(args.run_id.clone()),
                completed: records,
                agents,
            },
        )
        .await
        .with_context(|| format!("resuming workflow {workflow_name:?}"))?;

    eprintln!("run_id: {} (resumed)", outcome.run_id);
    output.human(&outcome.last_output)?;

    if outcome.success {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "workflow {:?} still failed after resume (see `tau workflow log {}`)",
            workflow_name,
            outcome.run_id
        ))
    }
}
