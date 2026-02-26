use std::cell::RefCell;
use std::path::Path;
use std::time::Instant;

use ratatui::text::Line;

use super::commands::{SlashCommand, get_default_commands};
use super::event::TuiEvent;
use super::tool_render::render_tool_result;
use crate::core::MessageAttachment;

const SIDEBAR_WIDTH: u16 = 38;
const LEFT_COLUMN_RIGHT_MARGIN: u16 = 2;

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

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User(String),
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
}

use crate::session::SessionMetadata;

pub struct ChatApp {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub cursor: usize,
    pub scroll_offset: usize,
    pub should_quit: bool,
    pub is_processing: bool,
    pub auto_scroll: bool, // When true, follow new content
    pub session_id: Option<String>,
    pub session_name: String,
    pub working_directory: String,
    pub context_budget: usize,
    processing_started_at: Option<Instant>,
    pub todo_items: Vec<TodoItemView>,
    // Cached rendered lines (rebuilt only when messages change)
    pub cached_lines: RefCell<Vec<Line<'static>>>,
    pub cached_width: RefCell<usize>,
    pub needs_rebuild: RefCell<bool>,
    pub available_sessions: Vec<SessionMetadata>,
    pub is_picking_session: bool,
    pub commands: Vec<SlashCommand>,
    pub filtered_commands: Vec<SlashCommand>,
    pub selected_command_index: usize,
    pub pending_attachments: Vec<MessageAttachment>,
    preferred_column: Option<usize>,
}

pub struct SubmittedInput {
    pub text: String,
    pub attachments: Vec<MessageAttachment>,
}

impl ChatApp {
    pub fn new(session_name: String, cwd: &Path, context_budget: usize) -> Self {
        let commands = get_default_commands();
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            scroll_offset: 0,
            should_quit: false,
            is_processing: false,
            auto_scroll: true,
            session_id: None,
            session_name,
            working_directory: cwd.display().to_string(),
            context_budget,
            processing_started_at: None,
            todo_items: Vec::new(),
            cached_lines: RefCell::new(Vec::new()),
            cached_width: RefCell::new(0),
            needs_rebuild: RefCell::new(true),
            available_sessions: Vec::new(),
            is_picking_session: false,
            commands,
            filtered_commands: Vec::new(),
            selected_command_index: 0,
            pending_attachments: Vec::new(),
            preferred_column: None,
        }
    }

    pub fn handle_event(&mut self, event: &TuiEvent) {
        match event {
            TuiEvent::Thinking(text) => {
                self.append_thinking_delta(text);
                self.mark_dirty();
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
                self.mark_dirty();
            }
            TuiEvent::AssistantDelta(delta) => {
                if let Some(ChatMessage::Assistant(existing)) = self.messages.last_mut() {
                    existing.push_str(delta);
                    self.mark_dirty();
                    return;
                }
                self.messages.push(ChatMessage::Assistant(delta.clone()));
                self.mark_dirty();
            }
            TuiEvent::AssistantDone => {
                self.set_processing(false);
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
            TuiEvent::Error(msg) => {
                self.messages.push(ChatMessage::Error(msg.clone()));
                self.set_processing(false);
                self.mark_dirty();
            }
            TuiEvent::Tick => {}
            TuiEvent::Key(_) => {}
        }
    }

    pub fn submit_input(&mut self) -> SubmittedInput {
        let input = std::mem::take(&mut self.input);
        let attachments = std::mem::take(&mut self.pending_attachments);
        self.cursor = 0;
        self.preferred_column = None;
        if !input.is_empty() || !attachments.is_empty() {
            let extracted_todos = extract_todos(&input);
            if self.todo_items.is_empty() && !extracted_todos.is_empty() {
                self.todo_items = extracted_todos;
            }
            self.messages.push(ChatMessage::User(input.clone()));
            self.set_processing(true);
            self.auto_scroll = true; // Follow the new response
            self.mark_dirty();
        }
        SubmittedInput {
            text: input,
            attachments,
        }
    }

    /// Get or rebuild cached lines for the given width (interior mutability)
    pub fn get_lines(&self, width: usize) -> std::cell::Ref<'_, Vec<Line<'static>>> {
        let needs_rebuild = *self.needs_rebuild.borrow();
        let cached_width = *self.cached_width.borrow();

        if needs_rebuild || cached_width != width {
            let lines = super::ui::build_message_lines(self, width);
            *self.cached_lines.borrow_mut() = lines;
            *self.cached_width.borrow_mut() = width;
            *self.needs_rebuild.borrow_mut() = false;
        }
        self.cached_lines.borrow()
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
            self.auto_scroll = false; // User took control
        }
    }

    pub fn scroll_down(&mut self, max_lines: usize, visible_height: usize) {
        if self.scroll_offset < max_lines.saturating_sub(1) {
            self.scroll_offset += 1;
        }
        // Re-enable auto-scroll when scrolled to bottom
        let max_offset = max_lines.saturating_sub(visible_height);
        if self.scroll_offset >= max_offset {
            self.auto_scroll = true;
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

    pub fn context_usage(&self) -> (usize, usize) {
        let boundary = self
            .messages
            .iter()
            .rposition(|message| matches!(message, ChatMessage::Compaction(_)))
            .unwrap_or(0);
        let mut chars = self.input.len();
        for message in self.messages.iter().skip(boundary) {
            chars += match message {
                ChatMessage::User(text)
                | ChatMessage::Assistant(text)
                | ChatMessage::Compaction(text)
                | ChatMessage::Thinking(text) => text.len(),
                ChatMessage::CompactionPending => 0,
                ChatMessage::ToolCall {
                    name, args, output, ..
                } => name.len() + args.len() + output.as_ref().map(|s| s.len()).unwrap_or(0),
                ChatMessage::Error(text) => text.len(),
            };
        }
        let estimated_tokens = chars / 4;
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

    fn append_thinking_delta(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }

        if let Some(ChatMessage::Thinking(existing)) = self.messages.last_mut() {
            existing.push_str(delta);
            return;
        }

        self.messages.push(ChatMessage::Thinking(delta.to_string()));
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

        // Find the matching ToolCall
        for message in self.messages.iter_mut().rev() {
            if let ChatMessage::ToolCall {
                name: tool_name,
                is_error: status,
                output: out,
                ..
            } = message
                && tool_name == name
                && status.is_none()
            {
                *status = Some(result.is_error);
                *out = Some(rendered.text);
                return;
            }
        }
    }

    pub fn set_processing(&mut self, processing: bool) {
        self.is_processing = processing;
        self.processing_started_at = if processing {
            Some(Instant::now())
        } else {
            None
        };
    }

    pub fn start_new_session(&mut self, session_name: String) {
        self.messages.clear();
        self.todo_items.clear();
        self.session_id = None;
        self.session_name = session_name;
        self.available_sessions.clear();
        self.is_picking_session = false;
        self.scroll_offset = 0;
        self.auto_scroll = true;
        self.set_processing(false);
        self.mark_dirty();
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
        *self.needs_rebuild.borrow_mut() = true;
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
}

impl TodoStatus {
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
    pub fn from_wire(priority: &str) -> Option<Self> {
        match priority {
            "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

impl Default for ChatApp {
    fn default() -> Self {
        Self::new("Session".to_string(), Path::new("."), 32_000)
    }
}

fn extract_todos(input: &str) -> Vec<TodoItemView> {
    let mut todos = Vec::new();
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let item = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| split_numbered_list(trimmed));

        if let Some(todo) = item {
            let normalized = todo.trim();
            if !normalized.is_empty() {
                todos.push(TodoItemView {
                    content: normalized.to_string(),
                    status: TodoStatus::Pending,
                    priority: TodoPriority::Medium,
                });
            }
        }
    }
    todos
}

fn split_numbered_list(line: &str) -> Option<&str> {
    let chars = line.char_indices();
    let mut end_digits = None;

    for (idx, ch) in chars {
        if ch.is_ascii_digit() {
            end_digits = Some(idx + ch.len_utf8());
            continue;
        }
        break;
    }

    let end = end_digits?;
    let rest = line.get(end..)?;
    if let Some(rest) = rest.strip_prefix('.') {
        return rest.strip_prefix(' ');
    }
    if let Some(rest) = rest.strip_prefix(')') {
        return rest.strip_prefix(' ');
    }
    None
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
