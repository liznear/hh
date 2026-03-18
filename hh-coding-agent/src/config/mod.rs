//! Configuration types for the coding agent.
//!
//! These types define the configuration needed for tool execution and permission management.
//! File-based config loading is handled by the consumer of this crate.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Tool configuration settings.
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

/// Permission configuration settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionSettings {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ask: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
    pub read: String,
    pub list: String,
    pub glob: String,
    pub grep: String,
    pub write: String,
    #[serde(default = "default_edit_permission")]
    pub edit: String,
    #[serde(default = "default_todo_write_permission")]
    pub todo_write: String,
    #[serde(default = "default_todo_read_permission")]
    pub todo_read: String,
    #[serde(default = "default_question_permission")]
    pub question: String,
    #[serde(default = "default_task_permission")]
    pub task: String,
    pub bash: String,
    pub web: String,
    #[serde(default)]
    pub capabilities: BTreeMap<String, String>,
}

fn default_edit_permission() -> String {
    "ask".to_string()
}

fn default_todo_write_permission() -> String {
    "allow".to_string()
}

fn default_todo_read_permission() -> String {
    "allow".to_string()
}

fn default_question_permission() -> String {
    "allow".to_string()
}

fn default_task_permission() -> String {
    "allow".to_string()
}

impl Default for PermissionSettings {
    fn default() -> Self {
        Self {
            allow: Vec::new(),
            ask: Vec::new(),
            deny: Vec::new(),
            read: "allow".to_string(),
            list: "allow".to_string(),
            glob: "allow".to_string(),
            grep: "allow".to_string(),
            write: "ask".to_string(),
            edit: default_edit_permission(),
            todo_write: default_todo_write_permission(),
            todo_read: default_todo_read_permission(),
            question: default_question_permission(),
            task: default_task_permission(),
            bash: "ask".to_string(),
            web: "ask".to_string(),
            capabilities: BTreeMap::new(),
        }
    }
}

/// Session configuration settings.
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

/// Agent configuration settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettings {
    pub max_steps: usize,
    #[serde(default = "default_sub_agent_max_depth")]
    pub sub_agent_max_depth: usize,
    #[serde(default = "default_parallel_subagents")]
    pub parallel_subagents: bool,
    #[serde(default = "default_max_parallel_subagents")]
    pub max_parallel_subagents: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip)]
    pub instructions_context: Option<String>,
}

fn default_sub_agent_max_depth() -> usize {
    2
}

fn default_parallel_subagents() -> bool {
    true
}

fn default_max_parallel_subagents() -> usize {
    5
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_steps: 0,
            sub_agent_max_depth: default_sub_agent_max_depth(),
            parallel_subagents: default_parallel_subagents(),
            max_parallel_subagents: default_max_parallel_subagents(),
            system_prompt: None,
            instructions_context: None,
        }
    }
}

impl AgentSettings {
    pub fn resolved_system_prompt(&self) -> String {
        let base_prompt = self
            .system_prompt
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(crate::core::system_prompt::default_system_prompt);

        match self.instructions_context.as_deref().map(str::trim) {
            Some("") | None => base_prompt,
            Some(instructions) => format!("{base_prompt}\n\n{instructions}"),
        }
    }
}

/// Aggregated settings for the coding agent.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub tools: ToolSettings,
    #[serde(default)]
    pub permissions: PermissionSettings,
    #[serde(default)]
    pub session: SessionSettings,
    #[serde(default)]
    pub agent: AgentSettings,
}

impl Settings {
    pub fn permission_policy_for_capability(&self, capability: &str) -> &str {
        let permission = &self.permissions;
        if let Some(raw) = permission.capabilities.get(capability) {
            return raw;
        }

        match capability {
            "read" => &permission.read,
            "list" => &permission.list,
            "glob" => &permission.glob,
            "grep" => &permission.grep,
            "write" => &permission.write,
            "edit" => &permission.edit,
            "todo_write" => &permission.todo_write,
            "todo_read" => &permission.todo_read,
            "question" => &permission.question,
            "task" => &permission.task,
            "bash" => &permission.bash,
            "web" => &permission.web,
            _ => "deny",
        }
    }
}
