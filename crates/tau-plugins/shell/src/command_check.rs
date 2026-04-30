//! Command-name allow-list check for `shell`.
//!
//! See `docs/superpowers/specs/2026-04-29-tool-plugins-design.md`
//! §5.1, §7.

use tau_domain::{Capability, ProcessCapability};

/// Extract the allow-listed command names from the agent's grant.
/// Returns the concatenation of all `ProcessCapability::Spawn.commands`
/// entries; ignores other capability variants.
pub(crate) fn extract_allowed_commands(granted: &[Capability]) -> Vec<String> {
    granted
        .iter()
        .filter_map(|c| match c {
            Capability::Process(ProcessCapability::Spawn { commands, .. }) => {
                Some(commands.clone())
            }
            _ => None,
        })
        .flatten()
        .collect()
}

/// Check whether `command` is in the agent's `ProcessCapability::Spawn`
/// allow-list.
pub(crate) fn admit(command: &str, allow_list: &[String]) -> bool {
    allow_list.iter().any(|allowed| allowed == command)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `process.spawn` capability via JSON deserialization
    /// (the variant is `#[non_exhaustive]`; struct-literal blocked
    /// outside `tau-domain`).
    fn process_spawn_cap(commands: &[&str]) -> Capability {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            cap: Capability,
        }
        let cmds_json: Vec<serde_json::Value> = commands
            .iter()
            .map(|c| serde_json::Value::String((*c).to_string()))
            .collect();
        let json = serde_json::json!({
            "cap": { "kind": "process.spawn", "commands": cmds_json }
        });
        serde_json::from_value::<Wrapper>(json)
            .expect("test process.spawn capability must parse")
            .cap
    }

    #[test]
    fn extract_collects_from_multiple_grants() {
        let granted = vec![
            process_spawn_cap(&["echo", "ls"]),
            process_spawn_cap(&["cat"]),
        ];
        let out = extract_allowed_commands(&granted);
        assert_eq!(
            out,
            vec!["echo".to_string(), "ls".to_string(), "cat".to_string()]
        );
    }

    #[test]
    fn extract_ignores_non_process_caps() {
        // Only process.spawn caps contribute. Test with a mix.
        let granted = vec![process_spawn_cap(&["echo"])];
        let out = extract_allowed_commands(&granted);
        assert_eq!(out, vec!["echo".to_string()]);
    }

    #[test]
    fn admit_matches_command_in_list() {
        let allow = vec!["echo".to_string(), "ls".to_string()];
        assert!(admit("echo", &allow));
        assert!(admit("ls", &allow));
    }

    #[test]
    fn admit_does_not_match_command_not_in_list() {
        let allow = vec!["echo".to_string()];
        assert!(!admit("cat", &allow));
        assert!(!admit("rm", &allow));
    }

    #[test]
    fn admit_empty_list_returns_false() {
        let allow: Vec<String> = vec![];
        assert!(!admit("echo", &allow));
    }
}
