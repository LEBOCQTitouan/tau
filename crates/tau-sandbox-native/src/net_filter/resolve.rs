//! DNS resolution for per-host network filtering.
//!
//! `resolve_hosts` resolves each hostname in the plan's `Network(Http) { hosts }`
//! list via tokio's async resolver (typically getaddrinfo). Multi-record A+AAAA
//! aware. 5-second timeout (caller-configurable) per the spec's Q3.A decision.
//!
//! Failures surface the hostname for actionable error messages.

use std::collections::HashSet;
use std::net::IpAddr;
use std::str::FromStr;
use std::time::Duration;

use super::error::NetFilterError;

/// Resolve a list of hostnames to a deduplicated set of IPs.
///
/// `127.0.0.1` literal is short-circuited (no DNS lookup).
/// Per-hostname timeout enforced via `tokio::time::timeout`.
#[allow(dead_code)]
pub(super) async fn resolve_hosts(
    hosts: &[String],
    timeout: Duration,
) -> Result<HashSet<IpAddr>, NetFilterError> {
    let mut all_ips: HashSet<IpAddr> = HashSet::new();

    for host in hosts {
        // Short-circuit literal 127.0.0.1; do not perform DNS lookup.
        if let Ok(ip) = IpAddr::from_str(host) {
            all_ips.insert(ip);
            continue;
        }

        // Use port 80 to satisfy lookup_host's SocketAddr requirement; we
        // discard the port from each returned SocketAddr.
        let lookup = tokio::net::lookup_host(format!("{host}:80"));

        let addrs = match tokio::time::timeout(timeout, lookup).await {
            Ok(Ok(iter)) => iter.collect::<Vec<_>>(),
            Ok(Err(io_err)) => {
                return Err(NetFilterError::DnsResolutionFailed {
                    host: host.clone(),
                    source: io_err,
                });
            }
            Err(_elapsed) => {
                return Err(NetFilterError::DnsResolutionFailed {
                    host: host.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("DNS lookup for '{host}' exceeded {:?}", timeout),
                    ),
                });
            }
        };

        if addrs.is_empty() {
            return Err(NetFilterError::DnsNoRecords {
                host: host.clone(),
            });
        }

        for sock_addr in addrs {
            all_ips.insert(sock_addr.ip());
        }
    }

    Ok(all_ips)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_hosts_list_returns_empty_set() {
        let ips = resolve_hosts(&[], Duration::from_secs(5)).await.unwrap();
        assert!(ips.is_empty());
    }

    #[tokio::test]
    async fn loopback_literal_short_circuits() {
        // No real DNS happens; if it did, this might be flaky in offline tests.
        let hosts = vec!["127.0.0.1".to_string()];
        let ips = resolve_hosts(&hosts, Duration::from_secs(1)).await.unwrap();
        assert_eq!(ips.len(), 1);
        assert!(ips.contains(&IpAddr::from([127, 0, 0, 1])));
    }

    #[tokio::test]
    async fn unresolvable_hostname_surfaces_in_error() {
        // `nonexistent.invalid` is reserved by RFC 6761 and guaranteed not to resolve.
        let hosts = vec!["definitely-nonexistent-host-12345.invalid".to_string()];
        let result = resolve_hosts(&hosts, Duration::from_secs(2)).await;
        match result {
            Err(NetFilterError::DnsResolutionFailed { host, .. }) => {
                assert_eq!(host, "definitely-nonexistent-host-12345.invalid");
            }
            Err(other) => panic!("expected DnsResolutionFailed, got {other:?}"),
            Ok(_) => panic!("expected resolution to fail"),
        }
    }

    #[tokio::test]
    async fn multiple_loopback_aliases_dedupe() {
        // 127.0.0.1 listed twice → single entry in the set.
        let hosts = vec!["127.0.0.1".to_string(), "127.0.0.1".to_string()];
        let ips = resolve_hosts(&hosts, Duration::from_secs(1)).await.unwrap();
        assert_eq!(ips.len(), 1);
    }
}
