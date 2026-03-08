use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::Line;
use serde::Deserialize;
use tokio::sync::{oneshot, watch};

struct RunningAgentTask {
    handle: tokio::task::JoinHandle<()>,
    cancel_tx: watch::Sender<bool>,
}

type QuestionResponder = std::sync::Arc<
    std::sync::Mutex<Option<oneshot::Sender<anyhow::Result<crate::core::QuestionAnswers>>>>,
>;

use super::commands::{SlashCommand, get_default_commands};
use super::event::{SubagentEventItem, TuiEvent};
use super::tool_render::render_tool_result;
use super::viewport_cache::MessageViewportCache;
use crate::core::MessageAttachment;

const SIDEBAR_WIDTH: u16 = 38;
const LEFT_COLUMN_RIGHT_MARGIN: u16 = 2;
const DEFAULT_CONTEXT_LIMIT: usize = 128_000;

#[derive(Debug, Clone, Copy)]
pub struct ScrollState {
    pub offset: usize,
    pub auto_follow: bool,
}

impl ScrollState {
    pub const fn new(auto_follow: bool) -> Self {
        Self {
            offset: 0,
            auto_follow,
        }
    }

    pub fn effective_offset(&self, total_lines: usize, visible_height: usize) -> usize {
        let max_offset = total_lines.saturating_sub(visible_height);
        if self.auto_follow {
            max_offset
        } else {
            self.offset.min(max_offset)
        }
    }

    pub fn scroll_up_steps(&mut self, total_lines: usize, visible_height: usize, steps: usize) {
        if steps == 0 {
            return;
        }

        if self.auto_follow {
            self.offset = total_lines.saturating_sub(visible_height);
            self.auto_follow = false;
        }

        self.offset = self.offset.saturating_sub(steps);
        self.auto_follow = false;
    }

    pub fn scroll_down_steps(&mut self, total_lines: usize, visible_height: usize, steps: usize) {
        if steps == 0 {
            return;
        }

        let max_offset = total_lines.saturating_sub(visible_height);
        self.offset = self.effective_offset(total_lines, visible_height);
        self.offset = self.offset.saturating_add(steps).min(max_offset);
        self.auto_follow = self.offset >= max_offset;
    }

    pub fn reset(&mut self, auto_follow: bool) {
        self.offset = 0;
        self.auto_follow = auto_follow;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItemView {
    pub content: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoPriority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatusView {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentItemView {
    pub task_id: String,
    pub session_id: String,
    pub name: String,
    pub parent_task_id: Option<String>,
    pub agent_name: String,
    pub prompt: String,
    pub summary: Option<String>,
    pub depth: usize,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub status: SubagentStatusView,
}

#[derive(Debug, Clone)]
pub struct SubagentSessionView {
    pub task_id: String,
    pub session_id: String,
    pub title: String,
    previous_messages: Vec<ChatMessage>,
    previous_scroll: ScrollState,
}

#[derive(Debug, Clone)]
pub struct TaskSessionTarget {
    pub task_id: String,
    pub session_id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User {
        text: String,
        queued: bool,
    },
    Assistant(String),
    CompactionPending,
    Compaction(String),
    Thinking(String),
    ToolCall {
        name: String,
        args: String,
        output: Option<String>,
        is_error: Option<bool>,
    },
    Error(String),
    Footer {
        agent_display_name: String,
        provider_name: String,
        model_name: String,
        duration: String,
        interrupted: bool,
    },
}

#[derive(Debug, Clone)]
pub struct PendingQuestionView {
    pub header: String,
    pub question: String,
    pub options: Vec<QuestionOptionView>,
    pub selected_index: usize,
    pub custom_mode: bool,
    pub custom_value: String,
    pub question_index: usize,
    pub total_questions: usize,
    pub multiple: bool,
}

#[derive(Debug, Clone)]
pub struct QuestionOptionView {
    pub label: String,
    pub description: String,
    pub selected: bool,
    pub active: bool,
    pub custom: bool,
    pub submit: bool,
}

#[derive(Debug)]
struct PendingQuestionState {
    questions: Vec<crate::core::QuestionPrompt>,
    answers: crate::core::QuestionAnswers,
    custom_values: Vec<String>,
    question_index: usize,
    selected_index: usize,
    custom_mode: bool,
    responder: Option<QuestionResponder>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionKeyResult {
    NotHandled,
    Handled,
    Submitted,
    Dismissed,
}

use crate::session::SessionMetadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionPosition {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct ClipboardNotice {
    pub x: u16,
    pub y: u16,
    pub expires_at: Instant,
}

impl SelectionPosition {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextSelection {
    None,
    InProgress {
        start: SelectionPosition,
    },
    Active {
        start: SelectionPosition,
        end: SelectionPosition,
    },
}

impl TextSelection {
    pub fn is_none(&self) -> bool {
        matches!(self, TextSelection::None)
    }

    pub fn is_active(&self) -> bool {
        matches!(self, TextSelection::Active { .. })
    }

    pub fn get_range(&self) -> Option<(SelectionPosition, SelectionPosition)> {
        match self {
            TextSelection::Active { start, end } => {
                let (start_pos, end_pos) = if start.line < end.line
                    || (start.line == end.line && start.column <= end.column)
                {
                    (*start, *end)
                } else {
                    (*end, *start)
                };
                Some((start_pos, end_pos))
            }
            _ => None,
        }
    }

    pub fn get_active_start(&self) -> Option<SelectionPosition> {
        match self {
            TextSelection::Active { start, .. } | TextSelection::InProgress { start } => {
                Some(*start)
            }
            TextSelection::None => None,
        }
    }
}

pub struct ChatApp {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub cursor: usize,
    pub message_scroll: ScrollState,
    pub sidebar_scroll: ScrollState,
    pub should_quit: bool,
    pub is_processing: bool,
    pub session_id: Option<String>,
    pub session_name: String,
    session_epoch: u64,
    run_epoch: u64,
    pub working_directory: String,
    pub git_branch: Option<String>,
    pub context_budget: usize,
    processing_started_at: Option<Instant>,
    pub todo_items: Vec<TodoItemView>,
    pub subagent_items: Vec<SubagentItemView>,
    message_viewport: MessageViewportCache,
    pub cached_sidebar_lines: RefCell<Vec<Line<'static>>>,
    pub cached_sidebar_width: RefCell<u16>,
    pub sidebar_needs_rebuild: RefCell<bool>,
    folded_sidebar_sections: HashSet<String>,
    pub cached_context_usage_estimate: RefCell<Option<usize>>,
    pub available_sessions: Vec<SessionMetadata>,
    pub is_picking_session: bool,
    pub commands: Vec<SlashCommand>,
    pub filtered_commands: Vec<SlashCommand>,
    pub selected_command_index: usize,
    pub pending_attachments: Vec<MessageAttachment>,
    pub current_model_ref: String,
    pub available_models: Vec<ModelOptionView>,
    pub last_context_tokens: Option<usize>,
    preferred_column: Option<usize>,
    // Text selection state
    pub text_selection: TextSelection,
    pub clipboard_notice: Option<ClipboardNotice>,
    pending_question: Option<PendingQuestionState>,
    // Agent state
    pub current_agent_name: Option<String>,
    pub available_agents: Vec<AgentOptionView>,
    subagent_session_stack: Vec<SubagentSessionView>,
    // Running agent task handle (for cancellation)
    agent_task: Option<RunningAgentTask>,
    esc_interrupt_pending: bool,
    // Footer state for completed agent runs
    last_run_duration: Option<String>,
    last_run_interrupted: bool,
    last_timer_refresh_second: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOptionView {
    pub name: String,
    pub display_name: String,
    pub color: Option<String>,
    pub mode: String,
}

pub struct SubmittedInput {
    pub text: String,
    pub attachments: Vec<MessageAttachment>,
    pub message_index: Option<usize>,
    pub queued: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelOptionView {
    pub full_id: String,
    pub provider_name: String,
    pub model_name: String,
    pub modality: String,
    pub max_context_size: usize,
}

impl ChatApp {
    pub fn new(session_name: String, cwd: &Path) -> Self {
        let commands = get_default_commands();
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            message_scroll: ScrollState::new(true),
            sidebar_scroll: ScrollState::new(false),
            should_quit: false,
            is_processing: false,
            session_id: None,
            session_name,
            session_epoch: 0,
            run_epoch: 0,
            working_directory: cwd.display().to_string(),
            git_branch: detect_git_branch(cwd),
            context_budget: DEFAULT_CONTEXT_LIMIT,
            processing_started_at: None,
            todo_items: Vec::new(),
            subagent_items: Vec::new(),
            message_viewport: MessageViewportCache::new(),
            cached_sidebar_lines: RefCell::new(Vec::new()),
            cached_sidebar_width: RefCell::new(0),
            sidebar_needs_rebuild: RefCell::new(true),
            folded_sidebar_sections: HashSet::new(),
            cached_context_usage_estimate: RefCell::new(None),
            available_sessions: Vec::new(),
            is_picking_session: false,
            commands,
            filtered_commands: Vec::new(),
            selected_command_index: 0,
            pending_attachments: Vec::new(),
            current_model_ref: String::new(),
            available_models: Vec::new(),
            last_context_tokens: None,
            preferred_column: None,
            text_selection: TextSelection::None,
            clipboard_notice: None,
            pending_question: None,
            current_agent_name: None,
            available_agents: Vec::new(),
            subagent_session_stack: Vec::new(),
            agent_task: None,
            esc_interrupt_pending: false,
            last_run_duration: None,
            last_run_interrupted: false,
            last_timer_refresh_second: None,
        }
    }

    pub fn set_agents(&mut self, agents: Vec<AgentOptionView>, selected: Option<String>) {
        self.available_agents = agents;
        self.current_agent_name = selected;
    }

    pub fn cycle_agent(&mut self) {
        if self.available_agents.is_empty() {
            return;
        }

        // Only cycle through primary agents
        let primary_agents: Vec<_> = self
            .available_agents
            .iter()
            .filter(|a| a.mode == "primary")
            .collect();

        if primary_agents.is_empty() {
            return;
        }

        let current = self.current_agent_name.as_deref();

        if let Some(current_name) = current {
            // Find current agent index among primary agents
            if let Some(pos) = primary_agents.iter().position(|a| a.name == current_name) {
                // Move to next primary agent
                let next_pos = (pos + 1) % primary_agents.len();
                self.current_agent_name = Some(primary_agents[next_pos].name.clone());
                return;
            }
        }

        // If no current agent or not found, select first primary agent
        self.current_agent_name = Some(primary_agents[0].name.clone());
    }

    pub fn selected_agent(&self) -> Option<&AgentOptionView> {
        self.current_agent_name
            .as_ref()
            .and_then(|name| self.available_agents.iter().find(|a| a.name == *name))
    }

    pub fn is_viewing_subagent_session(&self) -> bool {
        !self.subagent_session_stack.is_empty()
    }

    pub fn active_subagent_session(&self) -> Option<&SubagentSessionView> {
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
        messages: Vec<ChatMessage>,
    ) {
        let previous_messages = std::mem::replace(&mut self.messages, messages);
        let previous_scroll = self.message_scroll;
        self.message_scroll = ScrollState::new(true);
        self.subagent_session_stack.push(SubagentSessionView {
            task_id,
            session_id,
            title,
            previous_messages,
            previous_scroll,
        });
        self.mark_message_dirty();
    }

    pub fn close_subagent_session(&mut self) {
        let Some(view) = self.subagent_session_stack.pop() else {
            return;
        };

        self.messages = view.previous_messages;
        self.message_scroll = view.previous_scroll;
        self.mark_message_dirty();
    }

    pub fn replace_active_subagent_messages(&mut self, messages: Vec<ChatMessage>) {
        if self.subagent_session_stack.is_empty() {
            return;
        }
        self.messages = messages;
        self.mark_message_dirty();
    }

    pub fn task_session_target_at_visual_line(
        &self,
        wrap_width: usize,
        visual_line: usize,
    ) -> Option<TaskSessionTarget> {
        if self.messages.is_empty() || wrap_width == 0 {
            return None;
        }

        let (_, starts) = super::ui::build_message_lines_with_starts(self, wrap_width);
        if starts.is_empty() {
            return None;
        }

        let msg_idx = starts.partition_point(|start| *start <= visual_line);
        let msg_idx = msg_idx.saturating_sub(1);
        let message = self.messages.get(msg_idx)?;
        let ChatMessage::ToolCall {
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

    pub fn handle_event(&mut self, event: &TuiEvent) {
        if !self.subagent_session_stack.is_empty() {
            self.handle_event_in_main_context(event);
            return;
        }

        self.handle_event_inner(event);
    }

    fn handle_event_in_main_context(&mut self, event: &TuiEvent) {
        if self.subagent_session_stack.is_empty() {
            self.handle_event_inner(event);
            return;
        }

        let Some(_) = self.subagent_session_stack.first() else {
            self.handle_event_inner(event);
            return;
        };

        {
            let first_view = self
                .subagent_session_stack
                .first_mut()
                .expect("subagent stack non-empty");
            std::mem::swap(&mut self.messages, &mut first_view.previous_messages);
            std::mem::swap(&mut self.message_scroll, &mut first_view.previous_scroll);
        }
        self.handle_event_inner(event);
        {
            let first_view = self
                .subagent_session_stack
                .first_mut()
                .expect("subagent stack non-empty");
            std::mem::swap(&mut self.message_scroll, &mut first_view.previous_scroll);
            std::mem::swap(&mut self.messages, &mut first_view.previous_messages);
        }
    }

    fn handle_event_inner(&mut self, event: &TuiEvent) {
        match event {
            TuiEvent::Thinking(text) => {
                if self.append_thinking_delta(text) {
                    self.mark_message_tail_dirty();
                } else {
                    self.mark_message_dirty();
                }
            }
            TuiEvent::ToolStart { name, args } => {
                if !self.is_duplicate_pending_tool_call(name, args) {
                    self.messages.push(ChatMessage::ToolCall {
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
                if let Some(ChatMessage::Assistant(existing)) = self.messages.last_mut() {
                    existing.push_str(delta);
                    self.mark_message_tail_dirty();
                    return;
                }
                self.messages.push(ChatMessage::Assistant(delta.clone()));
                self.mark_message_dirty();
            }
            TuiEvent::RunnerStateUpdated(state) => {
                self.last_context_tokens = Some(state.context_tokens);
                self.todo_items = state
                    .todo_items
                    .iter()
                    .map(|item| TodoItemView {
                        content: item.content.clone(),
                        status: TodoStatus::from_core(item.status.clone()),
                        priority: TodoPriority::from_core(item.priority.clone()),
                    })
                    .collect();
                self.mark_dirty();
            }
            TuiEvent::AssistantDone => {
                self.set_processing(false);

                // Append footer if we have duration info
                if let (Some(duration), Some(agent)) = (
                    self.last_run_duration.take(),
                    self.selected_agent().cloned(),
                ) {
                    // Get provider and model names
                    let provider_name = self
                        .available_models
                        .iter()
                        .find(|model| model.full_id == self.selected_model_ref())
                        .map(|model| model.provider_name.clone())
                        .unwrap_or_default();
                    let model_name = self
                        .available_models
                        .iter()
                        .find(|model| model.full_id == self.selected_model_ref())
                        .map(|model| model.model_name.clone())
                        .unwrap_or_default();

                    self.messages.push(ChatMessage::Footer {
                        agent_display_name: agent.display_name.clone(),
                        provider_name,
                        model_name,
                        duration: duration.clone(),
                        interrupted: self.last_run_interrupted,
                    });
                    self.mark_dirty();

                    // Reset interrupted flag
                    self.last_run_interrupted = false;
                }
            }
            TuiEvent::Cancelled => {
                self.set_processing(false);

                if let (Some(duration), Some(agent)) = (
                    self.last_run_duration.take(),
                    self.selected_agent().cloned(),
                ) {
                    let provider_name = self
                        .available_models
                        .iter()
                        .find(|model| model.full_id == self.selected_model_ref())
                        .map(|model| model.provider_name.clone())
                        .unwrap_or_default();
                    let model_name = self
                        .available_models
                        .iter()
                        .find(|model| model.full_id == self.selected_model_ref())
                        .map(|model| model.model_name.clone())
                        .unwrap_or_default();

                    self.messages.push(ChatMessage::Footer {
                        agent_display_name: agent.display_name.clone(),
                        provider_name,
                        model_name,
                        duration: duration.clone(),
                        interrupted: true,
                    });
                    self.mark_dirty();
                    self.last_run_interrupted = false;
                }
            }
            TuiEvent::ApprovalRequired { call_id, request } => {
                self.messages.push(ChatMessage::Thinking(format!(
                    "approval required ({call_id}): {}",
                    request.body
                )));
                self.mark_message_dirty();
            }
            TuiEvent::QuestionRequired { call_id, prompts } => {
                self.messages.push(ChatMessage::Thinking(format!(
                    "question required ({call_id}): {} prompt(s)",
                    prompts.len()
                )));
                self.mark_message_dirty();
            }
            TuiEvent::QueuedMessagesConsumed(indexes) => {
                self.clear_queued_user_messages(indexes);
            }
            TuiEvent::SessionTitle(title) => {
                self.session_name = title.clone();
                self.mark_dirty();
            }
            TuiEvent::CompactionStart => {
                self.messages.push(ChatMessage::CompactionPending);
                self.mark_dirty();
            }
            TuiEvent::CompactionDone(summary) => {
                let mut replaced_pending = false;
                for message in self.messages.iter_mut().rev() {
                    if matches!(message, ChatMessage::CompactionPending) {
                        *message = ChatMessage::Compaction(summary.clone());
                        replaced_pending = true;
                        break;
                    }
                }
                if !replaced_pending {
                    self.messages.push(ChatMessage::Compaction(summary.clone()));
                }
                self.set_processing(false);
                self.mark_dirty();
            }
            TuiEvent::QuestionPrompt {
                questions,
                responder,
            } => {
                self.pending_question = Some(PendingQuestionState {
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
                self.messages.push(ChatMessage::Error(msg.clone()));
                self.set_processing(false);
                self.mark_dirty();
            }
            TuiEvent::Tick => {}
            TuiEvent::Key(_) => {}
        }
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

    pub fn submit_input(&mut self) -> SubmittedInput {
        let input = std::mem::take(&mut self.input);
        let attachments = std::mem::take(&mut self.pending_attachments);
        self.cursor = 0;
        self.preferred_column = None;

        let queued = self.is_processing;
        let mut submitted = SubmittedInput {
            text: input,
            attachments,
            message_index: None,
            queued,
        };

        if submitted.text.is_empty() && submitted.attachments.is_empty() {
            return submitted;
        }

        self.messages.push(ChatMessage::User {
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

    pub fn clear_queued_user_messages(&mut self, indexes: &[usize]) {
        let mut changed = false;
        for index in indexes {
            if let Some(ChatMessage::User {
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

    /// Get or rebuild cached lines for the given width (interior mutability)
    pub fn get_lines(&self, width: usize) -> std::cell::Ref<'_, Vec<Line<'static>>> {
        self.message_viewport.get_lines(self, width)
    }

    pub fn get_visible_lines(
        &self,
        wrap_width: usize,
        visible_height: usize,
        scroll_offset: usize,
    ) -> std::cell::Ref<'_, Vec<Line<'static>>> {
        self.message_viewport
            .get_visible_lines(self, wrap_width, visible_height, scroll_offset)
    }

    pub fn get_sidebar_lines(&self, width: u16) -> std::cell::Ref<'_, Vec<Line<'static>>> {
        let needs_rebuild = *self.sidebar_needs_rebuild.borrow();
        let cached_width = *self.cached_sidebar_width.borrow();

        if needs_rebuild || cached_width != width {
            let lines = super::ui::build_sidebar_lines(self, width);
            *self.cached_sidebar_lines.borrow_mut() = lines;
            *self.cached_sidebar_width.borrow_mut() = width;
            *self.sidebar_needs_rebuild.borrow_mut() = false;
        }

        self.cached_sidebar_lines.borrow()
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
            .rposition(|message| matches!(message, ChatMessage::Compaction(_)))
            .unwrap_or(0);
        let mut chars = self.input.len();
        for message in self.messages.iter().skip(boundary) {
            chars += match message {
                ChatMessage::User { text, .. }
                | ChatMessage::Assistant(text)
                | ChatMessage::Compaction(text)
                | ChatMessage::Thinking(text) => text.len(),
                ChatMessage::CompactionPending => 0,
                ChatMessage::ToolCall {
                    name, args, output, ..
                } => name.len() + args.len() + output.as_ref().map(|s| s.len()).unwrap_or(0),
                ChatMessage::Error(text) => text.len(),
                ChatMessage::Footer { .. } => 0,
            };
        }
        let estimated_tokens = chars / 4;
        *self.cached_context_usage_estimate.borrow_mut() = Some(estimated_tokens);
        (estimated_tokens, self.context_budget)
    }

    pub fn processing_step(&self, interval_ms: u128) -> usize {
        if !self.is_processing {
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
        if !self.is_processing {
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

    fn append_thinking_delta(&mut self, delta: &str) -> bool {
        if delta.is_empty() {
            return false;
        }

        if let Some(ChatMessage::Thinking(existing)) = self.messages.last_mut() {
            existing.push_str(delta);
            return true;
        }

        self.messages.push(ChatMessage::Thinking(delta.to_string()));
        false
    }

    fn is_duplicate_pending_tool_call(&self, name: &str, args: &serde_json::Value) -> bool {
        let Some(ChatMessage::ToolCall {
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
                        ChatMessage::ToolCall {
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
                    let ChatMessage::ToolCall {
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
                    let ChatMessage::ToolCall {
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
            && let Some(ChatMessage::ToolCall {
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

    pub fn set_processing(&mut self, processing: bool) {
        // Capture final duration and interrupted status when ending processing
        if !processing && self.is_processing {
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

        self.is_processing = processing;
        if !processing {
            self.clear_pending_esc_interrupt();
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

    pub fn session_epoch(&self) -> u64 {
        self.session_epoch
    }

    pub fn run_epoch(&self) -> u64 {
        self.run_epoch
    }

    pub fn bump_session_epoch(&mut self) {
        self.session_epoch = self.session_epoch.wrapping_add(1);
    }

    pub fn bump_run_epoch(&mut self) {
        self.run_epoch = self.run_epoch.wrapping_add(1);
    }

    pub fn start_new_session(&mut self, session_name: String) {
        self.bump_session_epoch();
        self.messages.clear();
        self.subagent_session_stack.clear();
        self.todo_items.clear();
        self.subagent_items.clear();
        self.last_context_tokens = None;
        self.session_id = None;
        self.session_name = session_name;
        self.available_sessions.clear();
        self.is_picking_session = false;
        self.message_scroll.reset(true);
        self.sidebar_scroll.reset(false);
        self.set_processing(false);
        self.pending_question = None;
        self.cancel_agent_task();
        self.mark_dirty();
    }

    /// Cancel any running agent task
    pub fn cancel_agent_task(&mut self) {
        if let Some(task) = self.agent_task.take() {
            self.bump_run_epoch();
            let _ = task.cancel_tx.send(true);
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    if !task.handle.is_finished() {
                        task.handle.abort();
                    }
                });
            } else if !task.handle.is_finished() {
                task.handle.abort();
            }
        }
        self.clear_pending_esc_interrupt();
    }

    /// Set the agent task handle
    pub fn set_agent_task(&mut self, handle: tokio::task::JoinHandle<()>) {
        let (cancel_tx, _cancel_rx) = watch::channel(false);
        self.set_agent_task_with_cancel(handle, cancel_tx);
    }

    pub fn set_agent_task_with_cancel(
        &mut self,
        handle: tokio::task::JoinHandle<()>,
        cancel_tx: watch::Sender<bool>,
    ) {
        // Cancel any existing task first
        self.cancel_agent_task();
        self.agent_task = Some(RunningAgentTask { handle, cancel_tx });
    }

    pub fn arm_esc_interrupt(&mut self) {
        self.esc_interrupt_pending = true;
    }

    pub fn clear_pending_esc_interrupt(&mut self) {
        self.esc_interrupt_pending = false;
    }

    pub fn should_interrupt_on_esc(&self) -> bool {
        self.esc_interrupt_pending
    }

    pub fn processing_interrupt_hint(&self) -> &'static str {
        if self.esc_interrupt_pending {
            "esc again to interrupt"
        } else {
            "esc interrupt"
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

    pub fn mark_dirty(&self) {
        self.mark_message_dirty();
        self.mark_sidebar_dirty();
    }

    pub fn mark_message_dirty(&self) {
        self.message_viewport.mark_full_dirty();
    }

    pub fn mark_message_tail_dirty(&self) {
        self.message_viewport.mark_tail_dirty();
    }

    pub fn mark_sidebar_dirty(&self) {
        *self.sidebar_needs_rebuild.borrow_mut() = true;
        *self.cached_context_usage_estimate.borrow_mut() = None;
    }

    pub fn is_sidebar_section_folded(&self, section_id: &str) -> bool {
        self.folded_sidebar_sections.contains(section_id)
    }

    pub fn toggle_sidebar_section_folded(&mut self, section_id: &str) {
        if !self.folded_sidebar_sections.insert(section_id.to_string()) {
            self.folded_sidebar_sections.remove(section_id);
        }
        self.mark_sidebar_dirty();
    }

    pub fn set_sidebar_section_folded(&mut self, section_id: &str, folded: bool) {
        if folded {
            self.folded_sidebar_sections.insert(section_id.to_string());
        } else {
            self.folded_sidebar_sections.remove(section_id);
        }
    }

    pub fn configure_models(
        &mut self,
        current_model_ref: String,
        available_models: Vec<ModelOptionView>,
    ) {
        self.current_model_ref = current_model_ref;
        self.available_models = available_models;
        self.context_budget = self
            .available_models
            .iter()
            .find(|model| model.full_id == self.current_model_ref)
            .map(|model| model.max_context_size)
            .unwrap_or(DEFAULT_CONTEXT_LIMIT);
        self.last_context_tokens = None;
        self.mark_sidebar_dirty();
    }

    pub fn selected_model_ref(&self) -> &str {
        self.current_model_ref.as_str()
    }

    pub fn set_selected_model(&mut self, model_ref: &str) {
        self.current_model_ref = model_ref.to_string();
        self.context_budget = self
            .available_models
            .iter()
            .find(|model| model.full_id == self.current_model_ref)
            .map(|model| model.max_context_size)
            .unwrap_or(DEFAULT_CONTEXT_LIMIT);
        self.last_context_tokens = None;
        self.mark_sidebar_dirty();
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

    // Text selection methods
    pub fn start_selection(&mut self, line: usize, column: usize) {
        self.text_selection = TextSelection::InProgress {
            start: SelectionPosition::new(line, column),
        };
    }

    pub fn update_selection(&mut self, line: usize, column: usize) {
        match &self.text_selection {
            TextSelection::InProgress { start } => {
                self.text_selection = TextSelection::Active {
                    start: *start,
                    end: SelectionPosition::new(line, column),
                };
            }
            TextSelection::Active { start, .. } => {
                self.text_selection = TextSelection::Active {
                    start: *start,
                    end: SelectionPosition::new(line, column),
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

    pub fn show_clipboard_notice(&mut self, x: u16, y: u16) {
        self.clipboard_notice = Some(ClipboardNotice {
            x,
            y,
            expires_at: Instant::now() + std::time::Duration::from_secs(1),
        });
    }

    pub fn on_periodic_tick(&mut self) -> bool {
        let mut needs_redraw = self.is_processing;
        if let Some(notice) = self.clipboard_notice
            && Instant::now() > notice.expires_at
        {
            self.clipboard_notice = None;
            needs_redraw = true;
        }

        if self.is_processing {
            let now_second = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs());
            if self.last_timer_refresh_second != Some(now_second) {
                self.last_timer_refresh_second = Some(now_second);
                self.mark_message_dirty();
                needs_redraw = true;
            }
        }

        needs_redraw
    }

    pub fn active_clipboard_notice(&self) -> Option<ClipboardNotice> {
        self.clipboard_notice
            .filter(|notice| Instant::now() <= notice.expires_at)
    }

    /// Get selected text from the lines
    pub fn get_selected_text(&self, lines: &[Line<'static>]) -> String {
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
                .map(|s| s.content.as_ref())
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

    /// Check if a point (line, column) is within the selection
    pub fn is_point_selected(&self, line: usize, column: usize) -> bool {
        let (start, end) = match self.text_selection.get_range() {
            Some(range) => range,
            None => return false,
        };

        if line > end.line || (line == end.line && column > end.column) {
            return false;
        }

        if line < start.line || (line == start.line && column < start.column) {
            return false;
        }

        true
    }
}

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

fn normalize_custom_input(value: &str) -> String {
    value.trim_end_matches('\n').to_string()
}
impl TodoStatus {
    pub fn from_core(status: crate::core::TodoStatus) -> Self {
        match status {
            crate::core::TodoStatus::Pending => Self::Pending,
            crate::core::TodoStatus::InProgress => Self::InProgress,
            crate::core::TodoStatus::Completed => Self::Completed,
            crate::core::TodoStatus::Cancelled => Self::Cancelled,
        }
    }

    pub fn from_wire(status: &str) -> Option<Self> {
        match status {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "completed" => Some(Self::Completed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl TodoPriority {
    pub fn from_core(priority: crate::core::TodoPriority) -> Self {
        match priority {
            crate::core::TodoPriority::High => Self::High,
            crate::core::TodoPriority::Medium => Self::Medium,
            crate::core::TodoPriority::Low => Self::Low,
        }
    }

    pub fn from_wire(priority: &str) -> Option<Self> {
        match priority {
            "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

impl SubagentStatusView {
    /// Maps wire/session lifecycle statuses into the TUI view enum.
    ///
    /// Keep aliases (`queued`/`done`/`error`) stable because manager polling events
    /// currently emit label strings while session replay uses lifecycle enums.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Pending | Self::Running)
    }

    pub fn from_wire(status: &str) -> Option<Self> {
        match status {
            "pending" | "queued" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "completed" | "done" => Some(Self::Completed),
            "failed" | "error" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub fn from_lifecycle(status: crate::session::types::SubAgentLifecycleStatus) -> Self {
        match status {
            crate::session::types::SubAgentLifecycleStatus::Pending => Self::Pending,
            crate::session::types::SubAgentLifecycleStatus::Running => Self::Running,
            crate::session::types::SubAgentLifecycleStatus::Completed => Self::Completed,
            crate::session::types::SubAgentLifecycleStatus::Failed => Self::Failed,
            crate::session::types::SubAgentLifecycleStatus::Cancelled => Self::Cancelled,
        }
    }
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

impl Default for ChatApp {
    fn default() -> Self {
        Self::new("Session".to_string(), Path::new("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::tui::event::SubagentEventItem;
    use crate::session::types::SubAgentLifecycleStatus;

    #[test]
    fn periodic_tick_marks_redraw_when_processing_second_changes() {
        let mut app = ChatApp::default();
        app.set_processing(true);
        app.last_timer_refresh_second = Some(0);

        assert!(app.on_periodic_tick());
    }

    #[test]
    fn submit_input_marks_message_as_queued_while_processing() {
        let mut app = ChatApp::default();
        app.set_processing(true);
        app.set_input("queued follow-up".to_string());

        let submitted = app.submit_input();

        assert!(submitted.queued);
        assert!(matches!(
            app.messages.first(),
            Some(ChatMessage::User { text, queued: true }) if text == "queued follow-up"
        ));
    }

    #[test]
    fn clear_queued_user_messages_clears_queued_flag_on_message() {
        let mut app = ChatApp::default();
        app.messages.push(ChatMessage::User {
            text: "queued follow-up".to_string(),
            queued: true,
        });

        app.clear_queued_user_messages(&[0]);

        assert!(matches!(
            app.messages.first(),
            Some(ChatMessage::User { queued: false, .. })
        ));
    }

    #[test]
    fn subagent_status_view_maps_manager_labels_and_session_statuses() {
        assert_eq!(
            SubagentStatusView::from_wire("queued"),
            Some(SubagentStatusView::Pending)
        );
        assert_eq!(
            SubagentStatusView::from_wire("running"),
            Some(SubagentStatusView::Running)
        );
        assert_eq!(
            SubagentStatusView::from_wire("done"),
            Some(SubagentStatusView::Completed)
        );
        assert_eq!(
            SubagentStatusView::from_wire("error"),
            Some(SubagentStatusView::Failed)
        );
        assert_eq!(
            SubagentStatusView::from_wire("cancelled"),
            Some(SubagentStatusView::Cancelled)
        );
        assert_eq!(SubagentStatusView::from_wire("unknown"), None);

        assert_eq!(
            SubagentStatusView::from_lifecycle(SubAgentLifecycleStatus::Pending),
            SubagentStatusView::Pending
        );
        assert_eq!(
            SubagentStatusView::from_lifecycle(SubAgentLifecycleStatus::Running),
            SubagentStatusView::Running
        );
        assert_eq!(
            SubagentStatusView::from_lifecycle(SubAgentLifecycleStatus::Completed),
            SubagentStatusView::Completed
        );
        assert_eq!(
            SubagentStatusView::from_lifecycle(SubAgentLifecycleStatus::Failed),
            SubagentStatusView::Failed
        );
        assert_eq!(
            SubagentStatusView::from_lifecycle(SubAgentLifecycleStatus::Cancelled),
            SubagentStatusView::Cancelled
        );
    }

    #[test]
    fn to_subagent_item_view_prefers_summary_and_falls_back_to_error() {
        let with_summary = SubagentEventItem {
            task_id: "task-1".to_string(),
            session_id: "session-1".to_string(),
            name: "name".to_string(),
            agent_name: "general".to_string(),
            status: "done".to_string(),
            prompt: "prompt".to_string(),
            depth: 1,
            parent_task_id: None,
            started_at: 10,
            finished_at: Some(11),
            summary: Some("summary".to_string()),
            error: Some("error text".to_string()),
        };

        let mapped = to_subagent_item_view(&with_summary).expect("item");
        assert_eq!(mapped.summary.as_deref(), Some("summary"));
        assert_eq!(mapped.status, SubagentStatusView::Completed);

        let with_error_only = SubagentEventItem {
            summary: None,
            status: "error".to_string(),
            ..with_summary
        };
        let mapped_error = to_subagent_item_view(&with_error_only).expect("item");
        assert_eq!(mapped_error.summary.as_deref(), Some("error text"));
        assert_eq!(mapped_error.status, SubagentStatusView::Failed);
    }

    #[test]
    fn task_session_target_is_detected_from_completed_task_tool_row() {
        let mut app = ChatApp::default();
        app.messages.push(ChatMessage::ToolCall {
            name: "task".to_string(),
            args: serde_json::json!({"name": "Inspect", "description": "d", "prompt": "p", "subagent_type": "general"}).to_string(),
            output: Some(
                serde_json::json!({
                    "task_id": "task-1",
                    "session_id": "session-1",
                    "status": "done",
                    "name": "Inspect code",
                    "agent_name": "general",
                    "prompt": "...",
                    "depth": 1,
                    "started_at": 1,
                    "finished_at": 2
                })
                .to_string(),
            ),
            is_error: Some(false),
        });

        let target = app
            .task_session_target_at_visual_line(120, 0)
            .expect("target");
        assert_eq!(target.task_id, "task-1");
        assert_eq!(target.session_id, "session-1");
        assert_eq!(target.name, "Inspect code");
    }

    #[test]
    fn task_session_target_is_detected_while_task_is_running() {
        let mut app = ChatApp::default();
        app.messages.push(ChatMessage::ToolCall {
            name: "task".to_string(),
            args: serde_json::json!({
                "name": "Inspect code",
                "description": "d",
                "prompt": "scan repo",
                "subagent_type": "general"
            })
            .to_string(),
            output: None,
            is_error: None,
        });
        app.subagent_items.push(SubagentItemView {
            task_id: "task-running".to_string(),
            session_id: "session-running".to_string(),
            name: "Inspect code".to_string(),
            parent_task_id: None,
            agent_name: "general".to_string(),
            prompt: "scan repo".to_string(),
            summary: None,
            depth: 1,
            started_at: 1,
            finished_at: None,
            status: SubagentStatusView::Running,
        });

        let target = app
            .task_session_target_at_visual_line(120, 0)
            .expect("target");
        assert_eq!(target.task_id, "task-running");
        assert_eq!(target.session_id, "session-running");
    }

    #[test]
    fn open_and_close_subagent_session_restores_previous_messages() {
        let mut app = ChatApp::default();
        app.messages
            .push(ChatMessage::Assistant("main transcript".to_string()));

        app.open_subagent_session(
            "task-1".to_string(),
            "session-1".to_string(),
            "Inspect code".to_string(),
            vec![ChatMessage::Assistant("child transcript".to_string())],
        );

        assert!(app.is_viewing_subagent_session());
        assert!(matches!(
            app.messages.first(),
            Some(ChatMessage::Assistant(text)) if text == "child transcript"
        ));

        app.close_subagent_session();
        assert!(!app.is_viewing_subagent_session());
        assert!(matches!(
            app.messages.first(),
            Some(ChatMessage::Assistant(text)) if text == "main transcript"
        ));
    }

    #[test]
    fn open_subagent_session_switches_active_subagent_view() {
        let mut app = ChatApp::default();
        app.messages
            .push(ChatMessage::Assistant("main transcript".to_string()));

        app.open_subagent_session(
            "task-1".to_string(),
            "session-1".to_string(),
            "First child".to_string(),
            vec![ChatMessage::Assistant("first child transcript".to_string())],
        );
        app.open_subagent_session(
            "task-2".to_string(),
            "session-2".to_string(),
            "Second child".to_string(),
            vec![ChatMessage::Assistant(
                "second child transcript".to_string(),
            )],
        );

        let active = app.active_subagent_session().expect("active view");
        assert_eq!(active.task_id, "task-2");
        assert_eq!(active.session_id, "session-2");
        assert_eq!(active.title, "Second child");
        assert!(matches!(
            app.messages.first(),
            Some(ChatMessage::Assistant(text)) if text == "second child transcript"
        ));

        app.close_subagent_session();
        assert!(app.is_viewing_subagent_session());
        assert!(matches!(
            app.messages.first(),
            Some(ChatMessage::Assistant(text)) if text == "first child transcript"
        ));

        app.close_subagent_session();
        assert!(matches!(
            app.messages.first(),
            Some(ChatMessage::Assistant(text)) if text == "main transcript"
        ));
    }

    #[test]
    fn task_session_target_derives_child_session_for_pending_call_id() {
        let mut app = ChatApp::default();
        app.open_subagent_session(
            "parent-task".to_string(),
            "parent-session".to_string(),
            "Parent".to_string(),
            vec![ChatMessage::ToolCall {
                name: "task".to_string(),
                args: serde_json::json!({
                    "name": "Nested task",
                    "description": "d",
                    "prompt": "p",
                    "subagent_type": "general",
                    "__call_id": "child-task"
                })
                .to_string(),
                output: None,
                is_error: None,
            }],
        );

        let target = app
            .task_session_target_at_visual_line(120, 0)
            .expect("target");
        assert_eq!(target.task_id, "child-task");
        assert_eq!(target.session_id, "parent-session-child-task");
        assert_eq!(target.name, "Nested task");
    }

    #[test]
    fn task_tool_end_matches_pending_row_by_task_args() {
        let mut app = ChatApp::default();
        app.messages.push(ChatMessage::ToolCall {
            name: "task".to_string(),
            args: serde_json::json!({
                "name": "First",
                "description": "d1",
                "prompt": "p1",
                "subagent_type": "explore"
            })
            .to_string(),
            output: None,
            is_error: None,
        });
        app.messages.push(ChatMessage::ToolCall {
            name: "task".to_string(),
            args: serde_json::json!({
                "name": "Second",
                "description": "d2",
                "prompt": "p2",
                "subagent_type": "explore"
            })
            .to_string(),
            output: None,
            is_error: None,
        });

        let result = crate::tool::ToolResult::ok_json_typed(
            "sub-agent completed",
            "application/vnd.hh.subagent.task+json",
            serde_json::json!({
                "task_id": "task-first",
                "session_id": "session-first",
                "status": "done",
                "name": "First",
                "agent_name": "explore",
                "prompt": "p1",
                "depth": 1,
                "started_at": 1,
                "finished_at": 2
            }),
        );

        app.handle_event(&crate::cli::tui::TuiEvent::ToolEnd {
            name: "task".to_string(),
            result,
        });

        assert!(matches!(
            app.messages.first(),
            Some(ChatMessage::ToolCall {
                is_error: Some(false),
                ..
            })
        ));
        assert!(matches!(
            app.messages.get(1),
            Some(ChatMessage::ToolCall { is_error: None, .. })
        ));
    }

    #[test]
    fn task_session_target_maps_each_pending_task_row() {
        let mut app = ChatApp::default();
        for idx in 1..=3 {
            app.messages.push(ChatMessage::ToolCall {
                name: "task".to_string(),
                args: serde_json::json!({
                    "name": format!("Task {idx}"),
                    "description": "desc",
                    "prompt": format!("prompt-{idx}"),
                    "subagent_type": "explore"
                })
                .to_string(),
                output: None,
                is_error: None,
            });
            app.subagent_items.push(SubagentItemView {
                task_id: format!("task-{idx}"),
                session_id: format!("session-{idx}"),
                name: format!("Task {idx}"),
                parent_task_id: None,
                agent_name: "explore".to_string(),
                prompt: format!("prompt-{idx}"),
                summary: None,
                depth: 1,
                started_at: idx as u64,
                finished_at: None,
                status: SubagentStatusView::Running,
            });
        }

        let (_, starts) = crate::cli::tui::ui::build_message_lines_with_starts(&app, 120);
        assert!(starts.len() >= 3);

        let first = app
            .task_session_target_at_visual_line(120, starts[0])
            .expect("first target");
        let second = app
            .task_session_target_at_visual_line(120, starts[1])
            .expect("second target");
        let third = app
            .task_session_target_at_visual_line(120, starts[2])
            .expect("third target");

        assert_eq!(first.session_id, "session-1");
        assert_eq!(second.session_id, "session-2");
        assert_eq!(third.session_id, "session-3");
    }
}

fn detect_git_branch(cwd: &Path) -> Option<String> {
    let branch = run_git_command(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch == "HEAD" {
        return run_git_command(cwd, &["rev-parse", "--short", "HEAD"])
            .map(|hash| format!("detached@{hash}"));
    }
    Some(branch)
}

fn run_git_command(cwd: &Path, args: &[&str]) -> Option<String> {
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
