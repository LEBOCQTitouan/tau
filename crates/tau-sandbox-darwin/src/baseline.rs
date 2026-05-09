//! Baseline SBPL allowlist.
//!
//! Every macOS process needs a minimum set of system path reads + Mach
//! lookups to bootstrap libc + dyld. Without these, even `/bin/echo` fails
//! to start under `(deny default)`.
//!
//! These rules were discovered empirically by running progressively-stricter
//! profiles against `/bin/echo` and adding rules until exec succeeded.
//!
//! Lock-step with macOS releases: if a new macOS version moves a system
//! library, integration tests will fail loudly and we update the baseline.

/// Minimum SBPL rule block plugins need before any plan-derived rules.
///
/// Notes on each block:
/// - `process*` allows the plugin to fork/exec children. sandbox-exec needs
///   `process-exec` to launch the plugin itself; `process-fork` for any
///   sub-shells the plugin spawns; etc. Children inherit the same profile,
///   so this doesn't widen the sandbox.
/// - `mach-lookup` is the single most important allow rule — Mach is
///   macOS's IPC primitive; libc bootstrap fails immediately if it can't
///   look up the bootstrap server.
/// - `sysctl-read` is needed by libc to determine CPU/RAM/uname info.
/// - `signal (target self)` lets the plugin signal itself (panic handler).
/// - `file-read*` over the dyld + libc paths covers the dynamic linker's
///   needs.
/// - `/private/tmp` is a symlink target on macOS that resolves to /tmp;
///   include both forms so the plugin can write tempfiles.
pub const SBPL_BASELINE: &str = r#"
;; ---- baseline: bootstrap libc + dyld ----
(allow process*)
(allow signal (target self))
(allow sysctl-read)
(allow mach-lookup)
(allow ipc-posix-shm)
(allow iokit-open)
(allow file-ioctl)

;; dyld + libc system paths
(allow file-read*
  (subpath "/usr/lib")
  (subpath "/usr/share")
  (subpath "/usr/libexec")
  (subpath "/System/Library")
  (subpath "/Library/Apple/System/Library")
  (subpath "/Library/Apple/usr"))

(allow file-read-data
  (literal "/")
  (literal "/private")
  (literal "/private/etc")
  (literal "/private/etc/localtime")
  (literal "/private/etc/hosts")
  (literal "/private/etc/resolv.conf")
  (literal "/dev/null")
  (literal "/dev/random")
  (literal "/dev/urandom")
  (literal "/dev/dtracehelper"))

(allow file-read-metadata)
"#;
