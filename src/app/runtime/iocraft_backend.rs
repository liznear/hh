use super::FrameContext;
use crate::app::ui::UiRect;

pub struct IocraftBackend {
    // Will be populated in Phase 3
}

impl IocraftBackend {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for IocraftBackend {
    fn default() -> Self {
        Self::new()
    }
}

pub struct IocraftFrameContext {
    // Will be populated in Phase 3
}

impl FrameContext for IocraftFrameContext {
    fn area(&self) -> UiRect {
        UiRect::default()
    }
}

impl super::TerminalBackend for IocraftBackend {
    fn size(&self) -> Result<(u16, u16), std::io::Error> {
        Ok((100, 40)) // Dummy
    }
}
