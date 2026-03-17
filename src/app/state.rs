use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

use crate::app::chat_state::{
    AgentOptionView, PendingQuestionView, QuestionKeyResult, QuestionOptionView, ScrollState,
    SubagentItemView, SubagentStatusView, TaskSessionTarget, TextSelection,
};
use crate::app::components::commands::{SlashCommand, get_default_commands};
use crate::app::core::{AppAction, Component};
use crate::app::events::{SubagentEventItem, TuiEvent};
use crate::core::MessageAttachment;
use crate::theme::tool_render::render_tool_result;

pub const MAX_ACTIONS_PER_TICK: usize = 256;
const DEFAULT_CONTEXT_LIMIT: usize = 128_000;
const SIDEBAR_WIDTH: u16 = 38;
const LEFT_COLUMN_RIGHT_MARGIN: u16 = 2;

struct RuntimeDispatchContext<'a> {
    settings: &'a crate::config::Settings,
    cwd: &'a std::path::Path,
    event_sender: &'a crate::app::events::TuiEventSender,
}

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

    // Migrated primitives
    pub messages: Vec<crate::app::chat_state::ChatMessage>,
    pub input: String,
    pub cursor: usize,
    pub message_scroll: ScrollState,
    pub text_selection: TextSelection,
    pub message_cache_generation: u64,
    pub is_picking_session: bool,
    pub available_sessions: Vec<crate::session::SessionMetadata>,
    pub session_id: Option<String>,
    pub session_name: String,
    pub session_epoch: u64,
    pub run_epoch: u64,
    pub current_model_ref: String,
    pub available_models: Vec<crate::app::chat_state::ModelOptionView>,
    pub current_agent_name: Option<String>,
    pub available_agents: Vec<AgentOptionView>,
    pub todo_items: Vec<crate::app::chat_state::TodoItemView>,
    pub subagent_items: Vec<crate::app::chat_state::SubagentItemView>,
    pub subagent_session_stack: Vec<crate::app::chat_state::SubagentSessionView>,
    pub pending_question: Option<crate::app::chat_state::PendingQuestionState>,
    pub esc_interrupt_pending: bool,
    pub last_run_duration: Option<String>,
    pub last_run_interrupted: bool,
    pub agent_task: Option<crate::app::chat_state::RunningAgentTask>,
    pub processing_started_at: Option<std::time::Instant>,
    pub last_timer_refresh_second: Option<u64>,
    pub last_context_tokens: Option<usize>,
    pub git_branch: Option<String>,
    pub context_budget: usize,
    pub cached_context_usage_estimate: RefCell<Option<usize>>,
    pub commands: Vec<SlashCommand>,
    pub filtered_commands: Vec<SlashCommand>,
    pub selected_command_index: usize,
    pub pending_attachments: Vec<MessageAttachment>,
    pub preferred_column: Option<usize>,
}

impl AppState {
    pub fn new(cwd: PathBuf) -> Self {
        let session_name = crate::app::utils::build_session_name(&cwd);
        let current_model_ref = String::new();
        let git_branch = detect_git_branch(&cwd);

        Self {
            cwd,
            should_quit: false,
            needs_redraw: true,
            context: SessionContext {
                active_session_id: None,
                model_label: current_model_ref.clone(),
                is_processing: false,
            },
            last_error: None,
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            message_scroll: ScrollState::new(true),
            text_selection: TextSelection::None,
            message_cache_generation: 0,
            is_picking_session: false,
            available_sessions: Vec::new(),
            session_id: None,
            session_name,
            session_epoch: 0,
            run_epoch: 0,
            current_model_ref,
            available_models: Vec::new(),
            current_agent_name: None,
            available_agents: Vec::new(),
            todo_items: Vec::new(),
            subagent_items: Vec::new(),
            subagent_session_stack: Vec::new(),
            pending_question: None,
            esc_interrupt_pending: false,
            last_run_duration: None,
            last_run_interrupted: false,
            agent_task: None,
            processing_started_at: None,
            last_timer_refresh_second: None,
            last_context_tokens: None,
            git_branch,
            context_budget: DEFAULT_CONTEXT_LIMIT,
            cached_context_usage_estimate: RefCell::new(None),
            commands: get_default_commands(),
            filtered_commands: Vec::new(),
            selected_command_index: 0,
            pending_attachments: Vec::new(),
            preferred_column: None,
        }
    }

    pub fn configure_models(
        &mut self,
        current_model_ref: String,
        available_models: Vec<crate::app::chat_state::ModelOptionView>,
    ) {
        self.current_model_ref = current_model_ref;
        self.context.model_label = self.current_model_ref.clone();
        self.available_models = available_models;
        self.context_budget = self
            .available_models
            .iter()
            .find(|model| model.full_id == self.current_model_ref)
            .map(|model| model.max_context_size)
            .unwrap_or(DEFAULT_CONTEXT_LIMIT);
        self.last_context_tokens = None;
    }

    pub fn set_agents(&mut self, agents: Vec<AgentOptionView>, selected: Option<String>) {
        self.available_agents = agents;
        self.current_agent_name = selected;
    }

    pub fn bump_session_epoch(&mut self) {
        self.session_epoch = self.session_epoch.wrapping_add(1);
    }

    pub fn bump_run_epoch(&mut self) {
        self.run_epoch = self.run_epoch.wrapping_add(1);
    }

    pub fn cancel_agent_task(&mut self) {
        if let Some(task) = self.agent_task.take()
            && !task.handle.is_finished()
        {
            self.run_epoch = self.run_epoch.wrapping_add(1);
            let _ = task.cancel_tx.send(true);
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    if !task.handle.is_finished() {
                        task.handle.abort();
                    }
                });
            } else {
                task.handle.abort();
            }
        }
        self.esc_interrupt_pending = false;
    }

    pub fn set_agent_task_with_cancel(
        &mut self,
        handle: tokio::task::JoinHandle<()>,
        cancel_tx: tokio::sync::watch::Sender<bool>,
    ) {
        self.cancel_agent_task();
        self.agent_task = Some(crate::app::chat_state::RunningAgentTask { handle, cancel_tx });
    }

    pub fn set_processing(&mut self, processing: bool) {
        if !processing && self.context.is_processing {
            if let Some(started) = self.processing_started_at {
                let elapsed_secs = started.elapsed().as_secs();
                let minutes = elapsed_secs / 60;
                let seconds = elapsed_secs % 60;
                self.last_run_duration = if minutes == 0 {
                    Some(format!("{}s", seconds))
                } else {
                    Some(format!("{}m {}s", minutes, seconds))
                };
            }
            self.last_run_interrupted = self.esc_interrupt_pending;
        }

        self.context.is_processing = processing;

        if !processing {
            self.esc_interrupt_pending = false;
            self.last_timer_refresh_second = None;
        }
        self.processing_started_at = if processing {
            self.last_timer_refresh_second = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(Some(0), |duration| Some(duration.as_secs()));
            Some(Instant::now())
        } else {
            None
        };
    }

    pub fn mark_dirty(&mut self) {
        self.message_cache_generation = self.message_cache_generation.wrapping_add(1);
        self.needs_redraw = true;
        *self.cached_context_usage_estimate.borrow_mut() = None;
    }

    pub fn message_cache_generation(&self) -> u64 {
        self.message_cache_generation
    }

    pub fn handle_agent_event(&mut self, event: &TuiEvent) {
        if !self.subagent_session_stack.is_empty() {
            // Swap in root messages to handle event in main context
            let first_view = self
                .subagent_session_stack
                .first_mut()
                .expect("subagent stack non-empty");

            std::mem::swap(&mut self.messages, &mut first_view.previous_messages);
            std::mem::swap(&mut self.message_scroll, &mut first_view.previous_scroll);

            // Temporarily use root messages
            self.handle_agent_event_inner(event);

            // Swap back
            let first_view = self
                .subagent_session_stack
                .first_mut()
                .expect("subagent stack non-empty");
            std::mem::swap(&mut self.message_scroll, &mut first_view.previous_scroll);
            std::mem::swap(&mut self.messages, &mut first_view.previous_messages);
        } else {
            self.handle_agent_event_inner(event);
        }
    }

    fn handle_agent_event_inner(&mut self, event: &TuiEvent) {
        match event {
            TuiEvent::Thinking(text) => {
                let _ = self.append_thinking_delta(text);
                self.mark_dirty();
            }
            TuiEvent::ToolStart { name, args } => {
                if !self.is_duplicate_pending_tool_call(name, args) {
                    self.messages
                        .push(crate::app::chat_state::ChatMessage::ToolCall {
                            name: name.clone(),
                            args: args.to_string(),
                            output: None,
                            is_error: None,
                        });
                }
                self.mark_dirty();
            }
            TuiEvent::ToolEnd { name, result } => {
                self.complete_tool_call(name, result);
                if name == "question" {
                    self.pending_question = None;
                }
                self.mark_dirty();
            }
            TuiEvent::AssistantDelta(delta) => {
                if let Some(crate::app::chat_state::ChatMessage::Assistant(existing)) =
                    self.messages.last_mut()
                {
                    existing.push_str(delta);
                    self.mark_dirty();
                    return;
                }
                self.messages
                    .push(crate::app::chat_state::ChatMessage::Assistant(
                        delta.clone(),
                    ));
                self.mark_dirty();
            }
            TuiEvent::RunnerStateUpdated(state) => {
                self.last_context_tokens = Some(state.context_tokens);
                self.todo_items = state
                    .todo_items
                    .iter()
                    .map(|item| crate::app::chat_state::TodoItemView {
                        content: item.content.clone(),
                        status: crate::app::chat_state::TodoStatus::from_core(item.status.clone()),
                        priority: crate::app::chat_state::TodoPriority::from_core(
                            item.priority.clone(),
                        ),
                    })
                    .collect();
                self.mark_dirty();
            }
            TuiEvent::AssistantDone => {
                self.set_processing(false);

                // Append footer if we have duration info
                if let Some(duration) = self.last_run_duration.take() {
                    let agent_display_name = self
                        .selected_agent()
                        .map(|a| a.display_name.clone())
                        .unwrap_or_else(|| "Agent".to_string());

                    let provider_name = self
                        .available_models
                        .iter()
                        .find(|model| model.full_id == self.current_model_ref)
                        .map(|model| model.provider_name.clone())
                        .unwrap_or_default();
                    let model_name = self
                        .available_models
                        .iter()
                        .find(|model| model.full_id == self.current_model_ref)
                        .map(|model| model.model_name.clone())
                        .unwrap_or_default();

                    self.messages
                        .push(crate::app::chat_state::ChatMessage::Footer {
                            agent_display_name,
                            provider_name,
                            model_name,
                            duration,
                            interrupted: self.last_run_interrupted,
                        });
                    self.mark_dirty();
                    self.last_run_interrupted = false;
                }
            }
            TuiEvent::Cancelled => {
                self.set_processing(false);

                if let Some(duration) = self.last_run_duration.take() {
                    let agent_display_name = self
                        .selected_agent()
                        .map(|a| a.display_name.clone())
                        .unwrap_or_else(|| "Agent".to_string());

                    let provider_name = self
                        .available_models
                        .iter()
                        .find(|model| model.full_id == self.current_model_ref)
                        .map(|model| model.provider_name.clone())
                        .unwrap_or_default();
                    let model_name = self
                        .available_models
                        .iter()
                        .find(|model| model.full_id == self.current_model_ref)
                        .map(|model| model.model_name.clone())
                        .unwrap_or_default();

                    self.messages
                        .push(crate::app::chat_state::ChatMessage::Footer {
                            agent_display_name,
                            provider_name,
                            model_name,
                            duration,
                            interrupted: true,
                        });
                    self.mark_dirty();
                    self.last_run_interrupted = false;
                }
            }
            TuiEvent::ApprovalRequired { call_id, request } => {
                self.messages
                    .push(crate::app::chat_state::ChatMessage::Thinking(format!(
                        "approval required ({call_id}): {}",
                        request.body
                    )));
                self.mark_dirty();
            }
            TuiEvent::QuestionRequired { call_id, prompts } => {
                self.messages
                    .push(crate::app::chat_state::ChatMessage::Thinking(format!(
                        "question required ({call_id}): {} prompt(s)",
                        prompts.len()
                    )));
                self.mark_dirty();
            }
            TuiEvent::QueuedMessagesConsumed(indexes) => {
                self.clear_queued_user_messages(indexes);
            }
            TuiEvent::SessionTitle(title) => {
                self.session_name = title.clone();
                self.mark_dirty();
            }
            TuiEvent::CompactionStart => {
                self.messages
                    .push(crate::app::chat_state::ChatMessage::CompactionPending);
                self.mark_dirty();
            }
            TuiEvent::CompactionDone(summary) => {
                let mut replaced_pending = false;
                for message in self.messages.iter_mut().rev() {
                    if matches!(
                        message,
                        crate::app::chat_state::ChatMessage::CompactionPending
                    ) {
                        *message = crate::app::chat_state::ChatMessage::Compaction(summary.clone());
                        replaced_pending = true;
                        break;
                    }
                }
                if !replaced_pending {
                    self.messages
                        .push(crate::app::chat_state::ChatMessage::Compaction(
                            summary.clone(),
                        ));
                }
                self.set_processing(false);
                self.mark_dirty();
            }
            TuiEvent::QuestionPrompt {
                questions,
                responder,
            } => {
                self.pending_question = Some(crate::app::chat_state::PendingQuestionState {
                    answers: vec![Vec::new(); questions.len()],
                    custom_values: vec![String::new(); questions.len()],
                    questions: questions.clone(),
                    question_index: 0,
                    selected_index: 0,
                    custom_mode: false,
                    responder: Some(responder.clone()),
                });
                self.mark_dirty();
            }
            TuiEvent::SubagentsChanged(items) => {
                self.subagent_items = items.iter().filter_map(to_subagent_item_view).collect();
                self.mark_dirty();
            }
            TuiEvent::Error(msg) => {
                self.messages
                    .push(crate::app::chat_state::ChatMessage::Error(msg.clone()));
                self.set_processing(false);
                self.mark_dirty();
            }
            TuiEvent::Tick => {}
            TuiEvent::Key(_) => {}
        }
    }

    fn append_thinking_delta(&mut self, delta: &str) -> bool {
        if delta.is_empty() {
            return false;
        }

        if let Some(crate::app::chat_state::ChatMessage::Thinking(existing)) =
            self.messages.last_mut()
        {
            existing.push_str(delta);
            return true;
        }

        self.messages
            .push(crate::app::chat_state::ChatMessage::Thinking(
                delta.to_string(),
            ));
        false
    }

    fn is_duplicate_pending_tool_call(&self, name: &str, args: &serde_json::Value) -> bool {
        let Some(crate::app::chat_state::ChatMessage::ToolCall {
            name: last_name,
            args: last_args,
            is_error,
            ..
        }) = self.messages.last()
        else {
            return false;
        };

        is_error.is_none() && last_name == name && last_args == &args.to_string()
    }

    fn complete_tool_call(&mut self, name: &str, result: &crate::tool::ToolResult) {
        let rendered = render_tool_result(name, result);
        if let Some(todos) = rendered.todos {
            self.todo_items = todos;
        }
        if name == "task" && !result.is_error {
            self.update_subagent_items_from_task_result(result);
        }

        let target_index = if name == "task" {
            let parsed = parse_task_tool_output(&result.payload)
                .or_else(|| serde_json::from_str::<TaskToolWireOutput>(&result.output).ok());

            if let Some(parsed) = parsed.as_ref()
                && let Some((idx, _)) = self.messages.iter().enumerate().rev().find(|(_, message)| {
                    matches!(
                        message,
                        crate::app::chat_state::ChatMessage::ToolCall {
                            name: tool_name,
                            args,
                            is_error: status,
                            ..
                        } if tool_name == name
                            && status.is_none()
                            && tool_start_call_id_from_text(args).as_deref() == Some(parsed.task_id.as_str())
                    )
                })
            {
                Some(idx)
            } else {
                self.messages
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(idx, message)| {
                        let crate::app::chat_state::ChatMessage::ToolCall {
                            name: tool_name,
                            args,
                            is_error: status,
                            ..
                        } = message
                        else {
                            return None;
                        };

                        if tool_name != name || status.is_some() {
                            return None;
                        }

                        if let Some(parsed) = parsed.as_ref() {
                            if task_call_args_match_result(args, parsed) {
                                return Some(idx);
                            }
                            return None;
                        }

                        Some(idx)
                    })
            }
        } else {
            self.messages
                .iter()
                .enumerate()
                .rev()
                .find_map(|(idx, message)| {
                    let crate::app::chat_state::ChatMessage::ToolCall {
                        name: tool_name,
                        is_error: status,
                        ..
                    } = message
                    else {
                        return None;
                    };

                    if tool_name == name && status.is_none() {
                        Some(idx)
                    } else {
                        None
                    }
                })
        };

        if let Some(idx) = target_index
            && let Some(crate::app::chat_state::ChatMessage::ToolCall {
                is_error: status,
                output: out,
                ..
            }) = self.messages.get_mut(idx)
        {
            *status = Some(result.is_error);
            *out = Some(rendered.text);
        }
    }

    fn update_subagent_items_from_task_result(&mut self, result: &crate::tool::ToolResult) {
        let parsed = parse_task_tool_output(&result.payload)
            .or_else(|| serde_json::from_str::<TaskToolWireOutput>(&result.output).ok());
        let Some(parsed) = parsed else {
            return;
        };

        let Some(status) = SubagentStatusView::from_wire(&parsed.status) else {
            return;
        };

        let item = SubagentItemView {
            task_id: parsed.task_id,
            session_id: parsed.session_id.unwrap_or_default(),
            name: parsed.name,
            parent_task_id: parsed.parent_task_id,
            agent_name: parsed.agent_name,
            prompt: parsed.prompt,
            summary: parsed.summary.or(parsed.error),
            depth: parsed.depth,
            started_at: parsed.started_at,
            finished_at: parsed.finished_at,
            status,
        };

        if let Some(existing) = self
            .subagent_items
            .iter_mut()
            .find(|existing| existing.task_id == item.task_id)
        {
            *existing = item;
        } else {
            self.subagent_items.push(item);
        }
    }

    pub fn clear_queued_user_messages(&mut self, indexes: &[usize]) {
        let mut changed = false;
        for index in indexes {
            if let Some(crate::app::chat_state::ChatMessage::User {
                queued: is_queued, ..
            }) = self.messages.get_mut(*index)
                && *is_queued
            {
                *is_queued = false;
                changed = true;
            }
        }
        if changed {
            self.mark_dirty();
        }
    }

    pub fn on_periodic_tick(&mut self) -> bool {
        let mut needs_redraw = self.context.is_processing;

        if self.context.is_processing {
            let now_second = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs());
            if self.last_timer_refresh_second != Some(now_second) {
                self.last_timer_refresh_second = Some(now_second);
                self.mark_dirty();
                needs_redraw = true;
            }
        }

        needs_redraw
    }

    pub fn submit_input(&mut self) -> crate::app::chat_state::SubmittedInput {
        let input = std::mem::take(&mut self.input);
        let attachments = std::mem::take(&mut self.pending_attachments);
        self.cursor = 0;
        self.preferred_column = None;

        let queued = self.context.is_processing;
        let mut submitted = crate::app::chat_state::SubmittedInput {
            text: input,
            attachments,
            message_index: None,
            queued,
        };

        if submitted.text.is_empty() && submitted.attachments.is_empty() {
            return submitted;
        }

        self.messages
            .push(crate::app::chat_state::ChatMessage::User {
                text: submitted.text.clone(),
                queued,
            });
        let message_index = self.messages.len().saturating_sub(1);
        submitted.message_index = Some(message_index);

        if !queued {
            self.set_processing(true);
        }

        self.message_scroll.auto_follow = true;
        self.mark_dirty();
        submitted
    }

    pub fn remove_message_at(&mut self, index: usize) {
        if index >= self.messages.len() {
            return;
        }
        self.messages.remove(index);
        self.mark_dirty();
    }

    pub fn is_viewing_subagent_session(&self) -> bool {
        !self.subagent_session_stack.is_empty()
    }

    pub fn active_subagent_session(&self) -> Option<&crate::app::chat_state::SubagentSessionView> {
        self.subagent_session_stack.last()
    }

    pub fn subagent_session_titles(&self) -> impl Iterator<Item = &str> {
        self.subagent_session_stack
            .iter()
            .map(|view| view.title.as_str())
    }

    pub fn subagent_session_depth(&self) -> usize {
        self.subagent_session_stack.len()
    }

    pub fn open_subagent_session(
        &mut self,
        task_id: String,
        session_id: String,
        title: String,
        messages: Vec<crate::app::chat_state::ChatMessage>,
    ) {
        let previous_messages = std::mem::replace(&mut self.messages, messages);
        let previous_scroll = self.message_scroll;
        self.message_scroll = crate::app::chat_state::ScrollState::new(true);
        self.subagent_session_stack
            .push(crate::app::chat_state::SubagentSessionView {
                task_id,
                session_id,
                title,
                previous_messages,
                previous_scroll,
            });

        self.mark_dirty();
    }

    pub fn close_subagent_session(&mut self) {
        let Some(view) = self.subagent_session_stack.pop() else {
            return;
        };
        self.messages = view.previous_messages;
        self.message_scroll = view.previous_scroll;

        self.mark_dirty();
    }

    pub fn replace_active_subagent_messages(
        &mut self,
        messages: Vec<crate::app::chat_state::ChatMessage>,
    ) {
        if self.subagent_session_stack.is_empty() {
            return;
        }
        self.messages = messages;
        self.mark_dirty();
    }

    pub fn has_pending_question(&self) -> bool {
        self.pending_question.is_some()
    }

    pub fn pending_question_view(&self) -> Option<PendingQuestionView> {
        let state = self.pending_question.as_ref()?;
        let question = state.questions.get(state.question_index)?;
        let mut options = Vec::new();
        let selected = state.answers[state.question_index].clone();

        for (idx, option) in question.options.iter().enumerate() {
            options.push(QuestionOptionView {
                label: option.label.clone(),
                description: option.description.clone(),
                selected: selected.contains(&option.label),
                active: idx == state.selected_index,
                custom: false,
                submit: false,
            });
        }

        if question.custom {
            options.push(QuestionOptionView {
                label: "Type your own answer".to_string(),
                description: String::new(),
                selected: !state.custom_values[state.question_index].trim().is_empty()
                    && selected.contains(&state.custom_values[state.question_index]),
                active: options.len() == state.selected_index,
                custom: true,
                submit: false,
            });
        }

        if question.multiple {
            options.push(QuestionOptionView {
                label: "Submit answers".to_string(),
                description: "Continue to the next question".to_string(),
                selected: false,
                active: options.len() == state.selected_index,
                custom: false,
                submit: true,
            });
        }

        Some(PendingQuestionView {
            header: question.header.clone(),
            question: question.question.clone(),
            options,
            selected_index: state.selected_index,
            custom_mode: state.custom_mode,
            custom_value: state.custom_values[state.question_index].clone(),
            question_index: state.question_index,
            total_questions: state.questions.len(),
            multiple: question.multiple,
        })
    }

    pub fn should_interrupt_on_esc(&self) -> bool {
        self.esc_interrupt_pending
    }

    pub fn arm_esc_interrupt(&mut self) {
        self.esc_interrupt_pending = true;
    }

    pub fn clear_pending_esc_interrupt(&mut self) {
        self.esc_interrupt_pending = false;
    }

    pub fn processing_interrupt_hint(&self) -> &'static str {
        if self.esc_interrupt_pending {
            "esc again to interrupt"
        } else {
            "esc interrupt"
        }
    }

    pub fn progress_panel_height(&self) -> u16 {
        0
    }

    pub fn message_viewport_height(&self, total_height: u16) -> usize {
        total_height.saturating_sub(self.progress_panel_height() + 3 + 1 + 1 + 1 + 2) as usize
    }

    pub fn message_wrap_width(&self, total_width: u16) -> usize {
        let main_width = if total_width > SIDEBAR_WIDTH {
            total_width.saturating_sub(SIDEBAR_WIDTH + LEFT_COLUMN_RIGHT_MARGIN)
        } else {
            total_width
        };
        main_width.saturating_sub(2) as usize
    }

    pub fn get_selected_text(&self, lines: &[crate::ui_compat::text::Line<'static>]) -> String {
        if !self.text_selection.is_active() {
            return String::new();
        }

        let (start, end) = match self.text_selection.get_range() {
            Some(range) => range,
            None => return String::new(),
        };

        if start.line >= lines.len() || end.line >= lines.len() {
            return String::new();
        }

        let mut selected_text = String::new();
        let start_idx = start.line;
        let end_idx = end.line;

        for (offset, line) in lines[start_idx..=end_idx].iter().enumerate() {
            let line_idx = start_idx + offset;
            let line_text = line
                .spans
                .iter()
                .map(|s| s.content.as_ref() as &str)
                .collect::<String>();

            let (start_col, end_col) = if line_idx == start_idx && line_idx == end_idx {
                (start.column, end.column)
            } else if line_idx == start_idx {
                (start.column, line_text.chars().count())
            } else if line_idx == end_idx {
                (0, end.column)
            } else {
                (0, line_text.chars().count())
            };

            let chars: Vec<char> = line_text.chars().collect();
            let clamped_start = start_col.min(chars.len());
            let clamped_end = end_col.min(chars.len());
            if clamped_start >= clamped_end {
                continue;
            }
            let selected_line = chars[clamped_start..clamped_end].iter().collect::<String>();

            selected_text.push_str(&selected_line);
            if line_idx < end_idx {
                selected_text.push('\n');
            }
        }

        selected_text
    }

    pub fn start_selection(&mut self, line: usize, column: usize) {
        self.text_selection = TextSelection::InProgress {
            start: crate::app::chat_state::SelectionPosition::new(line, column),
        };
    }

    pub fn update_selection(&mut self, line: usize, column: usize) {
        match &self.text_selection {
            TextSelection::InProgress { start } => {
                self.text_selection = TextSelection::Active {
                    start: *start,
                    end: crate::app::chat_state::SelectionPosition::new(line, column),
                };
            }
            TextSelection::Active { start, .. } => {
                self.text_selection = TextSelection::Active {
                    start: *start,
                    end: crate::app::chat_state::SelectionPosition::new(line, column),
                };
            }
            TextSelection::None => {
                self.start_selection(line, column);
            }
        }
    }

    pub fn end_selection(&mut self) {
        if let TextSelection::InProgress { .. } = self.text_selection {
            self.text_selection = TextSelection::None;
        }
    }

    pub fn clear_selection(&mut self) {
        self.text_selection = TextSelection::None;
    }

    pub fn selected_agent(&self) -> Option<&AgentOptionView> {
        self.current_agent_name
            .as_ref()
            .and_then(|name| self.available_agents.iter().find(|a| a.name == *name))
    }

    pub fn cycle_agent(&mut self) {
        if self.available_agents.is_empty() {
            return;
        }

        let primary_agents: Vec<_> = self
            .available_agents
            .iter()
            .filter(|a| a.mode == "primary")
            .collect();

        if primary_agents.is_empty() {
            return;
        }

        let current = self.current_agent_name.as_deref();

        if let Some(current_name) = current
            && let Some(pos) = primary_agents.iter().position(|a| a.name == current_name)
        {
            let next_pos = (pos + 1) % primary_agents.len();
            self.current_agent_name = Some(primary_agents[next_pos].name.clone());
            return;
        }

        self.current_agent_name = Some(primary_agents[0].name.clone());
    }

    pub fn task_session_target_at_visual_line(
        &self,
        wrap_width: usize,
        visual_line: usize,
    ) -> Option<TaskSessionTarget> {
        if self.messages.is_empty() || wrap_width == 0 {
            return None;
        }

        let (_, starts) = crate::app::render::build_message_lines_with_starts(self, wrap_width);
        if starts.is_empty() {
            return None;
        }

        let msg_idx = starts.partition_point(|start| *start <= visual_line);
        let msg_idx = msg_idx.saturating_sub(1);
        let message = self.messages.get(msg_idx)?;
        let crate::app::chat_state::ChatMessage::ToolCall {
            name,
            args,
            output,
            is_error,
            ..
        } = message
        else {
            return None;
        };

        if name != "task" {
            return None;
        }

        if let Some(output) = output.as_deref()
            && let Ok(parsed) = serde_json::from_str::<TaskToolWireOutput>(output)
            && let Some(session_id) = parsed.session_id
            && *is_error != Some(true)
        {
            return Some(TaskSessionTarget {
                task_id: parsed.task_id,
                session_id,
                name: parsed.name,
            });
        }

        if *is_error == Some(true) {
            return None;
        }

        if let Some(call_id) = tool_start_call_id_from_text(args)
            && let Some(item) = self
                .subagent_items
                .iter()
                .find(|item| item.task_id == call_id && !item.session_id.is_empty())
        {
            return Some(TaskSessionTarget {
                task_id: item.task_id.clone(),
                session_id: item.session_id.clone(),
                name: item.name.clone(),
            });
        }

        if let Some(call_id) = tool_start_call_id_from_text(args)
            && let Some(parent_session_id) = self.current_visible_session_id()
        {
            let task_name = serde_json::from_str::<TaskToolArgsWire>(args)
                .ok()
                .map(|parsed| parsed.name)
                .unwrap_or_else(|| "subagent task".to_string());
            return Some(TaskSessionTarget {
                task_id: call_id.clone(),
                session_id: format!("{parent_session_id}-{call_id}"),
                name: task_name,
            });
        }

        let args = serde_json::from_str::<TaskToolArgsWire>(args).ok()?;
        if let Some(item) = self.subagent_items.iter().rev().find(|item| {
            item.name == args.name
                && item.prompt == args.prompt
                && item.agent_name == args.subagent_type
                && !item.session_id.is_empty()
        }) {
            return Some(TaskSessionTarget {
                task_id: item.task_id.clone(),
                session_id: item.session_id.clone(),
                name: item.name.clone(),
            });
        }

        None
    }

    fn current_visible_session_id(&self) -> Option<&str> {
        self.active_subagent_session()
            .map(|view| view.session_id.as_str())
            .or(self.session_id.as_deref())
    }

    pub fn handle_question_key(&mut self, key_event: KeyEvent) -> QuestionKeyResult {
        let Some(state) = self.pending_question.as_mut() else {
            return QuestionKeyResult::NotHandled;
        };

        let Some(question) = state.questions.get(state.question_index).cloned() else {
            self.pending_question = None;
            return QuestionKeyResult::Dismissed;
        };

        if state.custom_mode {
            match key_event.code {
                KeyCode::Char(c) if !key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    state.custom_values[state.question_index].push(c);
                    self.mark_dirty();
                    return QuestionKeyResult::Handled;
                }
                KeyCode::Backspace => {
                    state.custom_values[state.question_index].pop();
                    self.mark_dirty();
                    return QuestionKeyResult::Handled;
                }
                KeyCode::Esc => {
                    let existing_custom = state.custom_values[state.question_index].clone();
                    if !existing_custom.is_empty() {
                        let normalized = normalize_custom_input(&existing_custom);
                        state.answers[state.question_index].retain(|item| item != &normalized);
                        state.custom_values[state.question_index].clear();
                    }
                    state.custom_mode = false;
                    self.mark_dirty();
                    return QuestionKeyResult::Handled;
                }
                KeyCode::Enter => {
                    if key_event.modifiers.contains(KeyModifiers::SHIFT) {
                        state.custom_values[state.question_index].push('\n');
                        self.mark_dirty();
                        return QuestionKeyResult::Handled;
                    }

                    let custom = normalize_custom_input(&state.custom_values[state.question_index]);
                    state.custom_mode = false;
                    if custom.trim().is_empty() {
                        self.mark_dirty();
                        return QuestionKeyResult::Handled;
                    }
                    if question.multiple {
                        if !state.answers[state.question_index].contains(&custom) {
                            state.answers[state.question_index].push(custom);
                        }
                        self.mark_dirty();
                        return QuestionKeyResult::Handled;
                    }

                    state.answers[state.question_index] = vec![custom];
                    return self.advance_or_submit_question();
                }
                _ => return QuestionKeyResult::Handled,
            }
        }

        let option_count =
            question.options.len() + usize::from(question.custom) + usize::from(question.multiple);

        match key_event.code {
            KeyCode::Char(_) if !key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                QuestionKeyResult::Handled
            }
            KeyCode::Up => {
                state.selected_index = if state.selected_index == 0 {
                    option_count.saturating_sub(1)
                } else {
                    state.selected_index.saturating_sub(1)
                };
                self.mark_dirty();
                QuestionKeyResult::Handled
            }
            KeyCode::Down => {
                state.selected_index = (state.selected_index + 1) % option_count.max(1);
                self.mark_dirty();
                QuestionKeyResult::Handled
            }
            KeyCode::Esc => {
                let existing_custom = state.custom_values[state.question_index].clone();
                if !existing_custom.is_empty() {
                    let normalized = normalize_custom_input(&existing_custom);
                    state.answers[state.question_index].retain(|item| item != &normalized);
                    state.custom_values[state.question_index].clear();
                    state.custom_mode = false;
                    self.mark_dirty();
                    return QuestionKeyResult::Handled;
                }

                self.finish_question_with_error(anyhow::anyhow!("question dismissed by user"));
                QuestionKeyResult::Dismissed
            }
            KeyCode::Char(digit) if digit.is_ascii_digit() => {
                let index = digit.to_digit(10).unwrap_or(0) as usize;
                if index == 0 {
                    return QuestionKeyResult::Handled;
                }
                let choice = index - 1;
                if choice < option_count {
                    state.selected_index = choice;
                    return self.apply_question_selection(question);
                }
                QuestionKeyResult::Handled
            }
            KeyCode::Enter => self.apply_question_selection(question),
            _ => QuestionKeyResult::Handled,
        }
    }

    fn apply_question_selection(
        &mut self,
        question: crate::core::QuestionPrompt,
    ) -> QuestionKeyResult {
        let Some(state) = self.pending_question.as_mut() else {
            return QuestionKeyResult::Dismissed;
        };

        let choice = state.selected_index;
        let custom_index = if question.custom {
            Some(question.options.len())
        } else {
            None
        };
        let submit_index = if question.multiple {
            question.options.len() + usize::from(question.custom)
        } else {
            usize::MAX
        };

        if choice < question.options.len() {
            let label = question.options[choice].label.clone();
            if question.multiple {
                if state.answers[state.question_index].contains(&label) {
                    state.answers[state.question_index].retain(|item| item != &label);
                } else {
                    state.answers[state.question_index].push(label);
                }
                self.mark_dirty();
                return QuestionKeyResult::Handled;
            }

            state.answers[state.question_index] = vec![label];
            return self.advance_or_submit_question();
        }

        if custom_index.is_some() && custom_index == Some(choice) {
            state.custom_mode = true;
            self.mark_dirty();
            return QuestionKeyResult::Handled;
        }

        if choice == submit_index {
            return self.advance_or_submit_question();
        }

        QuestionKeyResult::Handled
    }

    fn advance_or_submit_question(&mut self) -> QuestionKeyResult {
        let Some(state) = self.pending_question.as_mut() else {
            return QuestionKeyResult::Dismissed;
        };

        if state.question_index + 1 < state.questions.len() {
            state.question_index += 1;
            state.selected_index = 0;
            state.custom_mode = false;
            self.mark_dirty();
            return QuestionKeyResult::Handled;
        }

        let answers = state.answers.clone();
        self.finish_question_with_answers(answers);
        QuestionKeyResult::Submitted
    }

    fn finish_question_with_answers(&mut self, answers: crate::core::QuestionAnswers) {
        if let Some(mut pending) = self.pending_question.take()
            && let Some(guarded) = pending.responder.take()
            && let Ok(mut lock) = guarded.lock()
            && let Some(sender) = lock.take()
        {
            let _ = sender.send(Ok(answers));
        }
        self.mark_dirty();
    }

    fn finish_question_with_error(&mut self, error: anyhow::Error) {
        if let Some(mut pending) = self.pending_question.take()
            && let Some(guarded) = pending.responder.take()
            && let Ok(mut lock) = guarded.lock()
            && let Some(sender) = lock.take()
        {
            let _ = sender.send(Err(error));
        }
        self.mark_dirty();
    }

    pub fn context_usage(&self) -> (usize, usize) {
        if let Some(tokens) = self.last_context_tokens {
            return (tokens, self.context_budget);
        }

        if let Some(estimated_tokens) = *self.cached_context_usage_estimate.borrow() {
            return (estimated_tokens, self.context_budget);
        }

        let boundary = self
            .messages
            .iter()
            .rposition(|message| {
                matches!(message, crate::app::chat_state::ChatMessage::Compaction(_))
            })
            .unwrap_or(0);
        let mut chars = self.input.len();

        for message in self.messages.iter().skip(boundary) {
            chars += match message {
                crate::app::chat_state::ChatMessage::User { text, .. }
                | crate::app::chat_state::ChatMessage::Assistant(text)
                | crate::app::chat_state::ChatMessage::Compaction(text)
                | crate::app::chat_state::ChatMessage::Thinking(text) => text.len(),
                crate::app::chat_state::ChatMessage::CompactionPending => 0,
                crate::app::chat_state::ChatMessage::ToolCall {
                    name, args, output, ..
                } => name.len() + args.len() + output.as_ref().map(|s| s.len()).unwrap_or(0),
                crate::app::chat_state::ChatMessage::Error(text) => text.len(),
                crate::app::chat_state::ChatMessage::Footer { .. } => 0,
            };
        }
        let estimated_tokens = chars / 4;
        *self.cached_context_usage_estimate.borrow_mut() = Some(estimated_tokens);
        (estimated_tokens, self.context_budget)
    }

    pub fn processing_step(&self, interval_ms: u128) -> usize {
        if !self.context.is_processing {
            return 0;
        }

        let elapsed_ms = self
            .processing_started_at
            .map(|started| started.elapsed().as_millis())
            .unwrap_or_default();
        let interval = interval_ms.max(1);
        (elapsed_ms / interval) as usize
    }

    pub fn processing_duration(&self) -> String {
        if !self.context.is_processing {
            return String::new();
        }

        let elapsed_secs = self
            .processing_started_at
            .map(|started| started.elapsed().as_secs())
            .unwrap_or_default();

        let minutes = elapsed_secs / 60;
        let seconds = elapsed_secs % 60;

        if minutes == 0 {
            format!("{}s", seconds)
        } else {
            format!("{}m {}s", minutes, seconds)
        }
    }

    pub fn update_command_filtering(&mut self) {
        if self.input.starts_with('/') {
            let query = self.input.trim();
            self.filtered_commands = self
                .commands
                .iter()
                .filter(|cmd| cmd.name.starts_with(query))
                .cloned()
                .collect();
        } else {
            self.filtered_commands.clear();
        }

        if self.selected_command_index >= self.filtered_commands.len() {
            self.selected_command_index = 0;
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.preferred_column = None;
    }

    pub fn insert_str(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.input.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.preferred_column = None;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((idx, _)) = self.input[..self.cursor].char_indices().next_back() {
            self.input.drain(idx..self.cursor);
            self.cursor = idx;
            self.preferred_column = None;
        }
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.pending_attachments.clear();
        self.cursor = 0;
        self.preferred_column = None;
    }

    pub fn set_input(&mut self, value: String) {
        self.input = value;
        self.pending_attachments.clear();
        self.cursor = self.input.len();
        self.preferred_column = None;
    }

    pub fn add_pending_attachment(&mut self, attachment: MessageAttachment) {
        self.pending_attachments.push(attachment);
    }

    pub fn move_to_line_start(&mut self) {
        let (start, _) = current_line_bounds(&self.input, self.cursor);
        if self.cursor == start {
            let (line_index, _) = cursor_line_col(&self.input, self.cursor);
            if line_index > 0
                && let Some((prev_start, _)) = line_bounds_by_index(&self.input, line_index - 1)
            {
                self.cursor = prev_start;
            }
        } else {
            self.cursor = start;
        }
        self.preferred_column = None;
    }

    pub fn move_to_line_end(&mut self) {
        let (_, end) = current_line_bounds(&self.input, self.cursor);
        if self.cursor == end {
            let (line_index, _) = cursor_line_col(&self.input, self.cursor);
            if let Some((_, next_end)) = line_bounds_by_index(&self.input, line_index + 1) {
                self.cursor = next_end;
            }
        } else {
            self.cursor = end;
        }
        self.preferred_column = None;
    }

    pub fn move_cursor_up(&mut self) {
        self.move_cursor_vertical(-1);
    }

    pub fn move_cursor_down(&mut self) {
        self.move_cursor_vertical(1);
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        if let Some((idx, _)) = self.input[..self.cursor].char_indices().next_back() {
            self.cursor = idx;
            self.preferred_column = None;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        if let Some(ch) = self.input[self.cursor..].chars().next() {
            self.cursor += ch.len_utf8();
            self.preferred_column = None;
        }
    }

    fn move_cursor_vertical(&mut self, direction: isize) {
        if self.input.is_empty() {
            return;
        }

        let (line_index, column) = cursor_line_col(&self.input, self.cursor);
        let target_column = self.preferred_column.unwrap_or(column);
        let target_line = if direction < 0 {
            line_index.saturating_sub(1)
        } else {
            line_index + 1
        };

        if direction < 0 && line_index == 0 {
            return;
        }

        let total_lines = self.input.split('\n').count();
        if target_line >= total_lines {
            return;
        }

        self.cursor = line_col_to_cursor(&self.input, target_line, target_column);
        self.preferred_column = Some(target_column);
    }

    // Missing methods implementation
    pub fn start_new_session(&mut self, session_name: String) {
        self.bump_session_epoch();
        self.messages.clear();
        self.subagent_session_stack.clear();
        self.todo_items.clear();
        self.subagent_items.clear();
        self.last_context_tokens = None;
        self.session_id = None;
        self.session_name = session_name;
        self.context.active_session_id = None;
        self.available_sessions.clear();
        self.is_picking_session = false;
        self.message_scroll.reset(true);
        self.set_processing(false);
        self.pending_question = None;
        self.cancel_agent_task();
        self.mark_dirty();
    }

    pub fn set_selected_model(&mut self, model_ref: &str) {
        self.current_model_ref = model_ref.to_string();
        self.context.model_label = self.current_model_ref.clone();
        self.context_budget = self
            .available_models
            .iter()
            .find(|model| model.full_id == self.current_model_ref)
            .map(|model| model.max_context_size)
            .unwrap_or(DEFAULT_CONTEXT_LIMIT);
        self.last_context_tokens = None;
    }
}

// Helpers for tool result parsing
#[derive(Debug, Deserialize)]
struct TaskToolWireOutput {
    task_id: String,
    #[serde(default)]
    session_id: Option<String>,
    status: String,
    name: String,
    agent_name: String,
    prompt: String,
    depth: usize,
    #[serde(default)]
    parent_task_id: Option<String>,
    started_at: u64,
    #[serde(default)]
    finished_at: Option<u64>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TaskToolArgsWire {
    name: String,
    prompt: String,
    subagent_type: String,
}

fn parse_task_tool_output(value: &serde_json::Value) -> Option<TaskToolWireOutput> {
    serde_json::from_value(value.clone()).ok()
}

fn tool_start_call_id_from_text(args: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|value| {
            value
                .get("__call_id")
                .and_then(|entry| entry.as_str())
                .map(ToString::to_string)
        })
}

fn task_call_args_match_result(args: &str, parsed: &TaskToolWireOutput) -> bool {
    let Ok(task_args) = serde_json::from_str::<TaskToolArgsWire>(args) else {
        return false;
    };

    task_args.name == parsed.name
        && task_args.prompt == parsed.prompt
        && task_args.subagent_type == parsed.agent_name
}

fn to_subagent_item_view(item: &SubagentEventItem) -> Option<SubagentItemView> {
    let status = SubagentStatusView::from_wire(&item.status)?;
    Some(SubagentItemView {
        task_id: item.task_id.clone(),
        session_id: item.session_id.clone(),
        name: item.name.clone(),
        parent_task_id: item.parent_task_id.clone(),
        agent_name: item.agent_name.clone(),
        prompt: item.prompt.clone(),
        summary: item.summary.clone().or(item.error.clone()),
        depth: item.depth,
        started_at: item.started_at,
        finished_at: item.finished_at,
        status,
    })
}

fn normalize_custom_input(value: &str) -> String {
    value.trim_end_matches('\n').to_string()
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
        self.handle_input_event_with_runtime(event, None, None, None);
    }

    pub fn handle_input_event_with_runtime(
        &mut self,
        event: &crate::app::events::InputEvent,
        settings: Option<&crate::config::Settings>,
        cwd: Option<&std::path::Path>,
        event_sender: Option<&crate::app::events::TuiEventSender>,
    ) {
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
            if let (Some(settings), Some(cwd), Some(event_sender)) = (settings, cwd, event_sender) {
                self.dispatch_with_runtime(action, settings, cwd, event_sender);
            } else {
                self.dispatch(action);
            }
        }
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
            &mut self.state,
            &mut self.messages,
            &mut actions,
            terminal_size,
        )?;
        self.dispatch(AppAction::UpdateInput(
            self.state.input.clone(),
            self.state.cursor,
        ));
        for action in actions {
            self.dispatch_with_runtime(action, settings, cwd, event_sender);
        }
        Ok(())
    }

    pub fn dispatch_with_runtime(
        &mut self,
        initial: AppAction,
        settings: &crate::config::Settings,
        cwd: &std::path::Path,
        event_sender: &crate::app::events::TuiEventSender,
    ) {
        self.dispatch_internal(
            initial,
            Some(RuntimeDispatchContext {
                settings,
                cwd,
                event_sender,
            }),
        );
    }

    pub fn process_paste(&mut self, text: String) {
        crate::app::input::apply_paste(&mut self.state, text);
        self.dispatch(AppAction::UpdateInput(
            self.state.input.clone(),
            self.state.cursor,
        ));
    }

    pub fn process_area_scroll(
        &mut self,
        terminal_rect: crate::ui_compat::layout::Rect,
        x: u16,
        y: u16,
        up_steps: usize,
        down_steps: usize,
    ) {
        let mut actions = Vec::new();
        crate::app::input::handle_area_scroll(
            &mut self.state,
            &mut self.messages,
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
        terminal: &impl crate::app::runtime::TerminalBackend,
        settings: &crate::config::Settings,
        cwd: &std::path::Path,
        event_sender: &crate::app::events::TuiEventSender,
    ) {
        let mut actions = Vec::new();
        crate::app::input::handle_mouse_click(
            &mut self.state,
            &mut self.messages,
            &self.sidebar,
            &mut actions,
            x,
            y,
            terminal,
        );
        for action in actions {
            self.dispatch_with_runtime(action, settings, cwd, event_sender);
        }
    }

    pub fn process_mouse_drag(
        &mut self,
        x: u16,
        y: u16,
        terminal: &impl crate::app::runtime::TerminalBackend,
    ) {
        crate::app::input::handle_mouse_drag(&mut self.state, &mut self.messages, x, y, terminal);
    }

    pub fn process_mouse_release(
        &mut self,
        x: u16,
        y: u16,
        terminal: &impl crate::app::runtime::TerminalBackend,
    ) {
        if let Some(action) = crate::app::input::handle_mouse_release(
            &mut self.state,
            &mut self.messages,
            x,
            y,
            terminal,
        ) {
            self.dispatch(action);
        }
    }

    pub fn process_periodic_tick(
        &mut self,
        settings: &crate::config::Settings,
        cwd: &std::path::Path,
        event_sender: &crate::app::events::TuiEventSender,
    ) {
        self.dispatch(AppAction::PeriodicTick);

        self.dispatch_with_runtime(
            AppAction::RefreshActiveSubagentSession,
            settings,
            cwd,
            event_sender,
        );
    }

    pub fn dispatch(&mut self, initial: AppAction) {
        self.dispatch_internal(initial, None);
    }

    fn dispatch_internal(
        &mut self,
        initial: AppAction,
        runtime: Option<RuntimeDispatchContext<'_>>,
    ) {
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

            let action = match action {
                AppAction::SetAgentTask { handle, cancel_tx } => {
                    self.state.set_agent_task_with_cancel(handle, cancel_tx);
                    self.state.needs_redraw = true;
                    continue;
                }
                other => other,
            };

            self.reduce(&action);

            if let Some(runtime) = runtime.as_ref() {
                match &action {
                    AppAction::SubmitInput(text, attachments) => {
                        let submitted = crate::app::chat_state::SubmittedInput {
                            text: text.clone(),
                            attachments: attachments.clone(),
                            message_index: None,
                            queued: false,
                        };
                        let returned_actions =
                            crate::app::handlers::actions::handle_submitted_input(
                                submitted,
                                &self.state,
                                runtime.settings,
                                runtime.cwd,
                                runtime.event_sender,
                            );
                        for next in returned_actions {
                            queue.push_back(next);
                        }
                    }
                    AppAction::QueueUserMessage {
                        message,
                        message_index,
                    } => {
                        runtime.event_sender.enqueue_queued_user_message(
                            crate::core::QueuedUserMessage {
                                message: message.clone(),
                                message_index: Some(*message_index),
                            },
                        );
                    }
                    AppAction::OpenSubagentSession {
                        task_id,
                        session_id,
                        name,
                    } => {
                        if let Some(next) =
                            crate::app::handlers::session::load_subagent_session_action(
                                runtime.settings,
                                runtime.cwd,
                                task_id.clone(),
                                session_id.clone(),
                                name.clone(),
                            )
                        {
                            queue.push_back(next);
                        }
                    }
                    AppAction::RefreshActiveSubagentSession => {
                        if let Some(next) =
                            crate::app::handlers::session::load_active_subagent_session_action(
                                &self.state,
                                runtime.settings,
                                runtime.cwd,
                            )
                        {
                            queue.push_back(next);
                        }
                    }
                    _ => {}
                }
            }

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
                let _ = self.state.on_periodic_tick();
                self.state.needs_redraw = true;
            }
            AppAction::SelectSession(session_id) => {
                self.state.context.active_session_id = Some(session_id.clone());
                self.state.needs_redraw = true;
            }
            AppAction::CancelExecution => {
                self.state.cancel_agent_task();
                self.state.set_processing(false);
                self.state.needs_redraw = true;
            }
            AppAction::CancelAgentTask => {
                self.state.cancel_agent_task();
                self.state.needs_redraw = true;
            }
            AppAction::ReportDispatchOverflow => {
                self.state.needs_redraw = true;
            }
            AppAction::SubmitInput(..)
            | AppAction::QueueUserMessage { .. }
            | AppAction::SetAgentTask { .. }
            | AppAction::RunSlashCommand(..)
            | AppAction::OpenSubagentSession { .. }
            | AppAction::RefreshActiveSubagentSession
            | AppAction::ScrollMessages(..)
            | AppAction::ScrollSidebar(..)
            | AppAction::ToggleSidebarSection(..)
            | AppAction::ShowClipboardNotice { .. }
            | AppAction::UpdateInput(..)
            | AppAction::ClearInput => {}
            AppAction::SetSessionIdentity {
                session_id,
                session_name,
            } => {
                self.state.session_id = Some(session_id.clone());
                self.state.session_name = session_name.clone();
                self.state.context.active_session_id = self.state.session_id.clone();
                self.state.needs_redraw = true;
            }
            AppAction::ResumeSessionLoaded {
                session_id,
                session_name,
                messages,
                todo_items,
                subagent_items,
            } => {
                self.state.bump_session_epoch();
                self.state.session_id = Some(session_id.clone());
                self.state.session_name = session_name.clone();
                self.state.context.active_session_id = self.state.session_id.clone();
                self.state.last_context_tokens = None;
                self.state.is_picking_session = false;
                self.state.messages = messages.clone();
                self.state.todo_items = todo_items.clone();
                self.state.subagent_items = subagent_items.clone();
                self.state.needs_redraw = true;
            }
            AppAction::RemoveMessageAt(index) => {
                self.state.remove_message_at(*index);
                self.state.needs_redraw = true;
            }
            AppAction::ShowSessionPicker(sessions) => {
                self.state.available_sessions = sessions.clone();
                self.state.is_picking_session = true;
                self.state.needs_redraw = true;
            }
            AppAction::SubagentSessionLoaded {
                task_id,
                session_id,
                name,
                messages,
            } => {
                self.state.open_subagent_session(
                    task_id.clone(),
                    session_id.clone(),
                    name.clone(),
                    messages.clone(),
                );
                self.state.needs_redraw = true;
            }
            AppAction::ActiveSubagentMessagesLoaded { messages } => {
                self.state
                    .replace_active_subagent_messages(messages.clone());
                self.state.needs_redraw = true;
            }
            AppAction::SetProcessing(processing) => {
                self.state.set_processing(*processing);
                self.state.needs_redraw = true;
            }
            AppAction::AgentEvent(event) => {
                self.state.handle_agent_event(event);
                self.state.needs_redraw = true;
            }
            AppAction::UserMessageAppended(msg) => {
                self.state.messages.push(msg.clone());
                self.state.needs_redraw = true;
            }
            AppAction::AssistantMessageAppended(text) => {
                self.state
                    .messages
                    .push(crate::app::chat_state::ChatMessage::Assistant(text.clone()));
                self.state.needs_redraw = true;
            }
            AppAction::SystemMessageAppended(text) => {
                self.state
                    .messages
                    .push(crate::app::chat_state::ChatMessage::Assistant(text.clone()));
                self.state.needs_redraw = true;
            }
            AppAction::StartNewSession(session_name) => {
                self.state.start_new_session(session_name.clone());
                self.state.needs_redraw = true;
            }
            AppAction::SetSelectedModel(model_ref) => {
                self.state.set_selected_model(model_ref);
                self.state.needs_redraw = true;
            }
        }
    }

    pub fn render_root(&mut self, f: &mut crate::ui_compat::Frame<'_>) {
        crate::app::render::render_root_layout(f, self);
    }

    pub fn take_needs_redraw(&mut self) -> bool {
        let redraw = self.state.needs_redraw;
        self.state.needs_redraw = false;
        redraw
    }
}

fn current_line_bounds(input: &str, cursor: usize) -> (usize, usize) {
    let cursor = cursor.min(input.len());
    let start = input[..cursor].rfind('\n').map_or(0, |idx| idx + 1);
    let end = input[cursor..]
        .find('\n')
        .map_or(input.len(), |idx| cursor + idx);
    (start, end)
}

fn cursor_line_col(input: &str, cursor: usize) -> (usize, usize) {
    let cursor = cursor.min(input.len());
    let mut line = 0usize;
    let mut line_start = 0usize;

    for (idx, ch) in input.char_indices() {
        if idx >= cursor {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = idx + 1;
        }
    }

    let col = input[line_start..cursor].chars().count();
    (line, col)
}

fn line_col_to_cursor(input: &str, target_line: usize, target_col: usize) -> usize {
    let mut line_start = 0usize;

    for (line_idx, line) in input.split('\n').enumerate() {
        let line_end = line_start + line.len();
        if line_idx == target_line {
            let rel = line
                .char_indices()
                .nth(target_col)
                .map_or(line.len(), |(idx, _)| idx);
            return line_start + rel;
        }
        line_start = line_end + 1;
    }

    input.len()
}

fn line_bounds_by_index(input: &str, target_line: usize) -> Option<(usize, usize)> {
    let mut line_start = 0usize;

    for (line_idx, line) in input.split('\n').enumerate() {
        let line_end = line_start + line.len();
        if line_idx == target_line {
            return Some((line_start, line_end));
        }
        line_start = line_end + 1;
    }

    None
}

fn detect_git_branch(cwd: &std::path::Path) -> Option<String> {
    let branch = run_git_command(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch == "HEAD" {
        return run_git_command(cwd, &["rev-parse", "--short", "HEAD"])
            .map(|hash| format!("detached@{hash}"));
    }
    Some(branch)
}

fn run_git_command(cwd: &std::path::Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8(output.stdout).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::app::chat_state::ChatMessage;
    use crate::app::events::TuiEvent;

    fn build_app() -> App {
        App::new(AppState::new(Path::new(".").to_path_buf()))
    }

    #[test]
    fn dispatches_agent_event_into_runtime_state() {
        let mut app = build_app();
        app.state.set_processing(true);

        app.dispatch(AppAction::AgentEvent(TuiEvent::AssistantDelta(
            "hello".to_string(),
        )));
        app.dispatch(AppAction::AgentEvent(TuiEvent::AssistantDone));

        // Footer assertion removed because footer generation now lives in runtime state logic.
        // But we can check messages length.
        assert_eq!(app.state.messages.len(), 2); // assistant + footer

        let msg = app.state.messages.first().unwrap();
        match msg {
            ChatMessage::Assistant(text) => assert_eq!(text, "hello"),
            _ => panic!("Expected Assistant message"),
        }
        assert!(!app.state.context.is_processing);
    }

    #[test]
    fn set_processing_action_updates_runtime_processing_state() {
        let mut app = build_app();

        app.dispatch(AppAction::SetProcessing(true));
        assert!(app.state.context.is_processing);

        app.dispatch(AppAction::SetProcessing(false));
        assert!(!app.state.context.is_processing);
    }

    #[test]
    fn assistant_message_appended_updates_transcript() {
        let mut app = build_app();

        app.dispatch(AppAction::AssistantMessageAppended("ready".to_string()));

        assert!(matches!(
            app.state.messages.last(),
            Some(ChatMessage::Assistant(text)) if text == "ready"
        ));
    }

    #[test]
    fn dispatch_processes_component_followup_actions_in_order() {
        let mut app = build_app();
        app.messages.scroll.offset = 0;
        app.state.needs_redraw = false;

        app.dispatch(AppAction::ScrollMessages(-2));

        assert_eq!(app.messages.scroll.offset, 2);
        assert!(app.state.needs_redraw);
    }

    #[test]
    fn subagent_session_loaded_is_reduced_centrally() {
        let mut app = build_app();
        app.state.messages = vec![ChatMessage::Assistant("root".to_string())];

        app.dispatch(AppAction::SubagentSessionLoaded {
            task_id: "task-1".to_string(),
            session_id: "session-1".to_string(),
            name: "subagent-one".to_string(),
            messages: vec![ChatMessage::Assistant("child".to_string())],
        });

        assert_eq!(app.state.subagent_session_depth(), 1);
        assert!(matches!(
            app.state.messages.first(),
            Some(ChatMessage::Assistant(text)) if text == "child"
        ));
    }

    #[test]
    fn show_session_picker_is_reduced_centrally() {
        let mut app = build_app();
        assert!(!app.state.is_picking_session);

        app.dispatch(AppAction::ShowSessionPicker(vec![
            crate::session::SessionMetadata {
                id: "s1".to_string(),
                title: "Session One".to_string(),
                created_at: 1,
                last_updated_at: 1,
                parent_session_id: None,
                is_child_session: false,
                parent_tool_call_id: None,
                runner_state_snapshot: None,
            },
        ]));

        assert!(app.state.is_picking_session);
        assert_eq!(app.state.available_sessions.len(), 1);
        assert_eq!(app.state.available_sessions[0].title, "Session One");
    }

    #[test]
    fn remove_message_at_is_reduced_centrally() {
        let mut app = build_app();
        app.state.messages = vec![
            ChatMessage::Assistant("first".to_string()),
            ChatMessage::Assistant("second".to_string()),
        ];

        app.dispatch(AppAction::RemoveMessageAt(0));

        assert_eq!(app.state.messages.len(), 1);
        assert!(matches!(
            app.state.messages.first(),
            Some(ChatMessage::Assistant(text)) if text == "second"
        ));
    }

    #[test]
    fn resume_session_loaded_is_reduced_centrally() {
        let mut app = build_app();
        app.state.is_picking_session = true;
        app.state.session_id = Some("old".to_string());

        app.dispatch(AppAction::ResumeSessionLoaded {
            session_id: "new-session".to_string(),
            session_name: "Resumed Session".to_string(),
            messages: vec![ChatMessage::Assistant("restored".to_string())],
            todo_items: vec![],
            subagent_items: vec![],
        });

        assert!(!app.state.is_picking_session);
        assert_eq!(app.state.session_id.as_deref(), Some("new-session"));
        assert_eq!(app.state.session_name, "Resumed Session");
        assert!(matches!(
            app.state.messages.first(),
            Some(ChatMessage::Assistant(text)) if text == "restored"
        ));
    }

    #[test]
    fn set_session_identity_is_reduced_centrally() {
        let mut app = build_app();
        app.dispatch(AppAction::SetSessionIdentity {
            session_id: "session-42".to_string(),
            session_name: "Bootstrap Session".to_string(),
        });

        assert_eq!(app.state.session_id.as_deref(), Some("session-42"));
        assert_eq!(app.state.session_name, "Bootstrap Session");
    }
}

impl App {
    pub fn get_message_lines(
        &mut self,
        width: usize,
        height: usize,
    ) -> Vec<crate::ui_compat::text::Line<'static>> {
        let total_lines = self.messages.viewport.get_lines(&self.state, width).len();
        let scroll_offset = self
            .state
            .message_scroll
            .effective_offset(total_lines, height);
        self.messages
            .viewport
            .get_visible_lines(&self.state, width, height, scroll_offset)
            .to_vec()
    }

    pub fn get_sidebar_lines(
        &mut self,
        width: u16,
        height: usize,
    ) -> Vec<crate::ui_compat::text::Line<'static>> {
        let lines =
            crate::app::components::sidebar::build_sidebar_lines(&self.state, &self.sidebar, width);
        let total_lines = lines.len();
        let scroll_offset = self.sidebar.scroll.effective_offset(total_lines, height);
        lines.into_iter().skip(scroll_offset).take(height).collect()
    }
}
