//! Phase 0 verification probe — does GHA ubuntu-latest grant CAP_NET_ADMIN
//! inside an unprivileged user namespace + netns, with veth creation?
//!
//! Run via: `cargo test -p tau-sandbox-native --test probe_userns_net_caps -- --ignored --nocapture`
//! Result decides whether sub-project F's e2e tests run in CI.

#![cfg(target_os = "linux")]

use std::process::Command;

#[test]
#[ignore]
fn gha_supports_unprivileged_userns_with_veth_and_netlink() {
    // Step 1: unshare into user+net namespaces.
    let unshare = Command::new("unshare")
        .args([
            "--user",
            "--net",
            "--map-root-user",
            "--",
            "sh",
            "-c",
            // Inside the namespaces:
            // 1. Create a veth pair (CAP_NET_ADMIN required).
            // 2. Open an AF_NETLINK socket (CAP_NET_ADMIN required for SOCK_RAW).
            // 3. Delete the veth pair to clean up.
            r#"
            set -e
            ip link add veth-probe-host type veth peer name veth-probe-child || { echo "FAIL: ip link add"; exit 11; }
            python3 -c "import socket; s = socket.socket(socket.AF_NETLINK, socket.SOCK_RAW, 0); s.close()" || { echo "FAIL: AF_NETLINK"; exit 12; }
            ip link del veth-probe-host || { echo "FAIL: ip link del"; exit 13; }
            echo "PROBE OK"
            "#,
        ])
        .output()
        .expect("spawn unshare");

    let stdout = String::from_utf8_lossy(&unshare.stdout);
    let stderr = String::from_utf8_lossy(&unshare.stderr);

    eprintln!("--- unshare stdout ---\n{stdout}");
    eprintln!("--- unshare stderr ---\n{stderr}");
    eprintln!("--- unshare status: {:?} ---", unshare.status);

    assert!(
        unshare.status.success(),
        "GHA ubuntu-latest does not support unprivileged userns + netns + veth + AF_NETLINK; sub-project F's e2e tests will need probe-and-skip contingency"
    );
    assert!(stdout.contains("PROBE OK"), "probe did not reach OK marker");
}
