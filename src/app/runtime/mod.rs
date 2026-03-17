pub mod iocraft_backend;
pub mod ratatui_backend;

use crate::app::ui::UiRect;

pub trait FrameContext {
    fn area(&self) -> UiRect;
}

pub use ratatui_backend::{RatatuiFrameContext, RatatuiTerminal};
