//! Plan-validation rules for per-host network filtering.
//!
//! Rejects:
//! - Wildcard hosts (`*`, `*.example.com`).
//! - IP literals other than `127.0.0.1` (use hostnames; `127.0.0.1` is the
//!   only allowed literal for cross-netns test scenarios).

use super::error::NetFilterError;
use std::net::IpAddr;
use std::str::FromStr;

/// Validate a list of hostnames against the per-host filter rules.
///
/// Returns `Ok(())` if all hosts are acceptable, else the first failure.
pub fn validate_hosts(hosts: &[String]) -> Result<(), NetFilterError> {
    for host in hosts {
        if host == "*" || host.contains('*') {
            return Err(NetFilterError::WildcardForbidden { host: host.clone() });
        }

        // Reject IP literals except 127.0.0.1.
        if let Ok(ip) = IpAddr::from_str(host) {
            if ip != IpAddr::from([127, 0, 0, 1]) {
                return Err(NetFilterError::IpLiteralNotSupported { host: host.clone() });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_hosts_list_ok() {
        assert!(validate_hosts(&[]).is_ok());
    }

    #[test]
    fn hostnames_ok() {
        let hosts = vec!["api.anthropic.com".into(), "api.openai.com".into()];
        assert!(validate_hosts(&hosts).is_ok());
    }

    #[test]
    fn star_wildcard_rejected() {
        let hosts = vec!["*".into()];
        let err = validate_hosts(&hosts).unwrap_err();
        assert!(matches!(err, NetFilterError::WildcardForbidden { .. }));
    }

    #[test]
    fn domain_suffix_wildcard_rejected() {
        let hosts = vec!["*.anthropic.com".into()];
        let err = validate_hosts(&hosts).unwrap_err();
        assert!(
            matches!(err, NetFilterError::WildcardForbidden { host } if host == "*.anthropic.com")
        );
    }

    #[test]
    fn ip_literal_rejected_except_loopback() {
        let hosts = vec!["192.168.1.1".into()];
        let err = validate_hosts(&hosts).unwrap_err();
        assert!(matches!(err, NetFilterError::IpLiteralNotSupported { .. }));
    }

    #[test]
    fn loopback_literal_allowed() {
        let hosts = vec!["127.0.0.1".into()];
        assert!(validate_hosts(&hosts).is_ok());
    }
}
