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
    let read_paths = collect_paths(plan, |c| match c {
        Capability::Filesystem(FsCapability::Read { paths }) => Some(paths.clone()),
        _ => None,
    });
    let write_paths = collect_paths(plan, |c| match c {
        Capability::Filesystem(FsCapability::Write { paths, .. }) => Some(paths.clone()),
        _ => None,
    });

    // Resolve glob anchors (`${PROJECT}/...`) to absolute paths.
    // Prefer the CWD set on the Command (cmd.current_dir) so that callers
    // who redirect the child's working directory get correct anchor
    // resolution; fall back to the parent's CWD only when not set.
    let cwd = cmd
        .get_current_dir()
        .map(std::path::Path::to_path_buf)
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)
        .map_err(|e| SandboxError::WrapFailed {
            message: format!("cwd: {e}"),
        })?;
    let read_paths = resolve_anchors(&read_paths, &cwd);
    let write_paths = resolve_anchors(&write_paths, &cwd);

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
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
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
    use landlock::{AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, ABI};

    let abi = ABI::V1;

    let mut ruleset = Ruleset::default()
        .handle_access(AccessFs::from_all(abi))?
        .create()?;

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
