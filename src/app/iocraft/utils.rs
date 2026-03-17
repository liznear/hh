use crate::app::ui::text::{Color as UiColor, Line as UiLine, Modifier};
use iocraft::prelude::*;

pub fn ui_color_to_iocraft(color: UiColor) -> Option<Color> {
    match color {
        UiColor::Rgb(r, g, b) => Some(Color::Rgb { r, g, b }),
        UiColor::Black => Some(Color::Rgb { r: 0, g: 0, b: 0 }),
        UiColor::Red => Some(Color::Rgb { r: 255, g: 0, b: 0 }),
        UiColor::Green => Some(Color::Rgb { r: 0, g: 255, b: 0 }),
        UiColor::Yellow => Some(Color::Rgb {
            r: 255,
            g: 255,
            b: 0,
        }),
        UiColor::Blue => Some(Color::Rgb { r: 0, g: 0, b: 255 }),
        UiColor::Magenta => Some(Color::Rgb {
            r: 255,
            g: 0,
            b: 255,
        }),
        UiColor::Cyan => Some(Color::Rgb {
            r: 0,
            g: 255,
            b: 255,
        }),
        UiColor::White => Some(Color::Rgb {
            r: 255,
            g: 255,
            b: 255,
        }),
        UiColor::Gray => Some(Color::Rgb {
            r: 128,
            g: 128,
            b: 128,
        }),
        UiColor::DarkGray => Some(Color::Rgb {
            r: 64,
            g: 64,
            b: 64,
        }),
        UiColor::LightRed => Some(Color::Rgb {
            r: 255,
            g: 128,
            b: 128,
        }),
        UiColor::LightGreen => Some(Color::Rgb {
            r: 128,
            g: 255,
            b: 128,
        }),
        UiColor::LightYellow => Some(Color::Rgb {
            r: 255,
            g: 255,
            b: 128,
        }),
        UiColor::LightBlue => Some(Color::Rgb {
            r: 128,
            g: 128,
            b: 255,
        }),
        UiColor::LightMagenta => Some(Color::Rgb {
            r: 255,
            g: 128,
            b: 255,
        }),
        UiColor::LightCyan => Some(Color::Rgb {
            r: 128,
            g: 255,
            b: 255,
        }),
        UiColor::Reset | UiColor::Indexed(_) => None,
    }
}

#[allow(clippy::collapsible_if)]
pub fn ui_line_to_mixed_text(line: &UiLine) -> impl Into<AnyElement<'static>> {
    let mut contents = Vec::new();
    for span in &line.spans {
        let mut t = MixedTextContent::new(span.content.to_string());
        if let Some(fg) = span.style.fg {
            if let Some(color) = ui_color_to_iocraft(fg) {
                t = t.color(color);
            }
        }

        if span.style.add_modifier.contains(Modifier::BOLD) {
            t = t.weight(Weight::Bold);
        }
        if span.style.add_modifier.contains(Modifier::ITALIC) {
            t = t.italic();
        }
        if span.style.add_modifier.contains(Modifier::UNDERLINED) {
            t = t.decoration(TextDecoration::Underline);
        }
        if span.style.add_modifier.contains(Modifier::DIM) {
            t = t.weight(Weight::Light);
        }
        contents.push(t);
    }
    element!(MixedText(contents: contents))
}
