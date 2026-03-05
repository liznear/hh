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
    SubagentStatusView, SubmittedInput, TodoItemView, TodoPriority, TodoStatus,
};
pub use commands::SlashCommand;
pub use event::{ScopedTuiEvent, SubagentEventItem, TuiEvent, TuiEventSender};
pub use terminal::{Tui, TuiGuard, restore_terminal, setup_terminal};
pub use ui::{build_message_lines, render_app};
pub(crate) use ui::{compute_layout_rects, sidebar_section_header_hitboxes};

#[cfg(test)]
mod ui_tests;
