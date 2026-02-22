use crate::config::settings::Settings;
use anyhow::Context;
use std::{env, fs, path::PathBuf};

pub fn global_config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("hh/config.toml")
}

pub fn project_config_path(cwd: &std::path::Path) -> PathBuf {
    cwd.join(".hh/config.toml")
}

pub fn load_settings(cwd: &std::path::Path) -> anyhow::Result<Settings> {
    let mut settings = Settings::default();

    let global_path = global_config_path();
    if global_path.exists() {
        let content = fs::read_to_string(&global_path)
            .with_context(|| format!("failed reading {}", global_path.display()))?;
        let value: Settings = toml::from_str(&content)
            .with_context(|| format!("failed parsing {}", global_path.display()))?;
        merge_settings(&mut settings, value);
    }

    let project_path = project_config_path(cwd);
    if project_path.exists() {
        let content = fs::read_to_string(&project_path)
            .with_context(|| format!("failed reading {}", project_path.display()))?;
        let value: Settings = toml::from_str(&content)
            .with_context(|| format!("failed parsing {}", project_path.display()))?;
        merge_settings(&mut settings, value);
    }

    if let Ok(model) = env::var("HH_MODEL") {
        settings.provider.model = model;
    }
    if let Ok(base_url) = env::var("HH_BASE_URL") {
        settings.provider.base_url = base_url;
    }
    if let Ok(api_key_env) = env::var("HH_API_KEY_ENV") {
        settings.provider.api_key_env = api_key_env;
    }

    Ok(settings)
}

fn merge_settings(base: &mut Settings, override_with: Settings) {
    base.provider = override_with.provider;
    base.agent = override_with.agent;
    base.tools = override_with.tools;
    base.permission = override_with.permission;
    base.session = override_with.session;
}

pub fn write_default_project_config(cwd: &std::path::Path) -> anyhow::Result<PathBuf> {
    let config_dir = cwd.join(".hh");
    std::fs::create_dir_all(&config_dir)?;
    let config_path = config_dir.join("config.toml");
    let default = Settings::default();
    let text = toml::to_string_pretty(&default)?;
    std::fs::write(&config_path, text)?;
    Ok(config_path)
}
