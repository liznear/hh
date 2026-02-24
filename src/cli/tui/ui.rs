use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Stylize,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use serde_json::Value;

use super::app::{ChatApp, ChatMessage};
use super::tool_presentation::render_tool_start;

const MAX_TOOL_OUTPUT_LEN: usize = 200;
const SIDEBAR_WIDTH: u16 = 38;
const MAIN_OUTER_PADDING_X: u16 = 1;
const MAIN_OUTER_PADDING_Y: u16 = 1;
const CONTENT_HORIZONTAL_PADDING: u16 = 2;
const USER_MESSAGE_PADDING: usize = 2;

const PAGE_BG: Color = Color::Rgb(246, 247, 251);
const PANEL_BG: Color = Color::Rgb(255, 255, 255);
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

pub fn render_app(f: &mut Frame, app: &ChatApp) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(40), Constraint::Length(SIDEBAR_WIDTH)])
        .split(f.area());

    let main_area = inset_rect(columns[0], MAIN_OUTER_PADDING_X, MAIN_OUTER_PADDING_Y);
    let sidebar_area = if columns.len() > 1 {
        Some(columns[1])
    } else {
        None
    };

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1), // Status bar
            Constraint::Length(3), // Input area
            Constraint::Length(1), // Global processing indicator
        ])
        .split(main_area);

    f.render_widget(
        Block::default().style(Style::default().bg(PAGE_BG)),
        columns[0],
    );

    render_messages(f, app, main_chunks[0]);
    render_status(f, app, main_chunks[1]);
    render_input(f, app, main_chunks[2]);

    if !app.filtered_commands.is_empty() {
        let item_count = app.filtered_commands.len().min(5) as u16;
        let popup_height = item_count + 2;
        let popup_area = Rect {
            x: main_chunks[2].x + 1,
            y: main_chunks[2].y.saturating_sub(popup_height),
            width: 60,
            height: popup_height,
        };
        render_command_palette(f, app, popup_area);
    }

    render_processing_indicator(f, app, main_chunks[3]);

    if let Some(area) = sidebar_area {
        render_sidebar(f, app, area);
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
    let block = Block::default()
        .borders(Borders::LEFT)
        .style(Style::default().bg(PANEL_BG));
    let inner = block.inner(area);
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
                    inner.width.saturating_sub(14) as usize,
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

    let list_max = inner.height.saturating_sub(lines.len() as u16 + 1) as usize;
    append_sidebar_list(&mut lines, &app.todo_items, list_max);

    let sidebar = Paragraph::new(Text::from(lines))
        .style(Style::default().bg(PANEL_BG))
        .wrap(Wrap { trim: true });
    f.render_widget(sidebar, inner);
}

fn render_messages(f: &mut Frame, app: &ChatApp, area: ratatui::layout::Rect) {
    let panel = Block::default().style(Style::default().bg(PAGE_BG));
    let inner = panel.inner(area);
    f.render_widget(panel, area);

    let content = Rect {
        x: inner.x.saturating_add(CONTENT_HORIZONTAL_PADDING),
        y: inner.y,
        width: inner
            .width
            .saturating_sub(CONTENT_HORIZONTAL_PADDING.saturating_mul(2)),
        height: inner.height,
    };

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
                if !lines.is_empty() {
                    lines.push(Line::from(""));
                }

                let bubble_width = width.saturating_sub(USER_MESSAGE_PADDING * 2);
                let wrapped = wrap_text(text, bubble_width.max(1));
                for line in wrapped {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled("▌", Style::default().fg(ACCENT).bg(INPUT_PANEL_BG)),
                        Span::styled(
                            format!(" {}", line),
                            Style::default().fg(TEXT_PRIMARY).bg(INPUT_PANEL_BG),
                        ),
                    ]));
                }
            }
            ChatMessage::Assistant(text) => {
                lines.push(Line::from(""));
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("Assistant", Style::default().fg(TEXT_SECONDARY).bold()),
                ]));
                for line in parse_markdown_lines(text, width) {
                    lines.push(line);
                }
            }
            ChatMessage::Thinking(text) => {
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
                output: _,
                is_error,
            } => {
                let available_width = width.saturating_sub(4).max(1);

                // Parse args to Value for rendering
                let args_value: Value = serde_json::from_str(args).unwrap_or(Value::Null);
                let tool_view = render_tool_start(name, &args_value);
                let label = tool_view.line;

                if let Some(error) = is_error {
                    let symbol = if *error { "x" } else { "✓" };
                    let color = if *error { Color::Red } else { INPUT_ACCENT };
                    let wrapped = wrap_text(&label, available_width);
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
                    let wrapped = wrap_text(&label, available_width.saturating_sub(1)); // "->" is 2 chars + spaces
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
    let mut lines = Vec::new();

    for line in text.lines() {
        let spans = parse_markdown_spans(line);
        let wrapped = wrap_spans(&spans, width.saturating_sub(2));
        for wrapped_line in wrapped {
            let mut indented = vec![Span::raw("  ")];
            indented.extend(wrapped_line);
            lines.push(Line::from(indented));
        }
    }

    lines
}

/// Parse inline markdown (bold, code) into spans
fn parse_markdown_spans(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chars = text.chars().peekable();
    let mut current = String::new();

    while let Some(ch) = chars.next() {
        if ch == '*' && chars.peek() == Some(&'*') {
            // Bold text **...**
            chars.next(); // consume second *
            if !current.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut current)));
            }
            // Find closing **
            let mut bold_text = String::new();
            loop {
                match chars.next() {
                    Some('*') if chars.peek() == Some(&'*') => {
                        chars.next(); // consume second *
                        break;
                    }
                    Some(c) => bold_text.push(c),
                    None => {
                        // Unclosed bold, treat as literal
                        bold_text.insert(0, '*');
                        bold_text.insert(0, '*');
                        spans.push(Span::raw(bold_text));
                        return spans;
                    }
                }
            }
            spans.push(Span::styled(bold_text, Style::default().bold()));
        } else if ch == '`' {
            // Inline code `...`
            if !current.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut current)));
            }
            let mut code_text = String::new();
            loop {
                match chars.next() {
                    Some('`') => break,
                    Some(c) => code_text.push(c),
                    None => {
                        // Unclosed code, treat as literal
                        code_text.insert(0, '`');
                        spans.push(Span::raw(code_text));
                        return spans;
                    }
                }
            }
            spans.push(Span::styled(code_text, Style::default().fg(Color::Yellow)));
        } else {
            current.push(ch);
        }
    }

    if !current.is_empty() {
        spans.push(Span::raw(current));
    }

    spans
}

/// Wrap spans to fit within a given width
fn wrap_spans(spans: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    if width == 0 {
        return vec![spans.to_vec()];
    }

    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current_line: Vec<Span<'static>> = Vec::new();
    let mut current_line_len = 0;

    for span in spans {
        let span_style = span.style;
        let span_text = span.content.as_ref();

        for word in span_text.split_whitespace() {
            let word_len = word.chars().count();

            // Check if we need to start a new line
            let space_needed = if current_line_len > 0 { 1 } else { 0 };

            if current_line_len + space_needed + word_len > width && !current_line.is_empty() {
                lines.push(std::mem::take(&mut current_line));
                current_line_len = 0;
            }

            // Add space before word if not first word in line
            if current_line_len > 0 {
                current_line.push(Span::raw(" "));
                current_line_len += 1;
            }

            current_line.push(Span::styled(word.to_string(), span_style));
            current_line_len += word_len;
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(vec![]);
    }

    lines
}

/// Wrap text to a given width, returning a vector of lines.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }

    // Truncate very long outputs first
    let text = if text.len() > MAX_TOOL_OUTPUT_LEN {
        format!("{}...", &text[..MAX_TOOL_OUTPUT_LEN])
    } else {
        text.to_string()
    };

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

fn render_status(f: &mut Frame, app: &ChatApp, area: Rect) {
    let processing_text = if app.is_processing {
        "processing"
    } else {
        "ready"
    };
    let status = format!("{} | :quit | Ctrl+C", processing_text);

    let paragraph = Paragraph::new(status).style(Style::default().fg(TEXT_MUTED).bg(PAGE_BG));
    f.render_widget(paragraph, area);
}

fn render_input(f: &mut Frame, app: &ChatApp, area: Rect) {
    f.render_widget(
        Block::default().style(Style::default().bg(INPUT_PANEL_BG)),
        area,
    );

    let left_accent = Rect {
        x: area.x,
        y: area.y,
        width: 1,
        height: area.height,
    };
    f.render_widget(
        Paragraph::new(" ").style(Style::default().bg(PROGRESS_HEAD)),
        left_accent,
    );

    let input_value = if app.input.is_empty() {
        "Tell me more about this project...".to_string()
    } else {
        app.input.clone()
    };

    let content_y = area
        .y
        .saturating_add(1)
        .min(area.bottom().saturating_sub(1));
    let content_area = Rect {
        x: area.x.saturating_add(2),
        y: content_y,
        width: area.width.saturating_sub(3),
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
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::raw(" "));

    let bar_len = area.width.saturating_sub(20).clamp(8, 18) as usize;
    let head = app.processing_step(85) % bar_len;

    for idx in 0..bar_len {
        let style = if app.is_processing {
            if idx == head {
                Style::default().fg(PROGRESS_HEAD)
            } else if idx == (head + bar_len - 1) % bar_len || idx == (head + bar_len - 2) % bar_len
            {
                Style::default().fg(PROGRESS_TRAIL)
            } else {
                Style::default().fg(PROGRESS_TRACK)
            }
        } else {
            Style::default().fg(TEXT_MUTED)
        };
        spans.push(Span::styled("█", style));
    }

    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        if app.is_processing {
            "esc interrupt"
        } else {
            "ready"
        },
        Style::default().fg(TEXT_MUTED),
    ));

    let paragraph = Paragraph::new(Line::from(spans)).style(Style::default().bg(PAGE_BG));
    f.render_widget(paragraph, area);
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

fn append_sidebar_list(lines: &mut Vec<Line<'static>>, items: &[String], max_items: usize) {
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
        lines.push(Line::from(vec![
            Span::styled("  - ", Style::default().fg(INPUT_ACCENT)),
            Span::styled(item.clone(), Style::default().fg(TEXT_PRIMARY)),
        ]));
    }

    if items.len() > shown {
        lines.push(Line::from(Span::styled(
            "...",
            Style::default().fg(TEXT_MUTED).italic(),
        )));
    }
}
