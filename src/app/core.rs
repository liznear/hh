use ratatui::layout::Rect;

use crate::app::components::commands::SlashCommand;
use crate::app::events::TuiEvent;
use crate::app::input::InputEvent;
use crate::app::state::SessionContext;
use crate::core::MessageAttachment;

pub trait Component {
    fn update(&mut self, _action: &AppAction) -> Option<AppAction> {
        None
    }

    fn handle_event(&mut self, _event: &InputEvent) -> Option<AppAction> {
        None
    }

    fn render(&self, _f: &mut ratatui::Frame<'_>, _area: Rect, _ctx: &SessionContext) {}
}

#[derive(Debug, Clone)]
pub enum AppAction {
    Quit,
    Redraw,
    Input(InputEvent),
    PeriodicTick,
    SubmitInput(String, Vec<MessageAttachment>),
    RunSlashCommand(SlashCommand, String),
    CancelExecution,
    AgentEvent(TuiEvent),
    ScrollMessages(i32),
    SelectSession(String),
    ReportDispatchOverflow,
}
