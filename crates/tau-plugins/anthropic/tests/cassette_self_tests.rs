//! Driver test binary that pulls in the shared `common` module so its
//! `#[cfg(test)] mod self_tests` (cassette replayer self-tests) get
//! compiled and run.
//!
//! Tasks 11/12 add `tests/complete.rs` and `tests/streaming.rs`, both of
//! which `mod common;` for the helpers — once those exist, this driver
//! is still useful as a focused entrypoint for the cassette-replayer
//! self-tests alone.

mod common;
