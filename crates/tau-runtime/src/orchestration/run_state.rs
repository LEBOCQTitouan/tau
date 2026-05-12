//! Per-run mutable state container.
//!
//! Holds the TaskList, plan/notes scratchpad, TraceStream, and budget
//! counters. Threaded through every virtual-tool call. One RunState
//! exists per Run; the runtime kernel owns it for the run's lifetime.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use chrono::{DateTime, Utc};
use tau_ports::{RunBudget, RunId, RunStatus};

use crate::orchestration::{TaskList, TraceStream};

/// Per-run mutable state.
pub struct RunState {
    /// Run id.
    pub run_id: RunId,
    /// Root agent id.
    pub root_agent_id: tau_ports::AgentId,
    /// Task list.
    pub task_list: TaskList,
    /// Free-form scratchpad (run.note / run.plan).
    pub plan: String,
    /// Trace fan-out.
    pub trace: TraceStream,
    /// Immutable budget config.
    pub budget: RunBudget,
    /// Cumulative tokens used (atomic for cross-await updates).
    pub tokens_used: AtomicU64,
    /// Cumulative agents spawned.
    pub agents_spawned: AtomicU32,
    /// Current status.
    pub status: RunStatus,
    /// Started at.
    pub started_at: DateTime<Utc>,
    /// Ended at.
    pub ended_at: Option<DateTime<Utc>>,
}

impl RunState {
    /// New running RunState.
    pub fn new(
        run_id: RunId,
        root_agent_id: tau_ports::AgentId,
        budget: RunBudget,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            run_id,
            root_agent_id,
            task_list: TaskList::new(),
            plan: String::new(),
            trace: TraceStream::new(),
            budget,
            tokens_used: AtomicU64::new(0),
            agents_spawned: AtomicU32::new(0),
            status: RunStatus::Running,
            started_at: now,
            ended_at: None,
        }
    }

    /// Append to the plan scratchpad with a trailing newline.
    pub fn append_plan_note(&mut self, text: &str) {
        if !self.plan.is_empty() && !self.plan.ends_with('\n') {
            self.plan.push('\n');
        }
        self.plan.push_str(text);
        if !text.ends_with('\n') {
            self.plan.push('\n');
        }
    }

    /// Add tokens to the cumulative counter.
    pub fn add_tokens(&self, n: u64) {
        self.tokens_used.fetch_add(n, Ordering::Relaxed);
    }

    /// Increment the agent-spawn counter.
    pub fn record_agent_spawn(&self) {
        self.agents_spawned.fetch_add(1, Ordering::Relaxed);
    }

    /// Read-only snapshot.
    pub fn snapshot(&self, now: DateTime<Utc>) -> tau_ports::RunSnapshot {
        tau_ports::RunSnapshot {
            run_id: self.run_id.clone(),
            root_agent_id: self.root_agent_id.clone(),
            task_list: self.task_list.all(),
            plan: self.plan.clone(),
            budget: self.budget.clone(),
            tokens_used: self.tokens_used.load(Ordering::Relaxed),
            elapsed_secs: (now - self.started_at).num_seconds().max(0) as u64,
            agents_spawned: self.agents_spawned.load(Ordering::Relaxed),
            status: self.status,
            started_at: self.started_at,
            ended_at: self.ended_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_at(secs: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(secs, 0).unwrap()
    }

    #[test]
    fn append_plan_note_normalizes_newlines() {
        let mut rs = RunState::new("r".into(), "a".into(), RunBudget::default(), now_at(0));
        rs.append_plan_note("first");
        rs.append_plan_note("second");
        assert_eq!(rs.plan, "first\nsecond\n");
    }

    #[test]
    fn snapshot_reflects_current_state() {
        let rs = RunState::new("r".into(), "a".into(), RunBudget::default(), now_at(0));
        rs.add_tokens(500);
        rs.record_agent_spawn();
        let snap = rs.snapshot(now_at(60));
        assert_eq!(snap.tokens_used, 500);
        assert_eq!(snap.agents_spawned, 1);
        assert_eq!(snap.elapsed_secs, 60);
        assert_eq!(snap.status, RunStatus::Running);
    }
}
