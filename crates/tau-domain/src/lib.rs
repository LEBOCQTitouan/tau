#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Core domain types for tau. Pure data — no I/O, no plugin contracts.
//! See the constitution (G5) for why messages are the universal interaction primitive.

pub mod agent;
pub mod error;
pub mod id;
pub mod message;
pub mod package;
pub mod value;
pub mod version;

#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures;

pub use agent::{AgentDefinition, AgentStatus, FailureKind};
pub use error::{
    AgentIdError, PackageKindError, PackageManifestError, PackageNameError, PackageSourceError,
    PluginKindError, PortKindError,
};
pub use id::{AgentId, AgentInstanceId, MessageId, PackageName};
pub use message::{Address, Message, MessagePayload};
pub use package::{
    kinds, AgentCapability, Capability, CapabilityShape, CapabilityShapeSet, FsCapability,
    GitLocation, NetCapability, PackageDep, PackageId, PackageKind, PackageManifest, PackageSource,
    PluginKind, PluginManifest, PluginRequiredTier, PluginSandboxRequirements, PortKind,
    ProcessCapability, UncheckedManifest,
};
pub use crate::package::skill::{
    SkillContent, SkillContentError, SkillFrontmatter, SkillManifest, SKILL_DIR_VAR,
};
#[cfg(feature = "serde")]
pub use crate::package::skill::parse_skill_md;
pub use value::Value;
pub use version::{Version, VersionReq};

// External-crate re-exports for convenience: anything that takes a
// `tau_domain::Url` should accept a `url::Url` from anywhere in the tree.
pub use url::Url;
pub use uuid::Uuid;
