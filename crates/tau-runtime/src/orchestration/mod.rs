//! Multi-agent orchestration primitives.
//!
//! Implements the v1 primitive set defined in
//! `docs/superpowers/specs/2026-05-12-multi-agent-orchestration-design.md`
//! and ratified in `docs/decisions/0023-multi-agent-orchestration.md`:
//!
//! - [`task_list`] — `TaskList` state with atomic CAS lock + lease + heartbeat.
//! - [`trace`] — `TraceStream` with mpsc fan-out subscribers.
//! - [`run_state`] — per-run mutable state (`RunState`).
//! - [`virtual_tools`] — resolver intercepting `task.*`, `run.*`, and
//!   `agent.<kind>.spawn` before plugin dispatch.
//! - [`budget`] — budget breach detection.
//! - [`persistence`] — JSONL run-log writer.
//! - [`error`] — typed errors.
//!
//! The kernel entry point is `Runtime::spawn_root_agent` (declared in
//! `crate::run`).

pub mod budget;
pub mod error;
pub mod persistence;
pub mod run_state;
pub mod task_list;
pub mod trace;
pub mod virtual_tools;

pub use budget::BudgetWatchdog;
pub use error::OrchestrationError;
pub use run_state::RunState;
pub use task_list::TaskList;
pub use trace::{TraceStream, TraceSubscriber};
pub use virtual_tools::{
    check_capability_subset, dispatch, is_virtual, required_capability, validate_agent_spawn,
    AgentSpawnRequest,
};
