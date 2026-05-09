//! Strict-tier enforcement: landlock + user/network namespace unshare + seccomp BPF.
//!
//! The pre_exec hook runs three operations in this exact order:
//! 1. **Landlock** (`install_landlock`) — uses `landlock_*` syscalls that seccomp
//!    would block if installed first.
//! 2. **`unshare(flags)`** — drops the child into a fresh user namespace (gaining
//!    all capabilities within it) and a new network namespace. `CLONE_NEWNET` is
//!    always included (sub-project F; see `net::unshare_flags_for_plan`).
//!    Must run before seccomp blocks `unshare(2)`.
//! 3. **seccomp BPF filter** (`apply_filter`) — installed last; once active it
//!    blocks `unshare`, `landlock_*`, and any other syscall not in the allow-list.
//!    The allow-list is the baseline extended by capability-conditional rules:
//!    `exec::extend_with_exec_rules` (v0.1 no-op) and
//!    `net::extend_with_network_rules` (adds socket-family syscalls for `Network(Http)`).
//!
//! The BPF program is **compiled in the parent** (cheap, one-time) and the
//! compiled byte-slice is moved into the pre_exec closure by value. The child
//! only calls `prctl(PR_SET_NO_NEW_PRIVS)` + `seccomp(SET_MODE_FILTER)`.
//!
//! # Known limitation (async-signal-safety)
//! This closure inherits the async-signal-safety hazard documented in `light.rs`:
//! it runs between fork and exec in a multi-threaded (tokio) process.
//! `nix::sched::unshare` is a thin syscall wrapper (signal-safe).
//! `seccompiler::apply_filter` calls `prctl` then `seccomp` (both signal-safe).
//! The main remaining risk is from `install_landlock` (see light.rs for details).
//! A future task should consider a fork-server pattern to eliminate the window.

use std::convert::TryInto;
use std::os::unix::process::CommandExt;
use std::process::Command;

use nix::sched::unshare;
use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, SeccompRule};
use tau_ports::{SandboxError, SandboxHandle, SandboxPlan};

use crate::light::install_landlock_from_plan;

/// Build the baseline syscall allow-list as a rules map.
///
/// Exposed for unit tests that need to introspect the real baseline rather than
/// constructing their own ad-hoc copies. Each entry maps a syscall number to an
/// empty rules vec (meaning "unconditionally allow" — no argument matching).
///
/// # Syscall set rationale
/// The allow-list covers the baseline needs of a plugin communicating over stdio/socketpair
/// without network access. `SYS_socket`, `SYS_connect`, `SYS_bind`, and `SYS_listen` are
/// intentionally **excluded** — Task 5 will add them conditionally per `NetworkHttp` capability.
///
/// # Architecture note
/// Some syscall numbers differ between x86_64 and aarch64. This function uses
/// `#[cfg(target_arch)]` guards for arch-specific constants. The seccompiler crate handles
/// endianness; only the syscall numbers need to be arch-correct.
pub(crate) fn baseline_syscall_map() -> std::collections::BTreeMap<i64, Vec<SeccompRule>> {
    let mut rules: std::collections::BTreeMap<i64, Vec<SeccompRule>> =
        std::collections::BTreeMap::new();

    macro_rules! allow {
        ($($nr:expr),+ $(,)?) => {
            $(rules.entry($nr).or_default();)+
        };
    }

    // ---- File I/O ----
    allow!(
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_pread64,
        libc::SYS_pwrite64,
        libc::SYS_openat,
        libc::SYS_close,
        libc::SYS_fstat,
        libc::SYS_lseek,
        libc::SYS_readlinkat,
        libc::SYS_getdents64,
        libc::SYS_fcntl,
        libc::SYS_dup,
        libc::SYS_dup3,
        libc::SYS_pipe2,
        libc::SYS_mkdirat,
        libc::SYS_unlinkat,
        libc::SYS_linkat,
        libc::SYS_renameat,
        libc::SYS_renameat2,
        libc::SYS_symlinkat,
        libc::SYS_chdir,
        libc::SYS_fchdir,
        libc::SYS_getcwd,
        libc::SYS_umask,
        libc::SYS_faccessat,
        libc::SYS_truncate,
        libc::SYS_ftruncate,
    );

    // Arch-specific file I/O constants that only exist on x86_64.
    // dup2 is legacy (aarch64 uses dup3 exclusively).
    #[cfg(target_arch = "x86_64")]
    allow!(
        libc::SYS_stat,
        libc::SYS_lstat,
        libc::SYS_access,
        libc::SYS_pipe,
        libc::SYS_open,
        libc::SYS_creat,
        libc::SYS_dup2,
    );

    // openat2, newfstatat, statx, faccessat2 via raw syscall numbers.
    // Using raw numbers avoids libc version skew; values are stable in the kernel ABI.
    // x86_64: openat2=437, newfstatat=262, statx=332, faccessat2=439
    // aarch64: openat2=437, newfstatat=79,  statx=291, faccessat2=439
    #[cfg(target_arch = "x86_64")]
    allow!(
        262_i64, // newfstatat / fstatat64
        332_i64, // statx
        437_i64, // openat2
        439_i64, // faccessat2
    );

    #[cfg(target_arch = "aarch64")]
    allow!(
        79_i64,  // newfstatat
        291_i64, // statx
        437_i64, // openat2
        439_i64, // faccessat2
    );

    // ---- Memory ----
    allow!(
        libc::SYS_mmap,
        libc::SYS_munmap,
        libc::SYS_mprotect,
        libc::SYS_mremap,
        libc::SYS_madvise,
        libc::SYS_brk,
    );

    // mmap2 is 32-bit-only; not present on x86_64 or aarch64.

    // ---- Process / thread ----
    allow!(
        libc::SYS_clone,
        libc::SYS_wait4,
        libc::SYS_waitid,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_getppid,
        libc::SYS_set_tid_address,
        libc::SYS_set_robust_list,
        libc::SYS_prlimit64,
        libc::SYS_getrusage,
        libc::SYS_getuid,
        libc::SYS_geteuid,
        libc::SYS_getgid,
        libc::SYS_getegid,
        libc::SYS_setresuid,
        libc::SYS_setresgid,
        libc::SYS_setpgid,
        libc::SYS_getpgid,
        libc::SYS_getsid,
        libc::SYS_setsid,
        libc::SYS_getgroups,
        libc::SYS_prctl,
        libc::SYS_sched_yield,
        // T1 (2026-05-09): tokio's Builder::new_multi_thread sizes the
        // worker pool by calling sched_getaffinity. Without this, the
        // KillProcess mismatch action SIGSYSes the child before handshake.
        libc::SYS_sched_getaffinity,
        // Defensive: tokio doesn't currently call sched_setaffinity, but
        // some runtime configurations do. Allowed alongside getaffinity.
        libc::SYS_sched_setaffinity,
        libc::SYS_nanosleep,
        libc::SYS_clock_nanosleep,
        libc::SYS_clock_gettime,
        libc::SYS_clock_getres,
        libc::SYS_gettimeofday,
    );

    // arch_prctl is x86_64-only.
    #[cfg(target_arch = "x86_64")]
    allow!(libc::SYS_arch_prctl);

    // clone3: raw number 435 on both x86_64 and aarch64 (added in Linux 5.3).
    allow!(435_i64);

    // ---- Signals ----
    allow!(
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_rt_sigreturn,
        libc::SYS_kill,
        libc::SYS_tgkill,
        libc::SYS_sigaltstack,
    );

    // ---- Polling / async ----
    // ppoll, epoll_create1, epoll_ctl, epoll_pwait — exist on both x86_64 and aarch64.
    // poll, epoll_wait — x86_64 only (aarch64 uses ppoll and epoll_pwait exclusively).
    allow!(
        libc::SYS_ppoll,
        libc::SYS_epoll_create1,
        libc::SYS_epoll_ctl,
        libc::SYS_epoll_pwait,
        libc::SYS_eventfd2,
        libc::SYS_timerfd_create,
        libc::SYS_timerfd_settime,
        libc::SYS_futex,
    );

    // poll, epoll_create (legacy), epoll_wait, select, pselect6, eventfd — x86_64 only.
    // aarch64 uses only the newer variants (ppoll, epoll_create1, epoll_pwait).
    #[cfg(target_arch = "x86_64")]
    allow!(
        libc::SYS_poll,
        libc::SYS_epoll_create,
        libc::SYS_epoll_wait,
        libc::SYS_select,
        libc::SYS_pselect6,
        libc::SYS_eventfd,
    );

    // ---- Misc ----
    allow!(
        libc::SYS_exit,
        libc::SYS_exit_group,
        libc::SYS_restart_syscall,
        libc::SYS_sysinfo,
        libc::SYS_uname,
        libc::SYS_getrandom,
        libc::SYS_membarrier,
        libc::SYS_rseq,
    );

    // ---- IPC for plugin protocol (socketpair + send/recv; NOT socket/connect/bind/listen) ----
    allow!(
        libc::SYS_socketpair,
        libc::SYS_sendmsg,
        libc::SYS_recvmsg,
        libc::SYS_shutdown,
        libc::SYS_setsockopt,
        libc::SYS_getsockopt,
        libc::SYS_sendto,
        libc::SYS_recvfrom,
    );

    // execve / execveat — allowed at baseline; Task 5 tightens via argument filtering.
    allow!(libc::SYS_execve);

    // execveat: x86_64=322, aarch64=281
    #[cfg(target_arch = "x86_64")]
    allow!(322_i64);
    #[cfg(target_arch = "aarch64")]
    allow!(281_i64);

    rules
}

/// Build the baseline plugin allow-list seccomp BPF program.
///
/// Compiled in the **parent** process once; the resulting `BpfProgram` (a `Vec<sock_filter>`)
/// is moved into the pre_exec closure by value. The child only calls `prctl` + `seccomp`.
///
/// Used by unit tests (verifying baseline filter compiles); production callers
/// go through `baseline_syscall_map` + per-plan extensions + `compile_filter`.
#[allow(dead_code)]
pub(crate) fn build_baseline_filter() -> Result<BpfProgram, SandboxError> {
    let rules = baseline_syscall_map();
    compile_filter(rules)
}

/// Compile a rules map into a BPF program.
///
/// Shared by `build_baseline_filter` (for tests that use the raw baseline) and
/// `apply_strict` (which extends the baseline before compiling).
fn compile_filter(
    rules: std::collections::BTreeMap<i64, Vec<SeccompRule>>,
) -> Result<BpfProgram, SandboxError> {
    let arch: seccompiler::TargetArch =
        std::env::consts::ARCH
            .try_into()
            .map_err(|_| SandboxError::WrapFailed {
                message: format!(
                    "seccompiler does not support arch '{}'; cannot build strict filter",
                    std::env::consts::ARCH
                ),
            })?;

    let filter = SeccompFilter::new(
        rules,
        // mismatch_action: kill the whole process for unlisted syscalls.
        SeccompAction::KillProcess,
        // match_action: allow listed syscalls.
        SeccompAction::Allow,
        arch,
    )
    .map_err(|e| SandboxError::WrapFailed {
        message: format!("seccomp filter build error: {e}"),
    })?;

    filter
        .try_into()
        .map_err(|e: seccompiler::BackendError| SandboxError::WrapFailed {
            message: format!("seccomp BPF compile error: {e}"),
        })
}

/// Apply Strict-tier isolation to `cmd`:
/// 1. Landlock filesystem rules (from plan).
/// 2. `unshare` with `CLONE_NEWUSER | CLONE_NEWNET` (always both — sub-project F
///    simplified `net::unshare_flags_for_plan` to return both flags unconditionally).
/// 3. seccomp BPF allow-list filter (baseline extended by exec + network capability rules).
///
/// All preparation (path collection, rules extension, BPF compilation) happens in the
/// **parent** before `fork`. The `pre_exec` closure in the child only calls three
/// signal-safe operations: landlock, unshare, seccomp.
///
/// # Ordering within pre_exec
///
/// The order is fixed and must not be changed:
/// 1. Landlock — uses `landlock_*` syscalls that seccomp would block if installed first.
/// 2. unshare — must precede seccomp because `unshare(2)` is not in the allow-list.
/// 3. seccomp — installed last; once active it blocks everything not in the allow-list.
///
/// For plans with `Network(Http)`: spawns a userspace proxy task (T3-T4) in the parent
/// and wraps the child command with `tau-net-bridge` (T5). The proxy guard is nested
/// inside the returned `SandboxHandle` for LIFO cleanup.
pub(crate) fn apply_strict(
    plan: &SandboxPlan,
    cmd: &mut Command,
) -> Result<SandboxHandle, SandboxError> {
    // Collect landlock paths from the plan (same logic as light tier).
    // Made mutable so Network(Http) can append the proxy socket path.
    let (mut read_paths, mut write_paths) = crate::light::collect_landlock_paths(plan, cmd)?;

    // Collect exec-gated paths from Filesystem(Exec) and Process(Spawn) capabilities.
    // Resolve symlinks so landlock path matching covers both the link and its target.
    let exec_paths: Vec<std::path::PathBuf> = crate::light::collect_exec_paths(plan)
        .into_iter()
        .filter_map(|p| {
            match std::fs::canonicalize(&p) {
                Ok(canonical) if canonical == p => Some(vec![p]),
                Ok(canonical) => Some(vec![p, canonical]),
                Err(_) => None, // Skip unresolvable exec paths silently.
            }
        })
        .flatten()
        .collect();

    // Build the extended rules map: baseline → exec extension → network extension.
    let mut rules = baseline_syscall_map();
    crate::exec::extend_with_exec_rules(&mut rules, plan);
    crate::net::extend_with_network_rules(&mut rules, plan);

    // Compile the BPF program in the parent — cheap, deterministic.
    let bpf: BpfProgram = compile_filter(rules)?;

    // Determine unshare flags: always CLONE_NEWUSER | CLONE_NEWNET.
    // Sub-project F simplified unshare_flags_for_plan — see net.rs and
    // net_filter/INTEGRATION.md for the post-spawn hook plan (F task 6.5).
    let unshare_flags = crate::net::unshare_flags_for_plan(plan);

    // Determine if the plan requests outbound HTTP.
    let has_network_http = plan.capabilities.iter().any(|c| {
        matches!(
            c,
            tau_domain::Capability::Network(tau_domain::NetCapability::Http { .. })
        )
    });

    // For Network(Http): spawn the userspace proxy, extend landlock paths,
    // and wrap cmd with tau-net-bridge so the child dials through the proxy.
    // The proxy guard is returned inside the SandboxHandle for LIFO cleanup.
    let proxy_handle = if has_network_http {
        // Collect allowed hosts from all Http capabilities.
        let mut allowed_hosts: Vec<String> = Vec::new();
        for cap in &plan.capabilities {
            if let tau_domain::Capability::Network(tau_domain::NetCapability::Http {
                hosts, ..
            }) = cap
            {
                allowed_hosts.extend(hosts.iter().cloned());
            }
        }

        // Validate hosts: rejects wildcards + non-loopback IP literals.
        tau_sandbox_proxy::validate_hosts(&allowed_hosts).map_err(|e| SandboxError::Proxy {
            message: format!("host validation: {e}"),
        })?;

        // Spawn the proxy task in the parent's tokio runtime.
        let handle =
            tau_sandbox_proxy::spawn_proxy(allowed_hosts).map_err(|e| SandboxError::Proxy {
                message: format!("spawn_proxy: {e}"),
            })?;
        let proxy_sock_path = handle.sock_path().to_path_buf();

        // Grant the child read+write access to the proxy socket via landlock.
        read_paths.push(proxy_sock_path.clone());
        write_paths.push(proxy_sock_path.clone());

        // Snapshot the original program + args so we can wrap them.
        let original_program = cmd.get_program().to_os_string();
        let original_args: Vec<std::ffi::OsString> =
            cmd.get_args().map(|a| a.to_os_string()).collect();
        // Snapshot existing envs so they survive the cmd replacement.
        let original_envs: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)> = cmd
            .get_envs()
            .map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string())))
            .collect();

        // Replace the command: tau-net-bridge --proxy-sock=<path>
        //   --listen=127.0.0.1:8443 -- <original> <args>
        // std::process::Command has no "set program" method, so we rebuild it.
        // Bridge binary path: runtime env var, default to PATH lookup. Tests
        // set TAU_NET_BRIDGE_PATH to env!("CARGO_BIN_EXE_tau-net-bridge")
        // (that env var IS set in test contexts that depend on the bin target).
        let bridge_path = std::env::var_os("TAU_NET_BRIDGE_PATH")
            .unwrap_or_else(|| std::ffi::OsString::from("tau-net-bridge"));
        *cmd = std::process::Command::new(bridge_path);
        cmd.arg(format!("--proxy-sock={}", proxy_sock_path.display()))
            .arg("--listen=127.0.0.1:8443")
            .arg("--")
            .arg(&original_program)
            .args(&original_args);
        // Restore stdio piping. `std::process::Command` has no getter for
        // stdin/stdout/stderr so we cannot snapshot the caller's choice the
        // way we do for envs. Pipe all three unconditionally — this matches
        // tau-runtime's `plugin_host::process::spawn_and_handshake` (which
        // always sets `.stdin(piped()).stdout(piped()).stderr(piped())`)
        // and the controlled-env tests that route stdout/stderr through
        // `Command::output()`. The bridge process inherits these pipes
        // across its fork+exec to the plugin (see bin/tau-net-bridge.rs),
        // so the host-side Child handles produced after `spawn()` are the
        // pipes the plugin actually reads/writes through.
        //
        // Without this, the rebuild above silently drops the caller's
        // stdio settings and the spawn returns a Child whose
        // `stdin`/`stdout`/`stderr` are all `None`, panicking the
        // host-side handshake driver at `child.stdin.take().expect(...)`.
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        // Restore the original environment, then append HTTPS_PROXY.
        for (k, v) in original_envs {
            match v {
                Some(val) => {
                    cmd.env(k, val);
                }
                None => {
                    cmd.env_remove(k);
                }
            }
        }
        cmd.env("HTTPS_PROXY", "http://127.0.0.1:8443");

        Some(handle)
    } else {
        None
    };

    // KNOWN-LIMITATION: async-signal-safety — see light.rs for the full note.
    // Additional operations added here:
    // - `nix::sched::unshare` is a thin syscall wrapper; signal-safe.
    // - `seccompiler::apply_filter` calls `prctl` + `seccomp`; signal-safe.
    // The remaining allocation risk is from `install_landlock` (step 1).
    //
    // SAFETY: pre_exec runs in the child after fork() but before exec().
    // All operations (landlock, unshare, seccomp) are child-local and do
    // not affect the parent process.
    unsafe {
        cmd.pre_exec(move || {
            // Step 1: landlock filesystem isolation (read/write) + exec gating.
            install_landlock_from_plan(&read_paths, &write_paths, &exec_paths)
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            // Step 2: drop into new user namespace + isolated network namespace.
            unshare(unshare_flags).map_err(|e| std::io::Error::other(e.to_string()))?;

            // Step 3: install seccomp BPF allow-list (blocks unshare/landlock after this).
            seccompiler::apply_filter(bpf.as_slice())
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            Ok(())
        });
    }

    // Nest the proxy guard inside the SandboxHandle so it is dropped (LIFO)
    // when the handle is dropped.
    let mut handle = SandboxHandle::noop();
    if let Some(p) = proxy_handle {
        handle.nest_handle(Box::new(p));
    }

    Ok(handle)
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;

    /// Asserts that the baseline seccomp filter compiles to a non-empty BPF program.
    #[test]
    fn baseline_filter_compiles() {
        let prog = build_baseline_filter().expect("filter should compile");
        assert!(!prog.is_empty(), "compiled BPF program must be non-empty");
    }

    /// Asserts that `SYS_read` and `SYS_write` are in the real baseline allow-list.
    #[test]
    fn syscall_map_includes_read_write() {
        let map = baseline_syscall_map();
        assert!(
            map.contains_key(&libc::SYS_read),
            "SYS_read must be in baseline allow-list"
        );
        assert!(
            map.contains_key(&libc::SYS_write),
            "SYS_write must be in baseline allow-list"
        );
    }

    /// Asserts that `SYS_socket` is NOT in the baseline allow-list.
    ///
    /// Task 5 will add it conditionally when `NetworkHttp` capability is present.
    #[test]
    fn syscall_map_excludes_socket_baseline() {
        let map = baseline_syscall_map();
        assert!(
            !map.contains_key(&libc::SYS_socket),
            "SYS_socket must not appear in baseline allow-list; Task 5 adds it conditionally"
        );
        assert!(
            !map.contains_key(&libc::SYS_connect),
            "SYS_connect must not appear in baseline allow-list"
        );
        assert!(
            !map.contains_key(&libc::SYS_bind),
            "SYS_bind must not appear in baseline allow-list"
        );
        assert!(
            !map.contains_key(&libc::SYS_listen),
            "SYS_listen must not appear in baseline allow-list"
        );
    }

    /// Asserts that `apply_strict` returns a `SandboxHandle` without panicking.
    ///
    /// This does NOT spawn the command; it exercises BPF compilation + closure
    /// capture in the parent process only.
    #[test]
    fn baseline_syscall_map_includes_sched_getaffinity() {
        let map = baseline_syscall_map();
        assert!(
            map.contains_key(&libc::SYS_sched_getaffinity),
            "tokio Builder::new_multi_thread calls sched_getaffinity to size \
             the worker pool; without it the KillProcess mismatch action \
             SIGSYSes the child before handshake (T1 finding 2026-05-09)"
        );
        assert!(
            map.contains_key(&libc::SYS_sched_setaffinity),
            "sched_setaffinity is allowed defensively alongside getaffinity"
        );
    }

    #[test]
    fn apply_strict_routes_through_pre_exec() {
        let plan_json = serde_json::json!({
            "capabilities": [],
            "context": null,
            "limits": null,
        });
        let plan: tau_ports::SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");

        let mut cmd = Command::new("/bin/true");
        let handle = apply_strict(&plan, &mut cmd)
            .expect("apply_strict must succeed on a plan with no capabilities");
        drop(handle);
    }
}
