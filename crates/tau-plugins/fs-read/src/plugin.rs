//! `FsReadPlugin` — Tool impl for the fs-read plugin.
//!
//! Reads bytes from a single absolute path under the agent's
//! `fs.read` capability scope. The agent's grant (received via
//! `SessionContext.granted_capabilities`) carries the glob patterns
//! that constrain which paths are admissible.

use base64::Engine as _;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use tau_domain::{Capability, FsCapability, Value};
use tau_plugin_sdk::{ConfigError, Configure};
use tau_ports::{
    fixtures::{make_tool_result, make_tool_spec},
    SessionContext, Tool, ToolContent, ToolError, ToolResult, ToolSpec,
};

use crate::config::FsReadConfig;
use crate::path_check::{admit_with_deny, validate_path, BadArgs};

/// Per-session state derived from the agent's granted capabilities.
///
/// `init` extracts the agent's `FsCapability::Read.paths` glob list
/// from `SessionContext.granted_capabilities` and stashes it here so
/// `invoke` can perform path-glob admission without re-walking the
/// capability list per call.
pub struct FsReadSession {
    /// Glob patterns extracted from the active `FsCapability::Read.paths`
    /// entries. Empty iff the agent has no fs.read grant (which means
    /// the kernel should have already denied the call).
    allowed_globs: Vec<String>,
    /// Path globs to subtract from `allowed_globs`, populated from the
    /// `fs.read` entry in `SessionContext.deny_entries`. Deny wins —
    /// see spec §9.
    denied_globs: Vec<String>,
}

/// fs-read Tool plugin.
pub struct FsReadPlugin {
    #[allow(dead_code)] // reserved for future config knobs
    config: FsReadConfig,
}

impl Configure for FsReadPlugin {
    type Config = FsReadConfig;

    fn from_config(config: Self::Config) -> Result<Self, ConfigError> {
        Ok(FsReadPlugin { config })
    }
}

impl Tool for FsReadPlugin {
    type Session = FsReadSession;

    fn name(&self) -> &str {
        "fs-read"
    }

    fn schema(&self) -> ToolSpec {
        let schema_json = json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute path to the file. No `..` segments allowed."
                }
            },
            "required": ["path"],
        });
        let schema_value: Value = serde_json::from_str(
            &serde_json::to_string(&schema_json).expect("static JSON schema serializes"),
        )
        .expect("static JSON schema round-trips through tau_domain::Value");
        make_tool_spec(
            "fs-read".to_string(),
            "Read the bytes of a file at an absolute path.".to_string(),
            schema_value,
        )
    }

    fn capabilities(&self) -> &[Capability] {
        // Empty paths declares the structural capability; the agent's
        // grant carries the actual scope. The kernel's capability
        // check (run.rs:272) verifies the agent has *some* fs.read.
        // The plugin's invoke does the finer-grained glob check
        // against ctx.granted_capabilities.
        //
        // Build via JSON deserialization because `FsCapability::Read` is
        // `#[non_exhaustive]` and cannot be constructed via struct-literal
        // syntax outside of tau-domain.
        static CAPS: OnceLock<Vec<Capability>> = OnceLock::new();
        CAPS.get_or_init(|| {
            let cap: Capability = serde_json::from_str(r#"{"kind":"fs.read","paths":[]}"#)
                .expect("static fs.read capability JSON is valid");
            vec![cap]
        })
    }

    async fn init(&self, ctx: SessionContext) -> Result<Self::Session, ToolError> {
        let allowed_globs = extract_fs_read_paths(&ctx.granted_capabilities);
        let denied_globs = ctx
            .deny_entries
            .iter()
            .find(|e| e.kind == "fs.read")
            .map(|e| e.deny.clone())
            .unwrap_or_default();
        Ok(FsReadSession {
            allowed_globs,
            denied_globs,
        })
    }

    async fn invoke(
        &self,
        session: &mut Self::Session,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        let path_str = parse_path_arg(&args)?;
        let path =
            validate_path(path_str).map_err(|e| ToolError::BadArgs { reason: e.reason() })?;
        if !admit_with_deny(path, &session.allowed_globs, &session.denied_globs) {
            return Err(ToolError::BadArgs {
                reason: BadArgs::NotInScope.reason(),
            });
        }
        match tokio::fs::read(path).await {
            Ok(bytes) => {
                let len = bytes.len() as i64;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let mut map: BTreeMap<String, Value> = BTreeMap::new();
                map.insert("contents".into(), Value::String(b64));
                map.insert("size".into(), Value::Integer(len));
                Ok(make_tool_result(
                    vec![ToolContent::Json {
                        data: Value::Object(map),
                    }],
                    false,
                ))
            }
            Err(io_err) => Ok(make_tool_result(
                vec![ToolContent::Text {
                    text: format!("fs-read: {io_err}"),
                }],
                true, // semantic error to the LLM, NOT a ToolError
            )),
        }
    }

    async fn teardown(&self, _session: Self::Session) -> Result<(), ToolError> {
        Ok(())
    }
}

fn parse_path_arg(args: &Value) -> Result<&str, ToolError> {
    args.as_object()
        .and_then(|o| o.get("path"))
        .and_then(Value::as_string)
        .ok_or_else(|| ToolError::BadArgs {
            reason: "fs-read: missing or wrong-shape `path` arg".to_string(),
        })
}

fn extract_fs_read_paths(granted: &[Capability]) -> Vec<String> {
    granted
        .iter()
        .filter_map(|c| match c {
            Capability::Filesystem(FsCapability::Read { paths, .. }) => Some(paths.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deserialize a `Capability` from a JSON string.
    /// Used instead of struct-literal construction because `FsCapability::Read`
    /// and related variants are `#[non_exhaustive]`.
    fn cap(json: &str) -> Capability {
        serde_json::from_str(json).expect("test capability JSON must be valid")
    }

    #[test]
    fn extract_fs_read_paths_collects_from_multiple_grants() {
        let granted = vec![
            cap(r#"{"kind":"fs.read","paths":["/tmp/**"]}"#),
            cap(r#"{"kind":"fs.read","paths":["/var/log/**","/etc/**"]}"#),
            // Non-fs.read capabilities are ignored.
            cap(r#"{"kind":"process.spawn","commands":["echo"]}"#),
        ];
        let paths = extract_fs_read_paths(&granted);
        assert_eq!(
            paths,
            vec![
                "/tmp/**".to_string(),
                "/var/log/**".to_string(),
                "/etc/**".to_string(),
            ]
        );
    }

    #[test]
    fn extract_fs_read_paths_returns_empty_when_no_grants() {
        let granted: Vec<Capability> = vec![];
        let paths = extract_fs_read_paths(&granted);
        assert!(paths.is_empty());
    }
}
