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
}

impl AppState {
    pub fn new(cwd: PathBuf, legacy_chat_app: crate::app::chat_state::ChatApp) -> Self {
        Self {
            cwd,
            should_quit: false,
            needs_redraw: true,
            context: SessionContext::default(),
            last_error: None,
            legacy_chat_app,
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
            | AppAction::UserMessageAppended(_)
            | AppAction::AssistantMessageAppended(_)
            | AppAction::SystemMessageAppended(_)
            | AppAction::StartNewSession(_)
            | AppAction::SetSelectedModel(_)
            | AppAction::SetProcessing(_) => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::core::{AppAction, Component};
    use crate::app::input::InputEvent;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    struct LoopingComponent;

    impl Component for LoopingComponent {
        fn update(&mut self, action: &AppAction) -> Option<AppAction> {
            match action {
                AppAction::Redraw => Some(AppAction::Redraw),
                _ => None,
            }
        }
    }

    struct InputQuitComponent;

    impl Component for InputQuitComponent {
        fn handle_event(&mut self, event: &InputEvent) -> Option<AppAction> {
            match event {
                InputEvent::Key(key)
                    if key.code == KeyCode::Char('q')
                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    Some(AppAction::Quit)
                }
                _ => None,
            }
        }
    }

    #[test]
    fn dispatch_overflow_sets_error_and_redraw() {
        let mut app = App::new(AppState::new(std::path::PathBuf::from(".")));
        app.register_component(Box::new(LoopingComponent));

        app.dispatch(AppAction::Redraw);

        assert!(app.state.last_error.is_some());
        assert!(app.state.needs_redraw);
    }

    #[test]
    fn input_event_can_emit_component_action() {
        let mut app = App::new(AppState::new(std::path::PathBuf::from(".")));
        app.register_component(Box::new(InputQuitComponent));

        app.handle_input_event(&InputEvent::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
        )));

        assert!(app.state.should_quit);
    }
}
