//! Package metadata types (sources, manifests, capabilities).

pub mod capability;
pub mod manifest;
pub mod plugin;
pub mod sandbox;
pub mod skill;
pub mod source;

pub use capability::{
    AgentCapability, Capability, CapabilityShape, CapabilityShapeSet, FsCapability, NetCapability,
    ProcessCapability, SkillCapability,
};
pub use manifest::{kinds, PackageDep, PackageId, PackageKind, PackageManifest, UncheckedManifest};
pub use plugin::{PluginKind, PluginManifest, PortKind};
pub use sandbox::{PluginRequiredTier, PluginSandboxRequirements};
pub use source::{GitLocation, PackageSource};
