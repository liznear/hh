mod app;
mod commands;
mod debug;
mod event;
mod terminal;
mod tool_presentation;
mod ui;

pub use app::{ChatApp, ChatMessage};
pub use commands::SlashCommand;
pub use debug::DebugRenderer;
pub use event::{TuiEvent, TuiEventSender};
pub use terminal::{restore_terminal, setup_terminal, TuiGuard};
pub use ui::{build_message_lines, build_message_lines_internal, render_app};

#[cfg(test)]
mod ui_tests;
