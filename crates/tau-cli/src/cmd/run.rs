//! `tau run` — invoke an agent one-shot.
//!
//! Per spec §3.13 (run row) and §3.9.
//!
//! Reads the project `tau.toml`, resolves the named agent to an
//! `(AgentDefinition, PackageManifest)` pair, builds a [`Runtime`] with
//! either real plugins (Phase 1+) or — under `--features test-mock` —
//! a compiled-in mock LLM backend and `echo` tool, builds an initial
//! [`Message`] from `--prompt` / stdin, runs the agent, and maps the
//! resulting [`RunOutcome`] to stdout/stderr + an `ExitCode`.
//!
//! # Plugin loading caveat
//!
//! v0.1 has no plugin-loading layer (deferred to Phase 1+ per
//! `docs/retrospectives/phase-0-mid.md`). Without `--features test-mock`,
//! [`Runtime::builder`].build() returns
//! [`tau_runtime::BuildError::NoLlmBackend`] and `tau run` fails with
//! exit code 2. The `test-mock` feature exists so the integration tests
//! and downstream development work can exercise the run loop end-to-end.

use std::io::Read;

use anyhow::Context;
use tau_domain::{Address, AgentInstanceId, Message, MessagePayload};
use tau_runtime::{RunOptions, RunOutcome, Runtime};

use crate::cli::RunArgs;
use crate::config::ProjectConfig;
use crate::output::Output;

#[cfg(feature = "test-mock")]
pub(crate) mod mock_backend;

/// Marker error: the agent ran but reported [`RunOutcome::Failed`].
///
/// Threaded through the existing `anyhow::Result<()>` dispatch so
/// `lib::run_main` can downcast it and map to
/// [`crate::ExitCode::AgentFailed`] (exit code 1) — distinct from
/// kernel/CLI errors that map to [`crate::ExitCode::Error`] (exit
/// code 2). See the docstring on [`crate::ExitCode`] and ADR-0006 for
/// the Outcome/Error dichotomy.
#[derive(Debug, thiserror::Error)]
#[error("agent failed (exit code 1)")]
pub(crate) struct AgentFailed;

/// Run `tau run`.
pub async fn run(args: &RunArgs, output: &mut Output) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let project_path = cwd.join("tau.toml");
    let project = ProjectConfig::from_path(&project_path)
        .with_context(|| format!("project tau.toml required at {project_path:?}"))?;

    let entry = project.agents.get(&args.agent_id).ok_or_else(|| {
        anyhow::anyhow!(
            "agent id {:?} not found in project tau.toml (declare it under [agents.{}])",
            args.agent_id,
            args.agent_id
        )
    })?;

    let scope = tau_pkg::Scope::resolve(&cwd).context("resolving package scope")?;

    let (agent_def, manifest) = crate::config::build_agent_definition(entry, &cwd, &scope)
        .with_context(|| format!("resolving agent {:?}", args.agent_id))?;

    // Build the runtime. v0.1 plugin loading is deferred to Phase 1+
    // (per docs/retrospectives/phase-0-mid.md "What's NOT in scope"),
    // so without `--features test-mock` the registry is empty and
    // `RuntimeBuilder::build` returns `BuildError::NoLlmBackend`.
    #[allow(unused_mut)]
    let mut builder = Runtime::builder();
    #[cfg(feature = "test-mock")]
    {
        builder = builder
            .with_llm_backend(mock_backend::MockLlmBackend::new(entry.llm_backend.clone()))
            .with_tool(mock_backend::EchoTool);
    }

    let runtime = builder.build().context(
        "failed to build runtime (no LLM backend registered; plugin loading lands \
             in Phase 1+ — build with `--features test-mock` for testing)",
    )?;

    let mut options = RunOptions::default();
    if let Some(n) = args.max_turns {
        options.max_turns = n;
    }

    if args.dry_run {
        emit_dry_run(
            entry,
            &agent_def,
            &manifest,
            &options,
            args.prompt.as_deref(),
            output,
        )?;
        return Ok(());
    }

    // Build the initial user message from --prompt or stdin.
    let prompt_text = match &args.prompt {
        Some(s) => s.clone(),
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .context("reading initial prompt from stdin")?;
            buf
        }
    };

    let initial = Message::new(
        Address::User,
        // The kernel mints its own AgentInstanceId per run; this
        // recipient placeholder gets replaced once the loop assigns its
        // own. Using a fresh id keeps the typed Address::Agent variant
        // happy without leaking implementation detail.
        Address::Agent(AgentInstanceId::new()),
        MessagePayload::Text {
            content: prompt_text,
        },
    );

    let outcome = runtime
        .run(agent_def, manifest, initial, options)
        .await
        .context("running agent")?;

    match outcome {
        RunOutcome::Completed {
            ref final_message,
            total_turns,
            ref token_usage,
            ..
        } => {
            if output.is_json() {
                let payload = serde_json::json!({
                    "outcome": "completed",
                    "final_message": format_message_text(&final_message.payload),
                    "total_turns": total_turns,
                    "token_usage": {
                        "input_tokens": token_usage.input_tokens,
                        "output_tokens": token_usage.output_tokens,
                    },
                });
                output.json(&payload)?;
            } else {
                let text = format_message_text(&final_message.payload);
                output.human(&text)?;
            }
            Ok(())
        }
        RunOutcome::Failed {
            ref status,
            total_turns,
            ref token_usage,
            ..
        } => {
            if output.is_json() {
                let payload = serde_json::json!({
                    "outcome": "failed",
                    "status": format!("{status:?}"),
                    "total_turns": total_turns,
                    "token_usage": {
                        "input_tokens": token_usage.input_tokens,
                        "output_tokens": token_usage.output_tokens,
                    },
                });
                output.json(&payload)?;
            } else {
                output.error(format!("agent failed: {status:?}"))?;
            }
            // Marker error → ExitCode::AgentFailed (1) via downcast in
            // crate::run_main. Kernel/CLI errors map to ExitCode::Error
            // (2); this case is the explicit Outcome::Failed bucket.
            Err(AgentFailed.into())
        }
        // RunOutcome is `#[non_exhaustive]`; cross-crate match needs a
        // wildcard. Any future variant should be classified explicitly
        // by an ADR amendment; for now, treat unknown outcomes as a
        // kernel error.
        _ => Err(anyhow::anyhow!("unknown RunOutcome variant")),
    }
}

/// Project a [`MessagePayload`] to a single text string for display.
/// Non-text payloads degrade to a `Debug`-formatted preview.
fn format_message_text(payload: &MessagePayload) -> String {
    match payload {
        MessagePayload::Text { content } => content.clone(),
        other => format!("{other:?}"),
    }
}

/// Render the dry-run preview to stderr per spec §3.9.
fn emit_dry_run(
    entry: &crate::config::AgentEntry,
    agent_def: &tau_domain::AgentDefinition,
    manifest: &tau_domain::PackageManifest,
    options: &RunOptions,
    prompt: Option<&str>,
    output: &mut Output,
) -> anyhow::Result<()> {
    output.dry_run(format!(
        "agent:           {} ({})",
        entry.id, entry.display_name
    ))?;
    output.dry_run(format!(
        "package:         {} {}",
        manifest.name(),
        manifest.version()
    ))?;
    output.dry_run(format!("llm backend:     {}", entry.llm_backend))?;
    if let Some(sp) = &agent_def.system_prompt {
        let preview: String = sp.chars().take(80).collect();
        let suffix = if sp.chars().count() > 80 { "..." } else { "" };
        output.dry_run(format!("system prompt:   {preview}{suffix}"))?;
    } else {
        output.dry_run("system prompt:   (none)")?;
    }
    output.dry_run(format!(
        "granted caps:    {}",
        manifest.capabilities().len()
    ))?;
    output.dry_run(format!("max_turns:       {}", options.max_turns))?;
    let preview = prompt.unwrap_or("(stdin)");
    let len = preview.len();
    let trimmed: String = preview.chars().take(80).collect();
    let suffix = if preview.chars().count() > 80 {
        "..."
    } else {
        ""
    };
    output.dry_run(format!(
        "initial message: {:?}{} ({len} bytes)",
        trimmed, suffix
    ))?;
    output.dry_run("no LLM call made.")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tau_domain::Value;

    #[test]
    fn format_message_text_returns_text_content_directly() {
        let payload = MessagePayload::Text {
            content: "hello".into(),
        };
        assert_eq!(format_message_text(&payload), "hello");
    }

    #[test]
    fn format_message_text_falls_back_to_debug_for_non_text() {
        let payload = MessagePayload::ToolResult { body: Value::Null };
        let s = format_message_text(&payload);
        assert!(s.contains("ToolResult"), "got: {s}");
    }

    #[test]
    fn agent_failed_renders_error_message() {
        let err: anyhow::Error = AgentFailed.into();
        let s = format!("{err}");
        assert!(s.contains("agent failed"), "got: {s}");
    }
}
