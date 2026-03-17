use crate::ui_compat::style::{Color, Style};
use crate::ui_compat::text::{Line, Span};

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

pub fn ratatui_color_to_ui(c: Color) -> Option<UiColor> {
    match c {
        Color::Rgb(r, g, b) => Some(UiColor::rgb(r, g, b)),
        _ => None,
    }
}

pub fn ratatui_style_to_ui(s: Style) -> UiStyle {
    let mut ui = UiStyle::default();
    if let Some(fg) = s.fg.and_then(ratatui_color_to_ui) {
        ui = ui.fg(fg);
    }
    if let Some(bg) = s.bg.and_then(ratatui_color_to_ui) {
        ui = ui.bg(bg);
    }
    if s.add_modifier.contains(crate::ui_compat::style::Modifier::BOLD) {
        ui = ui.bold();
    }
    if s.add_modifier.contains(crate::ui_compat::style::Modifier::ITALIC) {
        ui = ui.italic();
    }
    if s.add_modifier.contains(crate::ui_compat::style::Modifier::DIM) {
        ui = ui.dim();
    }
    if s.add_modifier.contains(crate::ui_compat::style::Modifier::UNDERLINED) {
        ui = ui.underline();
    }
    ui
}

pub fn ratatui_span_to_ui(span: &Span<'_>) -> UiSpan {
    UiSpan::new(span.content.to_string(), ratatui_style_to_ui(span.style))
}

pub fn ratatui_line_to_ui(line: &Line<'_>) -> UiLine {
    UiLine::from(line.spans.iter().map(ratatui_span_to_ui).collect::<Vec<_>>())
}
