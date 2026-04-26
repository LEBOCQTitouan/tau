//! Package metadata types (sources, manifests, capabilities).

pub mod manifest;
pub mod source;

pub use manifest::{PackageDep, PackageId};
pub use source::{GitLocation, PackageSource};
