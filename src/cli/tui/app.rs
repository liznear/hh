use serde_json::Value;

use super::event::TuiEvent;
use crate::cli::render::truncate_text;

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User(String),
    Assistant(String),
    Thinking(String),
    ToolStart { name: String, args: String },
    ToolEnd { name: String, is_error: bool, output: String },
}

pub struct ChatApp {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub scroll_offset: usize,
    pub thinking_expanded: bool,
    pub should_quit: bool,
    pub is_processing: bool,
    pub auto_scroll: bool, // When true, follow new content
}

impl ChatApp {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            scroll_offset: 0,
            thinking_expanded: false,
            should_quit: false,
            is_processing: false,
            auto_scroll: true,
        }
    }

    pub fn handle_event(&mut self, event: &TuiEvent) {
        match event {
            TuiEvent::Thinking(text) => {
                if self.thinking_expanded {
                    if let Some(last) = self.messages.last_mut() {
                        if let ChatMessage::Thinking(existing) = last {
                            existing.push_str(text);
                            return;
                        }
                    }
                }
                // Only add a placeholder if we don't have a thinking message yet
                if !self.messages.iter().any(|m| matches!(m, ChatMessage::Thinking(_))) {
                    self.messages.push(ChatMessage::Thinking(
                        if self.thinking_expanded {
                            text.clone()
                        } else {
                            "…".to_string()
                        },
                    ));
                } else if self.thinking_expanded {
                    // Append to existing thinking message
                    if let Some(last) = self.messages.last_mut() {
                        if let ChatMessage::Thinking(existing) = last {
                            existing.push_str(text);
                        }
                    }
                }
            }
            TuiEvent::ToolStart { name, args } => {
                let args_preview = format_args_preview(args, 100);
                self.messages.push(ChatMessage::ToolStart {
                    name: name.clone(),
                    args: args_preview,
                });
            }
            TuiEvent::ToolEnd { name, is_error, output } => {
                self.messages.push(ChatMessage::ToolEnd {
                    name: name.clone(),
                    is_error: *is_error,
                    output: truncate_text(output, 200),
                });
            }
            TuiEvent::AssistantDelta(delta) => {
                if let Some(last) = self.messages.last_mut() {
                    if let ChatMessage::Assistant(existing) = last {
                        existing.push_str(delta);
                        return;
                    }
                }
                self.messages.push(ChatMessage::Assistant(delta.clone()));
            }
            TuiEvent::AssistantDone => {
                self.is_processing = false;
            }
            TuiEvent::Tick => {}
            TuiEvent::Key(_) => {}
        }
    }

    pub fn submit_input(&mut self) -> String {
        let input = std::mem::take(&mut self.input);
        if !input.is_empty() {
            self.messages.push(ChatMessage::User(input.clone()));
            self.is_processing = true;
            self.auto_scroll = true; // Follow the new response
        }
        input
    }

    pub fn toggle_thinking(&mut self) {
        self.thinking_expanded = !self.thinking_expanded;
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
}

impl Default for ChatApp {
    fn default() -> Self {
        Self::new()
    }
}

fn format_args_preview(args: &Value, max_len: usize) -> String {
    let compact = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    truncate_text(&compact, max_len)
}
