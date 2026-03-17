use iocraft::prelude::*;

pub fn to_iocraft_color(color: ratatui::style::Color) -> Color {
    match color {
        ratatui::style::Color::Rgb(r, g, b) => Color::Rgb { r, g, b },
        _ => Color::Reset,
    }
}

pub const fn page_bg() -> Color {
    Color::Rgb {
        r: 246,
        g: 247,
        b: 251,
    }
}

pub const fn sidebar_bg() -> Color {
    Color::Rgb {
        r: 234,
        g: 238,
        b: 246,
    }
}

pub const fn input_panel_bg() -> Color {
    Color::Rgb {
        r: 229,
        g: 233,
        b: 241,
    }
}

pub const fn command_palette_bg() -> Color {
    Color::Rgb {
        r: 214,
        g: 220,
        b: 232,
    }
}

pub const fn text_primary() -> Color {
    Color::Rgb {
        r: 37,
        g: 45,
        b: 58,
    }
}

pub const fn text_secondary() -> Color {
    Color::Rgb {
        r: 98,
        g: 108,
        b: 124,
    }
}

pub const fn text_muted() -> Color {
    Color::Rgb {
        r: 125,
        g: 133,
        b: 147,
    }
}

pub const fn accent() -> Color {
    Color::Rgb {
        r: 55,
        g: 114,
        b: 255,
    }
}

pub const fn input_accent() -> Color {
    Color::Rgb {
        r: 19,
        g: 164,
        b: 151,
    }
}

pub const fn selection_bg() -> Color {
    Color::Rgb {
        r: 55,
        g: 114,
        b: 255,
    }
}

pub const fn notice_bg() -> Color {
    Color::Rgb {
        r: 224,
        g: 227,
        b: 233,
    }
}

pub const fn progress_head() -> Color {
    Color::Rgb {
        r: 124,
        g: 72,
        b: 227,
    }
}

pub const fn thinking_label() -> Color {
    Color::Rgb {
        r: 227,
        g: 152,
        b: 67,
    }
}

pub const fn queued_tag_bg() -> Color {
    Color::Rgb {
        r: 201,
        g: 227,
        b: 255,
    }
}

pub const fn todo_active_fg() -> Color {
    Color::Rgb {
        r: 227,
        g: 152,
        b: 67,
    }
}

pub const fn question_border() -> Color {
    Color::Rgb {
        r: 220,
        g: 96,
        b: 180,
    }
}

pub const fn context_usage_yellow() -> Color {
    Color::Rgb {
        r: 214,
        g: 168,
        b: 46,
    }
}

pub const fn context_usage_orange() -> Color {
    Color::Rgb {
        r: 227,
        g: 136,
        b: 46,
    }
}

pub const fn context_usage_red() -> Color {
    Color::Rgb {
        r: 196,
        g: 64,
        b: 64,
    }
}

pub const fn diff_add_fg() -> Color {
    Color::Rgb {
        r: 25,
        g: 110,
        b: 61,
    }
}

pub const fn diff_add_bg() -> Color {
    Color::Rgb {
        r: 226,
        g: 244,
        b: 235,
    }
}

pub const fn diff_remove_fg() -> Color {
    Color::Rgb {
        r: 152,
        g: 45,
        b: 45,
    }
}

pub const fn diff_remove_bg() -> Color {
    Color::Rgb {
        r: 252,
        g: 235,
        b: 235,
    }
}

pub const fn diff_meta_fg() -> Color {
    Color::Rgb {
        r: 106,
        g: 114,
        b: 128,
    }
}
