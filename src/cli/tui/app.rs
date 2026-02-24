use std::cell::RefCell;
use std::path::Path;
use std::time::Instant;

use ratatui::text::Line;

use super::event::TuiEvent;

const SIDEBAR_WIDTH: u16 = 38;

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User(String),
    Assistant(String),
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
    pub scroll_offset: usize,
    pub should_quit: bool,
    pub is_processing: bool,
    pub auto_scroll: bool, // When true, follow new content
    pub session_id: Option<String>,
    pub session_name: String,
    pub working_directory: String,
    pub context_budget: usize,
    processing_started_at: Option<Instant>,
    pub todo_items: Vec<String>,
    // Cached rendered lines (rebuilt only when messages change)
    pub cached_lines: RefCell<Vec<Line<'static>>>,
    pub cached_width: RefCell<usize>,
    pub needs_rebuild: RefCell<bool>,
    pub available_sessions: Vec<SessionMetadata>,
    pub is_picking_session: bool,
}

impl ChatApp {
    pub fn new(session_name: String, cwd: &Path, context_budget: usize) -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
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
        }
    }

    pub fn handle_event(&mut self, event: &TuiEvent) {
        match event {
            TuiEvent::Thinking(text) => {
                self.append_thinking_delta(text);
                *self.needs_rebuild.borrow_mut() = true;
            }
            TuiEvent::ToolStart { name, args } => {
                self.messages.push(ChatMessage::ToolCall {
                    name: name.clone(),
                    args: args.to_string(),
                    output: None,
                    is_error: None,
                });
                *self.needs_rebuild.borrow_mut() = true;
            }
            TuiEvent::ToolEnd {
                name,
                is_error,
                output,
            } => {
                self.complete_tool_call(name, *is_error, output);
                *self.needs_rebuild.borrow_mut() = true;
            }
            TuiEvent::AssistantDelta(delta) => {
                if let Some(ChatMessage::Assistant(existing)) = self.messages.last_mut() {
                    existing.push_str(delta);
                    *self.needs_rebuild.borrow_mut() = true;
                    return;
                }
                self.messages.push(ChatMessage::Assistant(delta.clone()));
                *self.needs_rebuild.borrow_mut() = true;
            }
            TuiEvent::AssistantDone => {
                self.set_processing(false);
            }
            TuiEvent::Error(msg) => {
                self.messages.push(ChatMessage::Error(msg.clone()));
                self.set_processing(false);
                *self.needs_rebuild.borrow_mut() = true;
            }
            TuiEvent::Tick => {}
            TuiEvent::Key(_) => {}
        }
    }

    pub fn submit_input(&mut self) -> String {
        let input = std::mem::take(&mut self.input);
        if !input.is_empty() {
            let extracted_todos = extract_todos(&input);
            if !extracted_todos.is_empty() {
                self.todo_items = extracted_todos;
            }
            self.messages.push(ChatMessage::User(input.clone()));
            self.set_processing(true);
            self.auto_scroll = true; // Follow the new response
            *self.needs_rebuild.borrow_mut() = true;
        }
        input
    }

    /// Get or rebuild cached lines for the given width (interior mutability)
    pub fn get_lines(&self, width: usize) -> std::cell::Ref<'_, Vec<Line<'static>>> {
        let needs_rebuild = *self.needs_rebuild.borrow();
        let cached_width = *self.cached_width.borrow();

        if needs_rebuild || cached_width != width {
            let lines = super::ui::build_message_lines_internal(self, width);
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
        total_height.saturating_sub(self.progress_panel_height() + 1 + 3 + 1 + 2) as usize
    }

    pub fn message_wrap_width(&self, total_width: u16) -> usize {
        let main_width = if total_width > SIDEBAR_WIDTH {
            total_width.saturating_sub(SIDEBAR_WIDTH)
        } else {
            total_width
        };
        main_width.saturating_sub(2) as usize
    }

    pub fn context_usage(&self) -> (usize, usize) {
        let mut chars = self.input.len();
        for message in &self.messages {
            chars += match message {
                ChatMessage::User(text)
                | ChatMessage::Assistant(text)
                | ChatMessage::Thinking(text) => text.len(),
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

        let chunk = delta.replace('\n', " ");
        if let Some(ChatMessage::Thinking(existing)) = self.messages.last_mut() {
            existing.push_str(&chunk);
            return;
        }

        self.messages.push(ChatMessage::Thinking(chunk));
    }

    fn complete_tool_call(&mut self, name: &str, is_error: bool, output: &str) {
        // Find the matching ToolCall
        for message in self.messages.iter_mut().rev() {
            if let ChatMessage::ToolCall {
                name: tool_name,
                is_error: status,
                output: out,
                ..
            } = message
            {
                if tool_name == name && status.is_none() {
                    *status = Some(is_error);
                    *out = Some(output.to_string());
                    return;
                }
            }
        }
        // If not found, ignore? or create new? ignoring is safer for now.
    }

    pub fn set_processing(&mut self, processing: bool) {
        self.is_processing = processing;
        self.processing_started_at = if processing {
            Some(Instant::now())
        } else {
            None
        };
    }
}

impl Default for ChatApp {
    fn default() -> Self {
        Self::new("Session".to_string(), Path::new("."), 32_000)
    }
}

fn extract_todos(input: &str) -> Vec<String> {
    let mut todos = Vec::new();
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let item = if let Some(rest) = trimmed.strip_prefix("- ") {
            Some(rest)
        } else if let Some(rest) = trimmed.strip_prefix("* ") {
            Some(rest)
        } else {
            split_numbered_list(trimmed)
        };

        if let Some(todo) = item {
            let normalized = todo.trim();
            if !normalized.is_empty() {
                todos.push(normalized.to_string());
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
