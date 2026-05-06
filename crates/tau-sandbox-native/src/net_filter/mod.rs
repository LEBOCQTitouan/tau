//! Per-host network filtering for the strict sandbox tier (sub-project F).
//!
//! Public surface:
//! - `probe_prerequisites()` — runs at adapter init.
//! - `validate_hosts()` — plan-validation gate.
//! - `apply_per_host_filter()` — orchestrator called from `strict.rs::apply_strict`
//!   between unshare and seccomp.
//! - `NetFilterHandle` — cleanup-on-Drop guard for the parent-side veth.
//!
//! Internal modules: error, exec (CommandExecutor), probe, validate, resolve,
//! netns (veth + nsenter), rules (nft ruleset gen + apply), handle.

mod error;
mod exec;
mod handle;
mod netns;
mod probe;
mod resolve;
mod rules;
mod validate;

pub use error::NetFilterError;
pub use handle::NetFilterHandle;
pub use probe::probe_prerequisites;
pub use validate::validate_hosts;

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::time::Duration;

use tau_domain::{Capability, NetCapability};
use tau_ports::SandboxPlan;

/// Default timeout for DNS resolution per the spec's Q3 decision.
const DNS_TIMEOUT: Duration = Duration::from_secs(5);

/// Apply per-host network filtering for plans containing `Capability::Network(Http)`.
///
/// Returns a noop handle if the plan has no `Network(Http)` capability.
///
/// Otherwise, performs (in order):
/// 1. Validate hostnames (no wildcards, no IP literals except 127.0.0.1).
/// 2. Resolve hostnames via DNS (multi-record A+AAAA, 5s timeout).
/// 3. Set up a veth pair on the parent side (host end + IP + up).
/// 4. Move the child end into the child's netns (`ip link set ... netns <pid>`).
/// 5. Configure the child end via `nsenter` (assign IP, bring up, default route).
/// 6. Discover host DNS resolvers + generate the nftables ruleset.
/// 7. Apply the ruleset inside the child netns via `nsenter ... -- nft -f -`.
///
/// Returns a `NetFilterHandle` whose Drop cleans up the parent-side veth.
pub async fn apply_per_host_filter(
    plan: &SandboxPlan,
    child_pid: i32,
) -> Result<NetFilterHandle, NetFilterError> {
    // Extract Network(Http) capabilities. If none, return the noop handle.
    let mut hosts: Vec<String> = Vec::new();
    for cap in &plan.capabilities {
        if let Capability::Network(NetCapability::Http { hosts: h, .. }) = cap {
            hosts.extend(h.iter().cloned());
        }
    }

    if hosts.is_empty() {
        return Ok(NetFilterHandle::noop());
    }

    // 1. Validate.
    validate::validate_hosts(&hosts)?;

    // 2. Resolve.
    let allowed_ips: BTreeSet<IpAddr> = resolve::resolve_hosts(&hosts, DNS_TIMEOUT)
        .await?
        .into_iter()
        .collect();

    // 3. Set up veth pair.
    let exec = exec::RealCommandExecutor;
    let pair = netns::setup_veth_pair(&exec)?;
    // From here, we MUST clean up the host-end veth on any subsequent failure
    // because the NetFilterHandle is constructed only at the end. Use a guard.
    let mut cleanup_guard = ScopedVethCleanup::new(pair.name_host.clone());

    // 4. Move child end into child netns.
    netns::move_peer_to_netns(&exec, &pair, child_pid)?;

    // 5. Configure child end via nsenter.
    netns::assign_child_ip_and_up_via_nsenter(&exec, &pair, child_pid)?;

    // 6. Generate ruleset.
    let dns_servers = rules::discover_dns_servers();
    let ruleset = rules::generate_ruleset(&allowed_ips, &dns_servers);

    // 7. Apply ruleset inside child netns.
    rules::apply_ruleset(&exec, &ruleset, child_pid)?;

    // Success: dismiss the cleanup guard and hand off ownership to the handle.
    cleanup_guard.dismiss();
    Ok(NetFilterHandle::new(
        pair.name_host,
        IpAddr::V4(pair.parent_ip),
    ))
}

/// Scope guard that runs `ip link del <veth>` on Drop unless dismissed.
/// Used inside `apply_per_host_filter` to clean up if any step after
/// veth creation fails.
struct ScopedVethCleanup {
    veth_name: Option<String>,
}

impl ScopedVethCleanup {
    fn new(veth_name: String) -> Self {
        Self {
            veth_name: Some(veth_name),
        }
    }

    fn dismiss(&mut self) {
        self.veth_name = None;
    }
}

impl Drop for ScopedVethCleanup {
    fn drop(&mut self) {
        if let Some(name) = self.veth_name.take() {
            // Bring CommandExecutor into scope so .run() resolves on the
            // RealCommandExecutor instance.
            use exec::CommandExecutor;
            let executor = exec::RealCommandExecutor;
            let _ = executor.run("ip", &["link", "del", &name], None);
        }
    }
}
