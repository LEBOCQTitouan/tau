//! Runtime sandbox glue — declarative-requirements resolver + plan validation.

pub mod passthrough;
mod plan;
pub mod registry;
pub mod resolution_error;
pub mod resolver;
mod validation;

pub use plan::build_plan;
pub use resolution_error::{ResolutionError, ResolutionRejection};
pub use resolver::{resolve_adapter, resolve_adapter_forced, SandboxAdapter};
pub use validation::{validate_plan_against_adapter, SandboxValidationError};
