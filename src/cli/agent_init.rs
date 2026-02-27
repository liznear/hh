use crate::agent::AgentRegistry;
use crate::cli::tui::AgentOptionView;
use crate::config::Settings;

/// Load and configure agents for the chat app
pub fn initialize_agents(
    settings: &Settings,
) -> anyhow::Result<(Vec<AgentOptionView>, Option<String>)> {
    let loader = crate::agent::AgentLoader::new()?;
    let agents = loader.load_agents()?;
    let registry = AgentRegistry::new(agents);

    // Convert to view models
    let agent_views: Vec<AgentOptionView> = registry
        .list_agents()
        .iter()
        .map(|agent| AgentOptionView {
            name: agent.name.clone(),
            display_name: agent.display_name.clone(),
            color: agent.color.clone(),
            mode: format!("{:?}", agent.mode).to_lowercase(),
        })
        .collect();

    // Use selected agent from settings, or default to "build"
    let selected_agent = settings
        .selected_agent
        .clone()
        .or_else(|| {
            // Check if "build" is available, otherwise use first primary agent
            if agent_views.iter().any(|a| a.name == "build") {
                Some("build".to_string())
            } else if let Some(first_primary) = agent_views.iter().find(|a| a.mode == "primary") {
                Some(first_primary.name.clone())
            } else {
                None
            }
        });

    Ok((agent_views, selected_agent))
}
