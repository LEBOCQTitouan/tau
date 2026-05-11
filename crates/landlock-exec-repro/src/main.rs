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
    use landlock::{AccessFs, BitFlags};
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
    // Minimal allow-list: execve + everything libc needs to get to it after
    // landlock + unshare. We don't need a full production filter; the goal
    // is just to install A seccomp filter to mirror the strict-tier shape.
    let mut rules: BTreeMap<i64, Vec<seccompiler::SeccompRule>> = BTreeMap::new();
    for nr in [
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
        libc::SYS_arch_prctl,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_openat,
        libc::SYS_read,
        libc::SYS_readlinkat,
        libc::SYS_fstat,
    ] {
        rules.insert(nr, vec![]);
    }
    let arch = std::env::consts::ARCH.try_into().map_err(|_| 6)?;
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow, // permissive — execve always allowed
        SeccompAction::Allow,
        arch,
    )
    .map_err(|_| 6)?;
    let bpf: BpfProgram = filter.try_into().map_err(|_| 6)?;
    seccompiler::apply_filter(&bpf).map_err(|_| 7)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cfg = parse_args(&args);

    if cfg.target.as_os_str().is_empty() {
        eprintln!("--target=<PATH> required");
        std::process::exit(32 + 8);
    }

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

    let target = CString::new(cfg.target.to_string_lossy().as_bytes()).unwrap();
    let argv0 = target.clone();
    let arg1 = CString::new("hello").unwrap();
    let argv_ptrs: Vec<*const libc::c_char> =
        vec![argv0.as_ptr(), arg1.as_ptr(), std::ptr::null()];
    let envp_ptrs: Vec<*const libc::c_char> = vec![std::ptr::null()];
    unsafe {
        libc::execve(target.as_ptr(), argv_ptrs.as_ptr(), envp_ptrs.as_ptr());
        let errno = *libc::__errno_location();
        // Clamp to 0..=63 so 64+errno fits in valid exit-code range (0..=127).
        let clamped = errno.clamp(0, 63);
        std::process::exit(64 + clamped);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("landlock-exec-repro: Linux-only");
    std::process::exit(1);
}
