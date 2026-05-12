//! npm/cargo-style line-feed printer for multi-agent runs.
//!
//! Subscribes to a TraceEvent receiver, renders one line per significant
//! event, prints a summary table at end. No TUI; no cursor magic. Pipe-
//! friendly via space-padded alignment.

use std::collections::BTreeMap;

use tau_ports::{RunSnapshot, TraceEvent, TraceEventKind};
use tokio::sync::mpsc::UnboundedReceiver;

/// Per-agent aggregated stats for the summary table.
#[derive(Default, Clone)]
pub struct AgentStats {
    /// Number of turns completed.
    pub turns: u32,
    /// Total duration across all turns in milliseconds.
    pub duration_ms: u64,
    /// Total tokens used.
    pub tokens: u64,
}

/// Drain `rx` until the channel closes, printing one line per event.
/// Returns the final aggregated stats keyed by agent id.
pub async fn run_printer(mut rx: UnboundedReceiver<TraceEvent>) -> BTreeMap<String, AgentStats> {
    let mut stats: BTreeMap<String, AgentStats> = BTreeMap::new();

    while let Some(event) = rx.recv().await {
        match &event.kind {
            TraceEventKind::Spawn {
                child_id,
                agent_kind,
                ..
            } => {
                println!(
                    "  \u{25c6} {:<60} spawned",
                    format!("{agent_kind} ({child_id})")
                );
            }
            TraceEventKind::Turn {
                agent_id,
                turn_index,
                duration_ms,
            } => {
                let entry = stats.entry(agent_id.clone()).or_default();
                entry.turns += 1;
                entry.duration_ms += *duration_ms;
                println!(
                    "        Turn {agent_id}: {} ({:.1}s)",
                    turn_index + 1,
                    *duration_ms as f64 / 1000.0
                );
            }
            TraceEventKind::ToolCall {
                tool_name,
                duration_ms,
                status,
            } => {
                let marker = if status == "ok" { "  " } else { "\u{2717} " };
                println!(
                    "        {}Tool {tool_name:<30} {:.1}s",
                    marker,
                    *duration_ms as f64 / 1000.0
                );
            }
            TraceEventKind::TaskMutation { task_id, mutation } => {
                let icon = match mutation.as_str() {
                    "created" => "\u{2514} task created:",
                    "claimed" => "\u{2514} task claimed:",
                    "completed" => "\u{2514} task done:   ",
                    "failed" => "\u{2514} task failed: ",
                    "discarded" => "\u{2514} task discarded:",
                    _ => "\u{2514} task event:  ",
                };
                println!("    {icon} [{task_id}]");
            }
            TraceEventKind::PlanNote { snippet } => {
                println!("        plan: {snippet}");
            }
            TraceEventKind::BudgetWarn {
                budget,
                current,
                limit,
            } => {
                println!("    \u{26a0} budget {budget}: {current} / {limit}");
            }
            TraceEventKind::BudgetExceeded {
                budget,
                final_value,
                limit,
            } => {
                println!("    \u{2717} budget {budget} EXCEEDED: {final_value} > {limit}");
            }
            TraceEventKind::Completion { agent_id, status } => {
                let icon = if status == "completed" {
                    "\u{2713}"
                } else {
                    "\u{2717}"
                };
                let entry = stats.entry(agent_id.clone()).or_default();
                println!(
                    "  {icon} {agent_id:<60} {:.1}s \u{00b7} {} tok",
                    entry.duration_ms as f64 / 1000.0,
                    entry.tokens
                );
            }
            TraceEventKind::Abort { reason } => {
                println!("  \u{2717} aborted: {reason}");
            }
            TraceEventKind::OrphanedTasksAtTermination { task_ids } => {
                println!("  \u{26a0} orphaned tasks: {task_ids:?}");
            }
        }
    }

    stats
}

/// Print the summary table after the run completes.
pub fn print_summary(snapshot: &RunSnapshot, stats: &BTreeMap<String, AgentStats>) {
    println!();
    println!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    println!(
        "  Summary                                          {} tok \u{00b7} {:.1}s",
        snapshot.tokens_used, snapshot.elapsed_secs
    );
    println!();
    println!("      agent             turns    duration    tokens");
    for (agent_id, s) in stats {
        println!(
            "      {:<16}  {:>5}   {:>7.1}s   {:>7}",
            agent_id,
            s.turns,
            s.duration_ms as f64 / 1000.0,
            s.tokens
        );
    }
    println!("  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}");
    println!();
    println!("  run_id: {}", snapshot.run_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tokio::sync::mpsc;

    fn evt(kind: TraceEventKind) -> TraceEvent {
        TraceEvent {
            id: "e".into(),
            ts: Utc::now(),
            run_id: "r".into(),
            agent_id: None,
            kind,
        }
    }

    #[tokio::test]
    async fn printer_drains_events() {
        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(evt(TraceEventKind::Spawn {
            child_id: "agent_x".into(),
            agent_kind: "researcher".into(),
            grant_size: 2,
        }))
        .unwrap();
        tx.send(evt(TraceEventKind::Completion {
            agent_id: "agent_x".into(),
            status: "completed".into(),
        }))
        .unwrap();
        drop(tx);
        let stats = run_printer(rx).await;
        assert!(stats.contains_key("agent_x"));
    }
}
