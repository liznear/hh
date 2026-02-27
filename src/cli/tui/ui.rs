use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Stylize,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Clear, List, ListItem, Padding, Paragraph, Wrap},
};
use serde::Deserialize;
use serde_json::Value;
use std::iter::Peekable;

use super::app::{ChatApp, ChatMessage, TodoItemView, TodoStatus};
use super::markdown::markdown_to_lines_with_indent;
use super::tool_presentation::render_tool_start;

const SIDEBAR_WIDTH: u16 = 38;
const LEFT_COLUMN_RIGHT_MARGIN: u16 = 2;
const MAIN_OUTER_PADDING_X: u16 = 1;
const MAIN_OUTER_PADDING_Y: u16 = 1;
const MAX_TOOL_OUTPUT_LEN: usize = 200;
const USER_BUBBLE_INDENT: usize = 3;
const USER_BUBBLE_INNER_PADDING: usize = 1;
const MIN_DIFF_COLUMN_WIDTH: usize = 24;
const DIFF_LINE_NUMBER_WIDTH: usize = 4;
const MESSAGE_INDENT: &str = "     ";
const TOOL_PENDING_MARKER: &str = "-> ";
const PROCESSING_INDENT: &str = MESSAGE_INDENT;
const PROCESSING_STATUS_GAP: &str = "  ";
const SIDEBAR_INDENT: &str = "  ";
const SIDEBAR_LABEL_INDENT: &str = " ";

const PAGE_BG: Color = Color::Rgb(246, 247, 251);
const SIDEBAR_BG: Color = Color::Rgb(234, 238, 246);
const INPUT_PANEL_BG: Color = Color::Rgb(229, 233, 241);
const COMMAND_PALETTE_BG: Color = Color::Rgb(214, 220, 232);
const TEXT_PRIMARY: Color = Color::Rgb(37, 45, 58);
const TEXT_SECONDARY: Color = Color::Rgb(98, 108, 124);
const TEXT_MUTED: Color = Color::Rgb(125, 133, 147);
const ACCENT: Color = Color::Rgb(55, 114, 255);
const INPUT_ACCENT: Color = Color::Rgb(19, 164, 151);
const SELECTION_BG: Color = Color::Rgb(55, 114, 255);
const NOTICE_BG: Color = Color::Rgb(224, 227, 233);
const PROGRESS_TRACK: Color = Color::Rgb(203, 182, 248);
const PROGRESS_TRAIL: Color = Color::Rgb(162, 120, 238);
const PROGRESS_HEAD: Color = Color::Rgb(124, 72, 227);
const THINKING_LABEL: Color = Color::Rgb(227, 152, 67);
const CONTEXT_USAGE_YELLOW: Color = Color::Rgb(214, 168, 46);
const CONTEXT_USAGE_ORANGE: Color = Color::Rgb(227, 136, 46);
const CONTEXT_USAGE_RED: Color = Color::Rgb(196, 64, 64);
const DIFF_ADD_FG: Color = Color::Rgb(25, 110, 61);
const DIFF_ADD_BG: Color = Color::Rgb(226, 244, 235);
const DIFF_REMOVE_FG: Color = Color::Rgb(152, 45, 45);
const DIFF_REMOVE_BG: Color = Color::Rgb(252, 235, 235);
const DIFF_META_FG: Color = Color::Rgb(106, 114, 128);
const MAX_RENDERED_DIFF_LINES: usize = 120;
const MAX_RENDERED_DIFF_CHARS: usize = 8_000;
const MAX_INPUT_LINES: usize = 5;

#[derive(Debug, Deserialize)]
struct EditToolOutput {
    path: String,
    summary: EditDiffSummary,
    diff: String,
}

#[derive(Debug, Deserialize)]
struct EditDiffSummary {
    added_lines: usize,
    removed_lines: usize,
}

pub fn render_app(f: &mut Frame, app: &ChatApp) {
    f.render_widget(
        Block::default().style(Style::default().bg(PAGE_BG)),
        f.area(),
    );

    let app_area = inset_rect(f.area(), MAIN_OUTER_PADDING_X, MAIN_OUTER_PADDING_Y);
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(40),
            Constraint::Length(LEFT_COLUMN_RIGHT_MARGIN),
            Constraint::Length(SIDEBAR_WIDTH),
        ])
        .split(app_area);

    let main_area = columns[0];
    let sidebar_area = if columns.len() > 2 {
        Some(columns[2])
    } else {
        None
    };

    let input_content_width = main_area
        .width
        .saturating_sub(USER_BUBBLE_INDENT as u16 + 3) as usize;
    let input_line_count =
        input_line_count(&app.input, input_content_width).clamp(1, MAX_INPUT_LINES);
    let input_area_height = (input_line_count + 4) as u16;

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),                 // Space above progress
            Constraint::Length(1),                 // Global processing indicator
            Constraint::Length(1),                 // Space above input
            Constraint::Length(input_area_height), // Input area
        ])
        .split(main_area);

    render_messages(f, app, main_chunks[0]);
    render_processing_indicator(f, app, main_chunks[2]);
    render_input(f, app, main_chunks[4]);

    if !app.filtered_commands.is_empty() {
        let item_count = app.filtered_commands.len().min(5) as u16;
        let popup_height = item_count;
        let input_left = main_chunks[4].x.saturating_add(USER_BUBBLE_INDENT as u16);
        let input_width = main_chunks[4]
            .width
            .saturating_sub(USER_BUBBLE_INDENT as u16);
        let popup_area = Rect {
            x: input_left,
            y: main_chunks[4].y.saturating_sub(popup_height),
            width: input_width,
            height: popup_height,
        };
        render_command_palette(f, app, popup_area);
    }

    if let Some(area) = sidebar_area {
        let sidebar_bottom = main_chunks[4].bottom();
        let clipped_sidebar_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: sidebar_bottom.saturating_sub(area.y),
        };
        render_sidebar(f, app, clipped_sidebar_area);
    }

    render_clipboard_notice(f, app);
}

fn render_clipboard_notice(f: &mut Frame, app: &ChatApp) {
    let Some(notice) = app.active_clipboard_notice() else {
        return;
    };

    let label = "Copied";
    let width = (label.len() as u16).saturating_add(4);
    let height = 3u16;
    let area = f.area();

    if area.width < width || area.height < height {
        return;
    }

    let max_x = area.right().saturating_sub(width);
    let max_y = area.bottom().saturating_sub(height);
    let x = notice.x.saturating_add(1).clamp(area.x, max_x);
    let y = notice.y.saturating_sub(1).clamp(area.y, max_y);
    let popup = Rect {
        x,
        y,
        width,
        height,
    };

    f.render_widget(Clear, popup);
    let block = Block::default()
        .style(Style::default().bg(NOTICE_BG).fg(TEXT_MUTED))
        .padding(Padding::new(2, 2, 1, 1));
    let content = block.inner(popup);
    f.render_widget(block, popup);
    f.render_widget(
        Paragraph::new(label)
            .style(Style::default().fg(TEXT_PRIMARY).bg(NOTICE_BG))
            .wrap(Wrap { trim: true }),
        content,
    );
}

fn render_command_palette(f: &mut Frame, app: &ChatApp, area: Rect) {
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(COMMAND_PALETTE_BG)),
        area,
    );

    let name_width = app
        .filtered_commands
        .iter()
        .take(5)
        .map(|cmd| cmd.name.chars().count())
        .max()
        .unwrap_or(0)
        .clamp(12, 24)
        + 1;

    let content_width = area.width as usize;
    let list_left_padding = 2usize;
    let left_padding = " ".repeat(list_left_padding);
    let description_width = content_width.saturating_sub(list_left_padding + name_width + 1);

    let items: Vec<ListItem> = app
        .filtered_commands
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, cmd)| {
            let style = if i == app.selected_command_index {
                Style::default().fg(Color::White).bg(ACCENT)
            } else {
                Style::default().fg(TEXT_PRIMARY).bg(COMMAND_PALETTE_BG)
            };

            let description = truncate_chars(&cmd.description, description_width);

            ListItem::new(Line::from(vec![
                Span::raw(left_padding.clone()),
                Span::styled(format!("{:<name_width$}", cmd.name), Style::default()),
                Span::raw(" "),
                Span::styled(
                    description,
                    if i == app.selected_command_index {
                        Style::default().fg(Color::White)
                    } else {
                        Style::default().fg(TEXT_SECONDARY)
                    },
                ),
            ]))
            .style(style)
        })
        .collect();

    let list = List::new(items).style(Style::default().bg(COMMAND_PALETTE_BG));

    f.render_widget(list, area);
}

fn render_sidebar(f: &mut Frame, app: &ChatApp, area: Rect) {
    let block = Block::default().style(Style::default().bg(SIDEBAR_BG));
    let inner = block.inner(area);
    let content = inset_rect(inner, 2, 0);
    f.render_widget(block, area);

    let (used, budget) = app.context_usage();
    let context_percent = if budget == 0 {
        0
    } else {
        (used.saturating_mul(100) / budget).min(999)
    };
    let context_usage_color = if context_percent >= 60 {
        CONTEXT_USAGE_RED
    } else if context_percent >= 40 {
        CONTEXT_USAGE_ORANGE
    } else if context_percent >= 30 {
        CONTEXT_USAGE_YELLOW
    } else {
        TEXT_PRIMARY
    };

    let directory_text =
        format_sidebar_directory(&app.working_directory, app.git_branch.as_deref());
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            sidebar_prefixed(&app.session_name),
            Style::default().fg(TEXT_PRIMARY).bold(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            sidebar_prefixed(&abbreviate_path(
                &directory_text,
                content.width.saturating_sub(2) as usize,
            )),
            Style::default().fg(TEXT_PRIMARY),
        )),
        Line::from(""),
    ];

    let mut sections: Vec<Vec<Line<'static>>> = Vec::new();
    sections.push(vec![
        Line::from(Span::styled(
            sidebar_label("Context"),
            Style::default().fg(TEXT_SECONDARY).bold(),
        )),
        Line::from(Span::styled(
            sidebar_prefixed(&format!("{} / {} ({}%)", used, budget, context_percent)),
            Style::default().fg(context_usage_color),
        )),
    ]);

    let modified_files = collect_modified_files(&app.messages);
    if !modified_files.is_empty() {
        let mut modified_lines = vec![Line::from(Span::styled(
            sidebar_label("Modified Files"),
            Style::default().fg(TEXT_SECONDARY).bold(),
        ))];
        append_modified_file_list(&mut modified_lines, &modified_files, content.width as usize);
        sections.push(modified_lines);
    }

    if !app.todo_items.is_empty() {
        let mut todo_lines = vec![Line::from(Span::styled(
            sidebar_label("TODO"),
            Style::default().fg(TEXT_SECONDARY).bold(),
        ))];
        let done = app
            .todo_items
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .count();
        todo_lines.push(Line::from(Span::styled(
            sidebar_label(&format!("{} / {} done", done, app.todo_items.len())),
            Style::default().fg(TEXT_MUTED),
        )));

        append_sidebar_list(&mut todo_lines, &app.todo_items, app.todo_items.len());
        sections.push(todo_lines);
    }

    let section_count = sections.len();
    for (index, section) in sections.into_iter().enumerate() {
        lines.extend(section);
        if index + 1 < section_count {
            lines.push(Line::from(""));
        }
    }

    let sidebar = Paragraph::new(Text::from(lines))
        .style(Style::default().bg(SIDEBAR_BG))
        .wrap(Wrap { trim: true });
    f.render_widget(sidebar, content);
}

fn render_messages(f: &mut Frame, app: &ChatApp, area: ratatui::layout::Rect) {
    let panel = Block::default().style(Style::default().bg(PAGE_BG));
    let inner = panel.inner(area);
    f.render_widget(panel, area);

    let content = inner;

    let wrap_width = content.width as usize;
    let visible_height = content.height as usize;

    // Get cached lines and calculate scroll offset
    let lines = app.get_lines(wrap_width);
    let total_lines = lines.len();

    // Calculate scroll offset: auto-scroll to bottom if enabled, otherwise use manual offset
    let scroll_offset = if app.auto_scroll || app.scroll_offset + visible_height > total_lines {
        total_lines.saturating_sub(visible_height)
    } else {
        app.scroll_offset
    };

    let mut rendered_lines = lines.to_vec();
    apply_selection_highlight(&mut rendered_lines, app);
    let text = Text::from(rendered_lines);
    let paragraph = Paragraph::new(text)
        .style(Style::default().bg(PAGE_BG).fg(TEXT_PRIMARY))
        .scroll((scroll_offset as u16, 0));

    f.render_widget(paragraph, content);
}

fn apply_selection_highlight(lines: &mut [Line<'static>], app: &ChatApp) {
    let Some((start, end)) = app.text_selection.get_range() else {
        return;
    };

    for (line_idx, line) in lines.iter_mut().enumerate() {
        if line_idx < start.line || line_idx > end.line {
            continue;
        }

        let line_len = line_char_count(line);
        let start_col = if line_idx == start.line {
            start.column
        } else {
            0
        };
        let end_col = if line_idx == end.line {
            end.column
        } else {
            line_len
        };

        let clamped_start = start_col.min(line_len);
        let clamped_end = end_col.min(line_len);
        if clamped_start >= clamped_end {
            continue;
        }

        highlight_line_range(line, clamped_start, clamped_end);
    }
}

fn highlight_line_range(line: &mut Line<'static>, start: usize, end: usize) {
    let original_spans = std::mem::take(&mut line.spans);
    let mut highlighted = Vec::with_capacity(original_spans.len() + 2);
    let mut cursor = 0usize;

    for span in original_spans {
        let content = span.content.as_ref();
        let span_len = content.chars().count();
        let span_start = cursor;
        let span_end = span_start + span_len;

        if span_len == 0 || end <= span_start || start >= span_end {
            highlighted.push(span);
            cursor = span_end;
            continue;
        }

        let local_start = start.saturating_sub(span_start).min(span_len);
        let local_end = end.saturating_sub(span_start).min(span_len);

        if local_start > 0 {
            highlighted.push(Span::styled(
                char_slice(content, 0, local_start),
                span.style,
            ));
        }

        if local_start < local_end {
            let selected_style = span
                .style
                .patch(Style::default().bg(SELECTION_BG).fg(Color::White));
            highlighted.push(Span::styled(
                char_slice(content, local_start, local_end),
                selected_style,
            ));
        }

        if local_end < span_len {
            highlighted.push(Span::styled(
                char_slice(content, local_end, span_len),
                span.style,
            ));
        }

        cursor = span_end;
    }

    line.spans = highlighted;
}

fn line_char_count(line: &Line<'static>) -> usize {
    line.spans
        .iter()
        .map(|span| span.content.as_ref().chars().count())
        .sum()
}

fn char_slice(input: &str, start: usize, end: usize) -> String {
    input
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

/// Build message lines (used for caching and scroll bounds)
pub fn build_message_lines(app: &ChatApp, width: usize) -> Vec<Line<'static>> {
    build_message_lines_impl(app, width)
}

fn build_message_lines_impl(app: &ChatApp, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let tool_done_continuation = message_child_indent();
    let tool_pending_prefix = format!("{MESSAGE_INDENT}{TOOL_PENDING_MARKER}");
    let tool_pending_continuation = " ".repeat(tool_pending_prefix.chars().count());
    let tool_style = ToolCallRenderStyle {
        done_continuation: &tool_done_continuation,
        pending_prefix: &tool_pending_prefix,
        pending_continuation: &tool_pending_continuation,
    };

    for msg in &app.messages {
        match msg {
            ChatMessage::User(text) => {
                render_user_message_block(&mut lines, text, width);
            }
            ChatMessage::Assistant(text) => {
                ensure_single_blank_line(&mut lines);
                for line in parse_markdown_lines(text, width) {
                    lines.push(line);
                }
            }
            ChatMessage::CompactionPending => {
                render_compaction_block(&mut lines, None, width);
            }
            ChatMessage::Compaction(summary) => {
                render_compaction_block(&mut lines, Some(summary), width);
            }
            ChatMessage::Thinking(text) => {
                render_thinking_block(&mut lines, text, width);
            }
            ChatMessage::ToolCall {
                name,
                args,
                output,
                is_error,
            } => {
                let available_width = width.saturating_sub(4).max(1);
                render_tool_call_message(
                    &mut lines,
                    name,
                    args,
                    output.as_deref(),
                    *is_error,
                    available_width,
                    tool_style,
                );
            }
            ChatMessage::Error(text) => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::raw(MESSAGE_INDENT),
                    Span::styled("Error:", Style::default().fg(Color::Red).bold()),
                    Span::raw(" "),
                    Span::styled(text.clone(), Style::default().fg(Color::Red)),
                ]));
            }
        }
    }

    lines
}

/// Parse markdown text into styled lines with wrapping
fn parse_markdown_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    markdown_to_lines_with_indent(text, width, MESSAGE_INDENT)
}

fn parse_markdown_lines_unindented(text: &str, width: usize) -> Vec<Line<'static>> {
    markdown_to_lines_with_indent(text, width, "")
}

fn render_thinking_block(lines: &mut Vec<Line<'static>>, text: &str, width: usize) {
    ensure_single_blank_line(lines);

    let label = format!("{MESSAGE_INDENT}Thinking: ");
    let label_width = label.chars().count();
    let wrapped = parse_markdown_lines_unindented(text, width.saturating_sub(label_width).max(1));

    if wrapped.is_empty() {
        lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(THINKING_LABEL).italic(),
        )));
        lines.push(Line::from(""));
        return;
    }

    let continuation_indent = MESSAGE_INDENT.to_string();
    for (index, line) in wrapped.into_iter().enumerate() {
        let mut spans = Vec::with_capacity(line.spans.len() + 1);
        if index == 0 {
            spans.push(Span::styled(
                label.clone(),
                Style::default().fg(THINKING_LABEL).italic(),
            ));
        } else {
            spans.push(Span::raw(continuation_indent.clone()));
        }

        spans.extend(line.spans.into_iter().map(|span| {
            let style = span.style.fg(TEXT_SECONDARY);
            Span::styled(span.content.into_owned(), style)
        }));

        lines.push(Line::from(spans));
    }

    lines.push(Line::from(""));
}

fn render_compaction_block(lines: &mut Vec<Line<'static>>, summary: Option<&str>, width: usize) {
    ensure_single_blank_line(lines);

    let indent = MESSAGE_INDENT;
    let label = " Compaction ";
    let available = width.saturating_sub(indent.chars().count());
    let total_rule = available.max(label.chars().count() + 4);
    let side = total_rule.saturating_sub(label.chars().count()) / 2;
    let left = "-".repeat(side);
    let right = "-".repeat(total_rule.saturating_sub(side + label.chars().count()));

    lines.push(Line::from(vec![
        Span::raw(indent),
        Span::styled(left, Style::default().fg(TEXT_MUTED)),
        Span::styled(label, Style::default().fg(TEXT_MUTED)),
        Span::styled(right, Style::default().fg(TEXT_MUTED)),
    ]));
    lines.push(Line::from(""));

    if let Some(summary) = summary
        && !summary.trim().is_empty()
    {
        for line in parse_markdown_lines(summary, width) {
            lines.push(line);
        }
    }
}

/// Wrap text to a given width, returning a vector of lines.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }

    let mut result = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            result.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in line.split_whitespace() {
            if current.is_empty() {
                current = word.to_string();
            } else if current.len() + 1 + word.len() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                result.push(current);
                current = word.to_string();
            }
        }
        if !current.is_empty() {
            result.push(current);
        }
    }
    if result.is_empty() {
        result.push(String::new());
    }
    result
}

fn wrap_compact_text(text: &str, width: usize) -> Vec<String> {
    if text.chars().count() > MAX_TOOL_OUTPUT_LEN {
        let truncated = truncate_chars(text, MAX_TOOL_OUTPUT_LEN);
        return wrap_text(&truncated, width);
    }
    wrap_text(text, width)
}

fn push_wrapped_tool_rows(
    lines: &mut Vec<Line<'static>>,
    wrapped: &[String],
    first_prefix: Vec<Span<'static>>,
    continuation_prefix: Vec<Span<'static>>,
    text_style: Style,
) {
    for (index, text) in wrapped.iter().enumerate() {
        let mut row = if index == 0 {
            first_prefix.clone()
        } else {
            continuation_prefix.clone()
        };
        row.push(Span::styled(text.clone(), text_style));
        lines.push(Line::from(row));
    }
}

#[derive(Clone, Copy)]
struct ToolCallRenderStyle<'a> {
    done_continuation: &'a str,
    pending_prefix: &'a str,
    pending_continuation: &'a str,
}

fn render_tool_call_message(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    args: &str,
    output: Option<&str>,
    is_error: Option<bool>,
    available_width: usize,
    style: ToolCallRenderStyle<'_>,
) {
    let args_value: Value = serde_json::from_str(args).unwrap_or(Value::Null);
    let label = render_tool_start(name, &args_value).line;

    match is_error {
        Some(error) => {
            if !error
                && (name == "edit" || name == "write")
                && let Some(tool_output) = output
                && render_edit_diff_block(lines, name, tool_output, available_width)
            {
                return;
            }

            render_completed_tool_call(
                lines,
                name,
                &label,
                output,
                error,
                available_width,
                style.done_continuation,
            );
        }
        None => render_pending_tool_call(
            lines,
            &label,
            available_width,
            style.pending_prefix,
            style.pending_continuation,
        ),
    }
}

fn render_completed_tool_call(
    lines: &mut Vec<Line<'static>>,
    name: &str,
    label: &str,
    output: Option<&str>,
    is_error: bool,
    available_width: usize,
    tool_done_continuation: &str,
) {
    let completed_label = if is_error {
        label.to_string()
    } else {
        append_tool_result_count(name, label, output)
    };
    let symbol = if is_error { "x" } else { "✓" };
    let color = if is_error { Color::Red } else { INPUT_ACCENT };
    let wrapped = wrap_compact_text(&completed_label, available_width);

    push_wrapped_tool_rows(
        lines,
        &wrapped,
        vec![
            Span::raw(MESSAGE_INDENT),
            Span::styled(symbol, Style::default().fg(color).bold()),
            Span::raw(" "),
        ],
        vec![Span::raw(tool_done_continuation.to_string())],
        Style::default().fg(TEXT_SECONDARY),
    );
}

fn render_pending_tool_call(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    available_width: usize,
    tool_pending_prefix: &str,
    tool_pending_continuation: &str,
) {
    let wrapped = wrap_compact_text(label, available_width.saturating_sub(1));
    push_wrapped_tool_rows(
        lines,
        &wrapped,
        vec![Span::styled(
            tool_pending_prefix.to_string(),
            Style::default().fg(TEXT_MUTED),
        )],
        vec![Span::raw(tool_pending_continuation.to_string())],
        Style::default().fg(TEXT_SECONDARY),
    );
}

fn render_input(f: &mut Frame, app: &ChatApp, area: Rect) {
    let left_border_x = area.x.saturating_add(USER_BUBBLE_INDENT as u16);
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

    for y in area.y..area.bottom() {
        f.render_widget(
            Paragraph::new("▌").style(Style::default().fg(ACCENT).bg(INPUT_PANEL_BG)),
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
    let input_height = content_height.saturating_sub(2).max(1);
    let content_area = Rect {
        x: content_x,
        y: content_y,
        width: area
            .width
            .saturating_sub(content_x.saturating_sub(area.x) + 1),
        height: input_height,
    };

    let (input_value, cursor_row, cursor_col) = if app.input.is_empty() {
        ("Tell me more about this project...".to_string(), 0, 0)
    } else {
        let layout = input_viewport_layout(
            &app.input,
            app.cursor,
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
    let status = format!("{} {}", selected_provider_name(app), selected_model_name(app));
    f.render_widget(
        Paragraph::new(status)
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

fn selected_provider_name(app: &ChatApp) -> String {
    app.available_models
        .iter()
        .find(|model| model.full_id == app.selected_model_ref())
        .map(|model| model.provider_name.clone())
        .or_else(|| {
            app.selected_model_ref()
                .split_once('/')
                .map(|(provider, _)| provider.to_string())
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            app.selected_model_ref()
                .split_once('/')
                .map(|(provider, _)| provider.to_string())
                .unwrap_or_else(|| app.selected_model_ref().to_string())
        })
}

fn selected_model_name(app: &ChatApp) -> String {
    app.available_models
        .iter()
        .find(|model| model.full_id == app.selected_model_ref())
        .map(|model| model.model_name.clone())
        .or_else(|| {
            app.selected_model_ref()
                .split_once('/')
                .map(|(_, model)| model.to_string())
        })
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| {
            app.selected_model_ref()
                .split_once('/')
                .map(|(_, model)| model.to_string())
                .unwrap_or_else(|| app.selected_model_ref().to_string())
        })
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

fn input_line_count(input: &str, width: usize) -> usize {
    wrap_input_lines(input, width).len()
}

fn render_processing_indicator(f: &mut Frame, app: &ChatApp, area: Rect) {
    if !app.is_processing {
        return;
    }

    let mut spans: Vec<Span<'static>> = vec![Span::raw(PROCESSING_INDENT)];

    let bar_len = area.width.saturating_sub(35).clamp(6, 10) as usize;
    let head = scanner_position(app.processing_step(85), bar_len, 6);

    for idx in 0..bar_len {
        let distance = head.abs_diff(idx);
        let (glyph, style) = if distance == 0 {
            ("■", Style::default().fg(PROGRESS_HEAD))
        } else if distance == 1 {
            ("■", Style::default().fg(PROGRESS_TRAIL))
        } else if distance == 2 {
            ("■", Style::default().fg(PROGRESS_TRACK))
        } else {
            ("⬝", Style::default().fg(PROGRESS_TRACK))
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
        "esc interrupt",
        Style::default().fg(TEXT_MUTED),
    ));

    let paragraph = Paragraph::new(Line::from(spans)).style(Style::default().bg(PAGE_BG));
    f.render_widget(paragraph, area);
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

fn inset_rect(area: Rect, padding_x: u16, padding_y: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(padding_x),
        y: area.y.saturating_add(padding_y),
        width: area.width.saturating_sub(padding_x.saturating_mul(2)),
        height: area.height.saturating_sub(padding_y.saturating_mul(2)),
    }
}

fn abbreviate_path(path: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let path_chars = path.chars().count();
    if path_chars <= max_chars {
        return path.to_string();
    }

    let tail_chars = max_chars.saturating_sub(3);
    let tail: String = path
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("...{}", tail)
}

fn format_sidebar_directory(path: &str, git_branch: Option<&str>) -> String {
    let simplified = simplify_home_path(path);
    match git_branch {
        Some(branch) if !branch.is_empty() => format!("{simplified} @ {branch}"),
        _ => simplified,
    }
}

fn simplify_home_path(path: &str) -> String {
    let Some(home) = dirs::home_dir() else {
        return path.to_string();
    };

    let home = home.to_string_lossy();
    if path == home {
        return "~".to_string();
    }

    let home_prefix = format!("{home}/");
    if let Some(rest) = path.strip_prefix(&home_prefix) {
        return format!("~/{rest}");
    }

    path.to_string()
}

#[derive(Debug, Clone)]
struct ModifiedFileSummary {
    path: String,
    added_lines: usize,
    removed_lines: usize,
}

fn collect_modified_files(messages: &[ChatMessage]) -> Vec<ModifiedFileSummary> {
    let mut files: Vec<ModifiedFileSummary> = Vec::new();

    for message in messages {
        let ChatMessage::ToolCall {
            output, is_error, ..
        } = message
        else {
            continue;
        };

        if !matches!(is_error, Some(false)) {
            continue;
        }

        let Some(output) = output else {
            continue;
        };

        let Some(parsed) = parse_modified_file_summary(output) else {
            continue;
        };

        if let Some(existing) = files.iter_mut().find(|item| item.path == parsed.path) {
            existing.added_lines = existing.added_lines.saturating_add(parsed.added_lines);
            existing.removed_lines = existing.removed_lines.saturating_add(parsed.removed_lines);
            continue;
        }

        files.push(parsed);
    }

    files
}

fn parse_modified_file_summary(output: &str) -> Option<ModifiedFileSummary> {
    let value = serde_json::from_str::<Value>(output).ok()?;
    let path = value.get("path")?.as_str()?.to_string();
    let summary = value.get("summary")?;
    let added_lines = summary.get("added_lines")?.as_u64()? as usize;
    let removed_lines = summary.get("removed_lines")?.as_u64()? as usize;

    if added_lines == 0 && removed_lines == 0 {
        return None;
    }

    Some(ModifiedFileSummary {
        path,
        added_lines,
        removed_lines,
    })
}

fn append_modified_file_list(
    lines: &mut Vec<Line<'static>>,
    files: &[ModifiedFileSummary],
    content_width: usize,
) {
    let line_width = content_width.saturating_sub(SIDEBAR_INDENT.chars().count());

    for file in files {
        let added_text = if file.added_lines > 0 {
            format!("+{}", file.added_lines)
        } else {
            String::new()
        };
        let removed_text = if file.removed_lines > 0 {
            format!("-{}", file.removed_lines)
        } else {
            String::new()
        };
        let has_added = !added_text.is_empty();

        let gap = if has_added && !removed_text.is_empty() {
            1
        } else {
            0
        };
        let delta_len = added_text.chars().count() + removed_text.chars().count() + gap;
        let path_max = line_width.saturating_sub(delta_len + 1);
        let path_text = truncate_chars(&file.path, path_max.max(1));
        let spaces = line_width
            .saturating_sub(path_text.chars().count() + delta_len)
            .max(1);

        let mut spans = vec![
            Span::styled(
                sidebar_prefixed(&path_text),
                Style::default().fg(TEXT_SECONDARY),
            ),
            Span::raw(" ".repeat(spaces)),
        ];

        if has_added {
            spans.push(Span::styled(
                added_text,
                Style::default().fg(DIFF_ADD_FG).bold(),
            ));
        }
        if !removed_text.is_empty() {
            if has_added {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                removed_text,
                Style::default().fg(DIFF_REMOVE_FG).bold(),
            ));
        }

        lines.push(Line::from(spans));
    }
}

fn append_sidebar_list(lines: &mut Vec<Line<'static>>, items: &[TodoItemView], max_items: usize) {
    if max_items == 0 {
        return;
    }
    if items.is_empty() {
        lines.push(Line::from(Span::styled(
            sidebar_prefixed("none"),
            Style::default().fg(TEXT_MUTED),
        )));
        return;
    }

    let shown = items.len().min(max_items);
    for item in items.iter().take(shown) {
        let (marker, item_style) = match item.status {
            TodoStatus::Pending | TodoStatus::InProgress => {
                ("[ ] ", Style::default().fg(TEXT_PRIMARY))
            }
            TodoStatus::Completed => ("[x] ", Style::default().fg(TEXT_MUTED)),
            TodoStatus::Cancelled => ("[-] ", Style::default().fg(TEXT_MUTED)),
        };

        lines.push(Line::from(vec![
            Span::styled(sidebar_prefixed(marker), Style::default().fg(INPUT_ACCENT)),
            Span::styled(item.content.clone(), item_style),
        ]));
    }

    if items.len() > shown {
        lines.push(Line::from(Span::styled(
            "...",
            Style::default().fg(TEXT_MUTED).italic(),
        )));
    }
}

fn render_edit_diff_block(
    lines: &mut Vec<Line<'static>>,
    tool_name: &str,
    output: &str,
    available_width: usize,
) -> bool {
    let parsed: EditToolOutput = match serde_json::from_str(output) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let child_indent = message_child_indent();

    lines.push(Line::from(vec![
        Span::raw(MESSAGE_INDENT),
        Span::styled("✓ ", Style::default().fg(INPUT_ACCENT).bold()),
        Span::styled(
            format!(
                "{} {}  +{} -{}",
                tool_title(tool_name),
                parsed.path,
                parsed.summary.added_lines,
                parsed.summary.removed_lines
            ),
            Style::default().fg(TEXT_SECONDARY),
        ),
    ]));

    let (left_width, right_width) = diff_column_widths(available_width);
    if left_width < MIN_DIFF_COLUMN_WIDTH || right_width < MIN_DIFF_COLUMN_WIDTH {
        return render_edit_diff_block_single_column(lines, &parsed.diff, available_width);
    }

    let mut rendered_chars = 0;
    let mut truncated = false;

    let mut raw_lines = parsed.diff.lines().peekable();
    let mut cursor = DiffLineCursor::default();
    let mut rendered_lines = 0;
    while let Some(side_by_side) = next_diff_row(&mut raw_lines, &mut cursor) {
        let line_chars = side_by_side.total_chars();
        if rendered_lines >= MAX_RENDERED_DIFF_LINES
            || rendered_chars + line_chars > MAX_RENDERED_DIFF_CHARS
        {
            truncated = true;
            break;
        }
        rendered_chars += line_chars;
        rendered_lines += 1;

        render_side_by_side_diff_row(lines, &side_by_side, left_width, right_width);
    }

    if truncated {
        lines.push(Line::from(vec![
            Span::raw(child_indent.clone()),
            Span::styled(
                "... diff truncated",
                Style::default().fg(TEXT_MUTED).italic(),
            ),
        ]));
    }

    true
}

fn render_edit_diff_block_single_column(
    lines: &mut Vec<Line<'static>>,
    diff: &str,
    available_width: usize,
) -> bool {
    let mut rendered_chars = 0;
    let mut truncated = false;
    let child_indent = message_child_indent();

    for (rendered_lines, raw_line) in diff.lines().enumerate() {
        let line_chars = raw_line.chars().count();
        if rendered_lines >= MAX_RENDERED_DIFF_LINES
            || rendered_chars + line_chars > MAX_RENDERED_DIFF_CHARS
        {
            truncated = true;
            break;
        }
        rendered_chars += line_chars;

        let shown = truncate_chars(raw_line, available_width);
        let style = if raw_line.starts_with('+') && !raw_line.starts_with("+++") {
            Style::default().fg(DIFF_ADD_FG).bg(DIFF_ADD_BG)
        } else if raw_line.starts_with('-') && !raw_line.starts_with("---") {
            Style::default().fg(DIFF_REMOVE_FG).bg(DIFF_REMOVE_BG)
        } else if raw_line.starts_with("@@")
            || raw_line.starts_with("---")
            || raw_line.starts_with("+++")
        {
            Style::default().fg(DIFF_META_FG)
        } else {
            Style::default().fg(TEXT_MUTED)
        };

        lines.push(Line::from(vec![
            Span::raw(child_indent.clone()),
            Span::styled(shown, style),
        ]));
    }

    if truncated {
        lines.push(Line::from(vec![
            Span::raw(child_indent.clone()),
            Span::styled(
                "... diff truncated",
                Style::default().fg(TEXT_MUTED).italic(),
            ),
        ]));
    }

    true
}

fn render_user_message_block(lines: &mut Vec<Line<'static>>, text: &str, width: usize) {
    let content_width = width.saturating_sub(USER_BUBBLE_INDENT + 1).max(1);
    let text_width = content_width
        .saturating_sub(USER_BUBBLE_INNER_PADDING * 2)
        .max(1);
    let wrapped = wrap_text(text, text_width);

    ensure_single_blank_line(lines);
    lines.push(build_user_bubble_line("", content_width));
    for line in wrapped {
        lines.push(build_user_bubble_line(&line, content_width));
    }
    lines.push(build_user_bubble_line("", content_width));
    lines.push(Line::from(""));
}

fn ensure_single_blank_line(lines: &mut Vec<Line<'static>>) {
    if lines.is_empty() {
        return;
    }
    if let Some(last) = lines.last()
        && line_is_empty(last)
    {
        return;
    }
    lines.push(Line::from(""));
}

fn line_is_empty(line: &Line<'_>) -> bool {
    line.spans.iter().all(|span| span.content.is_empty())
}

fn build_user_bubble_line(content: &str, content_width: usize) -> Line<'static> {
    let trimmed = truncate_chars(
        content,
        content_width.saturating_sub(USER_BUBBLE_INNER_PADDING * 2),
    );
    let leading = " ".repeat(USER_BUBBLE_INNER_PADDING);
    let trailing_len = content_width
        .saturating_sub(USER_BUBBLE_INNER_PADDING * 2)
        .saturating_sub(trimmed.chars().count());
    let trailing = " ".repeat(trailing_len + USER_BUBBLE_INNER_PADDING);

    Line::from(vec![
        Span::raw(" ".repeat(USER_BUBBLE_INDENT)),
        Span::styled("▌", Style::default().fg(ACCENT).bg(INPUT_PANEL_BG)),
        Span::styled(
            format!("{}{}{}", leading, trimmed, trailing),
            Style::default().fg(TEXT_PRIMARY).bg(INPUT_PANEL_BG),
        ),
    ])
}

fn append_tool_result_count(name: &str, label: &str, output: Option<&str>) -> String {
    let Some(raw_output) = output else {
        return label.to_string();
    };
    let Ok(value) = serde_json::from_str::<Value>(raw_output) else {
        return label.to_string();
    };
    let Some(count) = value.get("count").and_then(|v| v.as_u64()) else {
        return label.to_string();
    };

    match name {
        "list" => format!("{label} ({count} entries)"),
        "glob" => format!("{label} ({count} files)"),
        "grep" => format!("{label} ({count} matches)"),
        _ => label.to_string(),
    }
}

fn diff_column_widths(available_width: usize) -> (usize, usize) {
    let inner_width = available_width.saturating_sub(7);
    let left = inner_width / 2;
    let right = inner_width.saturating_sub(left);
    (left, right)
}

#[derive(Debug)]
struct SideBySideDiffRow {
    left: Option<DiffCell>,
    right: Option<DiffCell>,
    kind: SideBySideDiffKind,
}

impl SideBySideDiffRow {
    fn total_chars(&self) -> usize {
        self.left
            .as_ref()
            .map(|cell| cell.text.chars().count())
            .unwrap_or(0)
            + self
                .right
                .as_ref()
                .map(|cell| cell.text.chars().count())
                .unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
struct DiffCell {
    line_number: Option<usize>,
    marker: Option<char>,
    text: String,
}

#[derive(Debug, Default)]
struct DiffLineCursor {
    left_line: Option<usize>,
    right_line: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
enum SideBySideDiffKind {
    Context,
    Removed,
    Added,
    Meta,
    Changed,
}

fn next_diff_row<'a>(
    lines: &mut Peekable<impl Iterator<Item = &'a str>>,
    cursor: &mut DiffLineCursor,
) -> Option<SideBySideDiffRow> {
    let raw = lines.next()?;

    if raw.starts_with("@@") || raw.starts_with("---") || raw.starts_with("+++") {
        if let Some((left, right)) = parse_hunk_line_numbers(raw) {
            cursor.left_line = Some(left);
            cursor.right_line = Some(right);
        }

        return Some(SideBySideDiffRow {
            left: Some(DiffCell {
                line_number: None,
                marker: None,
                text: raw.to_string(),
            }),
            right: Some(DiffCell {
                line_number: None,
                marker: None,
                text: raw.to_string(),
            }),
            kind: SideBySideDiffKind::Meta,
        });
    }

    if let Some(context_text) = raw.strip_prefix(' ') {
        return Some(SideBySideDiffRow {
            left: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.left_line),
                marker: None,
                text: context_text.to_string(),
            }),
            right: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.right_line),
                marker: None,
                text: context_text.to_string(),
            }),
            kind: SideBySideDiffKind::Context,
        });
    }

    if raw.starts_with('-') && !raw.starts_with("---") {
        if let Some(next) = lines.peek()
            && next.starts_with('+')
            && !next.starts_with("+++")
        {
            let added = lines.next().unwrap_or_default().to_string();
            let removed_text = raw.strip_prefix('-').unwrap_or(raw);
            let added_text = added.strip_prefix('+').unwrap_or(&added);
            return Some(SideBySideDiffRow {
                left: Some(DiffCell {
                    line_number: take_next_line_number(&mut cursor.left_line),
                    marker: Some('-'),
                    text: removed_text.to_string(),
                }),
                right: Some(DiffCell {
                    line_number: take_next_line_number(&mut cursor.right_line),
                    marker: Some('+'),
                    text: added_text.to_string(),
                }),
                kind: SideBySideDiffKind::Changed,
            });
        }

        let removed_text = raw.strip_prefix('-').unwrap_or(raw);

        return Some(SideBySideDiffRow {
            left: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.left_line),
                marker: Some('-'),
                text: removed_text.to_string(),
            }),
            right: None,
            kind: SideBySideDiffKind::Removed,
        });
    }

    if raw.starts_with('+') && !raw.starts_with("+++") {
        let added_text = raw.strip_prefix('+').unwrap_or(raw);
        return Some(SideBySideDiffRow {
            left: None,
            right: Some(DiffCell {
                line_number: take_next_line_number(&mut cursor.right_line),
                marker: Some('+'),
                text: added_text.to_string(),
            }),
            kind: SideBySideDiffKind::Added,
        });
    }

    Some(SideBySideDiffRow {
        left: Some(DiffCell {
            line_number: None,
            marker: None,
            text: raw.to_string(),
        }),
        right: Some(DiffCell {
            line_number: None,
            marker: None,
            text: raw.to_string(),
        }),
        kind: SideBySideDiffKind::Context,
    })
}

fn parse_hunk_line_numbers(raw: &str) -> Option<(usize, usize)> {
    if !raw.starts_with("@@") {
        return None;
    }

    let mut parts = raw.split_whitespace();
    let _ = parts.next()?;
    let left = parts.next()?;
    let right = parts.next()?;

    let left_start = left
        .strip_prefix('-')?
        .split(',')
        .next()?
        .parse::<usize>()
        .ok()?;
    let right_start = right
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse::<usize>()
        .ok()?;

    Some((left_start, right_start))
}

fn take_next_line_number(line_number: &mut Option<usize>) -> Option<usize> {
    match line_number {
        Some(current) => {
            let value = *current;
            *current = current.saturating_add(1);
            Some(value)
        }
        None => None,
    }
}

fn render_side_by_side_diff_row(
    lines: &mut Vec<Line<'static>>,
    row: &SideBySideDiffRow,
    left_width: usize,
    right_width: usize,
) {
    let left_text = render_diff_cell(row.left.as_ref(), left_width);
    let right_text = render_diff_cell(row.right.as_ref(), right_width);

    let (left_style, right_style) = match row.kind {
        SideBySideDiffKind::Context => (
            Style::default().fg(TEXT_MUTED),
            Style::default().fg(TEXT_MUTED),
        ),
        SideBySideDiffKind::Removed => (
            Style::default().fg(DIFF_REMOVE_FG).bg(DIFF_REMOVE_BG),
            Style::default().fg(TEXT_MUTED),
        ),
        SideBySideDiffKind::Added => (
            Style::default().fg(TEXT_MUTED),
            Style::default().fg(DIFF_ADD_FG).bg(DIFF_ADD_BG),
        ),
        SideBySideDiffKind::Meta => (
            Style::default().fg(DIFF_META_FG),
            Style::default().fg(DIFF_META_FG),
        ),
        SideBySideDiffKind::Changed => (
            Style::default().fg(DIFF_REMOVE_FG).bg(DIFF_REMOVE_BG),
            Style::default().fg(DIFF_ADD_FG).bg(DIFF_ADD_BG),
        ),
    };

    lines.push(Line::from(vec![
        Span::raw(message_child_indent()),
        Span::styled(left_text, left_style),
        Span::styled(" | ", Style::default().fg(DIFF_META_FG)),
        Span::styled(right_text, right_style),
    ]));
}

fn pad_for_column(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let shown = truncate_for_column(text, width);
    let shown_len = shown.chars().count();
    if shown_len >= width {
        shown
    } else {
        format!("{shown}{}", " ".repeat(width - shown_len))
    }
}

fn render_diff_cell(cell: Option<&DiffCell>, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let Some(cell) = cell else {
        return " ".repeat(width);
    };

    if cell.marker.is_none() && cell.line_number.is_none() {
        return pad_for_column(&cell.text, width);
    }

    let line_number = match cell.line_number {
        Some(n) => format!("{n:>width$}", width = DIFF_LINE_NUMBER_WIDTH),
        None => " ".repeat(DIFF_LINE_NUMBER_WIDTH),
    };
    let marker = cell.marker.unwrap_or(' ');
    let prefix = format!("{line_number} {marker} ");
    let prefix_width = prefix.chars().count();

    let combined = if width <= prefix_width {
        truncate_for_column(&prefix, width)
    } else {
        let content = truncate_for_column(&cell.text, width - prefix_width);
        format!("{prefix}{content}")
    };

    pad_for_column(&combined, width)
}

fn truncate_for_column(input: &str, max_chars: usize) -> String {
    truncate_chars_impl(input, max_chars, TruncationMode::FixedWidth)
}

fn tool_title(name: &str) -> &'static str {
    match name {
        "edit" => "Edit",
        "write" => "Write",
        _ => "Tool",
    }
}

fn sidebar_prefixed(text: &str) -> String {
    format!("{SIDEBAR_INDENT}{text}")
}

fn sidebar_label(text: &str) -> String {
    format!("{SIDEBAR_LABEL_INDENT}{text}")
}

fn message_child_indent() -> String {
    " ".repeat(MESSAGE_INDENT.chars().count() + 2)
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    truncate_chars_impl(input, max_chars, TruncationMode::AppendEllipsis)
}

#[derive(Clone, Copy)]
enum TruncationMode {
    FixedWidth,
    AppendEllipsis,
}

fn truncate_chars_impl(input: &str, max_chars: usize, mode: TruncationMode) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let mut chars = input.chars();
    let taken: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_none() {
        return taken;
    }

    match mode {
        TruncationMode::FixedWidth => {
            if max_chars <= 3 {
                ".".repeat(max_chars)
            } else {
                let visible: String = taken.chars().take(max_chars - 3).collect();
                format!("{visible}...")
            }
        }
        TruncationMode::AppendEllipsis => format!("{taken}..."),
    }
}
