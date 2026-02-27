mod app;
mod commands;
mod debug;
mod event;
mod markdown;
mod terminal;
mod tool_presentation;
mod tool_render;
mod ui;

pub use app::{ChatApp, ChatMessage, SubmittedInput};
pub use commands::SlashCommand;
pub use debug::DebugRenderer;
pub use event::{TuiEvent, TuiEventSender};
pub use terminal::{Tui, TuiGuard, restore_terminal, setup_terminal};
pub use ui::{build_message_lines, render_app};

#[cfg(test)]
mod ui_tests;
