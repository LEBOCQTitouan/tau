//! Minimal repro for sub-project E (per-command exec gating).
//!
//! Standalone Linux-only binary that mirrors tau-sandbox-native's pre_exec
//! sequence on a stripped-down skeleton, then calls execve directly.
//!
//! Exit codes:
//!   0           — unreachable (execve replaces the process)
//!   32 + L      — setup failed at layer L (1=landlock build, 2=create,
//!                 3=add rules, 4=restrict_self, 5=unshare,
//!                 6=seccomp compile, 7=seccomp apply, 8=arg parse)
//!   64 + errno  — execve failed with errno (clamped to 127)
//!
//! See docs/superpowers/specs/2026-05-11-sub-project-e-exec-gating-design.md
//! for the diagnostic matrix and methodology.

use std::ffi::CString;
use std::path::PathBuf;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const ARCH_SPECIFIC_SYSCALLS: &[i64] = &[libc::SYS_arch_prctl];
#[cfg(all(target_os = "linux", not(target_arch = "x86_64")))]
const ARCH_SPECIFIC_SYSCALLS: &[i64] = &[];

#[derive(Debug, Default, PartialEq, Eq)]
enum LandlockMode {
    #[default]
    Off,
    /// Apply BASELINE_SYSTEM_READ_PATHS only (no exec_path rule).
    Baseline,
    /// Apply baseline AND a file-level exec_path rule.
    BaselineExec,
    /// Apply ONLY a PathBeneath rule on the parent dir (e.g. `/usr/bin`) with
    /// the chosen grants — no baseline, no file-level rule.
    DirOnly,
}

/// Controls the exec strategy used by the harness.
#[derive(Debug, Default, PartialEq, Eq)]
enum ExecMode {
    /// Direct execve(target) — matches the T4 harness rows.
    #[default]
    Direct,
    /// Fork a child, then execvp(bare_name) with env cleared — mirrors the
    /// shell plugin's run_subprocess("echo", ...) path (grandchild / fork-depth
    /// divergence candidate B).
    ForkExecvp,
    /// Fork a child, then execvpe(bare_name) with PATH including /usr/local/bin —
    /// reproduces the production failure where glibc searches /usr/local/bin first,
    /// hits EACCES (landlock blocks Execute on the dir), and stops the PATH search.
    ForkExecvpWithPath,
    /// Apply unshare BEFORE landlock (matches production pre_exec order in
    /// strict.rs). Direct execve after setup — divergence candidate A.
    UnshareFirst,
}

#[derive(Debug, Default)]
struct Config {
    landlock: LandlockMode,
    exec_path: PathBuf,
    /// Comma-separated AccessFs flags applied to the exec_path rule.
    /// Recognized: "ReadFile", "ReadDir", "Execute", "FromAllV1".
    exec_grants: Vec<String>,
    /// Parent dir granted in `DirOnly` mode, e.g. `/usr/bin`.
    dir_only_path: PathBuf,
    dir_only_grants: Vec<String>,
    unshare_user: bool,
    unshare_net: bool,
    seccomp: bool,
    target: PathBuf,
    /// Controls whether to execve directly, fork+execvp, or unshare-first.
    exec_mode: ExecMode,
    /// Bare command name used in ForkExecvp mode (e.g. "echo").
    bare_cmd: String,
}

fn parse_args(args: &[String]) -> Config {
    let mut cfg = Config::default();
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        if let Some(v) = arg.strip_prefix("--landlock=") {
            cfg.landlock = match v {
                "off" => LandlockMode::Off,
                "baseline" => LandlockMode::Baseline,
                "baseline+exec" => LandlockMode::BaselineExec,
                "dir-only" => LandlockMode::DirOnly,
                _ => std::process::exit(32 + 8),
            };
        } else if let Some(v) = arg.strip_prefix("--exec-path=") {
            cfg.exec_path = PathBuf::from(v);
        } else if let Some(v) = arg.strip_prefix("--exec-grants=") {
            cfg.exec_grants = v.split(',').map(String::from).collect();
        } else if let Some(v) = arg.strip_prefix("--dir-only-path=") {
            cfg.dir_only_path = PathBuf::from(v);
        } else if let Some(v) = arg.strip_prefix("--dir-only-grants=") {
            cfg.dir_only_grants = v.split(',').map(String::from).collect();
        } else if arg == "--unshare-user" {
            cfg.unshare_user = true;
        } else if arg == "--unshare-net" {
            cfg.unshare_net = true;
        } else if arg == "--seccomp" {
            cfg.seccomp = true;
        } else if let Some(v) = arg.strip_prefix("--target=") {
            cfg.target = PathBuf::from(v);
        } else if let Some(v) = arg.strip_prefix("--exec-mode=") {
            cfg.exec_mode = match v {
                "direct" => ExecMode::Direct,
                "fork-execvp" => ExecMode::ForkExecvp,
                "fork-execvp-with-path" => ExecMode::ForkExecvpWithPath,
                "unshare-first" => ExecMode::UnshareFirst,
                _ => std::process::exit(32 + 8),
            };
        } else if let Some(v) = arg.strip_prefix("--bare-cmd=") {
            cfg.bare_cmd = v.to_string();
        } else {
            eprintln!("unknown arg: {arg}");
            std::process::exit(32 + 8);
        }
        i += 1;
    }
    cfg
}

#[cfg(target_os = "linux")]
fn parse_grants(grants: &[String]) -> landlock::BitFlags<landlock::AccessFs> {
    use landlock::{Access, AccessFs, BitFlags};
    let mut flags = BitFlags::<AccessFs>::empty();
    for g in grants {
        match g.as_str() {
            "ReadFile" => flags |= AccessFs::ReadFile,
            "ReadDir" => flags |= AccessFs::ReadDir,
            "Execute" => flags |= AccessFs::Execute,
            "FromAllV1" => flags |= AccessFs::from_all(landlock::ABI::V1),
            _ => {
                eprintln!("unknown grant: {g}");
                std::process::exit(32 + 8);
            }
        }
    }
    flags
}

/// Same as `tau-sandbox-native::light::BASELINE_SYSTEM_READ_PATHS`. Duplicated
/// here intentionally — the repro must match production exactly, but we don't
/// want a workspace dependency.
const BASELINE_SYSTEM_READ_PATHS: &[&str] = &[
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/lib",
    "/lib64",
    "/usr/lib",
    "/usr/lib64",
    "/etc",
    "/proc/self",
    "/sys/fs/cgroup",
    // Sub-project E exec-gating fix (2026-05-11): see light.rs comment.
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/local/lib",
    "/dev",
];

#[cfg(target_os = "linux")]
fn setup_landlock(cfg: &Config) -> Result<(), i32> {
    use landlock::{
        Access, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, ABI,
    };
    let abi = ABI::V1;
    let mut ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))
        .map_err(|_| 1)?
        .create()
        .map_err(|_| 2)?;
    match cfg.landlock {
        LandlockMode::Off => return Ok(()),
        LandlockMode::Baseline | LandlockMode::BaselineExec => {
            for sys_path in BASELINE_SYSTEM_READ_PATHS {
                if let Ok(fd) = PathFd::new(sys_path) {
                    ruleset = ruleset
                        .add_rule(PathBeneath::new(
                            fd,
                            AccessFs::ReadFile | AccessFs::ReadDir | AccessFs::Execute,
                        ))
                        .map_err(|_| 3)?;
                }
            }
            if cfg.landlock == LandlockMode::BaselineExec {
                let grants = parse_grants(&cfg.exec_grants);
                if let Ok(fd) = PathFd::new(&cfg.exec_path) {
                    ruleset = ruleset
                        .add_rule(PathBeneath::new(fd, grants))
                        .map_err(|_| 3)?;
                }
            }
        }
        LandlockMode::DirOnly => {
            let grants = parse_grants(&cfg.dir_only_grants);
            if let Ok(fd) = PathFd::new(&cfg.dir_only_path) {
                ruleset = ruleset
                    .add_rule(PathBeneath::new(fd, grants))
                    .map_err(|_| 3)?;
            }
        }
    }
    ruleset.restrict_self().map_err(|_| 4)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn setup_unshare(cfg: &Config) -> Result<(), i32> {
    use nix::sched::{unshare, CloneFlags};
    let mut flags = CloneFlags::empty();
    if cfg.unshare_user {
        flags |= CloneFlags::CLONE_NEWUSER;
    }
    if cfg.unshare_net {
        flags |= CloneFlags::CLONE_NEWNET;
    }
    unshare(flags).map_err(|_| 5)?;
    // If we unshared a user_ns, write uid_map / gid_map (best-effort).
    // Mirrors the tau-sandbox-native::strict best-effort path landed in PR #53.
    if cfg.unshare_user {
        let uid = nix::unistd::getuid().as_raw();
        let gid = nix::unistd::getgid().as_raw();
        let _ = std::fs::write("/proc/self/setgroups", "deny\n");
        let _ = std::fs::write("/proc/self/uid_map", format!("0 {uid} 1\n"));
        let _ = std::fs::write("/proc/self/gid_map", format!("0 {gid} 1\n"));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn setup_seccomp() -> Result<(), i32> {
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter};
    use std::collections::BTreeMap;
    // Allow-list: everything the harness + /usr/bin/echo need.
    // seccompiler 0.5: match_action != mismatch_action is required, so we use
    // Errno(EPERM) for unmatched syscalls. The list below covers all syscalls
    // observed via strace on the target (/usr/bin/echo) plus the ones needed
    // by the harness itself before execve.
    let mut rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> = BTreeMap::new();
    let base_syscalls: &[i64] = &[
        libc::SYS_execve,
        libc::SYS_exit,
        libc::SYS_exit_group,
        libc::SYS_write,
        libc::SYS_close,
        libc::SYS_brk,
        libc::SYS_mmap,
        libc::SYS_munmap,
        libc::SYS_mprotect,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_sigaltstack,
        libc::SYS_prctl,
        libc::SYS_set_tid_address,
        libc::SYS_set_robust_list,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_openat,
        libc::SYS_read,
        libc::SYS_readlinkat,
        libc::SYS_fstat,
        // Additional syscalls used by /usr/bin/echo and the dynamic linker:
        libc::SYS_faccessat,   // ld.so.preload check
        libc::SYS_newfstatat,  // ld.so.cache stat
        libc::SYS_prlimit64,   // glibc startup
        libc::SYS_getrandom,   // glibc stack canary
        libc::SYS_rseq,        // glibc restartable sequences
        libc::SYS_futex,       // used by glibc threading init
        // Fork+wait syscalls needed by ForkExecvp mode.
        libc::SYS_clone,       // fork() uses clone on Linux
        435_i64,               // clone3 (Linux 5.3+, x86_64 and aarch64)
        libc::SYS_wait4,       // waitpid() maps to wait4
        libc::SYS_waitid,      // alternate wait variant used by some libcs
        // execvpe search path syscalls: glibc's PATH search.
        262_i64,               // newfstatat/fstatat64 x86_64=262
        79_i64,                // newfstatat aarch64=79
        439_i64,               // faccessat2 (x86_64=439, aarch64=439)
        libc::SYS_getdents64,  // glibc PATH search reads directory entries
        434_i64,               // pidfd_open (Linux 5.3+): tokio process uses for child monitoring
    ];
    for nr in base_syscalls.iter().chain(ARCH_SPECIFIC_SYSCALLS.iter()) {
        rules.insert(*nr, vec![]);
    }
    let arch = std::env::consts::ARCH.try_into().map_err(|_| 6)?;
    // seccompiler 0.5 parameter order: new(rules, mismatch_action, match_action, arch).
    // Listed syscalls (in rules map) → Allow (match_action).
    // Everything else → Errno(EPERM) (mismatch_action).
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Errno(libc::EPERM as u32), // mismatch: unlisted syscalls → EPERM
        SeccompAction::Allow,                      // match: listed syscalls → Allow
        arch,
    )
    .map_err(|_| 6)?;
    let bpf: BpfProgram = filter.try_into().map_err(|_| 6)?;
    seccompiler::apply_filter(&bpf).map_err(|_| 7)?;
    Ok(())
}

/// Direct execve the target (replaces the current process).
#[cfg(target_os = "linux")]
fn do_execve_direct(target: &std::path::Path) -> ! {
    let target_c = CString::new(target.to_string_lossy().as_bytes()).unwrap();
    let argv0 = target_c.clone();
    let arg1 = CString::new("hello").unwrap();
    let argv_ptrs: Vec<*const libc::c_char> =
        vec![argv0.as_ptr(), arg1.as_ptr(), std::ptr::null()];
    let envp_ptrs: Vec<*const libc::c_char> = vec![std::ptr::null()];
    unsafe {
        libc::execve(target_c.as_ptr(), argv_ptrs.as_ptr(), envp_ptrs.as_ptr());
        let errno = *libc::__errno_location();
        let clamped = errno.clamp(0, 63);
        std::process::exit(64 + clamped);
    }
}

/// Fork a child, pass a PATH that includes /usr/local/bin, then execvpe(bare_cmd).
/// This reproduces the production failure: when PATH starts with /usr/local/bin,
/// glibc's execvp tries /usr/local/bin/echo first; landlock returns EACCES (not
/// ENOENT) because /usr/local/bin exists but lacks Execute permission in the
/// ruleset; glibc stops the search and returns EACCES. Adding /usr/local/bin to
/// BASELINE_SYSTEM_READ_PATHS gives it Execute permission → ENOENT → search
/// continues to /usr/bin/echo → success.
#[cfg(target_os = "linux")]
fn do_fork_execvp_with_path(bare_cmd: &str) -> ! {
    let cmd_c = CString::new(bare_cmd).unwrap();
    let arg1 = CString::new("hello").unwrap();
    let argv_ptrs: Vec<*const libc::c_char> =
        vec![cmd_c.as_ptr(), arg1.as_ptr(), std::ptr::null()];
    // Provide a PATH that includes /usr/local/bin BEFORE /usr/bin — this is
    // the typical glibc default PATH and matches what the shell plugin binary
    // receives from the tau-runtime host process.
    let path_val = CString::new("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin").unwrap();
    let envp_ptrs: Vec<*const libc::c_char> = vec![path_val.as_ptr(), std::ptr::null()];
    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            std::process::exit(32 + 9);
        }
        if pid == 0 {
            // Child: execvpe with PATH including /usr/local/bin first.
            libc::execvpe(cmd_c.as_ptr(), argv_ptrs.as_ptr(), envp_ptrs.as_ptr());
            let errno = *libc::__errno_location();
            let clamped = errno.clamp(0, 63);
            libc::_exit(64 + clamped);
        }
        // Parent: wait for child, propagate its exit code.
        let mut status: libc::c_int = 0;
        libc::waitpid(pid, &mut status, 0);
        if libc::WIFEXITED(status) {
            std::process::exit(libc::WEXITSTATUS(status));
        }
        std::process::exit(1);
    }
}

/// Fork a child, clear environment, then execvp(bare_cmd) in the child.
/// Mirrors shell plugin's run_subprocess("echo", ...) with env_clear().
/// The parent waits for the child and exits with the child's exit code.
#[cfg(target_os = "linux")]
fn do_fork_execvp(bare_cmd: &str) -> ! {
    let cmd_c = CString::new(bare_cmd).unwrap();
    let arg1 = CString::new("hello").unwrap();
    let argv_ptrs: Vec<*const libc::c_char> =
        vec![cmd_c.as_ptr(), arg1.as_ptr(), std::ptr::null()];
    // Clear environment (no PATH) — matches shell plugin's env_clear().
    let envp_ptrs: Vec<*const libc::c_char> = vec![std::ptr::null()];
    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            // fork failed
            std::process::exit(32 + 9);
        }
        if pid == 0 {
            // Child: execvp with cleared env
            // execvp searches PATH from the current env (cleared → no PATH →
            // glibc falls back to confstr(_CS_PATH) default path).
            libc::execvpe(cmd_c.as_ptr(), argv_ptrs.as_ptr(), envp_ptrs.as_ptr());
            let errno = *libc::__errno_location();
            let clamped = errno.clamp(0, 63);
            libc::_exit(64 + clamped);
        }
        // Parent: wait for child, propagate its exit code.
        let mut status: libc::c_int = 0;
        libc::waitpid(pid, &mut status, 0);
        if libc::WIFEXITED(status) {
            std::process::exit(libc::WEXITSTATUS(status));
        }
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg = parse_args(&args);

    let needs_bare_cmd = cfg.exec_mode == ExecMode::ForkExecvp
        || cfg.exec_mode == ExecMode::ForkExecvpWithPath;
    if cfg.target.as_os_str().is_empty() && !needs_bare_cmd {
        eprintln!("--target=<PATH> required");
        std::process::exit(32 + 8);
    }
    if needs_bare_cmd && cfg.bare_cmd.is_empty() {
        eprintln!("--bare-cmd=<NAME> required for fork-execvp modes");
        std::process::exit(32 + 8);
    }

    // --- ExecMode::UnshareFirst: apply unshare BEFORE landlock ---
    // Mirrors production strict.rs pre_exec order: unshare → uid_map → landlock → seccomp.
    if cfg.exec_mode == ExecMode::UnshareFirst {
        if cfg.unshare_user || cfg.unshare_net {
            if let Err(layer) = setup_unshare(&cfg) {
                std::process::exit(32 + layer);
            }
        }
        if cfg.landlock != LandlockMode::Off {
            if let Err(layer) = setup_landlock(&cfg) {
                std::process::exit(32 + layer);
            }
        }
        if cfg.seccomp {
            if let Err(layer) = setup_seccomp() {
                std::process::exit(32 + layer);
            }
        }
        do_execve_direct(&cfg.target);
    }

    // --- Default order: landlock → unshare → seccomp (original T4 harness) ---
    if cfg.landlock != LandlockMode::Off {
        if let Err(layer) = setup_landlock(&cfg) {
            std::process::exit(32 + layer);
        }
    }
    if cfg.unshare_user || cfg.unshare_net {
        if let Err(layer) = setup_unshare(&cfg) {
            std::process::exit(32 + layer);
        }
    }
    if cfg.seccomp {
        if let Err(layer) = setup_seccomp() {
            std::process::exit(32 + layer);
        }
    }

    // --- ExecMode::ForkExecvp: fork a child, execvp(bare_cmd) with no PATH ---
    if cfg.exec_mode == ExecMode::ForkExecvp {
        do_fork_execvp(&cfg.bare_cmd);
    }

    // --- ExecMode::ForkExecvpWithPath: fork, execvpe with /usr/local/bin in PATH ---
    // Reproduces production failure where glibc tries /usr/local/bin/echo first,
    // gets EACCES from landlock (Execute not granted), and stops the PATH search.
    if cfg.exec_mode == ExecMode::ForkExecvpWithPath {
        do_fork_execvp_with_path(&cfg.bare_cmd);
    }

    // --- ExecMode::Direct (default): execve the target directly ---
    do_execve_direct(&cfg.target);
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("landlock-exec-repro: Linux-only");
    std::process::exit(1);
}
