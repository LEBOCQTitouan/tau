//! Per-command exec gating helpers for the Strict sandbox tier.
//!
//! # v0.1 status — exec gating is a stub
//!
//! True per-command argument-filter exec gating (comparing `execve`'s path
//! argument against an allow-list) is **deferred** from v0.1. The reasons are:
//!
//! 1. **Plugin-startup chicken-and-egg.** The seccomp filter is installed in
//!    `pre_exec` — before the kernel runs `execve(<plugin binary>)`. If we
//!    deny `execve` at the seccomp layer, the plugin can never start. The
//!    filter must always allow `execve` so the initial spawn succeeds.
//!
//! 2. **Pointer-vs-content.** seccomp `SeccompCondition::Arg(0, ...)` compares
//!    the *pointer value* of `execve`'s `path` argument, not the string
//!    contents the pointer addresses. The kernel does not dereference user-space
//!    pointers in BPF. A per-path allow-list would require address-space layout
//!    knowledge the filter cannot have.
//!
//! 3. **Landlock V2 path-exec.** The correct long-term solution is to grant
//!    `AccessFs::Execute` (landlock V2, kernel >= 5.19) on the allowed binary
//!    paths and rely on the filesystem layer to gate execution, not seccomp arg
//!    matching. landlock V2 wiring is deferred to a future sub-project.
//!
//! # What this module does today
//!
//! `extend_with_exec_rules` is a **no-op stub**. The baseline allow-list
//! (built in `strict::baseline_syscall_map`) already unconditionally permits
//! `execve` and `execveat` — necessary for plugin startup. This function is a
//! placeholder for the future tightening work described below.
//!
//! # TODO(future) — per-command exec tightening
//!
//! - Wire landlock V2 `AccessFs::Execute` for `Capability::Filesystem(Exec { paths })`
//!   and `Capability::Process(Spawn { commands })`. Kernel >= 5.19 required; fall back
//!   gracefully on older kernels.
//! - Investigate `seccomp notify` (Linux 5.0+) as an alternative to BPF arg matching:
//!   a supervisor process receives a notification on each `execve` call and can
//!   inspect the path before deciding allow/deny.
//! - Consider a fork-server pattern to install seccomp *after* the initial exec,
//!   which would make in-BPF path-arg comparison feasible for subsequent execs.

use std::collections::BTreeMap;

use seccompiler::SeccompRule;
use tau_ports::SandboxPlan;

/// Extend the seccomp rules map with exec-gating rules for the given plan.
///
/// # v0.1 — stub
///
/// This function is currently a **no-op**. `execve` and `execveat` remain in
/// the allow-list unconditionally (as set by `baseline_syscall_map`). See the
/// module-level documentation for the full rationale and the deferred work
/// items.
///
/// # Arguments
///
/// - `rules` — mutable reference to the rules map built by `baseline_syscall_map`.
/// - `plan` — the sandbox plan whose capabilities describe which exec operations
///   the plugin is allowed to perform (informational only in v0.1).
#[allow(unused_variables)]
pub(crate) fn extend_with_exec_rules(
    rules: &mut BTreeMap<i64, Vec<SeccompRule>>,
    plan: &SandboxPlan,
) {
    // v0.1: no-op. See module doc for the full rationale.
    //
    // Future: inspect plan.capabilities for:
    //   Capability::Process(ProcessCapability::Spawn { commands })
    //   Capability::Filesystem(FsCapability::Exec { paths })
    // and wire landlock V2 Execute + seccomp-notify accordingly.
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;
    use crate::strict::baseline_syscall_map;

    /// The v0.1 exec extension must be a no-op: the rules map is identical
    /// before and after the call, regardless of what capabilities the plan has.
    #[test]
    fn exec_extension_is_noop_for_v01() {
        let plan_json = serde_json::json!({
            "capabilities": [
                { "kind": "fs.read", "paths": ["/tmp"] },
                { "kind": "process.spawn", "commands": ["git"] },
                { "kind": "fs.exec", "paths": ["/usr/bin/git"] },
            ],
            "context": null,
            "limits": null,
        });
        let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");

        let mut rules_before = baseline_syscall_map();
        let snapshot_before: Vec<i64> = rules_before.keys().copied().collect();

        extend_with_exec_rules(&mut rules_before, &plan);

        let snapshot_after: Vec<i64> = rules_before.keys().copied().collect();
        assert_eq!(
            snapshot_before, snapshot_after,
            "extend_with_exec_rules must be a no-op in v0.1 — rules map changed"
        );
    }

    /// Baseline map must still contain execve after the stub runs, so the
    /// initial plugin exec succeeds.
    #[test]
    fn execve_present_after_extension_with_no_caps() {
        let plan_json = serde_json::json!({
            "capabilities": [],
            "context": null,
            "limits": null,
        });
        let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");

        let mut rules = baseline_syscall_map();
        extend_with_exec_rules(&mut rules, &plan);

        assert!(
            rules.contains_key(&libc::SYS_execve),
            "SYS_execve must remain in allow-list after extend_with_exec_rules (needed for plugin startup)"
        );
    }

    /// Baseline map must still contain execve after the stub runs when a
    /// Process(Spawn) capability is present.
    #[test]
    fn execve_present_after_extension_with_process_spawn() {
        let plan_json = serde_json::json!({
            "capabilities": [
                { "kind": "process.spawn", "commands": ["echo"] },
            ],
            "context": null,
            "limits": null,
        });
        let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");

        let mut rules = baseline_syscall_map();
        extend_with_exec_rules(&mut rules, &plan);

        assert!(
            rules.contains_key(&libc::SYS_execve),
            "SYS_execve must remain in allow-list with Process(Spawn) capability"
        );
    }

    /// Baseline map must still contain execve after the stub runs when a
    /// Filesystem(Exec) capability is present.
    #[test]
    fn execve_present_after_extension_with_fs_exec() {
        let plan_json = serde_json::json!({
            "capabilities": [
                { "kind": "fs.exec", "paths": ["/usr/bin/git"] },
            ],
            "context": null,
            "limits": null,
        });
        let plan: SandboxPlan = serde_json::from_value(plan_json).expect("valid plan");

        let mut rules = baseline_syscall_map();
        extend_with_exec_rules(&mut rules, &plan);

        assert!(
            rules.contains_key(&libc::SYS_execve),
            "SYS_execve must remain in allow-list with Filesystem(Exec) capability"
        );
    }
}
