use crate::agent::{AgentLoader, AgentRegistry};
use crate::config::settings::Settings;
use anyhow::Context;
use serde_json::Value;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub fn global_config_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".config/hh/config.json")
}

pub fn global_instructions_path_for_home(home: &Path) -> Option<PathBuf> {
    [
        home.join(".config/hh/AGENTS.md"),
        home.join(".agents/AGENTS.md"),
        home.join(".config/claude/CLAUDE.md"),
    ]
    .into_iter()
    .find(|path| path.exists())
}

pub fn project_config_path(cwd: &Path) -> PathBuf {
    cwd.join(".hh/config.json")
}

pub fn project_instructions_path(cwd: &Path) -> Option<PathBuf> {
    [cwd.join("AGENTS.md"), cwd.join("CLAUDE.md")]
        .into_iter()
        .find(|path| path.exists())
}

pub fn claude_project_config_path(cwd: &Path) -> PathBuf {
    cwd.join(".claude/settings.json")
}

pub fn local_config_path(cwd: &Path) -> PathBuf {
    cwd.join(".hh/config.local.json")
}

pub fn claude_local_config_path(cwd: &Path) -> PathBuf {
    cwd.join(".claude/settings.local.json")
}

pub fn load_settings(cwd: &Path, agent_name: Option<String>) -> anyhow::Result<Settings> {
    let mut merged = serde_json::to_value(Settings::default()).context("serialize defaults")?;
    merge_settings_file(&mut merged, &global_config_path())?;

    for path in ancestor_config_paths(cwd, ".claude/settings.json") {
        merge_settings_file(&mut merged, &path)?;
    }
    for path in ancestor_config_paths(cwd, ".hh/config.json") {
        merge_settings_file(&mut merged, &path)?;
    }
    for path in ancestor_config_paths(cwd, ".claude/settings.local.json") {
        merge_settings_file(&mut merged, &path)?;
    }
    for path in ancestor_config_paths(cwd, ".hh/config.local.json") {
        merge_settings_file(&mut merged, &path)?;
    }

    let mut settings: Settings =
        serde_json::from_value(merged).context("deserialize merged settings")?;
    settings.session.root = expand_path(&settings.session.root);

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
    override_ui_renderer_mode(&mut settings);

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

    settings.agent.instructions_context = Some(load_instruction_context(cwd)?);

    Ok(settings)
}

fn load_instruction_context(cwd: &Path) -> anyhow::Result<String> {
    let mut sections = Vec::new();

    if let Some(home) = dirs::home_dir()
        && let Some(path) = global_instructions_path_for_home(&home)
    {
        sections.push(load_instruction_section("Global instructions", &path)?);
    }

    if let Some(path) = project_instructions_path(cwd) {
        sections.push(load_instruction_section("Project instructions", &path)?);
    }

    Ok(sections.join("\n\n"))
}

fn load_instruction_section(label: &str, path: &Path) -> anyhow::Result<String> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed reading instruction file {}", path.display()))?;
    Ok(format!(
        "<{} path=\"{}\">\n{}\n</{}>",
        label,
        path.display(),
        content,
        label
    ))
}

fn merge_settings_file(base: &mut Value, path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("failed reading {}", path.display()))?;
    let value: Value = serde_json::from_str(&content)
        .with_context(|| format!("failed parsing {}", path.display()))?;
    merge_json_value(base, value, &mut Vec::new());

    Ok(())
}

fn ancestor_config_paths(cwd: &Path, relative_path: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut current = Some(cwd);

    while let Some(dir) = current {
        paths.push(dir.join(relative_path));
        current = dir.parent();
    }

    paths.reverse();
    paths
}

fn merge_json_value(base: &mut Value, incoming: Value, path: &mut Vec<String>) {
    match (base, incoming) {
        (Value::Object(base_obj), Value::Object(incoming_obj)) => {
            for (key, incoming_value) in incoming_obj {
                if let Some(base_value) = base_obj.get_mut(&key) {
                    path.push(key.clone());
                    merge_json_value(base_value, incoming_value, path);
                    path.pop();
                } else {
                    base_obj.insert(key, incoming_value);
                }
            }
        }
        (Value::Array(base_arr), Value::Array(incoming_arr))
            if is_merged_permission_rule_array(path) =>
        {
            for item in incoming_arr {
                if !base_arr.contains(&item) {
                    base_arr.push(item);
                }
            }
        }
        (base_slot, incoming_value) => {
            *base_slot = incoming_value;
        }
    }
}

fn is_merged_permission_rule_array(path: &[String]) -> bool {
    if path.len() < 2 {
        return false;
    }

    let Some(last) = path.last() else {
        return false;
    };

    if !matches!(last.as_str(), "allow" | "ask" | "deny") {
        return false;
    }

    path[path.len() - 2].as_str() == "permissions"
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

fn override_ui_renderer_mode(settings: &mut Settings) {
    let Ok(value) = env::var("HH_UI_RENDERER_MODE") else {
        return;
    };

    let mode = match value.trim() {
        "legacy-lines" => crate::config::settings::UiRendererMode::LegacyLines,
        "widget-blocks" => crate::config::settings::UiRendererMode::WidgetBlocks,
        _ => return,
    };

    settings.ui.renderer_mode = mode;
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

pub fn upsert_local_permission_rule(
    cwd: &std::path::Path,
    list_name: &str,
    rule: &str,
) -> anyhow::Result<()> {
    if !matches!(list_name, "allow" | "deny") {
        anyhow::bail!("unsupported permission list: {list_name}");
    }

    let config_path = local_config_path(cwd);
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut root = if config_path.exists() {
        let raw = std::fs::read_to_string(&config_path)?;
        serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| Value::Object(Default::default()))
    } else {
        Value::Object(Default::default())
    };

    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("local config must be a JSON object"))?;
    let permissions = root_obj
        .entry("permissions")
        .or_insert_with(|| Value::Object(Default::default()));
    let permissions_obj = permissions
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("permissions must be a JSON object"))?;

    let list = permissions_obj
        .entry(list_name)
        .or_insert_with(|| Value::Array(Vec::new()));
    let list_arr = list
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("permissions.{list_name} must be an array"))?;

    let candidate = Value::String(rule.to_string());
    if !list_arr.contains(&candidate) {
        list_arr.push(candidate);
    }

    let text = serde_json::to_string_pretty(&root)?;
    std::fs::write(config_path, text)?;
    Ok(())
}
