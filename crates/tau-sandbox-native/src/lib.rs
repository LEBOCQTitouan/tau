//! Linux native sandbox adapter for tau.
//!
//! Implements [`tau_ports::Sandbox`] using:
//! - **landlock** (kernel 5.13+) for filesystem path isolation,
//! - **seccompiler** for syscall filtering (Strict tier — Task 4),
//! - **nix unshare** for user/network namespaces (Strict tier — Task 5).
//!
//! On non-Linux hosts the adapter exists but `probe()` returns
//! `SandboxProbe::Unavailable` and all other methods return
//! `SandboxError::Unavailable`.

#![deny(missing_docs)]

mod shape;

#[cfg(target_os = "linux")]
mod exec;
#[cfg(target_os = "linux")]
mod light;
#[cfg(target_os = "linux")]
mod net;
#[cfg(target_os = "linux")]
mod net_filter;
#[cfg(target_os = "linux")]
mod probe;
#[cfg(target_os = "linux")]
mod strict;

#[cfg(not(target_os = "linux"))]
mod stub;

use std::process::Command;

use tau_domain::CapabilityShapeSet;
use tau_ports::{Sandbox, SandboxError, SandboxHandle, SandboxPlan, SandboxProbe, SandboxTier};

/// Linux native sandbox adapter. Probe-driven: at construction time the
/// adapter is inert; calling [`Sandbox::probe`] discovers what the host
/// kernel can offer and the runtime caches the result.
pub struct NativeSandbox {
    name: String,
    // Used in #[cfg(target_os = "linux")] branches; suppress dead_code on other platforms.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    requested_tier: SandboxTier,

    /// F task 6.5: cached result of net_filter::probe_prerequisites().
    /// Lazy-initialized on first call to validate_plan for a Network(Http) plan.
    #[cfg(target_os = "linux")]
    net_filter_probe_cached:
        std::sync::OnceLock<Result<(), crate::net_filter::NetFilterError>>,

    /// F task 6.5: per-spawn map from SandboxHandle::sync_write_fd_value()
    /// to the pre-allocated VethSubnet. wrap_spawn inserts; apply_post_spawn
    /// looks up + removes.
    #[cfg(target_os = "linux")]
    veth_subnets: std::sync::Mutex<
        std::collections::HashMap<std::os::fd::RawFd, crate::net_filter::netns::VethSubnet>,
    >,
}

impl NativeSandbox {
    /// Construct an adapter that will deliver up to the given tier. The
    /// effective tier is `min(requested_tier, probe_tier)`.
    pub fn new(name: impl Into<String>, requested_tier: SandboxTier) -> Self {
        Self {
            name: name.into(),
            requested_tier,
            #[cfg(target_os = "linux")]
            net_filter_probe_cached: std::sync::OnceLock::new(),
            #[cfg(target_os = "linux")]
            veth_subnets: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Return (and cache) the result of net_filter::probe_prerequisites().
    #[cfg(target_os = "linux")]
    fn cached_net_filter_probe(&self) -> &Result<(), crate::net_filter::NetFilterError> {
        self.net_filter_probe_cached
            .get_or_init(crate::net_filter::probe_prerequisites)
    }
}

impl Sandbox for NativeSandbox {
    fn name(&self) -> &str {
        &self.name
    }

    async fn probe(&self) -> SandboxProbe {
        #[cfg(target_os = "linux")]
        {
            probe::probe(self.requested_tier).await
        }
        #[cfg(not(target_os = "linux"))]
        {
            stub::unavailable_probe()
        }
    }

    fn supported_shapes(&self) -> CapabilityShapeSet {
        #[cfg(target_os = "linux")]
        {
            shape::shapes_for_tier(self.requested_tier)
        }
        #[cfg(not(target_os = "linux"))]
        {
            CapabilityShapeSet::new()
        }
    }

    fn validate_plan(&self, plan: &SandboxPlan) -> Result<(), SandboxError> {
        let supported = self.supported_shapes();
        if supported.is_empty() {
            return Err(SandboxError::Unavailable {
                reason: "tau-sandbox-native requires Linux".into(),
            });
        }
        for cap in &plan.capabilities {
            let shape = cap.required_shape();
            if !supported.contains(&shape) {
                return Err(SandboxError::ShapeUnsupported { shape });
            }
        }

        #[cfg(target_os = "linux")]
        {
            let has_network_http = plan.capabilities.iter().any(|c| {
                matches!(
                    c,
                    tau_domain::Capability::Network(tau_domain::NetCapability::Http { .. })
                )
            });
            if has_network_http {
                // F task 6.5: hard-refuse Network(Http) plans on F-unavailable hosts.
                if let Err(probe_err) = self.cached_net_filter_probe() {
                    return Err(SandboxError::NetFilter {
                        message: format!(
                            "plan requires Network(Http) but net-filter prereq missing: {probe_err}"
                        ),
                    });
                }
                // Validate hostnames syntactically.
                let mut hosts: Vec<String> = Vec::new();
                for c in &plan.capabilities {
                    if let tau_domain::Capability::Network(
                        tau_domain::NetCapability::Http { hosts: h, .. },
                    ) = c
                    {
                        hosts.extend(h.iter().cloned());
                    }
                }
                crate::net_filter::validate_hosts(&hosts).map_err(|e| {
                    SandboxError::NetFilter {
                        message: e.to_string(),
                    }
                })?;
            }
        }

        Ok(())
    }

    async fn apply_post_spawn(
        &self,
        plan: &SandboxPlan,
        child_pid: i32,
        handle: &mut SandboxHandle,
    ) -> Result<(), SandboxError> {
        #[cfg(target_os = "linux")]
        {
            let has_network_http = plan.capabilities.iter().any(|c| {
                matches!(
                    c,
                    tau_domain::Capability::Network(tau_domain::NetCapability::Http { .. })
                )
            });
            if has_network_http {
                // Look up the pre-allocated subnet by sync_write_fd.
                let fd = handle.sync_write_fd_value().ok_or_else(|| {
                    SandboxError::NetFilter {
                        message: "no sync_write_fd on SandboxHandle (Network(Http) plan)"
                            .to_string(),
                    }
                })?;
                let subnet = self
                    .veth_subnets
                    .lock()
                    .expect("mutex")
                    .remove(&fd)
                    .ok_or_else(|| SandboxError::NetFilter {
                        message: "no pre-allocated subnet for handle".to_string(),
                    })?;

                let nf_handle =
                    crate::net_filter::apply_per_host_filter(plan, child_pid, subnet)
                        .await
                        .map_err(|e| SandboxError::NetFilter {
                            message: e.to_string(),
                        })?;

                handle.nest_handle(Box::new(nf_handle));
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (plan, child_pid, handle);
        Ok(())
    }

    async fn wrap_spawn(
        &self,
        plan: &SandboxPlan,
        cmd: &mut Command,
    ) -> Result<SandboxHandle, SandboxError> {
        self.validate_plan(plan)?;
        #[cfg(target_os = "linux")]
        {
            match self.requested_tier {
                SandboxTier::Light => light::apply_landlock(plan, cmd),
                SandboxTier::Strict => {
                    let (handle, veth_subnet) = strict::apply_strict(plan, cmd)?;
                    // F task 6.5: stash the subnet keyed by the handle's sync_write_fd.
                    // apply_post_spawn looks it up and removes it.
                    if let (Some(fd), Some(subnet)) =
                        (handle.sync_write_fd_value(), veth_subnet)
                    {
                        self.veth_subnets
                            .lock()
                            .expect("mutex")
                            .insert(fd, subnet);
                    }
                    Ok(handle)
                }
                SandboxTier::None => Ok(SandboxHandle::noop()),
                other => Err(SandboxError::Unsupported {
                    what: format!("tier {other:?} not implemented"),
                }),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (plan, cmd);
            Err(SandboxError::Unavailable {
                reason: "tau-sandbox-native requires Linux".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tau_domain::fixtures as domain_fixtures;
    #[cfg(target_os = "linux")]
    use tau_domain::CapabilityShape;
    use tau_ports::fixtures as ports_fixtures;

    #[test]
    fn name_and_tier_round_trip() {
        let s = NativeSandbox::new("native-light", SandboxTier::Light);
        assert_eq!(s.name(), "native-light");
    }

    #[test]
    fn supported_shapes_light_includes_fs() {
        let s = NativeSandbox::new("n", SandboxTier::Light);
        let supported = s.supported_shapes();
        #[cfg(target_os = "linux")]
        {
            assert!(supported.contains(&CapabilityShape::FilesystemRead));
            assert!(supported.contains(&CapabilityShape::FilesystemWrite));
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert!(supported.is_empty());
        }
    }

    #[test]
    fn validate_plan_rejects_unsupported_shape_at_light_tier() {
        let s = NativeSandbox::new("n", SandboxTier::Light);
        let plan =
            ports_fixtures::plan_from_capabilities(vec![domain_fixtures::cap_custom("weird")]);
        let err = s.validate_plan(&plan).expect_err("must reject");
        #[cfg(target_os = "linux")]
        assert!(matches!(err, SandboxError::ShapeUnsupported { .. }));
        #[cfg(not(target_os = "linux"))]
        assert!(matches!(err, SandboxError::Unavailable { .. }));
    }

    #[tokio::test]
    async fn probe_on_non_linux_is_unavailable() {
        #[cfg(not(target_os = "linux"))]
        {
            let s = NativeSandbox::new("n", SandboxTier::Light);
            let p = s.probe().await;
            assert!(matches!(p, SandboxProbe::Unavailable { .. }));
        }
    }

    #[test]
    fn validate_plan_unavailable_on_non_linux() {
        #[cfg(not(target_os = "linux"))]
        {
            let s = NativeSandbox::new("n", SandboxTier::Light);
            let plan =
                ports_fixtures::plan_from_capabilities(vec![domain_fixtures::cap_fs_read(&[
                    "/tmp",
                ])]);
            assert!(matches!(
                s.validate_plan(&plan),
                Err(SandboxError::Unavailable { .. })
            ));
        }
    }

    #[test]
    fn shapes_strict_tier_includes_exec_and_net() {
        let s = NativeSandbox::new("n", SandboxTier::Strict);
        let supported = s.supported_shapes();
        #[cfg(target_os = "linux")]
        {
            assert!(supported.contains(&CapabilityShape::ProcessExec));
            assert!(supported.contains(&CapabilityShape::NetworkHttp));
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert!(supported.is_empty());
        }
    }
}
