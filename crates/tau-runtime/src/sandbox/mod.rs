//! Runtime sandbox glue — adapter chain selection + plan validation.

mod chain;
pub mod passthrough;
mod plan;
pub mod registry;
pub mod resolution_error;
mod validation;

pub use chain::{select_adapter, SandboxAdapter, SandboxChainError};
pub use plan::build_plan;
pub use resolution_error::{ResolutionError, ResolutionRejection};
pub use validation::{validate_plan_against_adapter, SandboxValidationError};
