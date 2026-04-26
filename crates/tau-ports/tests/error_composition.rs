//! Integration tests for `?` propagation through
//! [`ToolError::Llm(LlmError)`] and [`ToolError::Storage(StorageError)`]
//! via `#[from]`.
//!
//! Asserts that:
//! - A function returning `Result<_, ToolError>` can `?`-propagate an
//!   inner [`LlmError`] yielding `ToolError::Llm(_)`.
//! - The same for [`StorageError`] yielding `ToolError::Storage(_)`.
//! - The `is_retryable()` predicate delegates correctly to the inner
//!   error's predicate for both composed variants.
//!
//! Gated behind the `test-fixtures` feature for suite-wide consistency.

#![cfg(feature = "test-fixtures")]

use tau_ports::error::{LlmError, StorageError, ToolError};

/// `?` on an `Err(LlmError)` inside a `Result<_, ToolError>` produces
/// a `ToolError::Llm(_)` with the inner error preserved.
#[test]
fn question_mark_propagates_llm_error() {
    fn op() -> Result<(), ToolError> {
        Err(LlmError::Transport {
            message: "tcp reset".into(),
        })?;
        Ok(())
    }

    let err = op().expect_err("should propagate");
    assert_eq!(
        err,
        ToolError::Llm(LlmError::Transport {
            message: "tcp reset".into(),
        }),
    );
    // is_retryable delegates: Transport is retryable, so the wrapped
    // ToolError is too.
    assert!(err.is_retryable());
}

/// `?` on an `Err(StorageError)` inside a `Result<_, ToolError>`
/// produces a `ToolError::Storage(_)` with the inner error preserved.
#[test]
fn question_mark_propagates_storage_error() {
    fn op() -> Result<(), ToolError> {
        Err(StorageError::Timeout)?;
        Ok(())
    }

    let err = op().expect_err("should propagate");
    assert_eq!(err, ToolError::Storage(StorageError::Timeout));
    // is_retryable delegates: Timeout is retryable.
    assert!(err.is_retryable());
}

/// Non-retryable inner errors yield non-retryable composed errors.
#[test]
fn is_retryable_delegates_non_retryable_paths() {
    let llm_auth: ToolError = LlmError::Auth {
        message: "no key".into(),
    }
    .into();
    assert!(!llm_auth.is_retryable());

    let storage_invalid: ToolError = StorageError::InvalidKey {
        reason: "bad".into(),
    }
    .into();
    assert!(!storage_invalid.is_retryable());
}

/// `From<LlmError> for ToolError` and `From<StorageError> for ToolError`
/// compose with `?` across multiple call frames (one error happens
/// deeper in the stack).
#[test]
fn composition_across_frames() {
    fn inner_llm() -> Result<u32, LlmError> {
        Err(LlmError::Provider {
            message: "5xx".into(),
        })
    }

    fn outer_tool() -> Result<u32, ToolError> {
        let n = inner_llm()?;
        Ok(n)
    }

    let err = outer_tool().expect_err("should propagate");
    assert!(matches!(err, ToolError::Llm(LlmError::Provider { .. })));
    assert!(err.is_retryable());

    fn inner_storage() -> Result<(), StorageError> {
        Err(StorageError::Unavailable {
            message: "offline".into(),
        })
    }

    fn outer_tool_storage() -> Result<(), ToolError> {
        inner_storage()?;
        Ok(())
    }

    let err = outer_tool_storage().expect_err("should propagate");
    assert!(matches!(
        err,
        ToolError::Storage(StorageError::Unavailable { .. }),
    ));
    assert!(err.is_retryable());
}
