use std::collections::VecDeque;
use std::path::PathBuf;

use crate::app::core::{AppAction, Component};

pub const MAX_ACTIONS_PER_TICK: usize = 256;

#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub active_session_id: Option<String>,
    pub model_label: String,
    pub is_processing: bool,
}

pub struct AppState {
    pub cwd: PathBuf,
    pub should_quit: bool,
    pub needs_redraw: bool,
    pub context: SessionContext,
    pub last_error: Option<String>,
    pub legacy_chat_app: crate::app::chat_state::ChatApp,

    // Migrated primitives
    pub messages: Vec<crate::app::chat_state::ChatMessage>,
    pub is_picking_session: bool,
    pub available_sessions: Vec<crate::session::SessionMetadata>,
    pub session_id: Option<String>,
    pub session_name: String,
    pub session_epoch: u64,
    pub run_epoch: u64,
    pub current_model_ref: String,
    pub available_models: Vec<crate::app::chat_state::ModelOptionView>,
}

impl AppState {
    pub fn new(cwd: PathBuf, mut legacy_chat_app: crate::app::chat_state::ChatApp) -> Self {
        let messages = std::mem::take(&mut legacy_chat_app.messages);
        let is_picking_session = legacy_chat_app.is_picking_session;
        let available_sessions = std::mem::take(&mut legacy_chat_app.available_sessions);
        let session_id = legacy_chat_app.session_id.clone();
        let session_name = legacy_chat_app.session_name.clone();
        let session_epoch = legacy_chat_app.session_epoch();
        let run_epoch = legacy_chat_app.run_epoch();
        let current_model_ref = legacy_chat_app.current_model_ref.clone();
        let available_models = legacy_chat_app.available_models.clone();

        Self {
            cwd,
            should_quit: false,
            needs_redraw: true,
            context: SessionContext::default(),
            last_error: None,
            legacy_chat_app,
            messages,
            is_picking_session,
            available_sessions,
            session_id,
            session_name,
            session_epoch,
            run_epoch,
            current_model_ref,
            available_models,
        }
    }
}

pub struct App {
    pub state: AppState,
    pub popups: crate::app::components::popups::PopupComponent,
    pub input: crate::app::components::input::InputComponent,
    pub messages: crate::app::components::messages::MessagesComponent,
    pub sidebar: crate::app::components::sidebar::SidebarComponent,
}

impl App {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            popups: crate::app::components::popups::PopupComponent::default(),
            input: crate::app::components::input::InputComponent::default(),
            messages: crate::app::components::messages::MessagesComponent::default(),
            sidebar: crate::app::components::sidebar::SidebarComponent::default(),
        }
    }

    pub fn handle_input_event(&mut self, event: &crate::app::input::InputEvent) {
        let mut queue = VecDeque::new();
        if let Some(action) = self.input.handle_event(event) { queue.push_back(action); }
        if let Some(action) = self.popups.handle_event(event) { queue.push_back(action); }
        if let Some(action) = self.messages.handle_event(event) { queue.push_back(action); }
        if let Some(action) = self.sidebar.handle_event(event) { queue.push_back(action); }
        while let Some(action) = queue.pop_front() {
            self.dispatch(action);
        }
    }

    pub fn dispatch(&mut self, initial: AppAction) {
        let mut queue = VecDeque::from([initial]);
        let mut processed = 0usize;

        while let Some(action) = queue.pop_front() {
            processed += 1;
            if processed > MAX_ACTIONS_PER_TICK {
                self.state.last_error =
                    Some("UI action overflow: dropped remaining actions for this tick".to_string());
                self.reduce(&AppAction::ReportDispatchOverflow);
                queue.clear();
                break;
            }

            self.reduce(&action);

            if let Some(next) = self.input.update(&action) { queue.push_back(next); }
            if let Some(next) = self.popups.update(&action) { queue.push_back(next); }
            if let Some(next) = self.messages.update(&action) { queue.push_back(next); }
            if let Some(next) = self.sidebar.update(&action) { queue.push_back(next); }
        }
    }

    fn reduce(&mut self, action: &AppAction) {
        match action {
            AppAction::Quit => {
                self.state.should_quit = true;
                self.state.needs_redraw = false;
            }
            AppAction::Redraw => {
                self.state.needs_redraw = true;
            }
            AppAction::Input(_) => {
                self.state.needs_redraw = true;
            }
            AppAction::PeriodicTick => {
                self.state.needs_redraw = true;
            }
            AppAction::SelectSession(session_id) => {
                self.state.context.active_session_id = Some(session_id.clone());
                self.state.needs_redraw = true;
            }
            AppAction::CancelExecution => {
                self.state.context.is_processing = false;
                self.state.needs_redraw = true;
            }
            AppAction::ReportDispatchOverflow => {
                self.state.needs_redraw = true;
            }
            AppAction::SubmitInput(..)
            | AppAction::RunSlashCommand(..)
            | AppAction::AgentEvent(..)
            | AppAction::ScrollMessages(..)
            | AppAction::ScrollSidebar(..)
            | AppAction::ToggleSidebarSection(..)
            | AppAction::ShowClipboardNotice { .. }
            | AppAction::UpdateInput(..)
            | AppAction::ClearInput
            | AppAction::SetProcessing(_) => {}
            AppAction::UserMessageAppended(msg) => {
                self.state.messages.push(msg.clone());
                self.state.needs_redraw = true;
            }
            AppAction::AssistantMessageAppended(text) => {
                self.state.messages.push(crate::app::chat_state::ChatMessage::Assistant(text.clone()));
                self.state.needs_redraw = true;
            }
            AppAction::SystemMessageAppended(text) => {
                self.state.messages.push(crate::app::chat_state::ChatMessage::Assistant(text.clone()));
                self.state.needs_redraw = true;
            }
            AppAction::StartNewSession(session_name) => {
                self.state.session_id = None;
                self.state.session_name = session_name.clone();
                self.state.messages.clear();
                self.state.session_epoch += 1;
                self.state.run_epoch += 1;
                self.state.legacy_chat_app.start_new_session(session_name.clone());
                self.state.needs_redraw = true;
            }
            AppAction::SetSelectedModel(model_ref) => {
                self.state.current_model_ref = model_ref.clone();
                self.state.legacy_chat_app.set_selected_model(model_ref);
                self.state.needs_redraw = true;
            }
        }
    }

    pub fn take_needs_redraw(&mut self) -> bool {
        let redraw = self.state.needs_redraw;
        self.state.needs_redraw = false;
        redraw
    }

    pub fn render_components(&self, f: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect) {
        self.popups.render(f, area, &self.state);
        self.messages.render(f, area, &self.state);
    }
}


