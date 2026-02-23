use crate::core::agent::AgentEvents;
use serde_json::Value;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingMode {
    Collapsed,
    Expanded,
}

#[derive(Debug, Clone)]
pub struct LiveRender {
    inner: Arc<Mutex<RenderState>>,
}

#[derive(Debug)]
struct RenderState {
    thinking_mode: ThinkingMode,
    thinking_placeholder_shown: bool,
    thinking_line_open: bool,
    assistant_line_open: bool,
}

impl LiveRender {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(RenderState {
                thinking_mode: ThinkingMode::Collapsed,
                thinking_placeholder_shown: false,
                thinking_line_open: false,
                assistant_line_open: false,
            })),
        }
    }

    pub fn begin_turn(&self) {
        if let Ok(mut state) = self.inner.lock() {
            state.thinking_placeholder_shown = false;
            state.thinking_line_open = false;
            state.assistant_line_open = false;
        }
    }

    pub fn toggle_thinking_mode(&self) -> ThinkingMode {
        if let Ok(mut state) = self.inner.lock() {
            state.thinking_mode = match state.thinking_mode {
                ThinkingMode::Collapsed => ThinkingMode::Expanded,
                ThinkingMode::Expanded => ThinkingMode::Collapsed,
            };
            state.thinking_mode
        } else {
            ThinkingMode::Collapsed
        }
    }

    pub fn thinking_mode(&self) -> ThinkingMode {
        self.inner
            .lock()
            .map(|s| s.thinking_mode)
            .unwrap_or(ThinkingMode::Collapsed)
    }
}

impl Default for LiveRender {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentEvents for LiveRender {
    fn on_thinking(&self, text: &str) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };

        match state.thinking_mode {
            ThinkingMode::Collapsed => {
                if !state.thinking_placeholder_shown {
                    if state.assistant_line_open {
                        println!();
                        state.assistant_line_open = false;
                    }
                    println!("thinking… (toggle with :thinking)");
                    state.thinking_placeholder_shown = true;
                }
            }
            ThinkingMode::Expanded => {
                if state.assistant_line_open {
                    println!();
                    state.assistant_line_open = false;
                }
                if !state.thinking_line_open {
                    print!("thinking> ");
                    state.thinking_line_open = true;
                }
                print!("{}", text);
                let _ = io::stdout().flush();
            }
        }
    }

    fn on_tool_start(&self, name: &str, args: &Value) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if state.assistant_line_open || state.thinking_line_open {
            println!();
            state.assistant_line_open = false;
            state.thinking_line_open = false;
        }
        println!("tool:{}> start {}", name, format_args_preview(args, 220));
    }

    fn on_tool_end(&self, name: &str, is_error: bool, output_preview: &str) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if state.assistant_line_open || state.thinking_line_open {
            println!();
            state.assistant_line_open = false;
            state.thinking_line_open = false;
        }
        let status = if is_error { "error" } else { "ok" };
        println!(
            "tool:{}> {} {}",
            name,
            status,
            truncate_text(output_preview, 220)
        );
    }

    fn on_assistant_delta(&self, delta: &str) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if state.thinking_line_open {
            println!();
            state.thinking_line_open = false;
        }
        if !state.assistant_line_open {
            print!("assistant> ");
            state.assistant_line_open = true;
        }
        print!("{}", delta);
        let _ = io::stdout().flush();
    }

    fn on_assistant_done(&self) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if state.thinking_line_open || state.assistant_line_open {
            println!();
            state.thinking_line_open = false;
            state.assistant_line_open = false;
        }
    }
}

pub fn print_assistant(text: &str) {
    println!("assistant> {}", text);
}

pub fn print_tool_log(name: &str, message: &str) {
    println!("tool:{}> {}", name, message);
}

pub fn print_error(message: &str) {
    eprintln!("error: {}", message);
}

pub fn print_info(message: &str) {
    println!("info: {}", message);
}

pub fn prompt_user() -> io::Result<String> {
    print!("you> ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

pub fn confirm(prompt: &str) -> io::Result<bool> {
    print!("{} [y/N]: ", prompt);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let normalized = input.trim().to_ascii_lowercase();
    Ok(normalized == "y" || normalized == "yes")
}

pub fn format_args_preview(args: &Value, max_len: usize) -> String {
    let compact = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    truncate_text(&compact, max_len)
}

pub fn truncate_text(input: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }

    let mut chars = input.chars();
    let head: String = chars.by_ref().take(max_len).collect();
    if chars.next().is_some() {
        format!("{}…", head)
    } else {
        head
    }
}
