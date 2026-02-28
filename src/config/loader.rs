use crate::agent::{AgentLoader, AgentRegistry};
use crate::config::settings::Settings;
use anyhow::Context;
use std::{env, fs, path::PathBuf};

pub fn global_config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("hh/config.json")
}

pub fn project_config_path(cwd: &std::path::Path) -> PathBuf {
    cwd.join(".hh/config.json")
}

pub fn load_settings(
    cwd: &std::path::Path,
    agent_name: Option<String>,
) -> anyhow::Result<Settings> {
    let mut settings = Settings::default();

    merge_settings_file(&mut settings, &global_config_path())?;
    merge_settings_file(&mut settings, &project_config_path(cwd))?;

    settings.normalize_models();
    override_from_env(&mut settings.models.default, "HH_MODEL");
    settings.normalize_models();
    override_selected_provider_field(&mut settings, "HH_BASE_URL", |provider, value| {
        provider.base_url = value;
    });
    override_selected_provider_field(&mut settings, "HH_API_KEY_ENV", |provider, value| {
        provider.api_key_env = value;
    });
    override_optional_from_env(&mut settings.agent.system_prompt, "HH_SYSTEM_PROMPT");

    // Apply agent settings if specified
    if let Some(name) = agent_name {
        settings.selected_agent = Some(name.clone());
        let loader = AgentLoader::new()?;
        let agents = loader.load_agents()?;
        let registry = AgentRegistry::new(agents);

        if let Some(agent) = registry.get_agent(&name) {
            settings.apply_agent_settings(agent);
        }
    }

    Ok(settings)
}

fn merge_settings(base: &mut Settings, override_with: Settings) {
    base.models = override_with.models;
    base.providers = override_with.providers;
    base.agent = override_with.agent;
    base.tools = override_with.tools;
    base.permission = override_with.permission;
    base.session.root = expand_path(&override_with.session.root);
}

fn merge_settings_file(settings: &mut Settings, path: &std::path::Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("failed reading {}", path.display()))?;
    let value: Settings = serde_json::from_str(&content)
        .with_context(|| format!("failed parsing {}", path.display()))?;
    merge_settings(settings, value);

    Ok(())
}

fn override_from_env(target: &mut String, key: &str) {
    if let Ok(value) = env::var(key) {
        *target = value;
    }
}

fn override_optional_from_env(target: &mut Option<String>, key: &str) {
    if let Ok(value) = env::var(key) {
        *target = Some(value);
    }
}

fn override_selected_provider_field(
    settings: &mut Settings,
    key: &str,
    mut apply: impl FnMut(&mut crate::config::settings::ProviderConfig, String),
) {
    let Ok(value) = env::var(key) else {
        return;
    };

    let Some((provider_id, _)) = settings.models.default.split_once('/') else {
        return;
    };

    if let Some(provider) = settings.providers.get_mut(provider_id) {
        apply(provider, value);
    }
}

fn expand_path(path: &std::path::Path) -> PathBuf {
    let path_str = path.to_string_lossy();

    if let Some(home) = dirs::home_dir() {
        if path_str == "~" {
            return home;
        }
        if path_str.starts_with('~') {
            return home.join(path_str[2..].trim_start_matches('/'));
        }
        if let Some(rest) = path_str.strip_prefix("$HOME") {
            return home.join(rest.trim_start_matches('/'));
        }
        if let Some(rest) = path_str.strip_prefix("${HOME}") {
            return home.join(rest.trim_start_matches('/'));
        }
    }

    path.to_path_buf()
}

pub fn write_default_project_config(cwd: &std::path::Path) -> anyhow::Result<PathBuf> {
    let config_dir = cwd.join(".hh");
    std::fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join("config.json");
    let mut default = Settings::default();
    default.normalize_models();
    let text = serde_json::to_string_pretty(&default)?;
    std::fs::write(&config_path, text)?;
    Ok(config_path)
}
