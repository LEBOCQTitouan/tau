//! Integration test: `PackageSource` grammar across known cases.
//!
//! Hand-picked cases complement the proptest fuzzing in
//! `proptest_package_source.rs` by pinning behaviour at boundary inputs
//! (empty rev marker, unsupported scheme, scp without user).

use std::str::FromStr;

use tau_domain::{GitLocation, PackageSource, PackageSourceError};

#[test]
fn https_no_rev() {
    let s = PackageSource::from_str("https://example.com/r.git").unwrap();
    let PackageSource::Git {
        location: GitLocation::Url(u),
        rev,
    } = s
    else {
        panic!("expected Git/Url variant");
    };
    assert_eq!(u.scheme(), "https");
    assert_eq!(rev, None);
}

#[test]
fn ssh_with_rev() {
    let s = PackageSource::from_str("ssh://git@example.com/r.git#main").unwrap();
    let PackageSource::Git { rev, .. } = s else {
        panic!("expected Git variant");
    };
    assert_eq!(rev.as_deref(), Some("main"));
}

#[test]
fn scp_no_user() {
    let s = PackageSource::from_str("example.com:r.git").unwrap();
    let PackageSource::Git {
        location: GitLocation::Scp { user, host, path },
        ..
    } = s
    else {
        panic!("expected Git/Scp variant");
    };
    assert!(user.is_none());
    assert_eq!(host, "example.com");
    assert_eq!(path, "r.git");
}

#[test]
fn rejects_ftp() {
    assert!(matches!(
        PackageSource::from_str("ftp://example.com/r.git"),
        Err(PackageSourceError::UnsupportedScheme { .. }),
    ));
}

#[test]
fn rejects_empty_rev_marker() {
    assert_eq!(
        PackageSource::from_str("https://example.com/r.git#"),
        Err(PackageSourceError::EmptyRevision),
    );
}
