use ratatui::layout::Rect;

use crate::app::components::commands::SlashCommand;
use crate::app::events::TuiEvent;
use crate::app::input::InputEvent;
use crate::core::MessageAttachment;

pub trait Component {
    fn update(&mut self, _action: &AppAction) -> Option<AppAction> {
        None
    }

    fn handle_event(&mut self, _event: &InputEvent) -> Option<AppAction> {
        None
    }

    fn render(
        &self,
        _f: &mut ratatui::Frame<'_>,
        _area: Rect,
        _state: &crate::app::state::AppState,
    ) {
    }
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
    ScrollSidebar(i32),
    ToggleSidebarSection(String),
    UpdateInput(String, usize),
    ClearInput,
    UserMessageAppended(crate::app::chat_state::ChatMessage),
    AssistantMessageAppended(String),
    SetProcessing(bool),
    StartNewSession(String),
    SetSelectedModel(String),
    SystemMessageAppended(String),
    SelectSession(String),
    ReportDispatchOverflow,
    ShowClipboardNotice { x: u16, y: u16 },
}
