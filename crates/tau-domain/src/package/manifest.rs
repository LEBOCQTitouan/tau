//! Package manifest types.
//!
//! Manifest deserialization (TOML/JSON) lives in `tau-pkg`; this module
//! owns only the data type, the typestate around validation, and serde
//! derives (under the `serde` feature).

use crate::id::PackageName;
use crate::package::capability::Capability;
use crate::package::source::PackageSource;
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

/// Package kind. Structural at v0.1: every kind goes through `Custom`.
/// Typed variants land additively as tau-runtime gains plugin trait
/// awareness for each kind.
///
/// See: [escape-hatches.md#packagekind-custom](../../../../../docs/explanation/escape-hatches.md#packagekind-custom).
///
/// # Example
///
/// ```ignore
/// // `PackageKind` is `#[non_exhaustive]` and cannot be built via struct
/// // expression from outside `tau-domain`. Construction is performed
/// // inside the crate (e.g. by manifest validation in tau-pkg).
/// use tau_domain::PackageKind;
/// let k = PackageKind::Custom { kind: "tool".into() };
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PackageKind {
    /// A package kind not yet typed in core.
    /// See: [escape-hatches.md#packagekind-custom](../../../../../docs/explanation/escape-hatches.md#packagekind-custom).
    Custom {
        /// The kind name. By convention one of [`crate::package::kinds`]'s
        /// constants (e.g. `"llm-backend"`, `"tool"`).
        kind: String,
    },
}

/// Canonical kind strings for `PackageKind::Custom.kind` and manifest
/// `kind` fields. Recommended convention; tau-domain validates only
/// "non-empty" so plugin authors who want a non-conforming kind name
/// can use `Custom` with arbitrary text.
pub mod kinds {
    /// LLM backend plugin kind.
    pub const LLM_BACKEND: &str = "llm-backend";
    /// Tool plugin kind.
    pub const TOOL: &str = "tool";
    /// Skill plugin kind.
    pub const SKILL: &str = "skill";
    /// Pipeline plugin kind.
    pub const PIPELINE: &str = "pipeline";
    /// MCP server plugin kind.
    pub const MCP_SERVER: &str = "mcp-server";
    /// Storage plugin kind.
    pub const STORAGE: &str = "storage";
    /// Sandbox plugin kind.
    pub const SANDBOX: &str = "sandbox";
}

/// Raw manifest as it appears on disk or on the wire. Deserializes from
/// TOML/JSON directly. May contain field combinations that violate
/// cross-field invariants — call [`UncheckedManifest::validate`] to
/// obtain a verified [`PackageManifest`].
///
/// # Example
///
/// ```no_run
/// use tau_domain::UncheckedManifest;
/// // toml::from_str::<UncheckedManifest>(&raw)?.validate()?;
/// # let _ = std::any::type_name::<UncheckedManifest>();
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UncheckedManifest {
    /// Package name.
    pub name: PackageName,
    /// Package version.
    pub version: Version,
    /// Free-form description.
    pub description: String,
    /// Authors (free-form, e.g. `"Acme Inc <support@acme.dev>"`).
    pub authors: Vec<String>,
    /// SPDX license expression as opaque text. `None` for unlicensed.
    pub license: Option<String>,
    /// Where the package lives.
    pub source: PackageSource,
    /// What the package provides.
    pub kind: PackageKind,
    /// Required dependencies.
    pub dependencies: Vec<PackageDep>,
    /// Capability declarations (G14).
    pub capabilities: Vec<Capability>,
}

/// Validated package manifest. By construction, satisfies all cross-field
/// invariants enforced by [`UncheckedManifest::validate`]. Cannot be
/// constructed directly — must go through validation.
///
/// To mutate a `PackageManifest`, downgrade via `Into<UncheckedManifest>`,
/// edit, then call [`UncheckedManifest::validate`] again.
///
/// # Example
///
/// ```no_run
/// use tau_domain::{UncheckedManifest, PackageManifest};
/// // let manifest: PackageManifest = unchecked.validate()?;
/// # let _ = std::any::type_name::<PackageManifest>();
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct PackageManifest(UncheckedManifest);

impl PackageManifest {
    /// Package name.
    pub fn name(&self) -> &PackageName {
        &self.0.name
    }
    /// Package version.
    pub fn version(&self) -> &Version {
        &self.0.version
    }
    /// Free-form description.
    pub fn description(&self) -> &str {
        &self.0.description
    }
    /// Authors.
    pub fn authors(&self) -> &[String] {
        &self.0.authors
    }
    /// SPDX license expression, if present.
    pub fn license(&self) -> Option<&str> {
        self.0.license.as_deref()
    }
    /// Source location.
    pub fn source(&self) -> &PackageSource {
        &self.0.source
    }
    /// Package kind.
    pub fn kind(&self) -> &PackageKind {
        &self.0.kind
    }
    /// Required dependencies.
    pub fn dependencies(&self) -> &[PackageDep] {
        &self.0.dependencies
    }
    /// Capability declarations.
    pub fn capabilities(&self) -> &[Capability] {
        &self.0.capabilities
    }

    /// Wrap a checked `UncheckedManifest` without re-running validation.
    /// Internal use only — public API must go through
    /// [`UncheckedManifest::validate`].
    #[allow(dead_code)] // used by validate() landing in Task 13 and by tests below
    pub(crate) fn from_checked(u: UncheckedManifest) -> Self {
        Self(u)
    }
}

impl From<PackageManifest> for UncheckedManifest {
    fn from(m: PackageManifest) -> Self {
        m.0
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for PackageManifest {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
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

#[cfg(test)]
mod manifest_tests {
    use super::*;
    use std::str::FromStr;

    fn fixture() -> UncheckedManifest {
        UncheckedManifest {
            name: PackageName::from_str("fs-tools").unwrap(),
            version: Version::parse("0.3.0").unwrap(),
            description: "fs tools".into(),
            authors: vec![],
            license: None,
            source: PackageSource::from_str("https://example.com/fs.git").unwrap(),
            kind: PackageKind::Custom {
                kind: "tool".into(),
            },
            dependencies: vec![],
            capabilities: vec![],
        }
    }

    #[test]
    fn package_manifest_accessors_work() {
        let m = PackageManifest::from_checked(fixture());
        assert_eq!(m.name().as_str(), "fs-tools");
        assert_eq!(m.description(), "fs tools");
        assert_eq!(m.dependencies().len(), 0);
    }

    #[test]
    fn round_trip_through_unchecked() {
        let m = PackageManifest::from_checked(fixture());
        let u: UncheckedManifest = m.into();
        let m2 = PackageManifest::from_checked(u);
        assert_eq!(m2.name().as_str(), "fs-tools");
    }
}
