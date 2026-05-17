//! Tau serve mode: JSON-RPC 2.0 over NDJSON-framed stdio.
//!
//! Public entry point: [`run`]. Builds a `Runtime` from
//! [`ServeOptions::project_path`], spawns the reader/dispatcher/writer
//! tasks, and blocks until shutdown.

mod cancel;
mod dispatch;
mod dispatch_run;
mod error_codes;
mod error_map;
mod framing;
mod handshake;
mod lifecycle;
mod methods;
mod options;
mod project;
mod protocol;
mod tracing_init;

pub use options::ServeOptions;

use anyhow::Result;

/// Run the serve loop until shutdown.
///
/// Builds the runtime, starts the I/O tasks, and blocks. Returns
/// `Ok(())` on graceful shutdown; returns `Err` on startup failure.
pub async fn run(opts: ServeOptions) -> Result<()> {
    lifecycle::run(opts).await
}
