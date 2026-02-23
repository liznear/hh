mod app;
mod debug;
mod event;
mod terminal;
mod ui;

pub use app::{ChatApp, ChatMessage};
pub use debug::DebugRenderer;
pub use event::{TuiEvent, TuiEventSender};
pub use terminal::{TuiGuard, restore_terminal, setup_terminal};
pub use ui::{build_message_lines, build_message_lines_internal, render_app};
