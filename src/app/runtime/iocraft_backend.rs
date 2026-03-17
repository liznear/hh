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
