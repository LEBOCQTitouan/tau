//! Per-host network filtering for the strict sandbox tier (sub-project F).
//!
//! Replaces the v0.1 over-permissive netns-inheritance fallback in
//! `crate::net::unshare_flags_for_plan` with real per-host egress filtering:
//!
//! 1. Probe prerequisites at adapter init: nft + ip + nsenter binaries +
//!    CAP_NET_ADMIN-in-userns. Hard-refuse on miss.
//! 2. Validate the plan's `Network(Http) { hosts }` list: no wildcards,
//!    no IP literals (except 127.0.0.1).
//! 3. Resolve hostnames to IPs via tokio DNS (multi-record A+AAAA, 5s timeout).
//! 4. Set up a veth pair between parent and child (fresh netns) via `ip`.
//! 5. Apply nftables ruleset in child netns via `nsenter --net=... -- nft -f -`.
//! 6. Return a `NetFilterHandle` whose `Drop` removes the parent veth.

mod error;
mod exec;
mod probe;
mod validate;
// Submodules for Tasks 3-6 (added incrementally):
// mod resolve;
// mod netns;
// mod rules;
// mod handle;

pub use error::NetFilterError;
pub use probe::probe_prerequisites;
pub use validate::validate_hosts;

// Public API surface (filled in by Tasks 3-6):
//
// pub async fn apply_per_host_filter(
//     plan: &SandboxPlan,
//     child_pid: i32,
// ) -> Result<NetFilterHandle, NetFilterError>;
//
// pub struct NetFilterHandle { /* ... */ }
// impl NetFilterHandle { pub fn parent_ip(&self) -> std::net::IpAddr { /* ... */ } }
