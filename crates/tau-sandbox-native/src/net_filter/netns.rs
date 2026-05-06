//! veth pair + netns setup for per-host network filtering.
//!
//! Lifecycle:
//! 1. Parent creates a veth pair on the host side: `ip link add <host> type veth peer name <child>`.
//! 2. Parent assigns IP to the host end + brings it up.
//! 3. Parent moves the child end into the child's netns: `ip link set <child> netns <pid>`.
//! 4. Child runs `ip` commands inside its netns (via nsenter from the parent):
//!    `ip link set lo up; ip addr add <child_ip>/30 dev <child>; ip link set <child> up;
//!     ip route add default via <parent_ip>`.
//!
//! All shell-out is mediated by the `CommandExecutor` trait so unit tests
//! can pass `MockCommandExecutor` to assert exact invocation sequences.
//!
//! IFNAMSIZ=16 (incl. NUL) → interface names must be ≤ 15 chars.
//! Format: `tsb<pid_5digits>-<seq>h` / `...c` (max 14 chars at pid=99999).

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU32, Ordering};

use super::error::NetFilterError;
use super::exec::CommandExecutor;

/// Parent + child interface names + the pre-allocated /30 subnet IPs.
#[derive(Debug, Clone)]
pub(crate) struct VethPair {
    pub name_host: String,
    pub name_child: String,
    pub parent_ip: Ipv4Addr,
    pub child_ip: Ipv4Addr,
}

/// Pre-allocated /30 subnet for a veth pair.
///
/// Returned by [`allocate_subnet`] (called in wrap_spawn, before fork) and
/// consumed by [`setup_veth_pair_with_subnet`] (called in apply_post_spawn,
/// after fork). The pre-allocation lets `wrap_spawn` set the
/// `TAU_NET_PARENT_VETH_IP` env var on `cmd` BEFORE spawn.
#[derive(Debug, Clone, Copy)]
pub(crate) struct VethSubnet {
    pub parent_ip: Ipv4Addr,
    pub child_ip: Ipv4Addr,
}

/// Allocate a /30 subnet without touching the kernel.
///
/// Pure function. Atomically reserves a slot via static AtomicU32.
///
/// /30 reserves 4 addresses:
/// - .0+0  network address
/// - .0+1  parent
/// - .0+2  child
/// - .0+3  broadcast
///
/// Subnet is `10.222.<pid_modulo_256>.<seq*4>/30`. Wrap-around at seq=63
/// (third octet's 252 hosts / 4 per pair = 63) reuses earlier IPs;
/// `ip link add` fails on an existing host-end name, prompting the caller
/// to retry (handled by the caller's seq atomic).
pub(crate) fn allocate_subnet() -> VethSubnet {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let third_octet = (pid % 256) as u8;
    // /30 hosts: 1, 5, 9, ..., 249. Wraps at 63 cycles per third-octet.
    let host_offset = ((seq * 4) % 252) as u8 + 1;
    VethSubnet {
        parent_ip: Ipv4Addr::new(10, 222, third_octet, host_offset),
        child_ip: Ipv4Addr::new(10, 222, third_octet, host_offset + 1),
    }
}

/// Generate a unique pair of veth interface names.
///
/// IFNAMSIZ-compliant: `tsb<pid%100000>-<seq>h` / `...c`.
/// At max pid (5 digits) + max seq (10 digits arbitrary), the name is
/// 13 chars at worst — well under the 15-char limit.
fn next_veth_names() -> (String, String) {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let host = format!("tsb{}-{}h", pid % 100000, seq);
    let child = format!("tsb{}-{}c", pid % 100000, seq);
    debug_assert!(
        host.len() < 16,
        "veth name too long: {host} ({} chars)",
        host.len()
    );
    debug_assert!(
        child.len() < 16,
        "veth name too long: {child} ({} chars)",
        child.len()
    );
    (host, child)
}

/// Create a veth pair on the host side. Assigns IP + brings up the host end.
///
/// Takes a pre-allocated `VethSubnet` (from [`allocate_subnet`]) so the
/// parent IP is known before fork and can be passed to the child via env var.
///
/// Returns the `VethPair` for downstream `move_peer_to_netns` +
/// `assign_child_ip_and_up_via_nsenter`.
pub(crate) fn setup_veth_pair_with_subnet(
    exec: &dyn CommandExecutor,
    subnet: VethSubnet,
) -> Result<VethPair, NetFilterError> {
    let (name_host, name_child) = next_veth_names();
    let parent_ip = subnet.parent_ip;
    let child_ip = subnet.child_ip;

    // Step 1: create the veth pair.
    let out = exec
        .run(
            "ip",
            &[
                "link",
                "add",
                &name_host,
                "type",
                "veth",
                "peer",
                "name",
                &name_child,
            ],
            None,
        )
        .map_err(|e| NetFilterError::NetnsSetup {
            context: "ip link add veth pair",
            source: e,
        })?;
    if !out.status.success() {
        return Err(NetFilterError::NetnsSetup {
            context: "ip link add veth pair",
            source: std::io::Error::other(String::from_utf8_lossy(&out.stderr).into_owned()),
        });
    }

    // Step 2: assign parent IP.
    let out = exec
        .run(
            "ip",
            &["addr", "add", &format!("{parent_ip}/30"), "dev", &name_host],
            None,
        )
        .map_err(|e| NetFilterError::NetnsSetup {
            context: "ip addr add (parent)",
            source: e,
        })?;
    if !out.status.success() {
        return Err(NetFilterError::NetnsSetup {
            context: "ip addr add (parent)",
            source: std::io::Error::other(String::from_utf8_lossy(&out.stderr).into_owned()),
        });
    }

    // Step 3: bring up parent end.
    let out = exec
        .run("ip", &["link", "set", &name_host, "up"], None)
        .map_err(|e| NetFilterError::NetnsSetup {
            context: "ip link set up (parent)",
            source: e,
        })?;
    if !out.status.success() {
        return Err(NetFilterError::NetnsSetup {
            context: "ip link set up (parent)",
            source: std::io::Error::other(String::from_utf8_lossy(&out.stderr).into_owned()),
        });
    }

    Ok(VethPair {
        name_host,
        name_child,
        parent_ip,
        child_ip,
    })
}

/// Move the child end of the veth pair into the child's netns (via PID).
pub(super) fn move_peer_to_netns(
    exec: &dyn CommandExecutor,
    pair: &VethPair,
    child_pid: i32,
) -> Result<(), NetFilterError> {
    let out = exec
        .run(
            "ip",
            &[
                "link",
                "set",
                &pair.name_child,
                "netns",
                &child_pid.to_string(),
            ],
            None,
        )
        .map_err(|e| NetFilterError::NetnsSetup {
            context: "ip link set netns",
            source: e,
        })?;
    if !out.status.success() {
        return Err(NetFilterError::NetnsSetup {
            context: "ip link set netns",
            source: std::io::Error::other(String::from_utf8_lossy(&out.stderr).into_owned()),
        });
    }
    Ok(())
}

/// Configure the child-side interface from the parent via nsenter.
///
/// Runs (inside child netns):
/// - `ip link set lo up`
/// - `ip addr add <child_ip>/30 dev <name_child>`
/// - `ip link set <name_child> up`
/// - `ip route add default via <parent_ip>`
pub(super) fn assign_child_ip_and_up_via_nsenter(
    exec: &dyn CommandExecutor,
    pair: &VethPair,
    child_pid: i32,
) -> Result<(), NetFilterError> {
    let netns_path = format!("--net=/proc/{child_pid}/ns/net");
    let child_ip_with_mask = format!("{}/30", pair.child_ip);
    let parent_ip_str = pair.parent_ip.to_string();

    let steps: &[(&str, &[&str])] = &[
        (
            "nsenter ip link set lo up",
            &[netns_path.as_str(), "ip", "link", "set", "lo", "up"],
        ),
        (
            "nsenter ip addr add (child)",
            &[
                netns_path.as_str(),
                "ip",
                "addr",
                "add",
                child_ip_with_mask.as_str(),
                "dev",
                pair.name_child.as_str(),
            ],
        ),
        (
            "nsenter ip link set up (child)",
            &[
                netns_path.as_str(),
                "ip",
                "link",
                "set",
                pair.name_child.as_str(),
                "up",
            ],
        ),
        (
            "nsenter ip route add default",
            &[
                netns_path.as_str(),
                "ip",
                "route",
                "add",
                "default",
                "via",
                parent_ip_str.as_str(),
            ],
        ),
    ];

    for (context, args) in steps {
        let out = exec
            .run("nsenter", args, None)
            .map_err(|e| NetFilterError::NetnsSetup { context, source: e })?;
        if !out.status.success() {
            return Err(NetFilterError::NetnsSetup {
                context,
                source: std::io::Error::other(String::from_utf8_lossy(&out.stderr).into_owned()),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::exec::test_support::{CannedOutput, MockCommandExecutor};
    use super::*;

    #[test]
    fn allocate_subnet_returns_valid_ipv4_pair_in_10_222_range() {
        let subnet = allocate_subnet();
        assert_eq!(subnet.parent_ip.octets()[0], 10);
        assert_eq!(subnet.parent_ip.octets()[1], 222);
        assert_eq!(subnet.child_ip.octets()[0], 10);
        assert_eq!(subnet.child_ip.octets()[1], 222);
        assert_eq!(
            subnet.parent_ip.octets()[2],
            subnet.child_ip.octets()[2],
            "same subnet"
        );
        assert_eq!(
            subnet.child_ip.octets()[3],
            subnet.parent_ip.octets()[3] + 1,
            "child is parent+1"
        );
    }

    #[test]
    fn next_veth_names_under_15_chars() {
        let (host, child) = next_veth_names();
        assert!(host.len() < 16, "{host} ({} chars)", host.len());
        assert!(child.len() < 16, "{child} ({} chars)", child.len());
        assert_ne!(host, child);
        assert!(host.ends_with('h'));
        assert!(child.ends_with('c'));
    }

    #[test]
    fn setup_veth_pair_with_subnet_invokes_three_ip_commands_in_order() {
        // MockCommandExecutor uses Vec::pop() — last element in vec is consumed first.
        // All three responses are CannedOutput::ok(), so order does not matter here.
        let exec = MockCommandExecutor::new(vec![
            CannedOutput::ok(),
            CannedOutput::ok(),
            CannedOutput::ok(),
        ]);
        let subnet = VethSubnet {
            parent_ip: Ipv4Addr::new(10, 222, 0, 1),
            child_ip: Ipv4Addr::new(10, 222, 0, 2),
        };

        let pair = setup_veth_pair_with_subnet(&exec, subnet).expect("setup");
        assert!(pair.name_host.starts_with("tsb"));
        assert!(pair.name_child.starts_with("tsb"));
        assert_eq!(pair.parent_ip, Ipv4Addr::new(10, 222, 0, 1));
        assert_eq!(pair.child_ip, Ipv4Addr::new(10, 222, 0, 2));

        let calls = exec.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].cmd, "ip");
        assert_eq!(calls[0].args[0..2], ["link", "add"]);
        assert_eq!(calls[1].cmd, "ip");
        assert_eq!(calls[1].args[0..2], ["addr", "add"]);
        assert_eq!(calls[2].cmd, "ip");
        assert_eq!(calls[2].args[0..2], ["link", "set"]);
        // Last arg should be "up".
        assert_eq!(calls[2].args.last().unwrap(), "up");
    }

    #[test]
    fn setup_veth_pair_with_subnet_propagates_ip_failure() {
        // First call fails immediately; only one canned response needed.
        // Vec::pop() returns the last element — single-element vec works fine.
        let exec = MockCommandExecutor::new(vec![CannedOutput::err(
            "RTNETLINK answers: Operation not permitted",
        )]);
        let subnet = VethSubnet {
            parent_ip: Ipv4Addr::new(10, 222, 0, 1),
            child_ip: Ipv4Addr::new(10, 222, 0, 2),
        };
        let result = setup_veth_pair_with_subnet(&exec, subnet);
        assert!(matches!(result, Err(NetFilterError::NetnsSetup { .. })));
    }

    #[test]
    fn assign_child_ip_and_up_via_nsenter_invokes_four_nsenter_commands() {
        // All four responses are ok(); Vec::pop() order does not matter.
        let exec = MockCommandExecutor::new(vec![
            CannedOutput::ok(),
            CannedOutput::ok(),
            CannedOutput::ok(),
            CannedOutput::ok(),
        ]);
        let pair = VethPair {
            name_host: "tsb1-0h".to_string(),
            name_child: "tsb1-0c".to_string(),
            parent_ip: Ipv4Addr::new(10, 222, 1, 1),
            child_ip: Ipv4Addr::new(10, 222, 1, 2),
        };
        assign_child_ip_and_up_via_nsenter(&exec, &pair, 12345).expect("assign");

        let calls = exec.calls();
        assert_eq!(calls.len(), 4);
        for c in &calls {
            assert_eq!(c.cmd, "nsenter");
            assert!(c.args[0].starts_with("--net=/proc/12345/ns/"));
            assert_eq!(c.args[1], "ip");
        }
        // Verify ordering of inner ip subcommands:
        assert_eq!(calls[0].args[2..6], ["link", "set", "lo", "up"]);
        assert_eq!(calls[1].args[2], "addr");
        assert_eq!(calls[1].args[3], "add");
        assert_eq!(calls[2].args[2..4], ["link", "set"]);
        assert_eq!(calls[3].args[2], "route");
    }
}
