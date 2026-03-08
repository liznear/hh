use crate::core::ApprovalChoice;
use crate::core::RunnerOutputObserver;
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

impl RunnerOutputObserver for LiveRender {
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

    fn on_tool_end(&self, name: &str, result: &crate::tool::ToolResult) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if state.assistant_line_open || state.thinking_line_open {
            println!();
            state.assistant_line_open = false;
            state.thinking_line_open = false;
        }
        let status = if result.is_error { "error" } else { "ok" };
        println!(
            "tool:{}> {} {}",
            name,
            status,
            truncate_text(&result.summary, 220)
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

    fn on_error(&self, message: &str) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if state.thinking_line_open || state.assistant_line_open {
            println!();
            state.thinking_line_open = false;
            state.assistant_line_open = false;
        }
        drop(state);
        print_error(message);
    }

    fn on_cancelled(&self) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if state.thinking_line_open || state.assistant_line_open {
            println!();
            state.thinking_line_open = false;
            state.assistant_line_open = false;
        }
        println!("assistant> cancelled");
    }

    fn on_approval_required(&self, call_id: &str, request: &crate::core::ApprovalRequest) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if state.assistant_line_open || state.thinking_line_open {
            println!();
            state.assistant_line_open = false;
            state.thinking_line_open = false;
        }
        println!(
            "approval> {} ({call_id})",
            truncate_text(&request.body, 220)
        );
    }

    fn on_question_required(&self, call_id: &str, prompts: &[crate::core::QuestionPrompt]) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        if state.assistant_line_open || state.thinking_line_open {
            println!();
            state.assistant_line_open = false;
            state.thinking_line_open = false;
        }
        println!("question> {} prompt(s) required ({call_id})", prompts.len());
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

pub fn prompt_approval(request: &crate::core::ApprovalRequest) -> io::Result<ApprovalChoice> {
    println!();
    println!("{}", request.title);
    println!("{}", request.body);
    println!();
    println!("1. Allow Once");
    println!("2. Always Allow in Session");
    println!("3. Deny");
    print!("Choose [1-3] (default: 3): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    Ok(match input.trim() {
        "1" => ApprovalChoice::AllowOnce,
        "2" => ApprovalChoice::AllowSession,
        _ => ApprovalChoice::Deny,
    })
}

pub fn ask_questions(
    questions: &[crate::core::QuestionPrompt],
) -> io::Result<crate::core::QuestionAnswers> {
    let mut answers = Vec::with_capacity(questions.len());

    for question in questions {
        println!();
        println!("{}", question.question);
        println!();

        for (index, option) in question.options.iter().enumerate() {
            println!("{}. {}", index + 1, option.label);
            if !option.description.trim().is_empty() {
                println!("   {}", option.description);
            }
        }

        let custom_index = if question.custom {
            let index = question.options.len() + 1;
            println!("{}. Type your own answer", index);
            Some(index)
        } else {
            None
        };

        if question.multiple {
            print!("Select option numbers (comma-separated), or press enter to skip: ");
        } else {
            print!("Select an option number, or press enter to skip: ");
        }
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();

        if trimmed.is_empty() {
            answers.push(Vec::new());
            continue;
        }

        let mut selected = Vec::new();
        let tokens = if question.multiple {
            trimmed
                .split(',')
                .map(str::trim)
                .filter(|token| !token.is_empty())
                .collect::<Vec<_>>()
        } else {
            vec![trimmed]
        };

        for token in tokens {
            let Ok(choice) = token.parse::<usize>() else {
                continue;
            };

            if let Some(index) = custom_index
                && choice == index
            {
                print!("Type your own answer: ");
                io::stdout().flush()?;
                let mut custom = String::new();
                io::stdin().read_line(&mut custom)?;
                let custom = custom.trim();
                if !custom.is_empty() {
                    selected.push(custom.to_string());
                }
                continue;
            }

            if let Some(option) = question.options.get(choice.saturating_sub(1)) {
                selected.push(option.label.clone());
            }
        }

        selected.sort();
        selected.dedup();
        answers.push(selected);
    }

    Ok(answers)
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
