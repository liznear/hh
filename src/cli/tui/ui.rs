use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::Stylize,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::app::{ChatApp, ChatMessage};

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
    let lines = build_message_lines(app);
    let total_lines = lines.len();

    let visible_height = area.height.saturating_sub(2) as usize; // Account for borders
    let scroll_offset = if app.scroll_offset + visible_height > total_lines {
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

fn build_message_lines(app: &ChatApp) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for msg in &app.messages {
        match msg {
            ChatMessage::User(text) => {
                lines.push(Line::from(vec![
                    Span::styled("you: ", Style::default().fg(Color::Cyan).bold()),
                    Span::raw(text.clone()),
                ]));
            }
            ChatMessage::Assistant(text) => {
                lines.push(Line::from(vec![
                    Span::styled(
                        "assistant: ",
                        Style::default().fg(Color::Green).bold(),
                    ),
                    Span::raw(text.clone()),
                ]));
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
                lines.push(Line::from(vec![
                    Span::styled("tool:", Style::default().fg(Color::Magenta)),
                    Span::styled(name.clone(), Style::default().fg(Color::Magenta).bold()),
                    Span::raw("> start "),
                    Span::raw(args.clone()),
                ]));
            }
            ChatMessage::ToolEnd { name, is_error, output } => {
                let status_color = if *is_error {
                    Color::Red
                } else {
                    Color::Green
                };
                let status = if *is_error { "error" } else { "ok" };
                lines.push(Line::from(vec![
                    Span::styled("tool:", Style::default().fg(Color::Magenta)),
                    Span::styled(name.clone(), Style::default().fg(Color::Magenta).bold()),
                    Span::raw("> "),
                    Span::styled(status, Style::default().fg(status_color)),
                    Span::raw(" "),
                    Span::raw(output.clone()),
                ]));
            }
        }
    }

    lines
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
