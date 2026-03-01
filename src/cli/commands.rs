use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "hh", about = "Happy Harness", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Start interactive chat session
    Chat {
        /// Limit autonomous turns; unlimited when omitted
        #[arg(long, value_parser = parse_positive_usize)]
        max_turns: Option<usize>,
        /// Select agent to use
        #[arg(long)]
        agent: Option<String>,
    },

    /// Run one prompt and exit
    Run {
        prompt: String,
        /// Limit autonomous turns; unlimited when omitted
        #[arg(long, value_parser = parse_positive_usize)]
        max_turns: Option<usize>,
        /// Select agent to use
        #[arg(long)]
        agent: Option<String>,
    },
    /// List available agents
    Agents,
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

fn parse_positive_usize(raw: &str) -> Result<usize, String> {
    let value = raw
        .parse::<usize>()
        .map_err(|_| format!("invalid integer: {raw}"))?;
    if value == 0 {
        return Err("value must be greater than 0".to_string());
    }
    Ok(value)
}
