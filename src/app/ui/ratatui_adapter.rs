use ratatui::prelude::Stylize;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::{UiColor, UiLine, UiSpan, UiStyle};

impl From<UiColor> for Color {
    fn from(c: UiColor) -> Self {
        Color::Rgb(c.r, c.g, c.b)
    }
}

impl From<UiStyle> for Style {
    fn from(s: UiStyle) -> Self {
        let mut style = Style::default();
        if let Some(fg) = s.fg {
            style = style.fg(fg.into());
        }
        if let Some(bg) = s.bg {
            style = style.bg(bg.into());
        }
        if s.bold {
            style = style.bold();
        }
        if s.italic {
            style = style.italic();
        }
        if s.dim {
            style = style.dim();
        }
        if s.underline {
            style = style.underlined();
        }
        style
    }
}

pub fn ui_line_to_ratatui(line: &UiLine) -> Line<'static> {
    Line::from(
        line.spans
            .iter()
            .map(|s| Span::styled(s.content.clone(), Style::from(s.style)))
            .collect::<Vec<_>>(),
    )
}

pub fn ui_lines_to_ratatui(lines: &[UiLine]) -> Vec<Line<'static>> {
    lines.iter().map(ui_line_to_ratatui).collect()
}

pub fn ui_span_to_ratatui(span: &UiSpan) -> Span<'static> {
    Span::styled(span.content.clone(), Style::from(span.style))
}
