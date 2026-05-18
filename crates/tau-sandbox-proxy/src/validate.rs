//! Allow-list validation for HTTP CONNECT proxy hosts.
//!
//! Reject:
//! - wildcards (any `*` in the hostname)
//! - IP literals (except 127.0.0.1 / ::1)
//!
//! Carried forward from F's deleted net_filter::validate; semantics unchanged.

use std::net::IpAddr;
use std::str::FromStr;

/// Errors returned by [`validate_hosts`] when a host doesn't pass the
/// CONNECT proxy's allow-list rules.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    /// A host contains a `*` wildcard — disallowed by the proxy.
    #[error("wildcard not allowed in host: {0}")]
    Wildcard(String),
    /// A host is an IP literal that isn't a loopback address.
    #[error("non-loopback IP literal not allowed: {0}")]
    NonLoopbackIp(String),
}

/// Reject any host in the slice that contains a wildcard or is a
/// non-loopback IP literal. Empty slices return `Ok(())`.
pub fn validate_hosts(hosts: &[String]) -> Result<(), ValidationError> {
    for host in hosts {
        if host.contains('*') {
            return Err(ValidationError::Wildcard(host.clone()));
        }
        if let Ok(ip) = IpAddr::from_str(host) {
            if !ip.is_loopback() {
                return Err(ValidationError::NonLoopbackIp(host.clone()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostnames_ok() {
        assert!(validate_hosts(&["api.anthropic.com".into()]).is_ok());
    }

    #[test]
    fn star_wildcard_rejected() {
        assert!(matches!(
            validate_hosts(&["*.example.com".into()]),
            Err(ValidationError::Wildcard(_))
        ));
    }

    #[test]
    fn ip_literal_rejected_except_loopback() {
        assert!(matches!(
            validate_hosts(&["8.8.8.8".into()]),
            Err(ValidationError::NonLoopbackIp(_))
        ));
    }

    #[test]
    fn loopback_literal_allowed() {
        assert!(validate_hosts(&["127.0.0.1".into()]).is_ok());
    }

    #[test]
    fn empty_list_ok() {
        assert!(validate_hosts(&[]).is_ok());
    }

    #[test]
    fn ipv6_loopback_allowed() {
        assert!(validate_hosts(&["::1".into()]).is_ok());
    }

    #[test]
    fn ipv6_non_loopback_rejected() {
        assert!(matches!(
            validate_hosts(&["2606:4700::1".into()]),
            Err(ValidationError::NonLoopbackIp(_))
        ));
    }
}
