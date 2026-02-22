use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_steps: 12,
            token_budget: 32_000,
        }
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
    pub bash: String,
    pub web: String,
}

impl Default for PermissionSettings {
    fn default() -> Self {
        Self {
            read: "allow".to_string(),
            list: "allow".to_string(),
            glob: "allow".to_string(),
            grep: "allow".to_string(),
            write: "ask".to_string(),
            bash: "ask".to_string(),
            web: "ask".to_string(),
        }
    }
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
