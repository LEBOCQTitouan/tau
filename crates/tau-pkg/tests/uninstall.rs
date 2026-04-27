//! Install-then-uninstall state verification.

mod fixtures;

use std::str::FromStr;

use tempfile::TempDir;

use tau_domain::PackageSource;
use tau_pkg::{install, list, uninstall, Scope};

#[test]
fn uninstall_removes_directory_and_lockfile_entry() {
    if !fixtures::git_available() {
        eprintln!("skipping: `git` not on PATH");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("tau-home");
    std::fs::create_dir_all(&project_root).unwrap();
    let scope = Scope::new_project(&project_root).unwrap();

    let bare = fixtures::make_fixture_repo(tmp.path(), "acme-tool", "1.0.0", "tool");
    let source = PackageSource::from_str(&fixtures::file_url(&bare)).unwrap();

    let installed = install(&source, &scope).unwrap();
    assert!(installed.installed_path.is_dir());

    let name: tau_domain::PackageName = "acme-tool".parse().unwrap();
    uninstall(&name, None, &scope).unwrap();

    assert!(
        !installed.installed_path.exists(),
        "package dir should be gone"
    );
    assert!(
        list(&scope).unwrap().is_empty(),
        "lockfile entry should be removed"
    );
}

#[test]
fn uninstall_returns_not_installed_when_package_unknown() {
    let tmp = TempDir::new().unwrap();
    let project_root = tmp.path().join("tau-home");
    std::fs::create_dir_all(&project_root).unwrap();
    let scope = Scope::new_project(&project_root).unwrap();

    let name: tau_domain::PackageName = "ghost-pkg".parse().unwrap();
    let err = uninstall(&name, None, &scope).unwrap_err();
    assert!(matches!(err, tau_pkg::UninstallError::NotInstalled { .. }));
}
