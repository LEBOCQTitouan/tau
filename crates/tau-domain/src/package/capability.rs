//! Capability declarations attached to a package manifest.
//!
//! Hierarchical typed enum: top-level by namespace
//! (`Filesystem`/`Network`/`Process`/`Agent`/`Custom`), per-namespace
//! verb enums underneath. Variant-level `#[non_exhaustive]` permits
//! additive field evolution.
//!
//! Wire format per ADR-0002: manifest TOML uses flat dot-namespaced
//! `kind = "fs.read"` form. The custom `Deserialize` impl on
//! [`Capability`] maps it onto the variant tree.

use std::collections::BTreeMap;

use crate::value::Value;

/// A capability declaration.
///
/// # Example
///
/// ```ignore
/// // Variant-level `#[non_exhaustive]` blocks struct-expression
/// // construction from outside the crate, so this example is illustrative
/// // only. In practice, `Capability` is built by deserializing a manifest.
/// use tau_domain::{Capability, FsCapability};
/// let cap = Capability::Filesystem(FsCapability::Read {
///     paths: vec!["${PROJECT}/**".into()],
/// });
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum Capability {
    /// Filesystem-related capability.
    Filesystem(FsCapability),
    /// Network-related capability.
    Network(NetCapability),
    /// Process spawning / signaling capability.
    Process(ProcessCapability),
    /// Inter-agent capability.
    Agent(AgentCapability),
    /// Plugin-specific capability not yet typed in core.
    /// See: [escape-hatches.md#capability-custom](../../../../../docs/explanation/escape-hatches.md#capability-custom).
    Custom {
        /// Capability name (e.g. `"mcp.tool.use"`).
        name: String,
        /// Capability parameters.
        params: BTreeMap<String, Value>,
    },
}

/// Filesystem capability verbs.
///
/// # Example
///
/// ```ignore
/// use tau_domain::FsCapability;
/// let cap = FsCapability::Read { paths: vec!["/tmp/**".into()] };
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FsCapability {
    /// Read paths matching the given glob patterns.
    #[non_exhaustive]
    Read {
        /// Glob patterns to grant read access on.
        paths: Vec<String>,
    },
    /// Write paths matching the given globs (with optional size cap).
    #[non_exhaustive]
    Write {
        /// Glob patterns to grant write access on.
        paths: Vec<String>,
        /// Optional maximum write size, in bytes.
        max_bytes: Option<u64>,
    },
    /// Execute (spawn) binaries from paths matching the given globs.
    #[non_exhaustive]
    Exec {
        /// Glob patterns of binaries permitted to execute.
        paths: Vec<String>,
    },
}

/// Network capability verbs.
///
/// # Example
///
/// ```ignore
/// use tau_domain::NetCapability;
/// let cap = NetCapability::Http {
///     hosts: vec!["api.example.com".into()],
///     methods: vec!["GET".into()],
/// };
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum NetCapability {
    /// HTTP requests to the allow-listed hosts and methods.
    #[non_exhaustive]
    Http {
        /// Allowed hosts (exact match or glob).
        hosts: Vec<String>,
        /// Allowed HTTP methods (uppercase by convention, e.g. `["GET", "POST"]`).
        methods: Vec<String>,
    },
}

/// Process capability verbs.
///
/// # Example
///
/// ```ignore
/// use tau_domain::ProcessCapability;
/// let cap = ProcessCapability::Spawn { commands: vec!["git".into()] };
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ProcessCapability {
    /// Spawn subprocesses for the allow-listed command names.
    #[non_exhaustive]
    Spawn {
        /// Allowed command names.
        commands: Vec<String>,
    },
}

/// Agent capability verbs.
///
/// # Example
///
/// ```ignore
/// use tau_domain::AgentCapability;
/// let cap = AgentCapability::Spawn { allowed_kinds: vec!["worker".into()] };
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AgentCapability {
    /// Spawn sub-agents whose package kind matches the allow-list.
    #[non_exhaustive]
    Spawn {
        /// Permitted package kinds (e.g. `["worker"]`).
        allowed_kinds: Vec<String>,
    },
}

#[cfg(feature = "serde")]
mod capability_de {
    use super::*;
    use serde::ser::SerializeMap;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Deserialize)]
    struct RawCapability {
        kind: String,
        #[serde(default)]
        paths: Option<Vec<String>>,
        #[serde(default)]
        max_bytes: Option<u64>,
        #[serde(default)]
        hosts: Option<Vec<String>>,
        #[serde(default)]
        methods: Option<Vec<String>>,
        #[serde(default)]
        commands: Option<Vec<String>>,
        #[serde(default)]
        allowed_kinds: Option<Vec<String>>,
        #[serde(flatten)]
        rest: BTreeMap<String, Value>,
    }

    impl<'de> Deserialize<'de> for Capability {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            let raw = RawCapability::deserialize(d)?;
            Ok(match raw.kind.as_str() {
                "fs.read" => Capability::Filesystem(FsCapability::Read {
                    paths: raw.paths.unwrap_or_default(),
                }),
                "fs.write" => Capability::Filesystem(FsCapability::Write {
                    paths: raw.paths.unwrap_or_default(),
                    max_bytes: raw.max_bytes,
                }),
                "fs.exec" => Capability::Filesystem(FsCapability::Exec {
                    paths: raw.paths.unwrap_or_default(),
                }),
                "net.http" => Capability::Network(NetCapability::Http {
                    hosts: raw.hosts.unwrap_or_default(),
                    methods: raw.methods.unwrap_or_default(),
                }),
                "process.spawn" => Capability::Process(ProcessCapability::Spawn {
                    commands: raw.commands.unwrap_or_default(),
                }),
                "agent.spawn" => Capability::Agent(AgentCapability::Spawn {
                    allowed_kinds: raw.allowed_kinds.unwrap_or_default(),
                }),
                _ => Capability::Custom {
                    name: raw.kind,
                    params: raw.rest,
                },
            })
        }
    }

    impl Serialize for Capability {
        fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            match self {
                Capability::Filesystem(FsCapability::Read { paths }) => {
                    let mut m = s.serialize_map(Some(2))?;
                    m.serialize_entry("kind", "fs.read")?;
                    m.serialize_entry("paths", paths)?;
                    m.end()
                }
                Capability::Filesystem(FsCapability::Write { paths, max_bytes }) => {
                    let len = if max_bytes.is_some() { 3 } else { 2 };
                    let mut m = s.serialize_map(Some(len))?;
                    m.serialize_entry("kind", "fs.write")?;
                    m.serialize_entry("paths", paths)?;
                    if let Some(b) = max_bytes {
                        m.serialize_entry("max_bytes", b)?;
                    }
                    m.end()
                }
                Capability::Filesystem(FsCapability::Exec { paths }) => {
                    let mut m = s.serialize_map(Some(2))?;
                    m.serialize_entry("kind", "fs.exec")?;
                    m.serialize_entry("paths", paths)?;
                    m.end()
                }
                Capability::Network(NetCapability::Http { hosts, methods }) => {
                    let mut m = s.serialize_map(Some(3))?;
                    m.serialize_entry("kind", "net.http")?;
                    m.serialize_entry("hosts", hosts)?;
                    m.serialize_entry("methods", methods)?;
                    m.end()
                }
                Capability::Process(ProcessCapability::Spawn { commands }) => {
                    let mut m = s.serialize_map(Some(2))?;
                    m.serialize_entry("kind", "process.spawn")?;
                    m.serialize_entry("commands", commands)?;
                    m.end()
                }
                Capability::Agent(AgentCapability::Spawn { allowed_kinds }) => {
                    let mut m = s.serialize_map(Some(2))?;
                    m.serialize_entry("kind", "agent.spawn")?;
                    m.serialize_entry("allowed_kinds", allowed_kinds)?;
                    m.end()
                }
                Capability::Custom { name, params } => {
                    let mut m = s.serialize_map(Some(1 + params.len()))?;
                    m.serialize_entry("kind", name)?;
                    for (k, v) in params {
                        m.serialize_entry(k, v)?;
                    }
                    m.end()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_read_constructs() {
        let c = Capability::Filesystem(FsCapability::Read {
            paths: vec!["/tmp/**".into()],
        });
        match &c {
            Capability::Filesystem(FsCapability::Read { paths }) => {
                assert_eq!(*paths, vec!["/tmp/**".to_string()]);
            }
            _ => panic!("expected Filesystem(Read), got {:?}", c),
        }
    }

    #[test]
    fn custom_constructs() {
        let mut params = BTreeMap::new();
        params.insert(
            "servers".into(),
            Value::Array(vec![Value::String("fs-mcp".into())]),
        );
        let _c = Capability::Custom {
            name: "mcp.tool.use".into(),
            params,
        };
    }

    #[cfg(feature = "serde")]
    #[test]
    fn fs_read_round_trips_through_json() {
        let cap = Capability::Filesystem(FsCapability::Read {
            paths: vec!["/tmp/**".into()],
        });
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(json, r#"{"kind":"fs.read","paths":["/tmp/**"]}"#);
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn custom_round_trips_through_json() {
        let mut params = BTreeMap::new();
        params.insert(
            "servers".into(),
            Value::Array(vec![Value::String("fs-mcp".into())]),
        );
        let cap = Capability::Custom {
            name: "mcp.tool.use".into(),
            params,
        };
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(json, r#"{"kind":"mcp.tool.use","servers":["fs-mcp"]}"#);
        let back: Capability = serde_json::from_str(&json).unwrap();
        assert_eq!(cap, back);
    }
}
