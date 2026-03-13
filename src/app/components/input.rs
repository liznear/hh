use ratatui::{
    Frame,
    layout::Rect,
    prelude::Stylize,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Wrap},
};

use crate::app::components::commands::{SlashCommand, get_default_commands};
use crate::app::core::{AppAction, Component};
use crate::app::state::AppState;
use crate::theme::colors::*;

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

    pub(crate) fn render_input(&self, f: &mut Frame, app: &AppState, area: Rect, layout: UiLayout) {
        render_input_local(f, app, self, area, layout);
    }

    pub(crate) fn render_processing_indicator(
        &self,
        f: &mut Frame,
        app: &AppState,
        area: Rect,
        layout: UiLayout,
    ) {
        render_processing_indicator_local(f, app, area, layout);
    }

    pub(crate) fn render_subagent_back_indicator(
        &self,
        f: &mut Frame,
        app: &AppState,
        area: Rect,
        layout: UiLayout,
    ) {
        render_subagent_back_indicator_local(f, app, area, layout);
    }
}

fn render_input_local(
    f: &mut Frame,
    app: &AppState,
    input_comp: &InputComponent,
    area: Rect,
    layout: UiLayout,
) {
    let left_border_x = area.x.saturating_add(layout.user_bubble_indent() as u16);
    f.render_widget(Block::default().style(Style::default().bg(PAGE_BG)), area);
    let input_panel_area = Rect {
        x: left_border_x,
        y: area.y,
        width: area
            .width
            .saturating_sub(left_border_x.saturating_sub(area.x)),
        height: area.height,
    };
    f.render_widget(
        Block::default().style(Style::default().bg(INPUT_PANEL_BG)),
        input_panel_area,
    );

    let border_color = app
        .selected_agent()
        .and_then(|agent| agent.color.as_ref())
        .and_then(|c| crate::agent::parse_color(c))
        .unwrap_or(ACCENT);

    for y in area.y..area.bottom() {
        f.render_widget(
            Paragraph::new("▌").style(Style::default().fg(border_color).bg(INPUT_PANEL_BG)),
            Rect {
                x: left_border_x,
                y,
                width: 1,
                height: 1,
            },
        );
    }

    let content_y = area
        .y
        .saturating_add(1)
        .min(area.bottom().saturating_sub(1));
    let content_x = left_border_x.saturating_add(2);
    let content_height = area.height.saturating_sub(2).max(1);
    let input_height = if app.has_pending_question() {
        content_height.max(1)
    } else {
        content_height.saturating_sub(2).max(1)
    };
    let content_area = Rect {
        x: content_x,
        y: content_y,
        width: area
            .width
            .saturating_sub(content_x.saturating_sub(area.x) + 1),
        height: input_height,
    };

    if let Some(question) = app.pending_question_view() {
        let mut lines = Vec::new();
        let mut custom_input_row: Option<usize> = None;
        let mut custom_input_indent: usize = 0;
        lines.push(Line::from(Span::styled(
            question.question,
            Style::default().fg(TEXT_PRIMARY).bold(),
        )));
        lines.push(Line::from(""));

        for (idx, option) in question.options.iter().enumerate() {
            let option_style = if option.active {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else if option.selected {
                Style::default().fg(INPUT_ACCENT)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let prefix = if option.submit {
                format!("{}. ", idx + 1)
            } else if question.multiple {
                format!(
                    "{}. [{}] ",
                    idx + 1,
                    if option.selected { "x" } else { " " }
                )
            } else {
                format!("{}. ", idx + 1)
            };
            let prefix_width = prefix.chars().count();

            lines.push(Line::from(vec![
                Span::styled(prefix, option_style),
                Span::styled(option.label.clone(), option_style),
            ]));

            if option.custom {
                custom_input_indent = prefix_width;
            }

            if !option.description.trim().is_empty() {
                for description_line in option.description.split('\n') {
                    lines.push(Line::from(vec![
                        Span::raw(" ".repeat(prefix_width)),
                        Span::styled(
                            description_line.to_string(),
                            Style::default().fg(TEXT_MUTED),
                        ),
                    ]));
                }
            }
        }

        if question.custom_mode {
            custom_input_row = Some(lines.len());
            if question.custom_value.is_empty() {
                lines.push(Line::from(vec![
                    Span::raw(" ".repeat(custom_input_indent)),
                    Span::styled("Type your own answer", Style::default().fg(TEXT_MUTED)),
                ]));
            } else {
                for custom_line in question.custom_value.split('\n') {
                    lines.push(Line::from(vec![
                        Span::raw(" ".repeat(custom_input_indent)),
                        Span::styled(custom_line.to_string(), Style::default().fg(TEXT_SECONDARY)),
                    ]));
                }
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("↑↓", Style::default().fg(TEXT_PRIMARY)),
            Span::styled(" select", Style::default().fg(TEXT_MUTED)),
            Span::raw("  "),
            Span::styled("enter", Style::default().fg(TEXT_PRIMARY)),
            Span::styled(
                if question.custom_mode {
                    " submit"
                } else if question.multiple {
                    " toggle/submit"
                } else {
                    " submit"
                },
                Style::default().fg(TEXT_MUTED),
            ),
            Span::raw(if question.custom_mode { "  " } else { "" }),
            Span::styled(
                if question.custom_mode {
                    "shift+enter"
                } else {
                    ""
                },
                Style::default().fg(TEXT_PRIMARY),
            ),
            Span::styled(
                if question.custom_mode { " newline" } else { "" },
                Style::default().fg(TEXT_MUTED),
            ),
            Span::raw("  "),
            Span::styled("esc", Style::default().fg(TEXT_PRIMARY)),
            Span::styled(" dismiss", Style::default().fg(TEXT_MUTED)),
        ]));

        f.render_widget(
            Paragraph::new(Text::from(lines))
                .style(Style::default().fg(TEXT_PRIMARY).bg(INPUT_PANEL_BG))
                .wrap(Wrap { trim: false }),
            content_area,
        );

        if question.custom_mode
            && let Some(base_row) = custom_input_row
        {
            let custom_lines: Vec<&str> = if question.custom_value.is_empty() {
                vec![""]
            } else {
                question.custom_value.split('\n').collect()
            };
            let row = base_row + custom_lines.len().saturating_sub(1);
            let col = custom_input_indent
                + custom_lines
                    .last()
                    .map(|line| line.chars().count())
                    .unwrap_or(0);
            if row < content_area.height as usize && col < content_area.width as usize {
                f.set_cursor_position((content_area.x + col as u16, content_area.y + row as u16));
            }
        }
        return;
    }

    let (input_value, cursor_row, cursor_col) = if input_comp.text.is_empty() {
        ("Tell me more about this project...".to_string(), 0, 0)
    } else {
        let layout = input_viewport_layout(
            &input_comp.text,
            input_comp.cursor,
            content_area.width as usize,
            content_area.height as usize,
        );
        (
            layout.lines.join("\n"),
            layout.cursor_row,
            layout.cursor_col,
        )
    };

    f.render_widget(
        Paragraph::new(input_value)
            .style(Style::default().fg(TEXT_PRIMARY).bg(INPUT_PANEL_BG))
            .wrap(Wrap { trim: false }),
        content_area,
    );

    if (cursor_col as u16) < content_area.width && (cursor_row as u16) < content_area.height {
        f.set_cursor_position((
            content_area.x + cursor_col as u16,
            content_area.y + cursor_row as u16,
        ));
    }

    let status_y = content_y
        .saturating_add(content_height.saturating_sub(1))
        .min(area.bottom().saturating_sub(1));

    let status_lines = build_status_line(app);
    f.render_widget(
        Paragraph::new(status_lines)
            .style(Style::default().fg(TEXT_MUTED).bg(INPUT_PANEL_BG))
            .wrap(Wrap { trim: false }),
        Rect {
            x: content_x,
            y: status_y,
            width: area
                .width
                .saturating_sub(content_x.saturating_sub(area.x) + 1),
            height: 1,
        },
    );
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

fn selected_provider_name(app: &AppState) -> String {
    app.available_models
        .iter()
        .find(|model| model.full_id == app.current_model_ref)
        .map(|model| model.provider_name.clone())
        .or_else(|| {
            app.current_model_ref
                .split_once('/')
                .map(|(provider, _)| provider.to_string())
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            app.current_model_ref
                .split_once('/')
                .map(|(provider, _)| provider.to_string())
                .unwrap_or_else(|| app.current_model_ref.to_string())
        })
}

fn selected_model_name(app: &AppState) -> String {
    app.available_models
        .iter()
        .find(|model| model.full_id == app.current_model_ref)
        .map(|model| model.model_name.clone())
        .or_else(|| {
            app.current_model_ref
                .split_once('/')
                .map(|(_, model)| model.to_string())
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            app.current_model_ref
                .split_once('/')
                .map(|(_, model)| model.to_string())
                .unwrap_or_else(|| app.current_model_ref.to_string())
        })
}

fn build_status_line(app: &AppState) -> Line<'static> {
    let provider_name = selected_provider_name(app);
    let model_name = selected_model_name(app);

    if let Some(agent) = app.selected_agent() {
        let agent_color = agent
            .color
            .as_ref()
            .and_then(|c| crate::agent::parse_color(c))
            .unwrap_or(TEXT_PRIMARY);

        Line::from(vec![
            Span::styled(agent.display_name.clone(), Style::default().fg(agent_color)),
            Span::raw("  "),
            Span::styled(provider_name, Style::default().fg(TEXT_MUTED)),
            Span::raw(" "),
            Span::styled(model_name, Style::default().fg(TEXT_MUTED)),
        ])
    } else {
        Line::from(vec![
            Span::styled(provider_name, Style::default().fg(TEXT_MUTED)),
            Span::raw(" "),
            Span::styled(model_name, Style::default().fg(TEXT_MUTED)),
        ])
    }
}

#[derive(Clone)]
struct WrappedInputLine {
    text: String,
    start: usize,
    end: usize,
}

struct InputViewportLayout {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
}

fn input_viewport_layout(
    input: &str,
    cursor: usize,
    width: usize,
    height: usize,
) -> InputViewportLayout {
    if input.is_empty() {
        return InputViewportLayout {
            lines: Vec::new(),
            cursor_row: 0,
            cursor_col: 0,
        };
    }

    let wrapped = wrap_input_lines(input, width);
    let (cursor_line, cursor_col) = cursor_visual_position(input, cursor, &wrapped);
    let start = viewport_start(cursor_line, wrapped.len(), height);
    let end = (start + height.max(1)).min(wrapped.len());
    let lines = wrapped[start..end]
        .iter()
        .map(|line| line.text.clone())
        .collect();

    InputViewportLayout {
        lines,
        cursor_row: cursor_line.saturating_sub(start),
        cursor_col,
    }
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

fn cursor_visual_position(
    input: &str,
    cursor: usize,
    lines: &[WrappedInputLine],
) -> (usize, usize) {
    if lines.is_empty() {
        return (0, 0);
    }

    let cursor = cursor.min(input.len());
    for (idx, line) in lines.iter().enumerate() {
        if cursor < line.start {
            continue;
        }
        if cursor == line.end
            && idx + 1 < lines.len()
            && lines[idx + 1].start == cursor
            && line.end > line.start
        {
            continue;
        }
        if cursor <= line.end {
            let slice_end = cursor.min(line.end);
            let col = input[line.start..slice_end].chars().count();
            return (idx, col);
        }
    }

    let last = &lines[lines.len() - 1];
    (lines.len() - 1, input[last.start..last.end].chars().count())
}

fn viewport_start(cursor_line: usize, total_lines: usize, height: usize) -> usize {
    let height = height.max(1);
    if total_lines <= height {
        return 0;
    }
    if cursor_line < height {
        return 0;
    }
    if cursor_line >= total_lines.saturating_sub(height) {
        return total_lines.saturating_sub(height);
    }
    cursor_line + 1 - height
}

pub(crate) fn input_line_count(input: &str, width: usize) -> usize {
    wrap_input_lines(input, width).len()
}

fn blend_color_with_white(color: Color, amount: f64) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    let to_rgb = match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((255, 0, 0)),
        Color::Green => Some((0, 200, 0)),
        Color::Yellow => Some((220, 180, 0)),
        Color::Blue => Some((0, 102, 255)),
        Color::Magenta => Some((200, 0, 200)),
        Color::Cyan => Some((0, 180, 200)),
        Color::White => Some((255, 255, 255)),
        Color::Gray | Color::DarkGray => Some((128, 128, 128)),
        Color::LightRed => Some((255, 110, 103)),
        Color::LightGreen => Some((105, 255, 105)),
        Color::LightYellow => Some((255, 255, 105)),
        Color::LightBlue => Some((98, 114, 164)),
        Color::LightMagenta => Some((246, 108, 181)),
        Color::LightCyan => Some((114, 159, 207)),
        Color::Indexed(_) | Color::Reset => None,
    };

    if let Some((r, g, b)) = to_rgb {
        Color::Rgb(
            (r as f64 + (255.0 - r as f64) * amount).round() as u8,
            (g as f64 + (255.0 - g as f64) * amount).round() as u8,
            (b as f64 + (255.0 - b as f64) * amount).round() as u8,
        )
    } else {
        color
    }
}

fn render_processing_indicator_local(f: &mut Frame, app: &AppState, area: Rect, layout: UiLayout) {
    if !app.context.is_processing {
        return;
    }

    let agent_color = app
        .selected_agent()
        .and_then(|agent| agent.color.as_ref())
        .and_then(|color_str| crate::agent::parse_color(color_str));

    let mut spans: Vec<Span<'static>> = vec![Span::raw(layout.message_indent())];

    let bar_len = area.width.saturating_sub(35).clamp(6, 10) as usize;
    let head = scanner_position(app.processing_step(85), bar_len, 6);
    let base_color = agent_color.unwrap_or(PROGRESS_HEAD);

    for idx in 0..bar_len {
        let distance = head.abs_diff(idx);
        let (glyph, style) = if distance == 0 {
            (
                "■",
                Style::default().fg(base_color).add_modifier(Modifier::BOLD),
            )
        } else if distance == 1 {
            (
                "■",
                Style::default().fg(blend_color_with_white(base_color, 0.30)),
            )
        } else if distance == 2 {
            (
                "■",
                Style::default().fg(blend_color_with_white(base_color, 0.40)),
            )
        } else {
            (
                "⬝",
                Style::default().fg(blend_color_with_white(base_color, 0.52)),
            )
        };
        spans.push(Span::styled(glyph, style));
    }

    spans.push(Span::raw(PROCESSING_STATUS_GAP));
    spans.push(Span::styled(
        app.processing_duration(),
        Style::default().fg(TEXT_MUTED),
    ));
    spans.push(Span::raw(PROCESSING_STATUS_GAP));
    spans.push(Span::styled(
        app.processing_interrupt_hint(),
        Style::default().fg(TEXT_MUTED),
    ));

    let paragraph = Paragraph::new(Line::from(spans)).style(Style::default().bg(PAGE_BG));
    f.render_widget(paragraph, area);
}

fn render_subagent_back_indicator_local(
    f: &mut Frame,
    app: &AppState,
    area: Rect,
    layout: UiLayout,
) {
    let Some(view) = app.active_subagent_session() else {
        return;
    };

    let subagent_item = app
        .subagent_items
        .iter()
        .find(|item| item.task_id == view.task_id);
    let duration_secs = subagent_item
        .map(|item| {
            let end = item.finished_at.unwrap_or_else(now_unix_secs);
            end.saturating_sub(item.started_at)
        })
        .unwrap_or(0);

    let is_terminal = subagent_item.is_some_and(|item| item.status.is_terminal());
    if is_terminal {
        render_subagent_footer_line_local(f, app, area, layout, duration_secs, subagent_item);
        return;
    }

    let mut spans: Vec<Span<'static>> = vec![Span::raw(layout.message_indent())];
    let bar_len = area.width.saturating_sub(44).clamp(6, 10) as usize;
    let head = scanner_position(app.processing_step(85), bar_len, 6);
    let base_color = app
        .selected_agent()
        .and_then(|agent| agent.color.as_ref())
        .and_then(|color_str| crate::agent::parse_color(color_str))
        .unwrap_or(PROGRESS_HEAD);

    for idx in 0..bar_len {
        let distance = head.abs_diff(idx);
        let (glyph, style) = if distance == 0 {
            (
                "■",
                Style::default().fg(base_color).add_modifier(Modifier::BOLD),
            )
        } else if distance == 1 {
            (
                "■",
                Style::default().fg(blend_color_with_white(base_color, 0.30)),
            )
        } else if distance == 2 {
            (
                "■",
                Style::default().fg(blend_color_with_white(base_color, 0.40)),
            )
        } else {
            (
                "⬝",
                Style::default().fg(blend_color_with_white(base_color, 0.52)),
            )
        };
        spans.push(Span::styled(glyph, style));
    }

    spans.push(Span::raw(PROCESSING_STATUS_GAP));
    spans.push(Span::styled(
        app.processing_duration(),
        Style::default().fg(TEXT_MUTED),
    ));
    spans.push(Span::raw(PROCESSING_STATUS_GAP));
    spans.push(Span::styled("esc", Style::default().fg(TEXT_MUTED)));
    let back_label = if app.subagent_session_depth() > 1 {
        " back to upper subagent"
    } else {
        " back to main agent"
    };
    spans.push(Span::styled(back_label, Style::default().fg(TEXT_MUTED)));

    let paragraph = Paragraph::new(Line::from(spans)).style(Style::default().bg(PAGE_BG));
    f.render_widget(paragraph, area);
}

fn render_subagent_footer_line_local(
    f: &mut Frame,
    app: &AppState,
    area: Rect,
    layout: UiLayout,
    duration_secs: u64,
    item: Option<&crate::app::chat_state::SubagentItemView>,
) {
    let agent_color = app
        .selected_agent()
        .and_then(|a| a.color.as_ref())
        .and_then(|c| crate::agent::parse_color(c))
        .unwrap_or(TEXT_PRIMARY);

    let provider_name = app
        .available_models
        .iter()
        .find(|model| model.full_id == app.current_model_ref)
        .map(|model| model.provider_name.clone())
        .unwrap_or_default();
    let model_name = app
        .available_models
        .iter()
        .find(|model| model.full_id == app.current_model_ref)
        .map(|model| model.model_name.clone())
        .unwrap_or_default();

    let is_failed = item.is_some_and(|row| {
        matches!(
            row.status,
            crate::app::chat_state::SubagentStatusView::Failed
                | crate::app::chat_state::SubagentStatusView::Cancelled
        )
    });
    let (status_symbol, status_color) = if is_failed {
        ("✗", Color::Red)
    } else {
        ("✓", Color::Rgb(25, 110, 61))
    };

    let mut spans = vec![
        Span::raw(layout.message_indent()),
        Span::styled(status_symbol, Style::default().fg(status_color)),
        Span::raw("  "),
        Span::styled(
            app.selected_agent()
                .map(|a| a.display_name.clone())
                .unwrap_or_else(|| "Agent".to_string()),
            Style::default().fg(agent_color),
        ),
        Span::raw("  "),
        Span::styled(provider_name, Style::default().fg(TEXT_MUTED)),
        Span::raw(" "),
        Span::styled(model_name, Style::default().fg(TEXT_MUTED)),
        Span::raw("  "),
        Span::styled(
            format_elapsed_seconds(duration_secs),
            Style::default().fg(TEXT_PRIMARY),
        ),
    ];

    if is_failed {
        spans.push(Span::raw("  "));
        spans.push(Span::styled("interrupted", Style::default().fg(Color::Red)));
    }

    let paragraph = Paragraph::new(Line::from(spans)).style(Style::default().bg(PAGE_BG));
    f.render_widget(paragraph, area);
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn format_elapsed_seconds(secs: u64) -> String {
    let minutes = secs / 60;
    let seconds = secs % 60;
    if minutes == 0 {
        format!("{}s", seconds)
    } else {
        format!("{}m {}s", minutes, seconds)
    }
}

fn scanner_position(step: usize, width: usize, hold_frames: usize) -> usize {
    if width <= 1 {
        return 0;
    }

    let travel = width - 1;
    let cycle = hold_frames + travel + hold_frames + travel;
    let phase = step % cycle;

    if phase < hold_frames {
        0
    } else if phase < hold_frames + travel {
        phase - hold_frames
    } else if phase < hold_frames + travel + hold_frames {
        travel
    } else {
        travel - (phase - hold_frames - travel - hold_frames)
    }
}
