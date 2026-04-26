#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! Core domain types for tau. Pure data — no I/O, no plugin contracts.
//! See the constitution (G5) for why messages are the universal interaction primitive.

pub mod error;
pub mod id;
pub mod value;

pub use error::{AgentIdError, PackageNameError};
pub use id::{AgentId, AgentInstanceId, MessageId, PackageName};
pub use value::Value;
