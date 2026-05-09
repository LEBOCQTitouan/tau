//! Win32 AppContainer profile + ACL helpers.
//!
//! Windows-only (the entire module is `cfg(target_os = "windows")`-gated
//! at the lib level). Compiles + runs only on Windows.
//!
//! # Status: stub
//!
//! This module currently ships **stub implementations** that return
//! placeholder values without calling Win32. The real `CreateAppContainerProfile`
//! / `SetEntriesInAclW` / `SetNamedSecurityInfoW` integration is the
//! work of a follow-up PR (see the spec at
//! `docs/superpowers/specs/2026-05-09-sandbox-windows-design.md`):
//! the Phase 1 scaffold here ships the API shape + cfg-gating + lifecycle
//! plumbing so the runtime can wire it in; Phase 2 (next PR) replaces the
//! stubs with real Win32 calls + ships Layer 4 integration tests on
//! `windows-latest` CI.
//!
//! Until Phase 2 lands, `wrap_spawn` succeeds (returns a `SandboxHandle`
//! whose drop revokes nothing) but the plugin is **not actually
//! sandboxed** on Windows. The `Sandbox::probe` documents this by
//! returning a non-Strict tier when Phase 2 is missing.

/// Indicates which kind of access an ACL grant or revoke should target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AccessKind {
    /// File-read access on the path.
    #[allow(dead_code)] // exercised only on the Windows runtime path
    Read,
    /// File-read + file-write access on the path.
    #[allow(dead_code)] // exercised only on the Windows runtime path
    Write,
}

/// Owned wrapper around an AppContainer SID.
///
/// In the Phase 2 implementation this owns a Win32 PSID allocated by
/// `DeriveAppContainerSidFromAppContainerName`; here it just stores the
/// profile name so subsequent stub calls can identify the SID.
#[derive(Debug, Clone)]
pub(crate) struct AppContainerSid {
    #[allow(dead_code)] // Phase 2 will use this to format the SID for ACL ops
    pub(crate) profile_name: String,
}

/// Stub: return an `AppContainerSid` carrying just the profile name.
///
/// Phase 2 calls `CreateAppContainerProfile` and stores the resulting PSID.
pub(crate) fn create_appcontainer_profile(name: &str) -> std::io::Result<AppContainerSid> {
    Ok(AppContainerSid {
        profile_name: name.to_string(),
    })
}

/// Stub: no-op.
///
/// Phase 2 calls `DeleteAppContainerProfile`.
pub(crate) fn delete_appcontainer_profile(_name: &str) -> std::io::Result<()> {
    Ok(())
}

/// Stub: no-op.
///
/// Phase 2 calls `SetEntriesInAclW` + `SetNamedSecurityInfoW` to add a
/// `GRANT_ACCESS` entry on the path's DACL targeting the AppContainer SID.
pub(crate) fn grant_access(
    _sid: &AppContainerSid,
    _path: &str,
    _kind: AccessKind,
) -> std::io::Result<()> {
    Ok(())
}

/// Stub: no-op.
///
/// Phase 2 calls `SetEntriesInAclW` + `SetNamedSecurityInfoW` to remove
/// the entry added by [`grant_access`].
pub(crate) fn revoke_access(
    _sid: &AppContainerSid,
    _path: &str,
    _kind: AccessKind,
) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_returns_named_sid() {
        let sid = create_appcontainer_profile("tau-test").expect("create");
        assert_eq!(sid.profile_name, "tau-test");
    }

    #[test]
    fn delete_is_noop() {
        delete_appcontainer_profile("tau-test").expect("delete");
    }

    #[test]
    fn grant_revoke_are_noops() {
        let sid = create_appcontainer_profile("tau-test").unwrap();
        grant_access(&sid, "C:\\path", AccessKind::Read).expect("grant");
        revoke_access(&sid, "C:\\path", AccessKind::Read).expect("revoke");
    }
}
