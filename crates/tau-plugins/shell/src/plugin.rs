//! `ShellPlugin` — Tool impl for the shell plugin.
//!
//! Runs allow-listed subprocesses with wall-clock timeout + output
//! capping. The agent's grant (received via
//! `SessionContext.granted_capabilities`) carries the
//! `ProcessCapability::Spawn.commands` allow-list that constrains
//! which command names are admissible.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use tau_domain::{Capability, Value};
use tau_plugin_sdk::{ConfigError, Configure};
use tau_ports::{
    fixtures::{make_tool_result, make_tool_spec},
    SessionContext, Tool, ToolContent, ToolError, ToolResult, ToolSpec,
};

use crate::command_check::{admit_with_deny, extract_allowed_commands};
use crate::config::{validate, ShellConfig};
use crate::runner::run_subprocess;

/// Per-session state derived from the agent's granted capabilities.
pub struct ShellSession {
    /// Command names extracted from the active
    /// `ProcessCapability::Spawn.commands` entries.
    allowed_commands: Vec<String>,
    /// Command names to subtract from `allowed_commands`, populated from
    /// the `process.spawn` entry in `SessionContext.deny_entries`. Deny
    /// wins — see spec §9.
    denied_commands: Vec<String>,
}

/// shell Tool plugin.
pub struct ShellPlugin {
    config: ShellConfig,
}

impl Configure for ShellPlugin {
    type Config = ShellConfig;

    fn from_config(config: Self::Config) -> Result<Self, ConfigError> {
        validate(&config)?;
        Ok(ShellPlugin { config })
    }
}

impl Tool for ShellPlugin {
    type Session = ShellSession;

    fn name(&self) -> &str {
        "shell"
    }

    fn schema(&self) -> ToolSpec {
        let schema_json = serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Command name (must be in agent's process.spawn allow-list)."
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "default": []
                },
                "timeout_secs": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 600,
                    "description": "Wall-clock timeout. Default 30; max 600."
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional absolute working directory."
                }
            },
            "required": ["command"],
        });
        let schema_value: Value = serde_json::from_str(
            &serde_json::to_string(&schema_json).expect("static JSON schema serializes"),
        )
        .expect("static JSON schema round-trips through tau_domain::Value");
        make_tool_spec(
            "shell".to_string(),
            "Run an allow-listed shell command with wall-clock timeout and 1 MiB output cap."
                .to_string(),
            schema_value,
        )
    }

    fn capabilities(&self) -> &[Capability] {
        // Empty commands declares the structural capability; the
        // agent's grant carries the actual allow-list.
        static CAPS: OnceLock<Vec<Capability>> = OnceLock::new();
        CAPS.get_or_init(|| {
            // ProcessCapability::Spawn is #[non_exhaustive] — construct
            // via JSON deserialization. Same pattern as fs-read's
            // capabilities() builder.
            let json = serde_json::json!({
                "kind": "process.spawn",
                "commands": []
            });
            let cap: Capability =
                serde_json::from_value(json).expect("structural process.spawn capability parses");
            vec![cap]
        })
    }

    async fn init(&self, ctx: SessionContext) -> Result<Self::Session, ToolError> {
        let allowed_commands = extract_allowed_commands(&ctx.granted_capabilities);
        let denied_commands = ctx
            .deny_entries
            .iter()
            .find(|e| e.kind == "process.spawn")
            .map(|e| e.deny.clone())
            .unwrap_or_default();
        Ok(ShellSession {
            allowed_commands,
            denied_commands,
        })
    }

    async fn invoke(
        &self,
        session: &mut Self::Session,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        let parsed = parse_args(&args)?;

        // Admit the command against the agent's grant.
        if !admit_with_deny(
            &parsed.command,
            &session.allowed_commands,
            &session.denied_commands,
        ) {
            return Err(ToolError::BadArgs {
                reason: format!("shell: command not in capability scope: {}", parsed.command),
            });
        }

        // Validate cwd is absolute when set.
        if let Some(cwd) = &parsed.cwd {
            if cwd.is_empty() {
                return Err(ToolError::BadArgs {
                    reason: "shell: cwd is empty".into(),
                });
            }
            if !std::path::Path::new(cwd).is_absolute() {
                return Err(ToolError::BadArgs {
                    reason: "shell: cwd is not absolute".into(),
                });
            }
        }

        // Compute effective timeout: clamp args.timeout_secs to
        // [1, max_timeout_secs]; default to config.default_timeout_secs.
        let requested = parsed
            .timeout_secs
            .unwrap_or(self.config.default_timeout_secs);
        let timeout = requested.max(1).min(self.config.max_timeout_secs);

        // Run the subprocess.
        let result = run_subprocess(
            &parsed.command,
            &parsed.args,
            timeout,
            parsed.cwd.as_deref(),
        )
        .await
        .map_err(|e| ToolError::Internal {
            message: format!("shell: spawn failed: {e}"),
        })?;

        // Encode the result as ToolContent::Json.
        let mut map: BTreeMap<String, Value> = BTreeMap::new();
        map.insert(
            "stdout".into(),
            Value::String(String::from_utf8_lossy(&result.stdout).into_owned()),
        );
        map.insert(
            "stderr".into(),
            Value::String(String::from_utf8_lossy(&result.stderr).into_owned()),
        );
        map.insert("exit_code".into(), Value::Integer(result.exit_code as i64));
        map.insert("timed_out".into(), Value::Bool(result.timed_out));
        map.insert(
            "stdout_truncated".into(),
            Value::Bool(result.stdout_truncated),
        );
        map.insert(
            "stderr_truncated".into(),
            Value::Bool(result.stderr_truncated),
        );

        let is_error = result.exit_code != 0;
        Ok(make_tool_result(
            vec![ToolContent::Json {
                data: Value::Object(map),
            }],
            is_error,
        ))
    }

    async fn teardown(&self, _session: Self::Session) -> Result<(), ToolError> {
        Ok(())
    }
}

#[derive(Debug)]
struct ParsedArgs {
    command: String,
    args: Vec<String>,
    timeout_secs: Option<u64>,
    cwd: Option<String>,
}

fn parse_args(args: &Value) -> Result<ParsedArgs, ToolError> {
    let obj = args.as_object().ok_or_else(|| ToolError::BadArgs {
        reason: "shell: args must be an object".into(),
    })?;
    let command = obj
        .get("command")
        .and_then(Value::as_string)
        .ok_or_else(|| ToolError::BadArgs {
            reason: "shell: missing or wrong-shape `command` arg".into(),
        })?
        .to_string();
    if command.is_empty() {
        return Err(ToolError::BadArgs {
            reason: "shell: `command` is empty".into(),
        });
    }

    let cmd_args: Vec<String> = match obj.get("args") {
        Some(Value::Array(arr)) => arr
            .iter()
            .map(|v| {
                v.as_string()
                    .map(|s| s.to_string())
                    .ok_or_else(|| ToolError::BadArgs {
                        reason: "shell: `args` array contains non-string".into(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => {
            return Err(ToolError::BadArgs {
                reason: "shell: `args` must be a string array".into(),
            })
        }
        None => Vec::new(),
    };

    let timeout_secs: Option<u64> = match obj.get("timeout_secs") {
        Some(Value::Integer(n)) => {
            if *n < 0 {
                return Err(ToolError::BadArgs {
                    reason: "shell: `timeout_secs` must be non-negative".into(),
                });
            }
            Some(*n as u64)
        }
        Some(_) => {
            return Err(ToolError::BadArgs {
                reason: "shell: `timeout_secs` must be an integer".into(),
            })
        }
        None => None,
    };

    let cwd: Option<String> = match obj.get("cwd") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => {
            return Err(ToolError::BadArgs {
                reason: "shell: `cwd` must be a string".into(),
            })
        }
        None => None,
    };

    Ok(ParsedArgs {
        command,
        args: cmd_args,
        timeout_secs,
        cwd,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_extracts_command_args_timeout_cwd() {
        let args: Value = serde_json::from_str(
            r#"{"command":"echo","args":["hi"],"timeout_secs":5,"cwd":"/tmp"}"#,
        )
        .unwrap();
        let parsed = parse_args(&args).unwrap();
        assert_eq!(parsed.command, "echo");
        assert_eq!(parsed.args, vec!["hi".to_string()]);
        assert_eq!(parsed.timeout_secs, Some(5));
        assert_eq!(parsed.cwd, Some("/tmp".to_string()));
    }

    #[test]
    fn parse_args_missing_command_returns_bad_args() {
        let args: Value = serde_json::from_str(r#"{"args":["x"]}"#).unwrap();
        let err = parse_args(&args).unwrap_err();
        assert!(matches!(err, ToolError::BadArgs { .. }));
    }

    #[test]
    fn parse_args_empty_command_returns_bad_args() {
        let args: Value = serde_json::from_str(r#"{"command":""}"#).unwrap();
        let err = parse_args(&args).unwrap_err();
        assert!(matches!(err, ToolError::BadArgs { .. }));
    }
}
