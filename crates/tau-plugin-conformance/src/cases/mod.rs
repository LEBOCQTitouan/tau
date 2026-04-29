//! Per-test case implementations.
//!
//! Each case is `pub(crate) async fn run<B: LlmBackend, F: Fn(String) -> B>(
//!     build_plugin: &F,
//!     cassettes_dir: &Path,
//! )`. The case spawns the cassette replayer, builds a plugin pointed at
//! it, and asserts on behavior. Panics on assertion failure (the
//! caller's `#[tokio::test]` fails accordingly).

pub(crate) mod batch_happy_path;
pub(crate) mod batch_with_tools;
pub(crate) mod error_auth;
pub(crate) mod error_rate_limited;
pub(crate) mod streaming_text;
pub(crate) mod streaming_tool_use;
