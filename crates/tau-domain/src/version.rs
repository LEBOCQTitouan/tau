//! Re-exports of the semver types used in package metadata.
//!
//! tau-domain does not wrap `semver::Version` or `semver::VersionReq`. The
//! Cargo / SemVer ecosystem already speaks these types; wrapping would add
//! ceremony without enforcing any invariant tau-domain currently needs.
//!
//! If a future ADR motivates normalization (e.g. forbidding pre-release
//! tags or build metadata), it lands as a wrapper newtype at that point.

pub use semver::{Version, VersionReq};
