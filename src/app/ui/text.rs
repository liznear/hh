#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
    Rgb(u8, u8, u8),
    Indexed(u8),
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct Modifier: u16 {
        const BOLD              = 0b0001;
        const DIM               = 0b0010;
        const ITALIC            = 0b0100;
        const UNDERLINED        = 0b1000;
        const SLOW_BLINK        = 0b0001_0000;
        const RAPID_BLINK       = 0b0010_0000;
        const REVERSED          = 0b0100_0000;
        const HIDDEN            = 0b1000_0000;
        const CROSSED_OUT       = 0b0001_0000_0000;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub add_modifier: Modifier,
    pub sub_modifier: Modifier,
}

impl Style {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn fg(mut self, color: Color) -> Self {
        self.fg = Some(color);
        self
    }
    pub fn bg(mut self, color: Color) -> Self {
        self.bg = Some(color);
        self
    }
    pub fn add_modifier(mut self, modifier: Modifier) -> Self {
        self.add_modifier |= modifier;
        self
    }
    pub fn remove_modifier(mut self, modifier: Modifier) -> Self {
        self.sub_modifier |= modifier;
        self
    }

    pub fn bold(self) -> Self {
        self.add_modifier(Modifier::BOLD)
    }
    pub fn italic(self) -> Self {
        self.add_modifier(Modifier::ITALIC)
    }
    pub fn dim(self) -> Self {
        self.add_modifier(Modifier::DIM)
    }
    pub fn underlined(self) -> Self {
        self.add_modifier(Modifier::UNDERLINED)
    }

    pub fn patch(mut self, other: Style) -> Self {
        if let Some(fg) = other.fg {
            self.fg = Some(fg);
        }
        if let Some(bg) = other.bg {
            self.bg = Some(bg);
        }
        self.add_modifier |= other.add_modifier;
        self.sub_modifier |= other.sub_modifier;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub content: String,
    pub style: Style,
}

impl Span {
    pub fn new(content: impl Into<String>, style: Style) -> Self {
        Self {
            content: content.into().to_string(),
            style,
        }
    }
    pub fn raw(content: impl Into<String>) -> Self {
        Self {
            content: content.into().to_string(),
            style: Style::default(),
        }
    }
    pub fn styled(content: impl Into<String>, style: Style) -> Self {
        Self::new(content, style)
    }
    pub fn width(&self) -> usize {
        unicode_width::UnicodeWidthStr::width(self.content.as_str())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub spans: Vec<Span>,
    pub style: Style,
}

impl Line {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn raw(content: impl Into<String>) -> Self {
        Self {
            spans: vec![Span::raw(content)],
            style: Style::default(),
        }
    }
    pub fn styled(content: impl Into<String>, style: Style) -> Self {
        Self {
            spans: vec![Span::styled(content, style)],
            style: Style::default(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.spans.iter().all(|s| s.content.is_empty())
    }
    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| s.width()).sum()
    }
}

impl From<String> for Line {
    fn from(s: String) -> Self {
        Line::raw(s)
    }
}
impl From<&str> for Line {
    fn from(s: &str) -> Self {
        Line::raw(s)
    }
}
impl From<Vec<Span>> for Line {
    fn from(spans: Vec<Span>) -> Self {
        Self {
            spans,
            style: Style::default(),
        }
    }
}
impl From<Span> for Line {
    fn from(span: Span) -> Self {
        Self {
            spans: vec![span],
            style: Style::default(),
        }
    }
}

pub type Text = Vec<Line>;

pub trait Stylize<T>: Sized {
    fn fg(self, color: Color) -> T;
    fn bg(self, color: Color) -> T;
    fn bold(self) -> T;
    fn italic(self) -> T;
    fn dim(self) -> T;
    fn underlined(self) -> T;
}

impl Stylize<Span> for Span {
    fn fg(mut self, color: Color) -> Self {
        self.style = self.style.fg(color);
        self
    }
    fn bg(mut self, color: Color) -> Self {
        self.style = self.style.bg(color);
        self
    }
    fn bold(mut self) -> Self {
        self.style = self.style.bold();
        self
    }
    fn italic(mut self) -> Self {
        self.style = self.style.italic();
        self
    }
    fn dim(mut self) -> Self {
        self.style = self.style.dim();
        self
    }
    fn underlined(mut self) -> Self {
        self.style = self.style.underlined();
        self
    }
}
