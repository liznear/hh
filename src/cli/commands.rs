use std::path::PathBuf;

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
    Chat {
        /// Also dump frames to files while running interactive TUI
        #[arg(long)]
        debug: Option<PathBuf>,
        /// Limit autonomous turns; unlimited when omitted
        #[arg(long, value_parser = parse_positive_usize)]
        max_turns: Option<usize>,
    },
    /// Replay debug frames from a directory
    Replay {
        /// Directory containing screen dump files
        dir: PathBuf,
        /// Delay between frames in milliseconds (default: 100)
        #[arg(short, long, default_value = "100")]
        delay: u64,
        /// Loop replay continuously
        #[arg(short, long)]
        loop_replay: bool,
    },
    /// Run one prompt and exit
    Run {
        prompt: String,
        /// Dump headless debug frames to this directory
        #[arg(long)]
        debug: Option<PathBuf>,
        /// Limit autonomous turns; unlimited when omitted
        #[arg(long, value_parser = parse_positive_usize)]
        max_turns: Option<usize>,
    },
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
