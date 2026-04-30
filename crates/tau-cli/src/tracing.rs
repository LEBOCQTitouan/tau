//! Tracing-subscriber configuration for tau-cli.
//!
//! Per spec §3.7: stderr-targeted subscriber, default level INFO scoped
//! to `tau=*`, verbosity flags promote (-v: DEBUG, -vv: TRACE), --quiet
//! demotes to WARN, --debug behaves as -v plus expanded error chain at
//! print time, RUST_LOG overrides everything (standard env_logger
//! convention).

use tracing_subscriber::filter::EnvFilter;

use crate::cli::Cli;

/// Compute the `EnvFilter` from CLI flags + `RUST_LOG` env.
///
/// Resolution order:
/// 1. `RUST_LOG` (if set) — used verbatim, overrides flags.
/// 2. `--verbose` count >= 2 → `"tau=trace"`.
/// 3. `--debug` OR `--verbose` count >= 1 → `"tau=debug"`.
/// 4. `--quiet` → `"tau=warn"`.
/// 5. Default → `"tau=info"`.
///
/// The default scopes to `tau=*` so plugin tracing doesn't flood unless
/// the user explicitly opts in via `RUST_LOG`.
pub fn build_filter(cli: &Cli) -> EnvFilter {
    if let Ok(env) = std::env::var("RUST_LOG") {
        return EnvFilter::new(env);
    }

    let level = if cli.verbose >= 2 {
        "trace"
    } else if cli.debug || cli.verbose >= 1 {
        "debug"
    } else if cli.quiet {
        "warn"
    } else {
        "info"
    };

    EnvFilter::new(format!("tau={level}"))
}

/// Install the global tracing subscriber. Must be called exactly once
/// per process; subsequent calls panic via `init()`.
///
/// Writes to stderr in the standard `tracing_subscriber::fmt` human-
/// readable format (timestamp + level + target + fields + message).
pub fn install(cli: &Cli) {
    let filter = build_filter(cli);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, ColorMode, Command, ListArgs, ListResource};

    fn make_cli(verbose: u8, quiet: bool, debug: bool) -> Cli {
        Cli {
            command: Command::List(ListArgs {
                resource: ListResource::Packages,
                global: false,
                all: false,
                capabilities: false,
                dry_run: false,
            }),
            verbose,
            quiet,
            debug,
            color: ColorMode::Auto,
            json: false,
            record_protocol: None,
        }
    }

    /// Mutex-guarded `RUST_LOG` mutation across tests since unit tests in the
    /// same binary share env state. Each test that touches RUST_LOG must
    /// take this lock to serialize.
    fn rust_log_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|p| p.into_inner())
    }

    #[test]
    fn build_filter_default_is_info() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(0, false, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=info"), "got: {filter}");
    }

    #[test]
    fn build_filter_minus_v_is_debug() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(1, false, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=debug"), "got: {filter}");
    }

    #[test]
    fn build_filter_minus_vv_is_trace() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(2, false, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=trace"), "got: {filter}");
    }

    #[test]
    fn build_filter_quiet_is_warn() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(0, true, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=warn"), "got: {filter}");
    }

    #[test]
    fn build_filter_debug_is_debug() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(0, false, true);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=debug"), "got: {filter}");
    }

    #[test]
    fn build_filter_rust_log_overrides_flags() {
        let _g = rust_log_lock();
        std::env::set_var("RUST_LOG", "my_plugin=trace");
        let cli = make_cli(0, true, false); // would be warn without RUST_LOG
        let filter = build_filter(&cli);
        // RUST_LOG verbatim — no `tau=` scope
        assert!(
            filter.to_string().contains("my_plugin=trace"),
            "got: {filter}"
        );
        std::env::remove_var("RUST_LOG");
    }

    #[test]
    fn build_filter_scopes_to_tau_when_no_rust_log() {
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(0, false, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().starts_with("tau="), "got: {filter}");
    }

    #[test]
    fn build_filter_minus_vv_takes_precedence_over_minus_v_logic() {
        // Sanity: order should check >= 2 first; otherwise -vv would
        // collapse to -v (a known spec self-review fix).
        let _g = rust_log_lock();
        std::env::remove_var("RUST_LOG");
        let cli = make_cli(2, false, false);
        let filter = build_filter(&cli);
        assert!(filter.to_string().contains("tau=trace"));
        assert!(!filter.to_string().contains("tau=debug"));
    }
}
