mod app;
mod commands;
mod debug;
mod event;
mod markdown;
mod terminal;
mod tool_presentation;
mod tool_render;
mod ui;

pub use app::{
    AgentOptionView, ChatApp, ChatMessage, ModelOptionView, QuestionKeyResult, SubagentItemView,
    SubagentStatusView, SubmittedInput,
};
pub use commands::SlashCommand;
pub use debug::DebugRenderer;
pub use event::{ScopedTuiEvent, SubagentEventItem, TuiEvent, TuiEventSender};
pub use terminal::{Tui, TuiGuard, restore_terminal, setup_terminal};
pub use ui::{build_message_lines, render_app};
pub(crate) use ui::{build_sidebar_lines, compute_layout_rects};

#[cfg(test)]
mod ui_tests;
