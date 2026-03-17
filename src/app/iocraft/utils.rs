use crate::app::ui::{UiColor, UiLine};
use iocraft::prelude::*;

pub fn ui_color_to_iocraft(color: UiColor) -> Color {
    Color::Rgb {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

pub fn ui_line_to_mixed_text(line: &UiLine) -> impl Into<AnyElement<'static>> {
    let mut contents = Vec::new();
    for span in &line.spans {
        let mut t = MixedTextContent::new(span.content.clone());
        if let Some(fg) = span.style.fg {
            t = t.color(ui_color_to_iocraft(fg));
        }
        if span.style.bold {
            t = t.weight(Weight::Bold);
        }
        if span.style.italic {
            t = t.italic();
        }
        if span.style.underline {
            t = t.decoration(TextDecoration::Underline);
        }
        if span.style.dim {
            t = t.weight(Weight::Light);
        }
        contents.push(t);
    }
    element!(MixedText(contents: contents))
}
