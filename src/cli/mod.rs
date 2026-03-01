pub mod agent_init;
pub mod chat;
pub mod commands;
pub mod render;
pub mod tui;

use crate::cli::commands::{Cli, Commands, ConfigCommand};
use crate::cli::render::LiveRender;
use crate::config::{load_settings, write_default_project_config};
use clap::Parser;

pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;

    match cli.command.unwrap_or(Commands::Chat {
        max_turns: None,
        agent: None,
    }) {
        Commands::Chat { max_turns, agent } => {
            let settings = load_settings(&cwd, agent)?;
            let settings = apply_max_turns(settings, max_turns);
            chat::run_chat(settings, &cwd).await
        }
        Commands::Run {
            prompt,
            max_turns,
            agent,
        } => {
            let settings = load_settings(&cwd, agent)?;
            let settings = apply_max_turns(settings, max_turns);
            let render = LiveRender::new();
            render.begin_turn();
            chat::run_single_prompt_with_events(settings, &cwd, prompt, render).await?;
            Ok(())
        }
        Commands::Tools => {
            let settings = load_settings(&cwd, None)?;
            let registry = crate::tool::registry::ToolRegistry::new(&settings, &cwd);
            for name in registry.names() {
                println!("{}", name);
            }
            Ok(())
        }
        Commands::Config { command } => match command {
            ConfigCommand::Init => {
                let path = write_default_project_config(&cwd)?;
                println!("wrote {}", path.display());
                Ok(())
            }
            ConfigCommand::Show => {
                let settings = load_settings(&cwd, None)?;
                let txt = serde_json::to_string_pretty(&settings)?;
                println!("{}", txt);
                Ok(())
            }
        },
        Commands::Agents => {
            let loader = crate::agent::AgentLoader::new()?;
            let agents = loader.load_agents()?;
            let registry = crate::agent::AgentRegistry::new(agents);

            println!("Available agents:");
            for agent in registry.list_agents() {
                let mode = if agent.mode == crate::agent::AgentMode::Primary {
                    "primary"
                } else {
                    "subagent"
                };
                println!(
                    "  {} ({}) - {} - {}",
                    agent.name, agent.display_name, mode, agent.description
                );
            }
            Ok(())
        }
    }
}

fn apply_max_turns(
    mut settings: crate::config::Settings,
    max_turns: Option<usize>,
) -> crate::config::Settings {
    if let Some(max_turns) = max_turns {
        settings.agent.max_steps = max_turns;
    }
    settings
}
