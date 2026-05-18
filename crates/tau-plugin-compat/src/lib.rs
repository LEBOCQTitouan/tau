#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Plugin compatibility verification harness — sub-project B (2026-05-04).
//!
//! This crate is test infrastructure only (`publish = false`). It exists
//! because Task 1 of sub-project B established that all 5 real shipped
//! plugins (anthropic, ollama, openai, fs-read, shell) declare
//! `[sandbox] required_tier = "strict"`, and we need automated tests that
//! verify each plugin actually works end-to-end under real sandbox
//! enforcement (not just `MockSandbox`-based theory).
//!
//! # Layout
//!
//! - `fixtures/controlled-env-binary/` — standalone Cargo project (NOT a
//!   workspace member; built on demand by Task 8's tests via
//!   `cargo build --manifest-path …`). Statically-linked test binary that
//!   performs predictable I/O so landlock V1 path resolution doesn't trip
//!   on dynamic linker probing or `/bin → /usr/bin` symlinks.
//! - `fixtures/projects/<plugin>/` — per-plugin tau project fixture
//!   (Task 5). Each contains a minimal `tau.toml` + `.tau/config.toml`
//!   declaring `[sandbox] required_tier = "strict"` plus any cassette /
//!   data files needed.
//! - `tests/layer3_check_sandbox.rs` (Task 6) — 5 tests; install plugin
//!   into tempdir, run `tau resolve --check-sandbox`, assert exit 0.
//! - `tests/layer4_container.rs` (Task 7) — 5 tests; force Container
//!   adapter, drive each plugin's golden path.
//! - `tests/layer4_native.rs` (Task 8) — 5 tests; force Native adapter,
//!   exercise Task 3's symlink-resolution fix. Linux-only.
//!
//! # Helper functions
//!
//! Helpers added in Tasks 6-8 will live alongside this doc-comment.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod driver;
pub mod startup_io;
