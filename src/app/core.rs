use crate::app::ui::geometry::Rect;

use crate::app::components::commands::SlashCommand;
use crate::app::events::{InputEvent, TuiEvent};
use crate::core::{Message, MessageAttachment};

pub trait Component {
    fn update(&mut self, _action: &AppAction) -> Option<AppAction> {
        None
    }

    fn handle_event(&mut self, _event: &InputEvent) -> Option<AppAction> {
        None
    }

    fn render(
        &self,
        _f: &mut crate::app::ui::geometry::Rect,
        _area: Rect,
        _state: &crate::app::state::SessionContext,
    ) {
    }
}

#[derive(Debug)]
pub enum AppAction {
    Quit,
    Redraw,
    Input(InputEvent),
    PeriodicTick,
    SubmitInput(String, Vec<MessageAttachment>),
    QueueUserMessage {
        message: Message,
        message_index: usize,
    },
    CancelAgentTask,
    SetAgentTask {
        handle: tokio::task::JoinHandle<()>,
        cancel_tx: tokio::sync::watch::Sender<bool>,
    },
    RemoveMessageAt(usize),
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
    SetSessionIdentity {
        session_id: String,
        session_name: String,
    },
    SetSelectedModel(String),
    ShowSessionPicker(Vec<crate::session::SessionMetadata>),
    SystemMessageAppended(String),
    SelectSession(String),
    OpenSubagentSession {
        task_id: String,
        session_id: String,
        name: String,
    },
    RefreshActiveSubagentSession,
    SubagentSessionLoaded {
        task_id: String,
        session_id: String,
        name: String,
        messages: Vec<crate::app::chat_state::ChatMessage>,
    },
    ActiveSubagentMessagesLoaded {
        messages: Vec<crate::app::chat_state::ChatMessage>,
    },
    ResumeSessionLoaded {
        session_id: String,
        session_name: String,
        messages: Vec<crate::app::chat_state::ChatMessage>,
        todo_items: Vec<crate::app::chat_state::TodoItemView>,
        subagent_items: Vec<crate::app::chat_state::SubagentItemView>,
    },
    ReportDispatchOverflow,
    ShowClipboardNotice {
        x: u16,
        y: u16,
    },
}
