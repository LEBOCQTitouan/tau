//! Project `tau.toml` deserialization, validation, and error taxonomy.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Unchecked deserialization shape — fields are typed but no semantic
/// validation has run. Use [`UncheckedProjectConfig::validate`] to
/// produce a [`ProjectConfig`].
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UncheckedProjectConfig {
    /// Top-level `[project]` table.
    pub project: UncheckedProject,
    /// Map of agent id → unchecked agent definition.
    #[serde(default)]
    pub agents: BTreeMap<String, UncheckedAgent>,
}

/// `[project]` table.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UncheckedProject {
    /// Free-form project name; required, validated non-empty.
    pub name: String,
    /// Optional human-readable description.
    #[serde(default)]
    pub description: String,
}

/// `[agents.<id>]` table.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UncheckedAgent {
    /// Human-readable agent name displayed in UIs.
    pub display_name: String,
    /// Package reference of the form `<name>@<semver-req>`.
    pub package: String,
    /// LLM backend identifier; resolved at lookup time.
    pub llm_backend: String,
    /// Optional `[agents.<id>.requires]` sub-table.
    #[serde(default)]
    pub requires: Option<UncheckedRequires>,
    /// Reserved for Phase 1+ per spec §3.2; presence at v0.1 is a hard error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<toml::Value>,
    /// Free-form `[agents.<id>.config]` sub-table; passed through.
    #[serde(default)]
    pub config: Option<toml::Table>,
    /// Optional `[agents.<id>.prompt]` sub-table.
    #[serde(default)]
    pub prompt: Option<UncheckedPrompt>,
}

/// `[agents.<id>.requires]` sub-table.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UncheckedRequires {
    /// Tool package names this agent advises requiring (advisory at v0.1).
    #[serde(default)]
    pub tools: Vec<String>,
    /// Phase 1+; ignored at v0.1.
    #[serde(default)]
    pub packages: Vec<String>,
}

/// `[agents.<id>.prompt]` sub-table.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UncheckedPrompt {
    /// Inline system prompt; mutually exclusive with `system_file`.
    #[serde(default)]
    pub system: Option<String>,
    /// Path to a system prompt file; mutually exclusive with `system`.
    #[serde(default)]
    pub system_file: Option<PathBuf>,
}

// ----- Validated shapes -----

/// Validated project config. Constructed via
/// [`UncheckedProjectConfig::validate`] only.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ProjectConfig {
    /// Validated, non-empty project name.
    pub project_name: String,
    /// Optional description (may be empty).
    pub description: String,
    /// Map of agent id → validated agent entry.
    pub agents: BTreeMap<String, AgentEntry>,
}

/// Validated entry for a single agent.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct AgentEntry {
    /// Agent id (the table key under `[agents.<id>]`).
    pub id: String,
    /// Display name.
    pub display_name: String,
    /// Package reference (`<name>@<semver-req>`).
    pub package: String,
    /// LLM backend identifier.
    pub llm_backend: String,
    /// Validated `requires` block.
    pub requires: RequiresEntry,
    /// Free-form configuration table.
    pub config: BTreeMap<String, toml::Value>,
    /// Validated prompt selection.
    pub prompt: PromptEntry,
}

/// Validated `requires` sub-table.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub struct RequiresEntry {
    /// Tool package names (advisory at v0.1).
    pub tools: Vec<String>,
}

/// Validated prompt selection. `system` and `system_file` are mutually
/// exclusive, so this enum encodes the three valid states.
#[non_exhaustive]
#[derive(Debug, Clone, Default)]
pub enum PromptEntry {
    /// No prompt configured.
    #[default]
    None,
    /// Inline prompt string.
    Inline(String),
    /// Path to an external prompt file.
    File(PathBuf),
}

// ----- Errors -----

/// Errors produced when loading or validating a project `tau.toml`.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ProjectConfigError {
    /// No `tau.toml` file found.
    #[error("project tau.toml not found in scope (run `tau init` to create one)")]
    NotFound,

    /// Filesystem read failure (other than "not found").
    #[error("failed to read project tau.toml at {path:?}: {source}")]
    Read {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// TOML parse failure.
    #[error("failed to parse project tau.toml at {path:?}: {source}")]
    Parse {
        /// Path that failed to parse.
        path: PathBuf,
        /// Underlying TOML parse error.
        #[source]
        source: toml::de::Error,
    },

    /// `project.name` was empty after trimming.
    #[error("project name must be non-empty")]
    EmptyProjectName,

    /// Generic per-agent semantic validation failure.
    #[error("agent {id:?}: {message}")]
    AgentValidation {
        /// Agent id that failed validation.
        id: String,
        /// Human-readable message describing the violation.
        message: String,
    },

    /// Agent declared a `[agents.<id>.capabilities]` sub-table; reserved for Phase 1+.
    #[error(
        "agent {id:?}: capability override is not supported at v0.1; \
         see Phase 1+ roadmap entry: docs/retrospectives/phase-0-mid.md \
         §'What's NOT in scope for this memo'"
    )]
    CapabilityOverrideUnsupported {
        /// Agent id that contained the unsupported capability override.
        id: String,
    },

    /// Agent declared both `prompt.system` and `prompt.system_file`.
    #[error("agent {id:?}: prompt requires exactly one of `system` or `system_file`, found both")]
    PromptAmbiguous {
        /// Agent id whose prompt block was ambiguous.
        id: String,
    },
}

// ----- Validation logic -----

impl UncheckedProjectConfig {
    /// Validate semantic invariants and produce a [`ProjectConfig`].
    pub fn validate(self) -> Result<ProjectConfig, ProjectConfigError> {
        if self.project.name.trim().is_empty() {
            return Err(ProjectConfigError::EmptyProjectName);
        }

        let mut agents = BTreeMap::new();
        for (id, raw) in self.agents {
            agents.insert(id.clone(), validate_agent(id, raw)?);
        }

        Ok(ProjectConfig {
            project_name: self.project.name,
            description: self.project.description,
            agents,
        })
    }
}

fn validate_agent(id: String, raw: UncheckedAgent) -> Result<AgentEntry, ProjectConfigError> {
    if raw.display_name.trim().is_empty() {
        return Err(ProjectConfigError::AgentValidation {
            id,
            message: "display_name must be non-empty".into(),
        });
    }
    if raw.package.trim().is_empty() {
        return Err(ProjectConfigError::AgentValidation {
            id,
            message: "package must be non-empty".into(),
        });
    }
    if raw.llm_backend.trim().is_empty() {
        return Err(ProjectConfigError::AgentValidation {
            id,
            message: "llm_backend must be non-empty".into(),
        });
    }

    if raw.capabilities.is_some() {
        return Err(ProjectConfigError::CapabilityOverrideUnsupported { id });
    }

    let prompt = match raw.prompt {
        None => PromptEntry::None,
        Some(p) => match (p.system, p.system_file) {
            (Some(s), None) => PromptEntry::Inline(s),
            (None, Some(f)) => PromptEntry::File(f),
            (Some(_), Some(_)) => return Err(ProjectConfigError::PromptAmbiguous { id }),
            (None, None) => PromptEntry::None,
        },
    };

    let requires = raw
        .requires
        .map_or(RequiresEntry::default(), |r| RequiresEntry {
            tools: r.tools,
            // r.packages ignored at v0.1
        });

    let config = raw
        .config
        .map(|t| t.into_iter().collect::<BTreeMap<_, _>>())
        .unwrap_or_default();

    Ok(AgentEntry {
        id,
        display_name: raw.display_name,
        package: raw.package,
        llm_backend: raw.llm_backend,
        requires,
        config,
        prompt,
    })
}

// ----- File entrypoint -----

impl ProjectConfig {
    /// Load and validate from a path. Convenience wrapper around the
    /// deserialize-then-validate pipeline.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, ProjectConfigError> {
        let path = path.as_ref();
        let bytes = std::fs::read_to_string(path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ProjectConfigError::NotFound
            } else {
                ProjectConfigError::Read {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })?;
        let unchecked: UncheckedProjectConfig =
            toml::from_str(&bytes).map_err(|source| ProjectConfigError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        unchecked.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml_str: &str) -> Result<ProjectConfig, ProjectConfigError> {
        let unchecked: UncheckedProjectConfig = toml::from_str(toml_str).unwrap();
        unchecked.validate()
    }

    #[test]
    fn parse_minimal_project_only_succeeds() {
        let cfg = parse("[project]\nname = \"x\"\n").unwrap();
        assert_eq!(cfg.project_name, "x");
        assert!(cfg.agents.is_empty());
    }

    #[test]
    fn parse_with_one_full_agent_succeeds() {
        let toml_str = r#"
            [project]
            name = "demo"

            [agents.reviewer]
            display_name = "Code Reviewer"
            package      = "code-reviewer@^0.1"
            llm_backend  = "anthropic"

            [agents.reviewer.requires]
            tools = ["fs-read"]

            [agents.reviewer.config]
            model = "claude"

            [agents.reviewer.prompt]
            system = "You are a careful reviewer."
        "#;
        let cfg = parse(toml_str).unwrap();
        assert_eq!(cfg.agents.len(), 1);
        let agent = cfg.agents.get("reviewer").unwrap();
        assert_eq!(agent.display_name, "Code Reviewer");
        assert_eq!(agent.requires.tools, vec!["fs-read".to_string()]);
        assert!(
            matches!(&agent.prompt, PromptEntry::Inline(s) if s == "You are a careful reviewer.")
        );
    }

    #[test]
    fn validate_rejects_empty_project_name() {
        let result = parse("[project]\nname = \"\"\n");
        assert!(matches!(result, Err(ProjectConfigError::EmptyProjectName)));
    }

    #[test]
    fn validate_rejects_capability_override() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = "anthropic"

            [agents.r.capabilities]
            "filesystem.read" = ["src/**"]
        "#;
        let result = parse(toml_str);
        let Err(ProjectConfigError::CapabilityOverrideUnsupported { id, .. }) = result else {
            panic!("expected CapabilityOverrideUnsupported: {result:?}")
        };
        assert_eq!(id, "r");
    }

    #[test]
    fn validate_rejects_prompt_with_both_system_and_system_file() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = "anthropic"

            [agents.r.prompt]
            system      = "inline"
            system_file = "prompts/r.md"
        "#;
        let result = parse(toml_str);
        let Err(ProjectConfigError::PromptAmbiguous { id, .. }) = result else {
            panic!("expected PromptAmbiguous: {result:?}")
        };
        assert_eq!(id, "r");
    }

    #[test]
    fn validate_accepts_prompt_with_only_system() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = "anthropic"

            [agents.r.prompt]
            system = "be helpful"
        "#;
        let cfg = parse(toml_str).unwrap();
        let agent = cfg.agents.get("r").unwrap();
        assert!(matches!(&agent.prompt, PromptEntry::Inline(s) if s == "be helpful"));
    }

    #[test]
    fn validate_accepts_prompt_with_only_system_file() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = "anthropic"

            [agents.r.prompt]
            system_file = "prompts/r.md"
        "#;
        let cfg = parse(toml_str).unwrap();
        let agent = cfg.agents.get("r").unwrap();
        let PromptEntry::File(p) = &agent.prompt else {
            panic!("expected File: {:?}", agent.prompt)
        };
        assert_eq!(p.to_str(), Some("prompts/r.md"));
    }

    #[test]
    fn validate_accepts_no_prompt_table() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = "anthropic"
        "#;
        let cfg = parse(toml_str).unwrap();
        let agent = cfg.agents.get("r").unwrap();
        assert!(matches!(&agent.prompt, PromptEntry::None));
    }

    #[test]
    fn validate_rejects_empty_display_name() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = ""
            package      = "p@^0.1"
            llm_backend  = "anthropic"
        "#;
        let result = parse(toml_str);
        let Err(ProjectConfigError::AgentValidation { id, message }) = result else {
            panic!("expected AgentValidation: {result:?}")
        };
        assert_eq!(id, "r");
        assert!(message.contains("display_name"));
    }

    #[test]
    fn validate_rejects_empty_package() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = ""
            llm_backend  = "anthropic"
        "#;
        let result = parse(toml_str);
        let Err(ProjectConfigError::AgentValidation { message, .. }) = result else {
            panic!()
        };
        assert!(message.contains("package"));
    }

    #[test]
    fn validate_rejects_empty_llm_backend() {
        let toml_str = r#"
            [project]
            name = "x"

            [agents.r]
            display_name = "R"
            package      = "p@^0.1"
            llm_backend  = ""
        "#;
        let result = parse(toml_str);
        let Err(ProjectConfigError::AgentValidation { message, .. }) = result else {
            panic!()
        };
        assert!(message.contains("llm_backend"));
    }

    #[test]
    fn parse_with_two_agents_keeps_both() {
        let toml_str = r#"
            [project]
            name = "demo"

            [agents.alpha]
            display_name = "Alpha"
            package      = "p@^0.1"
            llm_backend  = "anthropic"

            [agents.beta]
            display_name = "Beta"
            package      = "q@^0.1"
            llm_backend  = "openai"
        "#;
        let cfg = parse(toml_str).unwrap();
        assert_eq!(cfg.agents.len(), 2);
        assert!(cfg.agents.contains_key("alpha"));
        assert!(cfg.agents.contains_key("beta"));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// Generate a name that's a valid TOML key (alphanumeric + underscore).
    fn ident_strategy() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9_]{0,15}"
    }

    /// Generate a non-empty free-form string (no quotes, no backslashes).
    fn safe_string_strategy() -> impl Strategy<Value = String> {
        "[A-Za-z0-9 .]{1,30}"
    }

    fn agent_entry_strategy() -> impl Strategy<Value = (String, UncheckedAgent)> {
        (
            ident_strategy(),
            safe_string_strategy(), // display_name
            ident_strategy(),       // package name
            ident_strategy(),       // llm_backend
        )
            .prop_map(|(id, dn, pkg, llm)| {
                (
                    id,
                    UncheckedAgent {
                        display_name: dn,
                        package: format!("{pkg}@^0.1"),
                        llm_backend: llm,
                        requires: None,
                        capabilities: None,
                        config: None,
                        prompt: None,
                    },
                )
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

        /// Round-trip: serialize an UncheckedProjectConfig to TOML, parse-and-validate,
        /// validate the resulting ProjectConfig has the same agent ids.
        #[test]
        fn round_trip_preserves_agent_ids(
            project_name in safe_string_strategy(),
            agents in proptest::collection::vec(agent_entry_strategy(), 0..=3)
        ) {
            // Deduplicate ids (TOML can't have duplicate keys; UncheckedProjectConfig uses BTreeMap).
            let mut agent_map: std::collections::BTreeMap<String, UncheckedAgent> =
                std::collections::BTreeMap::new();
            for (id, agent) in agents {
                agent_map.insert(id, agent);
            }

            let original = UncheckedProjectConfig {
                project: UncheckedProject {
                    name: project_name.clone(),
                    description: String::new(),
                },
                agents: agent_map.clone(),
            };

            let toml_str = toml::to_string(&original).unwrap();

            let parsed: UncheckedProjectConfig = toml::from_str(&toml_str).unwrap();
            let validated = parsed.validate().unwrap();

            prop_assert_eq!(validated.project_name, project_name);
            prop_assert_eq!(
                validated.agents.keys().cloned().collect::<Vec<_>>(),
                agent_map.keys().cloned().collect::<Vec<_>>()
            );
        }
    }
}
