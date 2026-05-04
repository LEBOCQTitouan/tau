//! Test fixtures for `tau-domain` types.
//!
//! Gated behind the `test-fixtures` feature (off by default). Downstream
//! crates depend via:
//!
//! ```toml
//! [dev-dependencies]
//! tau-domain = { workspace = true, features = ["test-fixtures"] }
//! ```
//!
//! All helpers are deterministic where possible; UUID-based ones
//! (`any_message`, `any_agent_definition`-derived IDs) generate fresh
//! v7 UUIDs each call.

use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::SystemTime;

use crate::agent::AgentDefinition;
use crate::id::{AgentId, AgentInstanceId, MessageId, PackageName};
use crate::message::{Address, Message, MessagePayload};
use crate::package::manifest::{PackageId, PackageKind, UncheckedManifest};
use crate::package::{PackageManifest, PackageSource};
use crate::version::Version;

/// A deterministic, valid `PackageName`.
pub fn any_package_name() -> PackageName {
    PackageName::from_str("test-pkg").expect("valid")
}

/// A deterministic, valid `AgentId`.
pub fn any_agent_id() -> AgentId {
    AgentId::from_str("test-agent").expect("valid")
}

/// A deterministic, valid `PackageSource` (https URL, no rev).
pub fn any_package_source() -> PackageSource {
    PackageSource::from_str("https://example.com/test.git").expect("valid")
}

/// A minimal valid `UncheckedManifest`.
pub fn any_unchecked_manifest() -> UncheckedManifest {
    UncheckedManifest {
        name: any_package_name(),
        version: Version::parse("0.1.0").expect("valid"),
        description: "test package".into(),
        authors: vec![],
        license: None,
        source: any_package_source(),
        kind: PackageKind::Custom {
            kind: "tool".into(),
        },
        dependencies: vec![],
        capabilities: vec![],
        plugin: None,
        sandbox: crate::package::sandbox::PluginSandboxRequirements::default(),
    }
}

/// A minimal validated `PackageManifest`.
pub fn any_package_manifest() -> PackageManifest {
    any_unchecked_manifest()
        .validate()
        .expect("fixture should validate")
}

/// A minimal `AgentDefinition`.
pub fn any_agent_definition() -> AgentDefinition {
    AgentDefinition::new(
        any_agent_id(),
        "Test Agent".into(),
        PackageId {
            name: any_package_name(),
            version: Version::parse("0.1.0").expect("valid"),
        },
        any_package_name(),
    )
}

/// A minimal `Message` with a Text payload (fresh UUIDs).
pub fn any_message() -> Message {
    Message {
        id: MessageId::new(),
        sender: Address::User,
        recipient: Address::Agent(AgentInstanceId::new()),
        parent_id: None,
        created_at: SystemTime::UNIX_EPOCH,
        headers: BTreeMap::new(),
        payload: MessagePayload::Text {
            content: "hello".into(),
        },
    }
}
