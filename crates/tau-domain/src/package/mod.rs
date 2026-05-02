//! Package metadata types (sources, manifests, capabilities).

pub mod capability;
pub mod manifest;
pub mod plugin;
pub mod source;

pub use capability::{
    AgentCapability, Capability, CapabilityShape, CapabilityShapeSet, FsCapability, NetCapability,
    ProcessCapability,
};
pub use manifest::{kinds, PackageDep, PackageId, PackageKind, PackageManifest, UncheckedManifest};
pub use plugin::{PluginKind, PluginManifest, PortKind};
pub use source::{GitLocation, PackageSource};
