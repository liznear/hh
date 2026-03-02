mod app;
mod commands;
mod event;
mod markdown;
mod terminal;
mod tool_presentation;
mod tool_render;
mod ui;
mod viewport_cache;

pub use app::{
    AgentOptionView, ChatApp, ChatMessage, ModelOptionView, QuestionKeyResult, SubagentItemView,
    SubagentStatusView, SubmittedInput,
};
pub use commands::SlashCommand;
pub use event::{ScopedTuiEvent, SubagentEventItem, TuiEvent, TuiEventSender};
pub use terminal::{Tui, TuiGuard, restore_terminal, setup_terminal};
pub(crate) use ui::compute_layout_rects;
pub use ui::{build_message_lines, render_app};

#[cfg(test)]
mod ui_tests;
