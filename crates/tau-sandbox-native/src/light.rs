//! Light-tier enforcement: landlock filesystem isolation only.
//!
//! A `pre_exec` hook installs a landlock V1 ruleset in the child process
//! after `fork(2)` but before `exec(2)`. Installing in the parent would lock
//! down the tau process itself; `pre_exec` targets the child only.

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use tau_domain::{Capability, FsCapability};
use tau_ports::{SandboxError, SandboxHandle, SandboxPlan};

/// Collect and resolve landlock path lists from a plan + command CWD.
///
/// Returns `(read_paths, write_paths)` as resolved `PathBuf` vectors.
/// Shared by `apply_landlock` (Light tier) and `strict::apply_strict` (Strict tier).
pub(crate) fn collect_landlock_paths(
    plan: &SandboxPlan,
    cmd: &Command,
) -> Result<(Vec<std::path::PathBuf>, Vec<std::path::PathBuf>), SandboxError> {
    let read_strs = collect_paths(plan, |c| match c {
        Capability::Filesystem(FsCapability::Read { paths, .. }) => Some(paths.clone()),
        _ => None,
    });
    let write_strs = collect_paths(plan, |c| match c {
        Capability::Filesystem(FsCapability::Write { paths, .. }) => Some(paths.clone()),
        _ => None,
    });

    let cwd = cmd
        .get_current_dir()
        .map(std::path::Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)
        .map_err(|e| SandboxError::WrapFailed {
            message: format!("cwd: {e}"),
        })?;

    Ok((
        resolve_anchors(&read_strs, &cwd),
        resolve_anchors(&write_strs, &cwd),
    ))
}

/// Install a landlock ruleset from pre-resolved path lists.
///
/// Called from inside a `pre_exec` closure (in the child between fork and exec).
/// Exposed as `pub(crate)` so `strict::apply_strict` can reuse it.
pub(crate) fn install_landlock_from_plan(
    read_paths: &[std::path::PathBuf],
    write_paths: &[std::path::PathBuf],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    install_landlock(read_paths, write_paths)
}

/// Apply landlock rules to `cmd` via a `pre_exec` hook.
///
/// Rules are derived from the plan's filesystem capabilities. Non-filesystem
/// capabilities are accepted (they pass `validate_plan`) but not yet enforced
/// at Light tier; Strict tier (Tasks 4-5) will add seccomp + namespaces.
///
/// Returns [`SandboxHandle::noop`]: landlock state is per-thread inside the
/// child process and dies with the child on `_exit`; no parent-side
/// cleanup is needed.
pub(crate) fn apply_landlock(
    plan: &SandboxPlan,
    cmd: &mut Command,
) -> Result<SandboxHandle, SandboxError> {
    let (read_paths, write_paths) = collect_landlock_paths(plan, cmd)?;

    // KNOWN-LIMITATION: this pre_exec closure runs in the child between fork
    // and exec. POSIX guarantees only async-signal-safe operations in that
    // state. tau is multi-threaded (tokio), so any malloc held by another
    // thread at fork time can deadlock the child. The landlock builder
    // allocates internally and we format error strings on the failure path,
    // neither of which is async-signal-safe. Glibc's malloc holds for
    // microseconds; the deadlock window is small but nonzero. A future task
    // (Task 4 or a dedicated follow-up) should consider posix_spawn or a
    // fork-server pattern.
    //
    // SAFETY: pre_exec runs in the child after fork() but before exec().
    // Installing a landlock ruleset here is safe. The parent process is
    // unaffected.
    unsafe {
        cmd.pre_exec(move || {
            install_landlock(&read_paths, &write_paths)
                .map_err(|e| std::io::Error::other(e.to_string()))
        });
    }
    Ok(SandboxHandle::noop())
}

fn collect_paths<F>(plan: &SandboxPlan, extract: F) -> Vec<String>
where
    F: Fn(&Capability) -> Option<Vec<String>>,
{
    plan.capabilities
        .iter()
        .filter_map(extract)
        .flatten()
        .collect()
}

fn resolve_anchors(paths: &[String], cwd: &std::path::Path) -> Vec<PathBuf> {
    paths
        .iter()
        .map(|p| {
            let p = p.replace("${PROJECT}", cwd.to_string_lossy().as_ref());
            // Drop trailing glob suffix; landlock works on directory roots.
            // For "/tmp/**" we add "/tmp"; for "/tmp/x" we add "/tmp/x".
            let trimmed = p.trim_end_matches("/**").trim_end_matches("/*").to_string();
            PathBuf::from(trimmed)
        })
        .collect()
}

fn install_landlock(
    read_paths: &[PathBuf],
    write_paths: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use landlock::make_bitflags;
    use landlock::{
        Access, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, ABI,
    };

    let abi = ABI::V1;

    let mut ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))?
        .create()?;

    // Always allow read access to system binary + library paths so the
    // child can `execve` itself and load shared libraries. Without this,
    // landlock blocks the binary's own load and exec returns EACCES.
    // The user's plan-derived read_paths still narrow application access;
    // these system paths exist purely so the runtime mechanics work.
    let system_read_paths: &[&str] = &[
        "/bin",
        "/sbin",
        "/usr/bin",
        "/usr/sbin",
        "/lib",
        "/lib64",
        "/usr/lib",
        "/usr/lib64",
        "/etc",
    ];
    for sys_path in system_read_paths {
        if let Ok(fd) = PathFd::new(sys_path) {
            ruleset = ruleset.add_rule(PathBeneath::new(
                fd,
                make_bitflags!(AccessFs::{ReadFile | ReadDir}),
            ))?;
        }
        // Silently skip paths that don't exist on this system.
    }

    for p in read_paths {
        let fd = PathFd::new(p).map_err(|e| format!("landlock read path {}: {e}", p.display()))?;
        ruleset = ruleset.add_rule(PathBeneath::new(
            fd,
            make_bitflags!(AccessFs::{ReadFile | ReadDir}),
        ))?;
    }

    for p in write_paths {
        let fd = PathFd::new(p).map_err(|e| format!("landlock write path {}: {e}", p.display()))?;
        ruleset = ruleset.add_rule(PathBeneath::new(
            fd,
            make_bitflags!(AccessFs::{WriteFile | MakeReg | RemoveFile}),
        ))?;
    }

    ruleset.restrict_self()?;
    Ok(())
}
