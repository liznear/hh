use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Stylize,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use serde::Deserialize;
use serde_json::Value;
use std::iter::Peekable;

use super::app::{ChatApp, ChatMessage, TodoItemView, TodoStatus};
use super::markdown::markdown_to_lines;
use super::tool_presentation::render_tool_start;

const SIDEBAR_WIDTH: u16 = 38;
const LEFT_COLUMN_RIGHT_MARGIN: u16 = 2;
const MAIN_OUTER_PADDING_X: u16 = 1;
const MAIN_OUTER_PADDING_Y: u16 = 1;
const MAX_TOOL_OUTPUT_LEN: usize = 200;
const USER_BUBBLE_INDENT: usize = 2;
const USER_BUBBLE_INNER_PADDING: usize = 1;
const MIN_DIFF_COLUMN_WIDTH: usize = 14;

const PAGE_BG: Color = Color::Rgb(246, 247, 251);
const PANEL_BG: Color = Color::Rgb(255, 255, 255);
const SIDEBAR_BG: Color = Color::Rgb(234, 238, 246);
const INPUT_PANEL_BG: Color = Color::Rgb(229, 233, 241);
const TEXT_PRIMARY: Color = Color::Rgb(37, 45, 58);
const TEXT_SECONDARY: Color = Color::Rgb(98, 108, 124);
const TEXT_MUTED: Color = Color::Rgb(125, 133, 147);
const ACCENT: Color = Color::Rgb(55, 114, 255);
const INPUT_ACCENT: Color = Color::Rgb(19, 164, 151);
const PROGRESS_TRACK: Color = Color::Rgb(203, 182, 248);
const PROGRESS_TRAIL: Color = Color::Rgb(162, 120, 238);
const PROGRESS_HEAD: Color = Color::Rgb(124, 72, 227);
const THINKING_LABEL: Color = Color::Rgb(227, 152, 67);
const DIFF_ADD_FG: Color = Color::Rgb(25, 110, 61);
const DIFF_ADD_BG: Color = Color::Rgb(226, 244, 235);
const DIFF_REMOVE_FG: Color = Color::Rgb(152, 45, 45);
const DIFF_REMOVE_BG: Color = Color::Rgb(252, 235, 235);
const DIFF_META_FG: Color = Color::Rgb(106, 114, 128);
const MAX_RENDERED_DIFF_LINES: usize = 120;
const MAX_RENDERED_DIFF_CHARS: usize = 8_000;

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

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1), // Space above progress
            Constraint::Length(1), // Global processing indicator
            Constraint::Length(1), // Space above input
            Constraint::Length(3), // Input area
        ])
        .split(main_area);

    render_messages(f, app, main_chunks[0]);
    render_processing_indicator(f, app, main_chunks[2]);
    render_input(f, app, main_chunks[4]);

    if !app.filtered_commands.is_empty() {
        let item_count = app.filtered_commands.len().min(5) as u16;
        let popup_height = item_count + 2;
        let popup_area = Rect {
            x: main_chunks[4].x + 1,
            y: main_chunks[4].y.saturating_sub(popup_height),
            width: 60,
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
}

fn render_command_palette(f: &mut Frame, app: &ChatApp, area: Rect) {
    f.render_widget(Clear, area);

    let items: Vec<ListItem> = app
        .filtered_commands
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, cmd)| {
            let style = if i == app.selected_command_index {
                Style::default().fg(Color::White).bg(ACCENT)
            } else {
                Style::default().fg(TEXT_PRIMARY).bg(PANEL_BG)
            };

            ListItem::new(Line::from(vec![
                Span::styled(format!("{:<12}", cmd.name), Style::default().bold()),
                Span::raw(" "),
                Span::styled(
                    cmd.description.clone(),
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

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().bg(PANEL_BG)),
    );

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

    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            "  / / / / / / / /",
            Style::default().fg(ACCENT),
        )),
        Line::from(Span::styled(
            "  HH",
            Style::default().fg(INPUT_ACCENT).bold(),
        )),
        Line::from(Span::styled("  H H", Style::default().fg(ACCENT).bold())),
        Line::from(Span::styled(
            "  HHH",
            Style::default().fg(INPUT_ACCENT).bold(),
        )),
        Line::from(Span::styled(
            "  / / / / / / / /",
            Style::default().fg(ACCENT),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Session", Style::default().fg(TEXT_SECONDARY).bold()),
            Span::raw(": "),
            Span::styled(app.session_name.clone(), Style::default().fg(TEXT_PRIMARY)),
        ]),
        Line::from(vec![
            Span::styled(" Directory", Style::default().fg(TEXT_SECONDARY).bold()),
            Span::raw(": "),
            Span::styled(
                abbreviate_path(
                    &app.working_directory,
                    content.width.saturating_sub(14) as usize,
                ),
                Style::default().fg(TEXT_PRIMARY),
            ),
        ]),
        Line::from(vec![
            Span::styled(" Context", Style::default().fg(TEXT_SECONDARY).bold()),
            Span::raw(": "),
            Span::styled(
                format!("{} / {} ({}%)", used, budget, context_percent),
                Style::default().fg(TEXT_PRIMARY),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " TODO",
            Style::default().fg(TEXT_SECONDARY).bold(),
        )),
    ];

    if !app.todo_items.is_empty() {
        let done = app
            .todo_items
            .iter()
            .filter(|item| item.status == TodoStatus::Completed)
            .count();
        lines.push(Line::from(Span::styled(
            format!(" {} / {} done", done, app.todo_items.len()),
            Style::default().fg(TEXT_MUTED),
        )));
    }

    let list_max = content.height.saturating_sub(lines.len() as u16 + 1) as usize;
    append_sidebar_list(&mut lines, &app.todo_items, list_max);

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

    let text = Text::from(lines.to_vec());
    let paragraph = Paragraph::new(text)
        .style(Style::default().bg(PAGE_BG).fg(TEXT_PRIMARY))
        .scroll((scroll_offset as u16, 0));

    f.render_widget(paragraph, content);
}

/// Build message lines (used for caching in ChatApp)
pub fn build_message_lines_internal(app: &ChatApp, width: usize) -> Vec<Line<'static>> {
    build_message_lines_impl(app, width)
}

/// Public function for external callers (e.g., calculating scroll bounds)
pub fn build_message_lines(app: &ChatApp, width: usize) -> Vec<Line<'static>> {
    build_message_lines_impl(app, width)
}

fn build_message_lines_impl(app: &ChatApp, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for msg in &app.messages {
        match msg {
            ChatMessage::User(text) => {
                render_user_message_block(&mut lines, text, width);
            }
            ChatMessage::Assistant(text) => {
                for line in parse_markdown_lines(text, width) {
                    lines.push(line);
                }
            }
            ChatMessage::Thinking(text) => {
                ensure_single_blank_line(&mut lines);
                let available_width = width.saturating_sub(4).max(1);
                let wrapped = wrap_text(text, available_width);
                for (i, line) in wrapped.iter().enumerate() {
                    if i == 0 {
                        lines.push(Line::from(vec![
                            Span::styled(
                                "  Thinking: ",
                                Style::default().fg(THINKING_LABEL).italic(),
                            ),
                            Span::styled(line.clone(), Style::default().fg(THINKING_LABEL).bold()),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::raw("            "), // "  Thinking: " is 12 chars
                            Span::styled(line.clone(), Style::default().fg(THINKING_LABEL).bold()),
                        ]));
                    }
                }
            }
            ChatMessage::ToolCall {
                name,
                args,
                output,
                is_error,
            } => {
                let available_width = width.saturating_sub(4).max(1);

                // Parse args to Value for rendering
                let args_value: Value = serde_json::from_str(args).unwrap_or(Value::Null);
                let tool_view = render_tool_start(name, &args_value);
                let label = tool_view.line;

                if let Some(error) = is_error {
                    if !*error
                        && (name == "edit" || name == "write")
                        && let Some(tool_output) = output.as_deref()
                        && render_edit_diff_block(&mut lines, name, tool_output, available_width)
                    {
                        continue;
                    }

                    let completed_label = if !*error {
                        append_tool_result_count(name, &label, output.as_deref())
                    } else {
                        label.clone()
                    };
                    let symbol = if *error { "x" } else { "✓" };
                    let color = if *error { Color::Red } else { INPUT_ACCENT };
                    let wrapped = wrap_compact_text(&completed_label, available_width);
                    for (i, line) in wrapped.iter().enumerate() {
                        if i == 0 {
                            lines.push(Line::from(vec![
                                Span::raw("  "),
                                Span::styled(symbol, Style::default().fg(color).bold()),
                                Span::raw(" "),
                                Span::styled(line.clone(), Style::default().fg(TEXT_SECONDARY)),
                            ]));
                        } else {
                            lines.push(Line::from(vec![
                                Span::raw("    "), // Indent 4
                                Span::styled(line.clone(), Style::default().fg(TEXT_SECONDARY)),
                            ]));
                        }
                    }
                } else {
                    let wrapped = wrap_compact_text(&label, available_width.saturating_sub(1)); // "->" is 2 chars + spaces
                    for (i, line) in wrapped.iter().enumerate() {
                        if i == 0 {
                            lines.push(Line::from(vec![
                                Span::styled("  -> ", Style::default().fg(TEXT_MUTED)),
                                Span::styled(line.clone(), Style::default().fg(TEXT_SECONDARY)),
                            ]));
                        } else {
                            lines.push(Line::from(vec![
                                Span::raw("     "), // "  -> " is 5 chars
                                Span::styled(line.clone(), Style::default().fg(TEXT_SECONDARY)),
                            ]));
                        }
                    }
                }
            }
            ChatMessage::Error(text) => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
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
    markdown_to_lines(text, width)
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

    let input_value = if app.input.is_empty() {
        "Tell me more about this project...".to_string()
    } else {
        app.input.clone()
    };

    let content_y = area
        .y
        .saturating_add(1)
        .min(area.bottom().saturating_sub(1));
    let content_x = left_border_x.saturating_add(2);
    let content_area = Rect {
        x: content_x,
        y: content_y,
        width: area
            .width
            .saturating_sub(content_x.saturating_sub(area.x) + 1),
        height: 1,
    };

    f.render_widget(
        Paragraph::new(input_value).style(Style::default().fg(TEXT_PRIMARY).bg(INPUT_PANEL_BG)),
        content_area,
    );

    let cursor_x = app.input.chars().count() as u16;
    if cursor_x < content_area.width {
        f.set_cursor_position((content_area.x + cursor_x, content_area.y));
    }
}

fn render_processing_indicator(f: &mut Frame, app: &ChatApp, area: Rect) {
    if !app.is_processing {
        return;
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::raw("  "));

    let bar_len = area.width.saturating_sub(22).clamp(6, 10) as usize;
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

    spans.push(Span::raw("  "));
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

fn append_sidebar_list(lines: &mut Vec<Line<'static>>, items: &[TodoItemView], max_items: usize) {
    if max_items == 0 {
        return;
    }
    if items.is_empty() {
        lines.push(Line::from(Span::styled(
            "  none",
            Style::default().fg(TEXT_MUTED),
        )));
        return;
    }

    let shown = items.len().min(max_items);
    for item in items.iter().take(shown) {
        let (prefix, item_style) = match item.status {
            TodoStatus::Pending | TodoStatus::InProgress => {
                ("  [ ] ", Style::default().fg(TEXT_PRIMARY))
            }
            TodoStatus::Completed => ("  [x] ", Style::default().fg(TEXT_MUTED)),
            TodoStatus::Cancelled => ("  [-] ", Style::default().fg(TEXT_MUTED)),
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(INPUT_ACCENT)),
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

    lines.push(Line::from(vec![
        Span::raw("  "),
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
    let mut rendered_lines = 0;
    while let Some(side_by_side) = next_diff_row(&mut raw_lines) {
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
            Span::raw("    "),
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
            Span::raw("    "),
            Span::styled(shown, style),
        ]));
    }

    if truncated {
        lines.push(Line::from(vec![
            Span::raw("    "),
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
    left: Option<String>,
    right: Option<String>,
    kind: SideBySideDiffKind,
}

impl SideBySideDiffRow {
    fn total_chars(&self) -> usize {
        self.left.as_ref().map(|s| s.chars().count()).unwrap_or(0)
            + self.right.as_ref().map(|s| s.chars().count()).unwrap_or(0)
    }
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
) -> Option<SideBySideDiffRow> {
    let raw = lines.next()?;

    if raw.starts_with("@@") || raw.starts_with("---") || raw.starts_with("+++") {
        return Some(SideBySideDiffRow {
            left: Some(raw.to_string()),
            right: Some(raw.to_string()),
            kind: SideBySideDiffKind::Meta,
        });
    }

    if raw.starts_with('-') {
        if let Some(next) = lines.peek()
            && next.starts_with('+')
        {
            let added = lines.next().unwrap_or_default().to_string();
            return Some(SideBySideDiffRow {
                left: Some(raw.to_string()),
                right: Some(added),
                kind: SideBySideDiffKind::Changed,
            });
        }

        return Some(SideBySideDiffRow {
            left: Some(raw.to_string()),
            right: None,
            kind: SideBySideDiffKind::Removed,
        });
    }

    if raw.starts_with('+') {
        return Some(SideBySideDiffRow {
            left: None,
            right: Some(raw.to_string()),
            kind: SideBySideDiffKind::Added,
        });
    }

    Some(SideBySideDiffRow {
        left: Some(raw.to_string()),
        right: Some(raw.to_string()),
        kind: SideBySideDiffKind::Context,
    })
}

fn render_side_by_side_diff_row(
    lines: &mut Vec<Line<'static>>,
    row: &SideBySideDiffRow,
    left_width: usize,
    right_width: usize,
) {
    let left_raw = row.left.as_deref().unwrap_or("");
    let right_raw = row.right.as_deref().unwrap_or("");
    let left_text = pad_for_column(left_raw, left_width);
    let right_text = pad_for_column(right_raw, right_width);

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
        Span::raw("    "),
        Span::styled(left_text, left_style),
        Span::styled(" | ", Style::default().fg(DIFF_META_FG)),
        Span::styled(right_text, right_style),
    ]));
}

fn pad_for_column(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let shown = truncate_chars(text, width);
    let shown_len = shown.chars().count();
    if shown_len >= width {
        shown
    } else {
        format!("{shown}{}", " ".repeat(width - shown_len))
    }
}

fn tool_title(name: &str) -> &'static str {
    match name {
        "edit" => "Edit",
        "write" => "Write",
        _ => "Tool",
    }
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let mut chars = input.chars();
    let taken: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", taken)
    } else {
        taken
    }
}
