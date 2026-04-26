//! Package manifest types.
//!
//! Manifest deserialization (TOML/JSON) lives in `tau-pkg`; this module
//! owns only the data type, the typestate around validation, and serde
//! derives (under the `serde` feature).

use crate::id::PackageName;
use crate::version::{Version, VersionReq};

/// A dependency declaration: a package name plus a SemVer requirement.
///
/// # Example
///
/// ```ignore
/// // Construction is performed inside `tau-domain` (e.g. by manifest
/// // validation in tau-pkg). External crates receive `PackageDep` values;
/// // they cannot be built via struct expression because the type is
/// // `#[non_exhaustive]`.
/// use tau_domain::{PackageDep, PackageName, VersionReq};
/// use std::str::FromStr;
///
/// let dep = PackageDep {
///     name: PackageName::from_str("fs-tools").unwrap(),
///     version_req: VersionReq::parse("^0.3").unwrap(),
/// };
/// assert_eq!(dep.name.as_str(), "fs-tools");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PackageDep {
    /// Dependency package name.
    pub name: PackageName,
    /// SemVer version requirement.
    pub version_req: VersionReq,
}

/// A canonical package identity: `(name, version)`.
///
/// # Example
///
/// ```ignore
/// // Like `PackageDep`, `PackageId` is `#[non_exhaustive]` and cannot be
/// // built via struct expression from outside `tau-domain`. External
/// // crates receive instances from manifest validation in tau-pkg.
/// use tau_domain::{PackageId, PackageName, Version};
/// use std::str::FromStr;
///
/// let id = PackageId {
///     name: PackageName::from_str("fs-tools").unwrap(),
///     version: Version::parse("0.3.0").unwrap(),
/// };
/// assert_eq!(id.name.as_str(), "fs-tools");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PackageId {
    /// Package name.
    pub name: PackageName,
    /// Package version.
    pub version: Version,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn package_id_eq_works() {
        let a = PackageId {
            name: PackageName::from_str("foo").unwrap(),
            version: Version::parse("1.2.3").unwrap(),
        };
        let b = PackageId {
            name: PackageName::from_str("foo").unwrap(),
            version: Version::parse("1.2.3").unwrap(),
        };
        assert_eq!(a, b);
    }
}
