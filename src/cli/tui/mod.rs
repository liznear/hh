mod app;
mod event;
mod terminal;
mod ui;

pub use app::ChatApp;
pub use event::{TuiEvent, TuiEventSender};
pub use terminal::{restore_terminal, setup_terminal, TuiGuard};
pub use ui::render_app;
