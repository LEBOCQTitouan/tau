//! Package metadata types (sources, manifests, capabilities).

pub mod capability;
pub mod manifest;
pub mod source;

pub use capability::{AgentCapability, Capability, FsCapability, NetCapability, ProcessCapability};
pub use manifest::{kinds, PackageDep, PackageId, PackageKind, PackageManifest, UncheckedManifest};
pub use source::{GitLocation, PackageSource};
