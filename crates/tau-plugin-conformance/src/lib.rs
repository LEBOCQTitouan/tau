#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Parameterized conformance test suite for tau `LlmBackend` plugins.
//!
//! Charter (per ADR-0009): tests **mechanical correctness** (request
//! shape, response shape, error typing, stream chunk ordering) NOT
//! response quality (NG7). The suite is conservative: 6 baseline tests
//! at v0.1; extension requires a follow-up.
//!
//! # Usage
//!
//! From a plugin's `tests/conformance.rs`:
//!
//! ```ignore
//! use std::path::Path;
//! use tau_plugin_conformance::ConformanceSuite;
//!
//! #[tokio::test]
//! async fn run_conformance_suite() {
//!     ConformanceSuite::default()
//!         .run(
//!             |base_url| {
//!                 // Build a plugin pointing at the cassette server's URL.
//!                 my_plugin::Plugin::new(base_url)
//!             },
//!             Path::new("tests/conformance-cassettes"),
//!         )
//!         .await;
//! }
//! ```
//!
//! Each test reads `cassettes_dir/<test_name>.yaml`:
//! - `batch_happy_path.yaml`
//! - `batch_with_tools.yaml`
//! - `streaming_text.yaml`
//! - `streaming_tool_use.yaml`
//! - `error_rate_limited.yaml`
//! - `error_auth.yaml`
//!
//! See `docs/superpowers/specs/2026-04-29-openai-plugin-design.md`
//! §8.3.

use std::path::Path;

use tau_ports::LlmBackend;

mod cases;

/// Parameterized conformance suite. The catalog at v0.1 is fixed
/// (6 tests); future versions may gain configurable knobs (skip
/// individual cases, extend the catalog) — hence `#[non_exhaustive]`.
#[non_exhaustive]
#[derive(Debug, Default)]
pub struct ConformanceSuite {}

impl ConformanceSuite {
    /// Run the full battery against a plugin built by `build_plugin`.
    ///
    /// `build_plugin` is a closure that takes the per-test cassette
    /// server's URL and returns a fresh plugin instance pointed at it.
    /// The closure is called once per test (a fresh server is spawned
    /// for each test from the cassette file at
    /// `cassettes_dir/<test_name>.yaml`).
    ///
    /// Panics on the first assertion failure with a descriptive
    /// message including the test name. The caller's `#[tokio::test]`
    /// surface fails accordingly.
    pub async fn run<B, F>(&self, build_plugin: F, cassettes_dir: &Path)
    where
        B: LlmBackend,
        F: Fn(String) -> B + Send + Sync,
    {
        cases::batch_happy_path::run(&build_plugin, cassettes_dir).await;
        cases::batch_with_tools::run(&build_plugin, cassettes_dir).await;
        cases::streaming_text::run(&build_plugin, cassettes_dir).await;
        cases::streaming_tool_use::run(&build_plugin, cassettes_dir).await;
        cases::error_rate_limited::run(&build_plugin, cassettes_dir).await;
        cases::error_auth::run(&build_plugin, cassettes_dir).await;
    }
}
