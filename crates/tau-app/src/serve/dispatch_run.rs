//! Per-request executor for runtime.run and runtime.run_streaming.
//!
//! Task 10 leaves this as a stub. Task 11 fills in execute_batch
//! and execute_streaming.

use super::dispatch::Dispatcher;
use super::error_codes;
use super::protocol::Request;

pub async fn execute(disp: Dispatcher, req: Request, _streaming: bool) {
    disp.send_err(
        req.id,
        error_codes::INTERNAL_ERROR,
        "runtime.run executor not yet implemented (Task 11)".into(),
        None,
    )
    .await;
}
