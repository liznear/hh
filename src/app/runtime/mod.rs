pub mod iocraft_backend;
pub mod ratatui_backend;

use crate::app::ui::UiRect;

pub trait FrameContext {
    fn area(&self) -> UiRect;
}

pub trait TerminalBackend {
    fn size(&self) -> Result<(u16, u16), std::io::Error>;
}

pub use ratatui_backend::{RatatuiFrameContext, RatatuiTerminal};
