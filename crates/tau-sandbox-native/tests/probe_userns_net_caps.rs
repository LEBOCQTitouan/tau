//! Phase 0 verification probe — does GHA ubuntu-latest grant CAP_NET_ADMIN
//! inside an unprivileged user namespace + netns, with veth creation?
//!
//! Run via: `cargo test -p tau-sandbox-native --test probe_userns_net_caps -- --ignored --nocapture`
//! Result decides whether sub-project F's e2e tests run in CI.
//!
//! This probe uses pure-Rust syscalls (fork + nix::sched::unshare + direct
//! /proc/self/uid_map writes) to mirror what F's real code will do at runtime,
//! bypassing the `unshare(1)` CLI tool whose `--map-root-user` flag fails on GHA
//! with "write failed /proc/self/uid_map: Operation not permitted".

#![cfg(target_os = "linux")]

use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

use nix::sched::{unshare, CloneFlags};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::{fork, getgid, getuid, ForkResult};

/// Exit codes used by the child process to signal which step failed.
const EXIT_OK: i32 = 0;
const EXIT_UNSHARE: i32 = 11;
const EXIT_UID_MAP: i32 = 12;
const EXIT_NETLINK_SOCKET: i32 = 13;
const EXIT_VETH: i32 = 14;

#[test]
#[ignore]
fn gha_supports_unprivileged_userns_with_veth_and_netlink() {
    // Capture uid/gid *before* forking so the child can write them into
    // /proc/self/uid_map and /proc/self/gid_map.
    let orig_uid = getuid().as_raw();
    let orig_gid = getgid().as_raw();

    // SAFETY: We call fork() immediately and the child only performs
    // signal-safe operations (unshare syscall, file writes, socket(), exec of
    // `ip`).  No mutexes from the parent are held in the child.
    let child_pid = match unsafe { fork() }.expect("fork failed") {
        ForkResult::Parent { child } => child,
        ForkResult::Child => {
            // ---- child ----
            run_child(orig_uid, orig_gid);
        }
    };

    // ---- parent ----
    let status = waitpid(child_pid, None).expect("waitpid failed");

    let exit_code = match status {
        WaitStatus::Exited(_, code) => code,
        WaitStatus::Signaled(_, sig, _) => {
            panic!("child killed by signal {sig}");
        }
        other => panic!("unexpected wait status: {other:?}"),
    };

    let description = match exit_code {
        EXIT_OK => "success",
        EXIT_UNSHARE => "CLONE_NEWUSER | CLONE_NEWNET unshare failed",
        EXIT_UID_MAP => "uid_map / gid_map write failed",
        EXIT_NETLINK_SOCKET => "AF_NETLINK SOCK_RAW socket failed",
        EXIT_VETH => "veth pair creation failed",
        other => panic!("unexpected child exit code {other}"),
    };

    assert!(
        exit_code == EXIT_OK,
        "GHA ubuntu-latest does not support unprivileged userns + netns + veth + AF_NETLINK \
         (exit {exit_code}: {description}); \
         sub-project F's e2e tests will need probe-and-skip contingency"
    );
}

/// Executed in the child process.  Returns only on success (exit 0); all
/// failure paths call `std::process::exit` with a non-zero code so the parent
/// can distinguish them.
fn run_child(orig_uid: u32, orig_gid: u32) -> ! {
    // Step 1: drop into fresh user + network namespaces.
    if unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNET).is_err() {
        std::process::exit(EXIT_UNSHARE);
    }

    // Step 2: map ourselves to uid 0 / gid 0 inside the new userns.
    //
    // Required write order (enforced by the kernel since Linux 3.19):
    //   a) write "deny" to /proc/self/setgroups   (disables setgroups(2))
    //   b) write uid_map
    //   c) write gid_map
    let uid_map_line = format!("0 {orig_uid} 1\n");
    let gid_map_line = format!("0 {orig_gid} 1\n");

    let map_ok = fs::write("/proc/self/setgroups", "deny\n").is_ok()
        && fs::write("/proc/self/uid_map", uid_map_line.as_bytes()).is_ok()
        && fs::write("/proc/self/gid_map", gid_map_line.as_bytes()).is_ok();

    if !map_ok {
        std::process::exit(EXIT_UID_MAP);
    }

    // Step 3: open an AF_NETLINK / SOCK_RAW / NETLINK_ROUTE socket.
    // The workspace nix crate does not enable the `socket` feature, so we call
    // libc directly — matching what F's real code will do.
    //
    // SAFETY: socket() is a plain syscall with no preconditions; errors are
    // returned as a negative fd value.
    let netlink_fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            libc::NETLINK_ROUTE,
        )
    };
    if netlink_fd < 0 {
        std::process::exit(EXIT_NETLINK_SOCKET);
    }
    // Close immediately; we only needed to confirm the capability.
    unsafe { libc::close(netlink_fd) };

    // Step 4: create a veth pair inside the isolated netns (exercises
    // CAP_NET_ADMIN within the userns), then delete it to clean up.
    let add = Command::new("ip")
        .args(["link", "add", "veth-probe-host", "type", "veth", "peer", "name", "veth-probe-child"])
        .status();
    match add {
        Ok(s) if s.success() => {}
        _ => std::process::exit(EXIT_VETH),
    }

    let del = Command::new("ip")
        .args(["link", "del", "veth-probe-host"])
        .status();
    match del {
        Ok(s) if s.success() => {}
        // Deletion failure is non-fatal for the probe's purpose; log and continue.
        _ => eprintln!("probe: ip link del veth-probe-host failed (non-fatal)"),
    }

    eprintln!("PROBE OK");
    std::process::exit(EXIT_OK);
}
