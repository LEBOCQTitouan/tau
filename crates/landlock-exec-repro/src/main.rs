//! Minimal repro for sub-project E (per-command exec gating).
//!
//! Standalone Linux-only binary that mirrors tau-sandbox-native's pre_exec
//! sequence on a stripped-down skeleton, then calls execve directly. Used
//! to identify which landlock + namespace + seccomp configuration causes
//! execve(/usr/bin/echo) to return EACCES under the strict tier.
//!
//! Exit codes:
//!   0           — unreachable (execve replaces the process)
//!   32 + L      — setup failed at layer L (0=arg, 1=landlock build, 2=create,
//!                 3=add rules, 4=restrict_self, 5=unshare, 6=seccomp compile,
//!                 7=seccomp apply)
//!   64 + errno  — execve failed with errno (clamped to 127)
//!
//! See docs/superpowers/specs/2026-05-11-sub-project-e-exec-gating-design.md
//! for the diagnostic matrix and methodology.

#[cfg(target_os = "linux")]
fn main() {
    eprintln!("landlock-exec-repro: not yet implemented");
    std::process::exit(32);
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("landlock-exec-repro: Linux-only");
    std::process::exit(1);
}
