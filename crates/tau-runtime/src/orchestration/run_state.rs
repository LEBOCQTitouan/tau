//! Per-run mutable state. Implementation lands in Task 6.

use crate::orchestration::{TaskList, TraceStream};

/// Container threaded through every virtual-tool call. Implementation
/// lands in Task 6.
#[allow(missing_docs)]
pub struct RunState {
    pub task_list: TaskList,
    pub plan: String,
    pub trace: TraceStream,
}

impl RunState {
    /// Empty container.
    pub fn new_empty() -> Self {
        Self {
            task_list: TaskList::new(),
            plan: String::new(),
            trace: TraceStream::new(),
        }
    }
}
