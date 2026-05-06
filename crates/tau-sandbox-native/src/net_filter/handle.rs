//! NetFilterHandle: cleanup-on-Drop for the parent end of the veth pair.
//!
//! The child end disappears when the child netns is destroyed (typically
//! when the plugin process exits and its netns refcount drops to zero).
//! Only the parent end needs explicit cleanup.

use std::net::IpAddr;

use super::exec::{CommandExecutor, RealCommandExecutor};

/// Handle returned by `apply_per_host_filter`. Drop removes the parent veth.
///
/// `#[non_exhaustive]` blocks struct-literal construction outside this crate;
/// always go through `apply_per_host_filter`.
#[non_exhaustive]
pub struct NetFilterHandle {
    veth_name_host: String,
    parent_ip: IpAddr,
    cleaned_up: bool,
}

impl NetFilterHandle {
    pub(super) fn new(veth_name_host: String, parent_ip: IpAddr) -> Self {
        Self {
            veth_name_host,
            parent_ip,
            cleaned_up: false,
        }
    }

    /// Sentinel handle for plans without `Network(Http)` — no veth was created,
    /// nothing to clean up.
    pub(super) fn noop() -> Self {
        Self {
            veth_name_host: String::new(),
            parent_ip: IpAddr::from([0u8, 0, 0, 0]),
            cleaned_up: true, // never run cleanup
        }
    }

    /// Parent-side IP address of the veth pair. Tests reaching cross-netns
    /// services bind on this IP and read it via `TAU_NET_PARENT_VETH_IP`.
    pub fn parent_ip(&self) -> IpAddr {
        self.parent_ip
    }

    /// Whether this handle is the noop sentinel (no veth to clean up).
    pub fn is_noop(&self) -> bool {
        self.veth_name_host.is_empty()
    }

    pub(super) fn cleanup(&mut self) {
        if self.cleaned_up || self.veth_name_host.is_empty() {
            return;
        }
        self.cleaned_up = true;
        let exec = RealCommandExecutor;
        match exec.run("ip", &["link", "del", &self.veth_name_host], None) {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                tracing::warn!(
                    veth = %self.veth_name_host,
                    stderr = %String::from_utf8_lossy(&out.stderr),
                    "ip link del failed (interface leaked)"
                );
            }
            Err(e) => {
                tracing::warn!(
                    veth = %self.veth_name_host,
                    error = %e,
                    "ip link del invocation failed (interface leaked)"
                );
            }
        }
    }
}

impl Drop for NetFilterHandle {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_handle_has_empty_veth_name() {
        let h = NetFilterHandle::noop();
        assert!(h.is_noop());
        assert_eq!(h.parent_ip(), IpAddr::from([0u8, 0, 0, 0]));
    }

    #[test]
    fn noop_handle_drop_does_nothing() {
        // Just verify Drop doesn't panic. Real cleanup is exercised in
        // integration tests where a real veth pair exists.
        let h = NetFilterHandle::noop();
        drop(h);
    }

    #[test]
    fn cleanup_idempotent() {
        let mut h = NetFilterHandle::noop();
        h.cleanup();
        h.cleanup(); // second call is a no-op (cleaned_up flag)
    }

    #[test]
    fn parent_ip_accessor_returns_constructor_value() {
        let h = NetFilterHandle::new("tsb1-0h".to_string(), IpAddr::from([10u8, 222, 1, 1]));
        assert_eq!(h.parent_ip(), IpAddr::from([10u8, 222, 1, 1]));
        // Don't drop; that would call ip link del on a non-existent interface.
        std::mem::forget(h);
    }
}
