pub mod chat_state;
pub mod components;
pub mod core;
pub mod events;
pub mod handlers;
pub mod input;
pub mod iocraft;
pub mod render;
pub mod runtime;
pub mod state;
pub mod ui;
pub mod utils;

use crate::config::Settings;
use std::path::Path;

pub async fn run_interactive_chat(settings: Settings, cwd: &Path) -> anyhow::Result<()> {
    crate::app::iocraft::run_iocraft_app(settings, cwd.to_path_buf()).await
}

pub async fn run_single_prompt(
    settings: Settings,
    cwd: &Path,
    prompt: String,
) -> anyhow::Result<String> {
    crate::app::handlers::runner::run_single_prompt(settings, cwd, prompt).await
}
