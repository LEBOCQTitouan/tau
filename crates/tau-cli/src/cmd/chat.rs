//! `tau chat` — interactive REPL with an agent.
//!
//! Per spec §3.13 (chat row) and brainstorm Section 8.
//!
//! Mirrors `cmd::run`'s setup (project tau.toml → resolved
//! `(AgentDefinition, PackageManifest)` → spawned plugin processes →
//! [`Runtime`]), then enters a rustyline-driven REPL. Each user prompt
//! is sent through [`Runtime::run_with_history`] so the conversation
//! accumulates across turns; tool dispatch and capability filtering
//! are inherited from the kernel, not re-implemented here.
//!
//! # Plugin lifecycle
//!
//! Per spec §11: plugin processes spawn once at REPL entry and stay
//! alive for the duration of the session (multiplexed long-lived
//! lifecycle). On `/exit` (or EOF / Ctrl-D), the [`Runtime`] is
//! dropped, which drops the `Arc<dyn DynLlmBackend>` and per-tool
//! `Arc<dyn DynTool>`, which drops the underlying `PluginProcess` —
//! and `kill_on_drop` ensures the plugin subprocess exits cleanly.
//!
//! # Slash commands
//!
//! See [`SlashCommand`] for the recognised set. Parsing is intentionally
//! strict: only the four documented commands are slash-handled, and
//! anything starting with `/` that isn't a known command is forwarded
//! to the LLM as a normal prompt. This keeps the surface predictable
//! and lets users send messages that happen to start with `/`.
//!
//! # Failure handling
//!
//! - `Ok(RunOutcome::Failed { .. })`: the kernel reported a typed
//!   agent-level failure (e.g. `OutOfResources`). The REPL renders the
//!   detail in red, accumulates partial history + tokens, and continues
//!   — the user can ask a follow-up. (No process exit on agent failure
//!   here, unlike `tau run`; chat is interactive.)
//! - `Err(RuntimeError)`: kernel/operational error. The REPL prints the
//!   error and aborts with the marker bubbling up to `run_main`, which
//!   maps it to [`crate::ExitCode::Error`] (2).
//!
//! # JSON
//!
//! `--json` is rejected at handler entry: a streaming, terminal-driven
//! REPL has no useful JSON projection. (See spec §3.6.)

use anyhow::Context;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use tau_domain::{Address, AgentInstanceId, Message, MessagePayload};
use tau_plugin_protocol::handshake::TraceContext;
use tau_runtime::plugin_host::PluginHostOptions;
use tau_runtime::{RunOptions, RunOutcome, TokenUsage};

use crate::cli::ChatArgs;
use crate::cmd::plugin_loader;
use crate::config::ProjectConfig;
use crate::output::Output;

/// What a single line of REPL input represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplInput {
    /// A recognised slash command.
    Slash(SlashCommand),
    /// Free-form text to forward to the agent. Whitespace-preserved.
    Prompt(String),
}

/// Slash commands recognised in the REPL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    /// End the session and print the token-usage summary.
    Exit,
    /// Print the slash-command cheat sheet.
    Help,
    /// Drop the in-memory conversation history.
    Clear,
    /// Render the current conversation history.
    History,
}

/// Parse a single line of REPL input.
///
/// Trims the line for the slash-command match (so `"  /exit "` still
/// counts) but, for `Prompt`, returns the original line unchanged —
/// trailing whitespace can be load-bearing for some prompts.
///
/// Unknown slash forms (`/foo`) fall through to `Prompt`, intentionally:
/// see the module docs for the rationale.
pub fn parse_input(line: &str) -> ReplInput {
    let trimmed = line.trim();
    match trimmed {
        "/exit" => ReplInput::Slash(SlashCommand::Exit),
        "/help" => ReplInput::Slash(SlashCommand::Help),
        "/clear" => ReplInput::Slash(SlashCommand::Clear),
        "/history" => ReplInput::Slash(SlashCommand::History),
        _ => ReplInput::Prompt(line.to_string()),
    }
}

/// Run `tau chat`.
pub async fn run(args: &ChatArgs, output: &mut Output) -> anyhow::Result<()> {
    if output.is_json() {
        anyhow::bail!("tau chat does not support --json (REPL is interactive)");
    }

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

    let mut options = RunOptions::default();
    if let Some(n) = args.max_turns {
        options.max_turns = n;
    }

    if args.dry_run {
        // Dry-run skips plugin spawn entirely (per spec §3.9: no
        // process side-effects for a preview).
        emit_dry_run_preview(entry, &agent_def, &manifest, &options, output)?;
        return Ok(());
    }

    // ---- Plugin loading -----------------------------------------------------
    //
    // Spawn the LLM backend + tool plugins once for the entire REPL.
    // The runtime owns the Arc<dyn Dyn*> shims; on /exit (or EOF), the
    // runtime is dropped, which drops the shims, which drops the
    // PluginProcess (kill_on_drop ensures the child exits).

    let run_id = format!(
        "tau-chat-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let trace_context = TraceContext::new(run_id, args.agent_id.clone(), "root".to_string());
    let host_options = PluginHostOptions::default();

    let loaded = plugin_loader::load_plugins(entry, &scope, trace_context, host_options).await?;

    let runtime = loaded
        .builder
        .build()
        .context("failed to build runtime from spawned plugins")?;

    // Welcome banner.
    output.status(format!(
        "Welcome to tau chat with agent '{}' ({}@{}). Type /help for commands, /exit or Ctrl-D to quit.",
        entry.id,
        manifest.name(),
        manifest.version()
    ))?;

    let mut editor = DefaultEditor::new().context("initialising rustyline editor")?;
    let mut history: Vec<Message> = Vec::new();
    let mut total_tokens = TokenUsage::default();

    loop {
        let line = match editor.readline("> ") {
            Ok(line) => line,
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => {
                emit_session_summary(total_tokens, output)?;
                return Ok(());
            }
            Err(e) => {
                return Err(anyhow::Error::from(e).context("readline failure"));
            }
        };

        // Best-effort: rustyline returns Err if the history is full. We
        // don't surface that — it's pure input ergonomics, not state.
        let _ = editor.add_history_entry(line.as_str());

        match parse_input(&line) {
            ReplInput::Slash(SlashCommand::Exit) => {
                emit_session_summary(total_tokens, output)?;
                return Ok(());
            }
            ReplInput::Slash(SlashCommand::Help) => {
                print_help(output)?;
            }
            ReplInput::Slash(SlashCommand::Clear) => {
                history.clear();
                output.status("history cleared.")?;
            }
            ReplInput::Slash(SlashCommand::History) => {
                render_history(&history, output)?;
            }
            ReplInput::Prompt(text) if text.trim().is_empty() => {
                // Empty / whitespace-only input — no-op. Skip the round
                // trip rather than send an empty prompt.
                continue;
            }
            ReplInput::Prompt(text) => {
                let initial = Message::new(
                    Address::User,
                    // The kernel mints its own per-run AgentInstanceId;
                    // this recipient placeholder is overwritten by the
                    // loop. Fresh id keeps the typed Address::Agent
                    // variant honest.
                    Address::Agent(AgentInstanceId::new()),
                    MessagePayload::Text { content: text },
                );
                let outcome = runtime
                    .run_with_history(
                        agent_def.clone(),
                        manifest.clone(),
                        history.clone(),
                        initial,
                        options.clone(),
                    )
                    .await;

                match outcome {
                    Ok(RunOutcome::Completed {
                        final_message,
                        all_messages,
                        token_usage,
                        ..
                    }) => {
                        render_final_message(&final_message, output)?;
                        accumulate_tokens(&mut total_tokens, &token_usage);
                        history = all_messages;
                    }
                    Ok(RunOutcome::Failed {
                        status,
                        all_messages,
                        token_usage,
                        ..
                    }) => {
                        // Agent-level failure: render in red and let the
                        // user retry. (Distinct from a kernel error,
                        // which aborts the REPL below.)
                        output.error(format!("agent failed: {status:?}"))?;
                        accumulate_tokens(&mut total_tokens, &token_usage);
                        history = all_messages;
                    }
                    Ok(_) => {
                        return Err(anyhow::anyhow!("unknown RunOutcome variant"));
                    }
                    Err(e) => {
                        // Kernel/operational error: print and abort the
                        // REPL with a kernel-error exit. Token usage so
                        // far is still surfaced.
                        output.error(format!("kernel error: {e}"))?;
                        return Err(anyhow::Error::from(e));
                    }
                }
            }
        }
    }
}

/// Render the final message of a completed turn. Text payloads go
/// through termimad for markdown rendering; non-text payloads degrade
/// to a `Debug` preview through the standard human channel.
fn render_final_message(msg: &Message, output: &mut Output) -> anyhow::Result<()> {
    match &msg.payload {
        MessagePayload::Text { content } => {
            // termimad writes directly to stdout via crossterm; this
            // bypasses `Output`'s writer abstraction, but the REPL only
            // runs in non-JSON mode (rejected at entry) and stdout is
            // exactly where the agent's text response belongs.
            let skin = termimad::MadSkin::default_dark();
            skin.print_text(content);
            Ok(())
        }
        other => output
            .human(&format!("{other:?}"))
            .context("writing non-text final message"),
    }
}

/// Accumulate per-turn token usage into the session-wide running total.
fn accumulate_tokens(total: &mut TokenUsage, delta: &TokenUsage) {
    total.input_tokens = total.input_tokens.saturating_add(delta.input_tokens);
    total.output_tokens = total.output_tokens.saturating_add(delta.output_tokens);
    // `total_tokens` is the backend-reported unified count when present;
    // sum it when both sides are Some, otherwise leave the existing
    // value. Most backends report only input/output, so this is largely
    // a courtesy.
    total.total_tokens = match (total.total_tokens, delta.total_tokens) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };
}

/// Render a one-line summary of every message in the session history.
fn render_history(history: &[Message], output: &mut Output) -> anyhow::Result<()> {
    if history.is_empty() {
        output.human("(no history yet)")?;
        return Ok(());
    }
    for (i, msg) in history.iter().enumerate() {
        let preview = match &msg.payload {
            MessagePayload::Text { content } => {
                let p: String = content.chars().take(80).collect();
                if content.chars().count() > 80 {
                    format!("{p}...")
                } else {
                    p
                }
            }
            other => format!("{other:?}"),
        };
        output.human(&format!(
            "[{i}] {:?} -> {:?}: {preview}",
            msg.sender, msg.recipient
        ))?;
    }
    Ok(())
}

/// Print the slash-command cheat sheet to stdout (so `/help` shows up
/// in piped runs without a `--quiet`-able status channel suppressing it).
fn print_help(output: &mut Output) -> anyhow::Result<()> {
    output.human("Slash commands:")?;
    output.human("  /exit      End the session.")?;
    output.human("  /help      Show this help.")?;
    output.human("  /clear     Clear conversation history.")?;
    output.human("  /history   Show conversation history.")?;
    Ok(())
}

/// Emit the end-of-session token-usage summary to stderr.
fn emit_session_summary(tokens: TokenUsage, output: &mut Output) -> anyhow::Result<()> {
    output.status(format!(
        "session ended; total tokens: {} in / {} out",
        tokens.input_tokens, tokens.output_tokens
    ))?;
    Ok(())
}

/// Render the dry-run preview to stderr. Mirrors `cmd::run`'s preview,
/// minus the `initial message:` row (no prompt yet — we'd be entering
/// the REPL).
fn emit_dry_run_preview(
    entry: &crate::config::AgentEntry,
    agent_def: &tau_domain::AgentDefinition,
    manifest: &tau_domain::PackageManifest,
    options: &RunOptions,
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
    output.dry_run("REPL would start with the above setup.")?;
    output.dry_run("no session opened.")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exit() {
        assert_eq!(parse_input("/exit"), ReplInput::Slash(SlashCommand::Exit));
    }

    #[test]
    fn parse_help() {
        assert_eq!(parse_input("/help"), ReplInput::Slash(SlashCommand::Help));
    }

    #[test]
    fn parse_clear() {
        assert_eq!(parse_input("/clear"), ReplInput::Slash(SlashCommand::Clear));
    }

    #[test]
    fn parse_history() {
        assert_eq!(
            parse_input("/history"),
            ReplInput::Slash(SlashCommand::History)
        );
    }

    #[test]
    fn parse_unknown_slash_is_prompt() {
        // Unknown slash commands fall through to `Prompt` so the user
        // can send messages that happen to start with `/` without
        // surprise rejections.
        assert_eq!(parse_input("/foo"), ReplInput::Prompt("/foo".to_string()));
    }

    #[test]
    fn parse_text_is_prompt() {
        assert_eq!(
            parse_input("hello world"),
            ReplInput::Prompt("hello world".to_string())
        );
    }

    #[test]
    fn parse_strips_whitespace_for_slash_match() {
        // Leading/trailing whitespace on a slash command shouldn't
        // defeat recognition — most terminals append a trailing newline
        // and copy/paste often introduces leading whitespace.
        assert_eq!(
            parse_input("  /exit  "),
            ReplInput::Slash(SlashCommand::Exit)
        );
    }

    #[test]
    fn parse_text_preserves_whitespace() {
        // For a text Prompt, preserve the original line — whitespace
        // can be load-bearing in agent prompts (code blocks, indentation).
        let result = parse_input("  hello  ");
        assert_eq!(result, ReplInput::Prompt("  hello  ".to_string()));
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn accumulate_tokens_sums_input_and_output() {
        // `TokenUsage` is `#[non_exhaustive]`; struct-literal construction
        // is blocked from this crate. Use default() + field mutation.
        let mut total = TokenUsage::default();
        let mut delta = TokenUsage::default();
        delta.input_tokens = 10;
        delta.output_tokens = 5;
        accumulate_tokens(&mut total, &delta);
        assert_eq!(total.input_tokens, 10);
        assert_eq!(total.output_tokens, 5);
        assert_eq!(total.total_tokens, None);

        let mut delta2 = TokenUsage::default();
        delta2.input_tokens = 3;
        delta2.output_tokens = 7;
        delta2.total_tokens = Some(10);
        accumulate_tokens(&mut total, &delta2);
        assert_eq!(total.input_tokens, 13);
        assert_eq!(total.output_tokens, 12);
        // First delta had None; subsequent Some carries through.
        assert_eq!(total.total_tokens, Some(10));
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn accumulate_tokens_saturates_on_overflow() {
        let mut total = TokenUsage::default();
        total.input_tokens = u64::MAX;
        let mut delta = TokenUsage::default();
        delta.input_tokens = 1;
        accumulate_tokens(&mut total, &delta);
        // saturating_add prevents wrap; the running total stays pinned
        // at u64::MAX rather than rolling to 0.
        assert_eq!(total.input_tokens, u64::MAX);
    }
}
