use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::core::system_prompt::default_system_prompt;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub models: ModelSettings,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    pub agent: AgentSettings,
    pub tools: ToolSettings,
    pub permission: PermissionSettings,
    pub session: SessionSettings,
    #[serde(default)]
    pub selected_agent: Option<String>,
    #[serde(default)]
    pub agents: BTreeMap<String, AgentSpecificSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSettings {
    #[serde(default = "default_model_ref")]
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub display_name: String,
    pub base_url: String,
    pub api_key_env: String,
    #[serde(default)]
    pub models: BTreeMap<String, ModelMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelMetadata {
    #[serde(default, alias = "provider_model_id")]
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub modalities: ModelModalities,
    #[serde(default)]
    pub limits: ModelLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelModalityType {
    #[default]
    Text,
    Image,
    Audio,
    Video,
}

impl std::fmt::Display for ModelModalityType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Text => "text",
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
        };
        f.write_str(label)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelModalities {
    #[serde(default = "default_input_modalities")]
    pub input: Vec<ModelModalityType>,
    #[serde(default = "default_output_modalities")]
    pub output: Vec<ModelModalityType>,
}

impl Default for ModelModalities {
    fn default() -> Self {
        Self {
            input: default_input_modalities(),
            output: default_output_modalities(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelLimits {
    #[serde(default = "default_model_context_limit")]
    pub context: usize,
    #[serde(default = "default_model_output_limit")]
    pub output: usize,
}

impl Default for ModelLimits {
    fn default() -> Self {
        Self {
            context: default_model_context_limit(),
            output: default_model_output_limit(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedModel<'a> {
    pub provider_id: String,
    pub model_id: String,
    pub provider: &'a ProviderConfig,
    pub model: &'a ModelMetadata,
}

impl<'a> ResolvedModel<'a> {
    pub fn full_id(&self) -> String {
        format!("{}/{}", self.provider_id, self.model_id)
    }
}

impl Settings {
    pub fn selected_model_ref(&self) -> &str {
        self.models.default.as_str()
    }

    pub fn selected_model(&self) -> Option<ResolvedModel<'_>> {
        self.resolve_model_ref(self.models.default.as_str())
    }

    pub fn resolve_model_ref(&self, model_ref: &str) -> Option<ResolvedModel<'_>> {
        let (provider_id, model_id) = split_model_ref(model_ref)?;
        let provider = self.providers.get(provider_id)?;
        let model = provider.models.get(model_id)?;
        Some(ResolvedModel {
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
            provider,
            model,
        })
    }

    pub fn model_refs(&self) -> Vec<String> {
        let mut refs = Vec::new();
        for (provider_id, provider) in &self.providers {
            for model_id in provider.models.keys() {
                refs.push(format!("{provider_id}/{model_id}"));
            }
        }
        refs
    }

    pub fn normalize_models(&mut self) {
        if self.models.default.trim().is_empty() {
            self.models.default = default_model_ref();
        }

        if self.providers.is_empty() {
            self.providers = default_providers();
        }

        if !self.models.default.contains('/')
            && let Some(provider_id) = self.providers.keys().next().cloned()
        {
            self.models.default = format!("{provider_id}/{}", self.models.default);
        }

        if let Some((provider_id, model_id)) = split_model_ref(self.models.default.as_str())
            && let Some(provider) = self.providers.get_mut(provider_id)
            && !provider.models.contains_key(model_id)
        {
            provider.models.insert(
                model_id.to_string(),
                ModelMetadata {
                    id: model_id.to_string(),
                    display_name: model_id.to_string(),
                    modalities: ModelModalities::default(),
                    limits: ModelLimits::default(),
                },
            );
        }

        for provider in self.providers.values_mut() {
            for (model_id, model) in &mut provider.models {
                if model.id.trim().is_empty() {
                    model.id = model_id.clone();
                }
            }
        }

        if self.selected_model().is_none()
            && let Some((provider_id, provider)) = self.providers.iter().next()
            && let Some((model_id, _)) = provider.models.iter().next()
        {
            self.models.default = format!("{provider_id}/{model_id}");
        }
    }
}

impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            default: default_model_ref(),
        }
    }
}

fn default_provider_id() -> String {
    "openai".to_string()
}

fn default_provider_model() -> String {
    "gpt-4.1-mini".to_string()
}

fn default_model_ref() -> String {
    format!("{}/{}", default_provider_id(), default_provider_model())
}

fn default_provider_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

fn default_provider_display_name() -> String {
    "OpenAI".to_string()
}

fn default_providers() -> BTreeMap<String, ProviderConfig> {
    let mut providers = BTreeMap::new();
    providers.insert(
        default_provider_id(),
        ProviderConfig {
            display_name: default_provider_display_name(),
            base_url: default_provider_base_url(),
            api_key_env: default_api_key_env(),
            models: BTreeMap::from([(
                default_provider_model(),
                ModelMetadata {
                    id: default_provider_model(),
                    display_name: "GPT-4.1 mini".to_string(),
                    modalities: ModelModalities::default(),
                    limits: ModelLimits::default(),
                },
            )]),
        },
    );
    providers
}

fn split_model_ref(model_ref: &str) -> Option<(&str, &str)> {
    let (provider_id, model_id) = model_ref.split_once('/')?;
    let provider_id = provider_id.trim();
    let model_id = model_id.trim();
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }
    Some((provider_id, model_id))
}

fn default_input_modalities() -> Vec<ModelModalityType> {
    vec![ModelModalityType::Text, ModelModalityType::Image]
}

fn default_output_modalities() -> Vec<ModelModalityType> {
    vec![ModelModalityType::Text]
}

fn default_model_context_limit() -> usize {
    128_000
}

fn default_model_output_limit() -> usize {
    128_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettings {
    pub max_steps: usize,
    #[serde(default = "default_sub_agent_max_depth")]
    pub sub_agent_max_depth: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            max_steps: 0,
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
    #[serde(default = "default_todo_read_permission")]
    pub todo_read: String,
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
            todo_read: default_todo_read_permission(),
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

fn default_todo_read_permission() -> String {
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSpecificSettings {
    #[serde(default)]
    pub model: Option<String>,
}

impl Settings {
    pub fn apply_agent_settings(&mut self, agent: &crate::agent::AgentConfig) {
        // Apply agent system prompt if specified
        if let Some(prompt) = &agent.system_prompt {
            self.agent.system_prompt = Some(prompt.clone());
        }

        // Apply agent model or use global override
        let model_to_use = agent
            .model
            .as_ref()
            .or_else(|| self.agents.get(&agent.name).and_then(|s| s.model.as_ref()))
            .or_else(|| Some(&self.models.default));

        if let Some(model) = model_to_use {
            self.models.default = model.clone();
        }

        // Apply permission overrides
        for (capability, policy) in &agent.permission_overrides {
            match capability.as_str() {
                "read" => self.permission.read = policy.clone(),
                "list" => self.permission.list = policy.clone(),
                "glob" => self.permission.glob = policy.clone(),
                "grep" => self.permission.grep = policy.clone(),
                "write" => self.permission.write = policy.clone(),
                "edit" => self.permission.edit = policy.clone(),
                "todo_write" => self.permission.todo_write = policy.clone(),
                "todo_read" => self.permission.todo_read = policy.clone(),
                "bash" => self.permission.bash = policy.clone(),
                "web" => self.permission.web = policy.clone(),
                _ => {
                    self.permission
                        .capabilities
                        .insert(capability.clone(), policy.clone());
                }
            }
        }
    }
}
