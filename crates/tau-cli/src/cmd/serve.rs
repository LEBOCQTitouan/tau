//! `tau serve` — start serve mode (JSON-RPC over stdio).
//!
//! Thin wrapper around [`tau_app::serve::run`]. See ADR-0031 (forthcoming)
//! and `docs/superpowers/specs/2026-05-17-tau-serve-mode-design.md`.
//!
//! # Async vs sync
//!
//! `tau-cli` dispatches all subcommands via the `tokio::main` multi-thread
//! runtime. `tau_app::serve::run` works fine on that runtime, so we simply
//! `.await` it directly — no `LocalSet` or current-thread wrapper needed.

use std::time::Duration;

use anyhow::Result;
use tau_app::serve::ServeOptions;

use crate::cli::ServeArgs;

/// Run `tau serve`.
pub async fn run(args: &ServeArgs) -> Result<()> {
    let mut opts = ServeOptions::default();
    if let Some(p) = &args.project {
        opts.project_path = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
    }
    if let Some(n) = args.max_concurrent {
        opts.max_concurrent = n;
    }
    opts.idle_timeout = args.idle_timeout.map(Duration::from_secs);
    opts.ready_on_stderr = args.ready_on_stderr;
    opts.shutdown_grace = Duration::from_secs(args.shutdown_grace);

    tau_app::serve::run(opts).await
}
