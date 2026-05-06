//! nft ruleset generation + apply.
//!
//! `generate_ruleset` emits a deterministic `inet tau_sandbox` table with
//! an `output` chain: default `drop`, with `accept` rules for established
//! connections + per-IP allowlist + DNS resolvers.
//!
//! `apply_ruleset` shells out via `nsenter --net=/proc/<pid>/ns/net -- nft -f -`
//! with the ruleset text on stdin.
//!
//! `discover_dns_servers` parses /etc/resolv.conf for nameserver lines.

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::path::Path;

use super::error::NetFilterError;
use super::exec::CommandExecutor;

/// Generate a deterministic nft ruleset. Iteration order is stable because
/// the input is a `BTreeSet<IpAddr>`.
#[allow(dead_code)]
pub(super) fn generate_ruleset(allowed_ips: &BTreeSet<IpAddr>, dns_servers: &[IpAddr]) -> String {
    let mut lines: Vec<String> = vec![
        "table inet tau_sandbox {".to_string(),
        "    chain output {".to_string(),
        "        type filter hook output priority 0; policy drop;".to_string(),
        "        ct state established,related accept".to_string(),
    ];

    for ip in allowed_ips {
        match ip {
            IpAddr::V4(v4) => lines.push(format!("        ip daddr {} accept", v4)),
            IpAddr::V6(v6) => lines.push(format!("        ip6 daddr {} accept", v6)),
        }
    }

    for ip in dns_servers {
        match ip {
            IpAddr::V4(v4) => lines.push(format!("        udp dport 53 ip daddr {} accept", v4)),
            IpAddr::V6(v6) => lines.push(format!("        udp dport 53 ip6 daddr {} accept", v6)),
        }
    }

    lines.extend(["    }".to_string(), "}".to_string(), String::new()]);
    lines.join("\n")
}

/// Apply a generated ruleset inside the child's netns via nsenter+nft.
#[allow(dead_code)]
pub(super) fn apply_ruleset(
    exec: &dyn CommandExecutor,
    ruleset_text: &str,
    child_pid: i32,
) -> Result<(), NetFilterError> {
    let netns_arg = format!("--net=/proc/{child_pid}/ns/net");
    let out = exec
        .run(
            "nsenter",
            &[&netns_arg, "nft", "-f", "-"],
            Some(ruleset_text),
        )
        .map_err(|e| NetFilterError::NetnsSetup {
            context: "nsenter nft -f -",
            source: e,
        })?;
    if !out.status.success() {
        return Err(NetFilterError::NftApplyFailed {
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    Ok(())
}

/// Parse `/etc/resolv.conf` for `nameserver <ip>` lines.
/// Returns the parsed IPs in file-order; falls back to `[1.1.1.1, 8.8.8.8]`
/// if the file is missing, unreadable, or contains no parseable nameservers.
#[allow(dead_code)]
pub(super) fn discover_dns_servers() -> Vec<IpAddr> {
    discover_dns_servers_from_path(Path::new("/etc/resolv.conf"))
}

fn discover_dns_servers_from_path(path: &Path) -> Vec<IpAddr> {
    let fallback = || vec![IpAddr::from([1u8, 1, 1, 1]), IpAddr::from([8u8, 8, 8, 8])];

    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return fallback(),
    };

    let parsed: Vec<IpAddr> = content
        .lines()
        .filter_map(|line| {
            let line = line.split('#').next()?.trim();
            let mut parts = line.split_whitespace();
            if parts.next()? != "nameserver" {
                return None;
            }
            parts.next()?.parse::<IpAddr>().ok()
        })
        .collect();

    if parsed.is_empty() {
        fallback()
    } else {
        parsed
    }
}

#[cfg(test)]
mod tests {
    use super::super::exec::test_support::{CannedOutput, MockCommandExecutor};
    use super::*;

    fn ipv4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
        IpAddr::from([a, b, c, d])
    }

    fn ipv6(addr: &str) -> IpAddr {
        addr.parse().unwrap()
    }

    #[test]
    fn generate_ruleset_with_no_allowed_ips_only_has_ct_state_and_dns() {
        let ips: BTreeSet<IpAddr> = BTreeSet::new();
        let dns = vec![ipv4(1, 1, 1, 1), ipv4(8, 8, 8, 8)];
        let ruleset = generate_ruleset(&ips, &dns);
        insta::assert_snapshot!(ruleset);
    }

    #[test]
    fn generate_ruleset_with_ipv4_only() {
        let mut ips: BTreeSet<IpAddr> = BTreeSet::new();
        ips.insert(ipv4(93, 184, 216, 34));
        ips.insert(ipv4(140, 82, 121, 4));
        let dns = vec![ipv4(1, 1, 1, 1)];
        let ruleset = generate_ruleset(&ips, &dns);
        insta::assert_snapshot!(ruleset);
    }

    #[test]
    fn generate_ruleset_with_ipv4_and_ipv6_mixed() {
        let mut ips: BTreeSet<IpAddr> = BTreeSet::new();
        ips.insert(ipv4(93, 184, 216, 34));
        ips.insert(ipv6("2606:2800:220:1:248:1893:25c8:1946"));
        let dns = vec![ipv4(1, 1, 1, 1), ipv6("2606:4700:4700::1111")];
        let ruleset = generate_ruleset(&ips, &dns);
        insta::assert_snapshot!(ruleset);
    }

    #[test]
    fn generate_ruleset_is_deterministic() {
        let mut ips: BTreeSet<IpAddr> = BTreeSet::new();
        ips.insert(ipv4(93, 184, 216, 34));
        ips.insert(ipv4(140, 82, 121, 4));
        let dns = vec![ipv4(1, 1, 1, 1)];
        let r1 = generate_ruleset(&ips, &dns);
        let r2 = generate_ruleset(&ips, &dns);
        assert_eq!(r1, r2);
    }

    #[test]
    fn apply_ruleset_sends_text_via_stdin() {
        let exec = MockCommandExecutor::new(vec![CannedOutput::ok()]);
        apply_ruleset(&exec, "table inet tau_sandbox {}", 12345).expect("apply");
        let calls = exec.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].cmd, "nsenter");
        assert_eq!(calls[0].args[0], "--net=/proc/12345/ns/net");
        assert_eq!(calls[0].args[1], "nft");
        assert_eq!(calls[0].args[2], "-f");
        assert_eq!(calls[0].args[3], "-");
        assert_eq!(calls[0].stdin.as_deref(), Some("table inet tau_sandbox {}"));
    }

    #[test]
    fn discover_dns_servers_falls_back_when_file_missing() {
        let path = std::path::Path::new("/nonexistent/path/to/resolv.conf");
        let servers = discover_dns_servers_from_path(path);
        // Fallback: [1.1.1.1, 8.8.8.8]
        assert_eq!(servers.len(), 2);
        assert!(servers.contains(&ipv4(1, 1, 1, 1)));
        assert!(servers.contains(&ipv4(8, 8, 8, 8)));
    }
}
