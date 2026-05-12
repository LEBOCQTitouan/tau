//! `tau workflow run <name> [--input <s>]` — execute a workflow.

use std::sync::Arc;

use anyhow::Context;

use tau_pkg::Scope;
use tau_workflow::{RunOpts, Runner, Workflow};

use crate::cli::WorkflowRunArgs;
use crate::output::Output;

/// Run `tau workflow run`.
pub async fn run(args: &WorkflowRunArgs, output: &mut Output) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let scope = Scope::resolve(&cwd).context("resolving package scope")?;

    let wf_path = cwd
        .join("workflows")
        .join(format!("{}.toml", args.name));
    let workflow = Workflow::from_path(&wf_path)
        .with_context(|| format!("parsing workflow at {wf_path:?}"))?;

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
                input: args.input.clone(),
                run_id: None,
                completed: Vec::new(),
                agents,
            },
        )
        .await
        .with_context(|| format!("running workflow {:?}", args.name))?;

    eprintln!("run_id: {}", outcome.run_id);
    output.human(&outcome.last_output)?;

    if outcome.success {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "workflow {:?} failed (see `tau workflow log {}` for details)",
            args.name,
            outcome.run_id
        ))
    }
}
