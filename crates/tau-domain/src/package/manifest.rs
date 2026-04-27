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
pub enum PackageKind {
    /// A package kind not yet typed in core.
    /// See: [escape-hatches.md#packagekind-custom](../../../../../docs/explanation/escape-hatches.md#packagekind-custom).
    Custom {
        /// The kind name. By convention one of [`crate::package::kinds`]'s
        /// constants (e.g. `"llm-backend"`, `"tool"`).
        kind: String,
    },
}

#[cfg(feature = "serde")]
impl serde::Serialize for PackageKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            PackageKind::Custom { kind } => serializer.serialize_str(kind),
        }
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for PackageKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = PackageKind;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a package kind string (e.g. \"tool\", \"llm-backend\")")
            }
            fn visit_str<E>(self, v: &str) -> Result<PackageKind, E>
            where
                E: serde::de::Error,
            {
                if v.is_empty() {
                    return Err(E::custom("package kind cannot be empty"));
                }
                Ok(PackageKind::Custom { kind: v.to_owned() })
            }
            fn visit_string<E>(self, v: String) -> Result<PackageKind, E>
            where
                E: serde::de::Error,
            {
                if v.is_empty() {
                    return Err(E::custom("package kind cannot be empty"));
                }
                Ok(PackageKind::Custom { kind: v })
            }
        }
        deserializer.deserialize_str(Visitor)
    }
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

use crate::error::PackageManifestError;
use crate::package::capability::Capability as Cap;

impl UncheckedManifest {
    /// Run cross-field validation. Returns the validated manifest on
    /// success.
    ///
    /// Field types are already validated at construction (`PackageName`,
    /// `PackageSource`, etc.); this checks invariants those types
    /// can't enforce alone (non-empty description, non-empty Custom
    /// capability names, etc.).
    ///
    /// # Example
    ///
    /// ```ignore
    /// // `UncheckedManifest` is `#[non_exhaustive]`, so it cannot be
    /// // built via struct expression from outside `tau-domain`. In
    /// // practice, callers obtain one by deserializing a manifest file
    /// // (in tau-pkg) and then call `.validate()`.
    /// use tau_domain::{UncheckedManifest, PackageManifestError};
    /// // let err = unchecked.validate().unwrap_err();
    /// // assert_eq!(err, PackageManifestError::EmptyDescription);
    /// # let _ = std::any::type_name::<UncheckedManifest>();
    /// # let _ = std::any::type_name::<PackageManifestError>();
    /// ```
    pub fn validate(self) -> Result<PackageManifest, PackageManifestError> {
        if self.description.is_empty() {
            return Err(PackageManifestError::EmptyDescription);
        }
        // dependency names are already PackageName values (pre-validated),
        // but the loop is here as a hook for future per-dep invariants
        // (e.g. duplicate-name detection, version-range cross-checks).
        // The `index` is kept so it can be threaded into
        // `PackageManifestError::DependencyName { index, source }`.
        #[allow(clippy::unused_enumerate_index)]
        for (_index, _dep) in self.dependencies.iter().enumerate() {
            // no-op at v0.1
        }
        for (i, cap) in self.capabilities.iter().enumerate() {
            if let Cap::Custom { name, .. } = cap {
                if name.is_empty() {
                    return Err(PackageManifestError::CapabilityEmptyName { index: i });
                }
            }
        }
        Ok(PackageManifest::from_checked(self))
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::str::FromStr;

    fn good() -> UncheckedManifest {
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
    fn good_manifest_validates() {
        let m = good().validate().unwrap();
        assert_eq!(m.name().as_str(), "fs-tools");
    }

    #[test]
    fn empty_description_rejected() {
        let mut u = good();
        u.description = String::new();
        assert_eq!(
            u.validate().unwrap_err(),
            PackageManifestError::EmptyDescription
        );
    }

    #[test]
    fn empty_custom_capability_name_rejected() {
        let mut u = good();
        u.capabilities = vec![Cap::Custom {
            name: String::new(),
            params: BTreeMap::new(),
        }];
        let err = u.validate().unwrap_err();
        assert_eq!(err, PackageManifestError::CapabilityEmptyName { index: 0 });
    }
}
