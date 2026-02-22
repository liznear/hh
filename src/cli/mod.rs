pub mod chat;
pub mod commands;
pub mod replay;
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
        Commands::Chat {
            debug_headless,
            debug_dir,
            debug,
        } => {
            if debug_headless {
                let dir = debug_dir.unwrap_or_else(chat::generate_debug_dir);
                // In debug mode, read prompt from stdin or use empty
                let prompt = std::io::stdin().lines().next().transpose()?.unwrap_or_default();
                if prompt.is_empty() {
                    anyhow::bail!("--debug-headless requires a prompt via stdin, e.g.: echo 'your prompt' | hh chat --debug-headless");
                }
                chat::run_chat_debug_with_prompt(settings, &cwd, dir, prompt).await
            } else if let Some(debug_path) = debug {
                // Interactive mode with debug dumping
                chat::run_chat_with_debug(settings, &cwd, debug_path).await
            } else {
                chat::run_chat(settings, &cwd).await
            }
        }
        Commands::Replay {
            dir,
            delay,
            loop_replay,
        } => {
            replay::replay_frames(&dir, delay, loop_replay)?;
            Ok(())
        }
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
