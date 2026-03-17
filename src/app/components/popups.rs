use crate::app::chat_state::ClipboardNotice;
use crate::app::core::{AppAction, Component};

#[derive(Default)]
pub struct PopupComponent {
    pub clipboard_notice: Option<ClipboardNotice>,
}

impl Component for PopupComponent {
    fn update(&mut self, action: &AppAction) -> Option<AppAction> {
        match action {
            AppAction::ShowClipboardNotice { x, y } => {
                self.clipboard_notice = Some(ClipboardNotice {
                    x: *x,
                    y: *y,
                    expires_at: std::time::Instant::now() + std::time::Duration::from_millis(1500),
                });
                Some(AppAction::Redraw)
            }
            AppAction::PeriodicTick => {
                if let Some(notice) = &self.clipboard_notice
                    && std::time::Instant::now() > notice.expires_at
                {
                    self.clipboard_notice = None;
                    return Some(AppAction::Redraw);
                }
                None
            }
            _ => None,
        }
    }
}
