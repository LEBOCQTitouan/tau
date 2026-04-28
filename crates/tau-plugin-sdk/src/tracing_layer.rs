//! tracing-subscriber JSON layer that writes structured events to
//! stderr. The host (in `tau-runtime::plugin_host`) reads each line,
//! decodes the JSON, and re-emits as a `tracing::Event` on
//! `target = "plugin::<plugin_name>"`.

use std::sync::Once;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

static INSTALL_ONCE: Once = Once::new();

/// Install the SDK's stderr-JSON tracing layer.
///
/// Idempotent: subsequent calls are no-ops. Plugin authors should
/// call this once at the start of `main()`, OR call one of the
/// `run_*` runners (which install it internally).
///
/// The default filter level is `info`; override via `RUST_LOG` env var
/// (e.g., `RUST_LOG=tau_plugin_sdk=debug,my_plugin=trace`).
pub fn install() {
    INSTALL_ONCE.call_once(|| {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
        tracing_subscriber::registry()
            .with(filter)
            .with(
                fmt::layer()
                    .json()
                    .with_writer(std::io::stderr)
                    .with_current_span(true)
                    .with_span_list(false),
            )
            .init();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_is_idempotent() {
        // Call twice; second call should be a no-op (no panic).
        install();
        install();
    }
}
