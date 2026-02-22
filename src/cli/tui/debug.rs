use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use ratatui::{
    backend::TestBackend,
    layout::Rect,
    prelude::Stylize,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};

use super::app::{ChatApp, ChatMessage};

const DEBUG_WIDTH: u16 = 120;
const DEBUG_HEIGHT: u16 = 40;

pub struct DebugRenderer {
    terminal: Terminal<TestBackend>,
    output_dir: PathBuf,
    frame_count: usize,
}

impl DebugRenderer {
    pub fn new(output_dir: PathBuf) -> anyhow::Result<Self> {
        fs::create_dir_all(&output_dir)?;
        let backend = TestBackend::new(DEBUG_WIDTH, DEBUG_HEIGHT);
        let terminal = Terminal::new(backend)?;
        Ok(Self {
            terminal,
            output_dir,
            frame_count: 0,
        })
    }

    pub fn render(&mut self, app: &ChatApp) -> anyhow::Result<()> {
        self.terminal.draw(|f| {
            render_debug_app(f, app);
        })?;
        self.dump_screen()?;
        self.frame_count += 1;
        Ok(())
    }

    fn dump_screen(&self) -> anyhow::Result<()> {
        let filename = format!("screen-{:03}.txt", self.frame_count);
        let path = self.output_dir.join(filename);

        let mut file = File::create(&path)?;
        let buffer = self.terminal.backend().buffer();

        // Convert buffer to text representation
        for y in 0..DEBUG_HEIGHT {
            let mut line = String::new();
            for x in 0..DEBUG_WIDTH {
                let cell = &buffer[(x, y)];
                line.push_str(cell.symbol());
            }
            // Trim trailing whitespace but preserve content
            let trimmed = line.trim_end();
            writeln!(file, "{}", trimmed)?;
        }

        Ok(())
    }

    pub fn output_dir(&self) -> &std::path::Path {
        &self.output_dir
    }

    pub fn frame_count(&self) -> usize {
        self.frame_count
    }
}

fn render_debug_app(f: &mut ratatui::Frame, app: &ChatApp) {
    let area = Rect::new(0, 0, DEBUG_WIDTH, DEBUG_HEIGHT);

    // Split into messages, status, and input areas
    let chunks = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Min(3),    // Messages area
            ratatui::layout::Constraint::Length(1), // Status bar
            ratatui::layout::Constraint::Length(2), // Input area
        ])
        .split(area);

    render_debug_messages(f, app, chunks[0]);
    render_debug_status(f, app, chunks[1]);
    render_debug_input(f, app, chunks[2]);
}

fn render_debug_messages(f: &mut ratatui::Frame, app: &ChatApp, area: Rect) {
    let lines = build_debug_message_lines(app);
    let total_lines = lines.len();

    let visible_height = area.height.saturating_sub(2) as usize;
    let scroll_offset = if app.scroll_offset + visible_height > total_lines {
        total_lines.saturating_sub(visible_height)
    } else {
        app.scroll_offset
    };

    let text = ratatui::text::Text::from(lines);
    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::TOP).title("Messages"))
        .scroll((scroll_offset as u16, 0));

    f.render_widget(paragraph, area);
}

fn build_debug_message_lines(app: &ChatApp) -> Vec<Line<'static>> {
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
            ChatMessage::ToolEnd {
                name,
                is_error,
                output,
            } => {
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

fn render_debug_status(f: &mut ratatui::Frame, app: &ChatApp, area: Rect) {
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

fn render_debug_input(f: &mut ratatui::Frame, app: &ChatApp, area: Rect) {
    let prefix = if app.is_processing {
        "⏳ waiting for response..."
    } else {
        "> _"
    };

    let input_text = if app.is_processing {
        prefix.to_string()
    } else {
        format!("> {}", app.input)
    };

    let paragraph = Paragraph::new(input_text);
    f.render_widget(paragraph, area);
}
