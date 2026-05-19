//! Tau deployment target identifier (target triple) module.
//!
//! See spec `docs/superpowers/specs/2026-05-19-target-triple-registry-design.md`.

pub mod adapter_family;
pub mod parse;
pub mod platform;
pub mod triple;

pub use adapter_family::AdapterFamily;
pub use parse::ParseError;
pub use platform::Platform;
pub use triple::TargetTriple;
