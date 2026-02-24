use crate::cli::tui::ChatApp;
#[derive(Clone)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
}

impl SlashCommand {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
        }
    }
}

pub fn get_default_commands() -> Vec<SlashCommand> {
    vec![
        SlashCommand::new("/quit", "Exit the application"),
        SlashCommand::new("/resume", "Resume a previous session"),
    ]
}
