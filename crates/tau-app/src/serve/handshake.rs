//! Handshake state machine.
//!
//! Per spec §5.1: `meta.handshake` is required as the first call.
//! Non-`meta.*` calls before handshake → -32002. Double-handshake → -32003.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

/// Server's view of handshake state. Single atomic so any task can
/// check/transition without locks.
#[derive(Debug, Default, Clone)]
pub struct HandshakeState {
    state: Arc<AtomicU8>,
}

const STATE_HANDSHAKEN: u8 = 1;

/// Outcome of checking a method against current handshake state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// Method is allowed; proceed with dispatch.
    Allowed,
    /// Pre-handshake call to a non-meta method. Reject with -32002.
    HandshakeRequired,
    /// `meta.handshake` called twice. Reject with -32003.
    AlreadyHandshaken,
}

impl HandshakeState {
    /// Check whether a method is allowed in the current state.
    ///
    /// For `meta.handshake` itself, this returns `AlreadyHandshaken`
    /// when already handshaken; the caller should transition only
    /// when this returns `Allowed`.
    pub fn check(&self, method: &str) -> Check {
        let is_handshake_method = method == super::methods::META_HANDSHAKE;
        let is_meta = method.starts_with("meta.");
        let handshaken = self.state.load(Ordering::Acquire) == STATE_HANDSHAKEN;

        match (handshaken, is_handshake_method, is_meta) {
            (true, true, _) => Check::AlreadyHandshaken,
            (false, false, false) => Check::HandshakeRequired,
            (false, false, true) => Check::Allowed, // meta.ping pre-handshake is allowed
            (_, true, _) => Check::Allowed,
            (true, false, _) => Check::Allowed,
        }
    }

    /// Mark the handshake as complete. Idempotent — calling after
    /// already-handshaken is a no-op (caller should check first).
    pub fn mark_handshaken(&self) {
        self.state.store(STATE_HANDSHAKEN, Ordering::Release);
    }

    /// Whether the handshake has completed.
    pub fn is_handshaken(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_HANDSHAKEN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_handshake_meta_ping_allowed() {
        let s = HandshakeState::default();
        assert_eq!(s.check("meta.ping"), Check::Allowed);
    }

    #[test]
    fn pre_handshake_runtime_run_rejected() {
        let s = HandshakeState::default();
        assert_eq!(s.check("runtime.run"), Check::HandshakeRequired);
    }

    #[test]
    fn handshake_method_allowed_first_time() {
        let s = HandshakeState::default();
        assert_eq!(s.check("meta.handshake"), Check::Allowed);
    }

    #[test]
    fn second_handshake_rejected() {
        let s = HandshakeState::default();
        assert_eq!(s.check("meta.handshake"), Check::Allowed);
        s.mark_handshaken();
        assert_eq!(s.check("meta.handshake"), Check::AlreadyHandshaken);
    }

    #[test]
    fn post_handshake_runtime_run_allowed() {
        let s = HandshakeState::default();
        s.mark_handshaken();
        assert_eq!(s.check("runtime.run"), Check::Allowed);
    }
}
