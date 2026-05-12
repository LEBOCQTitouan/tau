//! Linear pipeline runner for tau agentic workflows.
//!
//! See `docs/superpowers/specs/2026-05-12-tau-workflow-design.md` for the
//! design + format. v1 supports linear sequential workflows defined under
//! `workflows/*.toml`, with step kinds `agent.run` and `tool.call`.
//! Append-only JSONL persistence under `<scope>/.tau/workflow-runs/`
//! enables `--resume` with strict drift checking.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod model;
pub mod persistence;
pub mod template;

pub use error::WorkflowError;
pub use model::{Step, StepKind, Workflow};
pub use persistence::{run_log_path, StepRecord, StepStatus};
pub use template::resolve as resolve_template;
