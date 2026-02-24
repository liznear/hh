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

    merge_settings_file(&mut settings, &global_config_path())?;
    merge_settings_file(&mut settings, &project_config_path(cwd))?;

    override_from_env(&mut settings.provider.model, "HH_MODEL");
    override_from_env(&mut settings.provider.base_url, "HH_BASE_URL");
    override_from_env(&mut settings.provider.api_key_env, "HH_API_KEY_ENV");

    Ok(settings)
}

fn merge_settings(base: &mut Settings, override_with: Settings) {
    base.provider = override_with.provider;
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
    let value: Settings =
        toml::from_str(&content).with_context(|| format!("failed parsing {}", path.display()))?;
    merge_settings(settings, value);

    Ok(())
}

fn override_from_env(target: &mut String, key: &str) {
    if let Ok(value) = env::var(key) {
        *target = value;
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
    let config_path = config_dir.join("config.toml");
    let default = Settings::default();
    let text = toml::to_string_pretty(&default)?;
    std::fs::write(&config_path, text)?;
    Ok(config_path)
}
