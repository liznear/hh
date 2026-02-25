use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::core::system_prompt::default_system_prompt;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    pub provider: ProviderSettings,
    pub agent: AgentSettings,
    pub tools: ToolSettings,
    pub permission: PermissionSettings,
    pub session: SessionSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSettings {
    pub base_url: String,
    pub model: String,
    pub api_key_env: String,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4.1-mini".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettings {
    pub max_steps: usize,
    pub token_budget: usize,
    #[serde(default = "default_sub_agent_max_depth")]
    pub sub_agent_max_depth: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_steps: 12,
            token_budget: 32_000,
            sub_agent_max_depth: default_sub_agent_max_depth(),
            system_prompt: None,
        }
    }
}

fn default_sub_agent_max_depth() -> usize {
    2
}

impl AgentSettings {
    pub fn resolved_system_prompt(&self) -> String {
        self.system_prompt
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(default_system_prompt)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSettings {
    pub fs: bool,
    pub bash: bool,
    pub web: bool,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            fs: true,
            bash: true,
            web: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionSettings {
    pub read: String,
    pub list: String,
    pub glob: String,
    pub grep: String,
    pub write: String,
    #[serde(default = "default_edit_permission")]
    pub edit: String,
    #[serde(default = "default_todo_write_permission")]
    pub todo_write: String,
    pub bash: String,
    pub web: String,
    #[serde(default)]
    pub capabilities: BTreeMap<String, String>,
}

impl Default for PermissionSettings {
    fn default() -> Self {
        Self {
            read: "allow".to_string(),
            list: "allow".to_string(),
            glob: "allow".to_string(),
            grep: "allow".to_string(),
            write: "ask".to_string(),
            edit: default_edit_permission(),
            todo_write: default_todo_write_permission(),
            bash: "ask".to_string(),
            web: "ask".to_string(),
            capabilities: BTreeMap::new(),
        }
    }
}

fn default_edit_permission() -> String {
    "ask".to_string()
}

fn default_todo_write_permission() -> String {
    "allow".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSettings {
    pub root: PathBuf,
}

impl Default for SessionSettings {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            root: home.join(".local/state/hh/sessions"),
        }
    }
}
