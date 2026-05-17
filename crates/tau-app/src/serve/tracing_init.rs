//! Tracing subscriber configured to write to stderr only.
//!
//! stdout is reserved for the JSON-RPC protocol. Any tracing/logging
//! sent to stdout would corrupt the protocol stream.

use tracing_subscriber::{fmt, EnvFilter};

/// Install a global tracing subscriber writing to stderr. Honors
/// `RUST_LOG`. Idempotent — safe to call multiple times.
pub fn install() {
    let _ = fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .try_init();
}
