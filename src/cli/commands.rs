use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "hh", about = "Happy Harness", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Start interactive chat session
    Chat,
    /// Run one prompt and exit
    Run { prompt: String },
    /// List available tools
    Tools,
    /// Manage configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Init,
    Show,
}
