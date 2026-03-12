use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::core::{AppAction, Component};
use crate::app::events::InputEvent;

pub struct InputActionComponent;

impl Component for InputActionComponent {
    fn handle_event(&mut self, event: &InputEvent) -> Option<AppAction> {
        match event {
            InputEvent::Refresh => Some(AppAction::Redraw),
            InputEvent::ScrollUp { .. } => Some(AppAction::ScrollMessages(-3)),
            InputEvent::ScrollDown { .. } => Some(AppAction::ScrollMessages(3)),
            InputEvent::Key(key)
                if key.code == KeyCode::Esc && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                Some(AppAction::CancelExecution)
            }
            _ => None,
        }
    }
}
