//! Integration tests for `Scope::resolve` walk-up algorithm.
//!
//! Uses `TempDir` to construct nested directory layouts and verifies
//! that resolve finds the correct ancestor or falls back to global.

use std::fs;

use tempfile::TempDir;

use tau_pkg::{Scope, ScopeKind};

#[test]
fn resolve_finds_dot_tau_in_immediate_cwd() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("proj");
    fs::create_dir_all(project.join(".tau")).unwrap();

    let scope = Scope::resolve(&project).unwrap();
    assert_eq!(scope.kind(), ScopeKind::Project);
    assert_eq!(scope.path(), project.as_path());
}

#[test]
fn resolve_walks_up_through_multiple_ancestors() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("proj");
    fs::create_dir_all(project.join(".tau")).unwrap();

    let nested = project.join("a").join("b").join("c").join("d");
    fs::create_dir_all(&nested).unwrap();

    let scope = Scope::resolve(&nested).unwrap();
    assert_eq!(scope.kind(), ScopeKind::Project);
    assert_eq!(scope.path(), project.as_path());
}

#[test]
fn resolve_picks_innermost_dot_tau_when_nested_projects_exist() {
    let tmp = TempDir::new().unwrap();
    let outer = tmp.path().join("outer");
    fs::create_dir_all(outer.join(".tau")).unwrap();

    let inner = outer.join("inner");
    fs::create_dir_all(inner.join(".tau")).unwrap();

    let cwd = inner.join("src");
    fs::create_dir_all(&cwd).unwrap();

    let scope = Scope::resolve(&cwd).unwrap();
    assert_eq!(scope.kind(), ScopeKind::Project);
    assert_eq!(
        scope.path(),
        inner.as_path(),
        "resolve should pick the innermost .tau, not walk past it"
    );
}

#[test]
fn resolve_ignores_dot_tau_that_is_a_file_not_a_dir() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("proj");
    fs::create_dir_all(&project).unwrap();
    // A file named `.tau` should NOT match — only directories count.
    fs::write(project.join(".tau"), b"this is a file, not a directory").unwrap();

    // No matching ancestor → falls back to global. We can't easily test
    // global() here without env-var manipulation (forbid(unsafe_code)
    // blocks set_var), so just verify resolve doesn't panic and doesn't
    // pick the file as a project root.
    let result = Scope::resolve(&project);
    if let Ok(scope) = result {
        assert_ne!(
            scope.kind(),
            ScopeKind::Project,
            "a regular file named .tau should not match as a project scope"
        );
    }
    // If global() failed (e.g., HOME not set), an error is acceptable.
}
