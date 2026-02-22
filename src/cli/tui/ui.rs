use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::Stylize,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::app::{ChatApp, ChatMessage};

const MAX_TOOL_OUTPUT_LEN: usize = 200;

pub fn render_app(f: &mut Frame, app: &ChatApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // Messages area
            Constraint::Length(1), // Status bar
            Constraint::Length(3), // Input area
        ])
        .split(f.area());

    // Render messages
    render_messages(f, app, chunks[0]);

    // Render status bar
    render_status(f, app, chunks[1]);

    // Render input
    render_input(f, app, chunks[2]);
}

fn render_messages(f: &mut Frame, app: &ChatApp, area: ratatui::layout::Rect) {
    // Use actual area width for wrapping (account for border)
    let wrap_width = area.width.saturating_sub(2) as usize;
    let lines = build_message_lines(app, wrap_width);
    let total_lines = lines.len();

    let visible_height = area.height.saturating_sub(2) as usize; // Account for borders

    // Calculate scroll offset: auto-scroll to bottom if enabled, otherwise use manual offset
    let scroll_offset = if app.auto_scroll {
        total_lines.saturating_sub(visible_height)
    } else if app.scroll_offset + visible_height > total_lines {
        total_lines.saturating_sub(visible_height)
    } else {
        app.scroll_offset
    };

    let text = Text::from(lines);
    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::TOP).title("Messages"))
        .scroll((scroll_offset as u16, 0));

    f.render_widget(paragraph, area);
}

fn build_message_lines(app: &ChatApp, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for msg in &app.messages {
        match msg {
            ChatMessage::User(text) => {
                // First line with prefix
                lines.push(Line::from(vec![
                    Span::styled("you: ", Style::default().fg(Color::Cyan).bold()),
                ]));
                // Wrapped content lines
                let wrapped = wrap_text(text, width);
                for line in wrapped {
                    lines.push(Line::from(Span::raw(line)));
                }
            }
            ChatMessage::Assistant(text) => {
                // First line with prefix
                lines.push(Line::from(vec![
                    Span::styled(
                        "assistant: ",
                        Style::default().fg(Color::Green).bold(),
                    ),
                ]));
                // Parse markdown and wrap
                for line in parse_markdown_lines(text, width) {
                    lines.push(line);
                }
            }
            ChatMessage::Thinking(text) => {
                let label = if app.thinking_expanded {
                    "thinking: "
                } else {
                    "thinking… "
                };
                lines.push(Line::from(vec![
                    Span::styled(label, Style::default().fg(Color::Yellow).italic()),
                    Span::styled(text.clone(), Style::default().fg(Color::Yellow)),
                ]));
            }
            ChatMessage::ToolStart { name, args } => {
                // Header line with name
                lines.push(Line::from(vec![
                    Span::styled("tool:", Style::default().fg(Color::Magenta)),
                    Span::styled(name.clone(), Style::default().fg(Color::Magenta).bold()),
                    Span::raw("> start"),
                ]));
                // Wrapped args on following lines with indentation
                if !args.is_empty() {
                    let wrapped = wrap_text(args, width.saturating_sub(4));
                    for line in wrapped {
                        lines.push(Line::from(vec![Span::styled(
                            format!("    {}", line),
                            Style::default().fg(Color::DarkGray),
                        )]));
                    }
                }
            }
            ChatMessage::ToolEnd { name, is_error, output } => {
                let status_color = if *is_error {
                    Color::Red
                } else {
                    Color::Green
                };
                let status = if *is_error { "error" } else { "ok" };
                // Header line with name and status
                lines.push(Line::from(vec![
                    Span::styled("tool:", Style::default().fg(Color::Magenta)),
                    Span::styled(name.clone(), Style::default().fg(Color::Magenta).bold()),
                    Span::raw("> "),
                    Span::styled(status, Style::default().fg(status_color).bold()),
                ]));
                // Wrapped output on following lines with indentation
                if !output.is_empty() {
                    let wrapped = wrap_text(output, width.saturating_sub(4));
                    let display_lines: Vec<_> = wrapped.into_iter().take(15).collect();
                    for line in display_lines {
                        lines.push(Line::from(vec![Span::styled(
                            format!("    {}", line),
                            Style::default().fg(Color::DarkGray),
                        )]));
                    }
                    if output.len() > MAX_TOOL_OUTPUT_LEN {
                        lines.push(Line::from(vec![Span::styled(
                            "    [... truncated ...]",
                            Style::default().fg(Color::DarkGray).italic(),
                        )]));
                    }
                }
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
        // Wrap the spans to fit width
        let wrapped = wrap_spans(&spans, width);
        for wrapped_line in wrapped {
            lines.push(Line::from(wrapped_line));
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
            spans.push(Span::styled(
                code_text,
                Style::default().fg(Color::Yellow),
            ));
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

fn render_status(f: &mut Frame, app: &ChatApp, area: ratatui::layout::Rect) {
    let thinking_text = if app.thinking_expanded {
        "Thinking: Expanded"
    } else {
        "Thinking: Collapsed"
    };

    let processing_text = if app.is_processing {
        " | Processing…"
    } else {
        ""
    };

    let status = format!(
        "{} | :thinking to toggle | :quit to exit{} | Ctrl+C",
        thinking_text, processing_text
    );

    let paragraph = Paragraph::new(status).style(Style::default().fg(Color::DarkGray));
    f.render_widget(paragraph, area);
}

fn render_input(f: &mut Frame, app: &ChatApp, area: ratatui::layout::Rect) {
    let prefix = if app.is_processing { "⏳ " } else { "> " };

    let input_text = format!("{}{}", prefix, app.input);
    let paragraph = Paragraph::new(input_text);

    // Always show cursor at end of input
    let cursor_x = (prefix.len() + app.input.len()) as u16;
    if cursor_x < area.width {
        f.set_cursor_position((area.x + cursor_x, area.y));
    }

    f.render_widget(paragraph, area);
}
