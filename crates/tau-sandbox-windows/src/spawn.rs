//! Windows-side spawn integration.
//!
//! `std::process::Command` on Windows uses `CreateProcessW` internally and
//! does not expose a `pre_exec` hook (that's Unix-only). To wrap the
//! spawn with `CreateProcessAsUserW` + `STARTUPINFOEXW` carrying
//! `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`, the adapter would need
//! to either:
//!
//! 1. Replace `Command::spawn` semantics (impossible without a fork of
//!    std::process::Command).
//! 2. Use the lower-level `windows` crate calls directly and bypass
//!    `Command` entirely.
//!
//! For Phase 1 (this PR) we register the AppContainer SID + capabilities
//! with the runtime via a per-Command env-var marker. The plugin host's
//! spawn path (in `tau-runtime::plugin_host::process`) is the natural
//! place to consume those markers and call `CreateProcessAsUserW`
//! instead of `Command::spawn`. That integration is **deferred** to a
//! follow-up: this crate ships the AppContainer profile + ACL grant +
//! tear-down infrastructure; the spawn-side wiring lands in a
//! companion PR once the runtime side is refactored to support
//! per-adapter spawn customisation.
//!
//! Until that lands, `wrap_spawn` returns `SandboxError::WrapFailed`
//! (with a clear message) for any plan beyond probe/validate. Tests for
//! the profile-generation + ACL paths run on Windows CI without
//! depending on the spawn integration.

use crate::acl::AppContainerSid;
use crate::profile::AppContainerCaps;

/// Register the given AppContainer SID + capabilities for the next spawn
/// of `cmd`. Implementation note above: actual `CreateProcessAsUserW`
/// integration is deferred to a runtime-side follow-up.
pub(crate) fn register_appcontainer_for_command(
    _cmd: &mut std::process::Command,
    _sid: &AppContainerSid,
    _caps: &AppContainerCaps,
) {
    // Phase 1 stub: profile + ACLs are set up at the OS level by lib.rs;
    // the spawn-side integration is the next iteration. For now the
    // adapter exists, validate_plan + probe work, and the SandboxHandle
    // teardown still runs (revoke ACLs + delete profile). Tests for the
    // pure-logic + ACL paths cover what's wired here.
}
