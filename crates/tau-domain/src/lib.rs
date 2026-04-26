#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Core domain types for tau. Pure data — no I/O, no plugin contracts.
//! See the constitution (G5) for why messages are the universal interaction primitive.

pub mod error;
pub mod id;
pub mod package;
pub mod value;
pub mod version;

pub use error::{AgentIdError, PackageKindError, PackageNameError, PackageSourceError};
pub use id::{AgentId, AgentInstanceId, MessageId, PackageName};
pub use package::{
    kinds, AgentCapability, Capability, FsCapability, GitLocation, NetCapability, PackageDep,
    PackageId, PackageKind, PackageManifest, PackageSource, ProcessCapability, UncheckedManifest,
};
pub use value::Value;
pub use version::{Version, VersionReq};

// External-crate re-exports for convenience: anything that takes a
// `tau_domain::Url` should accept a `url::Url` from anywhere in the tree.
pub use url::Url;
pub use uuid::Uuid;
