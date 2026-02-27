use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Primary,
    Subagent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub display_name: String,
    pub description: String,
    #[serde(default = "default_mode")]
    pub mode: AgentMode,
    #[serde(default)]
    pub permission_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    /// System prompt - can be set in frontmatter or from Markdown body
    #[serde(default)]
    pub system_prompt: Option<String>,
}

impl AgentConfig {
    pub fn builtin_build() -> Self {
        Self {
            name: "build".to_string(),
            display_name: "Build".to_string(),
            description: "Build agent with standard permissions".to_string(),
            mode: AgentMode::Primary,
            system_prompt: Some(crate::core::system_prompt::build_system_prompt()),
            permission_overrides: BTreeMap::new(),
            model: None,
            color: Some("blue".to_string()),
        }
    }

    pub fn builtin_plan() -> Self {
        let mut overrides = BTreeMap::new();
        overrides.insert("write".to_string(), "deny".to_string());
        overrides.insert("edit".to_string(), "deny".to_string());

        Self {
            name: "plan".to_string(),
            display_name: "Plan".to_string(),
            description: "Planning agent that analyzes without executing".to_string(),
            mode: AgentMode::Primary,
            system_prompt: Some(crate::core::system_prompt::plan_system_prompt()),
            permission_overrides: overrides,
            model: None,
            color: Some("pink".to_string()),
        }
    }
}

fn default_mode() -> AgentMode {
    AgentMode::Subagent
}

// Frontmatter structure for parsing agent Markdown files
#[derive(Debug, Deserialize)]
pub struct AgentFrontmatter {
    pub display_name: String,
    pub description: String,
    #[serde(default = "default_mode")]
    pub mode: AgentMode,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub tools: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub system_prompt: Option<String>,
}

impl AgentFrontmatter {
    pub fn to_agent_config(&self, name: String, body: Option<String>) -> AgentConfig {
        let mut permission_overrides = BTreeMap::new();
        if let Some(tools) = &self.tools {
            for (tool, policy) in tools {
                permission_overrides.insert(tool.clone(), policy.clone());
            }
        }

        // Use body as system prompt if provided, otherwise use frontmatter field
        let system_prompt = if let Some(body) = body {
            if body.trim().is_empty() {
                self.system_prompt.clone()
            } else {
                Some(body)
            }
        } else {
            self.system_prompt.clone()
        };

        AgentConfig {
            name,
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            mode: self.mode,
            permission_overrides,
            model: self.model.clone(),
            color: self.color.clone(),
            system_prompt,
        }
    }
}
