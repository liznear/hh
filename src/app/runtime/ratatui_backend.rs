use ratatui::{backend::CrosstermBackend, Frame, Terminal};
use std::io::Stdout;

use super::FrameContext;
use crate::app::ui::UiRect;

pub type RatatuiTerminal = Terminal<CrosstermBackend<Stdout>>;

pub struct RatatuiFrameContext<'a> {
    frame: &'a mut Frame<'a>,
}

impl<'a> RatatuiFrameContext<'a> {
    pub fn new(frame: &'a mut Frame<'a>) -> Self {
        Self { frame }
    }

    pub fn frame(&mut self) -> &mut Frame<'a> {
        self.frame
    }
}

impl FrameContext for RatatuiFrameContext<'_> {
    fn area(&self) -> UiRect {
        self.frame.area().into()
    }
}

impl super::TerminalBackend for RatatuiTerminal {
    fn size(&self) -> Result<(u16, u16), std::io::Error> {
        let r = ratatui::Terminal::size(self)?;
        Ok((r.width, r.height))
    }
}
