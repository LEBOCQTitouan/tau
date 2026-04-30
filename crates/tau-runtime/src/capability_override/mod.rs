//! Capability override — narrows a package manifest's grants under a
//! project tau.toml `[agents.<id>.capabilities]` table.
//!
//! Realizes ADR-0007 §4 reservation. The override never widens; the
//! parse-time and runtime checks both fail closed.
//!
//! See `docs/superpowers/specs/2026-04-30-capability-override-design.md`.

pub(crate) mod glob_subset;
