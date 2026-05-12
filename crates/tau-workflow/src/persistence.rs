//! Append-only JSONL persistence for workflow runs.
//!
//! One line per step completion. The run log file lives at
//! `<scope>/.tau/workflow-runs/<workflow-name>-<run-id>.jsonl`.
//! Lines are fsync'd after each write so a crash mid-write loses at
//! most the trailing partial line; replay tolerates that.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One step's completion record, serialized as a single JSONL line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepRecord {
    /// Log-line timestamp (record-emit time).
    pub ts: DateTime<Utc>,
    /// ULID of the run this record belongs to.
    pub run_id: String,
    /// Step id as declared in the workflow TOML.
    pub step_id: String,
    /// Zero-based index of the step in the workflow.
    pub step_index: usize,
    /// `"agent.run"` or `"tool.call"`.
    pub kind: String,
    /// Resolved input string passed to the step.
    pub input: String,
    /// Output text captured from the step.
    pub output: String,
    /// Wall-clock start of the step.
    pub started_at: DateTime<Utc>,
    /// Wall-clock end of the step.
    pub ended_at: DateTime<Utc>,
    /// Duration in milliseconds (`ended_at - started_at`).
    pub duration_ms: u64,
    /// `"ok"` or `"failed"`.
    pub status: StepStatus,
    /// On `status = "failed"`, an opaque error class for matching.
    /// `None` on `status = "ok"`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
    /// On `status = "failed"`, a human-readable detail line.
    /// `None` on `status = "ok"`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<String>,
}

/// Status of a step in a run log.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    /// Step completed successfully.
    Ok,
    /// Step terminated abnormally. Run aborted.
    Failed,
}

/// Builds the canonical run-log path:
/// `<scope_root>/.tau/workflow-runs/<workflow_name>-<run_id>.jsonl`.
pub fn run_log_path(scope_root: &std::path::Path, workflow_name: &str, run_id: &str) -> PathBuf {
    scope_root
        .join(".tau")
        .join("workflow-runs")
        .join(format!("{workflow_name}-{run_id}.jsonl"))
}
