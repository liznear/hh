use std::cell::RefCell;
use std::path::Path;

use ratatui::text::Line;
use serde_json::Value;

use super::event::TuiEvent;
use crate::cli::render::truncate_text;

const SIDEBAR_WIDTH: u16 = 38;

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User(String),
    Assistant(String),
    Thinking(String),
    ToolStart {
        name: String,
        args: String,
    },
    ToolEnd {
        name: String,
        is_error: bool,
        output: String,
    },
}

pub struct ChatApp {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub scroll_offset: usize,
    pub progress_expanded: bool,
    pub should_quit: bool,
    pub is_processing: bool,
    pub auto_scroll: bool, // When true, follow new content
    pub session_name: String,
    pub working_directory: String,
    pub context_budget: usize,
    pub progress_log: Vec<String>,
    pub todo_items: Vec<String>,
    // Cached rendered lines (rebuilt only when messages change)
    cached_lines: RefCell<Vec<Line<'static>>>,
    cached_width: RefCell<usize>,
    needs_rebuild: RefCell<bool>,
}

impl ChatApp {
    pub fn new(session_name: String, cwd: &Path, context_budget: usize) -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            scroll_offset: 0,
            progress_expanded: true,
            should_quit: false,
            is_processing: false,
            auto_scroll: true,
            session_name,
            working_directory: cwd.display().to_string(),
            context_budget,
            progress_log: Vec::new(),
            todo_items: Vec::new(),
            cached_lines: RefCell::new(Vec::new()),
            cached_width: RefCell::new(0),
            needs_rebuild: RefCell::new(true),
        }
    }

    pub fn handle_event(&mut self, event: &TuiEvent) {
        match event {
            TuiEvent::Thinking(text) => {
                self.push_progress_line(format!("thinking: {}", text.trim()));
            }
            TuiEvent::ToolStart { name, args } => {
                let args_preview = format_args_preview(args, 100);
                self.push_progress_line(format!("tool {} > start {}", name, args_preview));
            }
            TuiEvent::ToolEnd {
                name,
                is_error,
                output,
            } => {
                let status = if *is_error { "error" } else { "ok" };
                self.push_progress_line(format!(
                    "tool {} > {} {}",
                    name,
                    status,
                    truncate_text(output, 120)
                ));
            }
            TuiEvent::AssistantDelta(delta) => {
                if let Some(last) = self.messages.last_mut() {
                    if let ChatMessage::Assistant(existing) = last {
                        existing.push_str(delta);
                        *self.needs_rebuild.borrow_mut() = true;
                        return;
                    }
                }
                self.messages.push(ChatMessage::Assistant(delta.clone()));
                *self.needs_rebuild.borrow_mut() = true;
            }
            TuiEvent::AssistantDone => {
                self.is_processing = false;
                self.push_progress_line("assistant: done".to_string());
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
            self.push_progress_line("user: submitted prompt".to_string());
            self.is_processing = true;
            self.auto_scroll = true; // Follow the new response
            *self.needs_rebuild.borrow_mut() = true;
        }
        input
    }

    pub fn toggle_progress(&mut self) {
        self.progress_expanded = !self.progress_expanded;
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
        if self.progress_expanded { 12 } else { 3 }
    }

    pub fn message_viewport_height(&self, total_height: u16) -> usize {
        total_height.saturating_sub(self.progress_panel_height() + 1 + 3 + 2) as usize
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
                ChatMessage::ToolStart { name, args } => name.len() + args.len(),
                ChatMessage::ToolEnd { name, output, .. } => name.len() + output.len(),
            };
        }
        for line in &self.progress_log {
            chars += line.len();
        }
        let estimated_tokens = chars / 4;
        (estimated_tokens, self.context_budget)
    }

    fn push_progress_line(&mut self, line: String) {
        if line.trim().is_empty() {
            return;
        }
        self.progress_log.push(line);
        if self.progress_log.len() > 200 {
            self.progress_log.drain(0..(self.progress_log.len() - 200));
        }
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
    let mut chars = line.char_indices();
    let mut end_digits = None;

    while let Some((idx, ch)) = chars.next() {
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

fn format_args_preview(args: &Value, max_len: usize) -> String {
    let compact = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    truncate_text(&compact, max_len)
}
