//! Runtime sandbox glue — adapter chain selection + plan validation.

mod chain;

pub use chain::{select_adapter, SandboxAdapter, SandboxChainError};
