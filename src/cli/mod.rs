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
    let settings = load_settings(&cwd)?;

    match cli.command {
        Commands::Chat => chat::run_chat(settings, &cwd).await,
        Commands::Run { prompt } => {
            let render = LiveRender::new();
            render.begin_turn();
            chat::run_single_prompt_with_events(settings, &cwd, prompt, render).await?;
            Ok(())
        }
        Commands::Tools => {
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
                let txt = toml::to_string_pretty(&settings)?;
                println!("{}", txt);
                Ok(())
            }
        },
    }
}
