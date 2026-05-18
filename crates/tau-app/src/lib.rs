#![deny(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Application orchestration for tau's runtime. Wires ports to adapters.
//!
//! v1 ships the `serve` module: a JSON-RPC 2.0 over NDJSON-framed stdio
//! server exposing `Runtime::run` and `Runtime::run_streaming` as tau's
//! second public API surface (Constitution G6 / QG12).
//!
//! See spec at `docs/superpowers/specs/2026-05-17-tau-serve-mode-design.md`
//! and ADR-0033.
//!
//! Note: `unsafe_code` is `deny` rather than `forbid` because the
//! Linux-only PDEATHSIG setup in `serve::lifecycle::set_pdeathsig`
//! requires `unsafe { libc::prctl(...) }` for `PR_SET_PDEATHSIG`. The
//! single call site is explicitly `#[allow(unsafe_code)]` and
//! documented. No other unsafe code exists in this crate.

pub mod serve;
