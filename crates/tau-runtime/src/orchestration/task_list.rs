//! TaskList state with hierarchical task ids + atomic claim CAS + lease + heartbeat.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use tau_ports::{AgentId, Task, TaskEvent, TaskId, TaskListFilter, TaskStatus};

use crate::orchestration::error::OrchestrationError;

/// Default lease duration: 5 minutes.
pub const DEFAULT_LEASE: Duration = Duration::minutes(5);

/// In-memory task list. Lookups are O(1) by id; iteration is over tasks in
/// insertion order.
#[derive(Debug, Default)]
pub struct TaskList {
    by_id: HashMap<TaskId, Task>,
    /// Preserves the order tasks were created in.
    order: Vec<TaskId>,
    /// Per-scope counters for id allocation. Key is `None` for top-level,
    /// `Some(parent_id)` for children. Each scope gets its own monotonic counter
    /// so child ids are independent of top-level or sibling-scope numbering.
    scope_seq: HashMap<Option<TaskId>, u32>,
}

impl TaskList {
    /// Empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a task. If `parent_task_id` is `Some(p)`, the new id is
    /// `<p>.<seq>`; otherwise it's a top-level `<seq>` (zero-padded to 2).
    ///
    /// Sequence counters are per-scope (top-level vs each parent id) so that:
    /// - First top-level task → "01"
    /// - Second top-level task → "02"
    /// - First child of "01" → "01.01"
    pub fn create(
        &mut self,
        description: String,
        created_by: AgentId,
        parent_task_id: Option<TaskId>,
        owner: Option<AgentId>,
        now: DateTime<Utc>,
    ) -> Result<TaskId, OrchestrationError> {
        let scope = parent_task_id.clone();
        let seq = self.scope_seq.entry(scope).or_insert(0);
        *seq += 1;
        let n = *seq;

        let id = if let Some(ref parent) = parent_task_id {
            format!("{parent}.{n:02}")
        } else {
            format!("{n:02}")
        };

        let lease_expires_at = if owner.is_some() {
            Some(now + DEFAULT_LEASE)
        } else {
            None
        };

        let task = Task {
            id: id.clone(),
            description,
            parent_task_id,
            created_by: created_by.clone(),
            owner: owner.clone(),
            lease_expires_at,
            status: if owner.is_some() {
                TaskStatus::Claimed
            } else {
                TaskStatus::Pending
            },
            result_summary: None,
            error: None,
            events: vec![TaskEvent {
                ts: now,
                by: Some(created_by),
                kind: "created".into(),
                detail: None,
            }],
        };

        self.by_id.insert(id.clone(), task);
        self.order.push(id.clone());
        Ok(id)
    }

    /// Atomic compare-and-set: claim the task IF unclaimed OR lease expired.
    /// On success: sets owner + extends lease by DEFAULT_LEASE.
    /// On failure: returns `TaskLocked { by, until }`.
    pub fn claim(
        &mut self,
        task_id: &TaskId,
        agent: AgentId,
        now: DateTime<Utc>,
    ) -> Result<(), OrchestrationError> {
        let t = self.by_id.get_mut(task_id).ok_or_else(|| {
            OrchestrationError::TaskNotFound { task: task_id.clone() }
        })?;

        // Reject terminal states.
        if matches!(t.status, TaskStatus::Done | TaskStatus::Failed | TaskStatus::Discarded) {
            return Err(OrchestrationError::InvalidTaskTransition {
                task: task_id.clone(),
                target: "claimed".into(),
            });
        }

        // CAS: fail if currently owned AND lease has not expired.
        if let (Some(by), Some(until)) = (t.owner.clone(), t.lease_expires_at) {
            if until > now {
                return Err(OrchestrationError::TaskLocked {
                    task: task_id.clone(),
                    by,
                    until: until.to_rfc3339(),
                });
            }
        }

        t.owner = Some(agent.clone());
        t.lease_expires_at = Some(now + DEFAULT_LEASE);
        t.status = TaskStatus::Claimed;
        t.events.push(TaskEvent {
            ts: now,
            by: Some(agent),
            kind: "claimed".into(),
            detail: None,
        });
        Ok(())
    }

    /// Owner-only: extend the lease by DEFAULT_LEASE.
    pub fn heartbeat(
        &mut self,
        task_id: &TaskId,
        agent: &AgentId,
        now: DateTime<Utc>,
    ) -> Result<(), OrchestrationError> {
        let t = self.by_id.get_mut(task_id).ok_or_else(|| {
            OrchestrationError::TaskNotFound { task: task_id.clone() }
        })?;

        if t.owner.as_ref() != Some(agent) {
            return Err(OrchestrationError::NotTaskOwner {
                task: task_id.clone(),
                agent: agent.clone(),
            });
        }

        t.lease_expires_at = Some(now + DEFAULT_LEASE);
        t.events.push(TaskEvent {
            ts: now,
            by: Some(agent.clone()),
            kind: "heartbeat".into(),
            detail: None,
        });
        Ok(())
    }

    /// Owner-only: release the lock without completing. Status → Pending.
    pub fn release(
        &mut self,
        task_id: &TaskId,
        agent: &AgentId,
        now: DateTime<Utc>,
    ) -> Result<(), OrchestrationError> {
        let t = self.by_id.get_mut(task_id).ok_or_else(|| {
            OrchestrationError::TaskNotFound { task: task_id.clone() }
        })?;

        if t.owner.as_ref() != Some(agent) {
            return Err(OrchestrationError::NotTaskOwner {
                task: task_id.clone(),
                agent: agent.clone(),
            });
        }

        t.owner = None;
        t.lease_expires_at = None;
        t.status = TaskStatus::Pending;
        t.events.push(TaskEvent {
            ts: now,
            by: Some(agent.clone()),
            kind: "released".into(),
            detail: None,
        });
        Ok(())
    }

    /// Owner-only: set status + append optional notes.
    /// Only `TaskStatus::InProgress` is accepted as a manual status transition;
    /// terminal states go through `complete` / `fail` / `release`.
    pub fn update(
        &mut self,
        task_id: &TaskId,
        agent: &AgentId,
        new_status: Option<TaskStatus>,
        notes: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<(), OrchestrationError> {
        let t = self.by_id.get_mut(task_id).ok_or_else(|| {
            OrchestrationError::TaskNotFound { task: task_id.clone() }
        })?;

        if t.owner.as_ref() != Some(agent) {
            return Err(OrchestrationError::NotTaskOwner {
                task: task_id.clone(),
                agent: agent.clone(),
            });
        }

        if let Some(s) = new_status {
            // Only InProgress is a valid manual status transition.
            if !matches!(s, TaskStatus::InProgress) {
                return Err(OrchestrationError::InvalidTaskTransition {
                    task: task_id.clone(),
                    target: format!("{s:?}"),
                });
            }
            t.status = s;
        }

        t.events.push(TaskEvent {
            ts: now,
            by: Some(agent.clone()),
            kind: "updated".into(),
            detail: notes,
        });
        Ok(())
    }

    /// Owner-only: finalize as Done with a result.
    pub fn complete(
        &mut self,
        task_id: &TaskId,
        agent: &AgentId,
        result_summary: String,
        now: DateTime<Utc>,
    ) -> Result<(), OrchestrationError> {
        let t = self.by_id.get_mut(task_id).ok_or_else(|| {
            OrchestrationError::TaskNotFound { task: task_id.clone() }
        })?;

        if t.owner.as_ref() != Some(agent) {
            return Err(OrchestrationError::NotTaskOwner {
                task: task_id.clone(),
                agent: agent.clone(),
            });
        }

        t.status = TaskStatus::Done;
        t.result_summary = Some(result_summary.clone());
        t.owner = None;
        t.lease_expires_at = None;
        t.events.push(TaskEvent {
            ts: now,
            by: Some(agent.clone()),
            kind: "completed".into(),
            detail: Some(result_summary),
        });
        Ok(())
    }

    /// Owner-only: finalize as Failed with an error.
    pub fn fail(
        &mut self,
        task_id: &TaskId,
        agent: &AgentId,
        error: String,
        now: DateTime<Utc>,
    ) -> Result<(), OrchestrationError> {
        let t = self.by_id.get_mut(task_id).ok_or_else(|| {
            OrchestrationError::TaskNotFound { task: task_id.clone() }
        })?;

        if t.owner.as_ref() != Some(agent) {
            return Err(OrchestrationError::NotTaskOwner {
                task: task_id.clone(),
                agent: agent.clone(),
            });
        }

        t.status = TaskStatus::Failed;
        t.error = Some(error.clone());
        t.owner = None;
        t.lease_expires_at = None;
        t.events.push(TaskEvent {
            ts: now,
            by: Some(agent.clone()),
            kind: "failed".into(),
            detail: Some(error),
        });
        Ok(())
    }

    /// Any agent (with proper cap): mark as Discarded (orphan acceptance).
    /// No owner check — the orchestrator can discard any task.
    pub fn discard(
        &mut self,
        task_id: &TaskId,
        agent: &AgentId,
        reason: String,
        now: DateTime<Utc>,
    ) -> Result<(), OrchestrationError> {
        let t = self.by_id.get_mut(task_id).ok_or_else(|| {
            OrchestrationError::TaskNotFound { task: task_id.clone() }
        })?;

        t.status = TaskStatus::Discarded;
        t.owner = None;
        t.lease_expires_at = None;
        t.events.push(TaskEvent {
            ts: now,
            by: Some(agent.clone()),
            kind: "discarded".into(),
            detail: Some(reason),
        });
        Ok(())
    }

    /// Read: filter tasks by criteria (AND-combined).
    pub fn list(&self, filter: &TaskListFilter) -> Vec<Task> {
        self.order
            .iter()
            .filter_map(|id| self.by_id.get(id))
            .filter(|t| filter.status.is_none() || filter.status == Some(t.status))
            .filter(|t| {
                filter
                    .owner
                    .as_ref()
                    .map_or(true, |o| t.owner.as_ref() == Some(o))
            })
            .filter(|t| {
                filter
                    .parent
                    .as_ref()
                    .map_or(true, |p| t.parent_task_id.as_ref() == Some(p))
            })
            .filter(|t| !filter.unclaimed_only || t.owner.is_none())
            .cloned()
            .collect()
    }

    /// Read: get one task by id.
    pub fn get(&self, task_id: &TaskId) -> Option<&Task> {
        self.by_id.get(task_id)
    }

    /// Sweep: expire any lease whose `lease_expires_at < now`. Returns the
    /// ids whose owners were dropped.
    pub fn expire_leases(&mut self, now: DateTime<Utc>) -> Vec<TaskId> {
        let mut expired = Vec::new();
        for id in &self.order {
            if let Some(t) = self.by_id.get_mut(id) {
                if let Some(until) = t.lease_expires_at {
                    if until < now
                        && t.owner.is_some()
                        && matches!(t.status, TaskStatus::Claimed | TaskStatus::InProgress)
                    {
                        t.owner = None;
                        t.lease_expires_at = None;
                        t.status = TaskStatus::Pending;
                        t.events.push(TaskEvent {
                            ts: now,
                            by: None,
                            kind: "lease_expired".into(),
                            detail: None,
                        });
                        expired.push(id.clone());
                    }
                }
            }
        }
        expired
    }

    /// All tasks in creation order (for snapshots).
    pub fn all(&self) -> Vec<Task> {
        self.order
            .iter()
            .filter_map(|id| self.by_id.get(id).cloned())
            .collect()
    }

    /// True iff every task is in a terminal state (Done | Failed | Discarded).
    pub fn all_terminal(&self) -> bool {
        self.by_id.values().all(|t| {
            matches!(
                t.status,
                TaskStatus::Done | TaskStatus::Failed | TaskStatus::Discarded
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_at(secs: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(secs, 0).unwrap()
    }

    #[test]
    fn create_assigns_top_level_id() {
        let mut tl = TaskList::new();
        let id = tl
            .create("first".into(), "agent_a".into(), None, None, now_at(0))
            .unwrap();
        assert_eq!(id, "01");
        let id2 = tl
            .create("second".into(), "agent_a".into(), None, None, now_at(0))
            .unwrap();
        assert_eq!(id2, "02");
    }

    #[test]
    fn create_assigns_hierarchical_id() {
        let mut tl = TaskList::new();
        let p = tl
            .create("parent".into(), "a".into(), None, None, now_at(0))
            .unwrap();
        let c = tl
            .create("child".into(), "a".into(), Some(p.clone()), None, now_at(0))
            .unwrap();
        assert_eq!(c, "01.01");
    }

    #[test]
    fn claim_succeeds_when_unclaimed() {
        let mut tl = TaskList::new();
        let id = tl
            .create("t".into(), "a".into(), None, None, now_at(0))
            .unwrap();
        tl.claim(&id, "worker_1".into(), now_at(0)).unwrap();
        let t = tl.get(&id).unwrap();
        assert_eq!(t.owner.as_deref(), Some("worker_1"));
        assert_eq!(t.status, TaskStatus::Claimed);
        assert!(t.lease_expires_at.unwrap() > now_at(0));
    }

    #[test]
    fn claim_fails_when_locked() {
        let mut tl = TaskList::new();
        let id = tl
            .create("t".into(), "a".into(), None, None, now_at(0))
            .unwrap();
        tl.claim(&id, "worker_1".into(), now_at(0)).unwrap();
        let err = tl
            .claim(&id, "worker_2".into(), now_at(60))
            .unwrap_err();
        assert!(matches!(err, OrchestrationError::TaskLocked { .. }));
    }

    #[test]
    fn claim_succeeds_after_lease_expiry() {
        let mut tl = TaskList::new();
        let id = tl
            .create("t".into(), "a".into(), None, None, now_at(0))
            .unwrap();
        tl.claim(&id, "worker_1".into(), now_at(0)).unwrap();
        // Default lease is 5 min = 300s. Try claim from another worker at 400s.
        tl.claim(&id, "worker_2".into(), now_at(400)).unwrap();
        let t = tl.get(&id).unwrap();
        assert_eq!(t.owner.as_deref(), Some("worker_2"));
    }

    #[test]
    fn heartbeat_extends_lease() {
        let mut tl = TaskList::new();
        let id = tl
            .create("t".into(), "a".into(), None, None, now_at(0))
            .unwrap();
        tl.claim(&id, "w".into(), now_at(0)).unwrap();
        let initial_lease = tl.get(&id).unwrap().lease_expires_at.unwrap();
        tl.heartbeat(&id, &"w".into(), now_at(200)).unwrap();
        let extended = tl.get(&id).unwrap().lease_expires_at.unwrap();
        assert!(extended > initial_lease);
    }

    #[test]
    fn heartbeat_rejects_non_owner() {
        let mut tl = TaskList::new();
        let id = tl
            .create("t".into(), "a".into(), None, None, now_at(0))
            .unwrap();
        tl.claim(&id, "w1".into(), now_at(0)).unwrap();
        let err = tl
            .heartbeat(&id, &"w2".into(), now_at(60))
            .unwrap_err();
        assert!(matches!(err, OrchestrationError::NotTaskOwner { .. }));
    }

    #[test]
    fn complete_transitions_to_done_and_clears_lock() {
        let mut tl = TaskList::new();
        let id = tl
            .create("t".into(), "a".into(), None, None, now_at(0))
            .unwrap();
        tl.claim(&id, "w".into(), now_at(0)).unwrap();
        tl.complete(&id, &"w".into(), "did it".into(), now_at(30))
            .unwrap();
        let t = tl.get(&id).unwrap();
        assert_eq!(t.status, TaskStatus::Done);
        assert_eq!(t.owner, None);
        assert_eq!(t.result_summary.as_deref(), Some("did it"));
    }

    #[test]
    fn list_filters_by_status() {
        let mut tl = TaskList::new();
        let a = tl
            .create("a".into(), "x".into(), None, None, now_at(0))
            .unwrap();
        let _b = tl
            .create("b".into(), "x".into(), None, None, now_at(0))
            .unwrap();
        tl.claim(&a, "w".into(), now_at(0)).unwrap();
        tl.complete(&a, &"w".into(), "ok".into(), now_at(10)).unwrap();
        let done = tl.list(&TaskListFilter {
            status: Some(TaskStatus::Done),
            ..Default::default()
        });
        assert_eq!(done.len(), 1);
        let pending = tl.list(&TaskListFilter {
            status: Some(TaskStatus::Pending),
            ..Default::default()
        });
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn expire_leases_drops_owners() {
        let mut tl = TaskList::new();
        let id = tl
            .create("t".into(), "a".into(), None, None, now_at(0))
            .unwrap();
        tl.claim(&id, "w".into(), now_at(0)).unwrap();
        let expired = tl.expire_leases(now_at(400)); // 400s > 300s default lease
        assert_eq!(expired, vec![id.clone()]);
        let t = tl.get(&id).unwrap();
        assert!(t.owner.is_none());
        assert_eq!(t.status, TaskStatus::Pending);
    }

    #[test]
    fn all_terminal_true_only_when_every_task_terminal() {
        let mut tl = TaskList::new();
        let a = tl
            .create("a".into(), "x".into(), None, None, now_at(0))
            .unwrap();
        let b = tl
            .create("b".into(), "x".into(), None, None, now_at(0))
            .unwrap();
        assert!(!tl.all_terminal());
        tl.claim(&a, "w".into(), now_at(0)).unwrap();
        tl.complete(&a, &"w".into(), "ok".into(), now_at(10)).unwrap();
        assert!(!tl.all_terminal());
        tl.claim(&b, "w".into(), now_at(20)).unwrap();
        tl.fail(&b, &"w".into(), "nope".into(), now_at(30)).unwrap();
        assert!(tl.all_terminal());
    }
}
