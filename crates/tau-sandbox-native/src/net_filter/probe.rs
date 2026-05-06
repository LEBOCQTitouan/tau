//! Probe whether per-host network filtering prerequisites are available
//! on this host: `nft`, `ip`, `nsenter` binaries + CAP_NET_ADMIN-in-userns.
//!
//! Runs ONCE at adapter init; result is cached on the adapter struct.

use std::os::raw::c_int;
use std::path::PathBuf;

use super::error::NetFilterError;

/// AF_NETLINK socket family. From `<linux/socket.h>`.
const AF_NETLINK: c_int = 16;
/// SOCK_RAW. From `<sys/socket.h>`.
const SOCK_RAW: c_int = 3;
/// NETLINK_ROUTE protocol. From `<linux/netlink.h>`.
const NETLINK_ROUTE: c_int = 0;

/// Probe all prerequisites. Returns `Err` listing each missing item.
pub fn probe_prerequisites() -> Result<(), NetFilterError> {
    let mut missing: Vec<&'static str> = Vec::new();

    if which_binary("nft").is_none() {
        missing.push("nft");
    }
    if which_binary("ip").is_none() {
        missing.push("ip");
    }
    if which_binary("nsenter").is_none() {
        missing.push("nsenter");
    }

    if !probe_cap_net_admin_in_userns() {
        missing.push("CAP_NET_ADMIN-in-userns");
    }

    if missing.is_empty() {
        Ok(())
    } else {
        Err(NetFilterError::PrerequisitesUnavailable { missing })
    }
}

/// Locate a binary on `$PATH`. Returns `None` if not found or unreadable.
pub(crate) fn which_binary(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Spawn a child via `unshare(CLONE_NEWUSER | CLONE_NEWNET)` and try to
/// open `socket(AF_NETLINK, SOCK_RAW, NETLINK_ROUTE)`. Returns `true` if
/// successful, `false` if the kernel denies the operation.
///
/// Uses `nix::sched::unshare` + libc::socket directly to avoid a `nix` feature
/// flag (the workspace `nix` doesn't enable the `socket` feature).
fn probe_cap_net_admin_in_userns() -> bool {
    use nix::sched::{unshare, CloneFlags};
    use nix::sys::wait::{waitpid, WaitStatus};
    use nix::unistd::{fork, ForkResult};

    // Fork to isolate the namespace mutation from the parent process.
    // SAFETY: fork() is safe to call; the child only performs syscalls and
    // process_exit. No mutexes from the parent are held in the child.
    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => match waitpid(child, None) {
            Ok(WaitStatus::Exited(_, code)) => code == 0,
            _ => false,
        },
        Ok(ForkResult::Child) => {
            // Inside child: unshare + try opening AF_NETLINK socket.
            let result = unshare(CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWNET);
            if result.is_err() {
                std::process::exit(11);
            }
            // SAFETY: libc::socket is safe to call; the returned fd is closed
            // by process exit. We ignore the fd value; we only care about success.
            let fd = unsafe { libc::socket(AF_NETLINK, SOCK_RAW, NETLINK_ROUTE) };
            if fd < 0 {
                std::process::exit(12);
            }
            unsafe {
                libc::close(fd);
            }
            std::process::exit(0);
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn which_binary_finds_sh() {
        // /bin/sh is universally present on Linux.
        let result = which_binary("sh");
        assert!(result.is_some(), "sh should be found on PATH");
    }

    #[test]
    fn which_binary_returns_none_for_made_up_name() {
        let result = which_binary("definitely-not-a-real-binary-name-12345");
        assert!(result.is_none());
    }

    #[test]
    fn which_binary_returns_none_with_empty_path() {
        let original = std::env::var_os("PATH");
        // SAFETY: we restore PATH before the test ends.
        unsafe {
            std::env::remove_var("PATH");
        }
        let result = which_binary("sh");
        if let Some(p) = original {
            unsafe {
                std::env::set_var("PATH", p);
            }
        }
        assert!(result.is_none());
    }

    #[test]
    fn probe_cap_net_admin_in_userns_returns_a_bool() {
        // We can't deterministically assert true/false because it depends on
        // whether the test runner has unprivileged userns enabled. Just
        // verify the probe doesn't panic or hang.
        let _ = probe_cap_net_admin_in_userns();
    }

    #[test]
    fn probe_prerequisites_returns_some_result() {
        // On most CI/dev hosts the prereqs are present; on others they're not.
        // We just verify the function doesn't panic and returns a typed result.
        let _ = probe_prerequisites();
    }

    #[test]
    fn probe_prerequisites_error_lists_missing_items() {
        // Construct an error directly to verify the variant shape.
        let err = NetFilterError::PrerequisitesUnavailable {
            missing: vec!["nft", "ip"],
        };
        assert!(format!("{err}").contains("nft"));
        assert!(format!("{err}").contains("ip"));
    }
}
