//! Errors emitted by the framing and codec layers.

use thiserror::Error;

/// Failures from the framing and codec layers.
///
/// `#[non_exhaustive]`: additive variants do not break callers.
///
/// # Example
///
/// ```ignore
/// use tau_plugin_protocol::ProtocolError;
/// let err = ProtocolError::FrameTooLarge { len: 1, max: 0 };
/// assert!(format!("{err}").contains("frame too large"));
/// ```
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// Underlying IO error from the transport.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The receiving side observed end-of-stream while expecting more
    /// bytes. For host-side framers, this typically means the plugin
    /// process exited.
    #[error("frame truncated: expected {expected} more bytes, got EOF")]
    FrameTruncated {
        /// How many more bytes were expected.
        expected: usize,
    },

    /// A frame's length-prefix exceeded the configured max.
    #[error("frame too large: {len} bytes (max {max})")]
    FrameTooLarge {
        /// Reported length from the prefix.
        len: usize,
        /// Configured maximum.
        max: usize,
    },

    /// The frame body failed to decode as MessagePack.
    #[error("body decode failed: {0}")]
    BodyDecodeFailed(#[from] rmp_serde::decode::Error),

    /// Body encoding failed.
    #[error("body encode failed: {0}")]
    BodyEncodeFailed(#[from] rmp_serde::encode::Error),
}
