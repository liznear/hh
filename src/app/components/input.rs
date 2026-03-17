use crate::app::components::commands::{SlashCommand, get_default_commands};
use crate::app::core::{AppAction, Component};
use crate::app::state::AppState;

pub struct InputComponent {
    pub text: String,
    pub cursor: usize,
    pub commands: Vec<SlashCommand>,
    pub filtered_commands: Vec<SlashCommand>,
    pub selected_command_index: usize,
}

impl Default for InputComponent {
    fn default() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            commands: get_default_commands(),
            filtered_commands: Vec::new(),
            selected_command_index: 0,
        }
    }
}

impl Component for InputComponent {
    fn update(&mut self, action: &AppAction) -> Option<AppAction> {
        match action {
            AppAction::UpdateInput(text, cursor) => {
                self.text = text.clone();
                self.cursor = *cursor;
                self.update_command_filtering();
                Some(AppAction::Redraw)
            }
            AppAction::ClearInput => {
                self.text.clear();
                self.cursor = 0;
                self.filtered_commands.clear();
                Some(AppAction::Redraw)
            }
            _ => None,
        }
    }
}

impl InputComponent {
    pub fn update_command_filtering(&mut self) {
        if self.text.starts_with('/') {
            let query = self.text.trim();
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
}

pub(crate) fn question_prompt_line_count(app: &AppState, _width: usize) -> usize {
    let Some(question) = app.pending_question_view() else {
        return 1;
    };

    let body_rows = question
        .options
        .iter()
        .map(|option| {
            let description_rows = if option.description.trim().is_empty() {
                0
            } else {
                option.description.split('\n').count()
            };
            1 + description_rows
        })
        .sum::<usize>();
    let custom_rows = if question.custom_mode {
        question.custom_value.split('\n').count().max(1)
    } else {
        0
    };
    (body_rows + custom_rows + 4).max(1)
}

pub(crate) fn input_line_count(input: &str, width: usize) -> usize {
    wrap_input_lines(input, width).len()
}

fn wrap_input_lines(input: &str, width: usize) -> Vec<WrappedInputLine> {
    let max_width = width.max(1);
    let mut lines = Vec::new();
    let mut line_start = 0usize;
    let mut logical_lines = input.split('\n').peekable();

    while let Some(raw_line) = logical_lines.next() {
        push_wrapped_input_logical_line(&mut lines, raw_line, line_start, max_width);

        line_start += raw_line.len();
        if logical_lines.peek().is_some() {
            line_start += 1;
        }
    }

    if lines.is_empty() {
        lines.push(WrappedInputLine {
            text: String::new(),
            start: 0,
            end: 0,
        });
    }

    lines
}

fn push_wrapped_input_logical_line(
    lines: &mut Vec<WrappedInputLine>,
    raw_line: &str,
    line_start: usize,
    max_width: usize,
) {
    if raw_line.is_empty() {
        lines.push(WrappedInputLine {
            text: String::new(),
            start: line_start,
            end: line_start,
        });
        return;
    }

    let mut chunk_start_rel = 0usize;
    let mut chunk_chars = 0usize;

    for (rel, ch) in raw_line.char_indices() {
        if chunk_chars >= max_width {
            push_wrapped_input_chunk(lines, raw_line, line_start, chunk_start_rel, rel);
            chunk_start_rel = rel;
            chunk_chars = 0;
        }

        chunk_chars += 1;
        if rel + ch.len_utf8() == raw_line.len() {
            push_wrapped_input_chunk(lines, raw_line, line_start, chunk_start_rel, raw_line.len());
        }
    }
}

fn push_wrapped_input_chunk(
    lines: &mut Vec<WrappedInputLine>,
    raw_line: &str,
    line_start: usize,
    chunk_start_rel: usize,
    chunk_end_rel: usize,
) {
    lines.push(WrappedInputLine {
        text: raw_line[chunk_start_rel..chunk_end_rel].to_string(),
        start: line_start + chunk_start_rel,
        end: line_start + chunk_end_rel,
    });
}

#[derive(Clone)]
#[allow(dead_code)]
struct WrappedInputLine {
    text: String,
    start: usize,
    end: usize,
}
