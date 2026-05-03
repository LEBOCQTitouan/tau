//! Runtime sandbox glue — adapter chain selection + plan validation.

mod chain;
mod plan;
mod validation;

pub use chain::{select_adapter, SandboxAdapter, SandboxChainError};
pub use plan::build_plan;
pub use validation::{validate_plan_against_adapter, SandboxValidationError};
