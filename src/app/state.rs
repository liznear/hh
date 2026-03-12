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
        let is_processing = legacy_chat_app.is_processing;

        Self {
            cwd,
            should_quit: false,
            needs_redraw: true,
            context: SessionContext {
                active_session_id: session_id.clone(),
                model_label: current_model_ref.clone(),
                is_processing,
            },
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

    pub fn handle_input_event(&mut self, event: &crate::app::events::InputEvent) {
        let mut queue = VecDeque::new();
        if let Some(action) = self.input.handle_event(event) {
            queue.push_back(action);
        }
        if let Some(action) = self.popups.handle_event(event) {
            queue.push_back(action);
        }
        if let Some(action) = self.messages.handle_event(event) {
            queue.push_back(action);
        }
        if let Some(action) = self.sidebar.handle_event(event) {
            queue.push_back(action);
        }
        while let Some(action) = queue.pop_front() {
            self.dispatch(action);
        }
    }

    pub fn sync_runtime_from_legacy(&mut self) {
        self.state.should_quit |= self.state.legacy_chat_app.should_quit;
        self.state.session_epoch = self.state.legacy_chat_app.session_epoch();
        self.state.run_epoch = self.state.legacy_chat_app.run_epoch();
        self.state.context.is_processing = self.state.legacy_chat_app.is_processing;
        self.state.session_id = self.state.legacy_chat_app.session_id.clone();
        self.state.session_name = self.state.legacy_chat_app.session_name.clone();
        self.state.current_model_ref = self.state.legacy_chat_app.current_model_ref.clone();
        self.state.context.active_session_id = self.state.session_id.clone();
        self.state.context.model_label = self.state.current_model_ref.clone();
    }

    pub(crate) fn sync_migrated_runtime_from_legacy(&mut self) {
        self.state.messages = self.state.legacy_chat_app.messages.clone();
        self.state.available_sessions = self.state.legacy_chat_app.available_sessions.clone();
        self.state.available_models = self.state.legacy_chat_app.available_models.clone();
        self.state.is_picking_session = self.state.legacy_chat_app.is_picking_session;
        self.sync_runtime_from_legacy();
    }

    pub fn process_key_event<F>(
        &mut self,
        key_event: crossterm::event::KeyEvent,
        settings: &crate::config::Settings,
        cwd: &std::path::Path,
        event_sender: &crate::app::events::TuiEventSender,
        terminal_size: F,
    ) -> anyhow::Result<()>
    where
        F: FnMut() -> anyhow::Result<(u16, u16)>,
    {
        let mut actions = Vec::new();
        crate::app::input::handle_key_event(
            key_event,
            &mut self.state.legacy_chat_app,
            &self.messages,
            &mut actions,
            settings,
            cwd,
            event_sender,
            terminal_size,
        )?;
        self.sync_runtime_from_legacy();
        self.dispatch(AppAction::UpdateInput(
            self.state.legacy_chat_app.input.clone(),
            self.state.legacy_chat_app.cursor,
        ));
        for action in actions {
            self.dispatch(action);
        }
        Ok(())
    }

    pub fn process_paste(&mut self, text: String) {
        crate::app::input::apply_paste(&mut self.state.legacy_chat_app, text);
        self.sync_runtime_from_legacy();
        self.dispatch(AppAction::UpdateInput(
            self.state.legacy_chat_app.input.clone(),
            self.state.legacy_chat_app.cursor,
        ));
    }

    pub fn process_area_scroll(
        &mut self,
        terminal_rect: ratatui::layout::Rect,
        x: u16,
        y: u16,
        up_steps: usize,
        down_steps: usize,
    ) {
        let mut actions = Vec::new();
        crate::app::input::handle_area_scroll(
            &mut self.state.legacy_chat_app,
            &self.messages,
            &self.sidebar,
            &mut actions,
            terminal_rect,
            x,
            y,
            up_steps,
            down_steps,
        );
        for action in actions {
            self.dispatch(action);
        }
    }

    pub fn process_mouse_click(
        &mut self,
        x: u16,
        y: u16,
        terminal: &crate::app::terminal::Tui,
        settings: &crate::config::Settings,
        cwd: &std::path::Path,
    ) {
        let mut actions = Vec::new();
        crate::app::input::handle_mouse_click(
            &mut self.state.legacy_chat_app,
            &self.messages,
            &self.sidebar,
            &mut actions,
            x,
            y,
            terminal,
            settings,
            cwd,
        );
        self.sync_migrated_runtime_from_legacy();
        for action in actions {
            self.dispatch(action);
        }
    }

    pub fn process_mouse_drag(
        &mut self,
        x: u16,
        y: u16,
        terminal: &crate::app::terminal::Tui,
    ) {
        crate::app::input::handle_mouse_drag(
            &mut self.state.legacy_chat_app,
            &self.messages,
            x,
            y,
            terminal,
        );
        self.sync_runtime_from_legacy();
    }

    pub fn process_mouse_release(
        &mut self,
        x: u16,
        y: u16,
        terminal: &crate::app::terminal::Tui,
    ) {
        if let Some(action) = crate::app::input::handle_mouse_release(
            &mut self.state.legacy_chat_app,
            &self.messages,
            x,
            y,
            terminal,
        ) {
            self.dispatch(action);
        }
        self.sync_runtime_from_legacy();
    }

    pub fn process_periodic_tick(
        &mut self,
        settings: &crate::config::Settings,
        cwd: &std::path::Path,
    ) {
        self.dispatch(AppAction::PeriodicTick);

        if let Some(subagent_view) = self.state.legacy_chat_app.active_subagent_session()
            && let Ok(messages) = crate::app::input::load_session_messages(
                settings,
                cwd,
                &subagent_view.session_id,
            )
        {
            self.state
                .legacy_chat_app
                .replace_active_subagent_messages(messages);
            self.sync_migrated_runtime_from_legacy();
            self.dispatch(AppAction::Redraw);
        }

        if self.state.legacy_chat_app.on_periodic_tick() {
            self.sync_runtime_from_legacy();
            self.dispatch(AppAction::Redraw);
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

            if let Some(next) = self.input.update(&action) {
                queue.push_back(next);
            }
            if let Some(next) = self.popups.update(&action) {
                queue.push_back(next);
            }
            if let Some(next) = self.messages.update(&action) {
                queue.push_back(next);
            }
            if let Some(next) = self.sidebar.update(&action) {
                queue.push_back(next);
            }
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
            | AppAction::ScrollMessages(..)
            | AppAction::ScrollSidebar(..)
            | AppAction::ToggleSidebarSection(..)
            | AppAction::ShowClipboardNotice { .. }
            | AppAction::UpdateInput(..)
            | AppAction::ClearInput => {}
            AppAction::SetProcessing(processing) => {
                self.state.legacy_chat_app.set_processing(*processing);
                self.sync_runtime_from_legacy();
                self.state.needs_redraw = true;
            }
            AppAction::AgentEvent(event) => {
                self.state.legacy_chat_app.handle_event(event);
                self.sync_migrated_runtime_from_legacy();
                self.state.needs_redraw = true;
            }
            AppAction::UserMessageAppended(msg) => {
                self.state.messages.push(msg.clone());
                self.state.legacy_chat_app.messages.push(msg.clone());
                self.state.needs_redraw = true;
            }
            AppAction::AssistantMessageAppended(text) => {
                self.state
                    .messages
                    .push(crate::app::chat_state::ChatMessage::Assistant(text.clone()));
                self.state
                    .legacy_chat_app
                    .messages
                    .push(crate::app::chat_state::ChatMessage::Assistant(text.clone()));
                self.state.needs_redraw = true;
            }
            AppAction::SystemMessageAppended(text) => {
                self.state
                    .messages
                    .push(crate::app::chat_state::ChatMessage::Assistant(text.clone()));
                self.state
                    .legacy_chat_app
                    .messages
                    .push(crate::app::chat_state::ChatMessage::Assistant(text.clone()));
                self.state.needs_redraw = true;
            }
            AppAction::StartNewSession(session_name) => {
                self.state.session_id = None;
                self.state.session_name = session_name.clone();
                self.state.messages.clear();
                self.state.session_epoch += 1;
                self.state.run_epoch += 1;
                self.state
                    .legacy_chat_app
                    .start_new_session(session_name.clone());
                self.sync_migrated_runtime_from_legacy();
                self.state.needs_redraw = true;
            }
            AppAction::SetSelectedModel(model_ref) => {
                self.state.current_model_ref = model_ref.clone();
                self.state.legacy_chat_app.set_selected_model(model_ref);
                self.sync_runtime_from_legacy();
                self.state.needs_redraw = true;
            }
        }
    }

    pub fn render_root(&self, f: &mut ratatui::Frame<'_>) {
        crate::app::render::render_app(f, &self.state.legacy_chat_app, self);
        self.render_components(f, f.area());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::app::chat_state::{ChatApp, ChatMessage};
    use crate::app::events::TuiEvent;

    fn build_app() -> App {
        App::new(AppState::new(
            Path::new(".").to_path_buf(),
            ChatApp::default(),
        ))
    }

    #[test]
    fn dispatches_agent_event_into_legacy_chat_state() {
        let mut app = build_app();
        app.state.legacy_chat_app.set_processing(true);

        app.dispatch(AppAction::AgentEvent(TuiEvent::AssistantDelta(
            "hello".to_string(),
        )));
        app.dispatch(AppAction::AgentEvent(TuiEvent::AssistantDone));

        assert!(matches!(
            app.state.legacy_chat_app.messages.last(),
            Some(ChatMessage::Assistant(text)) if text == "hello"
        ));
        assert!(!app.state.legacy_chat_app.is_processing);
    }

    #[test]
    fn set_processing_action_updates_legacy_processing_state() {
        let mut app = build_app();

        app.dispatch(AppAction::SetProcessing(true));
        assert!(app.state.legacy_chat_app.is_processing);
        assert!(app.state.context.is_processing);

        app.dispatch(AppAction::SetProcessing(false));
        assert!(!app.state.legacy_chat_app.is_processing);
        assert!(!app.state.context.is_processing);
    }

    #[test]
    fn assistant_message_appended_updates_legacy_transcript() {
        let mut app = build_app();

        app.dispatch(AppAction::AssistantMessageAppended("ready".to_string()));

        assert!(matches!(
            app.state.legacy_chat_app.messages.last(),
            Some(ChatMessage::Assistant(text)) if text == "ready"
        ));
    }
}
