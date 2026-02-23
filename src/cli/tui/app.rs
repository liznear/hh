use std::cell::RefCell;
use std::path::Path;
use std::time::Instant;

use ratatui::text::Line;
use serde_json::Value;

use super::event::TuiEvent;
use crate::cli::render::truncate_text;

const SIDEBAR_WIDTH: u16 = 38;
const PROGRESS_PANEL_HEIGHT: u16 = 12;
const MAX_PROGRESS_LINES_PER_PROMPT: usize = 200;

#[derive(Debug, Clone, Default)]
pub struct PromptProgress {
    pub prompt: String,
    pub lines: Vec<String>,
}

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
    pub progress_modal_open: bool,
    pub should_quit: bool,
    pub is_processing: bool,
    pub auto_scroll: bool, // When true, follow new content
    pub session_name: String,
    pub working_directory: String,
    pub context_budget: usize,
    pub progress_sections: Vec<PromptProgress>,
    pub selected_progress_section: usize,
    processing_started_at: Option<Instant>,
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
            progress_modal_open: false,
            should_quit: false,
            is_processing: false,
            auto_scroll: true,
            session_name,
            working_directory: cwd.display().to_string(),
            context_budget,
            progress_sections: Vec::new(),
            selected_progress_section: 0,
            processing_started_at: None,
            todo_items: Vec::new(),
            cached_lines: RefCell::new(Vec::new()),
            cached_width: RefCell::new(0),
            needs_rebuild: RefCell::new(true),
        }
    }

    pub fn handle_event(&mut self, event: &TuiEvent) {
        match event {
            TuiEvent::Thinking(text) => {
                self.append_thinking_delta(text);
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
                self.set_processing(false);
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
            self.begin_prompt_progress(input.clone());
            self.push_progress_line("user: submitted prompt".to_string());
            self.set_processing(true);
            self.auto_scroll = true; // Follow the new response
            *self.needs_rebuild.borrow_mut() = true;
        }
        input
    }

    pub fn begin_prompt_progress(&mut self, prompt: String) {
        self.progress_sections.push(PromptProgress {
            prompt,
            lines: Vec::new(),
        });
        self.selected_progress_section = self.progress_sections.len().saturating_sub(1);
    }

    pub fn toggle_progress(&mut self) {
        self.progress_modal_open = !self.progress_modal_open;
    }

    pub fn selected_progress(&self) -> Option<&PromptProgress> {
        self.progress_sections.get(self.selected_progress_section)
    }

    pub fn is_progress_modal_open(&self) -> bool {
        self.progress_modal_open
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
        PROGRESS_PANEL_HEIGHT
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
                ChatMessage::ToolStart { name, args } => name.len() + args.len(),
                ChatMessage::ToolEnd { name, output, .. } => name.len() + output.len(),
            };
        }
        for section in &self.progress_sections {
            chars += section.prompt.len();
            for line in &section.lines {
                chars += line.len();
            }
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

    pub fn push_progress_line(&mut self, line: String) {
        if line.trim().is_empty() {
            return;
        }
        let section = self.active_progress_section_mut();
        section.lines.push(line);
        if section.lines.len() > MAX_PROGRESS_LINES_PER_PROMPT {
            section
                .lines
                .drain(0..(section.lines.len() - MAX_PROGRESS_LINES_PER_PROMPT));
        }
    }

    fn append_thinking_delta(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }

        let chunk = delta.replace('\n', " ");
        let section = self.active_progress_section_mut();

        if let Some(last) = section.lines.last_mut()
            && let Some(existing) = last.strip_prefix("thinking: ")
        {
            let mut merged = String::with_capacity(last.len() + chunk.len());
            merged.push_str("thinking: ");
            merged.push_str(existing);
            merged.push_str(&chunk);
            *last = merged;
            return;
        }

        section.lines.push(format!("thinking: {}", chunk));
    }

    fn active_progress_section_mut(&mut self) -> &mut PromptProgress {
        if self.progress_sections.is_empty() {
            self.progress_sections.push(PromptProgress {
                prompt: "(no prompt)".to_string(),
                lines: Vec::new(),
            });
            self.selected_progress_section = 0;
        }

        let idx = self
            .selected_progress_section
            .min(self.progress_sections.len() - 1);
        &mut self.progress_sections[idx]
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
